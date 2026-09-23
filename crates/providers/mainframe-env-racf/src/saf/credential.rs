//! Credential verification inside the durable SAF transaction authority.

use super::*;
use crate::authority::CredentialPolicyProblem;
use ring::hmac;
use zeroize::Zeroize;

/// Eight-character one-use ticket returned only to the issuing CICS call.
#[derive(Clone, Eq, PartialEq)]
pub struct IssuedPassTicket(Vec<u8>);

impl IssuedPassTicket {
    /// Borrow the ticket bytes for the single scoped output assignment.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.0
    }

    /// Move clear bytes into the single zeroizing CICS output allocation.
    #[must_use]
    pub fn into_bytes(mut self) -> Vec<u8> {
        std::mem::take(&mut self.0)
    }
}

impl std::fmt::Debug for IssuedPassTicket {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("IssuedPassTicket([REDACTED])")
    }
}

impl Drop for IssuedPassTicket {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}
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
    group: Option<&str>,
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

pub(super) fn apply_change_request(
    service: &RacfService,
    snapshot: &mut SecurityDatabaseSnapshot,
    context: &SafRequestContext,
    request: &RacrouteRequest,
    secret: Option<&[u8]>,
    states: &mut Vec<RacrouteState>,
) -> Result<(SafStatus, RacrouteResult), DecisionReason> {
    let RacrouteRequest::ChangeCredential {
        user, kind, group, ..
    } = request
    else {
        unreachable!("only credential changes reach this child")
    };
    change_cics_request(
        service,
        snapshot,
        context,
        user,
        secret,
        *kind,
        group.as_deref(),
        states,
    )
}

pub(super) fn is_authentication_request(request: &RacrouteRequest) -> bool {
    matches!(
        request,
        RacrouteRequest::Signon { .. }
            | RacrouteRequest::Verify { .. }
            | RacrouteRequest::VerifyCredential { .. }
            | RacrouteRequest::ChangeCredential { .. }
            | RacrouteRequest::IssuePassTicket { .. }
            | RacrouteRequest::RedeemPassTicket { .. }
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
            group,
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
            if group.is_some() {
                digest_saf_tag(digest, 0xc5);
                digest_saf_optional(digest, group.as_deref());
            }
        }
        RacrouteRequest::IssuePassTicket {
            application,
            binding_digest,
        } => {
            digest_saf_tag(digest, 0xc3);
            digest_saf_field(digest, application.as_bytes());
            digest_saf_field(digest, binding_digest);
        }
        RacrouteRequest::RedeemPassTicket {
            user,
            application,
            ticket_reference,
            binding_digest,
        } => {
            digest_saf_tag(digest, 0xc4);
            digest_saf_field(digest, user.as_str().as_bytes());
            digest_saf_field(digest, application.as_bytes());
            digest_saf_field(digest, ticket_reference.as_str().as_bytes());
            digest_saf_field(digest, binding_digest);
        }
        _ => unreachable!("only CICS credential requests are delegated"),
    }
}

pub(super) fn issue_passticket(
    snapshot: &mut SecurityDatabaseSnapshot,
    context: &SafRequestContext,
    application: &str,
    states: &mut Vec<RacrouteState>,
) -> Result<(SafStatus, RacrouteResult), DecisionReason> {
    let actor = context.caller().as_str();
    let environment = AccessEnvironment {
        tick: context.tick(),
        application: Some(application.into()),
        ..AccessEnvironment::default()
    };
    let origin = evaluate_access(
        snapshot,
        actor,
        "FACILITY",
        "IRR.RCVTPTGN",
        AccessLevel::Read,
        &environment,
        false,
    );
    if origin.status.reason != DecisionReason::Granted {
        states.push(RacrouteState::PolicyResolved);
        return Ok((
            origin.status,
            RacrouteResult::PassTicketIssued {
                decision: origin,
                origin_denied: true,
                ticket: None,
            },
        ));
    }
    let target = evaluate_access(
        snapshot,
        actor,
        "PTKTDATA",
        application,
        AccessLevel::Read,
        &environment,
        false,
    );
    states.push(RacrouteState::PolicyResolved);
    if target.status.reason != DecisionReason::Granted {
        return Ok((
            target.status,
            RacrouteResult::PassTicketIssued {
                decision: target,
                origin_denied: false,
                ticket: None,
            },
        ));
    }
    let principal = snapshot
        .principals
        .get(actor)
        .ok_or(DecisionReason::PrincipalNotFound)?;
    if principal.state != PrincipalState::Active {
        let denied = decision(
            DecisionReason::PrincipalInactive,
            AccessLevel::None,
            None,
            None,
        );
        return Ok((
            denied.status,
            RacrouteResult::PassTicketIssued {
                decision: denied,
                origin_denied: false,
                ticket: None,
            },
        ));
    }
    let verifier = principal
        .credential
        .as_ref()
        .or(principal.phrase_credential.as_ref())
        .ok_or(DecisionReason::CredentialInvalid)?;
    if snapshot.tokens.len() >= 65_536 {
        return Err(DecisionReason::ResourceExhausted);
    }
    let mut input = Vec::new();
    input.extend_from_slice(b"mainframe-env.passticket.issue@1\0");
    input.extend_from_slice(actor.as_bytes());
    input.extend_from_slice(application.as_bytes());
    input.extend_from_slice(context.idempotency_key().as_bytes());
    input.extend_from_slice(&context.tick().to_be_bytes());
    let key = hmac::Key::new(hmac::HMAC_SHA256, verifier.encoded_verifier.as_bytes());
    let tag = hmac::sign(&key, &input);
    const ALPHABET: &[u8; 32] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
    let mut ticket = vec![0; 8];
    for (position, byte) in ticket.iter_mut().zip(tag.as_ref()) {
        *position = ALPHABET[usize::from(*byte & 31)];
    }
    let digest = passticket_digest(verifier, actor, application, &ticket);
    if snapshot
        .tokens
        .values()
        .any(|token| token.token_digest == digest)
    {
        return Err(DecisionReason::ResourceExhausted);
    }
    let id = next_id("TOKEN", snapshot.generation, snapshot.tokens.len());
    let expires_tick = context
        .tick()
        .checked_add(10)
        .ok_or(DecisionReason::ResourceExhausted)?;
    snapshot.tokens.insert(
        id.clone(),
        SecurityToken {
            id: id.clone(),
            kind: TokenKind::PassTicket,
            owner: actor.into(),
            issuer: actor.into(),
            audience: Some(application.into()),
            token_reference: format!("cics:passticket:{id}"),
            token_digest: digest,
            scopes: BTreeSet::new(),
            issued_tick: context.tick(),
            expires_tick: Some(expires_tick),
            state: TokenState::Active,
            version: 1,
        },
    );
    Ok((
        target.status,
        RacrouteResult::PassTicketIssued {
            decision: target,
            origin_denied: false,
            ticket: Some(IssuedPassTicket(ticket)),
        },
    ))
}

pub(super) fn redeem_passticket(
    snapshot: &mut SecurityDatabaseSnapshot,
    context: &SafRequestContext,
    user: &PrincipalId,
    application: &str,
    secret: Option<&[u8]>,
    states: &mut Vec<RacrouteState>,
) -> Result<(SafStatus, RacrouteResult), DecisionReason> {
    let denied = |reason, failure| {
        let decision = decision(reason, AccessLevel::None, None, None);
        (
            decision.status,
            RacrouteResult::PassTicketRedeemed {
                decision,
                failure: Some(failure),
                user: user.as_str().into(),
            },
        )
    };
    let Some(principal) = snapshot.principals.get(user.as_str()) else {
        return Ok(denied(
            DecisionReason::PrincipalNotFound,
            CredentialFailure::UnknownUser,
        ));
    };
    if principal.state != PrincipalState::Active {
        return Ok(denied(
            DecisionReason::PrincipalInactive,
            CredentialFailure::Revoked,
        ));
    }
    let verifier = principal
        .credential
        .as_ref()
        .or(principal.phrase_credential.as_ref())
        .ok_or(DecisionReason::CredentialInvalid)?;
    let Some(secret) = secret else {
        return Ok(denied(
            DecisionReason::CredentialInvalid,
            CredentialFailure::InvalidCredential,
        ));
    };
    let ticket: [u8; 8] = match secret.try_into() {
        Ok(ticket) => ticket,
        Err(_) => {
            return Ok(denied(
                DecisionReason::CredentialInvalid,
                CredentialFailure::InvalidCredential,
            ));
        }
    };
    let digest = passticket_digest(verifier, user.as_str(), application, &ticket);
    let token_id = snapshot.tokens.values().find(|token| {
        token.kind == TokenKind::PassTicket
            && token.owner == user.as_str()
            && token.audience.as_deref() == Some(application)
            && token.token_digest == digest
            && token.state == TokenState::Active
            && token
                .expires_tick
                .is_some_and(|expiry| context.tick() < expiry)
    });
    let Some(token_id) = token_id.map(|token| token.id.clone()) else {
        let principal = snapshot
            .principals
            .get_mut(user.as_str())
            .ok_or(DecisionReason::RecoveryRequired)?;
        principal.invalid_count = Some(principal.invalid_count.unwrap_or(0).saturating_add(1));
        return Ok(denied(
            DecisionReason::CredentialInvalid,
            CredentialFailure::InvalidCredential,
        ));
    };
    let token = snapshot
        .tokens
        .get_mut(&token_id)
        .ok_or(DecisionReason::RecoveryRequired)?;
    token.state = TokenState::Revoked;
    token.version = token
        .version
        .checked_add(1)
        .ok_or(DecisionReason::ResourceExhausted)?;
    states.push(RacrouteState::PolicyResolved);
    let decision = decision(DecisionReason::Granted, AccessLevel::None, None, None);
    Ok((
        decision.status,
        RacrouteResult::PassTicketRedeemed {
            decision,
            failure: None,
            user: user.as_str().into(),
        },
    ))
}

fn passticket_digest(
    verifier: &CredentialVerifier,
    user: &str,
    application: &str,
    ticket: &[u8],
) -> String {
    let mut input = Vec::new();
    input.extend_from_slice(b"mainframe-env.passticket.digest@1\0");
    input.extend_from_slice(user.as_bytes());
    input.extend_from_slice(application.as_bytes());
    input.extend_from_slice(ticket);
    let key = hmac::Key::new(hmac::HMAC_SHA256, verifier.encoded_verifier.as_bytes());
    format!(
        "sha256:{:x}",
        Sha256::digest(hmac::sign(&key, &input).as_ref())
    )
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
