use super::super::*;
use super::handle_state::HandleState;
use mainframe_env_host_api::ProgramRequest;
use std::ops::{Deref, DerefMut};
use std::thread::ThreadId;

#[cfg(test)]
#[path = "host_boundary/lifecycle_tests.rs"]
mod lifecycle_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use mainframe_env_execution_api::{ArtifactRef, Principal, Selector};
    use mainframe_env_store::MemoryStore;

    fn fixture() -> (Arc<CicsService>, Invocation) {
        let service = crate::service::tests::service(Arc::new(MemoryStore::new(
            mainframe_env_store::StoreLimits::default(),
        )));
        let root = crate::service::tests::invocation();
        let session = SessionId::new("FRAME-SESSION", 64).unwrap();
        service
            .launch_terminal(
                root.clone(),
                &session,
                "MENU",
                24,
                80,
                "frame-csrf",
                1,
                10_000,
            )
            .unwrap();
        (service, root)
    }

    fn child(parent: &Invocation, ordinal: usize) -> Invocation {
        let mut child = parent.clone();
        child.parent_execution_id = Some(parent.execution_id.clone());
        child.execution_id =
            ExecutionId::new(format!("child-{ordinal}"), InvocationLimits::default()).unwrap();
        child.selector = Selector::new("program:CHILD", InvocationLimits::default()).unwrap();
        child.artifact = ArtifactRef::new("child-artifact", InvocationLimits::default()).unwrap();
        child
    }

    #[test]
    fn logical_frame_restores_caller_but_keeps_shared_task_updates() {
        let (service, root) = fixture();
        let mut command = CommandLease::acquire(&service, &root.run_unit_id).unwrap();
        command.handlers.insert("ERROR".into(), "CALLER".into());
        let actor = child(&root, 1);
        let loan = ProgramLease::acquire(
            &service,
            &mut command,
            "CHILD",
            Some(actor.artifact.clone()),
        )
        .unwrap();
        service.ensure_run(&actor).unwrap();
        {
            let mut child_command = CommandLease::acquire(&service, &root.run_unit_id).unwrap();
            assert!(child_command.handlers.is_empty());
            assert!(child_command.handle_stack.is_empty());
            assert_eq!(child_command.invocation.execution_id, root.execution_id);
            assert_eq!(
                child_command.current_program.effect_invocation.execution_id,
                actor.execution_id
            );
            let previous = HandleState::from_run(&child_command);
            child_command
                .handlers
                .insert("ERROR".into(), "CHILD".into());
            child_command
                .current_records
                .insert("DATA".into(), b"child-update".to_vec());
            child_command.undo.push(DatasetUndo::Delete {
                dataset: DatasetName::new("USER.DATA", 44).unwrap(),
                key: b"KEY".to_vec(),
            });
            let before = service
                .store
                .get_provider_state("cics-session", "FRAME-SESSION")
                .unwrap();
            super::super::handle_state::persist_handle_state(
                &service,
                &mut child_command,
                previous,
            )
            .unwrap();
            assert_eq!(
                service
                    .store
                    .get_provider_state("cics-session", "FRAME-SESSION")
                    .unwrap(),
                before
            );
            // Drop restores the task even on an early command error path.
        }
        loan.finish().unwrap();
        assert_eq!(command.handlers["ERROR"], "CALLER");
        assert_eq!(command.current_records["DATA"], b"child-update");
        assert_eq!(command.undo.len(), 1);
        assert_eq!(
            command.current_program.effect_invocation.execution_id,
            root.execution_id
        );
        assert_eq!(command.current_program.logical_level, 1);
        command.finish().unwrap();
        let state = service.lock().unwrap();
        assert_eq!(state.runs.len(), 1);
        assert!(state.task_dispatch.claims.is_empty());
    }

    #[test]
    fn logical_frame_admission_rejects_foreign_and_widened_children() {
        let (service, root) = fixture();
        let mut command = CommandLease::acquire(&service, &root.run_unit_id).unwrap();
        let valid = child(&root, 1);
        let loan = ProgramLease::acquire(
            &service,
            &mut command,
            "CHILD",
            Some(valid.artifact.clone()),
        )
        .unwrap();
        for case in 0..10 {
            let mut invalid = valid.clone();
            let limits = InvocationLimits::default();
            match case {
                0 => invalid.parent_execution_id = None,
                1 => {
                    invalid.principal = Principal::new(
                        PrincipalId::new("OTHER", limits).unwrap(),
                        root.principal.grants().clone(),
                        limits,
                    )
                    .unwrap()
                }
                2 => invalid.deadline_tick += 1,
                3 => invalid.limits.max_frames += 1,
                4 => invalid.selector = Selector::new("program:OTHER", limits).unwrap(),
                5 => invalid.artifact = ArtifactRef::new("foreign-artifact", limits).unwrap(),
                6 => {
                    invalid.provider_generations.insert(
                        CapabilityId::new("host.cics.execute", limits).unwrap(),
                        "foreign-generation".into(),
                    );
                }
                7 => {
                    invalid.bindings.insert(
                        "cics.session".into(),
                        BoundedPayload::new(
                            "mainframe-env.cics.session@1",
                            b"other-session".to_vec(),
                            limits,
                        )
                        .unwrap(),
                    );
                }
                8 => {
                    invalid.cancellation_probe =
                        Some(mainframe_env_execution_api::CancellationProbe::new());
                }
                9 => invalid.limits.max_effects += 1,
                _ => unreachable!(),
            }
            assert_eq!(
                service.ensure_run(&invalid),
                Err(HostProblem::Unauthorized),
                "case {case}"
            );
        }
        service.ensure_run(&valid).unwrap();
        let mut conflict = valid.clone();
        conflict.execution_id =
            ExecutionId::new("different-child", InvocationLimits::default()).unwrap();
        assert_eq!(
            service.ensure_run(&conflict),
            Err(HostProblem::IdempotencyConflict)
        );
        loan.finish().unwrap();
        command.finish().unwrap();
        assert_eq!(service.lock().unwrap().sessions.len(), 1);
    }

    #[test]
    fn logical_frame_fences_other_threads_and_duplicate_registration() {
        let (service, root) = fixture();
        let mut command = CommandLease::acquire(&service, &root.run_unit_id).unwrap();
        let valid = child(&root, 1);
        let loan = ProgramLease::acquire(
            &service,
            &mut command,
            "CHILD",
            Some(valid.artifact.clone()),
        )
        .unwrap();
        let foreign_service = service.clone();
        let foreign_actor = valid.clone();
        assert_eq!(
            std::thread::spawn(move || foreign_service.ensure_run(&foreign_actor))
                .join()
                .unwrap(),
            Err(HostProblem::Unauthorized)
        );
        let session = SessionId::new("FRAME-SESSION", 64).unwrap();
        let before = service
            .store
            .get_provider_state("cics-session", session.as_str())
            .unwrap();
        assert_eq!(
            service.suspend_terminal_run(&session, root.principal.id(), 2),
            Err(HostProblem::IdempotencyConflict)
        );
        assert_eq!(
            service.discard_terminal_run_if_present(&session, root.principal.id(), 2),
            Err(HostProblem::IdempotencyConflict)
        );
        assert_eq!(
            service.complete_terminal_run(&session, root.principal.id(), 2),
            Err(HostProblem::IdempotencyConflict)
        );
        assert_eq!(
            service.restore_terminal_run(root.clone(), &session, "MENU", Vec::new(), 2),
            Err(HostProblem::IdempotencyConflict)
        );
        assert_eq!(
            service.disconnect_terminal(&session, root.principal.id(), "frame-csrf", 2),
            Err(HostProblem::IdempotencyConflict)
        );
        assert_eq!(
            service
                .store
                .get_provider_state("cics-session", session.as_str())
                .unwrap(),
            before
        );
        assert_eq!(
            service.register_run(root.clone(), &session, "MENU", "ME01", "S001"),
            Err(HostProblem::IdempotencyConflict)
        );
        service.ensure_run(&valid).unwrap();
        drop(loan);
        drop(command);
        let state = service.lock().unwrap();
        assert_eq!(state.sessions.len(), 1);
        assert_eq!(state.runs.len(), 1);
        assert!(state.task_dispatch.claims.is_empty());
    }

    fn descend(service: &CicsService, task: &mut Run, level: usize) {
        let actor = child(&task.current_program.effect_invocation, level);
        if level == MAX_PROGRAM_LEVELS {
            assert!(matches!(
                ProgramLease::acquire(service, task, "CHILD", Some(actor.artifact.clone())),
                Err(HostProblem::ResourceExhausted)
            ));
            return;
        }
        let loan =
            ProgramLease::acquire(service, task, "CHILD", Some(actor.artifact.clone())).unwrap();
        service.ensure_run(&actor).unwrap();
        let mut command = CommandLease::acquire(service, &actor.run_unit_id).unwrap();
        assert_eq!(command.current_program.logical_level, (level + 1) as u32);
        descend(service, &mut command, level + 1);
        command.finish().unwrap();
        loan.finish().unwrap();
    }

    #[test]
    fn logical_frame_held_task_still_counts_against_run_capacity() {
        let (mut service, root) = fixture();
        Arc::get_mut(&mut service).unwrap().limits.max_runs = 1;
        let command = CommandLease::acquire(&service, &root.run_unit_id).unwrap();
        let session = SessionId::new("second-session", 64).unwrap();
        service.create_session(&session, 24, 80).unwrap();
        let mut second = root.clone();
        second.run_unit_id = RunUnitId::new("second-run", InvocationLimits::default()).unwrap();
        assert_eq!(
            service.register_run(second, &session, "MENU", "ME01", "S001"),
            Err(HostProblem::ResourceExhausted)
        );
        command.finish().unwrap();
        assert_eq!(service.lock().unwrap().runs.len(), 1);
    }

    #[test]
    fn logical_frame_recursion_is_bounded_and_restores_every_lease() {
        let (service, root) = fixture();
        let mut command = CommandLease::acquire(&service, &root.run_unit_id).unwrap();
        descend(&service, &mut command, 1);
        command.finish().unwrap();
        let state = service.lock().unwrap();
        assert_eq!(state.runs.len(), 1);
        assert_eq!(
            state.runs[&root.run_unit_id].current_program.logical_level,
            1
        );
        assert!(state.task_dispatch.claims.is_empty());
    }

    #[test]
    fn logical_frame_live_cancellation_restores_caller_without_session_mutation() {
        let (service, root) = fixture();
        let mut command = CommandLease::acquire(&service, &root.run_unit_id).unwrap();
        let probe = mainframe_env_execution_api::CancellationProbe::new();
        command.current_program.effect_invocation.cancellation_probe = Some(probe.clone());
        command.handlers.insert("ERROR".into(), "CALLER".into());
        let mut actor = child(&command.current_program.effect_invocation, 1);
        actor.deadline_tick -= 1;
        let before = service
            .store
            .get_provider_state("cics-session", "FRAME-SESSION")
            .unwrap();
        {
            let _loan = ProgramLease::acquire(
                &service,
                &mut command,
                "CHILD",
                Some(actor.artifact.clone()),
            )
            .unwrap();
            service.ensure_run(&actor).unwrap();
            probe.request();
            assert_eq!(service.ensure_run(&actor), Err(HostProblem::Cancelled));
        }
        assert_eq!(command.handlers["ERROR"], "CALLER");
        assert_eq!(command.current_program.logical_level, 1);
        assert_eq!(
            service
                .store
                .get_provider_state("cics-session", "FRAME-SESSION")
                .unwrap(),
            before
        );
        command.finish().unwrap();
        let state = service.lock().unwrap();
        assert_eq!(state.runs.len(), 1);
        assert!(state.task_dispatch.claims.is_empty());
    }

    #[test]
    fn logical_frame_narrowed_budget_bounds_further_reentry() {
        let (service, root) = fixture();
        let mut command = CommandLease::acquire(&service, &root.run_unit_id).unwrap();
        let mut actor = child(&root, 1);
        actor.limits.max_frames = 2;
        let loan = ProgramLease::acquire(
            &service,
            &mut command,
            "CHILD",
            Some(actor.artifact.clone()),
        )
        .unwrap();
        service.ensure_run(&actor).unwrap();
        let mut child_command = CommandLease::acquire(&service, &root.run_unit_id).unwrap();
        let descendant = child(&actor, 2);
        assert!(matches!(
            ProgramLease::acquire(
                &service,
                &mut child_command,
                "CHILD",
                Some(descendant.artifact)
            ),
            Err(HostProblem::ResourceExhausted)
        ));
        child_command.finish().unwrap();
        loan.finish().unwrap();
        command.finish().unwrap();
        assert!(service.lock().unwrap().task_dispatch.claims.is_empty());
    }

    #[test]
    fn logical_frame_bare_return_preserves_claimed_task_continuation() {
        let (service, root) = fixture();
        let continuation = DurableContinuation {
            transaction: "NEXT".into(),
            commarea: b"caller-state".to_vec(),
            claimed_by: Some(root.run_unit_id.as_str().into()),
            effect_key: "earlier-return".into(),
            version: 1,
        };
        service
            .persist_continuation("FRAME-SESSION", &continuation, None)
            .unwrap();
        service
            .lock()
            .unwrap()
            .continuations
            .insert("FRAME-SESSION".into(), continuation);
        let before = service
            .store
            .get_provider_state("cics-continuation", "FRAME-SESSION")
            .unwrap();
        let mut caller = CommandLease::acquire(&service, &root.run_unit_id).unwrap();
        let actor = child(&root, 1);
        let loan =
            ProgramLease::acquire(&service, &mut caller, "CHILD", Some(actor.artifact.clone()))
                .unwrap();
        service.ensure_run(&actor).unwrap();
        let child_command = CommandLease::acquire(&service, &root.run_unit_id).unwrap();
        assert_eq!(child_command.session, "FRAME-SESSION");
        assert_eq!(
            service.lock().unwrap().continuations["FRAME-SESSION"]
                .claimed_by
                .as_deref(),
            Some(child_command.invocation.run_unit_id.as_str())
        );
        let request = CicsRequest {
            operation: CicsOperation::Return,
            arguments: BTreeMap::new(),
            condition_policy: mainframe_env_host_api::CicsConditionPolicy::Default,
            mutation: Some(Mutation {
                sequence: 1,
                idempotency_key: IdempotencyKey::new("child-return", InvocationLimits::default())
                    .unwrap(),
                transaction: Some("MENU".into()),
            }),
        };
        let response =
            super::super::task_return::invoke(&service, &child_command, &request).unwrap();
        assert_eq!(response.disposition, CicsDisposition::Returned);
        assert_eq!(
            service
                .store
                .get_provider_state("cics-continuation", "FRAME-SESSION")
                .unwrap(),
            before
        );
        assert!(
            service
                .lock()
                .unwrap()
                .continuations
                .contains_key("FRAME-SESSION")
        );
        child_command.finish().unwrap();
        loan.finish().unwrap();
        // Root RETURN, unlike child RETURN, retires the claimed task continuation.
        assert_eq!(caller.session, "FRAME-SESSION");
        super::super::task_return::invoke(&service, &caller, &request).unwrap();
        assert_eq!(
            service
                .store
                .get_provider_state("cics-continuation", "FRAME-SESSION")
                .unwrap(),
            None
        );
        caller.finish().unwrap();
    }
}

const MAX_PROGRAM_LEVELS: usize = 16;

/// Volatile exclusive loans for the current synchronous selected executor.
/// Durable unknown-outcome authority remains the installed-call protocol.
#[derive(Default)]
pub(in crate::service) struct TaskDispatch {
    claims: BTreeMap<RunUnitId, TaskClaim>,
    cleaning_sessions: BTreeSet<String>,
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
        if self.cleaning_sessions.contains(session) {
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
}

fn command_ready(state: &State, run_unit: &RunUnitId) -> Result<(), HostProblem> {
    if let Some(task) = state.runs.get(run_unit) {
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
    handles: Option<HandleState>,
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
        state.runs.insert(run_unit.clone(), task);
        Ok(Self {
            service,
            frame: Some(caller.current_program.clone()),
            handles: Some(HandleState::from_run(caller)),
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
        state
            .task_dispatch
            .claims
            .get_mut(run_unit)
            .expect("validated program claim")
            .loans
            .pop();
        task.current_program = self.frame.take().expect("active program frame");
        self.handles
            .take()
            .expect("active program handles")
            .apply(&mut task);
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
    pub(in crate::service) fn nested(
        &self,
        run: &mut Run,
        request: HostRequest,
    ) -> Result<HostResult, HostProblem> {
        run.host_sequence = run
            .host_sequence
            .checked_add(1)
            .ok_or(HostProblem::ResourceExhausted)?;
        let key = request
            .is_mutating()
            .then(|| nested_key(run, run.host_sequence))
            .transpose()?;
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
            sequence: run.host_sequence,
            deadline_tick: actor.deadline_tick,
            idempotency_key: key,
            request,
        };
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
        if let Some(loan) = loan {
            loan.finish().map_err(|_| HostProblem::UnknownOutcome)?;
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
