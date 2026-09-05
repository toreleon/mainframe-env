use mainframe_env_diagnostics::{
    DiagnosticCode, DiagnosticLimits, ExecutionProblem, FailureCategory, Phase,
};
use mainframe_env_execution_api::{
    ExecutionOutcome, Invocation, LifecycleEvent, LifecycleEventKind, Machine, MachineDrive,
    MachineResume, Quantum,
};
use mainframe_env_host_api::{EffectRequest, EffectResult, HostProblem, ScopedHostService};
use mainframe_env_store_api::{
    CheckpointRecord, EffectRecord, EffectState, ExecutionRecord, ExecutionState, OutboxRecord,
    PlatformStore, StoreError,
};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CoordinatorLimits {
    pub quantum: Quantum,
    pub max_quanta: u64,
}

impl Default for CoordinatorLimits {
    fn default() -> Self {
        Self {
            quantum: Quantum::new(10_000, 16 * 1024 * 1024).expect("non-zero quantum"),
            max_quanta: 100_000,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ExecutionControl {
    pub now_tick: u64,
    pub cancellation_requested: bool,
}

/// The one 0.1 machine-driving authority used by CLI, batch, and server paths.
///
/// It owns drive ordering, cancellation/deadline observation, typed host-result
/// resumption, and the optional transactional execution journal.
pub struct ExecutionCoordinator {
    host: Option<Arc<ScopedHostService>>,
    store: Option<Arc<dyn PlatformStore>>,
    limits: CoordinatorLimits,
}

impl ExecutionCoordinator {
    #[must_use]
    pub fn local(limits: CoordinatorLimits) -> Self {
        Self {
            host: None,
            store: None,
            limits,
        }
    }

    #[must_use]
    pub fn with_host(host: Arc<ScopedHostService>, limits: CoordinatorLimits) -> Self {
        Self {
            host: Some(host),
            store: None,
            limits,
        }
    }

    #[must_use]
    pub fn durable(
        host: Arc<ScopedHostService>,
        store: Arc<dyn PlatformStore>,
        limits: CoordinatorLimits,
    ) -> Self {
        Self {
            host: Some(host),
            store: Some(store),
            limits,
        }
    }

    pub fn execute<M>(
        &self,
        machine: &mut M,
        invocation: &Invocation,
        control: ExecutionControl,
    ) -> ExecutionOutcome
    where
        M: Machine<Effect = EffectRequest, EffectResult = EffectResult>,
    {
        let mut journal = match self
            .store
            .as_ref()
            .map(|store| JournalCursor::admit(Arc::clone(store), invocation, control.now_tick))
            .transpose()
        {
            Ok(journal) => journal,
            Err(_) => return infrastructure_failure("execution admission persistence failed"),
        };
        if control.cancellation_requested {
            if record_step(
                &mut journal,
                Some(ExecutionState::Cancelled),
                LifecycleEventKind::Cancelled,
                None,
                None,
            )
            .is_err()
            {
                return infrastructure_failure("cancellation persistence failed");
            }
            return ExecutionOutcome::Cancelled;
        }
        if control.now_tick >= invocation.deadline_tick {
            if record_step(
                &mut journal,
                Some(ExecutionState::TimedOut),
                LifecycleEventKind::TimedOut,
                None,
                None,
            )
            .is_err()
            {
                return infrastructure_failure("timeout persistence failed");
            }
            return ExecutionOutcome::TimedOut;
        }
        for (state, event) in [
            (ExecutionState::Queued, LifecycleEventKind::Queued),
            (ExecutionState::Running, LifecycleEventKind::Started),
        ] {
            if record_step(&mut journal, Some(state), event, None, None).is_err() {
                return infrastructure_failure("execution dispatch persistence failed");
            }
        }
        let mut resume = MachineResume::Start;
        for _ in 0..self.limits.max_quanta {
            match machine.drive(resume, self.limits.quantum) {
                MachineDrive::Continue => resume = MachineResume::Start,
                MachineDrive::HostCall(effect) => {
                    let Some(host) = &self.host else {
                        let outcome = ExecutionOutcome::ProviderFailure(problem(
                            FailureCategory::ProviderFailure,
                            "selected execution profile has no host provider",
                        ));
                        let _ = record_step(
                            &mut journal,
                            Some(ExecutionState::Failed),
                            LifecycleEventKind::Failed,
                            None,
                            None,
                        );
                        return outcome;
                    };
                    let request_digest: [u8; 32] =
                        Sha256::digest(format!("{:?}", effect.request).as_bytes()).into();
                    let intent = effect.idempotency_key.as_ref().map(|key| EffectRecord {
                        execution_id: invocation.execution_id.clone(),
                        run_unit_id: invocation.run_unit_id.clone(),
                        sequence: effect.sequence,
                        key: key.clone(),
                        request_digest,
                        state: EffectState::Intent,
                        result_digest: None,
                    });
                    if record_step(
                        &mut journal,
                        None,
                        LifecycleEventKind::EffectIntent {
                            sequence: effect.sequence,
                        },
                        intent.clone(),
                        None,
                    )
                    .is_err()
                    {
                        return infrastructure_failure("effect intent persistence failed");
                    }
                    let result = host
                        .invoke(
                            invocation,
                            control.now_tick,
                            control.cancellation_requested,
                            effect,
                        )
                        .effect;
                    let result_record = intent.map(|mut record| {
                        record.state = match &result.outcome {
                            Ok(_) => EffectState::Completed,
                            Err(HostProblem::UnknownOutcome) => EffectState::UnknownOutcome,
                            Err(_) => EffectState::Failed,
                        };
                        record.result_digest =
                            Some(Sha256::digest(format!("{:?}", result.outcome).as_bytes()).into());
                        record
                    });
                    if record_step(
                        &mut journal,
                        None,
                        LifecycleEventKind::EffectResult {
                            sequence: result.sequence,
                        },
                        result_record,
                        None,
                    )
                    .is_err()
                    {
                        if matches!(&result.outcome, Err(HostProblem::UnknownOutcome)) {
                            return failed_outcome(problem(
                                FailureCategory::UnknownOutcome,
                                "host outcome unknown; result persistence also failed",
                            ));
                        }
                        return infrastructure_failure("effect result persistence failed");
                    }
                    resume = MachineResume::HostResult(result);
                }
                MachineDrive::Invoke(child) => {
                    if suspend(&mut journal, machine, invocation, None).is_err() {
                        return infrastructure_failure("child invocation checkpoint failed");
                    }
                    return ExecutionOutcome::Invoke(child);
                }
                MachineDrive::Transfer(transfer) => {
                    if suspend(&mut journal, machine, invocation, None).is_err() {
                        return infrastructure_failure("transfer checkpoint failed");
                    }
                    return ExecutionOutcome::Transfer(transfer);
                }
                MachineDrive::Suspended(suspension) => {
                    if suspend(
                        &mut journal,
                        machine,
                        invocation,
                        Some(suspension.resume_token.clone()),
                    )
                    .is_err()
                    {
                        return infrastructure_failure("suspension checkpoint failed");
                    }
                    return ExecutionOutcome::Suspended(suspension);
                }
                MachineDrive::Completed(completion) => {
                    if record_step(
                        &mut journal,
                        Some(ExecutionState::Completing),
                        LifecycleEventKind::Completing,
                        None,
                        None,
                    )
                    .and_then(|()| {
                        record_step(
                            &mut journal,
                            Some(ExecutionState::Completed),
                            LifecycleEventKind::Completed {
                                return_code: completion.return_code,
                            },
                            None,
                            None,
                        )
                    })
                    .is_err()
                    {
                        return infrastructure_failure("completion persistence failed");
                    }
                    return ExecutionOutcome::Completed(completion);
                }
                MachineDrive::Condition(condition) => {
                    if record_step(
                        &mut journal,
                        Some(ExecutionState::Failed),
                        LifecycleEventKind::Condition,
                        None,
                        None,
                    )
                    .is_err()
                    {
                        return infrastructure_failure("condition persistence failed");
                    }
                    return ExecutionOutcome::Condition(condition);
                }
                MachineDrive::Abend(abend) => {
                    if record_step(
                        &mut journal,
                        Some(ExecutionState::Failed),
                        LifecycleEventKind::Abend,
                        None,
                        None,
                    )
                    .is_err()
                    {
                        return infrastructure_failure("ABEND persistence failed");
                    }
                    return ExecutionOutcome::Abend(abend);
                }
                MachineDrive::Failed(problem) => {
                    let (state, event) = match problem.category {
                        FailureCategory::Cancelled => {
                            (ExecutionState::Cancelled, LifecycleEventKind::Cancelled)
                        }
                        FailureCategory::TimedOut => {
                            (ExecutionState::TimedOut, LifecycleEventKind::TimedOut)
                        }
                        _ => (ExecutionState::Failed, LifecycleEventKind::Failed),
                    };
                    if record_step(&mut journal, Some(state), event, None, None).is_err() {
                        if problem.has_unknown_outcome() {
                            return failed_outcome(problem);
                        }
                        return infrastructure_failure("failure persistence failed");
                    }
                    return failed_outcome(problem);
                }
            }
        }
        let outcome = ExecutionOutcome::ResourceExhausted(problem(
            FailureCategory::ResourceExhausted,
            "execution quantum limit exhausted",
        ));
        if record_step(
            &mut journal,
            Some(ExecutionState::Failed),
            LifecycleEventKind::Failed,
            None,
            None,
        )
        .is_err()
        {
            return infrastructure_failure("resource exhaustion persistence failed");
        }
        outcome
    }
}

struct JournalCursor {
    store: Arc<dyn PlatformStore>,
    execution_id: mainframe_env_execution_api::ExecutionId,
    run_unit_id: mainframe_env_execution_api::RunUnitId,
    attempt: u32,
    tick: u64,
    version: u64,
    sequence: u64,
}

impl JournalCursor {
    fn admit(
        store: Arc<dyn PlatformStore>,
        invocation: &Invocation,
        tick: u64,
    ) -> Result<Self, StoreError> {
        let event = LifecycleEvent {
            execution_id: invocation.execution_id.clone(),
            run_unit_id: invocation.run_unit_id.clone(),
            sequence: 1,
            attempt: invocation.attempt,
            tick,
            kind: LifecycleEventKind::Admitted,
        };
        let notification = notification(&event);
        store.admit_execution(
            ExecutionRecord {
                execution_id: invocation.execution_id.clone(),
                run_unit_id: invocation.run_unit_id.clone(),
                selector: invocation.selector.clone(),
                artifact: invocation.artifact.clone(),
                principal: invocation.principal.id().clone(),
                state: ExecutionState::Admitted,
                attempt: invocation.attempt,
                version: 1,
                owner_lease: None,
                lease_expiry_tick: None,
            },
            event,
            notification,
        )?;
        Ok(Self {
            store,
            execution_id: invocation.execution_id.clone(),
            run_unit_id: invocation.run_unit_id.clone(),
            attempt: invocation.attempt,
            tick,
            version: 1,
            sequence: 1,
        })
    }

    fn record(
        &mut self,
        next_state: Option<ExecutionState>,
        kind: LifecycleEventKind,
        effect: Option<EffectRecord>,
        checkpoint: Option<CheckpointRecord>,
    ) -> Result<(), StoreError> {
        self.sequence = self
            .sequence
            .checked_add(1)
            .ok_or(StoreError::InvalidSequence)?;
        let event = LifecycleEvent {
            execution_id: self.execution_id.clone(),
            run_unit_id: self.run_unit_id.clone(),
            sequence: self.sequence,
            attempt: self.attempt,
            tick: self.tick,
            kind,
        };
        let notification = notification(&event);
        let execution = self.store.commit_execution_step(
            &self.execution_id,
            self.version,
            next_state,
            event,
            effect,
            checkpoint,
            notification,
        )?;
        self.version = execution.version;
        Ok(())
    }
}

fn notification(event: &LifecycleEvent) -> OutboxRecord {
    OutboxRecord {
        notification_id: format!("{}:{:020}", event.execution_id, event.sequence),
        execution_id: event.execution_id.clone(),
        sequence: event.sequence,
        topic: "execution.lifecycle".into(),
        payload: format!("{:?}", event.kind).into_bytes(),
        attempt: 0,
        delivered: false,
        version: 1,
    }
}

fn record_step(
    journal: &mut Option<JournalCursor>,
    next_state: Option<ExecutionState>,
    event: LifecycleEventKind,
    effect: Option<EffectRecord>,
    checkpoint: Option<CheckpointRecord>,
) -> Result<(), StoreError> {
    if let Some(journal) = journal {
        journal.record(next_state, event, effect, checkpoint)
    } else {
        Ok(())
    }
}

fn suspend<M>(
    journal: &mut Option<JournalCursor>,
    machine: &M,
    invocation: &Invocation,
    session_id: Option<String>,
) -> Result<(), StoreError>
where
    M: Machine<Effect = EffectRequest, EffectResult = EffectResult>,
{
    let checkpoint = if journal.is_some() {
        let payload = machine
            .checkpoint()
            .ok_or(StoreError::IncompatibleVersion)?;
        let payload_digest: [u8; 32] = Sha256::digest(payload.bytes()).into();
        Some(CheckpointRecord {
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
            effect_sequence: machine.effect_sequence(),
            transaction: None,
            principal: invocation.principal.id().clone(),
            security_classification: "application-data".into(),
            encryption_key_reference: None,
            payload_size: payload.bytes().len() as u64,
            payload_digest,
            payload: payload.bytes().to_vec(),
        })
    } else {
        None
    };
    record_step(
        journal,
        Some(ExecutionState::Suspended),
        LifecycleEventKind::Suspended,
        None,
        checkpoint,
    )
}

fn infrastructure_failure(message: &str) -> ExecutionOutcome {
    ExecutionOutcome::InfrastructureFailure(problem(
        FailureCategory::InfrastructureFailure,
        message,
    ))
}

fn failed_outcome(problem: ExecutionProblem) -> ExecutionOutcome {
    if problem.has_unknown_outcome() {
        return ExecutionOutcome::ProviderFailure(problem);
    }
    match problem.category {
        FailureCategory::Cancelled => ExecutionOutcome::Cancelled,
        FailureCategory::TimedOut => ExecutionOutcome::TimedOut,
        FailureCategory::ResourceExhausted => ExecutionOutcome::ResourceExhausted(problem),
        FailureCategory::ProviderFailure | FailureCategory::UnknownOutcome => {
            ExecutionOutcome::ProviderFailure(problem)
        }
        FailureCategory::InfrastructureFailure => ExecutionOutcome::InfrastructureFailure(problem),
        _ => ExecutionOutcome::Rejected(problem),
    }
}

fn problem(category: FailureCategory, message: &str) -> ExecutionProblem {
    ExecutionProblem::new(
        DiagnosticCode::new("MECOORD0001").expect("static diagnostic code"),
        category,
        Phase::Execute,
        message,
        false,
        category == FailureCategory::UnknownOutcome,
        DiagnosticLimits::default(),
    )
    .expect("bounded static execution problem")
}

#[cfg(test)]
mod tests {
    use super::*;
    use mainframe_env_execution_api::{
        ArtifactRef, Completion, ExecutionId, IdempotencyKey, InvocationLimits, Principal,
        PrincipalId, RequestId, ResourceLimits, RunUnitId, Selector, ServiceClass, TraceId,
    };
    use std::collections::{BTreeMap, BTreeSet};

    struct CompleteMachine;
    impl Machine for CompleteMachine {
        type Effect = EffectRequest;
        type EffectResult = EffectResult;

        fn drive(
            &mut self,
            resume: MachineResume<Self::EffectResult>,
            _: Quantum,
        ) -> MachineDrive<Self::Effect> {
            match resume {
                MachineResume::Cancelled => MachineDrive::Failed(problem(
                    FailureCategory::Cancelled,
                    "cancelled before dispatch",
                )),
                _ => MachineDrive::Completed(Completion {
                    return_code: 0,
                    output: mainframe_env_execution_api::BoundedPayload::new(
                        "test@1",
                        b"OK".to_vec(),
                        InvocationLimits::default(),
                    )
                    .unwrap(),
                }),
            }
        }
    }

    fn invocation() -> Invocation {
        let limits = InvocationLimits::default();
        Invocation::new(
            RequestId::new("request", limits).unwrap(),
            ExecutionId::new("execution", limits).unwrap(),
            RunUnitId::new("run", limits).unwrap(),
            None,
            Selector::new("test", limits).unwrap(),
            ArtifactRef::new("artifact", limits).unwrap(),
            Principal::new(
                PrincipalId::new("USER", limits).unwrap(),
                BTreeSet::new(),
                limits,
            )
            .unwrap(),
            ServiceClass::Interactive,
            0,
            100,
            TraceId::new("trace", limits).unwrap(),
            IdempotencyKey::new("key", limits).unwrap(),
            1,
            ResourceLimits::default(),
            BTreeMap::new(),
            limits,
        )
        .unwrap()
    }

    #[test]
    fn completion_cancellation_and_timeout_use_one_driver() {
        let coordinator = ExecutionCoordinator::local(CoordinatorLimits::default());
        assert!(matches!(
            coordinator.execute(
                &mut CompleteMachine,
                &invocation(),
                ExecutionControl::default()
            ),
            ExecutionOutcome::Completed(_)
        ));
        assert_eq!(
            coordinator.execute(
                &mut CompleteMachine,
                &invocation(),
                ExecutionControl {
                    cancellation_requested: true,
                    ..ExecutionControl::default()
                }
            ),
            ExecutionOutcome::Cancelled
        );
        assert_eq!(
            coordinator.execute(
                &mut CompleteMachine,
                &invocation(),
                ExecutionControl {
                    now_tick: 100,
                    cancellation_requested: false,
                }
            ),
            ExecutionOutcome::TimedOut
        );
    }
}
