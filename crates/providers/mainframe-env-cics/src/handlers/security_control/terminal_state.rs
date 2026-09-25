//! Versioned terminal sign-on state, separate from the issuing task principal.

use super::super::super::{CicsService, Reader, Run, Session, field};
use mainframe_env_host_api::HostProblem;

/// Durable terminal identity established by SIGNON for later task attachment.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(in crate::service) struct TerminalIdentity {
    pub user: Option<String>,
    pub group: Option<String>,
    pub language: Option<String>,
    pub effect_key: Option<String>,
    pub request_digest: Option<[u8; 32]>,
}

pub(in crate::service) fn encode_terminal_identity(
    out: &mut Vec<u8>,
    identity: &TerminalIdentity,
) -> Result<(), HostProblem> {
    validate_identity(identity)?;
    for value in [
        identity.user.as_deref(),
        identity.group.as_deref(),
        identity.language.as_deref(),
        identity.effect_key.as_deref(),
    ] {
        field(out, value.unwrap_or("").as_bytes())?;
    }
    match identity.request_digest {
        Some(digest) => {
            out.push(1);
            out.extend_from_slice(&digest);
        }
        None => out.push(0),
    }
    Ok(())
}

pub(in crate::service) fn decode_terminal_identity(
    reader: &mut Reader<'_>,
    schema: u8,
) -> Result<TerminalIdentity, HostProblem> {
    if schema < 12 {
        return Ok(TerminalIdentity::default());
    }
    let mut next =
        || String::from_utf8(reader.field(128)?).map_err(|_| HostProblem::InfrastructureFailure);
    let user = next()?;
    let group = next()?;
    let language = next()?;
    let effect_key = next()?;
    let request_digest = match reader.take(1)?[0] {
        0 => None,
        1 => Some(
            reader
                .take(32)?
                .try_into()
                .map_err(|_| HostProblem::InfrastructureFailure)?,
        ),
        _ => return Err(HostProblem::InfrastructureFailure),
    };
    let identity = TerminalIdentity {
        user: (!user.is_empty()).then_some(user),
        group: (!group.is_empty()).then_some(group),
        language: (!language.is_empty()).then_some(language),
        effect_key: (!effect_key.is_empty()).then_some(effect_key),
        request_digest,
    };
    validate_identity(&identity)?;
    Ok(identity)
}

fn validate_identity(identity: &TerminalIdentity) -> Result<(), HostProblem> {
    let valid_name = |value: &str, max: usize| {
        !value.is_empty()
            && value.len() <= max
            && value.bytes().all(|byte| {
                byte.is_ascii_uppercase()
                    || byte.is_ascii_digit()
                    || matches!(byte, b'$' | b'@' | b'#')
            })
    };
    if identity
        .user
        .as_deref()
        .is_some_and(|value| !valid_name(value, 8))
        || identity
            .group
            .as_deref()
            .is_some_and(|value| !valid_name(value, 8))
        || identity
            .language
            .as_deref()
            .is_some_and(|value| !valid_name(value, 3))
        || identity.group.is_some() && identity.user.is_none()
        || identity.effect_key.is_some() != identity.request_digest.is_some()
        || identity.effect_key.as_deref().is_some_and(|value| {
            value.is_empty()
                || value.len() > 128
                || !value.bytes().all(|byte| byte.is_ascii_graphic())
        })
    {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(())
}

pub(in crate::service) fn validate_terminal_identity(
    value: &str,
    max: usize,
) -> Result<(), HostProblem> {
    if value.is_empty()
        || value.len() > max
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_graphic() && byte != b'/' && byte != b'\\')
    {
        Err(HostProblem::Malformed)
    } else {
        Ok(())
    }
}

pub(super) fn current(
    service: &CicsService,
    run: &Run,
    missing_terminal_response2: i32,
) -> Result<Session, HostProblem> {
    let state = service.lock()?;
    let session = state
        .sessions
        .get(&run.session)
        .cloned()
        .ok_or(HostProblem::NotFound)?;
    if session.input.terminal_id.is_none() {
        return Err(HostProblem::Condition {
            name: "INVREQ".into(),
            response: 16,
            response2: missing_terminal_response2,
        });
    }
    if session.principal != run.invocation.principal.id().as_str()
        || session.run_unit != run.invocation.run_unit_id.as_str()
        || session.transaction != run.transaction
    {
        return Err(HostProblem::Unauthorized);
    }
    Ok(session)
}

pub(super) fn persist(
    service: &CicsService,
    run: &Run,
    expected: &Session,
    identity: TerminalIdentity,
) -> Result<(), HostProblem> {
    validate_identity(&identity)?;
    let mut state = service.lock()?;
    let current = state
        .sessions
        .get(&run.session)
        .cloned()
        .ok_or(HostProblem::UnknownOutcome)?;
    if current.version != expected.version
        || current.principal != expected.principal
        || current.run_unit != expected.run_unit
        || current.terminal_identity != expected.terminal_identity
    {
        return Err(HostProblem::UnknownOutcome);
    }
    let mut next = current.clone();
    next.version = next
        .version
        .checked_add(1)
        .ok_or(HostProblem::ResourceExhausted)?;
    next.terminal_identity = identity;
    service
        .persist_session(&run.session, &next, Some(current.version))
        .map_err(|_| HostProblem::UnknownOutcome)?;
    state.sessions.insert(run.session.clone(), next);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_identity_codec_round_trips_and_rejects_malformed_user() {
        let identity = TerminalIdentity {
            user: Some("IBMUSER".into()),
            group: Some("GRP1".into()),
            language: Some("ENU".into()),
            effect_key: Some("CICS-SIGNON-1".into()),
            request_digest: Some([7; 32]),
        };
        let mut bytes = Vec::new();
        encode_terminal_identity(&mut bytes, &identity).unwrap();
        let mut reader = Reader {
            bytes: &bytes,
            at: 0,
        };
        assert_eq!(decode_terminal_identity(&mut reader, 12).unwrap(), identity);
        assert_eq!(reader.at, bytes.len());
        let mut invalid = identity;
        invalid.user = Some("BAD USER".into());
        assert_eq!(
            encode_terminal_identity(&mut Vec::new(), &invalid),
            Err(HostProblem::InfrastructureFailure)
        );
    }
}
