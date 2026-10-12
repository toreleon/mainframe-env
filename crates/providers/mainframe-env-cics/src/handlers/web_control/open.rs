use super::super::super::{
    CicsEffectReplay, CicsService, Run, bounded, cics_effect_replay_binding_digest,
    decimal_payload, encode_cics_effect_replay,
};
use super::model::{self, CicsWebEndpoint, WebClientSession};
use mainframe_env_execution_api::{AuditDecision, AuditRecord, InvocationLimits};
use mainframe_env_host_api::{
    AccessIntent, CicsDisposition, CicsRequest, CicsResponse, HostProblem, HostRequest, HostResult,
    canonical_audit_resource_digest, canonical_request_digest, canonical_result_digest,
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
    validate_request(request)?;
    let (endpoint, pooled, transport) = endpoint(service, run, request)?;
    if run.invocation.cancellation_requested() {
        return Err(HostProblem::Cancelled);
    }
    let opened = transport.open(&endpoint, &run.invocation);
    audit_transport(service, run, request, &opened)?;
    let version = opened.map_err(transport_problem)?;
    if version.major == 0 {
        return Err(HostProblem::Condition {
            name: "INVREQ".into(),
            response: 16,
            response2: 67,
        });
    }
    let token = token(run, request)?;
    if run.invocation.cancellation_requested() || expired_after_dispatch(service, run)? {
        let _ = transport.release(&endpoint, token, false, &run.invocation);
        audit_web_decision(service, run, request, AuditDecision::UnknownOutcome)?;
        return Err(HostProblem::UnknownOutcome);
    }
    let mut state = service.lock()?;
    if state.web.sessions.len() >= service.limits.max_web_sessions {
        let _ = transport.release(&endpoint, token, false, &run.invocation);
        return Err(HostProblem::ResourceExhausted);
    }
    let key = model::token_key(token);
    if state.web.sessions.contains_key(&key) {
        let _ = transport.release(&endpoint, token, false, &run.invocation);
        return Err(HostProblem::IdempotencyConflict);
    }
    let session = WebClientSession {
        token,
        owner_execution: run.invocation.execution_id.as_str().into(),
        owner_run_unit: run.invocation.run_unit_id.as_str().into(),
        transaction: run.transaction.clone(),
        endpoint: endpoint.clone(),
        pooled,
        http_version: version,
        server_closed: false,
        version: 1,
    };
    let payload = model::encode_session(&session)?;
    if state
        .web
        .bytes
        .checked_add(payload.len())
        .is_none_or(|total| total > service.limits.max_web_bytes)
    {
        let _ = transport.release(&endpoint, token, false, &run.invocation);
        return Err(HostProblem::ResourceExhausted);
    }
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
        .insert("SESSTOKEN".into(), bounded(token.to_vec())?);
    for (name, value) in [("HTTPVNUM", version.major), ("HTTPRNUM", version.minor)] {
        if request.arguments.contains_key(name) {
            response
                .outputs
                .insert(name.into(), decimal_payload(i64::from(value))?);
        }
    }
    let writes = vec![
        ProviderStateMutation::Put(ProviderStateWrite {
            record: ProviderStateRecord {
                namespace: model::SESSION_NAMESPACE.into(),
                key: key.clone(),
                version: 1,
                payload: payload.clone(),
            },
            expected_version: None,
        }),
        ProviderStateMutation::Put(replay_write(run, request, retention_tick, &response)?),
    ];
    if service.store.mutate_provider_states_atomic(writes).is_err() {
        let _ = transport.release(&endpoint, token, false, &run.invocation);
        audit_web_decision(service, run, request, AuditDecision::UnknownOutcome)?;
        return Err(HostProblem::UnknownOutcome);
    }
    state.web.bytes += payload.len();
    state.web.sessions.insert(key, session);
    drop(state);
    if service
        .replay_unknown_after_persist
        .swap(false, Ordering::SeqCst)
    {
        audit_web_decision(service, run, request, AuditDecision::UnknownOutcome)?;
        return Err(HostProblem::UnknownOutcome);
    }
    Ok(response)
}

fn endpoint(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<
    (
        CicsWebEndpoint,
        bool,
        std::sync::Arc<dyn super::CicsWebTransport>,
    ),
    HostProblem,
> {
    let state = service.lock()?;
    let transport = state
        .web
        .transport
        .clone()
        .ok_or(HostProblem::ProviderFailure)?;
    let (mut endpoint, pooled) = if let Some(value) = request.arguments.get("URIMAP") {
        let name = text(value.bytes(), 8)?.to_ascii_uppercase();
        let definition = state
            .web
            .urimaps
            .get(&name)
            .ok_or_else(|| condition("NOTFND", 13, 61))?;
        if !definition.enabled {
            return Err(condition("INVREQ", 16, 63));
        }
        let scheme = definition.scheme.clone();
        let port = effective_port(&scheme, definition.port)?;
        (
            CicsWebEndpoint {
                scheme,
                host: definition.host.clone(),
                port,
                default_path: definition.path.clone(),
                urimap: Some(name),
                code_page: 37,
                certificate: definition.certificate.clone(),
            },
            definition.pooled,
        )
    } else {
        let host = request
            .arguments
            .get("HOST")
            .ok_or(HostProblem::Malformed)?
            .bytes();
        let length = decimal(request, "HOSTLENGTH")?;
        let length = usize::try_from(length).map_err(|_| condition("LENGERR", 22, 21))?;
        if length == 0 || length > host.len() || length > 255 {
            return Err(condition("LENGERR", 22, 21));
        }
        let host = text(&host[..length], 255)?.to_ascii_lowercase();
        if !valid_host(&host) {
            return Err(condition("INVREQ", 16, 48));
        }
        let scheme = text(
            request
                .arguments
                .get("SCHEME")
                .ok_or(HostProblem::Malformed)?
                .bytes(),
            5,
        )?
        .to_ascii_uppercase();
        let port = request
            .arguments
            .get("PORTNUMBER")
            .map(|_| decimal(request, "PORTNUMBER"))
            .transpose()?
            .unwrap_or(0);
        let port = if port == 0 {
            0
        } else {
            u16::try_from(port).map_err(|_| condition("INVREQ", 16, 138))?
        };
        let port = effective_port(&scheme, port)?;
        let certificate = request
            .arguments
            .get("CERTIFICATE")
            .map(|value| text(value.bytes(), 32))
            .transpose()?;
        if certificate.is_some() && scheme != "HTTPS" {
            return Err(condition("INVREQ", 16, 23));
        }
        (
            CicsWebEndpoint {
                scheme,
                host,
                port,
                default_path: "/".into(),
                urimap: None,
                code_page: 37,
                certificate,
            },
            false,
        )
    };
    drop(state);
    if let Some(value) = request.arguments.get("CODEPAGE") {
        let code_page = text(value.bytes(), 8)?;
        endpoint.code_page = code_page
            .trim()
            .parse()
            .map_err(|_| condition("INVREQ", 16, 14))?;
        if endpoint.code_page != 37 {
            return Err(condition("INVREQ", 16, 14));
        }
    }
    if let Some(name) = endpoint.urimap.as_deref() {
        service.authorize(run, "URIMAP", name, AccessIntent::Read)?;
    }
    Ok((endpoint, pooled, transport))
}

fn validate_request(request: &CicsRequest) -> Result<(), HostProblem> {
    let allowed = [
        "HOST",
        "HOSTLENGTH",
        "PORTNUMBER",
        "SCHEME",
        "URIMAP",
        "CERTIFICATE",
        "CODEPAGE",
        "SESSTOKEN",
        "HTTPVNUM",
        "HTTPRNUM",
        "RESP",
        "RESP2",
        "OPTION.NOHANDLE",
    ];
    if request.mutation.is_none()
        || request
            .arguments
            .keys()
            .any(|name| !allowed.contains(&name.as_str()))
        || !request.arguments.contains_key("SESSTOKEN")
        || request.arguments.contains_key("RESP2") && !request.arguments.contains_key("RESP")
    {
        return Err(HostProblem::Malformed);
    }
    let urimap = request.arguments.contains_key("URIMAP");
    let host = request.arguments.contains_key("HOST");
    if urimap == host
        || urimap
            && ["HOSTLENGTH", "PORTNUMBER", "SCHEME", "CERTIFICATE"]
                .iter()
                .any(|name| request.arguments.contains_key(*name))
        || host
            && (!request.arguments.contains_key("HOSTLENGTH")
                || !request.arguments.contains_key("SCHEME"))
    {
        return Err(HostProblem::Malformed);
    }
    Ok(())
}

fn token(run: &Run, request: &CicsRequest) -> Result<[u8; 8], HostProblem> {
    let mutation = request
        .mutation
        .as_ref()
        .ok_or(HostProblem::MissingIdempotency)?;
    let mut digest = Sha256::new();
    digest.update(b"mainframe-env.cics-web-session-token@1\0");
    digest.update(run.invocation.execution_id.as_str().as_bytes());
    digest.update(b"\0");
    digest.update(run.invocation.run_unit_id.as_str().as_bytes());
    digest.update(b"\0");
    digest.update(mutation.idempotency_key.as_str().as_bytes());
    digest.finalize()[..8]
        .try_into()
        .map_err(|_| HostProblem::InfrastructureFailure)
}

pub(super) fn replay_write(
    run: &Run,
    request: &CicsRequest,
    retention_tick: u64,
    response: &CicsResponse,
) -> Result<ProviderStateWrite, HostProblem> {
    let mutation = request
        .mutation
        .as_ref()
        .ok_or(HostProblem::MissingIdempotency)?;
    let request_digest = canonical_request_digest(&HostRequest::Cics(request.clone()))
        .map_err(|_| HostProblem::ResourceExhausted)?;
    let result_digest = canonical_result_digest(&Ok(HostResult::Cics(response.clone())))
        .map_err(|_| HostProblem::InfrastructureFailure)?;
    let mut replay = CicsEffectReplay {
        effect_key: Some(mutation.idempotency_key.as_str().into()),
        owner_execution: Some(run.invocation.execution_id.as_str().into()),
        owner_run_unit: Some(run.invocation.run_unit_id.as_str().into()),
        sequence: Some(mutation.sequence),
        deadline_tick: Some(retention_tick),
        resolution_tick: None,
        request_digest,
        result_digest: Some(result_digest),
        binding_digest: None,
        response: response.clone(),
    };
    replay.binding_digest = Some(cics_effect_replay_binding_digest(&replay));
    Ok(ProviderStateWrite {
        record: ProviderStateRecord {
            namespace: "cics-effect-replay-v1".into(),
            key: mutation.idempotency_key.as_str().into(),
            version: 1,
            payload: encode_cics_effect_replay(&replay)?,
        },
        expected_version: None,
    })
}

pub(super) fn expired_after_dispatch(
    service: &CicsService,
    run: &Run,
) -> Result<bool, HostProblem> {
    match service.replay_clock.as_ref() {
        Some(clock) => Ok(clock.now_tick()? >= run.invocation.deadline_tick),
        None => Ok(false),
    }
}

pub(super) fn transport_problem(problem: HostProblem) -> HostProblem {
    match problem {
        HostProblem::TimedOut => condition("TIMEDOUT", 124, 62),
        HostProblem::Unauthorized => condition("NOTAUTH", 70, 100),
        HostProblem::Cancelled | HostProblem::UnknownOutcome => problem,
        _ => condition("IOERR", 17, 42),
    }
}

fn audit_transport(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
    result: &Result<super::CicsWebVersion, HostProblem>,
) -> Result<(), HostProblem> {
    let decision = match result {
        Ok(_) => AuditDecision::Success,
        Err(HostProblem::Cancelled) => AuditDecision::Cancelled,
        Err(HostProblem::TimedOut) => AuditDecision::TimedOut,
        Err(HostProblem::Unauthorized) => AuditDecision::Deny,
        Err(HostProblem::UnknownOutcome) => AuditDecision::UnknownOutcome,
        Err(HostProblem::InfrastructureFailure) => AuditDecision::InfrastructureFailure,
        Err(_) => AuditDecision::ProviderFailure,
    };
    audit_web_decision(service, run, request, decision)
}

pub(super) fn audit_web_decision(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
    decision: AuditDecision,
) -> Result<(), HostProblem> {
    run.host_sequence = run
        .host_sequence
        .checked_add(1)
        .ok_or(HostProblem::ResourceExhausted)?;
    let host_request = HostRequest::Cics(request.clone());
    service
        .store
        .record_audit(AuditRecord {
            execution_id: run.invocation.execution_id.clone(),
            run_unit_id: run.invocation.run_unit_id.clone(),
            attempt: run.invocation.attempt,
            effect_sequence: run.host_sequence,
            observed_tick: run.invocation.deadline_tick.saturating_sub(1),
            principal: run.invocation.principal.id().clone(),
            invocation_key: run.invocation.idempotency_key.clone(),
            capability: host_request.required_capability(InvocationLimits::default()),
            resource: canonical_audit_resource_digest(&host_request),
            decision,
        })
        .map_err(|_| HostProblem::UnknownOutcome)
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

fn text(bytes: &[u8], maximum: usize) -> Result<String, HostProblem> {
    if bytes.is_empty() || bytes.len() > maximum {
        return Err(HostProblem::Malformed);
    }
    std::str::from_utf8(bytes)
        .map(str::to_string)
        .map_err(|_| HostProblem::Malformed)
}

fn valid_host(host: &str) -> bool {
    !host.is_empty()
        && host
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b':'))
}

fn effective_port(scheme: &str, port: u16) -> Result<u16, HostProblem> {
    match (scheme, port) {
        ("HTTP", 0) => Ok(80),
        ("HTTPS", 0) => Ok(443),
        ("HTTP" | "HTTPS", port) => Ok(port),
        _ => Err(condition("INVREQ", 16, 40)),
    }
}

fn condition(name: &str, response: i32, response2: i32) -> HostProblem {
    HostProblem::Condition {
        name: name.into(),
        response,
        response2,
    }
}
