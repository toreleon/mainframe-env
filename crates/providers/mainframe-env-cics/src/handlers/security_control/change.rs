//! CICS credential changes through the bounded, replay-safe SAF bridge.

use super::super::super::{CicsService, Run};
use super::authority::{CicsCredentialChangeRequest, CicsCredentialKind};
use super::verify::{condition, live_tick, respond, text};
use mainframe_env_execution_api::{InvocationLimits, PrincipalId};
use mainframe_env_host_api::{
    CicsRequest, CicsResponse, HostProblem, HostRequest, canonical_request_digest,
};
use sha2::{Digest, Sha256};

pub(super) fn password(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
    retention_tick: u64,
) -> Result<CicsResponse, HostProblem> {
    change(
        service,
        run,
        request,
        retention_tick,
        CicsCredentialKind::Password,
    )
}

fn change(
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
    let (old_name, new_name) = match kind {
        CicsCredentialKind::Password => ("PASSWORD", "NEWPASSWORD"),
        CicsCredentialKind::Phrase => ("PHRASE", "NEWPHRASE"),
    };
    let old = request.arguments[old_name].bytes();
    let new = request.arguments[new_name].bytes();
    let (old_len, new_len) = match kind {
        CicsCredentialKind::Password => (trimmed_length(old), trimmed_length(new)),
        CicsCredentialKind::Phrase => {
            let old_len = length(request, "PHRASELEN", old.len(), 1)?;
            let new_len = length(request, "NEWPHRASELEN", new.len(), 2)?;
            (old_len, new_len)
        }
    };
    if old_len == 0
        || new_len == 0
        || old[..old_len].iter().all(|byte| *byte == b' ')
        || new[..new_len].iter().all(|byte| *byte == b' ')
    {
        return Err(condition("NOTAUTH", 70, 1));
    }
    if kind == CicsCredentialKind::Password && (old_len > 8 || new_len > 8) {
        return Err(HostProblem::Malformed);
    }
    let mutation = request
        .mutation
        .as_ref()
        .ok_or(HostProblem::MissingIdempotency)?;
    let mut identity = Sha256::new();
    identity.update(b"mainframe-env.cics-credential-change-effect@1\0");
    identity.update(run.invocation.execution_id.as_str().as_bytes());
    identity.update(run.invocation.run_unit_id.as_str().as_bytes());
    identity.update(mutation.idempotency_key.as_str().as_bytes());
    let idempotency_key = format!("CICS-CHANGE:{:x}", identity.finalize());
    let binding_digest = canonical_request_digest(&HostRequest::Cics(request.clone()))
        .map_err(|_| HostProblem::ResourceExhausted)?;
    let tick = live_tick(service, retention_tick)?;
    if run.invocation.cancellation_requested() {
        return Err(HostProblem::Cancelled);
    }
    if tick >= run.invocation.deadline_tick {
        return Err(HostProblem::TimedOut);
    }
    let result = service
        .security_authority()?
        .change_credential(CicsCredentialChangeRequest {
            actor: run.invocation.principal.id(),
            user: &user,
            current: &old[..old_len],
            proposed: &new[..new_len],
            kind,
            binding_digest,
            idempotency_key: &idempotency_key,
            correlation: &idempotency_key,
            tick,
        });
    let resolved_tick =
        live_tick(service, retention_tick).map_err(|_| HostProblem::UnknownOutcome)?;
    if run.invocation.cancellation_requested() || resolved_tick >= run.invocation.deadline_tick {
        return Err(HostProblem::UnknownOutcome);
    }
    respond(
        service,
        run,
        request,
        result.map_err(|_| HostProblem::UnknownOutcome)?,
    )
}

fn trimmed_length(secret: &[u8]) -> usize {
    secret
        .iter()
        .rposition(|byte| *byte != b' ')
        .map_or(0, |at| at + 1)
}

fn length(
    request: &CicsRequest,
    name: &str,
    available: usize,
    response2: i32,
) -> Result<usize, HostProblem> {
    let length = text(request, name)?
        .ok_or(HostProblem::Malformed)?
        .parse::<i64>()
        .map_err(|_| HostProblem::Malformed)?;
    if !(1..=100).contains(&length) || length as usize > available {
        return Err(condition("LENGERR", 22, response2));
    }
    Ok(length as usize)
}

fn validate_shape(request: &CicsRequest, kind: CicsCredentialKind) -> Result<(), HostProblem> {
    let (old, new) = match kind {
        CicsCredentialKind::Password => ("PASSWORD", "NEWPASSWORD"),
        CicsCredentialKind::Phrase => ("PHRASE", "NEWPHRASE"),
    };
    if !request.arguments.contains_key(old)
        || !request.arguments.contains_key(new)
        || !request.arguments.contains_key("USERID")
        || kind == CicsCredentialKind::Phrase
            && (!request.arguments.contains_key("PHRASELEN")
                || !request.arguments.contains_key("NEWPHRASELEN"))
        || request.arguments.contains_key("RESP2") && !request.arguments.contains_key("RESP")
        || request
            .arguments
            .iter()
            .any(|(name, value)| match name.as_str() {
                "PASSWORD" | "NEWPASSWORD" | "PHRASE" | "NEWPHRASE"
                    if name == old || name == new =>
                {
                    value.schema() != "mainframe-env.cics.secret@1"
                }
                "PHRASELEN" | "NEWPHRASELEN" if kind == CicsCredentialKind::Phrase => {
                    value.schema() != "mainframe-env.cics.decimal@1"
                }
                "USERID" => !matches!(
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
