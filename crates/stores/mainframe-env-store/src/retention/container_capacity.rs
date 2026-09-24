//! Exact private replay archive and command-data capacity replacement shape.

use mainframe_env_store_api::{
    ProviderRetentionDependency, ProviderStateArchiveDeletionWithCapacity, RetentionTarget,
    StoreError,
};
fn counts(payload: &[u8]) -> Result<[usize; 3], StoreError> {
    let value: serde_json::Value =
        serde_json::from_slice(payload).map_err(|_| StoreError::IncompatibleVersion)?;
    let object = value.as_object().ok_or(StoreError::IncompatibleVersion)?;
    if object.len() != 4
        || object
            .get("schema_version")
            .and_then(|value| value.as_u64())
            != Some(1)
    {
        return Err(StoreError::IncompatibleVersion);
    }
    let mut counts = [0; 3];
    for (index, name) in ["channels", "containers", "replays"].iter().enumerate() {
        counts[index] = usize::try_from(
            object
                .get(*name)
                .and_then(|value| value.as_u64())
                .ok_or(StoreError::IncompatibleVersion)?,
        )
        .map_err(|_| StoreError::IncompatibleVersion)?;
    }
    if counts[0] > 4_096 || counts[1] > 16_384 || counts[2] > 16_384 {
        return Err(StoreError::IncompatibleVersion);
    }
    Ok(counts)
}

pub(crate) fn validate_container_replay_archive(
    request: &ProviderStateArchiveDeletionWithCapacity,
    max_payload_bytes: usize,
) -> Result<(), StoreError> {
    let deletion = &request.deletion;
    super::validate_provider_deletion(deletion, max_payload_bytes, true)?;
    if deletion.target != RetentionTarget::CicsReplay
        || request.outer_receipts.len() != deletion.rows.len()
        || deletion.rows.iter().any(|candidate| {
            candidate.row.namespace != "cics-container-replay-v1"
                || candidate.observation.is_some()
                || !matches!(
                    candidate.dependency,
                    ProviderRetentionDependency::CoreEffect { .. }
                )
        })
    {
        return Err(StoreError::InvalidTransition);
    }
    let mut outer = std::collections::BTreeSet::new();
    for row in &request.outer_receipts {
        row.validate_write(max_payload_bytes)?;
        if row.namespace != "cics-effect-replay-v1"
            || !outer.insert(row.key.as_str())
            || !deletion
                .rows
                .iter()
                .any(|candidate| candidate.row.key == row.key)
        {
            return Err(StoreError::InvalidTransition);
        }
    }
    let source = &request.capacity_source;
    let replacement = &request.capacity_replacement;
    source.validate_write(max_payload_bytes)?;
    replacement.record.validate_write(max_payload_bytes)?;
    if source.namespace != "cics-container-capacity-v1"
        || source.key != "global"
        || source.payload.len() > 256
        || replacement.record.namespace != source.namespace
        || replacement.record.key != source.key
        || replacement.expected_version != Some(source.version)
        || replacement.record.version
            != source.version.checked_add(1).ok_or(StoreError::Conflict)?
        || replacement.record.payload.len() > 256
    {
        return Err(StoreError::InvalidTransition);
    }
    let before = counts(&source.payload)?;
    let after = counts(&replacement.record.payload)?;
    if after[0] != before[0]
        || after[1] != before[1]
        || after[2]
            != before[2]
                .checked_sub(deletion.rows.len())
                .ok_or(StoreError::IncompatibleVersion)?
    {
        return Err(StoreError::IncompatibleVersion);
    }
    Ok(())
}
