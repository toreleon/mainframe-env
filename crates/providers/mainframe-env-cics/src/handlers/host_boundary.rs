use super::super::*;
use super::handle_state::HandleState;
use mainframe_env_host_api::ProgramRequest;
use std::ops::{Deref, DerefMut};
use std::thread::ThreadId;

#[cfg(test)]
#[path = "host_boundary/lifecycle_tests.rs"]
mod lifecycle_tests;

#[cfg(test)]
#[path = "host_boundary/frame_tests.rs"]
mod tests;

const MAX_PROGRAM_LEVELS: usize = 16;

/// A program occurrence belongs to its durable CICS command and frame actor,
/// never to the volatile task-global host counter. Inputs are checked separately
/// by the installed-call fingerprint, so changed inputs cannot choose a new key.
fn program_effect_key(run: &Run, occurrence: u64) -> Result<IdempotencyKey, HostProblem> {
    let outer = run
        .outer_effect_key
        .as_deref()
        .ok_or(HostProblem::MissingIdempotency)?;
    if occurrence == 0 {
        return Err(HostProblem::Malformed);
    }
    let mut hash = Sha256::new();
    hash.update(b"mainframe-env.cics-program-occurrence@2\0");
    for field in [
        run.invocation.run_unit_id.as_str().as_bytes(),
        run.current_program
            .effect_invocation
            .execution_id
            .as_str()
            .as_bytes(),
        outer.as_bytes(),
    ] {
        hash.update((field.len() as u64).to_be_bytes());
        hash.update(field);
    }
    hash.update(occurrence.to_be_bytes());
    IdempotencyKey::new(
        format!("cics-program-v2:{:x}", hash.finalize()),
        InvocationLimits::default(),
    )
    .map_err(|_| HostProblem::ResourceExhausted)
}

/// Volatile exclusive loans for the current synchronous selected executor.
/// Durable unknown-outcome authority remains the installed-call protocol.
#[derive(Default)]
pub(in crate::service) struct TaskDispatch {
    claims: BTreeMap<RunUnitId, TaskClaim>,
    cleaning_sessions: BTreeSet<String>,
    // Bounded by held runs: a pending installed child cannot become idle success.
    // Cold uncertainty remains owned by core effects and installed-call rows.
    uncertain_sessions: BTreeSet<String>,
}

impl TaskDispatch {
    pub(in crate::service) fn absent_runs(&self, runs: &BTreeMap<RunUnitId, Run>) -> usize {
        self.claims
            .keys()
            .filter(|run| !runs.contains_key(*run))
            .count()
    }
    pub(in crate::service) fn active(&self, run_unit: &RunUnitId) -> bool {
        self.claims.contains_key(run_unit)
    }

    pub(in crate::service) fn require_idle_session(
        &self,
        session: &str,
    ) -> Result<(), HostProblem> {
        self.require_available_session(session)?;
        if self.claims.values().any(|claim| claim.session == session) {
            Err(HostProblem::IdempotencyConflict)
        } else {
            Ok(())
        }
    }

    pub(in crate::service) fn require_available_session(
        &self,
        session: &str,
    ) -> Result<(), HostProblem> {
        if self.uncertain_sessions.contains(session) {
            Err(HostProblem::UnknownOutcome)
        } else if self.cleaning_sessions.contains(session) {
            Err(HostProblem::IdempotencyConflict)
        } else {
            Ok(())
        }
    }
}

/// Keep resource cleanup exclusive while it calls provider stores outside the mutex.
pub(in crate::service) struct SessionCleanupLease<'a> {
    service: &'a CicsService,
    session: String,
    _thread_confined: std::marker::PhantomData<std::rc::Rc<()>>,
}

impl<'a> SessionCleanupLease<'a> {
    pub(in crate::service) fn acquire(
        service: &'a CicsService,
        session: &str,
    ) -> Result<Self, HostProblem> {
        let mut state = service.lock()?;
        Self::acquire_locked(service, &mut state, session)
    }

    pub(in crate::service) fn acquire_locked(
        service: &'a CicsService,
        state: &mut State,
        session: &str,
    ) -> Result<Self, HostProblem> {
        state.task_dispatch.require_idle_session(session)?;
        if !state.sessions.contains_key(session) {
            return Err(HostProblem::NotFound);
        }
        state.task_dispatch.cleaning_sessions.insert(session.into());
        Ok(Self {
            service,
            session: session.into(),
            _thread_confined: std::marker::PhantomData,
        })
    }
}

impl Drop for SessionCleanupLease<'_> {
    fn drop(&mut self) {
        if let Ok(mut state) = self.service.lock() {
            state.task_dispatch.cleaning_sessions.remove(&self.session);
        }
    }
}

struct TaskClaim {
    thread: ThreadId,
    session: String,
    commands: usize,
    loans: Vec<ChildAdmission>,
}

struct ChildAdmission {
    parent: Invocation,
    program: String,
    artifact: Option<mainframe_env_execution_api::ArtifactRef>,
    actor: Option<Invocation>,
    handles: HandleState,
}

fn command_ready(state: &State, run_unit: &RunUnitId) -> Result<(), HostProblem> {
    if let Some(task) = state.runs.get(run_unit) {
        if task.program_abend.is_some() {
            return Err(HostProblem::UnknownOutcome);
        }
        state
            .task_dispatch
            .require_available_session(&task.session)?;
    }
    if let Some(claim) = state.task_dispatch.claims.get(run_unit) {
        if claim.thread != std::thread::current().id()
            || claim.commands != claim.loans.len()
            || claim.loans.last().is_none_or(|loan| loan.actor.is_none())
        {
            return Err(HostProblem::Unauthorized);
        }
    }
    Ok(())
}

pub(super) fn admit_frame(state: &mut State, invocation: &Invocation) -> Result<(), HostProblem> {
    if let Some(task) = state.runs.get(&invocation.run_unit_id) {
        state
            .task_dispatch
            .require_available_session(&task.session)?;
    }
    let Some(claim) = state.task_dispatch.claims.get_mut(&invocation.run_unit_id) else {
        return Ok(());
    };
    if claim.thread != std::thread::current().id() || claim.commands != claim.loans.len() {
        return Err(HostProblem::Unauthorized);
    }
    let loan = claim.loans.last_mut().ok_or(HostProblem::Unauthorized)?;
    let task = state
        .runs
        .get(&invocation.run_unit_id)
        .ok_or(HostProblem::Unauthorized)?;
    if invocation.parent_execution_id.as_ref() != Some(&loan.parent.execution_id)
        || invocation.run_unit_id != loan.parent.run_unit_id
        || invocation.principal != loan.parent.principal
        || invocation.provider_generations != loan.parent.provider_generations
        || invocation.cancellation != loan.parent.cancellation
        || invocation.cancellation_probe != loan.parent.cancellation_probe
        || invocation.deadline_tick > loan.parent.deadline_tick
        || invocation.limits.max_frames > loan.parent.limits.max_frames
        || invocation.limits.max_steps > loan.parent.limits.max_steps
        || invocation.limits.max_storage_bytes > loan.parent.limits.max_storage_bytes
        || invocation.limits.max_output_bytes > loan.parent.limits.max_output_bytes
        || invocation.limits.max_effects > loan.parent.limits.max_effects
        || invocation.limits.max_events > loan.parent.limits.max_events
        || super::task_context::current_program(invocation).as_deref() != Some(&loan.program)
        || loan
            .artifact
            .as_ref()
            .is_some_and(|artifact| *artifact != invocation.artifact)
        || invocation.bindings.get("cics.execution-context")
            != loan.parent.bindings.get("cics.execution-context")
        || invocation
            .bindings
            .get("cics.session")
            .is_some_and(|session| {
                session.schema() != "mainframe-env.cics.session@1"
                    || session.bytes() != task.session.as_bytes()
            })
    {
        return Err(HostProblem::Unauthorized);
    }
    if loan.parent.cancellation_requested() {
        return Err(HostProblem::Cancelled);
    }
    if let Some(actor) = &loan.actor {
        if actor != invocation {
            return Err(HostProblem::IdempotencyConflict);
        }
    } else {
        loan.actor = Some(invocation.clone());
    }
    Ok(())
}

struct CommandLease<'a> {
    service: &'a CicsService,
    task: Option<Run>,
}

impl<'a> CommandLease<'a> {
    fn acquire(service: &'a CicsService, run_unit: &RunUnitId) -> Result<Self, HostProblem> {
        let mut state = service.lock()?;
        command_ready(&state, run_unit)?;
        let task = state
            .runs
            .remove(run_unit)
            .ok_or(HostProblem::Unauthorized)?;
        let claim = state
            .task_dispatch
            .claims
            .entry(run_unit.clone())
            .or_insert_with(|| TaskClaim {
                thread: std::thread::current().id(),
                session: task.session.clone(),
                commands: 0,
                loans: Vec::new(),
            });
        claim.commands += 1;
        Ok(Self {
            service,
            task: Some(task),
        })
    }

    fn restore(&mut self) -> Result<(), HostProblem> {
        let Some(task) = self.task.as_ref() else {
            return Ok(());
        };
        let mut state = self.service.lock()?;
        let run_unit = task.invocation.run_unit_id.clone();
        if state.runs.contains_key(&run_unit) {
            return Err(HostProblem::InfrastructureFailure);
        }
        let claim = state
            .task_dispatch
            .claims
            .get_mut(&run_unit)
            .ok_or(HostProblem::InfrastructureFailure)?;
        if claim.thread != std::thread::current().id() || claim.commands != claim.loans.len() + 1 {
            return Err(HostProblem::InfrastructureFailure);
        }
        claim.commands -= 1;
        if claim.commands == 0 {
            state.task_dispatch.claims.remove(&run_unit);
        }
        state
            .runs
            .insert(run_unit, self.task.take().expect("active command task"));
        Ok(())
    }

    fn finish(mut self) -> Result<(), HostProblem> {
        self.restore()
    }
}

impl Deref for CommandLease<'_> {
    type Target = Run;
    fn deref(&self) -> &Run {
        self.task.as_ref().expect("active command lease")
    }
}

impl DerefMut for CommandLease<'_> {
    fn deref_mut(&mut self) -> &mut Run {
        self.task.as_mut().expect("active command lease")
    }
}

impl Drop for CommandLease<'_> {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}

struct ProgramLease<'a> {
    service: &'a CicsService,
    caller: &'a mut Run,
    frame: Option<super::CurrentProgramFrame>,
    outer_effect_key: Option<String>,
    depth: usize,
}

impl<'a> ProgramLease<'a> {
    fn acquire(
        service: &'a CicsService,
        caller: &'a mut Run,
        program: &str,
        artifact: Option<mainframe_env_execution_api::ArtifactRef>,
    ) -> Result<Self, HostProblem> {
        let mut state = service.lock()?;
        let run_unit = &caller.invocation.run_unit_id;
        if state.runs.contains_key(run_unit) {
            return Err(HostProblem::InfrastructureFailure);
        }
        let claim = state
            .task_dispatch
            .claims
            .get_mut(run_unit)
            .ok_or(HostProblem::Unauthorized)?;
        if claim.thread != std::thread::current().id() || claim.commands != claim.loans.len() + 1 {
            return Err(HostProblem::Unauthorized);
        }
        let depth = claim.loans.len() + 1;
        if depth >= MAX_PROGRAM_LEVELS
            || depth >= caller.current_program.effect_invocation.limits.max_frames as usize
        {
            return Err(HostProblem::ResourceExhausted);
        }
        claim.loans.push(ChildAdmission {
            parent: caller.current_program.effect_invocation.clone(),
            program: program.to_ascii_uppercase(),
            artifact,
            actor: None,
            handles: HandleState::from_run(caller),
        });
        let mut task = caller.clone();
        task.current_program.logical_level += 1;
        task.current_program.invoking_program = caller.current_program.current.clone();
        task.current_program.return_program = caller.current_program.current.clone();
        task.current_program.current = Some(program.to_ascii_uppercase());
        task.current_program.parent_execution_id = Some(
            caller
                .current_program
                .effect_invocation
                .execution_id
                .clone(),
        );
        task.current_program.initial_entry = false;
        HandleState::default().apply(&mut task);
        task.latest_abend = caller.latest_abend.clone();
        state.runs.insert(run_unit.clone(), task);
        Ok(Self {
            service,
            frame: Some(caller.current_program.clone()),
            outer_effect_key: caller.outer_effect_key.clone(),
            caller,
            depth,
        })
    }

    fn restore(&mut self) -> Result<(), HostProblem> {
        if self.frame.is_none() {
            return Ok(());
        }
        let mut state = self.service.lock()?;
        let run_unit = &self.caller.invocation.run_unit_id;
        let claim = state
            .task_dispatch
            .claims
            .get_mut(run_unit)
            .ok_or(HostProblem::InfrastructureFailure)?;
        if claim.thread != std::thread::current().id()
            || claim.loans.len() != self.depth
            || claim.commands != self.depth
        {
            return Err(HostProblem::InfrastructureFailure);
        }
        let mut task = state
            .runs
            .remove(run_unit)
            .ok_or(HostProblem::InfrastructureFailure)?;
        let loan = state
            .task_dispatch
            .claims
            .get_mut(run_unit)
            .expect("validated program claim")
            .loans
            .pop()
            .expect("validated program loan");
        task.current_program = self.frame.take().expect("active program frame");
        let latest_abend = task.latest_abend.clone();
        loan.handles.apply(&mut task);
        task.latest_abend = latest_abend;
        if let Some(abend) = &task.program_abend {
            task.latest_abend = abend.record.clone();
            if abend.cancel_exits {
                task.abend_handler = None;
                task.cancelled_abend_handler = None;
                for frame in &mut task.handle_stack {
                    frame.abend_handler = None;
                    frame.cancelled_abend_handler = None;
                }
            }
        }
        task.outer_effect_key = self.outer_effect_key.take();
        *self.caller = task;
        Ok(())
    }

    fn finish(mut self) -> Result<(), HostProblem> {
        self.restore()
    }
}

impl Drop for ProgramLease<'_> {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}

impl CicsService {
    pub(in crate::service) fn ancestor_abend_exit(&self, run: &Run) -> Result<bool, HostProblem> {
        let state = self.lock()?;
        let Some(claim) = state.task_dispatch.claims.get(&run.invocation.run_unit_id) else {
            return Ok(false);
        };
        if claim.thread != std::thread::current().id() {
            return Err(HostProblem::Unauthorized);
        }
        Ok(claim
            .loans
            .iter()
            .rev()
            .any(|loan| loan.handles.abend_handler.is_some()))
    }

    pub(in crate::service) fn nested(
        &self,
        run: &mut Run,
        request: HostRequest,
    ) -> Result<HostResult, HostProblem> {
        run.host_sequence = run
            .host_sequence
            .checked_add(1)
            .ok_or(HostProblem::ResourceExhausted)?;
        let (sequence, key) =
            if matches!(&request, HostRequest::Program(ProgramRequest::Link { .. })) {
                run.current_program.program_occurrence = run
                    .current_program
                    .program_occurrence
                    .checked_add(1)
                    .filter(|value| {
                        *value
                            <= u64::from(run.current_program.effect_invocation.limits.max_effects)
                    })
                    .ok_or(HostProblem::ResourceExhausted)?;
                let sequence = run.current_program.program_occurrence;
                (sequence, Some(program_effect_key(run, sequence)?))
            } else {
                (
                    run.host_sequence,
                    request
                        .is_mutating()
                        .then(|| nested_key(run, run.host_sequence))
                        .transpose()?,
                )
            };
        let actor = run.current_program.effect_invocation.clone();
        let nested_invocation = key
            .as_ref()
            .filter(|_| {
                matches!(
                    &request,
                    HostRequest::Dataset(_)
                        | HostRequest::Db2(_)
                        | HostRequest::Ims(_)
                        | HostRequest::Mq(_)
                )
            })
            .map(|key| {
                invocation_with_nested_origin(
                    &actor,
                    key,
                    run.outer_effect_key
                        .as_deref()
                        .ok_or(HostProblem::InfrastructureFailure)?,
                )
            })
            .transpose()?;
        let invocation = nested_invocation.as_ref().unwrap_or(&actor);
        let effect = EffectRequest {
            run_unit: actor.run_unit_id.clone(),
            sequence,
            deadline_tick: actor.deadline_tick,
            idempotency_key: key,
            request,
        };
        let previous_handles = HandleState::from_run(run);
        let loan = match &effect.request {
            HostRequest::Program(ProgramRequest::Link {
                program, selection, ..
            }) => Some(ProgramLease::acquire(
                self,
                run,
                program.as_str(),
                selection.as_ref().map(|selected| selected.artifact.clone()),
            )?),
            _ => None,
        };
        let result = self.invoke_host(
            invocation,
            actor.deadline_tick.saturating_sub(1),
            false,
            effect,
        );
        let linked = loan.is_some();
        loan.map(ProgramLease::finish)
            .transpose()
            .map_err(|_| HostProblem::UnknownOutcome)?;
        if linked && matches!(result.outcome, Err(HostProblem::UnknownOutcome)) {
            self.lock()
                .map_err(|_| HostProblem::UnknownOutcome)?
                .task_dispatch
                .uncertain_sessions
                .insert(run.session.clone());
        }
        if linked
            && matches!(result.outcome, Ok(HostResult::Program(_)))
            && run.latest_abend != previous_handles.latest_abend
        {
            super::handle_state::persist_handle_state(self, run, previous_handles)
                .map_err(|_| HostProblem::UnknownOutcome)?;
        }
        result.outcome
    }

    pub(in crate::service) fn invoke_task_command(
        &self,
        effect: &EffectRequest,
        request: CicsRequest,
    ) -> Result<CicsResponse, HostProblem> {
        if !request.operation.supported() {
            return Err(HostProblem::Unsupported);
        }
        let replay_identity = if request.is_mutating() {
            let mutation = request
                .mutation
                .as_ref()
                .ok_or(HostProblem::MissingIdempotency)?;
            if effect.idempotency_key.as_ref() != Some(&mutation.idempotency_key)
                || mutation.sequence != effect.sequence
            {
                return Err(HostProblem::IdempotencyConflict);
            }
            Some((
                mutation.idempotency_key.clone(),
                canonical_request_digest(&HostRequest::Cics(request.clone()))
                    .map_err(|_| HostProblem::ResourceExhausted)?,
            ))
        } else {
            None
        };
        let (replay_owner, replay_run_unit) = {
            let state = self.lock()?;
            command_ready(&state, &effect.run_unit)?;
            let run = state
                .runs
                .get(&effect.run_unit)
                .ok_or(HostProblem::Unauthorized)?;
            (
                run.current_program
                    .effect_invocation
                    .execution_id
                    .as_str()
                    .to_string(),
                run.invocation.run_unit_id.as_str().to_string(),
            )
        };
        if let Some((key, digest)) = replay_identity.as_ref()
            && let Some(record) = self
                .store
                .get_provider_state("cics-effect-replay-v1", key.as_str())
                .map_err(store_error)?
        {
            let replay = decode_cics_effect_replay(&record.payload, self.limits)?;
            validate_cics_effect_replay_identity(
                &replay,
                key.as_str(),
                &replay_owner,
                &replay_run_unit,
                effect.sequence,
                *digest,
            )?;
            let replay = self
                .finalize_effect_replay(record, replay, effect.deadline_tick)
                .map_err(|_| HostProblem::UnknownOutcome)?;
            return handlers::validate_replay_response(&request, replay.response);
        }
        let mut run = CommandLease::acquire(self, &effect.run_unit)?;
        run.outer_effect_key = effect.idempotency_key.as_ref().map(ToString::to_string);
        run.current_program.program_occurrence = 0;
        let operation = request.operation;
        #[cfg(feature = "fault-injection")]
        let after_mutation_file = matches!(
            operation,
            CicsOperation::Write | CicsOperation::Rewrite | CicsOperation::Delete
        )
        .then(|| argument_text(&request, "FILE").or_else(|_| argument_text(&request, "DATASET")))
        .transpose()?
        .map(|name| name.trim().to_ascii_uppercase());
        let retention_tick = effect.deadline_tick.max(run.invocation.deadline_tick);
        let result = self.invoke_run(&mut run, request, retention_tick);
        // A suspended conversation has no completed reply for outer replay.
        // Its provider receipt is committed with the source-visible event.
        // Reissue then observes that receipt instead of consuming twice.
        let result = match (&result, replay_identity.as_ref()) {
            // A waiting CONVERSE has no result to replay until its peer frame arrives.
            (Ok(response), Some(_)) if handlers::deferred_converse(operation, response) => result,
            (Ok(response), Some((key, digest))) => {
                let result_digest = handlers::cics_result_digest(response)?;
                let mut replay = CicsEffectReplay {
                    effect_key: Some(key.as_str().into()),
                    owner_execution: Some(replay_owner.clone()),
                    owner_run_unit: Some(replay_run_unit.clone()),
                    sequence: Some(effect.sequence),
                    deadline_tick: Some(retention_tick),
                    resolution_tick: None,
                    request_digest: *digest,
                    result_digest: Some(result_digest),
                    binding_digest: None,
                    response: response.clone(),
                };
                replay.binding_digest = Some(cics_effect_replay_binding_digest(&replay));
                let payload = encode_cics_effect_replay(&replay)?;
                match self.store.put_provider_state(
                    ProviderStateRecord {
                        namespace: "cics-effect-replay-v1".into(),
                        key: key.as_str().into(),
                        version: 1,
                        payload,
                    },
                    None,
                ) {
                    Ok(()) => {
                        if self
                            .replay_unknown_after_persist
                            .swap(false, Ordering::SeqCst)
                        {
                            Err(HostProblem::UnknownOutcome)
                        } else {
                            self.finalize_effect_replay(
                                ProviderStateRecord {
                                    namespace: "cics-effect-replay-v1".into(),
                                    key: key.as_str().into(),
                                    version: 1,
                                    payload: encode_cics_effect_replay(&replay)?,
                                },
                                replay,
                                retention_tick,
                            )
                            .map(|_| result)
                            .unwrap_or(Err(HostProblem::UnknownOutcome))
                        }
                    }
                    Err(StoreError::AlreadyExists | StoreError::Conflict) => {
                        match self
                            .store
                            .get_provider_state("cics-effect-replay-v1", key.as_str())
                            .map_err(store_error)?
                        {
                            Some(record) => {
                                let replay =
                                    decode_cics_effect_replay(&record.payload, self.limits)?;
                                validate_cics_effect_replay_identity(
                                    &replay,
                                    key.as_str(),
                                    &replay_owner,
                                    &replay_run_unit,
                                    effect.sequence,
                                    *digest,
                                )?;
                                if replay.response != *response {
                                    Err(HostProblem::IdempotencyConflict)
                                } else {
                                    self.finalize_effect_replay(record, replay, retention_tick)
                                        .map(|_| result)
                                        .unwrap_or(Err(HostProblem::UnknownOutcome))
                                }
                            }
                            _ => Err(HostProblem::IdempotencyConflict),
                        }
                    }
                    Err(_) => Err(HostProblem::UnknownOutcome),
                }
            }
            _ => result,
        };
        #[cfg(feature = "fault-injection")]
        let mutation_fault = result.is_ok() && self.consume_mutation_fault(operation)?;
        #[cfg(feature = "fault-injection")]
        let file_fault = if result.is_ok() {
            match after_mutation_file {
                Some(file) => {
                    self.consume_file_fault(operation, &file, CicsFileFaultPoint::AfterMutation)?
                }
                None => false,
            }
        } else {
            false
        };
        #[cfg(feature = "fault-injection")]
        let result = if mutation_fault || file_fault {
            Err(HostProblem::UnknownOutcome)
        } else {
            result
        };
        if run.trace.len() < 4096 {
            run.trace.push(match &result {
                Ok(response) => CicsTraceEntry {
                    operation,
                    outcome: response.condition.clone(),
                    response: response.response,
                    response2: response.response2,
                    payload_bytes: response.payload.bytes().len(),
                },
                Err(problem) => CicsTraceEntry {
                    operation,
                    outcome: format!("{problem:?}"),
                    response: -1,
                    response2: 0,
                    payload_bytes: 0,
                },
            });
        }
        run.finish()?;
        result
    }
}
