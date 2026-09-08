use mainframe_env_diagnostics::{
    DiagnosticCode, DiagnosticLimits, ExecutionProblem, FailureCategory, Phase,
};
use mainframe_env_execution_api::{
    ExecutionOutcome, Invocation, LifecycleEvent, LifecycleEventKind, Machine, MachineDrive,
    MachineResume, Quantum,
};
use mainframe_env_host_api::{
    EffectRequest, EffectResult, HostProblem, ScopedHostService, canonical_request_digest,
    canonical_result_digest,
};
use mainframe_env_store_api::{
    CheckpointRecord, EffectDigestFormat, EffectRecord, EffectState, ExecutionRecord,
    ExecutionState, OutboxRecord, PlatformStore, StoreError,
};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::sync::Arc;

const LIFECYCLE_OUTBOX_TOPIC: &str = "execution.lifecycle.v1";
const LIFECYCLE_OUTBOX_DOMAIN: &[u8] = b"mainframe-env.execution-lifecycle@1\0";

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

/// Failure to observe live execution controls is fail-closed, not permission to run.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExecutionControlError {
    Unavailable,
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
        self.execute_with_control(machine, invocation, || Ok(control))
    }

    /// Sample controls at admission, after every bounded machine quantum, and
    /// immediately before dispatching an effect. The callback owns the clock
    /// domain; observations must be monotonic. No wall clock enters the machine.
    pub fn execute_with_control<M, F>(
        &self,
        machine: &mut M,
        invocation: &Invocation,
        mut observe: F,
    ) -> ExecutionOutcome
    where
        M: Machine<Effect = EffectRequest, EffectResult = EffectResult>,
        F: FnMut() -> Result<ExecutionControl, ExecutionControlError>,
    {
        let mut control = match observe() {
            Ok(control) => control,
            Err(_) => return infrastructure_failure("execution controls unavailable at admission"),
        };
        let mut journal = match self
            .store
            .as_ref()
            .map(|store| JournalCursor::admit(Arc::clone(store), invocation, control.now_tick))
            .transpose()
        {
            Ok(journal) => journal,
            Err(_) => return infrastructure_failure("execution admission persistence failed"),
        };
        if let Err(outcome) = check_control(
            control,
            None,
            invocation,
            invocation.deadline_tick,
            &mut journal,
        ) {
            return outcome;
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
            let drive = machine.drive(resume, self.limits.quantum);
            // Uncertainty from work already performed outranks newly observed
            // cancellation/deadlines (and control-source failures).
            if !matches!(&drive, MachineDrive::Failed(p) if p.has_unknown_outcome()) {
                control = match observe_checked(
                    &mut observe,
                    control,
                    invocation,
                    invocation.deadline_tick,
                    &mut journal,
                ) {
                    Ok(control) => control,
                    Err(outcome) => return outcome,
                };
            }
            match drive {
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
                    let request_digest = match canonical_request_digest(&effect.request) {
                        Ok(digest) => digest,
                        Err(_) => {
                            return failed_outcome(problem(
                                FailureCategory::ResourceExhausted,
                                "canonical host request exceeds the journal encoding budget",
                            ));
                        }
                    };
                    let intent = effect.idempotency_key.as_ref().map(|key| EffectRecord {
                        execution_id: invocation.execution_id.clone(),
                        run_unit_id: invocation.run_unit_id.clone(),
                        sequence: effect.sequence,
                        key: key.clone(),
                        digest_format: EffectDigestFormat::CanonicalHostV1,
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
                    control = match observe_checked(
                        &mut observe,
                        control,
                        invocation,
                        invocation.deadline_tick.min(effect.deadline_tick),
                        &mut journal,
                    ) {
                        Ok(control) => control,
                        Err(outcome) => return outcome,
                    };
                    let result = host
                        .invoke(
                            invocation,
                            control.now_tick,
                            control.cancellation_requested,
                            effect,
                        )
                        .effect;
                    let result_digest = match canonical_result_digest(&result.outcome) {
                        Ok(digest) => digest,
                        // Dispatch already happened; leave the durable intent for reconciliation.
                        Err(_) => {
                            return failed_outcome(problem(
                                FailureCategory::UnknownOutcome,
                                "host result cannot be encoded after dispatch; reconcile the intent",
                            ));
                        }
                    };
                    let result_record = intent.map(|mut record| {
                        record.state = match &result.outcome {
                            Ok(_) => EffectState::Completed,
                            Err(HostProblem::UnknownOutcome) => EffectState::UnknownOutcome,
                            Err(_) => EffectState::Failed,
                        };
                        record.result_digest = Some(result_digest);
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
                    if matches!(&result.outcome, Err(HostProblem::UnknownOutcome)) {
                        // Never poll again or resume an ordinary exception handler
                        // before preserving this already-observed uncertainty.
                        let _ = record_step(
                            &mut journal,
                            Some(ExecutionState::Failed),
                            LifecycleEventKind::Failed,
                            None,
                            None,
                        );
                        return failed_outcome(problem(
                            FailureCategory::UnknownOutcome,
                            "host outcome unknown",
                        ));
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

fn observe_checked<F>(
    observe: &mut F,
    previous: ExecutionControl,
    invocation: &Invocation,
    deadline: u64,
    journal: &mut Option<JournalCursor>,
) -> Result<ExecutionControl, ExecutionOutcome>
where
    F: FnMut() -> Result<ExecutionControl, ExecutionControlError>,
{
    let control = observe().map_err(|_| {
        let _ = record_step(
            journal,
            Some(ExecutionState::Failed),
            LifecycleEventKind::Failed,
            None,
            None,
        );
        infrastructure_failure("live execution controls unavailable")
    })?;
    check_control(control, Some(previous), invocation, deadline, journal)?;
    Ok(control)
}

fn check_control(
    control: ExecutionControl,
    previous: Option<ExecutionControl>,
    invocation: &Invocation,
    deadline: u64,
    journal: &mut Option<JournalCursor>,
) -> Result<(), ExecutionOutcome> {
    if previous.is_some_and(|previous| control.now_tick < previous.now_tick) {
        let _ = record_step(
            journal,
            Some(ExecutionState::Failed),
            LifecycleEventKind::Failed,
            None,
            None,
        );
        return Err(infrastructure_failure("execution clock regressed"));
    }
    if let Some(journal) = journal.as_mut() {
        journal.tick = control.now_tick;
    }
    let terminal = if control.cancellation_requested || invocation.cancellation.is_some() {
        Some((
            ExecutionState::Cancelled,
            LifecycleEventKind::Cancelled,
            ExecutionOutcome::Cancelled,
        ))
    } else if control.now_tick >= deadline {
        Some((
            ExecutionState::TimedOut,
            LifecycleEventKind::TimedOut,
            ExecutionOutcome::TimedOut,
        ))
    } else {
        None
    };
    if let Some((state, event, outcome)) = terminal {
        record_step(journal, Some(state), event, None, None).map_err(|_| {
            infrastructure_failure("execution control termination persistence failed")
        })?;
        return Err(outcome);
    }
    Ok(())
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
        topic: LIFECYCLE_OUTBOX_TOPIC.into(),
        payload: lifecycle_payload(&event.kind),
        attempt: 0,
        delivered: false,
        version: 1,
    }
}

fn lifecycle_payload(kind: &LifecycleEventKind) -> Vec<u8> {
    let mut payload = Vec::with_capacity(LIFECYCLE_OUTBOX_DOMAIN.len() + 9);
    payload.extend_from_slice(LIFECYCLE_OUTBOX_DOMAIN);
    match kind {
        LifecycleEventKind::Admitted => payload.push(1),
        LifecycleEventKind::Queued => payload.push(2),
        LifecycleEventKind::Claimed => payload.push(3),
        LifecycleEventKind::Started => payload.push(4),
        LifecycleEventKind::Completing => payload.push(5),
        LifecycleEventKind::EffectIntent { sequence } => {
            payload.push(6);
            payload.extend_from_slice(&sequence.to_be_bytes());
        }
        LifecycleEventKind::EffectResult { sequence } => {
            payload.push(7);
            payload.extend_from_slice(&sequence.to_be_bytes());
        }
        LifecycleEventKind::Suspended => payload.push(8),
        LifecycleEventKind::Resumed => payload.push(9),
        LifecycleEventKind::CancellationRequested => payload.push(10),
        LifecycleEventKind::Cancelled => payload.push(11),
        LifecycleEventKind::TimedOut => payload.push(12),
        LifecycleEventKind::Completed { return_code } => {
            payload.push(13);
            payload.extend_from_slice(&return_code.to_be_bytes());
        }
        LifecycleEventKind::Condition => payload.push(14),
        LifecycleEventKind::Abend => payload.push(15),
        LifecycleEventKind::Failed => payload.push(16),
    }
    payload
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

    fn hex(value: &[u8]) -> String {
        value.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    #[test]
    fn lifecycle_outbox_payloads_are_versioned_unique_and_golden() {
        let effect = lifecycle_payload(&LifecycleEventKind::EffectIntent {
            sequence: 0x0102_0304_0506_0708,
        });
        let completed = lifecycle_payload(&LifecycleEventKind::Completed { return_code: -12 });
        assert_eq!(
            hex(&effect),
            "6d61696e6672616d652d656e762e657865637574696f6e2d6c6966656379636c65403100060102030405060708"
        );
        assert_eq!(
            hex(&completed),
            "6d61696e6672616d652d656e762e657865637574696f6e2d6c6966656379636c654031000dfffffff4"
        );

        let invocation = invocation();
        let record = notification(&LifecycleEvent {
            execution_id: invocation.execution_id.clone(),
            run_unit_id: invocation.run_unit_id.clone(),
            sequence: 1,
            attempt: 1,
            tick: 0,
            kind: LifecycleEventKind::Completed { return_code: -12 },
        });
        assert_eq!(record.topic, "execution.lifecycle.v1");
        assert_eq!(record.payload, completed);

        let payloads = [
            LifecycleEventKind::Admitted,
            LifecycleEventKind::Queued,
            LifecycleEventKind::Claimed,
            LifecycleEventKind::Started,
            LifecycleEventKind::Completing,
            LifecycleEventKind::EffectIntent { sequence: 1 },
            LifecycleEventKind::EffectResult { sequence: 1 },
            LifecycleEventKind::Suspended,
            LifecycleEventKind::Resumed,
            LifecycleEventKind::CancellationRequested,
            LifecycleEventKind::Cancelled,
            LifecycleEventKind::TimedOut,
            LifecycleEventKind::Completed { return_code: 0 },
            LifecycleEventKind::Condition,
            LifecycleEventKind::Abend,
            LifecycleEventKind::Failed,
        ]
        .into_iter()
        .map(|kind| lifecycle_payload(&kind))
        .collect::<BTreeSet<_>>();
        assert_eq!(payloads.len(), 16);
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
    struct CpuMachine {
        drives: usize,
    }
    impl Machine for CpuMachine {
        type Effect = EffectRequest;
        type EffectResult = EffectResult;
        fn drive(
            &mut self,
            _: MachineResume<EffectResult>,
            _: Quantum,
        ) -> MachineDrive<EffectRequest> {
            self.drives += 1;
            MachineDrive::Continue
        }
    }

    #[test]
    fn live_cpu_controls_are_observed_after_one_quantum_before_budget_exhaustion() {
        for cancel in [true, false] {
            let coordinator = ExecutionCoordinator::local(CoordinatorLimits {
                max_quanta: 1,
                ..CoordinatorLimits::default()
            });
            let mut machine = CpuMachine { drives: 0 };
            let mut observations = 0;
            let result = coordinator.execute_with_control(&mut machine, &invocation(), || {
                observations += 1;
                Ok(ExecutionControl {
                    now_tick: if observations == 1 { 0 } else { 100 },
                    cancellation_requested: cancel && observations > 1,
                })
            });
            assert_eq!(
                result,
                if cancel {
                    ExecutionOutcome::Cancelled
                } else {
                    ExecutionOutcome::TimedOut
                }
            );
            assert_eq!(machine.drives, 1);
            assert_eq!(observations, 2);
        }
    }

    #[test]
    fn completion_cannot_hide_a_deadline_crossed_during_its_quantum() {
        let coordinator = ExecutionCoordinator::local(CoordinatorLimits::default());
        let mut observations = 0;
        let result = coordinator.execute_with_control(&mut CompleteMachine, &invocation(), || {
            observations += 1;
            Ok(ExecutionControl {
                now_tick: if observations == 1 { 1 } else { 100 },
                cancellation_requested: false,
            })
        });
        assert_eq!(result, ExecutionOutcome::TimedOut);
    }

    #[test]
    fn clock_regression_and_control_failure_stop_before_a_second_quantum() {
        for unavailable in [true, false] {
            let mut machine = CpuMachine { drives: 0 };
            let mut observations = 0;
            let result = ExecutionCoordinator::local(CoordinatorLimits::default())
                .execute_with_control(&mut machine, &invocation(), || {
                    observations += 1;
                    if unavailable && observations > 1 {
                        return Err(ExecutionControlError::Unavailable);
                    }
                    Ok(ExecutionControl {
                        now_tick: if observations == 1 { 50 } else { 49 },
                        cancellation_requested: false,
                    })
                });
            assert!(matches!(result, ExecutionOutcome::InfrastructureFailure(_)));
            assert_eq!(machine.drives, 1);
        }
    }

    #[test]
    fn observed_uncertainty_is_not_overwritten_by_a_new_stop_observation() {
        struct Uncertain;
        impl Machine for Uncertain {
            type Effect = EffectRequest;
            type EffectResult = EffectResult;
            fn drive(
                &mut self,
                _: MachineResume<EffectResult>,
                _: Quantum,
            ) -> MachineDrive<EffectRequest> {
                MachineDrive::Failed(problem(
                    FailureCategory::UnknownOutcome,
                    "already performed uncertain work",
                ))
            }
        }
        let mut observations = 0;
        let result = ExecutionCoordinator::local(CoordinatorLimits::default())
            .execute_with_control(&mut Uncertain, &invocation(), || {
                observations += 1;
                assert_eq!(
                    observations, 1,
                    "uncertainty must be preserved before any new observation"
                );
                Ok(ExecutionControl::default())
            });
        assert!(matches!(result, ExecutionOutcome::ProviderFailure(p) if p.has_unknown_outcome()));
    }

    #[test]
    fn observed_stop_prevents_new_host_dispatch() {
        struct Calling;
        impl Machine for Calling {
            type Effect = EffectRequest;
            type EffectResult = EffectResult;
            fn drive(
                &mut self,
                _: MachineResume<EffectResult>,
                _: Quantum,
            ) -> MachineDrive<EffectRequest> {
                MachineDrive::HostCall(EffectRequest {
                    run_unit: invocation().run_unit_id,
                    sequence: 1,
                    deadline_tick: 100,
                    idempotency_key: None,
                    request: mainframe_env_host_api::HostRequest::Program(
                        mainframe_env_host_api::ProgramRequest::Call {
                            program: mainframe_env_host_api::ProgramName::new("NEVER", 128)
                                .unwrap(),
                            payload: mainframe_env_execution_api::BoundedPayload::new(
                                "test@1",
                                vec![],
                                InvocationLimits::default(),
                            )
                            .unwrap(),
                            service: None,
                        },
                    ),
                })
            }
        }
        let mut observations = 0;
        let result = ExecutionCoordinator::local(CoordinatorLimits::default())
            .execute_with_control(&mut Calling, &invocation(), || {
                observations += 1;
                Ok(ExecutionControl {
                    now_tick: 0,
                    cancellation_requested: observations > 1,
                })
            });
        // A host lookup would fail in this local-only coordinator. The stop wins first.
        assert_eq!(result, ExecutionOutcome::Cancelled);
    }
}
