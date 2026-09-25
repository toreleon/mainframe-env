//! Source-shaped VERIFY TOKEN validation and task-scoped encryption-key issuance.

use super::super::super::{CicsService, Run, decimal_payload};
use super::authority::{CicsSecurityTokenKind, CicsTokenFailure, CicsTokenVerificationRequest};
use super::token_key;
use super::verify::{condition, live_tick, text};
use base64::Engine;
use mainframe_env_execution_api::{BoundedPayload, InvocationLimits};
use mainframe_env_host_api::{
    CicsDisposition, CicsRequest, CicsResponse, HostProblem, HostRequest, canonical_request_digest,
};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

pub(super) fn verify(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
    retention_tick: u64,
) -> Result<CicsResponse, HostProblem> {
    validate_shape(request)?;
    let kind = match option(request, "BASICAUTH", "JWT", "KERBEROS")? {
        "BASICAUTH" => CicsSecurityTokenKind::BasicAuth,
        "JWT" => CicsSecurityTokenKind::Jwt,
        "KERBEROS" => CicsSecurityTokenKind::Kerberos,
        _ => return Err(condition("INVREQ", 16, 31)),
    };
    let base64 = request.arguments.contains_key("OPTION.BASE64");
    if base64 && kind == CicsSecurityTokenKind::Jwt {
        return Err(condition("INVREQ", 16, 32));
    }
    if request.arguments.contains_key("OUTTOKEN") && kind != CicsSecurityTokenKind::Kerberos {
        return Err(condition("INVREQ", 16, 65));
    }
    let supplied = request
        .arguments
        .get("TOKEN")
        .ok_or(HostProblem::Malformed)?
        .bytes();
    let length = text(request, "TOKENLEN")?
        .ok_or(HostProblem::Malformed)?
        .parse::<usize>()
        .map_err(|_| HostProblem::Malformed)?;
    if length == 0 || length > supplied.len() {
        return Err(condition("INVREQ", 16, 60));
    }
    if kind == CicsSecurityTokenKind::Kerberos && length > 65_535 {
        return Err(condition("LENGERR", 22, 45));
    }
    let decoded = if base64 {
        Zeroizing::new(
            base64::engine::general_purpose::STANDARD
                .decode(&supplied[..length])
                .map_err(|_| condition("INVREQ", 16, 36))?,
        )
    } else {
        Zeroizing::new(supplied[..length].to_vec())
    };
    if decoded.is_empty() || decoded.len() > 65_535 {
        return Err(condition("INVREQ", 16, if base64 { 37 } else { 60 }));
    }
    if request.arguments.contains_key("OUTTOKEN") && decoded.starts_with(b"KRB5:CONF:MUTUAL:") {
        let capacity = text(request, "SET.MAXLENGTH")?
            .ok_or(HostProblem::Malformed)?
            .parse::<usize>()
            .map_err(|_| HostProblem::Malformed)?;
        if capacity < 76 {
            return Err(HostProblem::ResourceExhausted);
        }
    }
    let mutation = request
        .mutation
        .as_ref()
        .ok_or(HostProblem::MissingIdempotency)?;
    let mut identity = Sha256::new();
    identity.update(b"mainframe-env.cics-verify-token-effect@1\0");
    identity.update(run.invocation.execution_id.as_str().as_bytes());
    identity.update(run.invocation.run_unit_id.as_str().as_bytes());
    identity.update(mutation.idempotency_key.as_str().as_bytes());
    let effect_key = format!("CICS-VTOKEN:{:x}", identity.finalize());
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
        .verify_token(CicsTokenVerificationRequest {
            actor: run.invocation.principal.id(),
            token: &decoded,
            kind,
            application: &run.applid,
            binding_digest,
            idempotency_key: &effect_key,
            correlation: &effect_key,
            tick,
        });
    let resolved_tick =
        live_tick(service, retention_tick).map_err(|_| HostProblem::UnknownOutcome)?;
    if run.invocation.cancellation_requested() || resolved_tick >= run.invocation.deadline_tick {
        return Err(HostProblem::UnknownOutcome);
    }
    let verified = verified.map_err(|_| HostProblem::UnknownOutcome)?;
    if request.arguments.contains_key("ENCRYPTKEY") {
        token_key::release_task(service, run).map_err(|_| HostProblem::UnknownOutcome)?;
    }
    if verified.failure.is_none() == verified.user.is_none() {
        return Err(HostProblem::UnknownOutcome);
    }
    if verified.mutual
        && (!request.arguments.contains_key("OUTTOKEN")
            || !request.arguments.contains_key("OUTTOKENLEN"))
    {
        return Err(condition("INVREQ", 16, 52));
    }
    if request.arguments.contains_key("ENCRYPTKEY") && kind != CicsSecurityTokenKind::Kerberos {
        return Err(condition("INVREQ", 16, 31));
    }
    if request.arguments.contains_key("ENCRYPTKEY")
        && verified.failure.is_none()
        && !verified.confidential
    {
        return Err(condition("INVREQ", 16, 51));
    }
    let mut response = if let Some(failure) = verified.failure {
        let problem = match failure {
            CicsTokenFailure::Malformed => condition("INVREQ", 16, 60),
            CicsTokenFailure::UnsignedJwt => condition("INVREQ", 16, 103),
            CicsTokenFailure::MalformedKerberos => condition("INVREQ", 16, 50),
            CicsTokenFailure::Rejected => condition("NOTAUTH", 70, 61),
            CicsTokenFailure::Revoked => condition("NOTAUTH", 70, 70),
            CicsTokenFailure::UnknownUser => condition("NOTAUTH", 70, 20),
            CicsTokenFailure::JwtUnavailable => condition("INVREQ", 16, 65),
            CicsTokenFailure::KerberosUnavailable => condition("INVREQ", 16, 53),
            CicsTokenFailure::PolicyUnavailable => condition("INVREQ", 16, 100),
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
    if let Some(user) = verified.user
        && request.arguments.contains_key("ISUSERID")
    {
        response.outputs.insert(
            "ISUSERID".into(),
            payload(
                "mainframe-env.cics.payload@1",
                format!("{user:<8}").into_bytes(),
            )?,
        );
    }
    if request.arguments.contains_key("OUTTOKEN") && verified.failure.is_none() {
        if verified.mutual {
            let out_token = verified.out_token.ok_or(HostProblem::UnknownOutcome)?;
            response.outputs.insert(
                "OUTTOKENLEN".into(),
                decimal_payload(out_token.bytes().len() as i64)?,
            );
            response.outputs.insert("OUTTOKEN".into(), out_token);
        } else {
            response
                .outputs
                .insert("OUTTOKENLEN".into(), decimal_payload(0)?);
            response.outputs.insert(
                "OUTTOKEN".into(),
                payload("mainframe-env.cics.pointer-null@1", Vec::new())?,
            );
        }
    }
    if verified.failure.is_none() && request.arguments.contains_key("ENCRYPTKEY") {
        let handle = token_key::install(service, run, &decoded, &effect_key, binding_digest, tick)?;
        response.outputs.insert(
            "ENCRYPTKEY".into(),
            payload("mainframe-env.cics.payload@1", handle.to_vec())?,
        );
    }
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
    Ok(response)
}

fn option<'a>(
    request: &'a CicsRequest,
    first: &'a str,
    second: &'a str,
    third: &'a str,
) -> Result<&'a str, HostProblem> {
    let selected = [first, second, third]
        .into_iter()
        .filter(|name| request.arguments.contains_key(&format!("OPTION.{name}")))
        .collect::<Vec<_>>();
    if selected.len() != 1 {
        return Err(condition("INVREQ", 16, 31));
    }
    Ok(selected[0])
}

fn payload(schema: &str, bytes: Vec<u8>) -> Result<BoundedPayload, HostProblem> {
    BoundedPayload::new(schema, bytes, InvocationLimits::default())
        .map_err(|_| HostProblem::ResourceExhausted)
}

fn validate_shape(request: &CicsRequest) -> Result<(), HostProblem> {
    if !request.arguments.contains_key("TOKEN")
        || !request.arguments.contains_key("TOKENLEN")
        || request.arguments.contains_key("OUTTOKEN")
            != request.arguments.contains_key("OUTTOKENLEN")
        || request.arguments.contains_key("RESP2") && !request.arguments.contains_key("RESP")
        || request
            .arguments
            .iter()
            .any(|(name, value)| match name.as_str() {
                "TOKEN" => value.schema() != "mainframe-env.cics.secret@1",
                "TOKENLEN" => value.schema() != "mainframe-env.cics.decimal@1",
                "SET.MAXLENGTH" => value.schema() != "mainframe-env.cics.decimal@1",
                "ISUSERID" | "ENCRYPTKEY" | "OUTTOKEN" | "OUTTOKENLEN" | "ESMRESP"
                | "ESMREASON" | "RESP" | "RESP2" => {
                    value.schema() != "mainframe-env.cics.argument@1"
                }
                "OPTION.BASICAUTH" | "OPTION.JWT" | "OPTION.KERBEROS" | "OPTION.BIT"
                | "OPTION.BASE64" | "OPTION.NOHANDLE" => {
                    value.schema() != "mainframe-env.cics.option@1" || !value.bytes().is_empty()
                }
                _ => true,
            })
        || request.arguments.contains_key("OPTION.BIT")
            && request.arguments.contains_key("OPTION.BASE64")
    {
        return Err(HostProblem::Malformed);
    }
    Ok(())
}
