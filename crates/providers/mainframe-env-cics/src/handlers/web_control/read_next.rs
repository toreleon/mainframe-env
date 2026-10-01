use super::super::super::{CicsService, Run, bounded, decimal_payload};
use super::{model, open};
use mainframe_env_execution_api::AuditDecision;
use mainframe_env_host_api::{CicsDisposition, CicsRequest, CicsResponse, HostProblem};
use mainframe_env_store_api::{ProviderStateMutation, ProviderStateRecord, ProviderStateWrite};
use std::sync::atomic::Ordering;

pub(super) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
    retention_tick: u64,
) -> Result<CicsResponse, HostProblem> {
    let result = invoke_inner(service, run, request, retention_tick);
    let decision = match &result {
        Ok(response) if response.response == 0 => AuditDecision::Success,
        Ok(_) => AuditDecision::ProviderFailure,
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
    let Some((name, value)) = browse.entries.get(browse.cursor) else {
        return Err(condition("ENDFILE", 20, 0));
    };
    let client = token.is_some();
    let name_capacity = capacity(
        service,
        request,
        "NAMELENGTH",
        "BROWSENAME.MAXLENGTH",
        if client { 35 } else { 1 },
    )?;
    let value_capacity = capacity(
        service,
        request,
        "VALUELENGTH",
        "VALUE.MAXLENGTH",
        if client { 55 } else { 1 },
    )?;
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
    response.outputs.insert(
        "BROWSENAME".into(),
        bounded(name[..name.len().min(name_capacity)].to_vec())?,
    );
    response.outputs.insert(
        "VALUE".into(),
        bounded(value[..value.len().min(value_capacity)].to_vec())?,
    );
    response.outputs.insert(
        "NAMELENGTH".into(),
        decimal_payload(i64::try_from(name.len()).map_err(|_| HostProblem::ResourceExhausted)?)?,
    );
    response.outputs.insert(
        "VALUELENGTH".into(),
        decimal_payload(i64::try_from(value.len()).map_err(|_| HostProblem::ResourceExhausted)?)?,
    );
    if name.len() > name_capacity || value.len() > value_capacity {
        response.condition = "LENGERR".into();
        response.response = 22;
        response.response2 = if name.len() > name_capacity {
            if client { 51 } else { 4 }
        } else if client {
            52
        } else {
            5
        };
        return Ok(response);
    }
    let mut next = browse.clone();
    next.cursor += 1;
    next.version += 1;
    let payload = model::encode_browse(&next)?;
    let writes = vec![
        ProviderStateMutation::Put(ProviderStateWrite {
            record: ProviderStateRecord {
                namespace: model::BROWSE_NAMESPACE.into(),
                key: key.clone(),
                version: next.version,
                payload,
            },
            expected_version: Some(browse.version),
        }),
        ProviderStateMutation::Put(open::replay_write(run, request, retention_tick, &response)?),
    ];
    if service.store.mutate_provider_states_atomic(writes).is_err() {
        return Err(HostProblem::UnknownOutcome);
    }
    state.web.browses.insert(key, next);
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
        "BROWSENAME",
        "BROWSENAME.MAXLENGTH",
        "NAMELENGTH",
        "SESSTOKEN",
        "VALUE",
        "VALUE.MAXLENGTH",
        "VALUELENGTH",
        "RESP",
        "RESP2",
    ];
    if request.mutation.is_none()
        || request
            .arguments
            .keys()
            .any(|name| !ALLOWED.contains(&name.as_str()))
        || request.arguments.contains_key("RESP2") && !request.arguments.contains_key("RESP")
        || ["BROWSENAME", "NAMELENGTH", "VALUE", "VALUELENGTH"]
            .iter()
            .any(|name| !request.arguments.contains_key(*name))
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

fn capacity(
    service: &CicsService,
    request: &CicsRequest,
    length: &str,
    declared: &str,
    response2: i32,
) -> Result<usize, HostProblem> {
    let value = request
        .arguments
        .get(length)
        .ok_or(HostProblem::Malformed)?;
    if value.schema() != "mainframe-env.cics.decimal@1" {
        return Err(HostProblem::Malformed);
    }
    let number = std::str::from_utf8(value.bytes())
        .map_err(|_| HostProblem::Malformed)?
        .parse::<i64>()
        .map_err(|_| HostProblem::Malformed)?;
    if number <= 0 {
        return Err(condition("LENGERR", 22, response2));
    }
    let number = usize::try_from(number).map_err(|_| HostProblem::ResourceExhausted)?;
    if number > service.limits.max_web_bytes {
        return Err(HostProblem::ResourceExhausted);
    }
    if let Some(maximum) = request.arguments.get(declared) {
        let maximum = std::str::from_utf8(maximum.bytes())
            .map_err(|_| HostProblem::Malformed)?
            .parse::<usize>()
            .map_err(|_| HostProblem::Malformed)?;
        if number > maximum {
            return Err(HostProblem::Malformed);
        }
    }
    Ok(number)
}

fn condition(name: &str, response: i32, response2: i32) -> HostProblem {
    HostProblem::Condition {
        name: name.into(),
        response,
        response2,
    }
}
