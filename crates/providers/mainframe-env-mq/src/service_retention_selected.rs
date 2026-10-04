//! Read-only protection of selected MQ recovery graphs, using the owning codec.

use super::*;
use crate::retention::MqSelectedRetentionDependencies;
use mainframe_env_execution_api::ExecutionId;
use mainframe_env_store_api::MAX_CORE_RETENTION_DEPENDENCIES;

pub(crate) fn selected_retention_dependencies(
    records: Vec<ProviderStateRecord>,
    limits: MqLimits,
) -> Result<MqSelectedRetentionDependencies, HostProblem> {
    let reader = rich_state::ReaderLimits {
        legacy: limits,
        ..Default::default()
    };
    // Preflight before even the small typed marker is allocated. The owning
    // decoder below applies all remaining positive/profile/combined budgets.
    if records.len() > reader.records {
        return Err(HostProblem::ResourceExhausted);
    }
    let mut total = 0usize;
    for record in &records {
        total = total
            .checked_add(record.payload.len())
            .ok_or(HostProblem::ResourceExhausted)?;
        if record.payload.len() > reader.row_bytes || total > reader.total_bytes {
            return Err(HostProblem::ResourceExhausted);
        }
    }
    let markers: Vec<_> = records
        .iter()
        .filter(|row| row.namespace == STATE_NAMESPACE && row.key == STATE_KEY)
        .collect();
    let [marker] = markers.as_slice() else {
        return Err(HostProblem::Malformed);
    };
    let marker: rich_state::RichMarker =
        serde_json::from_slice(&marker.payload).map_err(|_| HostProblem::Malformed)?;
    // Historical on-disk identity is sufficient for read validation only. No
    // service is opened and no live generation, incarnation or owner is minted.
    let (generation, fence) = marker.identity.generation_and_fence();
    let rich_state::StoredAuthority::Rich(state) =
        rich_state::decode_records(records, generation, fence, reader)
            .map_err(selection::selection_error)?
    else {
        return Err(HostProblem::Unsupported);
    };
    let mut executions = BTreeSet::new();
    let mut effect_keys = BTreeSet::new();
    for (execution, key) in state
        .receipts
        .values()
        .map(|receipt| receipt.retained_dependency())
        .chain(state.ownership.retained_dependencies())
    {
        executions.insert(
            ExecutionId::new(execution, InvocationLimits::default())
                .map_err(|_| HostProblem::Malformed)?,
        );
        effect_keys.insert(
            IdempotencyKey::new(key, InvocationLimits::default())
                .map_err(|_| HostProblem::Malformed)?,
        );
        if executions.len() > MAX_CORE_RETENTION_DEPENDENCIES
            || effect_keys.len() > MAX_CORE_RETENTION_DEPENDENCIES
        {
            return Err(HostProblem::ResourceExhausted);
        }
    }
    Ok(MqSelectedRetentionDependencies {
        executions: executions.into_iter().collect(),
        effect_keys: effect_keys.into_iter().collect(),
    })
}
