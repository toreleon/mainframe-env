//! Bounded generic deletion-plan shape validation; provider payloads remain provider-owned.
use super::*;

pub(crate) fn validate_provider_deletion(
    request: &ProviderStateArchiveDeletion,
    max_payload_bytes: usize,
    allow_container_replay: bool,
) -> Result<(), StoreError> {
    if request.archived_tick == 0
        || request.watermark_tick > request.archived_tick
        || request.rows.is_empty()
        || request.rows.len() > MAX_RETENTION_BATCH
    {
        return Err(StoreError::InvalidTransition);
    }
    let allowed_namespaces: &[&str] = match request.target {
        RetentionTarget::Db2Replay => &["db2-v1-replay"],
        RetentionTarget::ImsReplay => &["ims-v1-replay"],
        RetentionTarget::MqReplay => &["mq-v1-replay"],
        RetentionTarget::CicsReplay if allow_container_replay => &["cics-container-replay-v1"],
        RetentionTarget::CicsReplay => &["cics-effect-replay-v1"],
        RetentionTarget::DatasetReplay => &["dataset-replay"],
        RetentionTarget::CicsUnitOfWork => &["cics-uow", "cics-uow-undo"],
        RetentionTarget::CobolLifecycle => &[
            "cobol-call-replay@1",
            "cobol-call-protocol@1",
            "cobol-call-protocol@2",
            "cobol-run-state@1",
            "cobol-cancel@1",
        ],
        RetentionTarget::SpoolJobs => &["jes-spool"],
        RetentionTarget::ConsoleLog => &["console-log"],
        _ => return Err(StoreError::InvalidTransition),
    };
    let mut identities = std::collections::BTreeSet::new();
    let candidate_identities = request
        .rows
        .iter()
        .map(|candidate| (candidate.row.namespace.as_str(), candidate.row.key.as_str()))
        .collect::<std::collections::BTreeSet<_>>();
    for candidate in &request.rows {
        let row = &candidate.row;
        let namespace_allowed = allowed_namespaces.contains(&row.namespace.as_str())
            || request.target == RetentionTarget::CobolLifecycle
                && row.namespace.starts_with("cobol-instance@1:");
        if !namespace_allowed
            || row.key.is_empty()
            || row.key.len() > 1024
            || row.version == 0
            || row.payload.len() > max_payload_bytes
            || candidate.retention_tick == 0
            || candidate.retention_tick > request.watermark_tick
            || !identities.insert((row.namespace.as_str(), row.key.as_str()))
        {
            return Err(StoreError::IncompatibleVersion);
        }
        if let Some(proof) = &candidate.observation {
            let observation = &proof.observation;
            observation::validate(observation)?;
            if proof.version == 0
                || observation.target != request.target
                || observation.namespace != row.namespace
                || observation.key != row.key
                || observation.source_version != row.version
                || observation.source_digest != source_digest(&row.payload)
                || observation.owner_execution != candidate.owner_execution
                || observation.observed_tick > candidate.retention_tick
            {
                return Err(StoreError::IncompatibleVersion);
            }
        }
        match &candidate.dependency {
            ProviderRetentionDependency::CoreEffect { key, .. }
                if key.as_str() == row.key
                    && candidate.owner_execution.is_some()
                    && candidate.owner_run_unit.is_some() => {}
            ProviderRetentionDependency::CicsNested {
                provenance,
                absent,
                required_executions,
            } => {
                let owner = candidate
                    .owner_execution
                    .as_ref()
                    .ok_or(StoreError::IncompatibleVersion)?;
                let run = candidate
                    .owner_run_unit
                    .as_ref()
                    .ok_or(StoreError::IncompatibleVersion)?;
                let prefix = format!("cics:{}:", run.as_str());
                if !row
                    .key
                    .strip_prefix(&prefix)
                    .is_some_and(|sequence| sequence.parse::<u64>().is_ok_and(|value| value != 0))
                    || owner.as_str().is_empty()
                {
                    return Err(StoreError::IncompatibleVersion);
                }
                provenance.validate_write(max_payload_bytes)?;
                if provenance.namespace != "cics-uow"
                    || absent.is_empty()
                    || absent.len() > 8
                    || required_executions.len() > 32
                    || absent.iter().any(|identity| {
                        identity.namespace.is_empty()
                            || identity.namespace.len() > MAX_PROVIDER_NAMESPACE_BYTES
                            || identity.key.is_empty()
                            || identity.key.len() > MAX_PROVIDER_KEY_BYTES
                    })
                {
                    return Err(StoreError::IncompatibleVersion);
                }
            }
            ProviderRetentionDependency::ProviderGraph {
                required_rows,
                required_executions,
            } if candidate.owner_execution.is_some()
                && candidate.owner_run_unit.is_some()
                && required_rows.len() <= 32
                && required_executions.len() <= 32
                && required_rows.iter().all(|required| {
                    required.validate_write(max_payload_bytes).is_ok()
                        && !candidate_identities
                            .contains(&(required.namespace.as_str(), required.key.as_str()))
                }) => {}
            ProviderRetentionDependency::DirectProduct
                if request.target == RetentionTarget::ConsoleLog
                    && candidate.owner_execution.is_none()
                    && candidate.owner_run_unit.is_none() => {}
            ProviderRetentionDependency::None
                if request.target == RetentionTarget::SpoolJobs
                    && candidate.owner_execution.is_none()
                    && candidate.owner_run_unit.is_none() => {}
            _ => return Err(StoreError::IncompatibleVersion),
        }
    }
    Ok(())
}
