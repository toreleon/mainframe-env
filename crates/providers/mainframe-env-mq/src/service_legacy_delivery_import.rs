//! Read-only quiescent import planning for the single service authority.
//! No public selection or automatic upgrade. Admission, audit/effect composition
//! and adoption after the full atomic transaction remain manager-owned.

use super::rich_state::RICH_MARKER_SCHEMA;
use super::*;
use crate::delivery::checkpoint::rows::{
    DeliveryRowError, DeliveryRowIdentity, DeliveryRowLimits, DeliveryRows,
};
use crate::{MqDeliveryError, MqDeliveryKernel, MqDeliveryLimits, MqObjectName};
use mainframe_env_host_api::{
    MqExpiry, MqMessage, MqMessageDescriptor, MqMessageIdentifiers, MqMessageLimits,
    MqMessageOrdering, MqPersistence, MqPriority,
};

#[derive(Clone, Copy, Default)]
pub(crate) struct LegacyImportLimits {
    pub(crate) delivery: MqDeliveryLimits,
    pub(crate) message: MqMessageLimits,
    pub(crate) rows: DeliveryRowLimits,
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum LegacyImportError {
    NonQuiescent,
    InvalidSource,
    SourceChanged,
    RichStatePresent,
    Bounds,
    Kernel(MqDeliveryError),
    Rows(DeliveryRowError),
    Host(HostProblem),
}
impl From<HostProblem> for LegacyImportError {
    fn from(value: HostProblem) -> Self {
        Self::Host(value)
    }
}
impl From<MqDeliveryError> for LegacyImportError {
    fn from(value: MqDeliveryError) -> Self {
        Self::Kernel(value)
    }
}
impl From<DeliveryRowError> for LegacyImportError {
    fn from(value: DeliveryRowError) -> Self {
        Self::Rows(value)
    }
}

// Neither v1 RowStoreManifest nor legacy State can decode this shape. The v2
// reader is manager-owned; the old reader fails rather than opening empty state.
#[derive(Serialize)]
struct RichMarker {
    schema_version: &'static str,
    target_row_prefix: &'static str,
    identity: DeliveryRowIdentity,
    source_manifest_version: u64,
    source_catalog_version: u64,
    legacy_next_handle: u32,
}

pub(crate) struct LegacyDeliveryImportPlan {
    mutations: Vec<ProviderStateMutation>,
    catalog: MqObjectCatalog,
    kernel: MqDeliveryKernel,
    target: DeliveryRows,
}
impl LegacyDeliveryImportPlan {
    pub(crate) fn mutations(&self) -> &[ProviderStateMutation] {
        &self.mutations
    }

    /// Adopt ONLY after the full composed transaction commits. Failure does not
    /// authorize retrying or redispatching a mutation.
    pub(crate) fn into_parts(
        self,
    ) -> (
        Vec<ProviderStateMutation>,
        MqObjectCatalog,
        MqDeliveryKernel,
        DeliveryRows,
    ) {
        (self.mutations, self.catalog, self.kernel, self.target)
    }
}
impl MqService {
    /// Private plan only, never invoked by open or a public route.
    pub(crate) fn plan_legacy_delivery_import(
        &self,
        generation: u64,
        fence: u64,
        limits: LegacyImportLimits,
    ) -> Result<LegacyDeliveryImportPlan, LegacyImportError> {
        let durable = self.lock()?;
        plan(
            &*self.store,
            &durable,
            self.limits,
            generation,
            fence,
            limits,
        )
    }
}

fn plan(
    store: &dyn ProviderStateStore,
    durable: &DurableState,
    source_limits: MqLimits,
    generation: u64,
    fence: u64,
    limits: LegacyImportLimits,
) -> Result<LegacyDeliveryImportPlan, LegacyImportError> {
    validate_source_limits(source_limits)?;
    validate_state(&durable.state, source_limits)?;
    let state = &durable.state;
    if !state.pending.is_empty() || state.handles.values().any(|h| !h.is_empty()) {
        return Err(LegacyImportError::NonQuiescent);
    }
    let catalog = state
        .catalog
        .as_deref()
        .ok_or(LegacyImportError::InvalidSource)?;
    if state.definitions.is_some() {
        return Err(LegacyImportError::InvalidSource);
    }
    let identity = DeliveryRowIdentity::new(catalog, generation, fence)?;
    // Stable bounded physical snapshot. All old logical publishers CAS its
    // manifest, so a publication after this scan conflicts with the final CAS.
    let max_rows = source_limits
        .max_queues
        .checked_add(source_limits.max_handles)
        .and_then(|n| n.checked_add(source_limits.max_pending_units))
        .and_then(|n| n.checked_add(source_limits.max_replays))
        .and_then(|n| n.checked_add(2))
        .ok_or(LegacyImportError::Bounds)?;
    let source = store
        .list_provider_state_prefix("mq-", max_rows + 1)
        .map_err(store_error)?;
    if source.len() > max_rows {
        return Err(LegacyImportError::Bounds);
    }
    let mut records = BTreeMap::new();
    let mut bytes = 0usize;
    for record in source {
        if record
            .namespace
            .starts_with(crate::delivery::checkpoint::rows::PREFIX)
        {
            return Err(LegacyImportError::RichStatePresent);
        }
        if ![
            STATE_NAMESPACE,
            CATALOG_NAMESPACE,
            QUEUE_NAMESPACE,
            HANDLE_NAMESPACE,
            PENDING_NAMESPACE,
            REPLAY_NAMESPACE,
        ]
        .contains(&record.namespace.as_str())
            || record
                .validate_write(source_limits.max_state_bytes)
                .is_err()
        {
            return Err(LegacyImportError::InvalidSource);
        }
        bytes = bytes
            .checked_add(record.payload.len())
            .ok_or(LegacyImportError::Bounds)?;
        if bytes > source_limits.max_state_bytes {
            return Err(LegacyImportError::Bounds);
        }
        let key = (record.namespace.clone(), record.key.clone());
        if records.insert(key, record).is_some() {
            return Err(LegacyImportError::InvalidSource);
        }
    }
    let expected_keys = std::iter::once((STATE_NAMESPACE.into(), STATE_KEY.into()))
        .chain(std::iter::once((
            CATALOG_NAMESPACE.into(),
            CATALOG_KEY.into(),
        )))
        .chain(
            state
                .queues
                .keys()
                .map(|k| (QUEUE_NAMESPACE.into(), k.clone())),
        )
        .chain(
            state
                .handles
                .keys()
                .map(|k| (HANDLE_NAMESPACE.into(), k.clone())),
        )
        .chain(
            state
                .replay
                .keys()
                .map(|k| (REPLAY_NAMESPACE.into(), k.clone())),
        )
        .collect::<BTreeSet<_>>();
    if records.keys().cloned().collect::<BTreeSet<_>>() != expected_keys
        || durable.versions.keys().cloned().collect::<BTreeSet<_>>() != expected_keys
    {
        return Err(LegacyImportError::SourceChanged);
    }
    for (key, record) in &records {
        if durable.versions.get(key) != Some(&record.version) {
            return Err(LegacyImportError::SourceChanged);
        }
    }
    let manifest = &records[&(STATE_NAMESPACE.into(), STATE_KEY.into())];
    let old: RowStoreManifest =
        serde_json::from_slice(&manifest.payload).map_err(|_| LegacyImportError::InvalidSource)?;
    if old
        != (RowStoreManifest {
            schema_version: ROW_STORE_SCHEMA.into(),
            definitions: None,
            next_handle: state.next_handle,
        })
    {
        return Err(LegacyImportError::InvalidSource);
    }
    let catalog_row = &records[&(CATALOG_NAMESPACE.into(), CATALOG_KEY.into())];
    let catalog_text: String = decode_source(catalog_row)?;
    let loaded_catalog = MqObjectCatalog::decode(
        catalog_text.as_bytes(),
        crate::object_service::catalog_limits(source_limits),
    )
    .map_err(|_| LegacyImportError::InvalidSource)?;
    if &loaded_catalog != catalog {
        return Err(LegacyImportError::InvalidSource);
    }
    let mut queues = BTreeMap::new();
    for (name, queue) in &state.queues {
        let strict: StrictQueue = decode_source(&records[&(QUEUE_NAMESPACE.into(), name.clone())])?;
        if strict.0 != **queue {
            return Err(LegacyImportError::InvalidSource);
        }
        queues.insert(
            MqObjectName::new(name).map_err(|_| LegacyImportError::InvalidSource)?,
            strict
                .0
                .messages
                .into_iter()
                .map(compatibility_message)
                .collect(),
        );
    }
    for (name, handles) in &state.handles {
        let loaded: BTreeMap<u32, String> =
            decode_source(&records[&(HANDLE_NAMESPACE.into(), name.clone())])?;
        if loaded != **handles || !loaded.is_empty() {
            return Err(LegacyImportError::NonQuiescent);
        }
    }
    for (key, replay) in &state.replay {
        let loaded: RecordedResult =
            decode_source(&records[&(REPLAY_NAMESPACE.into(), key.clone())])?;
        if loaded != **replay {
            return Err(LegacyImportError::InvalidSource);
        }
    }
    let kernel =
        MqDeliveryKernel::import_legacy_queues(catalog, limits.delivery, limits.message, queues)?;
    let target = DeliveryRows::initialize(store, &kernel, catalog, identity.clone(), limits.rows)?;
    let (mut mutations, target) = target.into_parts();
    for record in records
        .values()
        .filter(|r| [QUEUE_NAMESPACE, HANDLE_NAMESPACE].contains(&r.namespace.as_str()))
    {
        mutations.push(ProviderStateMutation::Delete {
            namespace: record.namespace.clone(),
            key: record.key.clone(),
            expected_version: record.version,
        });
    }
    // Exact-byte retained catalog fence. Replay rows never enter the batch.
    mutations.push(dependency_put(catalog_row, catalog_row.payload.clone())?);
    let marker = RichMarker {
        schema_version: RICH_MARKER_SCHEMA,
        target_row_prefix: crate::delivery::checkpoint::rows::PREFIX,
        identity,
        source_manifest_version: manifest.version,
        source_catalog_version: catalog_row.version,
        legacy_next_handle: state.next_handle,
    };
    mutations.push(dependency_put(
        manifest,
        serde_json::to_vec(&marker).map_err(|_| LegacyImportError::InvalidSource)?,
    )?);
    let batch_bytes = mutations
        .iter()
        .try_fold(0usize, |n, m| {
            n.checked_add(match m {
                ProviderStateMutation::Put(w) => w.record.payload.len(),
                _ => 0,
            })
        })
        .ok_or(LegacyImportError::Bounds)?;
    if mutations.len() > limits.rows.mutations || batch_bytes > limits.rows.total_bytes
        || mutations.iter().any(|m| matches!(m, ProviderStateMutation::Put(w) if w.record.payload.len() > limits.rows.row_bytes)) { return Err(LegacyImportError::Bounds); }
    Ok(LegacyDeliveryImportPlan {
        mutations,
        catalog: catalog.clone(),
        kernel,
        target,
    })
}

fn dependency_put(
    record: &ProviderStateRecord,
    payload: Vec<u8>,
) -> Result<ProviderStateMutation, LegacyImportError> {
    let version = record
        .version
        .checked_add(1)
        .filter(|v| *v <= i64::MAX as u64)
        .ok_or(LegacyImportError::Bounds)?;
    Ok(ProviderStateMutation::Put(ProviderStateWrite {
        record: ProviderStateRecord {
            version,
            payload,
            ..record.clone()
        },
        expected_version: Some(record.version),
    }))
}
fn decode_source<T: DeserializeOwned>(
    record: &ProviderStateRecord,
) -> Result<T, LegacyImportError> {
    let row: ObjectRow<T> =
        serde_json::from_slice(&record.payload).map_err(|_| LegacyImportError::InvalidSource)?;
    if row.schema_version != OBJECT_ROW_SCHEMA || row.object_key != record.key {
        return Err(LegacyImportError::InvalidSource);
    }
    Ok(row.value)
}

// Remote derives retain the owned legacy types, with strict import-only field
// validation. They do not alter the historical v1 reader or canonical bytes.
#[derive(Deserialize)]
#[serde(remote = "Message", deny_unknown_fields)]
struct LegacyMessage {
    data: Vec<u8>,
    message_id: Vec<u8>,
    correlation_id: Vec<u8>,
}
#[derive(Deserialize)]
#[serde(transparent)]
struct StrictMessage(#[serde(with = "LegacyMessage")] Message);
#[derive(Deserialize)]
#[serde(remote = "Queue", deny_unknown_fields)]
struct LegacyQueue {
    #[serde(deserialize_with = "required_trigger")]
    trigger_program: Option<String>,
    #[serde(deserialize_with = "strict_messages")]
    messages: Vec<Message>,
}
#[derive(Deserialize)]
#[serde(transparent)]
struct StrictQueue(#[serde(with = "LegacyQueue")] Queue);
fn strict_messages<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<Message>, D::Error> {
    Ok(Vec::<StrictMessage>::deserialize(d)?
        .into_iter()
        .map(|m| m.0)
        .collect())
}
fn required_trigger<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    Option::deserialize(d)
}
fn compatibility_message(message: Message) -> MqMessage {
    MqMessage {
        descriptor: MqMessageDescriptor {
            identifiers: MqMessageIdentifiers {
                message_id: Some(message.message_id),
                correlation_id: Some(message.correlation_id),
                group_id: None,
            },
            format: None,
            expiry: MqExpiry::Unlimited,
            persistence: MqPersistence::Persistent,
            priority: MqPriority::QueueDefault,
            ordering: MqMessageOrdering::default(),
        },
        body: message.data,
        properties: Vec::new(),
    }
}
fn validate_source_limits(limits: MqLimits) -> Result<(), LegacyImportError> {
    let max = MqLimits::default();
    for (n, ceiling) in [
        (limits.max_queues, max.max_queues),
        (limits.max_messages_per_queue, max.max_messages_per_queue),
        (limits.max_message_bytes, max.max_message_bytes),
        (limits.max_handles, max.max_handles),
        (limits.max_pending_units, max.max_pending_units),
        (limits.max_replays, max.max_replays),
        (limits.max_state_bytes, max.max_state_bytes),
    ] {
        if n == 0 || n > ceiling {
            return Err(LegacyImportError::Bounds);
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "service_legacy_delivery_import/tests.rs"]
mod tests;
