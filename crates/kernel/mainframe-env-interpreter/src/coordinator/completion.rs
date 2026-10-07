//! Optional terminal capture within the existing final journal transaction.
use super::*;
use mainframe_env_execution_api::BoundedPayload;

pub(super) fn record_completion<M>(
    journal: &mut Option<JournalCursor>,
    machine: &M,
    invocation: &Invocation,
    return_code: i32,
) -> Result<(), StoreError>
where
    M: Machine<Effect = EffectRequest, EffectResult = EffectResult>,
{
    record_step(
        journal,
        Some(ExecutionState::Completing),
        LifecycleEventKind::Completing,
        None,
        None,
    )?;
    // Capture only the live machine that returned Completed. No stale store
    // checkpoint is consulted and an unavailable optional capture stays absent.
    let checkpoint = if journal.is_some() {
        machine
            .completion_checkpoint()
            .map(|payload| checkpoint_record(invocation, machine.effect_sequence(), None, payload))
    } else {
        None
    };
    record_step(
        journal,
        Some(ExecutionState::Completed),
        LifecycleEventKind::Completed { return_code },
        None,
        checkpoint,
    )
}

pub(super) fn checkpoint_record(
    invocation: &Invocation,
    effect_sequence: u64,
    session_id: Option<String>,
    payload: BoundedPayload,
) -> CheckpointRecord {
    CheckpointRecord {
        execution_id: invocation.execution_id.clone(),
        run_unit_id: invocation.run_unit_id.clone(),
        session_id,
        schema_version: 1,
        machine_schema_version: 1,
        artifact: invocation.artifact.clone(),
        provider_generation: crate::INTERPRETER_GENERATION.into(),
        required_host_interfaces: BTreeMap::from([
            ("mainframe-env.execution-api".into(), "1".into()),
            ("mainframe-env.host-api".into(), "1".into()),
        ]),
        effect_sequence,
        transaction: None,
        principal: invocation.principal.id().clone(),
        security_classification: "application-data".into(),
        encryption_key_reference: None,
        payload_size: payload.bytes().len() as u64,
        payload_digest: Sha256::digest(payload.bytes()).into(),
        payload: payload.bytes().to_vec(),
    }
}

#[cfg(test)]
mod tests;
