//! Durable full-BMS route dispatch for local terminal sessions.

use super::*;
use mainframe_env_host_api::AccessIntent;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

mod queue;
mod selection;
mod timing;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RouteHistory {
    title: Vec<u8>,
    recipients: Vec<String>,
    requested_tick: u64,
    due_tick: u64,
}

const HISTORY_NAMESPACE: &str = "cics-bms-route-history-v1";

impl CicsService {
    /// Read the last routed title and terminal identities for one source session.
    pub fn last_route(
        &self,
        session: &mainframe_env_host_api::SessionId,
    ) -> Result<Option<(Vec<u8>, Vec<String>, u64)>, HostProblem> {
        self.store
            .get_provider_state(HISTORY_NAMESPACE, session.as_str())
            .map_err(store_error)?
            .map(|row| {
                if row.version == 0 {
                    return Err(HostProblem::InfrastructureFailure);
                }
                let history: RouteHistory = serde_json::from_slice(&row.payload)
                    .map_err(|_| HostProblem::InfrastructureFailure)?;
                if history.title.len() > 256
                    || history.recipients.len() > self.limits.max_sessions
                    || history.due_tick < history.requested_tick
                {
                    return Err(HostProblem::InfrastructureFailure);
                }
                Ok((history.title, history.recipients, history.due_tick))
            })
            .transpose()
    }
}

fn history_write(
    service: &CicsService,
    session: &str,
    history: RouteHistory,
) -> Result<ProviderStateMutation, HostProblem> {
    let existing = service
        .store
        .get_provider_state(HISTORY_NAMESPACE, session)
        .map_err(store_error)?;
    let version = existing.as_ref().map_or(0, |row| row.version);
    Ok(ProviderStateMutation::Put(ProviderStateWrite {
        record: ProviderStateRecord {
            namespace: HISTORY_NAMESPACE.into(),
            key: session.into(),
            version: version
                .checked_add(1)
                .ok_or(HostProblem::ResourceExhausted)?,
            payload: serde_json::to_vec(&history)
                .map_err(|_| HostProblem::InfrastructureFailure)?,
        },
        expected_version: (version != 0).then_some(version),
    }))
}

pub(super) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    selection::validate_request(request, service.limits.max_screen_bytes)?;
    super::super::validate_purge_message_context(run)?;
    if let Some(response) = receipt_for_request(service, run, request)? {
        return Ok(response);
    }
    service.authorize(run, "FACILITY", "CICS.TERMINAL.ROUTE", AccessIntent::Update)?;
    let sessions = service.lock()?.sessions.clone();
    let origin = sessions.get(&run.session).ok_or(HostProblem::NotFound)?;
    let mut state = read_state(service, &run.session)?;
    let message = state
        .message
        .as_ref()
        .ok_or_else(|| condition("INVREQ", 16))?;
    if message.mode == 2 {
        return Err(condition("INVREQ", 16));
    }
    if let Some(reqid) = selection::text_name(request, "REQID", 2)?
        && reqid != message.reqid
    {
        return Err(condition("IGREQID", 39));
    }
    let page = if message.payload.is_empty() {
        origin.screen.clone()
    } else {
        message.payload.clone()
    };
    if page.len() > service.limits.max_screen_bytes {
        return Err(HostProblem::ResourceExhausted);
    }
    let title = request
        .arguments
        .get("TITLE")
        .map(|value| value.bytes().to_vec())
        .unwrap_or_default();
    let frames = message.frames.clone();
    let (selected, mut failed) = selection::recipients(service, run, request, &sessions)?;
    if selected.is_empty() {
        return Err(condition("RTEFAIL", 33));
    }
    let selected_terminals = selected
        .iter()
        .filter_map(|(_, session)| session.input.terminal_id.clone())
        .collect::<Vec<_>>();
    let (now, due) = timing::due_tick(service, request)?;
    let mut mutations = Vec::new();
    let mut delivered = BTreeMap::new();
    if due > now {
        let mut queue = queue::read(service)?;
        for (key, session) in &selected {
            let terminal = session
                .input
                .terminal_id
                .as_ref()
                .ok_or(HostProblem::InfrastructureFailure)?;
            queue.pending.push(queue::PendingRoute {
                due_tick: due,
                session: key.clone(),
                terminal: terminal.clone(),
                image: page.clone(),
                title: title.clone(),
                errterm: selection::text_name(request, "ERRTERM", 4)?,
                frames: frames.clone(),
            });
        }
        mutations.push(queue::write(service, &queue)?);
    } else {
        for (key, session) in selected {
            let mut next = session.clone();
            let mut target_state = read_state(service, &key)?;
            if frames
                .iter()
                .try_for_each(|frame| control::apply_frame(&mut target_state, &mut next, frame))
                .is_err()
            {
                failed += 1;
                continue;
            }
            next.screen = page.clone();
            next.version = session
                .version
                .checked_add(1)
                .ok_or(HostProblem::ResourceExhausted)?;
            mutations.push(session_write(&key, &session, &next)?);
            mutations.push(state_write(&key, &target_state, service)?);
            delivered.insert(key, next);
        }
        if delivered.is_empty() {
            return Err(condition("RTEFAIL", 33));
        }
    }
    state.message = None;
    state.last_page = page;
    mutations.push(state_write(&run.session, &state, service)?);
    let terminals = if due > now {
        selected_terminals
    } else {
        delivered
            .values()
            .filter_map(|session| session.input.terminal_id.clone())
            .collect()
    };
    mutations.push(history_write(
        service,
        &run.session,
        RouteHistory {
            title,
            recipients: terminals,
            requested_tick: now,
            due_tick: due,
        },
    )?);
    let mut receipt = base_receipt(run, request)?;
    if failed > 0 {
        receipt.condition = "RTESOME".into();
        receipt.response = 34;
    }
    mutations.push(receipt_write(request, &receipt, service)?);
    match service.store.mutate_provider_states_atomic(mutations) {
        Ok(()) => {
            if !delivered.is_empty() {
                let mut live = service.lock().map_err(mutation_problem)?;
                for (key, next) in delivered {
                    live.sessions.insert(key, next);
                }
            }
            receipt_response(service, run, &receipt)
        }
        Err(
            mainframe_env_store_api::StoreError::Conflict
            | mainframe_env_store_api::StoreError::AlreadyExists,
        ) => receipt_for_request(service, run, request)?.ok_or(HostProblem::IdempotencyConflict),
        Err(error) => Err(mutation_problem(store_error(error))),
    }
}

fn condition(name: &str, response: i32) -> HostProblem {
    HostProblem::Condition {
        name: name.into(),
        response,
        response2: 0,
    }
}
