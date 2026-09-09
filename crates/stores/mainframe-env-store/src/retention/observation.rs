use super::{target_name, validate_archive_row_domain};
use mainframe_env_store_api::{
    ArchivedRetentionRow, MAX_PROVIDER_KEY_BYTES, MAX_PROVIDER_NAMESPACE_BYTES,
    RetentionObservation, StoreError,
};
use sha2::{Digest, Sha256};

pub(crate) const OBSERVATION_FIXED_BYTES: u64 = 96;

pub(crate) fn source_digest(payload: &[u8]) -> [u8; 32] {
    Sha256::digest(payload).into()
}

pub(crate) fn validate(observation: &RetentionObservation) -> Result<(), StoreError> {
    if observation.namespace.is_empty()
        || observation.namespace.len() > MAX_PROVIDER_NAMESPACE_BYTES
        || observation.key.is_empty()
        || observation.key.len() > MAX_PROVIDER_KEY_BYTES
        || observation.source_version == 0
        || observation.source_version > i64::MAX as u64
        || observation.observed_tick == 0
    {
        return Err(StoreError::IncompatibleVersion);
    }
    validate_archive_row_domain(
        observation.target,
        &ArchivedRetentionRow {
            namespace: observation.namespace.clone(),
            key: observation.key.clone(),
            version: observation.source_version,
            payload: Vec::new(),
            retention_tick: observation.observed_tick,
            owner_execution: observation.owner_execution.clone(),
        },
    )?;
    storage_bytes(observation).map(|_| ())
}

pub(crate) fn storage_bytes(observation: &RetentionObservation) -> Result<u64, StoreError> {
    let variable = target_name(observation.target)
        .len()
        .checked_add(observation.namespace.len())
        .and_then(|bytes| bytes.checked_add(observation.key.len()))
        .and_then(|bytes| {
            bytes.checked_add(
                observation
                    .owner_execution
                    .as_ref()
                    .map_or(0, |owner| owner.as_str().len()),
            )
        })
        .and_then(|bytes| bytes.checked_add(32))
        .ok_or(StoreError::CapacityExceeded)?;
    OBSERVATION_FIXED_BYTES
        .checked_add(u64::try_from(variable).map_err(|_| StoreError::CapacityExceeded)?)
        .ok_or(StoreError::CapacityExceeded)
}

pub(crate) fn matches_source(
    observation: &RetentionObservation,
    source_version: u64,
    payload: &[u8],
) -> bool {
    observation.source_version == source_version
        && observation.source_digest == source_digest(payload)
}
