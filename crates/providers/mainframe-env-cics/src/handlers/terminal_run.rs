use super::super::{CicsService, CicsTraceEntry, handlers};
use mainframe_env_execution_api::{InvocationLimits, PrincipalId, RunUnitId};
use mainframe_env_host_api::{CicsUnitOfWorkOutcome, HostProblem, SessionId};
use sha2::{Digest, Sha256};

pub(in crate::service) fn terminal_secret_digest(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

impl CicsService {
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
        let run = state.runs.get(&run_id).cloned();
        drop(state);
        if let Some(run) = &run {
            handlers::release_task_state(self, &run)?;
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
        let run = {
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
        handlers::release_task_state(self, &run)?;
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
