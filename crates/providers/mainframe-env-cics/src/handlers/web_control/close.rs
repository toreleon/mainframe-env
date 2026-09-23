use super::super::super::{CicsService, Run};
use super::{model, open};
use mainframe_env_execution_api::AuditDecision;
use mainframe_env_host_api::{
    AccessIntent, CicsDisposition, CicsRequest, CicsResponse, HostProblem,
};
use mainframe_env_store_api::ProviderStateMutation;
use std::sync::atomic::Ordering;

pub(super) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
    retention_tick: u64,
) -> Result<CicsResponse, HostProblem> {
    if request.mutation.is_none()
        || request
            .arguments
            .keys()
            .any(|name| !["SESSTOKEN", "RESP", "RESP2", "OPTION.NOHANDLE"].contains(&name.as_str()))
        || request.arguments.contains_key("RESP2") && !request.arguments.contains_key("RESP")
    {
        return Err(HostProblem::Malformed);
    }
    let token_bytes = request
        .arguments
        .get("SESSTOKEN")
        .ok_or(HostProblem::Malformed)?
        .bytes();
    let token: [u8; 8] = token_bytes
        .try_into()
        .map_err(|_| condition("NOTOPEN", 19, 144))?;
    let key = model::token_key(token);
    let (session, transport) = {
        let state = service.lock()?;
        let session = state
            .web
            .sessions
            .get(&key)
            .filter(|session| {
                session.owner_execution == run.invocation.execution_id.as_str()
                    && session.owner_run_unit == run.invocation.run_unit_id.as_str()
                    && session.transaction == run.transaction
            })
            .cloned()
            .ok_or_else(|| condition("NOTOPEN", 19, 27))?;
        let transport = state
            .web
            .transport
            .clone()
            .ok_or(HostProblem::ProviderFailure)?;
        (session, transport)
    };
    if let Some(name) = session.endpoint.urimap.as_deref() {
        service.authorize(run, "URIMAP", name, AccessIntent::Read)?;
    }
    if run.invocation.cancellation_requested() {
        return Err(HostProblem::Cancelled);
    }
    let result = transport.release(&session.endpoint, token, session.pooled, &run.invocation);
    let decision = match &result {
        Ok(()) => AuditDecision::Success,
        Err(HostProblem::Cancelled) => AuditDecision::Cancelled,
        Err(HostProblem::TimedOut) => AuditDecision::TimedOut,
        Err(HostProblem::Unauthorized) => AuditDecision::Deny,
        Err(HostProblem::UnknownOutcome) => AuditDecision::UnknownOutcome,
        Err(HostProblem::InfrastructureFailure) => AuditDecision::InfrastructureFailure,
        Err(_) => AuditDecision::ProviderFailure,
    };
    open::audit_web_decision(service, run, request, decision)?;
    result.map_err(open::transport_problem)?;
    if run.invocation.cancellation_requested() || open::expired_after_dispatch(service, run)? {
        open::audit_web_decision(service, run, request, AuditDecision::UnknownOutcome)?;
        return Err(HostProblem::UnknownOutcome);
    }
    let response = service.response(
        run,
        CicsDisposition::Complete,
        "NORMAL",
        0,
        0,
        None,
        None,
        Vec::new(),
    )?;
    let mut state = service.lock()?;
    if state.web.sessions.get(&key) != Some(&session) {
        return Err(HostProblem::UnknownOutcome);
    }
    let header_key = model::header_stage_key(run.invocation.run_unit_id.as_str(), Some(token));
    let staged = state.web.pending_headers.get(&header_key).cloned();
    let client_response = state.web.client_responses.get(&key).cloned();
    let mut mutations = vec![
        ProviderStateMutation::Delete {
            namespace: model::SESSION_NAMESPACE.into(),
            key: key.clone(),
            expected_version: session.version,
        },
        ProviderStateMutation::Put(open::replay_write(run, request, retention_tick, &response)?),
    ];
    if let Some(headers) = &staged {
        mutations.push(ProviderStateMutation::Delete {
            namespace: model::HEADER_NAMESPACE.into(),
            key: header_key.clone(),
            expected_version: headers.version,
        });
    }
    if let Some(client_response) = &client_response {
        mutations.push(ProviderStateMutation::Delete {
            namespace: model::CLIENT_RESPONSE_NAMESPACE.into(),
            key: key.clone(),
            expected_version: client_response.version,
        });
    }
    if service
        .store
        .mutate_provider_states_atomic(mutations)
        .is_err()
    {
        drop(state);
        open::audit_web_decision(service, run, request, AuditDecision::UnknownOutcome)?;
        return Err(HostProblem::UnknownOutcome);
    }
    state.web.bytes = state
        .web
        .bytes
        .checked_sub(model::encode_session(&session)?.len())
        .ok_or(HostProblem::InfrastructureFailure)?;
    state.web.sessions.remove(&key);
    if let Some(headers) = staged {
        state.web.bytes = state
            .web
            .bytes
            .checked_sub(model::encode_header_stage(&headers)?.len())
            .ok_or(HostProblem::InfrastructureFailure)?;
        state.web.pending_headers.remove(&header_key);
    }
    if let Some(client_response) = client_response {
        state.web.bytes = state
            .web
            .bytes
            .checked_sub(model::encode_client_response(&client_response)?.len())
            .ok_or(HostProblem::InfrastructureFailure)?;
        state.web.client_responses.remove(&key);
    }
    drop(state);
    if service
        .replay_unknown_after_persist
        .swap(false, Ordering::SeqCst)
    {
        open::audit_web_decision(service, run, request, AuditDecision::UnknownOutcome)?;
        return Err(HostProblem::UnknownOutcome);
    }
    Ok(response)
}

fn condition(name: &str, response: i32, response2: i32) -> HostProblem {
    HostProblem::Condition {
        name: name.into(),
        response,
        response2,
    }
}
