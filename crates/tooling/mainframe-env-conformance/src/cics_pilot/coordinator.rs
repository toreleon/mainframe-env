use mainframe_env_compiler_api::PublishedArtifact;
use mainframe_env_execution_api::{
    IdempotencyKey, Invocation, InvocationLimits, LifecycleEventKind, Quantum,
};
use mainframe_env_host_api::ScopedHostService;
use mainframe_env_interpreter::{
    CoordinatorLimits, ExecutionControl, ExecutionCoordinator, ReferenceMachine,
};
use mainframe_env_ir::CodecLimits;
use mainframe_env_store_api::{EffectState, ExecutionState, PlatformStore};
use std::sync::Arc;

pub(super) struct PilotExecution {
    coordinator: ExecutionCoordinator,
    store: Arc<dyn PlatformStore>,
}

impl PilotExecution {
    pub(super) fn new(host: Arc<ScopedHostService>, store: Arc<dyn PlatformStore>) -> Self {
        let coordinator = ExecutionCoordinator::durable(
            host,
            store.clone(),
            CoordinatorLimits {
                quantum: Quantum::new(128, 4096).expect("bounded CICS pilot quantum"),
                ..CoordinatorLimits::default()
            },
        );
        Self { coordinator, store }
    }

    #[cfg(test)]
    pub(super) fn coordinator(&self) -> &ExecutionCoordinator {
        &self.coordinator
    }
}

pub(super) fn drive_artifact(
    artifact: &PublishedArtifact,
    invocation: Invocation,
    execution: &PilotExecution,
) -> Result<String, String> {
    let mut machine = ReferenceMachine::from_binary(
        artifact.payload(),
        invocation.clone(),
        CodecLimits::default(),
    )
    .map_err(|problem| format!("{problem:?}"))?;
    match execution.coordinator.execute(
        &mut machine,
        &invocation,
        ExecutionControl {
            now_tick: 1,
            cancellation_requested: false,
        },
    ) {
        mainframe_env_execution_api::ExecutionOutcome::Completed(done) => {
            validate_journal(execution.store.as_ref(), artifact, &invocation)?;
            String::from_utf8(done.output.bytes().to_vec())
                .map_err(|_| "CICS pilot application output is not UTF-8".into())
        }
        other => Err(format!(
            "CICS pilot coordinator did not complete: {other:?}"
        )),
    }
}

fn validate_journal(
    store: &dyn PlatformStore,
    artifact: &PublishedArtifact,
    invocation: &Invocation,
) -> Result<(), String> {
    let module = mainframe_env_ir::decode_binary(artifact.payload(), CodecLimits::default())
        .map_err(|problem| problem.to_string())?;
    let cics_operations = module
        .regions()
        .iter()
        .flat_map(|region| &region.blocks)
        .flat_map(|block| &block.operations)
        .filter(|operation| {
            matches!(
                operation.identity.namespace(),
                "cics.file" | "cics.recovery" | "cics.task"
            )
        })
        .collect::<Vec<_>>();
    if cics_operations.is_empty() {
        return Err("CICS pilot artifact has no typed CICS effects".into());
    }
    let expected_sequences = (1..=cics_operations.len())
        .map(|sequence| u64::try_from(sequence).map_err(|_| "too many CICS effects".to_string()))
        .collect::<Result<Vec<_>, _>>()?;
    let execution = store
        .get_execution(&invocation.execution_id)
        .map_err(|problem| problem.to_string())?
        .ok_or_else(|| "coordinator did not admit the CICS pilot execution".to_string())?;
    let events = store
        .events(&invocation.execution_id, 1, 65_536)
        .map_err(|problem| problem.to_string())?;
    let intent_sequences = events
        .iter()
        .filter_map(|event| match event.kind {
            LifecycleEventKind::EffectIntent { sequence } => Some(sequence),
            _ => None,
        })
        .collect::<Vec<_>>();
    let result_sequences = events
        .iter()
        .filter_map(|event| match event.kind {
            LifecycleEventKind::EffectResult { sequence } => Some(sequence),
            _ => None,
        })
        .collect::<Vec<_>>();
    let ordered = events
        .iter()
        .enumerate()
        .all(|(index, event)| event.sequence == index as u64 + 1);
    if execution.state != ExecutionState::Completed
        || execution.terminal_tick.is_none()
        || execution.version != events.last().map_or(0, |event| event.sequence)
        || !ordered
        || !matches!(
            events.first().map(|event| &event.kind),
            Some(LifecycleEventKind::Admitted)
        )
        || !matches!(
            events.get(1).map(|event| &event.kind),
            Some(LifecycleEventKind::Queued)
        )
        || !matches!(
            events.get(2).map(|event| &event.kind),
            Some(LifecycleEventKind::Started)
        )
        || !matches!(
            events.iter().rev().nth(1).map(|event| &event.kind),
            Some(LifecycleEventKind::Completing)
        )
        || !matches!(
            events.last().map(|event| &event.kind),
            Some(LifecycleEventKind::Completed { .. })
        )
        || intent_sequences != expected_sequences
        || result_sequences != expected_sequences
    {
        return Err("CICS pilot coordinator lifecycle or effect ordering drifted".into());
    }
    let cics_audits = store
        .audit_records(&invocation.execution_id, 1, 65_536)
        .map_err(|problem| problem.to_string())?
        .into_iter()
        .filter(|record| record.capability.as_str() == "host.cics.execute")
        .collect::<Vec<_>>();
    if cics_audits
        .iter()
        .map(|record| record.effect_sequence)
        .collect::<Vec<_>>()
        != expected_sequences
    {
        return Err("CICS pilot coordinator audit sequence drifted".into());
    }
    for (index, _) in cics_operations.iter().enumerate().filter(|(_, operation)| {
        matches!(
            operation.identity.name(),
            "deq" | "enq" | "rewrite" | "syncpoint"
        )
    }) {
        let sequence = u64::try_from(index + 1).map_err(|_| "too many CICS effects")?;
        let key = IdempotencyKey::new(
            format!("{}:{sequence}", invocation.idempotency_key.as_str()),
            InvocationLimits::default(),
        )
        .map_err(|problem| problem.to_string())?;
        let effect = store
            .effect(&key)
            .map_err(|problem| problem.to_string())?
            .ok_or_else(|| format!("coordinator omitted mutating CICS effect {sequence}"))?;
        if effect.state != EffectState::Completed
            || effect.execution_id != invocation.execution_id
            || effect.sequence != sequence
        {
            return Err(format!(
                "coordinator mutating CICS effect {sequence} did not complete exactly once"
            ));
        }
    }
    Ok(())
}
