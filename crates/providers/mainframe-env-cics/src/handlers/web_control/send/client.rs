use super::super::{model, open};
use super::*;
use mainframe_env_host_api::{
    AccessIntent, CicsDisposition, HostRequest, canonical_request_digest,
};
use mainframe_env_store_api::{ProviderStateMutation, ProviderStateRecord, ProviderStateWrite};
use sha2::{Digest, Sha256};
use std::sync::atomic::Ordering;

pub(super) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
    retention_tick: u64,
    input: SendInput,
) -> Result<CicsResponse, HostProblem> {
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
        if previous.as_ref().is_some_and(|response| !response.received) {
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
        cursor: 0,
        received: false,
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
