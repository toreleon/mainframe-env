use super::super::{
    CicsService, Session, handlers, reject_reserved_nested_origin, validate_terminal_identity,
};
use super::IntervalStartRecord;
use mainframe_env_execution_api::Invocation;
use mainframe_env_host_api::{HostProblem, SessionId};
use std::collections::BTreeMap;

/// Immutable task identity resolved from one promoted local START request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CicsStartTask {
    /// Canonical START request identifier and retained-data key.
    pub request_id: String,
    /// Local transaction selected when the request expires.
    pub transaction: String,
    /// Principal bound when START was accepted.
    pub principal: String,
    /// Principal facility requested for the task, when supported.
    pub terminal: Option<String>,
}

pub(in crate::service) fn from_interval_record(record: &IntervalStartRecord) -> CicsStartTask {
    CicsStartTask {
        request_id: record.request_id.clone(),
        transaction: record.transaction.clone(),
        principal: record.principal.clone(),
        terminal: record.terminal.clone(),
    }
}

impl CicsService {
    /// Create or restore the facility-less CICS task owned by a local START request.
    pub fn launch_background_task(
        &self,
        invocation: Invocation,
        session: &SessionId,
        transaction: &str,
    ) -> Result<(), HostProblem> {
        reject_reserved_nested_origin(&invocation)?;
        validate_terminal_identity(transaction, 16)?;
        let retained_session = self.lock()?.sessions.contains_key(session.as_str());
        if !retained_session {
            self.authorize_terminal(&invocation, transaction)?;
        }
        let (undo, undo_version) = self.load_undo(&invocation.run_unit_id)?;
        let mut state = self.lock()?;
        let existing = state.sessions.get(session.as_str()).cloned();
        if let Some(current) = &existing
            && (current.principal != invocation.principal.id().as_str()
                || current.transaction != transaction.to_ascii_uppercase()
                || current.run_unit != invocation.run_unit_id.as_str()
                || !current.csrf_sha256.is_empty()
                || current.input.terminal_id.is_some())
        {
            return Err(HostProblem::IdempotencyConflict);
        }
        if let Some(run) = state.runs.get(&invocation.run_unit_id) {
            return if run.session == session.as_str()
                && run.transaction == transaction.to_ascii_uppercase()
                && run.invocation == invocation
            {
                Ok(())
            } else {
                Err(HostProblem::IdempotencyConflict)
            };
        }
        if state.runs.len() >= self.limits.max_runs {
            return Err(HostProblem::ResourceExhausted);
        }
        let new_session = existing.is_none();
        let current = existing.unwrap_or_else(|| Session {
            rows: 1,
            columns: 1,
            principal: invocation.principal.id().as_str().into(),
            transaction: transaction.to_ascii_uppercase(),
            run_unit: invocation.run_unit_id.as_str().into(),
            user_corr_data: Vec::new(),
            user_corr_effect_key: None,
            user_corr_request_digest: None,
            csrf_sha256: String::new(),
            idle_timeout_ticks: u64::MAX,
            expires_at_tick: u64::MAX,
            connected: true,
            aid: 0,
            screen: Vec::new(),
            input: handlers::TerminalInput::default(),
            suspended: false,
            mapset: None,
            map: None,
            field_protection: BTreeMap::new(),
            field_modified: BTreeMap::new(),
            field_values: BTreeMap::new(),
            handle_state: handlers::HandleState::default(),
            version: 1,
        });
        if new_session {
            if state.sessions.len() >= self.limits.max_sessions {
                return Err(HostProblem::ResourceExhausted);
            }
            self.persist_session(session.as_str(), &current, None)?;
            state
                .sessions
                .insert(session.as_str().into(), current.clone());
        }
        state.runs.insert(
            invocation.run_unit_id.clone(),
            handlers::new_run_with_state(
                invocation.clone(),
                session.as_str(),
                transaction,
                "ME01",
                "S001",
                handlers::RunSeed {
                    originating_task: invocation.run_unit_id.as_str().into(),
                    retrieve: Vec::new(),
                    undo,
                    undo_version,
                    handle_state: current.handle_state,
                },
            ),
        );
        Ok(())
    }

    /// Delete a facility-less task session after its execution becomes terminal.
    pub fn finish_background_task(
        &self,
        session: &SessionId,
        principal: &mainframe_env_execution_api::PrincipalId,
    ) -> Result<(), HostProblem> {
        let mut state = self.lock()?;
        let Some(current) = state.sessions.get(session.as_str()).cloned() else {
            return Ok(());
        };
        if current.principal != principal.as_str() {
            return Err(HostProblem::Unauthorized);
        }
        if !current.csrf_sha256.is_empty()
            || current.input.terminal_id.is_some()
            || state
                .runs
                .values()
                .any(|run| run.session == session.as_str())
            || state.continuations.contains_key(session.as_str())
        {
            return Err(HostProblem::IdempotencyConflict);
        }
        self.store
            .delete_provider_state("cics-session", session.as_str(), current.version)
            .map_err(super::store_error)?;
        state.sessions.remove(session.as_str());
        Ok(())
    }
}
