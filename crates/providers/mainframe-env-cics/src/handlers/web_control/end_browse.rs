use super::super::super::{CicsService, Run};
use super::{model, open};
use mainframe_env_execution_api::AuditDecision;
use mainframe_env_host_api::{CicsDisposition, CicsRequest, CicsResponse, HostProblem};
use mainframe_env_store_api::ProviderStateMutation;
use std::sync::atomic::Ordering;

pub(super) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
    retention_tick: u64,
) -> Result<CicsResponse, HostProblem> {
    let result = invoke_inner(service, run, request, retention_tick);
    let decision = match &result {
        Ok(_) => AuditDecision::Success,
        Err(HostProblem::Unauthorized) => AuditDecision::Deny,
        Err(HostProblem::Cancelled) => AuditDecision::Cancelled,
        Err(HostProblem::TimedOut) => AuditDecision::TimedOut,
        Err(HostProblem::UnknownOutcome) => AuditDecision::UnknownOutcome,
        Err(HostProblem::InfrastructureFailure) => AuditDecision::InfrastructureFailure,
        Err(_) => AuditDecision::ProviderFailure,
    };
    open::audit_web_decision(service, run, request, decision)?;
    result
}

fn invoke_inner(
    service: &CicsService,
    run: &Run,
    request: &CicsRequest,
    retention_tick: u64,
) -> Result<CicsResponse, HostProblem> {
    let kind = validate_request(request)?;
    if run.invocation.cancellation_requested() {
        return Err(HostProblem::Cancelled);
    }
    let key = model::browse_key(run.invocation.run_unit_id.as_str(), kind);
    let mut state = service.lock()?;
    let browse = state
        .web
        .browses
        .get(&key)
        .filter(|browse| {
            browse.owner_execution == run.invocation.execution_id.as_str()
                && browse.owner_run_unit == run.invocation.run_unit_id.as_str()
                && browse.transaction == run.transaction
        })
        .cloned()
        .ok_or_else(|| condition("INVREQ", 16, 4))?;
    let token = request
        .arguments
        .get("SESSTOKEN")
        .map(|value| {
            value
                .bytes()
                .try_into()
                .map_err(|_| condition("NOTOPEN", 19, 27))
        })
        .transpose()?;
    if browse.client_token != token {
        return Err(condition("NOTOPEN", 19, 27));
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
    let writes = vec![
        ProviderStateMutation::Delete {
            namespace: model::BROWSE_NAMESPACE.into(),
            key: key.clone(),
            expected_version: browse.version,
        },
        ProviderStateMutation::Put(open::replay_write(run, request, retention_tick, &response)?),
    ];
    if service.store.mutate_provider_states_atomic(writes).is_err() {
        return Err(HostProblem::UnknownOutcome);
    }
    state.web.bytes = state
        .web
        .bytes
        .checked_sub(model::encode_browse(&browse)?.len())
        .ok_or(HostProblem::InfrastructureFailure)?;
    state.web.browses.remove(&key);
    drop(state);
    if run.invocation.cancellation_requested()
        || open::expired_after_dispatch(service, run)?
        || service
            .replay_unknown_after_persist
            .swap(false, Ordering::SeqCst)
    {
        return Err(HostProblem::UnknownOutcome);
    }
    Ok(response)
}

fn validate_request(request: &CicsRequest) -> Result<&'static str, HostProblem> {
    const ALLOWED: &[&str] = &[
        "OPTION.HTTPHEADER",
        "OPTION.QUERYPARM",
        "OPTION.FORMFIELD",
        "OPTION.NOHANDLE",
        "SESSTOKEN",
        "RESP",
        "RESP2",
    ];
    if request.mutation.is_none()
        || request
            .arguments
            .keys()
            .any(|name| !ALLOWED.contains(&name.as_str()))
        || request.arguments.contains_key("RESP2") && !request.arguments.contains_key("RESP")
    {
        return Err(HostProblem::Malformed);
    }
    let selected = ["HTTPHEADER", "QUERYPARM", "FORMFIELD"]
        .into_iter()
        .filter(|kind| request.arguments.contains_key(&format!("OPTION.{kind}")))
        .collect::<Vec<_>>();
    if selected.len() != 1
        || selected[0] != "HTTPHEADER" && request.arguments.contains_key("SESSTOKEN")
    {
        return Err(HostProblem::Malformed);
    }
    Ok(selected[0])
}

fn condition(name: &str, response: i32, response2: i32) -> HostProblem {
    HostProblem::Condition {
        name: name.into(),
        response,
        response2,
    }
}
