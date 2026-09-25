use super::super::super::{CicsService, Run};
use super::{model, open};
use mainframe_env_execution_api::AuditDecision;
use mainframe_env_host_api::{
    AccessIntent, CicsDisposition, CicsRequest, CicsResponse, HostProblem,
};
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
    run: &mut Run,
    request: &CicsRequest,
    retention_tick: u64,
) -> Result<CicsResponse, HostProblem> {
    validate_request(request)?;
    let name_length = decimal(request, "NAMELENGTH")?;
    let value_length = decimal(request, "VALUELENGTH")?;
    if name_length <= 0 {
        return Err(condition("LENGERR", 22, 35));
    }
    if !(1..=32000).contains(&value_length) {
        return Err(condition("LENGERR", 22, 55));
    }
    let name_length = usize::try_from(name_length).map_err(|_| HostProblem::ResourceExhausted)?;
    let value_length = usize::try_from(value_length).map_err(|_| HostProblem::ResourceExhausted)?;
    let name_bytes = request.arguments["HTTPHEADER"].bytes();
    let value_bytes = request.arguments["VALUE"].bytes();
    if name_length > name_bytes.len() || name_length > 128 || value_length > value_bytes.len() {
        return Err(HostProblem::Malformed);
    }
    let name_bytes = &name_bytes[..name_length];
    let value_bytes = &value_bytes[..value_length];
    if !name_bytes.iter().all(|byte| {
        byte.is_ascii_alphanumeric()
            || matches!(
                byte,
                b'!' | b'#'
                    | b'$'
                    | b'%'
                    | b'&'
                    | b'\''
                    | b'*'
                    | b'+'
                    | b'-'
                    | b'.'
                    | b'^'
                    | b'_'
                    | b'`'
                    | b'|'
                    | b'~'
            )
    }) || value_bytes.iter().any(|byte| {
        *byte == b'\r' || *byte == b'\n' || *byte < 0x20 && *byte != b'\t' || *byte == 0x7f
    }) {
        return Err(condition("INVREQ", 16, 19));
    }
    let name = std::str::from_utf8(name_bytes)
        .map_err(|_| HostProblem::Malformed)?
        .to_string();
    let value = std::str::from_utf8(value_bytes)
        .map_err(|_| HostProblem::Malformed)?
        .to_string();
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
    let urimap = if let Some(token) = token {
        let state = service.lock()?;
        let session = state
            .web
            .sessions
            .get(&model::token_key(token))
            .filter(|session| {
                session.owner_execution == run.invocation.execution_id.as_str()
                    && session.owner_run_unit == run.invocation.run_unit_id.as_str()
                    && session.transaction == run.transaction
            })
            .ok_or_else(|| condition("NOTOPEN", 19, 27))?;
        if session.server_closed {
            return Err(condition("INVREQ", 16, 74));
        }
        if [
            "ARM-CORRELATOR",
            "CONNECTION",
            "CONTENT-LENGTH",
            "DATE",
            "EXPECT",
            "HOST",
            "SERVER",
            "TRANSFER-ENCODING",
            "USER-AGENT",
            "WWW-AUTHENTICATE",
        ]
        .iter()
        .any(|forbidden| name.eq_ignore_ascii_case(forbidden))
        {
            return Err(condition("INVREQ", 16, 19));
        }
        session.endpoint.urimap.clone()
    } else {
        let state = service.lock()?;
        if !state
            .web
            .inbound
            .contains_key(run.invocation.run_unit_id.as_str())
        {
            return Err(condition("INVREQ", 16, 1));
        }
        None
    };
    if let Some(resource) = urimap.as_deref() {
        service.authorize(run, "URIMAP", resource, AccessIntent::Read)?;
    }
    if run.invocation.cancellation_requested() {
        return Err(HostProblem::Cancelled);
    }
    let key = model::header_stage_key(run.invocation.run_unit_id.as_str(), token);
    let mut state = service.lock()?;
    let previous = state.web.pending_headers.get(&key).cloned();
    let mut next = previous.clone().unwrap_or(model::WebHeaderStage {
        owner_execution: run.invocation.execution_id.as_str().into(),
        owner_run_unit: run.invocation.run_unit_id.as_str().into(),
        transaction: run.transaction.clone(),
        client_token: token,
        headers: Vec::new(),
        version: 0,
    });
    if next.owner_execution != run.invocation.execution_id.as_str()
        || next.owner_run_unit != run.invocation.run_unit_id.as_str()
        || next.transaction != run.transaction
        || next.client_token != token
    {
        return Err(HostProblem::Unauthorized);
    }
    if next.headers.len() >= 128 {
        return Err(HostProblem::ResourceExhausted);
    }
    next.headers.push((name, value));
    next.version += 1;
    let payload = model::encode_header_stage(&next)?;
    let previous_bytes = previous
        .as_ref()
        .map(model::encode_header_stage)
        .transpose()?
        .map_or(0, |bytes| bytes.len());
    let next_bytes = state
        .web
        .bytes
        .checked_sub(previous_bytes)
        .and_then(|bytes| bytes.checked_add(payload.len()))
        .filter(|bytes| *bytes <= service.limits.max_web_bytes)
        .ok_or(HostProblem::ResourceExhausted)?;
    if previous.is_none() && state.web.pending_headers.len() >= service.limits.max_web_sessions {
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
                namespace: model::HEADER_NAMESPACE.into(),
                key: key.clone(),
                version: next.version,
                payload,
            },
            expected_version: previous.as_ref().map(|stage| stage.version),
        }),
        ProviderStateMutation::Put(open::replay_write(run, request, retention_tick, &response)?),
    ];
    if service.store.mutate_provider_states_atomic(writes).is_err() {
        return Err(HostProblem::UnknownOutcome);
    }
    state.web.bytes = next_bytes;
    state.web.pending_headers.insert(key, next);
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

fn validate_request(request: &CicsRequest) -> Result<(), HostProblem> {
    const ALLOWED: &[&str] = &[
        "HTTPHEADER",
        "NAMELENGTH",
        "SESSTOKEN",
        "VALUE",
        "VALUELENGTH",
        "RESP",
        "RESP2",
        "OPTION.NOHANDLE",
    ];
    if request.mutation.is_none()
        || request
            .arguments
            .keys()
            .any(|name| !ALLOWED.contains(&name.as_str()))
        || request.arguments.contains_key("RESP2") && !request.arguments.contains_key("RESP")
        || ["HTTPHEADER", "NAMELENGTH", "VALUE", "VALUELENGTH"]
            .iter()
            .any(|name| !request.arguments.contains_key(*name))
    {
        return Err(HostProblem::Malformed);
    }
    Ok(())
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
