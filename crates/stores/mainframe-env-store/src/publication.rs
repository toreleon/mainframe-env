//! Shared validation, using the existing effect/execution/audit authorities.

use crate::validation;
#[cfg(test)]
pub(crate) mod tests;
use mainframe_env_store_api::{
    AuditedProviderPublication, EffectDigestFormat, EffectRecord, ExecutionRecord, ExecutionState,
    StoreError,
};

pub(crate) fn validate(
    request: &AuditedProviderPublication,
    max_payload: usize,
) -> Result<(), StoreError> {
    request.validate_mutations(max_payload)?;
    validation::new_intent(&request.intent)?;
    validation::audit(&request.audit)?;
    let intent = &request.intent;
    let audit = &request.audit;
    if intent.digest_format != EffectDigestFormat::CanonicalHostV1
        || intent.intent.created_tick == 0
        || intent.intent.recovery_after_tick > i64::MAX as u64
        || intent.intent.recovery_lease.is_some()
        || request.observed_tick == 0
        || request.observed_tick > i64::MAX as u64
        || request.observed_tick < intent.intent.created_tick
        || request.observed_tick >= intent.intent.recovery_after_tick
    {
        return Err(StoreError::LeaseConflict);
    }
    if audit.execution_id != intent.execution_id
        || audit.run_unit_id != intent.run_unit_id
        || audit.attempt != intent.intent.attempt
        || audit.effect_sequence != intent.sequence
        || audit.observed_tick != request.observed_tick
        || intent.intent.capability.as_ref() != Some(&audit.capability)
        || intent.intent.audit_resource != Some(audit.resource)
        || intent.intent.audit_invocation_key.as_ref() != Some(&audit.invocation_key)
    {
        return Err(StoreError::Conflict);
    }
    Ok(())
}

/// Must run inside the same physical transaction/lock as all publication writes.
pub(crate) fn assert_fence(
    request: &AuditedProviderPublication,
    retained: &EffectRecord,
    execution: &ExecutionRecord,
    clock_tick: u64,
) -> Result<(), StoreError> {
    if retained != &request.intent {
        return Err(StoreError::Conflict);
    }
    validation::effect_execution(execution, retained)?;
    if execution.state != ExecutionState::Running
        || execution.terminal_tick.is_some()
        || execution.principal != request.audit.principal
    {
        return Err(StoreError::Conflict);
    }
    if clock_tick > request.observed_tick
        || execution
            .lease_expiry_tick
            .is_some_and(|expiry| expiry <= request.observed_tick)
    {
        return Err(StoreError::LeaseConflict);
    }
    Ok(())
}
