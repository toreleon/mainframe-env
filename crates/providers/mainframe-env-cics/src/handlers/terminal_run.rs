use super::super::{CicsService, CicsTraceEntry, handlers};
use mainframe_env_execution_api::{InvocationLimits, PrincipalId, RunUnitId};
use mainframe_env_host_api::{HostProblem, SessionId};

impl CicsService {
    pub fn complete_terminal_run(
        &self,
        session: &SessionId,
        principal: &PrincipalId,
        now_tick: u64,
    ) -> Result<(), HostProblem> {
        self.finish_terminal_run(session, principal, now_tick, true)
    }

    /// Remove a handed-off volatile run while retaining its durable HANDLE state.
    pub fn suspend_terminal_run(
        &self,
        session: &SessionId,
        principal: &PrincipalId,
        now_tick: u64,
    ) -> Result<(), HostProblem> {
        self.finish_terminal_run(session, principal, now_tick, false)
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
        let run_id = RunUnitId::new(&current.run_unit, InvocationLimits::default())
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        let mut state = self.lock()?;
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
        if let Some(run) = state.runs.get(&run_id).cloned() {
            handlers::release_task_state(self, &run)?;
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
    ) -> Result<(), HostProblem> {
        let current = self.public_session(session, principal, None, now_tick)?;
        let run_id = RunUnitId::new(&current.run_unit, InvocationLimits::default())
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        let mut state = self.lock()?;
        let run = state
            .runs
            .get(&run_id)
            .cloned()
            .ok_or(HostProblem::NotFound)?;
        if run.session != session.as_str() || run.invocation.principal.id() != principal {
            return Err(HostProblem::Unauthorized);
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
        handlers::release_task_state(self, &run)?;
        state.runs.remove(&run_id);
        Ok(())
    }
}
