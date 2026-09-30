use mainframe_env_host_api::HostProblem;
use mainframe_env_store_api::{
    ProviderStateMutation, ProviderStateRecord, ProviderStateStore, ProviderStateWrite, StoreError,
};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

pub(crate) const CATALOG_NAMESPACE: &str = "ims-tm-v1-catalog";
pub(crate) const CATALOG_KEY: &str = "transactions";
pub(crate) const MESSAGE_NAMESPACE: &str = "ims-tm-v1-message";
pub(crate) const SESSION_NAMESPACE: &str = "ims-tm-v1-session";
pub(crate) const CONVERSATION_NAMESPACE: &str = "ims-tm-v1-conversation";
pub(crate) const OUTBOUND_NAMESPACE: &str = "ims-tm-v1-outbound";
pub(crate) const REPLAY_NAMESPACE: &str = "ims-tm-v1-replay";
const OBJECT_SCHEMA: &str = "mainframe-env.ims-tm-object@1";

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ObjectRow<T> {
    schema_version: String,
    object_key: String,
    value: T,
}

pub(crate) fn read<T: DeserializeOwned>(
    store: &dyn ProviderStateStore,
    namespace: &str,
    key: &str,
    max_bytes: usize,
) -> Result<Option<(u64, T)>, HostProblem> {
    store
        .get_provider_state(namespace, key)
        .map_err(store_error)?
        .map(|record| decode(record, namespace, key, max_bytes))
        .transpose()
}

pub(crate) fn list<T: DeserializeOwned>(
    store: &dyn ProviderStateStore,
    namespace: &str,
    max: usize,
    max_bytes: usize,
) -> Result<Vec<(String, u64, T)>, HostProblem> {
    let requested = max.checked_add(1).ok_or(HostProblem::ResourceExhausted)?;
    let rows = store
        .list_provider_state(namespace, requested)
        .map_err(store_error)?;
    if rows.len() > max {
        return Err(HostProblem::ResourceExhausted);
    }
    rows.into_iter()
        .map(|record| {
            let key = record.key.clone();
            let (version, value) = decode(record, namespace, &key, max_bytes)?;
            Ok((key, version, value))
        })
        .collect()
}

fn decode<T: DeserializeOwned>(
    record: ProviderStateRecord,
    namespace: &str,
    key: &str,
    max_bytes: usize,
) -> Result<(u64, T), HostProblem> {
    if record.namespace != namespace
        || record.key != key
        || record.version == 0
        || record.payload.len() > max_bytes
    {
        return Err(HostProblem::InfrastructureFailure);
    }
    let row: ObjectRow<T> =
        serde_json::from_slice(&record.payload).map_err(|_| HostProblem::InfrastructureFailure)?;
    if row.schema_version != OBJECT_SCHEMA || row.object_key != key {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok((record.version, row.value))
}

pub(crate) fn put<T: Serialize>(
    namespace: &str,
    key: &str,
    value: &T,
    current_version: Option<u64>,
    max_bytes: usize,
) -> Result<ProviderStateMutation, HostProblem> {
    let payload = serde_json::to_vec(&ObjectRow {
        schema_version: OBJECT_SCHEMA.to_string(),
        object_key: key.to_string(),
        value,
    })
    .map_err(|_| HostProblem::InfrastructureFailure)?;
    if payload.len() > max_bytes {
        return Err(HostProblem::ResourceExhausted);
    }
    let version = current_version
        .unwrap_or(0)
        .checked_add(1)
        .ok_or(HostProblem::ResourceExhausted)?;
    Ok(ProviderStateMutation::Put(ProviderStateWrite {
        record: ProviderStateRecord {
            namespace: namespace.into(),
            key: key.into(),
            version,
            payload,
        },
        expected_version: current_version,
    }))
}

pub(crate) fn delete(namespace: &str, key: &str, version: u64) -> ProviderStateMutation {
    ProviderStateMutation::Delete {
        namespace: namespace.into(),
        key: key.into(),
        expected_version: version,
    }
}

pub(crate) fn mutate(
    store: &dyn ProviderStateStore,
    mutations: Vec<ProviderStateMutation>,
) -> Result<(), HostProblem> {
    if mutations.is_empty() {
        return Ok(());
    }
    store
        .mutate_provider_states_atomic(mutations)
        .map_err(store_error)
}

pub(crate) fn store_error(problem: StoreError) -> HostProblem {
    match problem {
        StoreError::Conflict | StoreError::AlreadyExists => HostProblem::IdempotencyConflict,
        StoreError::CapacityExceeded | StoreError::PayloadTooLarge => {
            HostProblem::ResourceExhausted
        }
        _ => HostProblem::InfrastructureFailure,
    }
}
