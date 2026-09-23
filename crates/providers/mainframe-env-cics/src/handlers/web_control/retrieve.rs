use super::super::super::{CicsService, Run, bounded};
use super::open;
use mainframe_env_execution_api::AuditDecision;
use mainframe_env_host_api::{CicsDisposition, CicsRequest, CicsResponse, HostProblem};

pub(super) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    let result = invoke_inner(service, run, request);
    let decision = match &result {
        Ok(_) => AuditDecision::Success,
        Err(HostProblem::Unauthorized) => AuditDecision::Deny,
        Err(HostProblem::Cancelled) => AuditDecision::Cancelled,
        Err(HostProblem::TimedOut) => AuditDecision::TimedOut,
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
) -> Result<CicsResponse, HostProblem> {
    if request.mutation.is_some()
        || !request.arguments.contains_key("DOCTOKEN")
        || request
            .arguments
            .keys()
            .any(|name| !["DOCTOKEN", "RESP", "RESP2", "OPTION.NOHANDLE"].contains(&name.as_str()))
        || request.arguments.contains_key("RESP2") && !request.arguments.contains_key("RESP")
    {
        return Err(HostProblem::Malformed);
    }
    let state = service.lock()?;
    let key = run.invocation.run_unit_id.as_str();
    let inbound = state
        .web
        .inbound
        .get(key)
        .ok_or_else(|| condition("INVREQ", 16, 1))?;
    if !inbound.http {
        return Err(condition("INVREQ", 16, 1));
    }
    let reply = state
        .web
        .server_responses
        .get(key)
        .ok_or_else(|| condition("INVREQ", 16, 2))?;
    if reply.owner_execution != run.invocation.execution_id.as_str()
        || reply.owner_run_unit != key
        || reply.transaction != run.transaction
    {
        return Err(HostProblem::Unauthorized);
    }
    let token = reply
        .response
        .document_token
        .filter(|_| reply.response.eventual)
        .ok_or_else(|| condition("NOTFND", 13, 1))?;
    drop(state);
    let mut response = service.response(
        run,
        CicsDisposition::Complete,
        "NORMAL",
        0,
        0,
        None,
        None,
        Vec::new(),
    )?;
    response
        .outputs
        .insert("DOCTOKEN".into(), bounded(token.to_vec())?);
    Ok(response)
}

fn condition(name: &str, response: i32, response2: i32) -> HostProblem {
    HostProblem::Condition {
        name: name.into(),
        response,
        response2,
    }
}
