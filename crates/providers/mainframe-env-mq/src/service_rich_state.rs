//! Private stored-authority selection for later single-service composition.
//! Reads never publish, migrate, mint owners or select a public runtime.

use super::*;
use crate::delivery::checkpoint::rows::{
    DeliveryRowError, DeliveryRowIdentity, DeliveryRowLimits, DeliveryRows, PREFIX,
};
use crate::object_service::catalog_limits;
use crate::{MqDeliveryKernel, MqDeliveryLimits};
use mainframe_env_host_api::{MqMessageLimits, MqPersistence};
use mainframe_env_store_api::MAX_PROVIDER_STATE_SCAN;

#[path = "service_rich_state/strict.rs"]
mod strict;

#[path = "service_rich_state/publication.rs"]
pub(super) mod publication;
#[path = "service_rich_state/upgrade.rs"]
mod upgrade;

/// Physical snapshot budget, separately from each original authority's limits.
#[derive(Clone, Copy, Debug)]
pub(super) struct ReaderLimits {
    pub(super) records: usize,
    pub(super) row_bytes: usize,
    pub(super) total_bytes: usize,
    pub(super) legacy: MqLimits,
    pub(super) delivery: MqDeliveryLimits,
    pub(super) message: MqMessageLimits,
    pub(super) rows: DeliveryRowLimits,
    pub(super) default_persistence: MqPersistence,
}

impl Default for ReaderLimits {
    fn default() -> Self {
        let legacy = MqLimits::default();
        let rows = DeliveryRowLimits::default();
        Self {
            records: 2
                + legacy.max_queues
                + legacy.max_handles
                + legacy.max_pending_units
                + legacy.max_replays
                + rows.rows,
            row_bytes: legacy.max_state_bytes,
            // Import may retain a full legacy replay corpus beside rich rows.
            total_bytes: legacy.max_state_bytes + rows.total_bytes,
            legacy,
            delivery: MqDeliveryLimits::default(),
            message: MqMessageLimits::default(),
            rows,
            default_persistence: MqPersistence::Persistent,
        }
    }
}

impl ReaderLimits {
    fn validate(self) -> Result<(), ReadError> {
        let max = Self::default();
        let a = self.legacy;
        let b = max.legacy;
        for (value, ceiling) in [
            (self.records, max.records),
            (self.row_bytes, max.row_bytes),
            (self.total_bytes, max.total_bytes),
            (a.max_queues, b.max_queues),
            (a.max_messages_per_queue, b.max_messages_per_queue),
            (a.max_message_bytes, b.max_message_bytes),
            (a.max_handles, b.max_handles),
            (a.max_pending_units, b.max_pending_units),
            (a.max_replays, b.max_replays),
            (a.max_state_bytes, b.max_state_bytes),
        ] {
            if value == 0 || value > ceiling {
                return Err(ReadError::Bounds);
            }
        }
        if self
            .records
            .checked_add(1)
            .is_none_or(|n| n > MAX_PROVIDER_STATE_SCAN)
        {
            return Err(ReadError::Bounds);
        }
        // Validate these even on v1, before reading physical records.
        let a = self.delivery;
        let b = max.delivery;
        let r = self.rows;
        let s = max.rows;
        for (value, ceiling) in [
            (a.queues, b.queues),
            (a.depth_per_queue, b.depth_per_queue),
            (a.total_bytes, b.total_bytes),
            (a.pending_operations, b.pending_operations),
            (a.cursors, b.cursors),
            (a.finalized_units, b.finalized_units),
            (a.snapshot_bytes, b.snapshot_bytes),
            (r.rows, s.rows),
            (r.row_bytes, s.row_bytes),
            (r.total_bytes, s.total_bytes),
            (r.mutations, s.mutations),
        ] {
            if value == 0 || value > ceiling {
                return Err(ReadError::Bounds);
            }
        }
        self.message.validate().map_err(|_| ReadError::Bounds)
    }
}

#[derive(Debug, Eq, PartialEq)]
pub(super) enum ReadError {
    Bounds,
    Missing,
    Corrupt,
    Identity,
    Legacy(HostProblem),
    Rows(DeliveryRowError),
    Store(StoreError),
}
impl From<HostProblem> for ReadError {
    fn from(e: HostProblem) -> Self {
        Self::Legacy(e)
    }
}
impl From<DeliveryRowError> for ReadError {
    fn from(e: DeliveryRowError) -> Self {
        Self::Rows(e)
    }
}

pub(super) enum StoredAuthority {
    Legacy(DurableState),
    Rich(RichStoredState),
}

pub(super) struct RichStoredState {
    pub(super) catalog: Arc<MqObjectCatalog>,
    pub(super) delivery: MqDeliveryKernel,
    pub(super) rows: DeliveryRows,
    pub(super) marker: RichMarker,
    /// Exact physical dependencies, including retained replay and marker rows.
    pub(super) versions: RowVersions,
    pub(super) retained_records: Vec<ProviderStateRecord>,
    /// Legacy replay values, never interpreted as issued opaque handle tokens.
    pub(super) replay: BTreeMap<String, RecordedResult>,
    pub(super) ownership: selection::operations::rows::OwnershipRows,
    pub(super) receipts: BTreeMap<String, selection::operations::receipt::OccurrenceReceipt>,
    pub(super) runtime: Option<selection::operations::SelectedRuntime>,
    limits: ReaderLimits,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub(super) struct RichMarker {
    schema_version: String,
    target_row_prefix: String,
    pub(super) identity: DeliveryRowIdentity,
    pub(super) source_manifest_version: u64,
    pub(super) source_catalog_version: u64,
    pub(super) legacy_next_handle: u32,
}

/// Generation/fence are trusted service inputs, never application envelope data.
/// Both backends capture this bounded prefix in one owned lock/SELECT snapshot.
pub(super) fn read(
    store: &dyn ProviderStateStore,
    generation: u64,
    fence: u64,
    limits: ReaderLimits,
) -> Result<StoredAuthority, ReadError> {
    limits.validate()?;
    let records = store
        .list_provider_state_prefix("mq-", limits.records + 1)
        .map_err(ReadError::Store)?;
    decode_records(records, generation, fence, limits)
}

/// Shared captured-record path. It has no store reference and cannot rescan.
pub(super) fn decode_records(
    records: Vec<ProviderStateRecord>,
    generation: u64,
    fence: u64,
    limits: ReaderLimits,
) -> Result<StoredAuthority, ReadError> {
    limits.validate()?;
    if generation == 0 || fence == 0 || generation > i64::MAX as u64 || fence > i64::MAX as u64 {
        return Err(ReadError::Identity);
    }
    if records.len() > limits.records {
        return Err(ReadError::Bounds);
    }
    // All physical budgets and identities are checked before typed decoding.
    let mut total = 0usize;
    let mut legacy_bytes = 0usize;
    let mut replay_bytes = 0usize;
    for record in &records {
        total = total
            .checked_add(record.payload.len())
            .ok_or(ReadError::Bounds)?;
        if record.payload.len() > limits.row_bytes || total > limits.total_bytes {
            return Err(ReadError::Bounds);
        }
        if record.validate_write(limits.row_bytes).is_err()
            || !matches!(
                record.namespace.as_str(),
                STATE_NAMESPACE
                    | CATALOG_NAMESPACE
                    | QUEUE_NAMESPACE
                    | HANDLE_NAMESPACE
                    | PENDING_NAMESPACE
                    | REPLAY_NAMESPACE
            ) && !record.namespace.starts_with(PREFIX)
                && !selection::operations::rows::is_ownership_namespace(&record.namespace)
                && record.namespace != selection::operations::receipt::NAMESPACE
        {
            return Err(ReadError::Corrupt);
        }
        if !record.namespace.starts_with(PREFIX) {
            legacy_bytes = legacy_bytes
                .checked_add(record.payload.len())
                .ok_or(ReadError::Bounds)?;
            if record.payload.len() > limits.legacy.max_state_bytes {
                return Err(ReadError::Bounds);
            }
        }
        if record.namespace == REPLAY_NAMESPACE {
            replay_bytes = replay_bytes
                .checked_add(record.payload.len())
                .ok_or(ReadError::Bounds)?;
        }
    }
    let mut versions = RowVersions::new();
    for record in &records {
        if versions
            .insert(
                (record.namespace.clone(), record.key.clone()),
                record.version,
            )
            .is_some()
        {
            return Err(ReadError::Corrupt);
        }
    }
    let manifests: Vec<_> = records
        .iter()
        .filter(|r| r.namespace == STATE_NAMESPACE)
        .collect();
    let manifest = match manifests.as_slice() {
        [] => return Err(ReadError::Missing),
        [r] if r.key == STATE_KEY => *r,
        _ => return Err(ReadError::Corrupt),
    };
    #[derive(Deserialize)]
    struct Schema {
        schema_version: String,
    }
    let schema: Schema =
        serde_json::from_slice(&manifest.payload).map_err(|_| ReadError::Corrupt)?;
    let catalogs = decode_family::<String>(&records, CATALOG_NAMESPACE, 1, limits)?;
    let catalog = match catalogs.into_iter().next() {
        None => None,
        Some((key, bytes)) if key == CATALOG_KEY => Some(Arc::new(
            MqObjectCatalog::decode(bytes.as_bytes(), catalog_limits(limits.legacy))
                .map_err(|_| ReadError::Corrupt)?,
        )),
        _ => return Err(ReadError::Corrupt),
    };
    match schema.schema_version.as_str() {
        ROW_STORE_SCHEMA => {
            if legacy_bytes > limits.legacy.max_state_bytes {
                return Err(ReadError::Bounds);
            }
            if records.iter().any(|r| {
                r.namespace.starts_with(PREFIX)
                    || selection::operations::rows::is_ownership_namespace(&r.namespace)
                    || r.namespace == selection::operations::receipt::NAMESPACE
            }) {
                return Err(ReadError::Corrupt);
            }
            let manifest = strict::manifest(&manifest.payload)?;
            if manifest.definitions.is_some() || manifest.next_handle == 0 {
                return Err(ReadError::Corrupt);
            }
            let state = State {
                definitions: None,
                catalog,
                queues: decode_family::<strict::StrictQueue>(
                    &records,
                    QUEUE_NAMESPACE,
                    limits.legacy.max_queues,
                    limits,
                )?
                .into_iter()
                .map(|(k, v)| (k, Arc::new(v.0)))
                .collect(),
                handles: decode_family::<strict::StrictHandles>(
                    &records,
                    HANDLE_NAMESPACE,
                    limits.legacy.max_handles,
                    limits,
                )?
                .into_iter()
                .map(|(k, v)| (k, Arc::new(v.0)))
                .collect(),
                pending: decode_family::<strict::StrictPending>(
                    &records,
                    PENDING_NAMESPACE,
                    limits.legacy.max_pending_units,
                    limits,
                )?
                .into_iter()
                .map(|(k, v)| (k, Arc::new(v.0)))
                .collect(),
                replay: decode_replay(&records, limits)?
                    .into_iter()
                    .map(|(k, v)| (k, Arc::new(v)))
                    .collect(),
                next_handle: manifest.next_handle,
            };
            for run in state.handles.keys().chain(state.pending.keys()) {
                mainframe_env_execution_api::RunUnitId::new(run, InvocationLimits::default())
                    .map_err(|_| ReadError::Corrupt)?;
            }
            validate_state(&state, limits.legacy)?;
            Ok(StoredAuthority::Legacy(DurableState { versions, state }))
        }
        legacy_delivery_import::RICH_MARKER_SCHEMA => {
            // The replacement marker can be larger than the source manifest.
            // Retained replay keeps its old ceiling; marker/catalog overhead
            // belongs to the combined physical budget, not a phantom v1 state.
            if replay_bytes > limits.legacy.max_state_bytes {
                return Err(ReadError::Bounds);
            }
            let marker: RichMarker =
                serde_json::from_slice(&manifest.payload).map_err(|_| ReadError::Corrupt)?;
            if marker.target_row_prefix != PREFIX
                || marker.legacy_next_handle == 0
                || marker.schema_version != legacy_delivery_import::RICH_MARKER_SCHEMA
                || records.iter().any(|r| {
                    matches!(
                        r.namespace.as_str(),
                        QUEUE_NAMESPACE | HANDLE_NAMESPACE | PENDING_NAMESPACE
                    )
                })
            {
                return Err(ReadError::Corrupt);
            }
            let catalog = catalog.ok_or(ReadError::Corrupt)?;
            let catalog_version = versions[&(CATALOG_NAMESPACE.into(), CATALOG_KEY.into())];
            for (source, current) in [
                (marker.source_manifest_version, manifest.version),
                (marker.source_catalog_version, catalog_version),
            ] {
                // Import's CAS increments each dependency once; later advances
                // are valid. Provenance is a historical bound, not current CAS.
                if source == 0 || source >= i64::MAX as u64 || current <= source {
                    return Err(ReadError::Corrupt);
                }
            }
            let expected = DeliveryRowIdentity::new(&catalog, generation, fence)?;
            if marker.identity != expected {
                return Err(ReadError::Identity);
            }
            let replay = decode_replay(&records, limits)?;
            let ownership = selection::operations::rows::OwnershipRows::restore(
                &records,
                generation,
                fence,
                limits.legacy,
            )?;
            let receipts = selection::operations::receipt::restore(
                &records,
                ownership.control.as_ref(),
                limits.legacy,
            )?;
            let (rich_records, retained_records) = records
                .into_iter()
                .partition(|r| r.namespace.starts_with(PREFIX));
            let (rows, delivery) = DeliveryRows::restore(
                rich_records,
                &catalog,
                expected,
                limits.rows,
                limits.delivery,
                limits.message,
                limits.default_persistence,
            )?;
            ownership.validate_delivery(&delivery, &rows)?;
            Ok(StoredAuthority::Rich(RichStoredState {
                catalog,
                delivery,
                rows,
                marker,
                versions,
                retained_records,
                replay,
                ownership,
                receipts,
                runtime: None,
                limits,
            }))
        }
        _ => Err(ReadError::Corrupt),
    }
}

fn decode_replay(
    records: &[ProviderStateRecord],
    limits: ReaderLimits,
) -> Result<BTreeMap<String, RecordedResult>, ReadError> {
    let replay = decode_family::<RecordedResult>(
        records,
        REPLAY_NAMESPACE,
        limits.legacy.max_replays,
        limits,
    )?;
    for (key, value) in &replay {
        IdempotencyKey::new(key, InvocationLimits::default()).map_err(|_| ReadError::Corrupt)?;
        validate_mq_recorded_result(key, value, limits.legacy).map_err(|_| ReadError::Corrupt)?;
    }
    Ok(replay)
}

fn decode_family<T: DeserializeOwned>(
    records: &[ProviderStateRecord],
    namespace: &str,
    max: usize,
    limits: ReaderLimits,
) -> Result<BTreeMap<String, T>, ReadError> {
    rows::decode_row_map(
        records.iter().filter(|r| r.namespace == namespace),
        namespace,
        max,
        limits.legacy,
        &mut RowVersions::new(),
    )
    .map_err(Into::into)
}

#[cfg(test)]
#[path = "service_rich_state/tests.rs"]
mod tests;
