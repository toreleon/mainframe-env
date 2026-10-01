//! One-use encrypted PassTicket, bound to the latest same-task Kerberos key.

use super::super::super::{CicsService, Run, decimal_payload};
use super::authority::{CicsPassTicketFailure, CicsPassTicketRequest};
use super::token_key;
use super::verify::{condition, live_tick, text};
use mainframe_env_execution_api::{BoundedPayload, InvocationLimits};
use mainframe_env_host_api::{
    CicsDisposition, CicsRequest, CicsResponse, HostProblem, HostRequest, canonical_request_digest,
};
use ring::aead::{self, Aad, LessSafeKey, Nonce, UnboundKey};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

pub(super) fn issue(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
    retention_tick: u64,
) -> Result<CicsResponse, HostProblem> {
    validate_shape(request)?;
    let application = text(request, "ESMAPPNAME")?
        .ok_or(HostProblem::Malformed)?
        .trim_end()
        .to_ascii_uppercase();
    if application.is_empty()
        || application.len() > 8
        || !application.bytes().all(|byte| byte.is_ascii_alphanumeric())
    {
        return Err(condition("INVREQ", 16, 247));
    }
    let supplied = request
        .arguments
        .get("ENCRYPTKEY")
        .ok_or(HostProblem::Malformed)?
        .bytes();
    if supplied.len() != 4 {
        return Err(condition("INVREQ", 16, 255));
    }
    let capacity = text(request, "SET.MAXLENGTH")?
        .ok_or(HostProblem::Malformed)?
        .parse::<usize>()
        .map_err(|_| HostProblem::Malformed)?;
    if capacity < 36 {
        return Err(HostProblem::ResourceExhausted);
    }
    let tick = live_tick(service, retention_tick)?;
    if run.invocation.cancellation_requested() {
        return Err(HostProblem::Cancelled);
    }
    if tick >= run.invocation.deadline_tick {
        return Err(HostProblem::TimedOut);
    }
    let key = token_key::valid(service, run, supplied, tick)?
        .ok_or_else(|| condition("INVREQ", 16, 255))?;
    let mutation = request
        .mutation
        .as_ref()
        .ok_or(HostProblem::MissingIdempotency)?;
    let mut identity = Sha256::new();
    identity.update(b"mainframe-env.cics-encryptptkt-effect@1\0");
    identity.update(run.invocation.execution_id.as_str().as_bytes());
    identity.update(run.invocation.run_unit_id.as_str().as_bytes());
    identity.update(mutation.idempotency_key.as_str().as_bytes());
    let effect_key = format!("CICS-EPTKT:{:x}", identity.finalize());
    let binding_digest = canonical_request_digest(&HostRequest::Cics(request.clone()))
        .map_err(|_| HostProblem::ResourceExhausted)?;
    let issued = service
        .security_authority()?
        .issue_passticket(CicsPassTicketRequest {
            actor: run.invocation.principal.id(),
            application: &application,
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
    let issued = issued.map_err(|_| HostProblem::UnknownOutcome)?;
    if issued.failure.is_none() == issued.ticket.is_none() {
        return Err(HostProblem::UnknownOutcome);
    }
    let mut response = if let Some(failure) = issued.failure {
        let problem = match failure {
            CicsPassTicketFailure::DefaultUser => condition("INVREQ", 16, 256),
            CicsPassTicketFailure::RegionDenied => condition("NOTAUTH", 70, 260),
            CicsPassTicketFailure::TargetDenied => condition("NOTAUTH", 70, 250),
            CicsPassTicketFailure::SecurityUnavailable => condition("INVREQ", 16, 251),
            CicsPassTicketFailure::Unsupported => condition("INVREQ", 16, 254),
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
    if let Some(ticket) = issued.ticket {
        if ticket.schema() != "mainframe-env.cics.secret@1" || ticket.bytes().len() != 8 {
            return Err(HostProblem::UnknownOutcome);
        }
        let nonce = nonce(&key.key, &effect_key, binding_digest);
        let mut ciphertext = Zeroizing::new(ticket.bytes().to_vec());
        let unbound = UnboundKey::new(&aead::AES_256_GCM, key.key.as_ref())
            .map_err(|_| HostProblem::UnknownOutcome)?;
        let tag = LessSafeKey::new(unbound)
            .seal_in_place_separate_tag(
                Nonce::assume_unique_for_key(nonce),
                Aad::from(application.as_bytes()),
                ciphertext.as_mut_slice(),
            )
            .map_err(|_| HostProblem::UnknownOutcome)?;
        let mut envelope = nonce.to_vec();
        envelope.extend_from_slice(&ciphertext);
        envelope.extend_from_slice(tag.as_ref());
        token_key::consume(service, run, &key)?;
        response
            .outputs
            .insert("FLENGTH".into(), decimal_payload(envelope.len() as i64)?);
        response.outputs.insert(
            "ENCRYPTPTKT".into(),
            BoundedPayload::new(
                "mainframe-env.cics.payload@1",
                envelope,
                InvocationLimits::default(),
            )
            .map_err(|_| HostProblem::UnknownOutcome)?,
        );
    }
    for (name, value) in [
        ("ESMRESP", issued.esm_response),
        ("ESMREASON", issued.esm_reason),
    ] {
        if request.arguments.contains_key(name) {
            response
                .outputs
                .insert(name.into(), decimal_payload(value)?);
        }
    }
    Ok(response)
}

fn nonce(key: &[u8; 32], effect: &str, binding: [u8; 32]) -> [u8; 12] {
    let mut digest = Sha256::new();
    digest.update(b"mainframe-env.cics-encryptptkt-nonce@1\0");
    digest.update(key);
    digest.update(effect.as_bytes());
    digest.update(binding);
    let hash = digest.finalize();
    let mut result = [0; 12];
    result.copy_from_slice(&hash[..12]);
    result
}

fn validate_shape(request: &CicsRequest) -> Result<(), HostProblem> {
    if ![
        "ENCRYPTKEY",
        "ENCRYPTPTKT",
        "FLENGTH",
        "ESMAPPNAME",
        "SET.MAXLENGTH",
    ]
    .iter()
    .all(|name| request.arguments.contains_key(*name))
        || request.arguments.contains_key("RESP2") && !request.arguments.contains_key("RESP")
        || request
            .arguments
            .iter()
            .any(|(name, value)| match name.as_str() {
                "ENCRYPTKEY" => value.schema() != "mainframe-env.cics.storage-value@1",
                "ESMAPPNAME" => !matches!(
                    value.schema(),
                    "mainframe-env.cics.storage-value@1" | "mainframe-env.cics.literal@1"
                ),
                "SET.MAXLENGTH" => value.schema() != "mainframe-env.cics.decimal@1",
                "ENCRYPTPTKT" | "FLENGTH" | "ESMRESP" | "ESMREASON" | "RESP" | "RESP2" => {
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
