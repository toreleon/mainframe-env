//! Terminal-scoped SIGNON without changing the issuing task principal.

use super::super::super::{CicsService, Run};
use super::authority::{CicsCredentialChangeRequest, CicsCredentialKind, CicsCredentialRequest};
use super::terminal_state::{self, TerminalIdentity};
use super::verify::{condition, live_tick, respond_signon, text};
use mainframe_env_execution_api::{BoundedPayload, InvocationLimits, PrincipalId};
use mainframe_env_host_api::{
    CicsRequest, CicsResponse, HostProblem, HostRequest, canonical_request_digest,
};
use sha2::{Digest, Sha256};

pub(super) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
    retention_tick: u64,
) -> Result<CicsResponse, HostProblem> {
    validate_shape(request)?;
    let user = text(request, "USERID")?
        .ok_or(HostProblem::Malformed)?
        .trim_end()
        .to_ascii_uppercase();
    if user.is_empty() {
        return Err(condition("USERIDERR", 69, 30));
    }
    if user.contains(' ') {
        return Err(condition("INVREQ", 16, 12));
    }
    let user = PrincipalId::new(user, InvocationLimits::default())
        .map_err(|_| condition("USERIDERR", 69, 8))?;
    let group = text(request, "GROUPID")?.and_then(|value| {
        let value = value.trim_end().to_ascii_uppercase();
        (!value.is_empty()).then_some(value)
    });
    if group
        .as_ref()
        .is_some_and(|value| value.len() > 8 || value.contains(' '))
    {
        return Err(condition("NOTAUTH", 70, 23));
    }
    let language = language(request)?;
    if let Some(oidcard) = request.arguments.get("OIDCARD")
        && oidcard.bytes().iter().any(|byte| *byte != b' ')
    {
        return Err(condition("NOTAUTH", 70, 6));
    }
    let password = request.arguments.contains_key("PASSWORD");
    let kind = if password {
        CicsCredentialKind::Password
    } else {
        CicsCredentialKind::Phrase
    };
    let (old_name, new_name) = if password {
        ("PASSWORD", "NEWPASSWORD")
    } else {
        ("PHRASE", "NEWPHRASE")
    };
    let old = request.arguments[old_name].bytes();
    if password && old.len() != 8 {
        return Err(HostProblem::Malformed);
    }
    let old_len = if password {
        trimmed_length(old)
    } else {
        length(request, "PHRASELEN", old.len(), 1, false)?
    };
    if old_len == 0 || old[..old_len].iter().all(|byte| *byte == b' ') {
        return Err(condition("NOTAUTH", 70, 1));
    }
    let new = request.arguments.get(new_name).map(BoundedPayload::bytes);
    if password && new.is_some_and(|value| value.len() != 8) {
        return Err(HostProblem::Malformed);
    }
    let new_len = match (new, kind) {
        (Some(value), CicsCredentialKind::Password) => trimmed_length(value),
        (Some(value), CicsCredentialKind::Phrase) => {
            length(request, "NEWPHRASELEN", value.len(), 2, true)?
        }
        (None, _) => 0,
    };
    if kind == CicsCredentialKind::Phrase && new_len > 0 && (old_len <= 8) != (new_len <= 8) {
        return Err(condition("INVREQ", 16, 2));
    }
    let mutation = request
        .mutation
        .as_ref()
        .ok_or(HostProblem::MissingIdempotency)?;
    let mut identity = Sha256::new();
    identity.update(b"mainframe-env.cics-signon-effect@1\0");
    identity.update(run.invocation.execution_id.as_str().as_bytes());
    identity.update(run.invocation.run_unit_id.as_str().as_bytes());
    identity.update(mutation.idempotency_key.as_str().as_bytes());
    let key = format!("CICS-SIGNON:{:x}", identity.finalize());
    let digest = canonical_request_digest(&HostRequest::Cics(request.clone()))
        .map_err(|_| HostProblem::ResourceExhausted)?;
    let terminal = terminal_state::current(service, run, 10)?;
    if terminal.terminal_identity.effect_key.as_deref() == Some(key.as_str()) {
        return Err(HostProblem::UnknownOutcome);
    }
    if terminal.terminal_identity.user.is_some() {
        return Err(condition("INVREQ", 16, 9));
    }
    let tick = live_tick(service, retention_tick)?;
    if run.invocation.cancellation_requested() {
        return Err(HostProblem::Cancelled);
    }
    if tick >= run.invocation.deadline_tick {
        return Err(HostProblem::TimedOut);
    }
    let verified = if let Some(new) = new.filter(|_| new_len > 0) {
        let change_key = format!("{key}:CHANGE");
        service
            .security_authority()?
            .change_credential(CicsCredentialChangeRequest {
                actor: run.invocation.principal.id(),
                user: &user,
                current: &old[..old_len],
                proposed: &new[..new_len],
                kind,
                group: group.as_deref(),
                binding_digest: digest,
                idempotency_key: &change_key,
                correlation: &key,
                tick,
            })
    } else {
        let verify_key = format!("{key}:VERIFY");
        service
            .security_authority()?
            .verify_credential(CicsCredentialRequest {
                actor: run.invocation.principal.id(),
                user: &user,
                credential: &old[..old_len],
                kind,
                group: group.as_deref(),
                binding_digest: digest,
                idempotency_key: &verify_key,
                correlation: &key,
                tick,
            })
    };
    after_saf(service, run, retention_tick)?;
    let verified = verified.map_err(|_| HostProblem::UnknownOutcome)?;
    if verified.failure.is_some() {
        return respond_signon(service, run, request, verified);
    }
    let mut response = respond_signon(service, run, request, verified)?;
    for (name, value) in [
        ("LANGINUSE", language.0.as_bytes()),
        ("NATLANGINUSE", &language.1[..]),
    ] {
        if request.arguments.contains_key(name) {
            response.outputs.insert(
                name.into(),
                BoundedPayload::new(
                    "mainframe-env.cics.text@1",
                    value.to_vec(),
                    InvocationLimits::default(),
                )
                .map_err(|_| HostProblem::ResourceExhausted)?,
            );
        }
    }
    terminal_state::persist(
        service,
        run,
        &terminal,
        TerminalIdentity {
            user: Some(user.as_str().into()),
            group,
            language: Some(language.0),
            effect_key: Some(key),
            request_digest: Some(digest),
        },
    )?;
    Ok(response)
}

fn after_saf(service: &CicsService, run: &Run, retention_tick: u64) -> Result<(), HostProblem> {
    let tick = live_tick(service, retention_tick).map_err(|_| HostProblem::UnknownOutcome)?;
    if run.invocation.cancellation_requested() || tick >= run.invocation.deadline_tick {
        Err(HostProblem::UnknownOutcome)
    } else {
        Ok(())
    }
}

fn trimmed_length(bytes: &[u8]) -> usize {
    bytes
        .iter()
        .rposition(|byte| *byte != b' ')
        .map_or(0, |at| at + 1)
}

fn length(
    request: &CicsRequest,
    name: &str,
    available: usize,
    response2: i32,
    allow_zero: bool,
) -> Result<usize, HostProblem> {
    let value = text(request, name)?
        .ok_or(HostProblem::Malformed)?
        .parse::<i64>()
        .map_err(|_| HostProblem::Malformed)?;
    if !(if allow_zero { 0..=100 } else { 1..=100 }).contains(&value) || value as usize > available
    {
        return Err(condition("LENGERR", 22, response2));
    }
    Ok(value as usize)
}

fn language(request: &CicsRequest) -> Result<(String, [u8; 1]), HostProblem> {
    let long = text(request, "LANGUAGECODE")?.map(|value| value.trim_end().to_ascii_uppercase());
    let short = text(request, "NATLANG")?.map(|value| value.trim_end().to_ascii_uppercase());
    if long.is_some() && short.is_some() {
        return Err(condition("INVREQ", 16, 28));
    }
    let (code, nat) = match (long.as_deref(), short.as_deref()) {
        (Some("ENU"), _) | (None, None) | (None, Some("E")) => ("ENU", *b"E"),
        (Some("CHS"), _) | (None, Some("C")) => ("CHS", *b"C"),
        (Some("JPN"), _) | (None, Some("J")) => ("JPN", *b"J"),
        _ => return Err(condition("INVREQ", 16, 28)),
    };
    Ok((code.into(), nat))
}

fn validate_shape(request: &CicsRequest) -> Result<(), HostProblem> {
    let password = request.arguments.contains_key("PASSWORD");
    let phrase = request.arguments.contains_key("PHRASE");
    if !request.arguments.contains_key("USERID")
        || password == phrase
        || phrase != request.arguments.contains_key("PHRASELEN")
        || request.arguments.contains_key("NEWPASSWORD") && !password
        || request.arguments.contains_key("NEWPHRASE") && !phrase
        || request.arguments.contains_key("NEWPHRASELEN")
            != request.arguments.contains_key("NEWPHRASE")
        || request.arguments.contains_key("RESP2") && !request.arguments.contains_key("RESP")
        || request
            .arguments
            .iter()
            .any(|(name, value)| match name.as_str() {
                "PASSWORD" | "NEWPASSWORD" | "PHRASE" | "NEWPHRASE" | "OIDCARD" => {
                    value.schema() != "mainframe-env.cics.secret@1"
                }
                "PHRASELEN" | "NEWPHRASELEN" => value.schema() != "mainframe-env.cics.decimal@1",
                "USERID" | "GROUPID" | "LANGUAGECODE" | "NATLANG" => !matches!(
                    value.schema(),
                    "mainframe-env.cics.storage-value@1" | "mainframe-env.cics.literal@1"
                ),
                "CHANGETIME" | "DAYSLEFT" | "ESMRESP" | "ESMREASON" | "EXPIRYTIME"
                | "INVALIDCOUNT" | "LASTUSETIME" | "LANGINUSE" | "NATLANGINUSE" | "RESP"
                | "RESP2" => value.schema() != "mainframe-env.cics.argument@1",
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
