use mainframe_env_diagnostics::{
    DiagnosticCode, DiagnosticLimits, ExecutionProblem, FailureCategory, Phase,
};
use mainframe_env_execution_api::{
    AuditRecord, ExecutionOutcome, Invocation, InvocationLimits, LifecycleEvent,
    LifecycleEventKind, Machine, MachineDrive, MachineResume, ParticipantContractProblem, Quantum,
    TransactionParticipantContract, TransactionParticipantDescriptor,
    read_transaction_participant_contract,
};
use mainframe_env_host_api::{
    EffectRequest, EffectResult, HostProblem, ScopedHostService, canonical_audit_resource_digest,
    canonical_request_digest, canonical_result_digest,
};
use mainframe_env_store_api::{
    AuditSink, CheckpointRecord, EffectDigestFormat, EffectIntentMetadata, EffectRecord,
    EffectState, ExecutionRecord, ExecutionState, OutboxRecord, PlatformStore, StoreError,
};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::sync::Arc;

mod original_dispatch;
mod root_terminal;
use root_terminal::NativeProgress;
pub use root_terminal::{
    NativeChildEnrollment, NativeRootAdmission, NativeRootConfiguration, NativeRootHooks,
    NativeRootTermination, WinningRootTerminal,
};

const LIFECYCLE_OUTBOX_TOPIC: &str = "execution.lifecycle.v1";

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
    audit_sink: Option<Arc<dyn AuditSink>>,
    limits: CoordinatorLimits,
}

struct PlatformAuditSink(Arc<dyn PlatformStore>);

impl AuditSink for PlatformAuditSink {
    fn record_audit(&self, record: AuditRecord) -> Result<(), StoreError> {
        self.0.record_audit(record)
    }

    fn audit_records(
        &self,
        execution_id: &mainframe_env_execution_api::ExecutionId,
        start_effect_sequence: u64,
        max: usize,
    ) -> Result<Vec<AuditRecord>, StoreError> {
        self.0
            .audit_records(execution_id, start_effect_sequence, max)
    }
}

impl ExecutionCoordinator {
    #[must_use]
    pub fn local(limits: CoordinatorLimits) -> Self {
        Self {
            host: None,
            store: None,
            audit_sink: None,
            limits,
        }
    }

    #[must_use]
    pub fn with_host(
        host: Arc<ScopedHostService>,
        audit_sink: Arc<dyn AuditSink>,
        limits: CoordinatorLimits,
    ) -> Self {
        Self {
            host: Some(host),
            store: None,
            audit_sink: Some(audit_sink),
            limits,
        }
    }

    /// Execute without a lifecycle journal while retaining every host decision in a platform
    /// store's typed audit sink. Callers that share execution authority with the store must use
    /// [`Self::durable`] instead.
    #[must_use]
    pub fn with_host_audit_store(
        host: Arc<ScopedHostService>,
        store: Arc<dyn PlatformStore>,
        limits: CoordinatorLimits,
    ) -> Self {
        Self::with_host(host, Arc::new(PlatformAuditSink(store)), limits)
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
            audit_sink: None,
            limits,
        }
    }

    /// Return the additive participant contract consumed by this coordinator.
    ///
    /// The descriptor does not select or dispatch a provider. Host effects
    /// continue through the existing scoped service and durable effect journal.
    pub fn transaction_participant_contract(
        &self,
    ) -> Result<&'static TransactionParticipantContract, ParticipantContractProblem> {
        read_transaction_participant_contract(1)
    }

    /// Resolve one declared participant without changing capability readiness.
    pub fn transaction_participant_descriptor(
        &self,
        provider_id: &str,
    ) -> Result<&'static TransactionParticipantDescriptor, ParticipantContractProblem> {
        self.transaction_participant_contract()?
            .participant(provider_id)
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
        observe: F,
    ) -> ExecutionOutcome
    where
        M: Machine<Effect = EffectRequest, EffectResult = EffectResult>,
        F: FnMut() -> Result<ExecutionControl, ExecutionControlError>,
    {
        self.execute_inner(machine, invocation, observe, false, None, None, None)
    }

    /// Resume a durably journaled execution after a process boundary.
    ///
    /// Only a matching non-terminal execution can be resumed. A previously
    /// completed effect is dispatched solely through its original idempotency
    /// identity and its replayed result must match the committed canonical
    /// digest. An unresolved intent or unknown result is never redispatched.
    pub fn execute_resumable_with_control<M, F>(
        &self,
        machine: &mut M,
        invocation: &Invocation,
        observe: F,
    ) -> ExecutionOutcome
    where
        M: Machine<Effect = EffectRequest, EffectResult = EffectResult>,
        F: FnMut() -> Result<ExecutionControl, ExecutionControlError>,
    {
        self.execute_inner(machine, invocation, observe, true, None, None, None)
    }

    /// Close a suspended execution after its checkpoint has been durably
    /// transferred to a product-owned continuation.
    ///
    /// The state transition and its lifecycle event are committed atomically.
    /// The now-redundant interpreter checkpoint is then removed; a caller may
    /// safely retry that cleanup when recovering the terminal execution.
    pub fn complete_suspended_handoff(
        &self,
        invocation: &Invocation,
        tick: u64,
    ) -> Result<(), StoreError> {
        let store = self.store.as_ref().ok_or_else(|| {
            StoreError::Infrastructure("handoff completion requires an execution store".into())
        })?;
        let (mut journal, state) =
            JournalCursor::open(Arc::clone(store), invocation, tick, true, None, None)?;
        if state != Some(ExecutionState::Suspended) {
            return Err(StoreError::InvalidTransition);
        }
        journal.record(
            Some(ExecutionState::Completed),
            LifecycleEventKind::HandoffCompleted,
            None,
            None,
            None,
        )?;
        match store.delete_checkpoint(&invocation.execution_id) {
            Ok(()) | Err(StoreError::NotFound) => Ok(()),
            Err(problem) => Err(problem),
        }
    }

    fn execute_inner<M, F>(
        &self,
        machine: &mut M,
        invocation: &Invocation,
        mut observe: F,
        resumable: bool,
        native: Option<&mut NativeProgress<'_>>,
        prepare_native: Option<&mut dyn FnMut(&mut M) -> Result<(), HostProblem>>,
        child: Option<&root_terminal::NativeChildEnrollment>,
    ) -> ExecutionOutcome
    where
        M: Machine<Effect = EffectRequest, EffectResult = EffectResult>,
        F: FnMut() -> Result<ExecutionControl, ExecutionControlError>,
    {
        if resumable && self.store.is_none() {
            return infrastructure_failure("durable resume requires an execution store");
        }
        let mut control = match observe() {
            Ok(control) => control,
            Err(_) => return infrastructure_failure("execution controls unavailable at admission"),
        };
        let (mut journal, resumed_from) = match self
            .store
            .as_ref()
            .map(|store| {
                JournalCursor::open(
                    Arc::clone(store),
                    invocation,
                    control.now_tick,
                    resumable,
                    native,
                    child,
                )
            })
            .transpose()
        {
            Ok(Some((journal, resumed_from))) => (Some(journal), resumed_from),
            Ok(None) => (None, None),
            Err(_) => return infrastructure_failure("execution admission persistence failed"),
        };
        if let Some(prepare) = prepare_native {
            if !journal.as_ref().is_some_and(|j| j.native.is_some()) || prepare(machine).is_err() {
                return failed_outcome(problem(
                    FailureCategory::UnknownOutcome,
                    "native compiled machine preparation refused",
                ));
            }
        }
        if journal.as_ref().is_some_and(|j| j.native.is_some()) {
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
        if let Err(outcome) = check_control(
            control,
            None,
            invocation,
            invocation.deadline_tick,
            &mut journal,
        ) {
            return outcome;
        }
        let dispatch = match resumed_from {
            None => vec![
                (Some(ExecutionState::Queued), LifecycleEventKind::Queued),
                (Some(ExecutionState::Running), LifecycleEventKind::Started),
            ],
            Some(ExecutionState::Admitted) => vec![
                (Some(ExecutionState::Queued), LifecycleEventKind::Resumed),
                (Some(ExecutionState::Running), LifecycleEventKind::Started),
            ],
            Some(ExecutionState::Queued) => {
                vec![(Some(ExecutionState::Running), LifecycleEventKind::Resumed)]
            }
            Some(ExecutionState::Running) => {
                vec![(None, LifecycleEventKind::Resumed)]
            }
            Some(ExecutionState::Suspended) => vec![
                (Some(ExecutionState::Queued), LifecycleEventKind::Resumed),
                (Some(ExecutionState::Running), LifecycleEventKind::Started),
            ],
            Some(
                ExecutionState::Completing
                | ExecutionState::Completed
                | ExecutionState::Failed
                | ExecutionState::Cancelled
                | ExecutionState::TimedOut
                | ExecutionState::DeadLetter,
            ) => return infrastructure_failure("terminal execution cannot be resumed"),
        };
        for (state, event) in dispatch {
            if record_step(&mut journal, state, event, None, None).is_err() {
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
                    let dispatch = original_dispatch::OriginalDispatch {
                        coordinator: self,
                        invocation,
                        journal: &mut journal,
                        control: &mut control,
                        observe: &mut observe,
                        resumable,
                        native_child: child.is_some(),
                    };
                    match dispatch.dispatch(effect) {
                        Ok(result) => resume = MachineResume::HostResult(result),
                        Err(outcome) => return outcome,
                    }
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
                    if let Some(progress) = journal.as_mut().and_then(|j| j.native.as_deref_mut()) {
                        progress.completed(&completion);
                    }
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
                    if let Some(progress) = journal.as_mut().and_then(|j| j.native.as_deref_mut()) {
                        progress.abended(&abend);
                    }
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
    let terminal = if control.cancellation_requested || invocation.cancellation_requested() {
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

struct JournalCursor<'n, 'h> {
    store: Arc<dyn PlatformStore>,
    execution_id: mainframe_env_execution_api::ExecutionId,
    run_unit_id: mainframe_env_execution_api::RunUnitId,
    attempt: u32,
    tick: u64,
    version: u64,
    sequence: u64,
    native: Option<&'n mut NativeProgress<'h>>,
}

impl<'n, 'h> JournalCursor<'n, 'h> {
    fn open(
        store: Arc<dyn PlatformStore>,
        invocation: &Invocation,
        tick: u64,
        resumable: bool,
        mut native: Option<&'n mut NativeProgress<'h>>,
        child: Option<&root_terminal::NativeChildEnrollment>,
    ) -> Result<(Self, Option<ExecutionState>), StoreError> {
        if let Some(execution) = store.get_execution(&invocation.execution_id)? {
            if !resumable
                || execution.run_unit_id != invocation.run_unit_id
                || execution.principal != *invocation.principal.id()
                || execution.attempt != invocation.attempt
                || (execution.state != ExecutionState::Suspended
                    && (execution.selector != invocation.selector
                        || execution.artifact != invocation.artifact))
            {
                return Err(StoreError::Conflict);
            }
            let events = store.events(&invocation.execution_id, 1, 65_536)?;
            let last = events.last().ok_or(StoreError::IncompatibleVersion)?;
            if last.execution_id != invocation.execution_id
                || last.run_unit_id != invocation.run_unit_id
                || last.attempt != invocation.attempt
                || last.sequence != execution.version
            {
                return Err(StoreError::IncompatibleVersion);
            }
            let state = execution.state;
            return Ok((
                Self {
                    store,
                    execution_id: invocation.execution_id.clone(),
                    run_unit_id: invocation.run_unit_id.clone(),
                    attempt: invocation.attempt,
                    tick,
                    version: execution.version,
                    sequence: last.sequence,
                    native,
                },
                Some(state),
            ));
        }
        let event = LifecycleEvent {
            execution_id: invocation.execution_id.clone(),
            run_unit_id: invocation.run_unit_id.clone(),
            sequence: 1,
            attempt: invocation.attempt,
            tick,
            kind: LifecycleEventKind::Admitted,
        };
        let notification = notification(&event);
        let execution = ExecutionRecord {
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
            terminal_tick: None,
        };
        if let Some(child) = child {
            child.admit(&store, invocation, execution, event, notification)?;
        } else if let Some(progress) = native.as_deref_mut() {
            progress.admit(&store, execution, event, notification)?;
        } else {
            store.admit_execution(execution, event, notification)?;
        }
        Ok((
            Self {
                store,
                execution_id: invocation.execution_id.clone(),
                run_unit_id: invocation.run_unit_id.clone(),
                attempt: invocation.attempt,
                tick,
                version: 1,
                sequence: 1,
                native,
            },
            None,
        ))
    }

    fn record(
        &mut self,
        next_state: Option<ExecutionState>,
        kind: LifecycleEventKind,
        effect: Option<EffectRecord>,
        checkpoint: Option<CheckpointRecord>,
        audit: Option<AuditRecord>,
    ) -> Result<(), StoreError> {
        if self.native.is_some()
            && (checkpoint.is_some()
                || next_state.is_some_and(|s| {
                    s.terminal()
                        || s == ExecutionState::Completing
                        || s == ExecutionState::Suspended
                }))
        {
            let progress = self.native.take().ok_or(StoreError::InvalidTransition)?;
            let result = progress.intercept(self, &kind);
            self.native = Some(progress);
            return result;
        }
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
            audit,
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
        delivered_tick: None,
        version: 1,
    }
}

fn lifecycle_payload(kind: &LifecycleEventKind) -> Vec<u8> {
    mainframe_env_execution_api::lifecycle_notification_payload(kind)
}

fn record_step(
    journal: &mut Option<JournalCursor>,
    next_state: Option<ExecutionState>,
    event: LifecycleEventKind,
    effect: Option<EffectRecord>,
    checkpoint: Option<CheckpointRecord>,
) -> Result<(), StoreError> {
    if let Some(journal) = journal {
        journal.record(next_state, event, effect, checkpoint, None)
    } else {
        Ok(())
    }
}

fn record_audited_step(
    journal: &mut Option<JournalCursor>,
    audit_sink: Option<&dyn AuditSink>,
    next_state: Option<ExecutionState>,
    event: LifecycleEventKind,
    effect: Option<EffectRecord>,
    checkpoint: Option<CheckpointRecord>,
    audit: AuditRecord,
) -> Result<(), StoreError> {
    if let Some(journal) = journal {
        journal.record(next_state, event, effect, checkpoint, Some(audit))
    } else {
        audit_sink
            .ok_or(StoreError::Infrastructure(
                "host execution requires an audit sink".into(),
            ))?
            .record_audit(audit)
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
        ArtifactRef, AuditDecision, AuditRecord, CapabilityId, Completion, ExecutionId,
        IdempotencyKey, InvocationLimits, Principal, PrincipalId, RequestId, ResourceLimits,
        RunUnitId, Selector, ServiceClass, TraceId,
    };
    use mainframe_env_host_api::{
        CapabilityDescriptor, HostLimits, HostProvider, HostRequest, HostResult, RegistrySnapshot,
        StateRequest,
    };
    use std::collections::{BTreeMap, BTreeSet};
    use std::sync::Mutex;

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
            LifecycleEventKind::HandoffCompleted,
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
        assert_eq!(payloads.len(), 17);
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

    #[test]
    fn coordinator_exposes_one_validated_participant_authority() {
        let coordinator = ExecutionCoordinator::local(CoordinatorLimits::default());
        let contract = coordinator.transaction_participant_contract().unwrap();
        assert_eq!(contract.validate(), Ok(()));
        assert_eq!(
            coordinator
                .transaction_participant_descriptor("cics")
                .unwrap()
                .status,
            mainframe_env_execution_api::ParticipantStatus::Accepted
        );
        for provider in ["db2", "ims", "mq"] {
            let descriptor = coordinator
                .transaction_participant_descriptor(provider)
                .unwrap();
            assert_eq!(
                descriptor.status,
                mainframe_env_execution_api::ParticipantStatus::Pending
            );
            assert!(descriptor.capabilities.is_none());
        }
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

    struct FixedProvider {
        descriptor: CapabilityDescriptor,
        outcome: Result<HostResult, HostProblem>,
    }

    impl HostProvider for FixedProvider {
        fn descriptor(&self) -> &CapabilityDescriptor {
            &self.descriptor
        }

        fn invoke(&self, _: &Invocation, request: EffectRequest) -> EffectResult {
            EffectResult {
                sequence: request.sequence,
                outcome: self.outcome.clone(),
            }
        }
    }

    struct OneHostCall(Option<EffectRequest>);

    impl Machine for OneHostCall {
        type Effect = EffectRequest;
        type EffectResult = EffectResult;

        fn drive(
            &mut self,
            resume: MachineResume<Self::EffectResult>,
            _: Quantum,
        ) -> MachineDrive<Self::Effect> {
            match resume {
                MachineResume::Start => MachineDrive::HostCall(self.0.take().unwrap()),
                MachineResume::HostResult(_) => MachineDrive::Completed(Completion {
                    return_code: 0,
                    output: mainframe_env_execution_api::BoundedPayload::new(
                        "test@1",
                        Vec::new(),
                        InvocationLimits::default(),
                    )
                    .unwrap(),
                }),
                other => panic!("unexpected resume: {other:?}"),
            }
        }
    }

    struct RecordingAuditSink {
        capacity: usize,
        records: Mutex<Vec<AuditRecord>>,
    }

    impl RecordingAuditSink {
        fn new(capacity: usize) -> Self {
            Self {
                capacity,
                records: Mutex::new(Vec::new()),
            }
        }
    }

    impl AuditSink for RecordingAuditSink {
        fn record_audit(&self, record: AuditRecord) -> Result<(), StoreError> {
            let mut records = self.records.lock().map_err(|_| StoreError::Poisoned)?;
            if records.len() >= self.capacity {
                return Err(StoreError::CapacityExceeded);
            }
            records.push(record);
            Ok(())
        }

        fn audit_records(
            &self,
            execution_id: &ExecutionId,
            start_effect_sequence: u64,
            max: usize,
        ) -> Result<Vec<AuditRecord>, StoreError> {
            Ok(self
                .records
                .lock()
                .map_err(|_| StoreError::Poisoned)?
                .iter()
                .filter(|record| {
                    &record.execution_id == execution_id
                        && record.effect_sequence >= start_effect_sequence
                })
                .take(max)
                .cloned()
                .collect())
        }
    }

    fn audited_host(outcome: Result<HostResult, HostProblem>) -> Arc<ScopedHostService> {
        let limits = InvocationLimits::default();
        let capability = CapabilityId::new("host.state.read", limits).unwrap();
        let provider: Arc<dyn HostProvider> = Arc::new(FixedProvider {
            descriptor: CapabilityDescriptor {
                capability,
                provider_id: "audit-test".into(),
                generation: "audit-test@1".into(),
                request_schema: "state-request@1".into(),
                result_schema: "state-result@1".into(),
                max_request_bytes: 4096,
                max_result_bytes: 4096,
                ready: true,
            },
            outcome,
        });
        Arc::new(ScopedHostService::new(
            Arc::new(RegistrySnapshot::new(1, vec![provider], limits).unwrap()),
            HostLimits::default(),
        ))
    }

    fn audit_invocation(granted: bool) -> Invocation {
        let limits = InvocationLimits::default();
        let capability = CapabilityId::new("host.state.read", limits).unwrap();
        let mut invocation = invocation();
        invocation.principal = Principal::new(
            PrincipalId::new("USER", limits).unwrap(),
            granted.then_some(capability).into_iter().collect(),
            limits,
        )
        .unwrap();
        invocation
    }

    fn audit_request(invocation: &Invocation) -> EffectRequest {
        EffectRequest {
            run_unit: invocation.run_unit_id.clone(),
            sequence: 1,
            deadline_tick: 100,
            idempotency_key: None,
            request: HostRequest::State(StateRequest::Get { key: "one".into() }),
        }
    }

    #[test]
    fn mandatory_sink_persists_success_deny_cancellation_and_provider_failure() {
        for (granted, outcome, expected) in [
            (
                true,
                Ok(HostResult::State {
                    value: Some(vec![1]),
                    version: 1,
                }),
                AuditDecision::Success,
            ),
            (
                false,
                Ok(HostResult::State {
                    value: Some(vec![1]),
                    version: 1,
                }),
                AuditDecision::Deny,
            ),
            (
                true,
                Err(HostProblem::ProviderFailure),
                AuditDecision::ProviderFailure,
            ),
            (true, Err(HostProblem::Cancelled), AuditDecision::Cancelled),
        ] {
            let invocation = audit_invocation(granted);
            let sink = Arc::new(RecordingAuditSink::new(1));
            let coordinator = ExecutionCoordinator::with_host(
                audited_host(outcome),
                sink.clone(),
                CoordinatorLimits::default(),
            );
            let result = coordinator.execute(
                &mut OneHostCall(Some(audit_request(&invocation))),
                &invocation,
                ExecutionControl::default(),
            );
            assert!(matches!(result, ExecutionOutcome::Completed(_)));
            let records = sink.audit_records(&invocation.execution_id, 1, 8).unwrap();
            assert_eq!(records.len(), 1);
            assert_eq!(records[0].decision, expected);
            assert_eq!(records[0].principal, *invocation.principal.id());
            assert_eq!(records[0].capability.as_str(), "host.state.read");
        }
    }

    #[test]
    fn audit_capacity_saturation_blocks_non_durable_host_result() {
        let invocation = audit_invocation(true);
        let coordinator = ExecutionCoordinator::with_host(
            audited_host(Ok(HostResult::State {
                value: None,
                version: 1,
            })),
            Arc::new(RecordingAuditSink::new(0)),
            CoordinatorLimits::default(),
        );
        assert!(matches!(
            coordinator.execute(
                &mut OneHostCall(Some(audit_request(&invocation))),
                &invocation,
                ExecutionControl::default(),
            ),
            ExecutionOutcome::InfrastructureFailure(_)
        ));
    }
}
