use super::super::{model, open};
use super::*;
use mainframe_env_host_api::{AccessIntent, CicsDisposition};
use mainframe_env_store_api::{ProviderStateMutation, ProviderStateRecord, ProviderStateWrite};
use std::sync::atomic::Ordering;

pub(super) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
    retention_tick: u64,
    input: SendInput,
) -> Result<CicsResponse, HostProblem> {
    if matches!(input.status, 204 | 205 | 304) && !input.body.is_empty() {
        return Err(condition("INVREQ", 16, 72));
    }
    let urimap = {
        let state = service.lock()?;
        let inbound = state
            .web
            .inbound
            .get(run.invocation.run_unit_id.as_str())
            .ok_or_else(|| condition("INVREQ", 16, 1))?;
        if !inbound.http {
            return Err(condition("INVREQ", 16, 3));
        }
        inbound.urimap.clone()
    };
    if let Some(name) = urimap.as_deref() {
        service.authorize(run, "URIMAP", name, AccessIntent::Read)?;
    }
    if run.invocation.cancellation_requested() {
        return Err(HostProblem::Cancelled);
    }
    let mut state = service.lock()?;
    let key = run.invocation.run_unit_id.as_str().to_string();
    let prior = state.web.server_responses.get(&key).cloned();
    if prior.as_ref().is_some_and(|reply| !reply.response.eventual) {
        return Err(condition("INVREQ", 16, 75));
    }
    let header_key = model::header_stage_key(&key, None);
    let staged = state.web.pending_headers.get(&header_key).cloned();
    let mut headers = staged
        .as_ref()
        .map_or_else(Vec::new, |stage| stage.headers.clone());
    if let Some(media_type) = input.media_type
        && !headers
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case("Content-Type"))
    {
        headers.push(("Content-Type".into(), media_type));
    }
    if input.close {
        headers.push(("Connection".into(), "close".into()));
    }
    if headers.len() > service.limits.max_web_headers {
        return Err(HostProblem::ResourceExhausted);
    }
    let reply = model::WebServerReply {
        owner_execution: run.invocation.execution_id.as_str().into(),
        owner_run_unit: key.clone(),
        transaction: run.transaction.clone(),
        response: model::CicsWebServerResponse {
            status: input.status,
            reason: input.reason,
            headers,
            body: input.body,
            eventual: input.eventual,
            close: input.close,
            document_token: input.eventual.then_some(input.document_token).flatten(),
        },
        version: prior.as_ref().map_or(1, |reply| reply.version + 1),
    };
    let payload = model::encode_server_reply(&reply)?;
    let previous_bytes = prior
        .as_ref()
        .map(model::encode_server_reply)
        .transpose()?
        .map_or(0, |bytes| bytes.len());
    let header_bytes = staged
        .as_ref()
        .map(model::encode_header_stage)
        .transpose()?
        .map_or(0, |bytes| bytes.len());
    let total = state
        .web
        .bytes
        .checked_sub(previous_bytes)
        .and_then(|bytes| bytes.checked_sub(header_bytes))
        .and_then(|bytes| bytes.checked_add(payload.len()))
        .filter(|total| *total <= service.limits.max_web_bytes)
        .ok_or(HostProblem::ResourceExhausted)?;
    if prior.is_none() && state.web.server_responses.len() >= service.limits.max_web_sessions {
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
    let mut writes = vec![
        ProviderStateMutation::Put(ProviderStateWrite {
            record: ProviderStateRecord {
                namespace: model::SERVER_RESPONSE_NAMESPACE.into(),
                key: key.clone(),
                version: reply.version,
                payload,
            },
            expected_version: prior.as_ref().map(|reply| reply.version),
        }),
        ProviderStateMutation::Put(open::replay_write(run, request, retention_tick, &response)?),
    ];
    if let Some(staged) = &staged {
        writes.push(ProviderStateMutation::Delete {
            namespace: model::HEADER_NAMESPACE.into(),
            key: header_key.clone(),
            expected_version: staged.version,
        });
    }
    if service.store.mutate_provider_states_atomic(writes).is_err() {
        return Err(HostProblem::UnknownOutcome);
    }
    state.web.bytes = total;
    state.web.server_responses.insert(key, reply);
    state.web.pending_headers.remove(&header_key);
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
