use super::super::{CicsService, CicsTraceEntry, handlers};
use mainframe_env_execution_api::{Invocation, InvocationLimits, PrincipalId, RunUnitId};
use mainframe_env_host_api::{CicsUnitOfWorkOutcome, HostProblem, SessionId};
use sha2::{Digest, Sha256};

pub(in crate::service) fn terminal_secret_digest(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

impl CicsService {
    /// Restore an online program actor while retaining its original CICS task owner.
    ///
    /// The embedding must attest the task/actor handoff through its durable
    /// execution authority before calling. The provider checks matching run,
    /// principal, grants, generations and non-widening controls; it never infers
    /// ownership from acquisition/UOW rows. Warm task resources remain shared;
    /// cold restoration reloads only existing durable HANDLE/undo authorities.
    /// Live command loans, conflicting ownership, stale sessions or capacity
    /// return an error before replacing any volatile run.
    #[allow(clippy::too_many_arguments)]
    pub fn restore_terminal_program_run(
        &self,
        task: Invocation,
        actor: Invocation,
        session: &SessionId,
        transaction: &str,
        retrieve: Vec<u8>,
        now_tick: u64,
    ) -> Result<(), HostProblem> {
        super::super::reject_reserved_nested_origin(&task)?;
        super::super::reject_reserved_nested_origin(&actor)?;
        super::super::validate_terminal_identity(transaction, 16)?;
        validate_restore_scope(&task, &actor)?;
        if retrieve.len() > self.limits.max_screen_bytes {
            return Err(HostProblem::ResourceExhausted);
        }
        let current = self.public_session(session, actor.principal.id(), None, now_tick)?;
        if current.run_unit != actor.run_unit_id.as_str()
            || current.transaction != transaction.to_ascii_uppercase()
        {
            return Err(HostProblem::IdempotencyConflict);
        }
        let (undo, undo_version) = self.load_undo(&actor.run_unit_id)?;
        let mut state = self.lock()?;
        state.task_dispatch.require_idle_session(session.as_str())?;
        if state
            .sessions
            .get(session.as_str())
            .is_none_or(|saved| saved.version != current.version)
        {
            return Err(HostProblem::IdempotencyConflict);
        }
        let existing = state.runs.get(&actor.run_unit_id);
        if existing.is_some_and(|run| {
            run.session != session.as_str()
                || run.invocation.execution_id != task.execution_id
                || run.invocation.attempt != task.attempt
                || run.invocation.principal != task.principal
                || run.invocation.provider_generations != task.provider_generations
        }) {
            return Err(HostProblem::IdempotencyConflict);
        }
        let other_runs = state
            .runs
            .values()
            .filter(|run| run.session != session.as_str())
            .count();
        if other_runs + state.task_dispatch.absent_runs(&state.runs) >= self.limits.max_runs {
            return Err(HostProblem::ResourceExhausted);
        }
        let channel = if actor.bindings.contains_key("cics.channel") {
            super::task_context::current_channel(&actor)
        } else {
            existing.and_then(|run| run.current_program.channel.clone())
        };
        let mut restored = match existing {
            Some(run) => {
                let mut restored = run.clone();
                restored.invocation = task;
                restored.retrieve = retrieve;
                restored.undo = undo;
                restored.undo_version = undo_version;
                current.handle_state.clone().apply(&mut restored);
                // Trace is collected by the embedding before transfer/restoration.
                restored.trace.clear();
                restored.host_sequence = 0;
                restored.outer_effect_key = None;
                restored.program_abend = None;
                restored
            }
            None => handlers::new_run_with_state(
                task,
                session.as_str(),
                transaction,
                "ME01",
                "S001",
                handlers::RunSeed {
                    originating_task: current.run_unit,
                    retrieve,
                    undo,
                    undo_version,
                    handle_state: current.handle_state,
                },
            ),
        };
        // Replacement is at the same root logical level, not a fabricated LINK.
        restored.current_program = handlers::CurrentProgramFrame {
            current: super::task_context::current_program(&actor),
            channel,
            parent_execution_id: actor.parent_execution_id.clone(),
            effect_invocation: actor,
            program_occurrence: 0,
            logical_level: 1,
            invoking_program: None,
            return_program: None,
            initial_entry: false,
        };
        state.runs.retain(|_, run| run.session != session.as_str());
        state
            .runs
            .insert(restored.invocation.run_unit_id.clone(), restored);
        Ok(())
    }

    pub fn complete_terminal_run(
        &self,
        session: &SessionId,
        principal: &PrincipalId,
        now_tick: u64,
    ) -> Result<(), HostProblem> {
        self.finish_terminal_run(
            session,
            principal,
            now_tick,
            true,
            Some(CicsUnitOfWorkOutcome::Committed),
        )
    }

    /// Back out task-local state after a known abnormal terminal outcome.
    pub fn abort_terminal_run(
        &self,
        session: &SessionId,
        principal: &PrincipalId,
        now_tick: u64,
    ) -> Result<(), HostProblem> {
        self.finish_terminal_run(
            session,
            principal,
            now_tick,
            true,
            Some(CicsUnitOfWorkOutcome::RolledBack),
        )
    }

    /// Remove a handed-off volatile run while retaining its durable HANDLE state.
    pub fn suspend_terminal_run(
        &self,
        session: &SessionId,
        principal: &PrincipalId,
        now_tick: u64,
    ) -> Result<(), HostProblem> {
        self.finish_terminal_run(session, principal, now_tick, false, None)
    }

    /// Discard a terminal run and its HANDLE state after a terminal outcome.
    pub fn discard_terminal_run_if_present(
        &self,
        session: &SessionId,
        principal: &PrincipalId,
        now_tick: u64,
    ) -> Result<Vec<CicsTraceEntry>, HostProblem> {
        self.discard_terminal_run(session, principal, now_tick, true)
    }

    /// Discard only the volatile run after a completed terminal handoff.
    pub fn discard_handed_off_terminal_run_if_present(
        &self,
        session: &SessionId,
        principal: &PrincipalId,
        now_tick: u64,
    ) -> Result<Vec<CicsTraceEntry>, HostProblem> {
        self.discard_terminal_run(session, principal, now_tick, false)
    }

    fn discard_terminal_run(
        &self,
        session: &SessionId,
        principal: &PrincipalId,
        now_tick: u64,
        clear_handle_state: bool,
    ) -> Result<Vec<CicsTraceEntry>, HostProblem> {
        let current = self.public_session(session, principal, None, now_tick)?;
        let _cleanup = handlers::SessionCleanupLease::acquire(self, session.as_str())?;
        let run_id = RunUnitId::new(&current.run_unit, InvocationLimits::default())
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        let mut state = self.lock()?;
        if state
            .sessions
            .get(session.as_str())
            .is_none_or(|value| value.version != current.version)
        {
            return Err(HostProblem::IdempotencyConflict);
        }
        let trace = match state.runs.get(&run_id) {
            Some(run)
                if run.session == session.as_str()
                    && run.invocation.principal.id() == principal =>
            {
                run.trace.clone()
            }
            Some(_) => return Err(HostProblem::Unauthorized),
            None => Vec::new(),
        };
        if clear_handle_state && current.handle_state != handlers::HandleState::default() {
            let mut discarded = current.clone();
            discarded.version = discarded
                .version
                .checked_add(1)
                .ok_or(HostProblem::ResourceExhausted)?;
            discarded.handle_state = handlers::HandleState::default();
            self.persist_session(session.as_str(), &discarded, Some(current.version))?;
            state.sessions.insert(session.as_str().into(), discarded);
        }
        if let Some(current) = state.continuations.get(session.as_str()).cloned()
            && current.claimed_by.as_deref() == Some(run_id.as_str())
        {
            let mut released = current.clone();
            released.version = released
                .version
                .checked_add(1)
                .ok_or(HostProblem::ResourceExhausted)?;
            released.claimed_by = None;
            self.persist_continuation(session.as_str(), &released, Some(current.version))?;
            state
                .continuations
                .insert(session.as_str().into(), released);
        }
        let mut run = state.runs.get(&run_id).cloned();
        drop(state);
        if let Some(run) = &mut run {
            handlers::release_terminal_task_state(self, run)?;
        }
        let mut state = self.lock()?;
        if state.runs.get(&run_id).is_some_and(|current| {
            run.as_ref().is_none_or(|run| {
                current.session != run.session
                    || current.invocation.principal.id() != run.invocation.principal.id()
            })
        }) {
            return Err(HostProblem::IdempotencyConflict);
        }
        state.runs.remove(&run_id);
        Ok(trace)
    }

    fn finish_terminal_run(
        &self,
        session: &SessionId,
        principal: &PrincipalId,
        now_tick: u64,
        clear_handle_state: bool,
        outcome: Option<CicsUnitOfWorkOutcome>,
    ) -> Result<(), HostProblem> {
        let current = self.public_session(session, principal, None, now_tick)?;
        let _cleanup = handlers::SessionCleanupLease::acquire(self, session.as_str())?;
        let run_id = RunUnitId::new(&current.run_unit, InvocationLimits::default())
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        let mut run = {
            let state = self.lock()?;
            if state
                .sessions
                .get(session.as_str())
                .is_none_or(|value| value.version != current.version)
            {
                return Err(HostProblem::IdempotencyConflict);
            }
            state
                .runs
                .get(&run_id)
                .cloned()
                .ok_or(HostProblem::NotFound)?
        };
        if run.session != session.as_str() || run.invocation.principal.id() != principal {
            return Err(HostProblem::Unauthorized);
        }
        if let Some(outcome) = outcome {
            super::interval_control::finish_protected_starts(self, &run, outcome)?;
            super::issue_device::finish_task(self, &run, outcome)?;
        }
        let mut state = self.lock()?;
        if state.runs.get(&run_id).is_none_or(|current| {
            current.session != run.session
                || current.invocation.principal.id() != run.invocation.principal.id()
        }) {
            return Err(HostProblem::IdempotencyConflict);
        }
        if clear_handle_state && current.handle_state != handlers::HandleState::default() {
            let mut completed = current.clone();
            completed.version = completed
                .version
                .checked_add(1)
                .ok_or(HostProblem::ResourceExhausted)?;
            completed.handle_state = handlers::HandleState::default();
            self.persist_session(session.as_str(), &completed, Some(current.version))?;
            state.sessions.insert(session.as_str().into(), completed);
        }
        if let Some(current) = state.continuations.get(session.as_str()).cloned()
            && current.claimed_by.as_deref() == Some(run_id.as_str())
        {
            let mut released = current.clone();
            released.version = released
                .version
                .checked_add(1)
                .ok_or(HostProblem::ResourceExhausted)?;
            released.claimed_by = None;
            self.persist_continuation(session.as_str(), &released, Some(current.version))?;
            state
                .continuations
                .insert(session.as_str().into(), released);
        }
        drop(state);
        handlers::release_terminal_task_state(self, &mut run)?;
        let mut state = self.lock()?;
        if state.runs.get(&run_id).is_none_or(|current| {
            current.session != run.session
                || current.invocation.principal.id() != run.invocation.principal.id()
        }) {
            return Err(HostProblem::IdempotencyConflict);
        }
        state.runs.remove(&run_id);
        Ok(())
    }
}

fn validate_restore_scope(task: &Invocation, actor: &Invocation) -> Result<(), HostProblem> {
    if task.run_unit_id != actor.run_unit_id
        || task.attempt != actor.attempt
        || task.principal != actor.principal
        || task.provider_generations != actor.provider_generations
        || task.cancellation != actor.cancellation
        || task.cancellation_probe != actor.cancellation_probe
        || actor.deadline_tick > task.deadline_tick
        || actor.limits.max_frames > task.limits.max_frames
        || actor.limits.max_steps > task.limits.max_steps
        || actor.limits.max_storage_bytes > task.limits.max_storage_bytes
        || actor.limits.max_output_bytes > task.limits.max_output_bytes
        || actor.limits.max_effects > task.limits.max_effects
        || actor.limits.max_events > task.limits.max_events
        || actor.bindings.get("cics.execution-context")
            != task.bindings.get("cics.execution-context")
        || actor.bindings.get("cics.syncpoint.remote-outcome")
            != task.bindings.get("cics.syncpoint.remote-outcome")
    {
        return Err(HostProblem::Unauthorized);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use mainframe_env_execution_api::{
        ArtifactRef, CapabilityId, ExecutionId, Principal, Selector,
    };
    use mainframe_env_store::MemoryStore;
    use std::{collections::BTreeSet, sync::Arc};

    fn fixture(max_runs: usize) -> (Arc<CicsService>, Invocation, SessionId) {
        let mut service =
            crate::service::tests::service(Arc::new(MemoryStore::new(Default::default())));
        Arc::get_mut(&mut service).unwrap().limits.max_runs = max_runs;
        let task = crate::service::tests::invocation();
        let session = SessionId::new("restore-task", 64).unwrap();
        service
            .launch_terminal(
                task.clone(),
                &session,
                "MENU",
                24,
                80,
                "restore-csrf",
                1,
                10_000,
            )
            .unwrap();
        (service, task, session)
    }

    fn actor(task: &Invocation, name: &str) -> Invocation {
        let mut actor = task.clone();
        actor.execution_id = ExecutionId::new(name, InvocationLimits::default()).unwrap();
        actor.selector = Selector::new("program:EXIT", InvocationLimits::default()).unwrap();
        actor.artifact = ArtifactRef::new("exit-artifact", InvocationLimits::default()).unwrap();
        actor
    }

    #[test]
    fn terminal_program_restore_preserves_task_resources_across_repeated_actor_replacement() {
        let (service, task, session) = fixture(1);
        {
            let mut state = service.lock().unwrap();
            let run = state.runs.get_mut(&task.run_unit_id).unwrap();
            run.current_records.insert("DATA".into(), b"KEY".to_vec());
            run.file_updates
                .current_record_values
                .insert("DATA".into(), b"RECORD".to_vec());
            run.browses.insert("DATA".into(), "cursor".into());
            run.current_program.channel = Some("ROOT-CHANNEL".into());
        }
        for name in ["exit-one", "exit-two"] {
            let actor = actor(&task, name);
            service
                .restore_terminal_program_run(
                    task.clone(),
                    actor.clone(),
                    &session,
                    "MENU",
                    b"AREA".to_vec(),
                    2,
                )
                .unwrap();
            service.ensure_run(&actor).unwrap();
            let state = service.lock().unwrap();
            let run = state.runs.get(&task.run_unit_id).unwrap();
            assert_eq!(run.invocation.execution_id, task.execution_id);
            assert_eq!(
                run.current_program.effect_invocation.execution_id,
                actor.execution_id
            );
            assert_eq!(run.current_program.current.as_deref(), Some("EXIT"));
            assert_eq!(run.current_program.logical_level, 1);
            assert_eq!(run.current_records["DATA"], b"KEY");
            assert_eq!(run.file_updates.current_record_values["DATA"], b"RECORD");
            assert_eq!(run.browses["DATA"], "cursor");
            assert_eq!(run.current_program.channel.as_deref(), Some("ROOT-CHANNEL"));
        }
    }

    #[test]
    fn terminal_program_restore_rejects_widening_and_foreign_owner_without_mutation() {
        for case in 0..15 {
            let (service, task, session) = fixture(1);
            let before = service
                .store
                .get_provider_state("cics-session", session.as_str())
                .unwrap();
            let mut foreign_task = task.clone();
            let mut next = actor(&task, "exit");
            match case {
                0 => {
                    next.run_unit_id =
                        RunUnitId::new("foreign-run", InvocationLimits::default()).unwrap()
                }
                1 => next.deadline_tick += 1,
                2 => next.limits.max_steps += 1,
                3 => next.limits.max_storage_bytes += 1,
                4 => next.limits.max_output_bytes += 1,
                5 => next.limits.max_effects += 1,
                6 => next.limits.max_events += 1,
                7 => next.limits.max_frames += 1,
                8 => {
                    next.provider_generations.insert(
                        CapabilityId::new("host.cics.execute", InvocationLimits::default())
                            .unwrap(),
                        "foreign".into(),
                    );
                }
                9 => {
                    next.principal = Principal::new(
                        next.principal.id().clone(),
                        BTreeSet::new(),
                        InvocationLimits::default(),
                    )
                    .unwrap()
                }
                10 => {
                    foreign_task.execution_id =
                        ExecutionId::new("foreign-owner", InvocationLimits::default()).unwrap()
                }
                11 => next.attempt += 1,
                12 => {
                    next.cancellation_probe =
                        Some(mainframe_env_execution_api::CancellationProbe::new())
                }
                _ => {
                    next.bindings.insert(
                        if case == 13 {
                            "cics.execution-context"
                        } else {
                            "cics.syncpoint.remote-outcome"
                        }
                        .into(),
                        mainframe_env_execution_api::BoundedPayload::new(
                            "test@1",
                            b"foreign".to_vec(),
                            InvocationLimits::default(),
                        )
                        .unwrap(),
                    );
                }
            }
            assert!(
                service
                    .restore_terminal_program_run(
                        foreign_task,
                        next,
                        &session,
                        "MENU",
                        Vec::new(),
                        2
                    )
                    .is_err(),
                "case {case}"
            );
            assert_eq!(
                service
                    .store
                    .get_provider_state("cics-session", session.as_str())
                    .unwrap(),
                before
            );
            let state = service.lock().unwrap();
            let run = state.runs.get(&task.run_unit_id).unwrap();
            assert_eq!(run.invocation, task);
            assert_eq!(run.current_program.effect_invocation, task);
        }
    }

    #[test]
    fn terminal_program_restore_capacity_failure_does_not_remove_an_existing_task() {
        let (service, task, session) = fixture(1);
        let other = actor(&task, "other-execution");
        let mut other = handlers::new_run(other, "OTHER-SESSION", "MENU", "ME01", "S001");
        other.invocation.run_unit_id =
            RunUnitId::new("other-run", InvocationLimits::default()).unwrap();
        service
            .lock()
            .unwrap()
            .runs
            .insert(other.invocation.run_unit_id.clone(), other);
        assert_eq!(
            service.restore_terminal_program_run(
                task.clone(),
                actor(&task, "exit"),
                &session,
                "MENU",
                Vec::new(),
                2
            ),
            Err(HostProblem::ResourceExhausted)
        );
        let state = service.lock().unwrap();
        assert_eq!(state.runs.len(), 2);
        assert_eq!(state.runs[&task.run_unit_id].invocation, task);
    }

    #[test]
    fn terminal_program_restore_legacy_same_actor_remains_supported() {
        let (service, task, session) = fixture(1);
        service
            .restore_terminal_run(task.clone(), &session, "MENU", Vec::new(), 2)
            .unwrap();
        assert_eq!(
            service.lock().unwrap().runs[&task.run_unit_id]
                .current_program
                .effect_invocation,
            task
        );
    }
}
