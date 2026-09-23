use super::super::super::{CicsService, Run, decimal_payload};
use super::authority::{
    CicsCredentialFailure, CicsCredentialKind, CicsCredentialRequest, CicsCredentialVerification,
};
use mainframe_env_execution_api::{InvocationLimits, PrincipalId};
use mainframe_env_host_api::{
    CicsDisposition, CicsRequest, CicsResponse, HostProblem, HostRequest, canonical_request_digest,
};
use sha2::{Digest, Sha256};

pub(super) fn password(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
    retention_tick: u64,
) -> Result<CicsResponse, HostProblem> {
    verify(
        service,
        run,
        request,
        retention_tick,
        CicsCredentialKind::Password,
    )
}

pub(super) fn phrase(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
    retention_tick: u64,
) -> Result<CicsResponse, HostProblem> {
    verify(
        service,
        run,
        request,
        retention_tick,
        CicsCredentialKind::Phrase,
    )
}

fn verify(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
    retention_tick: u64,
    kind: CicsCredentialKind,
) -> Result<CicsResponse, HostProblem> {
    validate_shape(request, kind)?;
    let user = text(request, "USERID")?
        .ok_or(HostProblem::Malformed)?
        .trim_end()
        .to_ascii_uppercase();
    if user.is_empty() || user.contains(' ') {
        return Err(condition("INVREQ", 16, 32));
    }
    let user = PrincipalId::new(user, InvocationLimits::default())
        .map_err(|_| condition("USERIDERR", 69, 8))?;
    let group = text(request, "GROUPID")?.map(|group| group.trim_end().to_ascii_uppercase());
    let secret_name = match kind {
        CicsCredentialKind::Password => "PASSWORD",
        CicsCredentialKind::Phrase => "PHRASE",
    };
    let secret = request
        .arguments
        .get(secret_name)
        .ok_or(HostProblem::Malformed)?
        .bytes();
    let length = match kind {
        CicsCredentialKind::Password => secret
            .iter()
            .rposition(|byte| *byte != b' ')
            .map_or(0, |at| at + 1),
        CicsCredentialKind::Phrase => {
            let length = text(request, "PHRASELEN")?
                .ok_or(HostProblem::Malformed)?
                .parse::<i64>()
                .map_err(|_| HostProblem::Malformed)?;
            if !(1..=100).contains(&length) || length as usize > secret.len() {
                return Err(condition("LENGERR", 22, 1));
            }
            length as usize
        }
    };
    if length == 0 || secret[..length].iter().all(|byte| *byte == b' ') {
        return Err(condition("NOTAUTH", 70, 1));
    }
    if kind == CicsCredentialKind::Password && length > 8 {
        return Err(HostProblem::Malformed);
    }
    let secret = &secret[..length];
    let mutation = request
        .mutation
        .as_ref()
        .ok_or(HostProblem::MissingIdempotency)?;
    let mut identity = Sha256::new();
    identity.update(b"mainframe-env.cics-credential-effect@1\0");
    identity.update(run.invocation.execution_id.as_str().as_bytes());
    identity.update(run.invocation.run_unit_id.as_str().as_bytes());
    identity.update(mutation.idempotency_key.as_str().as_bytes());
    let idempotency_key = format!("CICS-CRED:{:x}", identity.finalize());
    let binding_digest = canonical_request_digest(&HostRequest::Cics(request.clone()))
        .map_err(|_| HostProblem::ResourceExhausted)?;
    let tick = live_tick(service, retention_tick)?;
    if run.invocation.cancellation_requested() {
        return Err(HostProblem::Cancelled);
    }
    if tick >= run.invocation.deadline_tick {
        return Err(HostProblem::TimedOut);
    }
    let verified = service
        .security_authority()?
        .verify_credential(CicsCredentialRequest {
            actor: run.invocation.principal.id(),
            user: &user,
            credential: secret,
            kind,
            group: group.as_deref(),
            binding_digest,
            idempotency_key: &idempotency_key,
            correlation: &idempotency_key,
            tick,
        })?;
    let resolved_tick =
        live_tick(service, retention_tick).map_err(|_| HostProblem::UnknownOutcome)?;
    if run.invocation.cancellation_requested() || resolved_tick >= run.invocation.deadline_tick {
        return Err(HostProblem::UnknownOutcome);
    }
    respond(service, run, request, verified)
}

fn respond(
    service: &CicsService,
    run: &Run,
    request: &CicsRequest,
    verified: CicsCredentialVerification,
) -> Result<CicsResponse, HostProblem> {
    let mut response = if let Some(failure) = verified.failure {
        let problem = match failure {
            CicsCredentialFailure::UnknownUser => condition("USERIDERR", 69, 8),
            CicsCredentialFailure::Revoked => condition("NOTAUTH", 70, 19),
            CicsCredentialFailure::NewCredentialRequired => condition("NOTAUTH", 70, 3),
            CicsCredentialFailure::InvalidCredential => condition("NOTAUTH", 70, 2),
            CicsCredentialFailure::UnknownGroup | CicsCredentialFailure::GroupNotConnected => {
                condition("NOTAUTH", 70, 23)
            }
            CicsCredentialFailure::GroupRevoked => condition("NOTAUTH", 70, 20),
            CicsCredentialFailure::PolicyUnavailable => condition("INVREQ", 16, 18),
        };
        super::super::condition(service, run, &request.condition_policy, problem)?
    } else {
        service.response(
            run,
            CicsDisposition::Complete,
            "NORMAL",
            0,
            0,
            None,
            None,
            Vec::new(),
        )?
    };
    for (name, value) in [
        ("ESMRESP", verified.esm_response),
        ("ESMREASON", verified.esm_reason),
    ] {
        if request.arguments.contains_key(name) {
            response
                .outputs
                .insert(name.into(), decimal_payload(value)?);
        }
    }
    if let Some(details) = verified.details {
        for (name, value) in [
            ("CHANGETIME", details.changed_tick),
            ("DAYSLEFT", i64::from(details.days_left)),
            ("EXPIRYTIME", details.expiry_tick),
            ("INVALIDCOUNT", i64::from(details.invalid_count)),
            ("LASTUSETIME", details.last_use_tick),
        ] {
            if request.arguments.contains_key(name) {
                response
                    .outputs
                    .insert(name.into(), decimal_payload(value)?);
            }
        }
    }
    Ok(response)
}

fn validate_shape(request: &CicsRequest, kind: CicsCredentialKind) -> Result<(), HostProblem> {
    let secret_name = match kind {
        CicsCredentialKind::Password => "PASSWORD",
        CicsCredentialKind::Phrase => "PHRASE",
    };
    if !request.arguments.contains_key(secret_name)
        || !request.arguments.contains_key("USERID")
        || kind == CicsCredentialKind::Phrase && !request.arguments.contains_key("PHRASELEN")
        || request.arguments.contains_key("RESP2") && !request.arguments.contains_key("RESP")
        || request
            .arguments
            .iter()
            .any(|(name, value)| match name.as_str() {
                "PASSWORD" | "PHRASE" if name == secret_name => {
                    value.schema() != "mainframe-env.cics.secret@1"
                }
                "PHRASELEN" if kind == CicsCredentialKind::Phrase => {
                    value.schema() != "mainframe-env.cics.decimal@1"
                }
                "USERID" | "GROUPID" => !matches!(
                    value.schema(),
                    "mainframe-env.cics.storage-value@1" | "mainframe-env.cics.literal@1"
                ),
                "CHANGETIME" | "DAYSLEFT" | "ESMRESP" | "ESMREASON" | "EXPIRYTIME"
                | "INVALIDCOUNT" | "LASTUSETIME" | "RESP" | "RESP2" => {
                    value.schema() != "mainframe-env.cics.argument@1"
                }
                "OPTION.NOHANDLE" => {
                    value.schema() != "mainframe-env.cics.option@1" || !value.bytes().is_empty()
                }
                _ => true,
            })
    {
        return Err(HostProblem::Malformed);
    }
    Ok(())
}

fn live_tick(service: &CicsService, fallback: u64) -> Result<u64, HostProblem> {
    match &service.replay_clock {
        Some(clock) => match clock.now_tick()? {
            0 => Err(HostProblem::InfrastructureFailure),
            tick => Ok(tick),
        },
        None => Ok(fallback.saturating_sub(1)),
    }
}

fn text(request: &CicsRequest, name: &str) -> Result<Option<String>, HostProblem> {
    request
        .arguments
        .get(name)
        .map(|value| String::from_utf8(value.bytes().to_vec()).map_err(|_| HostProblem::Malformed))
        .transpose()
}

fn condition(name: &str, response: i32, response2: i32) -> HostProblem {
    HostProblem::Condition {
        name: name.into(),
        response,
        response2,
    }
}
