//! Credential verification inside the durable SAF transaction authority.

use super::*;
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
