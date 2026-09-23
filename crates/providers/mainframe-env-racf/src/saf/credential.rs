//! Credential verification inside the durable SAF transaction authority.

use super::*;
use crate::authority::CredentialPolicyProblem;
use crate::model::{CredentialVerifier, connection_key};

/// Credential field selected for one SAF verification.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum CredentialKind {
    /// The standard password field.
    Password,
    /// The phrase field, using a standard password for lengths up to eight.
    Phrase,
}

/// Source-distinct rejection produced by credential verification.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum CredentialFailure {
    /// The user ID is not present in the security database.
    UnknownUser,
    /// The user ID is revoked, suspended, or locked.
    Revoked,
    /// The required password or phrase is expired or has not been set.
    NewCredentialRequired,
    /// The supplied credential did not verify.
    InvalidCredential,
    /// The new credential violates the active RACF policy or history.
    UnacceptableNewCredential,
    /// The old and new phrase lengths select different credential fields.
    MismatchedCredentialKind,
    /// The requested group does not exist.
    UnknownGroup,
    /// The user is not connected to the requested group.
    GroupNotConnected,
    /// The user's requested group connection is revoked.
    GroupRevoked,
    /// SAF cannot evaluate authentication with the current subsystem state.
    PolicyUnavailable,
}

/// Nonsecret profile status returned only after successful verification.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CredentialDetails {
    /// Credential change time in the caller's logical clock units.
    pub changed_tick: i64,
    /// Days until expiry, or negative one for a nonexpiring credential.
    pub days_left: i16,
    /// Expiry time, or negative one for a nonexpiring credential.
    pub expiry_tick: i64,
    /// Failed verification count observed before this successful verification.
    pub invalid_count: u16,
    /// Prior successful-use time, or zero when no prior use exists.
    pub last_use_tick: i64,
}

pub(super) fn verify_cics_request(
    snapshot: &mut SecurityDatabaseSnapshot,
    context: &SafRequestContext,
    user: &PrincipalId,
    secret: Option<&[u8]>,
    kind: CredentialKind,
    group: Option<&str>,
    states: &mut Vec<RacrouteState>,
) -> Result<(SafStatus, RacrouteResult), DecisionReason> {
    let Some(principal) = snapshot.principals.get(user.as_str()) else {
        return Ok(denied(
            DecisionReason::PrincipalNotFound,
            CredentialFailure::UnknownUser,
        ));
    };
    match principal.state {
        PrincipalState::PasswordExpired => {
            return Ok(denied(
                DecisionReason::PrincipalInactive,
                CredentialFailure::NewCredentialRequired,
            ));
        }
        PrincipalState::Revoked | PrincipalState::Suspended | PrincipalState::Locked => {
            return Ok(denied(
                DecisionReason::PrincipalInactive,
                CredentialFailure::Revoked,
            ));
        }
        PrincipalState::Active => {}
    }
    let credential = match kind {
        CredentialKind::Password => principal
            .credential
            .as_ref()
            .filter(|value| !value.is_phrase),
        CredentialKind::Phrase if secret.is_some_and(|secret| secret.len() <= 8) => principal
            .credential
            .as_ref()
            .filter(|value| !value.is_phrase),
        CredentialKind::Phrase => principal.phrase_credential.as_ref().or_else(|| {
            principal
                .credential
                .as_ref()
                .filter(|value| value.is_phrase)
        }),
    };
    let Some(credential) = credential else {
        return Ok(denied(
            DecisionReason::CredentialInvalid,
            CredentialFailure::NewCredentialRequired,
        ));
    };
    let Some(secret) = secret else {
        return Ok(denied(
            DecisionReason::CredentialInvalid,
            CredentialFailure::InvalidCredential,
        ));
    };
    let changed_tick = credential.changed_tick;
    if !verify_secret(credential, secret)? {
        let principal = snapshot
            .principals
            .get_mut(user.as_str())
            .ok_or(DecisionReason::RecoveryRequired)?;
        principal.invalid_count = Some(principal.invalid_count.unwrap_or(0).saturating_add(1));
        return Ok(denied(
            DecisionReason::CredentialInvalid,
            CredentialFailure::InvalidCredential,
        ));
    }
    if let Some(group) = group {
        if !snapshot.groups.contains_key(group) {
            return Ok(denied(
                DecisionReason::InsufficientAccess,
                CredentialFailure::UnknownGroup,
            ));
        }
        let Some(connection) = snapshot
            .connections
            .get(&connection_key(user.as_str(), group))
        else {
            return Ok(denied(
                DecisionReason::InsufficientAccess,
                CredentialFailure::GroupNotConnected,
            ));
        };
        if connection.revoked {
            return Ok(denied(
                DecisionReason::InsufficientAccess,
                CredentialFailure::GroupRevoked,
            ));
        }
    }
    let principal = snapshot
        .principals
        .get_mut(user.as_str())
        .ok_or(DecisionReason::RecoveryRequired)?;
    let details = CredentialDetails {
        changed_tick: i64::try_from(changed_tick).map_err(|_| DecisionReason::ResourceExhausted)?,
        days_left: -1,
        expiry_tick: -1,
        invalid_count: principal.invalid_count.unwrap_or(255),
        last_use_tick: i64::try_from(principal.last_use_tick.unwrap_or(0))
            .map_err(|_| DecisionReason::ResourceExhausted)?,
    };
    principal.invalid_count = Some(0);
    principal.last_use_tick = Some(context.tick());
    states.push(RacrouteState::PolicyResolved);
    let decision = decision(DecisionReason::Granted, AccessLevel::None, None, None);
    Ok((
        decision.status,
        RacrouteResult::CredentialVerified {
            decision,
            failure: None,
            details: Some(details),
        },
    ))
}

fn denied(reason: DecisionReason, failure: CredentialFailure) -> (SafStatus, RacrouteResult) {
    let decision = decision(reason, AccessLevel::None, None, None);
    (
        decision.status,
        RacrouteResult::CredentialVerified {
            decision,
            failure: Some(failure),
            details: None,
        },
    )
}

fn verify_secret(credential: &CredentialVerifier, secret: &[u8]) -> Result<bool, DecisionReason> {
    let parsed = PasswordHash::new(&credential.encoded_verifier)
        .map_err(|_| DecisionReason::PolicyUnavailable)?;
    Ok(Argon2::default().verify_password(secret, &parsed).is_ok())
}

pub(super) fn change_cics_request(
    service: &RacfService,
    snapshot: &mut SecurityDatabaseSnapshot,
    context: &SafRequestContext,
    user: &PrincipalId,
    packed: Option<&[u8]>,
    kind: CredentialKind,
    states: &mut Vec<RacrouteState>,
) -> Result<(SafStatus, RacrouteResult), DecisionReason> {
    let packed = packed.ok_or(DecisionReason::CredentialInvalid)?;
    let [old_hi, old_lo, new_hi, new_lo, ..] = packed else {
        return Err(DecisionReason::MalformedRequest);
    };
    let old_len = usize::from(u16::from_be_bytes([*old_hi, *old_lo]));
    let new_len = usize::from(u16::from_be_bytes([*new_hi, *new_lo]));
    if old_len == 0
        || new_len == 0
        || old_len > 100
        || new_len > 100
        || packed.len() != 4 + old_len + new_len
    {
        return Err(DecisionReason::MalformedRequest);
    }
    let old = &packed[4..4 + old_len];
    let new = &packed[4 + old_len..];
    if kind == CredentialKind::Password && (old_len > 8 || new_len > 8) {
        return Err(DecisionReason::MalformedRequest);
    }
    if kind == CredentialKind::Phrase && (old_len <= 8) != (new_len <= 8) {
        return Ok(denied(
            DecisionReason::CredentialInvalid,
            CredentialFailure::MismatchedCredentialKind,
        ));
    }
    let Some(principal) = snapshot.principals.get(user.as_str()) else {
        return Ok(denied(
            DecisionReason::PrincipalNotFound,
            CredentialFailure::UnknownUser,
        ));
    };
    if matches!(
        principal.state,
        PrincipalState::Revoked | PrincipalState::Suspended | PrincipalState::Locked
    ) {
        return Ok(denied(
            DecisionReason::PrincipalInactive,
            CredentialFailure::Revoked,
        ));
    }
    let phrase_slot = kind == CredentialKind::Phrase && old_len > 8;
    let existing = if phrase_slot {
        principal.phrase_credential.as_ref().or_else(|| {
            principal
                .credential
                .as_ref()
                .filter(|value| value.is_phrase)
        })
    } else {
        principal
            .credential
            .as_ref()
            .filter(|value| !value.is_phrase)
    };
    let Some(existing) = existing else {
        return Ok(denied(
            DecisionReason::CredentialInvalid,
            CredentialFailure::NewCredentialRequired,
        ));
    };
    if !verify_secret(existing, old)? {
        let principal = snapshot
            .principals
            .get_mut(user.as_str())
            .ok_or(DecisionReason::RecoveryRequired)?;
        principal.invalid_count = Some(principal.invalid_count.unwrap_or(0).saturating_add(1));
        return Ok(denied(
            DecisionReason::CredentialInvalid,
            CredentialFailure::InvalidCredential,
        ));
    }
    let next = match service.credential_from_bytes(
        &snapshot.policy,
        user.as_str(),
        Some(existing),
        new,
        phrase_slot,
        context.tick(),
    ) {
        Ok(next) => next,
        Err(CredentialPolicyProblem::Invalid | CredentialPolicyProblem::Reused) => {
            return Ok(denied(
                DecisionReason::CredentialInvalid,
                CredentialFailure::UnacceptableNewCredential,
            ));
        }
        Err(CredentialPolicyProblem::Infrastructure) => {
            return Err(DecisionReason::PolicyUnavailable);
        }
    };
    let principal = snapshot
        .principals
        .get_mut(user.as_str())
        .ok_or(DecisionReason::RecoveryRequired)?;
    let details = CredentialDetails {
        changed_tick: i64::try_from(context.tick())
            .map_err(|_| DecisionReason::ResourceExhausted)?,
        days_left: -1,
        expiry_tick: -1,
        invalid_count: principal.invalid_count.unwrap_or(255),
        last_use_tick: i64::try_from(principal.last_use_tick.unwrap_or(0))
            .map_err(|_| DecisionReason::ResourceExhausted)?,
    };
    if phrase_slot {
        principal.phrase_credential = Some(next);
    } else {
        principal.credential = Some(next);
    }
    principal.state = PrincipalState::Active;
    principal.invalid_count = Some(0);
    principal.last_use_tick = Some(context.tick());
    principal.version = principal
        .version
        .checked_add(1)
        .ok_or(DecisionReason::ResourceExhausted)?;
    states.push(RacrouteState::PolicyResolved);
    let decision = decision(DecisionReason::Granted, AccessLevel::None, None, None);
    Ok((
        decision.status,
        RacrouteResult::CredentialVerified {
            decision,
            failure: None,
            details: Some(details),
        },
    ))
}

pub(super) fn is_authentication_request(request: &RacrouteRequest) -> bool {
    matches!(
        request,
        RacrouteRequest::Signon { .. }
            | RacrouteRequest::Verify { .. }
            | RacrouteRequest::VerifyCredential { .. }
            | RacrouteRequest::ChangeCredential { .. }
            | RacrouteRequest::Verifyx { .. }
    )
}

pub(super) fn digest_cics_request(digest: &mut Sha256, request: &RacrouteRequest) {
    match request {
        RacrouteRequest::VerifyCredential {
            user,
            credential_reference,
            kind,
            group,
            binding_digest,
        } => {
            digest_saf_tag(digest, 0xc1);
            digest_saf_field(digest, user.as_str().as_bytes());
            digest_saf_field(digest, credential_reference.as_str().as_bytes());
            digest_saf_tag(
                digest,
                match kind {
                    CredentialKind::Password => 1,
                    CredentialKind::Phrase => 2,
                },
            );
            digest_saf_optional(digest, group.as_deref());
            digest_saf_field(digest, binding_digest);
        }
        RacrouteRequest::ChangeCredential {
            user,
            credential_reference,
            kind,
            binding_digest,
        } => {
            digest_saf_tag(digest, 0xc2);
            digest_saf_field(digest, user.as_str().as_bytes());
            digest_saf_field(digest, credential_reference.as_str().as_bytes());
            digest_saf_tag(
                digest,
                match kind {
                    CredentialKind::Password => 1,
                    CredentialKind::Phrase => 2,
                },
            );
            digest_saf_field(digest, binding_digest);
        }
        _ => unreachable!("only CICS credential requests are delegated"),
    }
}

pub(super) fn build_mfa_proof(
    service: &RacfService,
    user: &str,
    supplied_reference: Option<&SecretRef>,
) -> Result<Option<MfaProof>, HostProblem> {
    let snapshot = service.database.read()?;
    let Some(factor) = snapshot
        .mfa_factors
        .values()
        .find(|factor| factor.owner == user && factor.active)
        .cloned()
    else {
        return Ok(None);
    };
    let valid = supplied_reference.is_some_and(|supplied_reference| {
        let expected_reference = SecretRef::new(&factor.secret_reference, Default::default());
        expected_reference.is_ok_and(|expected_reference| {
            service
                .secrets
                .resolve(&expected_reference)
                .ok()
                .zip(service.secrets.resolve(supplied_reference).ok())
                .is_some_and(|(expected, supplied)| {
                    let expected_key = ring::hmac::Key::new(ring::hmac::HMAC_SHA256, &expected);
                    let supplied_key = ring::hmac::Key::new(ring::hmac::HMAC_SHA256, &supplied);
                    let supplied_tag = ring::hmac::sign(&supplied_key, b"racf-mfa-proof");
                    ring::hmac::verify(&expected_key, b"racf-mfa-proof", supplied_tag.as_ref())
                        .is_ok()
                })
        })
    });
    Ok(Some(MfaProof {
        factor_id: factor.id,
        factor_reference: factor.secret_reference,
        valid,
    }))
}

pub(super) fn verify_credential(
    snapshot: &SecurityDatabaseSnapshot,
    user: &str,
    secret: &[u8],
) -> Result<DecisionReason, DecisionReason> {
    let Some(principal) = snapshot.principals.get(user) else {
        return Ok(DecisionReason::CredentialInvalid);
    };
    match principal.state {
        PrincipalState::Active => {}
        PrincipalState::PasswordExpired
        | PrincipalState::Revoked
        | PrincipalState::Suspended
        | PrincipalState::Locked => return Ok(DecisionReason::PrincipalInactive),
    }
    let Some(credential) = &principal.credential else {
        return Ok(DecisionReason::CredentialInvalid);
    };
    Ok(if verify_secret(credential, secret)? {
        DecisionReason::Granted
    } else {
        DecisionReason::CredentialInvalid
    })
}
