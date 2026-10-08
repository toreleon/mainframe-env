//! Private row boundary for the single service authority, not another runtime.
//!
//! The service owns admission, SAF, catalog generations, recovery lease epochs,
//! audit/replay composition and migration. It must publish the returned mutations
//! together in its existing store transaction, and adopt `next` only on success.
//! These namespaces never read or migrate legacy MQ rows. Missing state is an
//! error; initial publication is an explicit service decision, not an open fallback.

use super::*;
use crate::service::{OBJECT_ROW_SCHEMA, ObjectRow, encode_object_row};
use mainframe_env_store_api::{
    ProviderStateMutation, ProviderStateRecord, ProviderStateStore, ProviderStateWrite, StoreError,
};
use serde::de::DeserializeOwned;
use sha2::{Digest, Sha256};

pub(crate) const PREFIX: &str = "mq-delivery-live-v1-";
const META: &str = "mq-delivery-live-v1-meta";
const QUEUE: &str = "mq-delivery-live-v1-queue";
const PENDING: &str = "mq-delivery-live-v1-pending";
const FINAL: &str = "mq-delivery-live-v1-final";
const CURSOR: &str = "mq-delivery-live-v1-cursor";
const META_KEY: &str = "state";
const SCHEMA: &str = "mainframe-env.mq-delivery-rows@1";
const SCHEMA_TWO: &str = "mainframe-env.mq-delivery-rows@2";
/// Exact existing modeled row families; not a prefix mutation permission.
pub(crate) const TERMINAL_NAMESPACES: [&str; 5] = [META, QUEUE, PENDING, FINAL, CURSOR];
mod upgrade;
type Key = (String, String);
type Records = BTreeMap<Key, ProviderStateRecord>;

/// Physical bounds in addition to the existing kernel/message/checkpoint guards.
#[derive(Clone, Copy, Debug)]
pub(crate) struct DeliveryRowLimits {
    pub(crate) rows: usize,
    pub(crate) row_bytes: usize,
    pub(crate) total_bytes: usize,
    pub(crate) mutations: usize,
}

impl Default for DeliveryRowLimits {
    fn default() -> Self {
        // One metadata row and the frozen queue/UOW/decision/cursor ceilings.
        Self {
            rows: 28_193,
            row_bytes: 64 << 20,
            total_bytes: 64 << 20,
            mutations: 56_385,
        }
    }
}

impl DeliveryRowLimits {
    fn validate(self) -> Result<(), DeliveryRowError> {
        let ceiling = Self::default();
        for (value, max) in [
            (self.rows, ceiling.rows),
            (self.row_bytes, ceiling.row_bytes),
            (self.total_bytes, ceiling.total_bytes),
            (self.mutations, ceiling.mutations),
        ] {
            if value == 0 || value > max {
                return Err(DeliveryRowError::Bounds);
            }
        }
        Ok(())
    }
}

/// Trusted service generation and recovery fence, bound to all catalog bytes.
/// One queue manager uses this namespace family in a physical store. Catalog
/// generation changes require manager-owned migration, never an ordinary delta.
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct DeliveryRowIdentity {
    catalog_sha256: [u8; 32],
    generation: u64,
    fence: u64,
}

impl DeliveryRowIdentity {
    /// Private checked persisted-fence step; not a coordinator or recovery permit.
    pub(crate) fn next_fence(&self) -> Result<Self, DeliveryRowError> {
        let fence = self
            .fence
            .checked_add(1)
            .filter(|f| *f <= i64::MAX as u64)
            .ok_or(DeliveryRowError::Identity)?;
        if self.generation == 0 || self.generation > i64::MAX as u64 || self.fence == 0 {
            return Err(DeliveryRowError::Identity);
        }
        Ok(Self {
            fence,
            ..self.clone()
        })
    }

    pub(crate) fn generation_and_fence(&self) -> (u64, u64) {
        (self.generation, self.fence)
    }
    pub(crate) fn new(
        catalog: &MqObjectCatalog,
        generation: u64,
        fence: u64,
    ) -> Result<Self, DeliveryRowError> {
        if generation == 0 || fence == 0 || generation > i64::MAX as u64 || fence > i64::MAX as u64
        {
            return Err(DeliveryRowError::Identity);
        }
        let bytes = catalog
            .encode()
            .map_err(|e| DeliveryRowError::Kernel(e.into()))?;
        Ok(Self {
            catalog_sha256: Sha256::digest(bytes).into(),
            generation,
            fence,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum DeliveryRowError {
    Bounds,
    Missing,
    Corrupt,
    Identity,
    Kernel(MqDeliveryError),
    Store(StoreError),
}

impl From<MqDeliveryError> for DeliveryRowError {
    fn from(error: MqDeliveryError) -> Self {
        Self::Kernel(error)
    }
}
impl From<StoreError> for DeliveryRowError {
    fn from(error: StoreError) -> Self {
        Self::Store(error)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Metadata {
    schema_version: String,
    identity: DeliveryRowIdentity,
    manager: MqObjectName,
    default_persistent: bool,
    tick: u64,
    next_id: u64,
    next_cursor: u64,
    counts: [usize; 4],
    rows_sha256: [u8; 32],
}

/// A validated loaded generation; records cannot be supplied or edited by callers.
#[derive(Clone, Debug)]
pub(crate) struct DeliveryRows {
    records: Records,
    metadata: Metadata,
    limits: DeliveryRowLimits,
}

pub(crate) struct DeliveryRowDelta {
    mutations: Vec<ProviderStateMutation>,
    next: DeliveryRows,
}

impl DeliveryRowDelta {
    #[cfg(test)]
    pub(crate) fn mutations(&self) -> &[ProviderStateMutation] {
        &self.mutations
    }

    /// The service must not adopt the returned generation before the full
    /// transaction (including its audit/replay writes) has committed.
    pub(crate) fn into_parts(self) -> (Vec<ProviderStateMutation>, DeliveryRows) {
        (self.mutations, self.next)
    }
}

impl DeliveryRows {
    /// Immutable physical projection for same-authority composed validation.
    /// This neither scans a store nor exposes mutable row or message state.
    pub(crate) fn captured_records(&self) -> impl Iterator<Item = &ProviderStateRecord> {
        self.records.values()
    }

    /// Metadata-only checked fence step. Preserve every member payload/version,
    /// including pending/final/cursor state, without reprojecting live policy.
    pub(crate) fn next_fence_delta(&self) -> Result<DeliveryRowDelta, DeliveryRowError> {
        self.limits.validate()?;
        let mut metadata = self.metadata.clone();
        metadata.identity = metadata.identity.next_fence()?;
        let old = self
            .records
            .get(&key(META, META_KEY))
            .ok_or(DeliveryRowError::Corrupt)?;
        let mut records: Records = self
            .records
            .iter()
            .filter(|(k, _)| k.0 != META)
            .map(|(k, r)| (k.clone(), r.clone()))
            .collect();
        let mut bytes = records
            .values()
            .try_fold(0usize, |n, r| n.checked_add(r.payload.len()))
            .ok_or(DeliveryRowError::Bounds)?;
        let mut mutations = Vec::new();
        put(
            META,
            META_KEY,
            encode_object_row(META_KEY, &metadata).map_err(|_| DeliveryRowError::Corrupt)?,
            Some(old),
            &mut records,
            &mut mutations,
            self.limits,
            &mut bytes,
        )?;
        Ok(DeliveryRowDelta {
            mutations,
            next: Self {
                records,
                metadata,
                limits: self.limits,
            },
        })
    }
    /// Explicit creation only. The service must authorize creating/migrating this
    /// authority and reconcile legacy state before calling this. Never falls back
    /// from failed restore. Orphan rich rows prohibit initialization.
    pub(crate) fn initialize(
        store: &dyn ProviderStateStore,
        kernel: &MqDeliveryKernel,
        catalog: &MqObjectCatalog,
        identity: DeliveryRowIdentity,
        limits: DeliveryRowLimits,
    ) -> Result<DeliveryRowDelta, DeliveryRowError> {
        limits.validate()?;
        if !store.list_provider_state_prefix(PREFIX, 1)?.is_empty() {
            return Err(DeliveryRowError::Corrupt);
        }
        prepare(None, kernel, catalog, identity, limits)
    }

    /// Pure restore of an already captured physical snapshot. No store reads.
    pub(crate) fn restore(
        records: Vec<ProviderStateRecord>,
        catalog: &MqObjectCatalog,
        expected: DeliveryRowIdentity,
        limits: DeliveryRowLimits,
        kernel_limits: MqDeliveryLimits,
        message_limits: MqMessageLimits,
        default_persistence: MqPersistence,
    ) -> Result<(Self, MqDeliveryKernel), DeliveryRowError> {
        limits.validate()?;
        let kernel =
            MqDeliveryKernel::new(catalog, kernel_limits, message_limits, default_persistence)?;
        if records.is_empty() {
            return Err(DeliveryRowError::Missing);
        }
        if records.len() > limits.rows {
            return Err(DeliveryRowError::Bounds);
        }
        let mut map = Records::new();
        let mut total = 0usize;
        for record in records {
            total = total
                .checked_add(record.payload.len())
                .ok_or(DeliveryRowError::Bounds)?;
            if record.payload.len() > limits.row_bytes || total > limits.total_bytes {
                return Err(DeliveryRowError::Bounds);
            }
            preflight::check(&record.payload, kernel_limits, message_limits)?;
            if ![META, QUEUE, PENDING, FINAL, CURSOR].contains(&record.namespace.as_str())
                || record.validate_write(limits.row_bytes).is_err()
                || map
                    .insert((record.namespace.clone(), record.key.clone()), record)
                    .is_some()
            {
                return Err(DeliveryRowError::Corrupt);
            }
        }
        let meta_record = map
            .get(&key(META, META_KEY))
            .ok_or(DeliveryRowError::Corrupt)?;
        let metadata: Metadata = decode(meta_record)?;
        if metadata.schema_version != SCHEMA && metadata.schema_version != SCHEMA_TWO {
            return Err(DeliveryRowError::Corrupt);
        }
        if metadata.identity != expected
            || expected != DeliveryRowIdentity::new(catalog, expected.generation, expected.fence)?
        {
            return Err(DeliveryRowError::Identity);
        }
        if metadata.rows_sha256 != digest_rows(&map, metadata.schema_version == SCHEMA_TWO)
            || map.values().any(|r| r.version > meta_record.version)
        {
            return Err(DeliveryRowError::Corrupt);
        }
        let actual = counts(&map);
        if actual != metadata.counts
            || actual[0] > kernel_limits.queues
            || actual[1] > kernel_limits.pending_operations
            || actual[2] > kernel_limits.finalized_units
            || actual[3] > kernel_limits.cursors
        {
            return Err(DeliveryRowError::Bounds);
        }
        let mut snapshot = Checkpoint {
            schema_version: if metadata.schema_version == SCHEMA_TWO {
                projection::LIVE_SCHEMA
            } else {
                MqDeliveryKernel::LIVE_CHECKPOINT_SCHEMA
            }
            .into(),
            manager: metadata.manager.clone(),
            default_persistent: metadata.default_persistent,
            tick: metadata.tick,
            next_id: metadata.next_id,
            next_cursor: metadata.next_cursor,
            queues: Vec::new(),
            pending: Vec::new(),
            finalized: Vec::new(),
            cursors: Vec::new(),
        };
        for record in map.values() {
            let expected_key = match record.namespace.as_str() {
                META => META_KEY.into(),
                QUEUE => {
                    let row: LiveQueue = if metadata.schema_version == SCHEMA_TWO {
                        decode::<projection::QueueTwo>(record)?.into_live()?
                    } else {
                        decode(record)?
                    };
                    let id = row.name.as_str().to_string();
                    snapshot.queues.push(row);
                    id
                }
                PENDING => {
                    let row: LiveUnit = if metadata.schema_version == SCHEMA_TWO {
                        decode::<projection::UnitTwo>(record)?.into_live()?
                    } else {
                        decode(record)?
                    };
                    let id = number_key(row.unit);
                    snapshot.pending.push(row);
                    id
                }
                FINAL => {
                    let row: LiveFinalized = decode(record)?;
                    let id = number_key(row.unit);
                    snapshot.finalized.push(row);
                    id
                }
                CURSOR => {
                    let row: LiveCursor = decode(record)?;
                    let id = number_key(row.token);
                    snapshot.cursors.push(row);
                    id
                }
                _ => return Err(DeliveryRowError::Corrupt),
            };
            if record.key != expected_key {
                return Err(DeliveryRowError::Corrupt);
            }
        }
        // Existing projection/decoder owns all message and cross-row semantics.
        encode_current(&snapshot, kernel_limits.snapshot_bytes)?;
        let restored = MqDeliveryKernel::restore_projection(snapshot, kernel)?;
        Ok((
            Self {
                records: map,
                metadata,
                limits,
            },
            restored,
        ))
    }

    /// Always includes the metadata CAS, even if object payloads did not change.
    /// A higher trusted recovery fence can be published under that same CAS;
    /// generation/catalog changes are rejected and need explicit migration.
    pub(crate) fn delta(
        &self,
        kernel: &MqDeliveryKernel,
        catalog: &MqObjectCatalog,
        identity: DeliveryRowIdentity,
    ) -> Result<DeliveryRowDelta, DeliveryRowError> {
        if identity.catalog_sha256 != self.metadata.identity.catalog_sha256
            || identity.generation != self.metadata.identity.generation
            || identity.fence < self.metadata.identity.fence
            || kernel.manager != self.metadata.manager
            || (kernel.default_persistence == MqPersistence::Persistent)
                != self.metadata.default_persistent
            || kernel.tick < self.metadata.tick
            || kernel.next_id < self.metadata.next_id
            || kernel.next_cursor < self.metadata.next_cursor
        {
            return Err(DeliveryRowError::Identity);
        }
        if kernel.schema_two != (self.metadata.schema_version == SCHEMA_TWO) {
            return Err(DeliveryRowError::Identity);
        }
        for record in self.records.values().filter(|r| r.namespace == QUEUE) {
            let row = if kernel.schema_two {
                decode::<projection::QueueTwo>(record)?.into_live()?
            } else {
                decode::<LiveQueue>(record)?
            };
            if kernel
                .queues
                .get(&row.name)
                .is_none_or(|q| q.profile != row.profile)
            {
                return Err(DeliveryRowError::Identity);
            }
        }
        // Kernel lifetime retains decisions and empty pending units. A caller
        // cannot replace a loaded authority with a newly seeded kernel and
        // silently discard those replay references.
        for record in self.records.values() {
            if record.namespace == FINAL {
                let row: LiveFinalized = decode(record)?;
                if kernel.finalized.get(&row.unit) != Some(&row.committed) {
                    return Err(DeliveryRowError::Identity);
                }
            } else if record.namespace == PENDING {
                let unit = record
                    .key
                    .parse::<u64>()
                    .map_err(|_| DeliveryRowError::Corrupt)?;
                if !kernel.pending.contains_key(&unit) && !kernel.finalized.contains_key(&unit) {
                    return Err(DeliveryRowError::Identity);
                }
            }
        }
        prepare(Some(self), kernel, catalog, identity, self.limits)
    }
}

fn prepare(
    current: Option<&DeliveryRows>,
    kernel: &MqDeliveryKernel,
    catalog: &MqObjectCatalog,
    identity: DeliveryRowIdentity,
    limits: DeliveryRowLimits,
) -> Result<DeliveryRowDelta, DeliveryRowError> {
    if identity != DeliveryRowIdentity::new(catalog, identity.generation, identity.fence)? {
        return Err(DeliveryRowError::Identity);
    }
    let snapshot = kernel.live_projection()?;
    encode_current(&snapshot, kernel.limits.snapshot_bytes)?;
    let validated = MqDeliveryKernel::restore_projection(
        snapshot,
        MqDeliveryKernel::new(
            catalog,
            kernel.limits,
            kernel.message_limits,
            kernel.default_persistence,
        )?,
    )?;
    let snapshot = validated.live_projection()?;
    let object_count = snapshot
        .queues
        .len()
        .checked_add(snapshot.pending.len())
        .and_then(|n| n.checked_add(snapshot.finalized.len()))
        .and_then(|n| n.checked_add(snapshot.cursors.len()))
        .and_then(|n| n.checked_add(1))
        .ok_or(DeliveryRowError::Bounds)?;
    if object_count > limits.rows {
        return Err(DeliveryRowError::Bounds);
    }
    let mut records = Records::new();
    let mut mutations = Vec::new();
    let mut records_bytes = 0usize;
    for row in &snapshot.queues {
        project_encoded(
            QUEUE,
            row.name.as_str(),
            &if kernel.schema_two {
                encode_object_row(row.name.as_str(), &projection::QueueTwo::from_live(row)?)
                    .map_err(|_| DeliveryRowError::Corrupt)?
            } else {
                encode_object_row(row.name.as_str(), row).map_err(|_| DeliveryRowError::Corrupt)?
            },
            current,
            &mut records,
            &mut mutations,
            limits,
            &mut records_bytes,
        )?;
    }
    for row in &snapshot.pending {
        project_encoded(
            PENDING,
            &number_key(row.unit),
            &if kernel.schema_two {
                encode_object_row(&number_key(row.unit), &projection::UnitTwo::from_live(row)?)
                    .map_err(|_| DeliveryRowError::Corrupt)?
            } else {
                encode_object_row(&number_key(row.unit), row)
                    .map_err(|_| DeliveryRowError::Corrupt)?
            },
            current,
            &mut records,
            &mut mutations,
            limits,
            &mut records_bytes,
        )?;
    }
    for row in &snapshot.finalized {
        project(
            FINAL,
            &number_key(row.unit),
            row,
            current,
            &mut records,
            &mut mutations,
            limits,
            &mut records_bytes,
        )?;
    }
    for row in &snapshot.cursors {
        project(
            CURSOR,
            &number_key(row.token),
            row,
            current,
            &mut records,
            &mut mutations,
            limits,
            &mut records_bytes,
        )?;
    }
    if let Some(current) = current {
        for (id, old) in &current.records {
            if old.namespace != META && !records.contains_key(id) {
                if mutations.len() >= limits.mutations {
                    return Err(DeliveryRowError::Bounds);
                }
                mutations.push(ProviderStateMutation::Delete {
                    namespace: old.namespace.clone(),
                    key: old.key.clone(),
                    expected_version: old.version,
                });
            }
        }
    }
    let metadata = Metadata {
        schema_version: if kernel.schema_two {
            SCHEMA_TWO
        } else {
            SCHEMA
        }
        .into(),
        identity,
        manager: snapshot.manager,
        default_persistent: snapshot.default_persistent,
        tick: snapshot.tick,
        next_id: snapshot.next_id,
        next_cursor: snapshot.next_cursor,
        counts: counts(&records),
        rows_sha256: digest_rows(&records, kernel.schema_two),
    };
    // Metadata comes last so backends must also roll back earlier object changes
    // if a competing generation wins this fence.
    let old_meta = current.and_then(|c| c.records.get(&key(META, META_KEY)));
    let payload = encode_object_row(META_KEY, &metadata).map_err(|_| DeliveryRowError::Corrupt)?;
    put(
        META,
        META_KEY,
        payload,
        old_meta,
        &mut records,
        &mut mutations,
        limits,
        &mut records_bytes,
    )?;
    let bytes = records
        .values()
        .try_fold(0usize, |n, r| n.checked_add(r.payload.len()))
        .ok_or(DeliveryRowError::Bounds)?;
    let mutation_bytes = mutations
        .iter()
        .try_fold(0usize, |n, m| {
            n.checked_add(match m {
                ProviderStateMutation::Put(w) => w.record.payload.len(),
                _ => 0,
            })
        })
        .ok_or(DeliveryRowError::Bounds)?;
    if records.len() > limits.rows
        || mutations.len() > limits.mutations
        || bytes > limits.total_bytes
        || mutation_bytes > limits.total_bytes
    {
        return Err(DeliveryRowError::Bounds);
    }
    Ok(DeliveryRowDelta {
        mutations,
        next: DeliveryRows {
            records,
            metadata,
            limits,
        },
    })
}

fn project<T: Serialize>(
    namespace: &str,
    id: &str,
    value: &T,
    current: Option<&DeliveryRows>,
    records: &mut Records,
    mutations: &mut Vec<ProviderStateMutation>,
    limits: DeliveryRowLimits,
    records_bytes: &mut usize,
) -> Result<(), DeliveryRowError> {
    let payload = encode_object_row(id, value).map_err(|_| DeliveryRowError::Corrupt)?;
    project_encoded(
        namespace,
        id,
        &payload,
        current,
        records,
        mutations,
        limits,
        records_bytes,
    )
}

#[allow(clippy::too_many_arguments)]
fn project_encoded(
    namespace: &str,
    id: &str,
    payload: &[u8],
    current: Option<&DeliveryRows>,
    records: &mut Records,
    mutations: &mut Vec<ProviderStateMutation>,
    limits: DeliveryRowLimits,
    records_bytes: &mut usize,
) -> Result<(), DeliveryRowError> {
    let old = current.and_then(|c| c.records.get(&key(namespace, id)));
    if let Some(old) = old.filter(|r| r.payload == payload) {
        check_record_budget(records, records_bytes, old.payload.len(), limits)?;
        records.insert(key(namespace, id), old.clone());
        return Ok(());
    }
    put(
        namespace,
        id,
        payload.to_vec(),
        old,
        records,
        mutations,
        limits,
        records_bytes,
    )
}

fn put(
    namespace: &str,
    id: &str,
    payload: Vec<u8>,
    old: Option<&ProviderStateRecord>,
    records: &mut Records,
    mutations: &mut Vec<ProviderStateMutation>,
    limits: DeliveryRowLimits,
    records_bytes: &mut usize,
) -> Result<(), DeliveryRowError> {
    check_record_budget(records, records_bytes, payload.len(), limits)?;
    if mutations.len() >= limits.mutations {
        return Err(DeliveryRowError::Bounds);
    }
    let expected = old.map(|r| r.version);
    let version = expected
        .unwrap_or(0)
        .checked_add(1)
        .filter(|v| *v <= i64::MAX as u64)
        .ok_or(DeliveryRowError::Bounds)?;
    let record = ProviderStateRecord {
        namespace: namespace.into(),
        key: id.into(),
        version,
        payload,
    };
    records.insert(key(namespace, id), record.clone());
    mutations.push(ProviderStateMutation::Put(ProviderStateWrite {
        record,
        expected_version: expected,
    }));
    Ok(())
}

fn check_record_budget(
    records: &Records,
    records_bytes: &mut usize,
    added_bytes: usize,
    limits: DeliveryRowLimits,
) -> Result<(), DeliveryRowError> {
    let remaining = limits
        .total_bytes
        .checked_sub(*records_bytes)
        .ok_or(DeliveryRowError::Bounds)?;
    if added_bytes > limits.row_bytes || added_bytes > remaining || records.len() >= limits.rows {
        return Err(DeliveryRowError::Bounds);
    }
    *records_bytes += added_bytes;
    Ok(())
}

fn decode<T: DeserializeOwned>(record: &ProviderStateRecord) -> Result<T, DeliveryRowError> {
    let row: ObjectRow<T> =
        serde_json::from_slice(&record.payload).map_err(|_| DeliveryRowError::Corrupt)?;
    if row.schema_version != OBJECT_ROW_SCHEMA || row.object_key != record.key {
        return Err(DeliveryRowError::Corrupt);
    }
    Ok(row.value)
}

fn counts(records: &Records) -> [usize; 4] {
    let mut counts = [0; 4];
    for row in records.values() {
        if let Some(index) = [QUEUE, PENDING, FINAL, CURSOR]
            .iter()
            .position(|ns| *ns == row.namespace)
        {
            counts[index] += 1;
        }
    }
    counts
}

fn digest_rows(records: &Records, two: bool) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(if two {
        b"mainframe-env.mq-delivery-rows-content@2\0"
    } else {
        b"mainframe-env.mq-delivery-rows-content@1\0"
    });
    for row in records.values().filter(|r| r.namespace != META) {
        for bytes in [
            row.namespace.as_bytes(),
            row.key.as_bytes(),
            row.payload.as_slice(),
        ] {
            hash.update((bytes.len() as u64).to_be_bytes());
            hash.update(bytes);
        }
        hash.update(row.version.to_be_bytes());
    }
    hash.finalize().into()
}

fn key(namespace: &str, id: &str) -> Key {
    (namespace.into(), id.into())
}
fn number_key(value: u64) -> String {
    format!("{value:020}")
}

fn encode_current(snapshot: &Checkpoint, limit: usize) -> Result<Vec<u8>, MqDeliveryError> {
    if snapshot.schema_version == projection::LIVE_SCHEMA {
        projection::encode(snapshot, limit)
    } else {
        encode_projection(snapshot, limit)
    }
}

#[cfg(test)]
#[path = "rows/full_tests.rs"]
mod full_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
pub(crate) use tests::load_fixture;
