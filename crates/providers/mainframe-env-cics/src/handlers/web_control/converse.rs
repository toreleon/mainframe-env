use super::super::super::{CicsService, Run, bounded, decimal_payload};
use super::{model, open, receive, send};
use mainframe_env_execution_api::AuditDecision;
use mainframe_env_host_api::{
    AccessIntent, CicsDisposition, CicsRequest, CicsResponse, HostProblem, HostRequest,
    canonical_request_digest,
};
use mainframe_env_store_api::{ProviderStateMutation, ProviderStateRecord, ProviderStateWrite};
use sha2::{Digest, Sha256};
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

fn parse_request(
    service: &CicsService,
    run: &Run,
    request: &CicsRequest,
) -> Result<(send::SendInput, usize, Option<usize>), HostProblem> {
    const SEND: &[&str] = &[
        "SESSTOKEN",
        "METHOD",
        "PATH",
        "PATHLENGTH",
        "URIMAP",
        "QUERYSTRING",
        "QUERYSTRLEN",
        "FROM",
        "FROMLENGTH",
        "DOCTOKEN",
        "MEDIATYPE",
        "CLOSESTATUS",
        "RESP",
        "RESP2",
        "OPTION.NOHANDLE",
    ];
    const RESULT: &[&str] = &[
        "INTO",
        "INTO.MAXLENGTH",
        "TOLENGTH",
        "MAXLENGTH",
        "STATUSCODE",
        "STATUSTEXT",
        "STATUSTEXT.MAXLENGTH",
        "STATUSLEN",
        "BODYCHARSET",
        "OPTION.NOTRUNCATE",
        "OPTION.NOCLICONVERT",
    ];
    if request.mutation.is_none()
        || request
            .arguments
            .keys()
            .any(|name| !SEND.contains(&name.as_str()) && !RESULT.contains(&name.as_str()))
        || !["SESSTOKEN", "METHOD", "INTO", "TOLENGTH", "MAXLENGTH"]
            .iter()
            .all(|name| request.arguments.contains_key(*name))
        || request.arguments.contains_key("STATUSTEXT")
            != request.arguments.contains_key("STATUSLEN")
    {
        return Err(HostProblem::Malformed);
    }
    let maximum = decimal(request, "MAXLENGTH")?;
    if maximum <= 0 {
        return Err(condition("LENGERR", 22, 16));
    }
    let maximum = usize::try_from(maximum).map_err(|_| HostProblem::ResourceExhausted)?;
    if maximum > service.limits.max_web_bytes {
        return Err(HostProblem::ResourceExhausted);
    }
    if let Some(declared) = request.arguments.get("INTO.MAXLENGTH") {
        let declared = std::str::from_utf8(declared.bytes())
            .map_err(|_| HostProblem::Malformed)?
            .parse::<usize>()
            .map_err(|_| HostProblem::Malformed)?;
        if maximum > declared {
            return Err(HostProblem::Malformed);
        }
    }
    let status_capacity = if request.arguments.contains_key("STATUSLEN") {
        let value = decimal(request, "STATUSLEN")?;
        if value <= 0 {
            return Err(condition("LENGERR", 22, 59));
        }
        let capacity = usize::try_from(value).map_err(|_| HostProblem::ResourceExhausted)?;
        if let Some(declared) = request.arguments.get("STATUSTEXT.MAXLENGTH") {
            let declared = std::str::from_utf8(declared.bytes())
                .map_err(|_| HostProblem::Malformed)?
                .parse::<usize>()
                .map_err(|_| HostProblem::Malformed)?;
            if capacity > declared {
                return Err(HostProblem::Malformed);
            }
        }
        Some(capacity)
    } else {
        None
    };
    let mut send_request = request.clone();
    send_request
        .arguments
        .retain(|name, _| SEND.contains(&name.as_str()));
    let input = send::parse(service, run, &send_request)?;
    if input.token.is_none() {
        return Err(HostProblem::Malformed);
    }
    Ok((input, maximum, status_capacity))
}

fn result_from_received(
    service: &CicsService,
    run: &Run,
    request: &CicsRequest,
    received: &model::CicsWebResponse,
    maximum: usize,
    status_capacity: Option<usize>,
    code_page: u16,
) -> Result<(CicsResponse, usize), HostProblem> {
    let media = received
        .headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("Content-Type"))
        .map_or("", |(_, value)| value.as_str());
    let (body, consumed) = receive::chunk(
        &received.body,
        maximum,
        media,
        Some(code_page),
        request.arguments.contains_key("OPTION.NOCLICONVERT"),
        service.limits.max_web_bytes,
    )?;
    let remaining = consumed < received.body.len();
    let keep = request.arguments.contains_key("OPTION.NOTRUNCATE");
    let cursor = if remaining && !keep {
        received.body.len()
    } else {
        consumed
    };
    let mut response = service.response(
        run,
        CicsDisposition::Complete,
        if remaining { "LENGERR" } else { "NORMAL" },
        if remaining { 22 } else { 0 },
        if remaining {
            if keep { 36 } else { 57 }
        } else {
            0
        },
        None,
        None,
        Vec::new(),
    )?;
    response
        .outputs
        .insert("INTO".into(), bounded(body.clone())?);
    response
        .outputs
        .insert("TOLENGTH".into(), decimal_payload(body.len() as i64)?);
    if request.arguments.contains_key("STATUSCODE") {
        response.outputs.insert(
            "STATUSCODE".into(),
            decimal_payload(received.status.into())?,
        );
    }
    if let Some(capacity) = status_capacity {
        let value = received.reason.as_bytes();
        response.outputs.insert(
            "STATUSTEXT".into(),
            bounded(value[..value.len().min(capacity)].to_vec())?,
        );
        response
            .outputs
            .insert("STATUSLEN".into(), decimal_payload(value.len() as i64)?);
        if value.len() > capacity && response.response == 0 {
            response.condition = "LENGERR".into();
            response.response = 22;
            response.response2 = 58;
        }
    }
    if request.arguments.contains_key("MEDIATYPE") {
        response.outputs.insert(
            "MEDIATYPE".into(),
            bounded(
                media
                    .split(';')
                    .next()
                    .unwrap_or("")
                    .trim()
                    .as_bytes()
                    .iter()
                    .take(56)
                    .copied()
                    .collect(),
            )?,
        );
    }
    if request.arguments.contains_key("BODYCHARSET") {
        let charset = media
            .split(';')
            .skip(1)
            .find_map(|part| part.trim().strip_prefix("charset="))
            .unwrap_or("");
        response.outputs.insert(
            "BODYCHARSET".into(),
            bounded(charset.as_bytes().iter().take(40).copied().collect())?,
        );
    }
    Ok((response, cursor))
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

fn invoke_inner(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
    retention_tick: u64,
) -> Result<CicsResponse, HostProblem> {
    let (input, maximum, status_capacity) = parse_request(service, run, request)?;
    let token = input.token.ok_or(HostProblem::InfrastructureFailure)?;
    let session_key = model::token_key(token);
    let stage_key = model::header_stage_key(run.invocation.run_unit_id.as_str(), Some(token));
    let (session, stage, previous, transport, path, urimap) = {
        let state = service.lock()?;
        let session = state
            .web
            .sessions
            .get(&session_key)
            .filter(|session| {
                session.owner_execution == run.invocation.execution_id.as_str()
                    && session.owner_run_unit == run.invocation.run_unit_id.as_str()
                    && session.transaction == run.transaction
            })
            .cloned()
            .ok_or_else(|| condition("NOTOPEN", 19, 27))?;
        if session.server_closed {
            return Err(condition("INVREQ", 16, 74));
        }
        let previous = state.web.client_responses.get(&session_key).cloned();
        if previous.as_ref().is_some_and(|response| {
            !response.received || response.cursor < response.response.body.len()
        }) {
            return Err(condition("INVREQ", 16, 79));
        }
        let stage = state.web.pending_headers.get(&stage_key).cloned();
        let transport = state
            .web
            .transport
            .clone()
            .ok_or(HostProblem::ProviderFailure)?;
        let (path, urimap) = if let Some(name) = input.urimap.as_ref() {
            let name = name.to_ascii_uppercase();
            let definition = state
                .web
                .urimaps
                .get(&name)
                .ok_or_else(|| condition("NOTFND", 13, 61))?;
            if !definition.enabled {
                return Err(condition("INVREQ", 16, 63));
            }
            if definition.host != session.endpoint.host {
                return Err(condition("INVREQ", 16, 64));
            }
            (definition.path.clone(), Some(name))
        } else if let Some(path) = input.path.as_ref() {
            (path.clone(), session.endpoint.urimap.clone())
        } else if session.endpoint.urimap.is_some() {
            (
                session.endpoint.default_path.clone(),
                session.endpoint.urimap.clone(),
            )
        } else {
            return Err(condition("INVREQ", 16, 49));
        };
        (session, stage, previous, transport, path, urimap)
    };
    if let Some(name) = urimap.as_deref() {
        service.authorize(run, "URIMAP", name, AccessIntent::Read)?;
    }
    let path_resource = format!(
        "PATH.{:x}",
        Sha256::digest(format!("{}{}", session.endpoint.host, path).as_bytes())
    );
    service.authorize(run, "WEBPATH", &path_resource, AccessIntent::Read)?;
    if run.invocation.cancellation_requested() {
        return Err(HostProblem::Cancelled);
    }
    let mut headers = stage
        .as_ref()
        .map_or_else(Vec::new, |stage| stage.headers.clone());
    if let Some(media_type) = input.media_type {
        if !headers
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case("Content-Type"))
        {
            headers.push(("Content-Type".into(), media_type));
        }
    }
    if !input.body.is_empty()
        && !headers
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case("Content-Type"))
    {
        return Err(condition("INVREQ", 16, 76));
    }
    if !input.body.is_empty()
        && headers.iter().any(|(name, value)| {
            name.eq_ignore_ascii_case("Content-Type")
                && value.to_ascii_lowercase().starts_with("text/")
        })
        && !request.arguments.contains_key("OPTION.NOCLICONVERT")
    {
        return Err(condition("INVREQ", 16, 46));
    }
    if input.close {
        headers.push(("Connection".into(), "close".into()));
    }
    if headers.len() > service.limits.max_web_headers {
        return Err(HostProblem::ResourceExhausted);
    }
    let outbound = model::CicsWebRequest {
        method: input.method.ok_or(HostProblem::InfrastructureFailure)?,
        path,
        query: input.query,
        headers,
        body: input.body,
        close: input.close,
    };
    let mutation = request
        .mutation
        .as_ref()
        .ok_or(HostProblem::MissingIdempotency)?;
    let digest = canonical_request_digest(&HostRequest::Cics(request.clone()))
        .map_err(|_| HostProblem::ResourceExhausted)?;
    let mut marker = b"MECWDP01".to_vec();
    marker.extend_from_slice(&digest);
    marker.extend_from_slice(run.invocation.execution_id.as_str().as_bytes());
    marker.push(0);
    marker.extend_from_slice(run.invocation.run_unit_id.as_str().as_bytes());
    if let Some(record) = service
        .store
        .get_provider_state(model::DISPATCH_NAMESPACE, mutation.idempotency_key.as_str())
        .map_err(|_| HostProblem::UnknownOutcome)?
    {
        return if record.payload == marker {
            Err(HostProblem::UnknownOutcome)
        } else {
            Err(HostProblem::IdempotencyConflict)
        };
    }
    service
        .store
        .put_provider_state(
            ProviderStateRecord {
                namespace: model::DISPATCH_NAMESPACE.into(),
                key: mutation.idempotency_key.as_str().into(),
                version: 1,
                payload: marker,
            },
            None,
        )
        .map_err(|_| HostProblem::UnknownOutcome)?;
    let received = transport
        .exchange(&session.endpoint, token, &outbound, &run.invocation)
        .map_err(|_| HostProblem::UnknownOutcome)?;
    if received.version.major == 0
        || !(100..=599).contains(&received.status)
        || received.reason.len() > 256
        || received.headers.len() > service.limits.max_web_headers
        || received.body.len() > service.limits.max_web_bytes
        || received.headers.iter().any(|(name, value)| {
            name.is_empty()
                || name.len() > 128
                || value.len() > 32000
                || name
                    .bytes()
                    .any(|byte| !byte.is_ascii_alphanumeric() && byte != b'-')
                || value.bytes().any(|byte| matches!(byte, b'\r' | b'\n'))
        })
    {
        return Err(HostProblem::UnknownOutcome);
    }
    let (response, cursor) = result_from_received(
        service,
        run,
        request,
        &received,
        maximum,
        status_capacity,
        session.endpoint.code_page,
    )
    .map_err(|_| HostProblem::UnknownOutcome)?;
    let mut state = service.lock()?;
    if state.web.sessions.get(&session_key) != Some(&session)
        || state.web.pending_headers.get(&stage_key) != stage.as_ref()
        || state.web.client_responses.get(&session_key) != previous.as_ref()
    {
        return Err(HostProblem::UnknownOutcome);
    }
    let client_response = model::WebClientResponseState {
        owner_execution: run.invocation.execution_id.as_str().into(),
        owner_run_unit: run.invocation.run_unit_id.as_str().into(),
        transaction: run.transaction.clone(),
        token,
        response: received,
        cursor,
        received: true,
        version: previous.as_ref().map_or(1, |prior| prior.version + 1),
    };
    let payload = model::encode_client_response(&client_response)?;
    let previous_bytes = previous
        .as_ref()
        .map(model::encode_client_response)
        .transpose()?
        .map_or(0, |bytes| bytes.len());
    let stage_bytes = stage
        .as_ref()
        .map(model::encode_header_stage)
        .transpose()?
        .map_or(0, |bytes| bytes.len());
    let total = state
        .web
        .bytes
        .checked_sub(previous_bytes)
        .and_then(|bytes| bytes.checked_sub(stage_bytes))
        .and_then(|bytes| bytes.checked_add(payload.len()))
        .filter(|total| *total <= service.limits.max_web_bytes)
        .ok_or(HostProblem::UnknownOutcome)?;
    let close = input.close
        || client_response
            .response
            .headers
            .iter()
            .any(|(name, value)| {
                name.eq_ignore_ascii_case("Connection") && value.eq_ignore_ascii_case("close")
            });
    let mut next_session = session.clone();
    if close {
        next_session.server_closed = true;
        next_session.version += 1;
    }
    let mut writes = vec![
        ProviderStateMutation::Put(ProviderStateWrite {
            record: ProviderStateRecord {
                namespace: model::CLIENT_RESPONSE_NAMESPACE.into(),
                key: session_key.clone(),
                version: client_response.version,
                payload,
            },
            expected_version: previous.as_ref().map(|prior| prior.version),
        }),
        ProviderStateMutation::Delete {
            namespace: model::DISPATCH_NAMESPACE.into(),
            key: mutation.idempotency_key.as_str().into(),
            expected_version: 1,
        },
        ProviderStateMutation::Put(open::replay_write(run, request, retention_tick, &response)?),
    ];
    if let Some(stage) = &stage {
        writes.push(ProviderStateMutation::Delete {
            namespace: model::HEADER_NAMESPACE.into(),
            key: stage_key.clone(),
            expected_version: stage.version,
        });
    }
    if close {
        writes.push(ProviderStateMutation::Put(ProviderStateWrite {
            record: ProviderStateRecord {
                namespace: model::SESSION_NAMESPACE.into(),
                key: session_key.clone(),
                version: next_session.version,
                payload: model::encode_session(&next_session)?,
            },
            expected_version: Some(session.version),
        }));
    }
    if service.store.mutate_provider_states_atomic(writes).is_err() {
        return Err(HostProblem::UnknownOutcome);
    }
    state.web.bytes = total;
    state
        .web
        .client_responses
        .insert(session_key.clone(), client_response);
    state.web.pending_headers.remove(&stage_key);
    if close {
        state.web.sessions.insert(session_key, next_session);
    }
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
