use super::super::super::{CicsService, Run};
use super::{model, open, read};
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
    if state.web.browses.contains_key(&key) {
        return Err(condition(
            "ILLOGIC",
            21,
            if kind == "HTTPHEADER" { 10 } else { 5 },
        ));
    }
    let (entries, token) = if let Some(value) = request.arguments.get("SESSTOKEN") {
        let token: [u8; 8] = value
            .bytes()
            .try_into()
            .map_err(|_| condition("NOTOPEN", 19, 27))?;
        state
            .web
            .sessions
            .get(&model::token_key(token))
            .filter(|session| {
                session.owner_execution == run.invocation.execution_id.as_str()
                    && session.owner_run_unit == run.invocation.run_unit_id.as_str()
                    && session.transaction == run.transaction
            })
            .ok_or_else(|| condition("NOTOPEN", 19, 27))?;
        return Err(condition("INVREQ", 16, 43));
    } else {
        let inbound = state
            .web
            .inbound
            .get(run.invocation.run_unit_id.as_str())
            .ok_or_else(|| condition("INVREQ", 16, 1))?;
        if !inbound.http {
            return Err(condition("INVREQ", 16, 3));
        }
        let entries = match kind {
            "HTTPHEADER" => inbound
                .headers
                .iter()
                .map(|(name, value)| (name.as_bytes().to_vec(), value.as_bytes().to_vec()))
                .collect(),
            "QUERYPARM" => read::url_encoded_pairs(inbound.query.as_bytes())?,
            "FORMFIELD" => {
                let bytes = if inbound.method == "GET" {
                    inbound.query.as_bytes()
                } else {
                    let form = inbound.headers.iter().any(|(name, value)| {
                        name.eq_ignore_ascii_case("Content-Type")
                            && value
                                .to_ascii_lowercase()
                                .starts_with("application/x-www-form-urlencoded")
                    });
                    if !form {
                        return Err(condition("INVREQ", 16, 153));
                    }
                    inbound.body.as_slice()
                };
                read::url_encoded_pairs(bytes)?
            }
            _ => return Err(HostProblem::InfrastructureFailure),
        };
        (entries, None)
    };
    let entries: Vec<(Vec<u8>, Vec<u8>)> = entries;
    if entries.is_empty() {
        return Err(condition(
            "INVREQ",
            16,
            if kind == "HTTPHEADER" { 43 } else { 13 },
        ));
    }
    let cursor = if let Some(start) = request.arguments.get("BROWSESTARTNAME") {
        let length = decimal(request, "NAMELENGTH")?;
        if length <= 0 {
            return Err(condition("LENGERR", 22, 1));
        }
        let length = usize::try_from(length).map_err(|_| HostProblem::ResourceExhausted)?;
        if length > start.bytes().len() || length > 128 {
            return Err(HostProblem::Malformed);
        }
        entries
            .iter()
            .position(|(name, _)| name.eq_ignore_ascii_case(&start.bytes()[..length]))
            .ok_or_else(|| condition("NOTFND", 13, 1))?
    } else {
        0
    };
    let browse = model::WebBrowse {
        owner_execution: run.invocation.execution_id.as_str().into(),
        owner_run_unit: run.invocation.run_unit_id.as_str().into(),
        transaction: run.transaction.clone(),
        kind: kind.into(),
        client_token: token,
        entries,
        cursor,
        version: 1,
    };
    let payload = model::encode_browse(&browse)?;
    if state.web.browses.len() >= service.limits.max_web_sessions
        || state
            .web
            .bytes
            .checked_add(payload.len())
            .is_none_or(|total| total > service.limits.max_web_bytes)
    {
        return Err(HostProblem::ResourceExhausted);
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
        ProviderStateMutation::Put(ProviderStateWrite {
            record: ProviderStateRecord {
                namespace: model::BROWSE_NAMESPACE.into(),
                key: key.clone(),
                version: 1,
                payload: payload.clone(),
            },
            expected_version: None,
        }),
        ProviderStateMutation::Put(open::replay_write(run, request, retention_tick, &response)?),
    ];
    if service.store.mutate_provider_states_atomic(writes).is_err() {
        return Err(HostProblem::UnknownOutcome);
    }
    state.web.bytes += payload.len();
    state.web.browses.insert(key, browse);
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
        "BROWSESTARTNAME",
        "NAMELENGTH",
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
        || request.arguments.contains_key("BROWSESTARTNAME")
            != request.arguments.contains_key("NAMELENGTH")
    {
        return Err(HostProblem::Malformed);
    }
    let selected = ["HTTPHEADER", "QUERYPARM", "FORMFIELD"]
        .into_iter()
        .filter(|kind| request.arguments.contains_key(&format!("OPTION.{kind}")))
        .collect::<Vec<_>>();
    if selected.len() != 1
        || selected[0] == "HTTPHEADER" && request.arguments.contains_key("BROWSESTARTNAME")
        || selected[0] != "HTTPHEADER" && request.arguments.contains_key("SESSTOKEN")
    {
        return Err(HostProblem::Malformed);
    }
    Ok(selected[0])
}

fn decimal(request: &CicsRequest, name: &str) -> Result<i64, HostProblem> {
    let value = request.arguments.get(name).ok_or(HostProblem::Malformed)?;
    if value.schema() != "mainframe-env.cics.decimal@1" {
        return Err(HostProblem::Malformed);
    }
    std::str::from_utf8(value.bytes())
        .map_err(|_| HostProblem::Malformed)?
        .parse()
        .map_err(|_| HostProblem::Malformed)
}

fn condition(name: &str, response: i32, response2: i32) -> HostProblem {
    HostProblem::Condition {
        name: name.into(),
        response,
        response2,
    }
}
