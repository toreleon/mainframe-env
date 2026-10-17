//! Terminal session lookup and expiration outside the CICS state mutex.

use super::super::*;

impl CicsService {
    pub fn disconnect_terminal(
        &self,
        session: &SessionId,
        principal: &PrincipalId,
        csrf_token: &str,
        now_tick: u64,
    ) -> Result<(), HostProblem> {
        let current = self.public_session(session, principal, Some(csrf_token), now_tick)?;
        let _cleanup = handlers::SessionCleanupLease::acquire(self, session.as_str())?;
        let state = self.lock()?;
        if state
            .sessions
            .get(session.as_str())
            .is_none_or(|value| value.version != current.version)
        {
            return Err(HostProblem::IdempotencyConflict);
        }
        let mut runs = state
            .runs
            .values()
            .filter(|run| run.session == session.as_str())
            .cloned()
            .collect::<Vec<_>>();
        drop(state);
        for run in &mut runs {
            handlers::release_terminal_task_state(self, run)?;
        }
        let mut state = self.lock()?;
        if state
            .sessions
            .get(session.as_str())
            .is_none_or(|value| value.version != current.version)
            || state
                .runs
                .values()
                .filter(|run| run.session == session.as_str())
                .map(|run| run.invocation.run_unit_id.as_str())
                .collect::<BTreeSet<_>>()
                != runs
                    .iter()
                    .map(|run| run.invocation.run_unit_id.as_str())
                    .collect::<BTreeSet<_>>()
        {
            return Err(HostProblem::IdempotencyConflict);
        }
        for run in &runs {
            handlers::discard_task_starts(self, &mut state.interval_records, run)?;
        }
        self.store
            .delete_provider_state("cics-session", session.as_str(), current.version)
            .map_err(store_error)?;
        state.sessions.remove(session.as_str());
        state.runs.retain(|_, run| run.session != session.as_str());
        Ok(())
    }

    pub(in crate::service) fn public_session(
        &self,
        session: &SessionId,
        principal: &PrincipalId,
        csrf_token: Option<&str>,
        now_tick: u64,
    ) -> Result<Session, HostProblem> {
        let mut state = self.lock()?;
        let current = state
            .sessions
            .get(session.as_str())
            .cloned()
            .ok_or(HostProblem::NotFound)?;
        if current.principal.is_empty() || current.principal != principal.as_str() {
            return Err(HostProblem::Unauthorized);
        }
        if let Some(token) = csrf_token
            && (token.is_empty() || handlers::terminal_secret_digest(token) != current.csrf_sha256)
        {
            return Err(HostProblem::Unauthorized);
        }
        if !current.connected {
            return Err(HostProblem::NotFound);
        }
        state
            .task_dispatch
            .require_available_session(session.as_str())?;
        if now_tick >= current.expires_at_tick {
            let mut runs = state
                .runs
                .values()
                .filter(|run| run.session == session.as_str())
                .cloned()
                .collect::<Vec<_>>();
            let _cleanup =
                handlers::SessionCleanupLease::acquire_locked(self, &mut state, session.as_str())?;
            drop(state);
            for run in &mut runs {
                handlers::release_terminal_task_state(self, run)?;
            }
            let mut state = self.lock()?;
            if state
                .sessions
                .get(session.as_str())
                .is_none_or(|value| value.version != current.version)
                || state
                    .runs
                    .values()
                    .filter(|run| run.session == session.as_str())
                    .map(|run| run.invocation.run_unit_id.as_str())
                    .collect::<BTreeSet<_>>()
                    != runs
                        .iter()
                        .map(|run| run.invocation.run_unit_id.as_str())
                        .collect::<BTreeSet<_>>()
            {
                return Err(HostProblem::IdempotencyConflict);
            }
            for run in &runs {
                handlers::discard_task_starts(self, &mut state.interval_records, run)?;
            }
            self.store
                .delete_provider_state("cics-session", session.as_str(), current.version)
                .map_err(store_error)?;
            state.sessions.remove(session.as_str());
            state.runs.retain(|_, run| run.session != session.as_str());
            return Err(HostProblem::TimedOut);
        }
        Ok(current)
    }
}
