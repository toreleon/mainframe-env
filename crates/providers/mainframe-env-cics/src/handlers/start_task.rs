use super::super::{
    CicsService, Session, handlers, reject_reserved_nested_origin, validate_terminal_identity,
};
use super::IntervalStartRecord;
use mainframe_env_execution_api::{Invocation, InvocationLimits, PrincipalId};
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
    /// Whether this was a noncancelable START ATTACH with STARTCODE U.
    pub attached: bool,
}

/// Retained terminal session selected by a terminal-associated START.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CicsStartTerminal {
    /// Durable session identity that owns the virtual terminal.
    pub session: SessionId,
    /// Signed-on terminal principal.
    pub principal: PrincipalId,
    /// Whether no CICS run currently owns the terminal.
    pub available: bool,
}

pub(in crate::service) fn from_interval_record(record: &IntervalStartRecord) -> CicsStartTask {
    CicsStartTask {
        request_id: record.request_id.clone(),
        transaction: record.transaction.clone(),
        principal: record.principal.clone(),
        terminal: record.terminal.clone(),
        attached: record.state == super::interval_control::IntervalStartState::AttachedReady,
    }
}

impl CicsService {
    /// Resolve an active virtual terminal for a promoted START request.
    pub fn resolve_start_terminal(
        &self,
        terminal: &str,
    ) -> Result<Option<CicsStartTerminal>, HostProblem> {
        validate_terminal_identity(terminal, 4)?;
        let state = self.lock()?;
        let mut matches = state.sessions.iter().filter(|(_, session)| {
            session.connected && session.input.terminal_id.as_deref() == Some(terminal)
        });
        let Some((session_name, session)) = matches.next() else {
            return Ok(None);
        };
        if matches.next().is_some() || session.principal.is_empty() {
            return Err(HostProblem::InfrastructureFailure);
        }
        let session_id =
            SessionId::new(session_name, InvocationLimits::default().max_binding_bytes)
                .map_err(|_| HostProblem::InfrastructureFailure)?;
        let principal = PrincipalId::new(&session.principal, InvocationLimits::default())
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        let available = !state
            .runs
            .values()
            .any(|run| run.session == session_name.as_str());
        Ok(Some(CicsStartTerminal {
            session: session_id,
            principal,
            available,
        }))
    }

    pub(in crate::service) fn start_terminal_principal(
        &self,
        terminal: &str,
    ) -> Result<Option<String>, HostProblem> {
        let state = self.lock()?;
        let mut principals = state
            .sessions
            .values()
            .filter(|session| {
                session.connected
                    && session.input.terminal_id.as_deref() == Some(terminal)
                    && !session.principal.is_empty()
            })
            .map(|session| session.principal.clone());
        let principal = principals.next();
        if principals.next().is_some() {
            return Err(HostProblem::InfrastructureFailure);
        }
        Ok(principal)
    }

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
        let bridged = invocation.bindings.contains_key("cics.bridge-request");
        let current = existing.unwrap_or_else(|| Session {
            rows: if bridged { 24 } else { 1 },
            columns: if bridged { 80 } else { 1 },
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

    /// Create or restore a started task on an available terminal session.
    pub fn launch_started_terminal_task(
        &self,
        invocation: Invocation,
        session: &SessionId,
        transaction: &str,
        now_tick: u64,
    ) -> Result<(), HostProblem> {
        reject_reserved_nested_origin(&invocation)?;
        validate_terminal_identity(transaction, 16)?;
        let current = self.public_session(session, invocation.principal.id(), None, now_tick)?;
        if current.input.terminal_id.is_none() {
            return Err(HostProblem::Malformed);
        }
        self.authorize_terminal(&invocation, transaction)?;
        let (undo, undo_version) = self.load_undo(&invocation.run_unit_id)?;
        let mut state = self.lock()?;
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
        if state
            .runs
            .values()
            .any(|run| run.session == session.as_str())
        {
            return Err(HostProblem::IdempotencyConflict);
        }
        if state.runs.len() >= self.limits.max_runs {
            return Err(HostProblem::ResourceExhausted);
        }
        let retained = current.run_unit == invocation.run_unit_id.as_str()
            && current.transaction == transaction.to_ascii_uppercase();
        let active = if retained {
            current
        } else {
            let mut active = current;
            let previous = active.version;
            active.version = previous
                .checked_add(1)
                .ok_or(HostProblem::ResourceExhausted)?;
            active.run_unit = invocation.run_unit_id.as_str().into();
            active.transaction = transaction.to_ascii_uppercase();
            active.expires_at_tick = now_tick
                .checked_add(active.idle_timeout_ticks)
                .ok_or(HostProblem::ResourceExhausted)?;
            self.persist_session(session.as_str(), &active, Some(previous))?;
            state
                .sessions
                .insert(session.as_str().into(), active.clone());
            active
        };
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
                    handle_state: active.handle_state,
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
