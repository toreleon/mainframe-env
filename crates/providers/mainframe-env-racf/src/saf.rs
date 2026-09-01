use crate::RacfService;
use crate::command::{RacrouteRequestType, racroute_descriptors};
use crate::model::{
    AccessCondition, AccessLevel, Acee, AceeState, AuditFieldValue, AuditPolicy, DecisionOutcome,
    DecisionReason, PrincipalState, RaclistCache, ResourceProfile, SafDecision, SafStatus,
    SecurityAuditRecord, SecurityDatabaseSnapshot, SecurityToken, SignonSession,
    SignonSessionState, TokenKind, TokenState, profile_key,
};
use argon2::Argon2;
use argon2::password_hash::{PasswordVerifier, phc::PasswordHash};
use mainframe_env_execution_api::PrincipalId;
use mainframe_env_host_api::{HostProblem, SecretRef};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SafRequestContext {
    caller: PrincipalId,
    acee_id: Option<String>,
    delegated_by: Option<PrincipalId>,
    idempotency_key: String,
    correlation: String,
    tick: u64,
}

impl SafRequestContext {
    pub fn new(
        caller: PrincipalId,
        acee_id: Option<String>,
        delegated_by: Option<PrincipalId>,
        idempotency_key: impl Into<String>,
        correlation: impl Into<String>,
        tick: u64,
    ) -> Result<Self, HostProblem> {
        let idempotency_key = normalized_id(idempotency_key.into(), 128)?;
        let correlation = correlation.into();
        if correlation.is_empty()
            || correlation.len() > 246
            || correlation.chars().any(char::is_control)
            || acee_id
                .as_ref()
                .is_some_and(|value| normalized_id(value.clone(), 246).is_err())
        {
            return Err(HostProblem::Malformed);
        }
        Ok(Self {
            caller,
            acee_id,
            delegated_by,
            idempotency_key,
            correlation,
            tick,
        })
    }

    #[must_use]
    pub fn caller(&self) -> &PrincipalId {
        &self.caller
    }

    #[must_use]
    pub fn acee_id(&self) -> Option<&str> {
        self.acee_id.as_deref()
    }

    #[must_use]
    pub fn delegated_by(&self) -> Option<&PrincipalId> {
        self.delegated_by.as_ref()
    }

    #[must_use]
    pub fn idempotency_key(&self) -> &str {
        &self.idempotency_key
    }

    #[must_use]
    pub fn correlation(&self) -> &str {
        &self.correlation
    }

    #[must_use]
    pub const fn tick(&self) -> u64 {
        self.tick
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AccessEnvironment {
    pub terminal: Option<String>,
    pub console: Option<String>,
    pub system: Option<String>,
    pub application: Option<String>,
    pub tick: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SafDefineAction {
    Add,
    Alter,
    Delete,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SafVerifyAction {
    AuthenticateOnly,
    CreateAcee,
    DeleteAcee,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SafExtractKind {
    User,
    Group,
    Profile,
    Acee,
    Token,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RacrouteRequest {
    Audit {
        action: String,
        resource_digest: String,
        decision: DecisionOutcome,
        fields: BTreeMap<String, AuditFieldValue>,
    },
    Auth {
        class: String,
        resource: String,
        access: AccessLevel,
        environment: AccessEnvironment,
    },
    Define {
        action: SafDefineAction,
        class: String,
        resource: String,
        owner: String,
        uacc: AccessLevel,
        generic: bool,
    },
    Dirauth {
        node: String,
    },
    Extract {
        kind: SafExtractKind,
        class: Option<String>,
        name: String,
    },
    Fastauth {
        class: String,
        resource: String,
        access: AccessLevel,
        environment: AccessEnvironment,
    },
    List {
        class: String,
        global: bool,
        refresh: bool,
    },
    Signon {
        user: PrincipalId,
        credential_reference: SecretRef,
    },
    Stat {
        class: Option<String>,
    },
    Tokenbld {
        owner: PrincipalId,
        kind: TokenKind,
        token_reference: String,
        token_digest: String,
        scopes: BTreeSet<String>,
        expires_tick: Option<u64>,
    },
    Tokenmap {
        token_digest: String,
    },
    Tokenxtr {
        token_id: String,
    },
    Verify {
        user: PrincipalId,
        credential_reference: SecretRef,
        action: SafVerifyAction,
        acee_id: Option<String>,
    },
    Verifyx {
        user: PrincipalId,
        credential_reference: SecretRef,
        mfa_reference: Option<SecretRef>,
        action: SafVerifyAction,
        acee_id: Option<String>,
        parent_acee: Option<String>,
    },
}

impl RacrouteRequest {
    #[must_use]
    pub const fn request_type(&self) -> RacrouteRequestType {
        match self {
            Self::Audit { .. } => RacrouteRequestType::Audit,
            Self::Auth { .. } => RacrouteRequestType::Auth,
            Self::Define { .. } => RacrouteRequestType::Define,
            Self::Dirauth { .. } => RacrouteRequestType::Dirauth,
            Self::Extract { .. } => RacrouteRequestType::Extract,
            Self::Fastauth { .. } => RacrouteRequestType::Fastauth,
            Self::List { .. } => RacrouteRequestType::List,
            Self::Signon { .. } => RacrouteRequestType::Signon,
            Self::Stat { .. } => RacrouteRequestType::Stat,
            Self::Tokenbld { .. } => RacrouteRequestType::Tokenbld,
            Self::Tokenmap { .. } => RacrouteRequestType::Tokenmap,
            Self::Tokenxtr { .. } => RacrouteRequestType::Tokenxtr,
            Self::Verify { .. } => RacrouteRequestType::Verify,
            Self::Verifyx { .. } => RacrouteRequestType::Verifyx,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RacrouteState {
    Received,
    Validated,
    AceeResolved,
    PolicyResolved,
    Committed,
    Denied,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AceeSummary {
    pub id: String,
    pub principal: String,
    pub default_group: Option<String>,
    pub groups: BTreeSet<String>,
    pub parent: Option<String>,
    pub delegated_by: Option<String>,
    pub token_ids: BTreeSet<String>,
    pub version: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TokenMetadata {
    pub id: String,
    pub kind: TokenKind,
    pub owner: String,
    pub issuer: String,
    pub token_digest: String,
    pub scopes: BTreeSet<String>,
    pub issued_tick: u64,
    pub expires_tick: Option<u64>,
    pub state: TokenState,
    pub version: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExtractedSecurityRecord {
    User {
        id: String,
        owner: String,
        state: PrincipalState,
        groups: BTreeSet<String>,
        attributes: BTreeSet<String>,
    },
    Group {
        name: String,
        owner: String,
        superior_group: Option<String>,
        members: BTreeSet<String>,
    },
    Profile {
        class: String,
        name: String,
        owner: String,
        uacc: AccessLevel,
        generic: bool,
        access_entries: usize,
    },
    Acee(AceeSummary),
    Token(TokenMetadata),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RacrouteResult {
    Audit {
        audit_id: String,
    },
    Decision(SafDecision),
    Defined {
        class: String,
        resource: String,
    },
    Extracted(ExtractedSecurityRecord),
    Listed {
        class: String,
        profiles: Vec<String>,
        cache_generation: Option<u64>,
    },
    SignedOn(AceeSummary),
    Statistics {
        users: usize,
        groups: usize,
        profiles: usize,
        acees: usize,
        tokens: usize,
    },
    TokenBuilt(TokenMetadata),
    TokenMapped {
        token_id: String,
        acee: AceeSummary,
    },
    TokenExtracted(TokenMetadata),
    Verified {
        decision: SafDecision,
        acee: Option<AceeSummary>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RacrouteOutcome {
    pub request_type: RacrouteRequestType,
    pub status: SafStatus,
    pub states: Vec<RacrouteState>,
    pub result: Option<RacrouteResult>,
    pub generation: u64,
}

struct MfaProof {
    factor_id: String,
    factor_reference: String,
    valid: bool,
}

fn build_mfa_proof(
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

pub(crate) fn execute(
    service: &RacfService,
    context: &SafRequestContext,
    request: RacrouteRequest,
) -> Result<RacrouteOutcome, HostProblem> {
    let descriptor = racroute_descriptors()
        .iter()
        .copied()
        .find(|descriptor| descriptor.request_type() == request.request_type())
        .ok_or(HostProblem::Unsupported)?;
    let secret = match &request {
        RacrouteRequest::Signon {
            credential_reference,
            ..
        }
        | RacrouteRequest::Verify {
            credential_reference,
            ..
        }
        | RacrouteRequest::Verifyx {
            credential_reference,
            ..
        } => Some(service.secrets.resolve(credential_reference)?),
        _ => None,
    };
    let mfa_proof = match &request {
        RacrouteRequest::Verifyx {
            user,
            mfa_reference,
            ..
        } => build_mfa_proof(service, user.as_str(), mfa_reference.as_ref())?,
        _ => None,
    };
    let request_digest = request_digest(context, &request);
    let ((status, result, mut states), generation) = service.database.mutate_retry(|snapshot| {
        let mut states = vec![RacrouteState::Received, RacrouteState::Validated];
        let mut staged = snapshot.clone();
        let applied = apply_request(
            &mut staged,
            context,
            &request,
            secret.as_deref(),
            mfa_proof.as_ref(),
            &mut states,
        );
        match applied {
            Ok((status, mut result)) => {
                if request.request_type() != RacrouteRequestType::Audit {
                    let audit_id = append_audit(
                        &mut staged,
                        context,
                        descriptor.keyword(),
                        status,
                        &request_digest,
                    )?;
                    attach_audit(&mut result, audit_id);
                }
                states.push(if status.reason == DecisionReason::Granted {
                    RacrouteState::Committed
                } else {
                    RacrouteState::Denied
                });
                *snapshot = staged;
                Ok(((status, Some(result), states), true))
            }
            Err(reason) => {
                let status = status_for_reason(reason);
                let _ = append_audit(
                    snapshot,
                    context,
                    descriptor.keyword(),
                    status,
                    &request_digest,
                )?;
                states.push(RacrouteState::Denied);
                Ok(((status, None, states), true))
            }
        }
    })?;
    states.shrink_to_fit();
    Ok(RacrouteOutcome {
        request_type: request.request_type(),
        status,
        states,
        result,
        generation,
    })
}

fn apply_request(
    snapshot: &mut SecurityDatabaseSnapshot,
    context: &SafRequestContext,
    request: &RacrouteRequest,
    secret: Option<&[u8]>,
    mfa_proof: Option<&MfaProof>,
    states: &mut Vec<RacrouteState>,
) -> Result<(SafStatus, RacrouteResult), DecisionReason> {
    if !snapshot.principals.contains_key(context.caller().as_str()) {
        return Err(DecisionReason::PrincipalNotFound);
    }
    match request {
        RacrouteRequest::Audit {
            action,
            resource_digest,
            decision,
            fields,
        } => {
            let status = status_for_reason(DecisionReason::Granted);
            let id = next_id("AUDIT", snapshot.generation, snapshot.audits.len());
            let fields = redact_fields(fields.clone());
            snapshot.audits.push(SecurityAuditRecord {
                id: id.clone(),
                correlation: context.correlation().into(),
                actor: context.caller().as_str().into(),
                action: normalized_text(action, 246)?,
                class: None,
                resource_digest: Some(normalized_digest(resource_digest)?),
                decision: *decision,
                status,
                fields,
                tick: context.tick(),
            });
            Ok((status, RacrouteResult::Audit { audit_id: id }))
        }
        RacrouteRequest::Auth {
            class,
            resource,
            access,
            environment,
        } => {
            let principal = context_principal(snapshot, context, false, states)?;
            states.push(RacrouteState::PolicyResolved);
            let decision = evaluate_access(
                snapshot,
                &principal,
                class,
                resource,
                *access,
                environment,
                false,
            );
            Ok((decision.status, RacrouteResult::Decision(decision)))
        }
        RacrouteRequest::Fastauth {
            class,
            resource,
            access,
            environment,
        } => {
            let principal = context_principal(snapshot, context, true, states)?;
            states.push(RacrouteState::PolicyResolved);
            let decision = evaluate_access(
                snapshot,
                &principal,
                class,
                resource,
                *access,
                environment,
                true,
            );
            Ok((decision.status, RacrouteResult::Decision(decision)))
        }
        RacrouteRequest::Define {
            action,
            class,
            resource,
            owner,
            uacc,
            generic,
        } => {
            let actor = context_principal(snapshot, context, true, states)?;
            let class = normalized_class(class)?;
            let resource = normalized_profile(resource)?;
            let owner = normalized_principal(owner)?;
            if !snapshot.principals.contains_key(&owner) && !snapshot.groups.contains_key(&owner) {
                return Err(DecisionReason::PrincipalNotFound);
            }
            let class_record = snapshot
                .classes
                .get(&class)
                .ok_or(DecisionReason::ClassInactive)?;
            if *generic && !class_record.generic_allowed {
                return Err(DecisionReason::MalformedRequest);
            }
            let key = profile_key(&class, &resource);
            match action {
                SafDefineAction::Add => {
                    require_special(snapshot, &actor)?;
                    if snapshot.profiles.contains_key(&key) {
                        return Err(DecisionReason::MalformedRequest);
                    }
                    snapshot.profiles.insert(
                        key,
                        ResourceProfile {
                            class: class.clone(),
                            name: resource.clone(),
                            generic: *generic,
                            owner,
                            uacc: *uacc,
                            audit: AuditPolicy::Failures,
                            security_level: 0,
                            security_label: None,
                            categories: BTreeSet::new(),
                            access_list: snapshot
                                .policy
                                .add_creator
                                .then(|| crate::AccessControlEntry {
                                    principal: actor.clone(),
                                    access: AccessLevel::Alter,
                                    when: None,
                                    audit: AuditPolicy::None,
                                })
                                .into_iter()
                                .collect(),
                            segments: BTreeMap::new(),
                            version: 1,
                        },
                    );
                }
                SafDefineAction::Alter => {
                    let special = is_special(snapshot, &actor);
                    let profile = snapshot
                        .profiles
                        .get_mut(&key)
                        .ok_or(DecisionReason::ProfileNotFound)?;
                    if !special && profile.owner != actor {
                        return Err(DecisionReason::InsufficientAccess);
                    }
                    profile.owner = owner;
                    profile.uacc = *uacc;
                    profile.version = profile
                        .version
                        .checked_add(1)
                        .ok_or(DecisionReason::ResourceExhausted)?;
                }
                SafDefineAction::Delete => {
                    let profile = snapshot
                        .profiles
                        .get(&key)
                        .ok_or(DecisionReason::ProfileNotFound)?;
                    if !is_special(snapshot, &actor) && profile.owner != actor {
                        return Err(DecisionReason::InsufficientAccess);
                    }
                    snapshot.profiles.remove(&key);
                }
            }
            states.push(RacrouteState::PolicyResolved);
            Ok((
                status_for_reason(DecisionReason::Granted),
                RacrouteResult::Defined { class, resource },
            ))
        }
        RacrouteRequest::Dirauth { node } => {
            let principal = context_principal(snapshot, context, true, states)?;
            let resource = format!("DIRECT.{}", normalized_id(node.clone(), 32)?);
            let decision = evaluate_access(
                snapshot,
                &principal,
                "RRSFDATA",
                &resource,
                AccessLevel::Read,
                &AccessEnvironment::default(),
                false,
            );
            states.push(RacrouteState::PolicyResolved);
            Ok((decision.status, RacrouteResult::Decision(decision)))
        }
        RacrouteRequest::Extract { kind, class, name } => {
            let actor = context_principal(snapshot, context, true, states)?;
            let record = extract(snapshot, &actor, *kind, class.as_deref(), name)?;
            Ok((
                status_for_reason(DecisionReason::Granted),
                RacrouteResult::Extracted(record),
            ))
        }
        RacrouteRequest::List {
            class,
            global,
            refresh,
        } => {
            let actor = context_principal(snapshot, context, true, states)?;
            let class = normalized_class(class)?;
            if *global || *refresh {
                require_special(snapshot, &actor)?;
                let descriptor = snapshot
                    .classes
                    .get_mut(&class)
                    .ok_or(DecisionReason::ClassInactive)?;
                descriptor.raclist = true;
                descriptor.version = descriptor
                    .version
                    .checked_add(1)
                    .ok_or(DecisionReason::ResourceExhausted)?;
                refresh_cache(snapshot, &class)?;
            }
            let profiles = snapshot
                .profiles
                .values()
                .filter(|profile| profile.class == class)
                .map(|profile| profile.name.clone())
                .collect();
            states.push(RacrouteState::PolicyResolved);
            Ok((
                status_for_reason(DecisionReason::Granted),
                RacrouteResult::Listed {
                    class: class.clone(),
                    profiles,
                    cache_generation: snapshot
                        .raclist_caches
                        .get(&class)
                        .map(|cache| cache.built_generation),
                },
            ))
        }
        RacrouteRequest::Signon { user, .. } => verify_request(
            snapshot,
            context,
            user,
            SafVerifyAction::CreateAcee,
            None,
            None,
            secret,
            None,
            states,
            true,
        ),
        RacrouteRequest::Stat { class } => {
            let _ = context_principal(snapshot, context, true, states)?;
            let class = class
                .as_ref()
                .map(|class| normalized_class(class))
                .transpose()?;
            let profiles = class.as_ref().map_or(snapshot.profiles.len(), |class| {
                snapshot
                    .profiles
                    .values()
                    .filter(|profile| &profile.class == class)
                    .count()
            });
            Ok((
                status_for_reason(DecisionReason::Granted),
                RacrouteResult::Statistics {
                    users: snapshot.principals.len(),
                    groups: snapshot.groups.len(),
                    profiles,
                    acees: snapshot.acees.len(),
                    tokens: snapshot.tokens.len(),
                },
            ))
        }
        RacrouteRequest::Tokenbld {
            owner,
            kind,
            token_reference,
            token_digest,
            scopes,
            expires_tick,
        } => {
            let actor = context_principal(snapshot, context, true, states)?;
            if actor != owner.as_str() && !is_special(snapshot, &actor) {
                return Err(DecisionReason::InsufficientAccess);
            }
            if !snapshot.principals.contains_key(owner.as_str())
                || expires_tick.is_some_and(|expiry| expiry <= context.tick())
            {
                return Err(DecisionReason::MalformedRequest);
            }
            let id = next_id("TOKEN", snapshot.generation, snapshot.tokens.len());
            let token = SecurityToken {
                id: id.clone(),
                kind: *kind,
                owner: owner.as_str().into(),
                issuer: actor,
                audience: None,
                token_reference: normalized_reference(token_reference)?,
                token_digest: normalized_digest(token_digest)?,
                scopes: scopes.clone(),
                issued_tick: context.tick(),
                expires_tick: *expires_tick,
                state: TokenState::Active,
                version: 1,
            };
            snapshot.tokens.insert(id, token.clone());
            Ok((
                status_for_reason(DecisionReason::Granted),
                RacrouteResult::TokenBuilt(token_metadata(&token)),
            ))
        }
        RacrouteRequest::Tokenmap { token_digest } => {
            let digest = normalized_digest(token_digest)?;
            let token = snapshot
                .tokens
                .values()
                .find(|token| token.token_digest == digest)
                .cloned()
                .ok_or(DecisionReason::TokenInvalid)?;
            if token.state != TokenState::Active
                || token
                    .expires_tick
                    .is_some_and(|expiry| expiry <= context.tick())
            {
                return Err(DecisionReason::TokenInvalid);
            }
            let acee = create_acee(
                snapshot,
                context,
                &token.owner,
                None,
                BTreeSet::from([token.id.clone()]),
            )?;
            Ok((
                status_for_reason(DecisionReason::Granted),
                RacrouteResult::TokenMapped {
                    token_id: token.id,
                    acee: acee_summary(&acee),
                },
            ))
        }
        RacrouteRequest::Tokenxtr { token_id } => {
            let actor = context_principal(snapshot, context, true, states)?;
            let token_id = normalized_id(token_id.clone(), 246)?;
            let token = snapshot
                .tokens
                .get(&token_id)
                .ok_or(DecisionReason::TokenInvalid)?;
            if actor != token.owner && !is_special(snapshot, &actor) {
                return Err(DecisionReason::InsufficientAccess);
            }
            Ok((
                status_for_reason(DecisionReason::Granted),
                RacrouteResult::TokenExtracted(token_metadata(token)),
            ))
        }
        RacrouteRequest::Verify {
            user,
            action,
            acee_id,
            ..
        } => verify_request(
            snapshot,
            context,
            user,
            *action,
            acee_id.as_deref(),
            None,
            secret,
            None,
            states,
            false,
        ),
        RacrouteRequest::Verifyx {
            user,
            action,
            acee_id,
            parent_acee,
            ..
        } => verify_request(
            snapshot,
            context,
            user,
            *action,
            acee_id.as_deref(),
            parent_acee.as_deref(),
            secret,
            mfa_proof,
            states,
            false,
        ),
    }
}

#[allow(clippy::too_many_arguments)]
fn verify_request(
    snapshot: &mut SecurityDatabaseSnapshot,
    context: &SafRequestContext,
    user: &PrincipalId,
    action: SafVerifyAction,
    acee_id: Option<&str>,
    parent_acee: Option<&str>,
    secret: Option<&[u8]>,
    mfa_proof: Option<&MfaProof>,
    states: &mut Vec<RacrouteState>,
    signon: bool,
) -> Result<(SafStatus, RacrouteResult), DecisionReason> {
    if action == SafVerifyAction::DeleteAcee {
        let id = normalized_id(
            acee_id.ok_or(DecisionReason::MalformedRequest)?.to_string(),
            246,
        )?;
        let special = is_special(snapshot, context.caller().as_str());
        let acee = snapshot
            .acees
            .get_mut(&id)
            .ok_or(DecisionReason::AceeInvalid)?;
        if acee.principal != context.caller().as_str() && !special {
            return Err(DecisionReason::InsufficientAccess);
        }
        acee.state = AceeState::Deleted;
        acee.version = acee
            .version
            .checked_add(1)
            .ok_or(DecisionReason::ResourceExhausted)?;
        for session in snapshot
            .signon_sessions
            .values_mut()
            .filter(|session| session.acee_id == id)
        {
            session.state = SignonSessionState::SignedOff;
            session.version = session
                .version
                .checked_add(1)
                .ok_or(DecisionReason::ResourceExhausted)?;
        }
        states.push(RacrouteState::AceeResolved);
        let decision = decision(DecisionReason::Granted, AccessLevel::None, None, None);
        return Ok((
            decision.status,
            RacrouteResult::Verified {
                decision,
                acee: None,
            },
        ));
    }
    let mut reason = verify_credential(
        snapshot,
        user.as_str(),
        secret.ok_or(DecisionReason::CredentialInvalid)?,
    )?;
    if reason == DecisionReason::Granted {
        let configured = snapshot
            .mfa_factors
            .values()
            .find(|factor| factor.owner == user.as_str() && factor.active);
        if let Some(configured) = configured
            && !mfa_proof.is_some_and(|proof| {
                proof.valid
                    && proof.factor_id == configured.id
                    && proof.factor_reference == configured.secret_reference
            })
        {
            reason = DecisionReason::MfaInvalid;
        }
    }
    let mut acee = None;
    if reason == DecisionReason::Granted && action == SafVerifyAction::CreateAcee {
        let created = create_acee(
            snapshot,
            context,
            user.as_str(),
            parent_acee,
            BTreeSet::new(),
        )?;
        states.push(RacrouteState::AceeResolved);
        acee = Some(acee_summary(&created));
    }
    let decision = decision(reason, AccessLevel::None, None, None);
    let result = if signon && reason == DecisionReason::Granted {
        let signed_on = acee.clone().ok_or(DecisionReason::AceeInvalid)?;
        let session_id = next_id(
            "SESSION",
            snapshot.generation,
            snapshot.signon_sessions.len(),
        );
        snapshot.signon_sessions.insert(
            session_id.clone(),
            SignonSession {
                id: session_id,
                user: signed_on.principal.clone(),
                node: None,
                acee_id: signed_on.id.clone(),
                created_tick: context.tick(),
                state: SignonSessionState::Active,
                version: 1,
            },
        );
        RacrouteResult::SignedOn(signed_on)
    } else {
        RacrouteResult::Verified {
            decision: decision.clone(),
            acee,
        }
    };
    Ok((decision.status, result))
}

fn verify_credential(
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
    let parsed = PasswordHash::new(&credential.encoded_verifier)
        .map_err(|_| DecisionReason::PolicyUnavailable)?;
    Ok(
        if Argon2::default().verify_password(secret, &parsed).is_ok() {
            DecisionReason::Granted
        } else {
            DecisionReason::CredentialInvalid
        },
    )
}

fn create_acee(
    snapshot: &mut SecurityDatabaseSnapshot,
    context: &SafRequestContext,
    principal: &str,
    parent: Option<&str>,
    token_ids: BTreeSet<String>,
) -> Result<Acee, DecisionReason> {
    let profile = snapshot
        .principals
        .get(principal)
        .ok_or(DecisionReason::PrincipalNotFound)?;
    if let Some(parent) = parent {
        let parent_acee = snapshot
            .acees
            .get(parent)
            .ok_or(DecisionReason::AceeInvalid)?;
        if parent_acee.state != AceeState::Active
            || parent_acee.principal != context.caller().as_str()
                && !is_special(snapshot, context.caller().as_str())
        {
            return Err(DecisionReason::InsufficientAccess);
        }
    }
    let groups = effective_groups(snapshot, principal);
    let id = next_id("ACEE", snapshot.generation, snapshot.acees.len());
    let acee = Acee {
        id: id.clone(),
        principal: principal.into(),
        default_group: profile.default_group.clone(),
        groups,
        parent: parent.map(str::to_string),
        delegated_by: context.delegated_by().map(|value| value.as_str().into()),
        security_level: profile.security_level,
        security_label: profile.security_label.clone(),
        categories: profile.categories.clone(),
        token_ids,
        created_tick: context.tick(),
        state: AceeState::Active,
        version: 1,
    };
    snapshot.acees.insert(id, acee.clone());
    Ok(acee)
}

pub(crate) fn evaluate_access(
    snapshot: &SecurityDatabaseSnapshot,
    principal: &str,
    class: &str,
    resource: &str,
    requested: AccessLevel,
    environment: &AccessEnvironment,
    fastauth: bool,
) -> SafDecision {
    let Ok(class) = normalized_class(class) else {
        return decision(
            DecisionReason::MalformedRequest,
            AccessLevel::None,
            None,
            None,
        );
    };
    let Ok(resource) = normalized_profile(resource) else {
        return decision(
            DecisionReason::MalformedRequest,
            AccessLevel::None,
            None,
            None,
        );
    };
    if !snapshot.subsystem.running || !snapshot.database_status.active {
        return decision(
            DecisionReason::PolicyUnavailable,
            AccessLevel::None,
            None,
            None,
        );
    }
    let Some(user) = snapshot.principals.get(principal) else {
        return decision(
            DecisionReason::PrincipalNotFound,
            AccessLevel::None,
            None,
            None,
        );
    };
    if !matches!(
        user.state,
        PrincipalState::Active | PrincipalState::PasswordExpired
    ) {
        return decision(
            DecisionReason::PrincipalInactive,
            AccessLevel::None,
            None,
            None,
        );
    }
    let Some(class_record) = snapshot.classes.get(&class) else {
        return decision(DecisionReason::ClassInactive, AccessLevel::None, None, None);
    };
    if !class_record.active || class == "PROGRAM" && !snapshot.policy.program_control {
        return decision(DecisionReason::ClassInactive, AccessLevel::None, None, None);
    }
    let cache = class_record
        .raclist
        .then(|| snapshot.raclist_caches.get(&class))
        .flatten();
    if (fastauth || class_record.raclist) && cache.is_none() {
        return decision(
            DecisionReason::PolicyUnavailable,
            AccessLevel::None,
            None,
            None,
        );
    }
    let profiles = cache.map_or_else(
        || snapshot.profiles.values().collect::<Vec<_>>(),
        |cache| cache.profiles.values().collect::<Vec<_>>(),
    );
    let selected = profiles
        .into_iter()
        .filter(|profile| {
            profile.class == class
                && if profile.generic {
                    class_record.generic_active && generic_match(&profile.name, &resource)
                } else {
                    profile.name == resource
                }
        })
        .max_by_key(|profile| specificity(&profile.name));
    let Some(profile) = selected else {
        return decision(
            DecisionReason::ProfileNotFound,
            AccessLevel::None,
            None,
            cache.map(|cache| cache.built_generation),
        );
    };
    if profile.security_level > user.security_level
        || profile
            .security_label
            .as_ref()
            .is_some_and(|label| user.security_label.as_ref() != Some(label))
        || !profile.categories.is_subset(&user.categories)
    {
        return decision(
            DecisionReason::SecurityLabelMismatch,
            AccessLevel::None,
            Some(profile.name.clone()),
            cache.map(|cache| cache.built_generation),
        );
    }
    let groups = effective_groups(snapshot, principal);
    let mut direct = Vec::new();
    let mut group = Vec::new();
    for entry in &profile.access_list {
        if entry.principal == principal {
            direct.push(entry);
        } else if groups.contains(&entry.principal) {
            group.push(entry);
        }
    }
    let matching = if direct.is_empty() { group } else { direct };
    let mut granted = None;
    let mut conditional_seen = false;
    for entry in matching {
        if let Some(condition) = &entry.when {
            conditional_seen = true;
            if !condition_matches(condition, environment) {
                continue;
            }
        }
        if granted.is_none_or(|current: AccessLevel| entry.access > current) {
            granted = Some(entry.access);
        }
    }
    let granted = granted.unwrap_or(profile.uacc);
    let reason = if granted.permits(requested) {
        DecisionReason::Granted
    } else if conditional_seen {
        DecisionReason::ConditionNotSatisfied
    } else {
        DecisionReason::InsufficientAccess
    };
    decision(
        reason,
        granted,
        Some(profile.name.clone()),
        cache.map(|cache| cache.built_generation),
    )
}

pub(crate) fn status_for_reason(reason: DecisionReason) -> SafStatus {
    match reason {
        DecisionReason::Granted => SafStatus {
            saf_return_code: 0,
            racf_return_code: 0,
            racf_reason_code: 0,
            reason,
        },
        DecisionReason::DefaultDeny
        | DecisionReason::InsufficientAccess
        | DecisionReason::ConditionNotSatisfied
        | DecisionReason::SecurityLabelMismatch => SafStatus {
            saf_return_code: 8,
            racf_return_code: 8,
            racf_reason_code: 4,
            reason,
        },
        DecisionReason::ProfileNotFound | DecisionReason::PrincipalNotFound => SafStatus {
            saf_return_code: 8,
            racf_return_code: 8,
            racf_reason_code: 8,
            reason,
        },
        DecisionReason::ResourceExhausted => SafStatus {
            saf_return_code: 12,
            racf_return_code: 12,
            racf_reason_code: 16,
            reason,
        },
        DecisionReason::PolicyUnavailable | DecisionReason::StoreUnavailable => SafStatus {
            saf_return_code: 12,
            racf_return_code: 12,
            racf_reason_code: 20,
            reason,
        },
        _ => SafStatus {
            saf_return_code: 8,
            racf_return_code: 8,
            racf_reason_code: 12,
            reason,
        },
    }
}

fn decision(
    reason: DecisionReason,
    granted_access: AccessLevel,
    matched_profile: Option<String>,
    cache_generation: Option<u64>,
) -> SafDecision {
    SafDecision {
        outcome: if reason == DecisionReason::Granted {
            DecisionOutcome::Allow
        } else {
            DecisionOutcome::Deny
        },
        status: status_for_reason(reason),
        granted_access,
        matched_profile,
        audit_id: None,
        cache_generation,
    }
}

fn context_principal(
    snapshot: &SecurityDatabaseSnapshot,
    context: &SafRequestContext,
    require_acee: bool,
    states: &mut Vec<RacrouteState>,
) -> Result<String, DecisionReason> {
    if let Some(id) = context.acee_id() {
        let acee = snapshot.acees.get(id).ok_or(DecisionReason::AceeInvalid)?;
        if acee.state != AceeState::Active
            || acee.principal != context.caller().as_str()
                && !is_special(snapshot, context.caller().as_str())
        {
            return Err(DecisionReason::AceeInvalid);
        }
        states.push(RacrouteState::AceeResolved);
        Ok(acee.principal.clone())
    } else if require_acee {
        Err(DecisionReason::AceeInvalid)
    } else {
        Ok(context.caller().as_str().into())
    }
}

fn extract(
    snapshot: &SecurityDatabaseSnapshot,
    actor: &str,
    kind: SafExtractKind,
    class: Option<&str>,
    name: &str,
) -> Result<ExtractedSecurityRecord, DecisionReason> {
    match kind {
        SafExtractKind::User => {
            let name = normalized_principal(name)?;
            if actor != name && !is_auditor_or_special(snapshot, actor) {
                return Err(DecisionReason::InsufficientAccess);
            }
            let user = snapshot
                .principals
                .get(&name)
                .ok_or(DecisionReason::PrincipalNotFound)?;
            Ok(ExtractedSecurityRecord::User {
                id: user.id.clone(),
                owner: user.owner.clone(),
                state: user.state,
                groups: effective_groups(snapshot, &name),
                attributes: user.attributes.clone(),
            })
        }
        SafExtractKind::Group => {
            let name = normalized_principal(name)?;
            let group = snapshot
                .groups
                .get(&name)
                .ok_or(DecisionReason::PrincipalNotFound)?;
            Ok(ExtractedSecurityRecord::Group {
                name: group.name.clone(),
                owner: group.owner.clone(),
                superior_group: group.superior_group.clone(),
                members: snapshot
                    .connections
                    .values()
                    .filter(|connection| connection.group == name && !connection.revoked)
                    .map(|connection| connection.user.clone())
                    .collect(),
            })
        }
        SafExtractKind::Profile => {
            let class = normalized_class(class.ok_or(DecisionReason::MalformedRequest)?)?;
            let name = normalized_profile(name)?;
            let profile = snapshot
                .profiles
                .get(&profile_key(&class, &name))
                .ok_or(DecisionReason::ProfileNotFound)?;
            Ok(ExtractedSecurityRecord::Profile {
                class,
                name,
                owner: profile.owner.clone(),
                uacc: profile.uacc,
                generic: profile.generic,
                access_entries: profile.access_list.len(),
            })
        }
        SafExtractKind::Acee => snapshot
            .acees
            .get(&normalized_id(name.to_string(), 246)?)
            .map(|acee| ExtractedSecurityRecord::Acee(acee_summary(acee)))
            .ok_or(DecisionReason::AceeInvalid),
        SafExtractKind::Token => {
            let token = snapshot
                .tokens
                .get(&normalized_id(name.to_string(), 246)?)
                .ok_or(DecisionReason::TokenInvalid)?;
            if actor != token.owner && !is_special(snapshot, actor) {
                return Err(DecisionReason::InsufficientAccess);
            }
            Ok(ExtractedSecurityRecord::Token(token_metadata(token)))
        }
    }
}

fn refresh_cache(
    snapshot: &mut SecurityDatabaseSnapshot,
    class: &str,
) -> Result<(), DecisionReason> {
    let profiles = snapshot
        .profiles
        .iter()
        .filter(|(_, profile)| profile.class == class)
        .map(|(key, profile)| (key.clone(), profile.clone()))
        .collect();
    let version = snapshot.raclist_caches.get(class).map_or(Ok(1), |cache| {
        cache
            .version
            .checked_add(1)
            .ok_or(DecisionReason::ResourceExhausted)
    })?;
    snapshot.raclist_caches.insert(
        class.into(),
        RaclistCache {
            class: class.into(),
            built_generation: snapshot.generation,
            profiles,
            version,
        },
    );
    Ok(())
}

fn append_audit(
    snapshot: &mut SecurityDatabaseSnapshot,
    context: &SafRequestContext,
    action: &str,
    status: SafStatus,
    request_digest: &str,
) -> Result<String, HostProblem> {
    if snapshot.audits.len() >= 65_536 {
        return Err(HostProblem::ResourceExhausted);
    }
    let id = next_id("AUDIT", snapshot.generation, snapshot.audits.len());
    snapshot.audits.push(SecurityAuditRecord {
        id: id.clone(),
        correlation: context.correlation().into(),
        actor: context.caller().as_str().into(),
        action: action.into(),
        class: None,
        resource_digest: Some(request_digest.into()),
        decision: if status.reason == DecisionReason::Granted {
            DecisionOutcome::Allow
        } else {
            DecisionOutcome::Deny
        },
        status,
        fields: BTreeMap::from([(
            "ACEE".into(),
            context.acee_id().map_or(AuditFieldValue::Redacted, |id| {
                AuditFieldValue::Reference(id.into())
            }),
        )]),
        tick: context.tick(),
    });
    Ok(id)
}

fn attach_audit(result: &mut RacrouteResult, audit_id: String) {
    match result {
        RacrouteResult::Decision(decision) | RacrouteResult::Verified { decision, .. } => {
            decision.audit_id = Some(audit_id)
        }
        _ => {}
    }
}

fn effective_groups(snapshot: &SecurityDatabaseSnapshot, principal: &str) -> BTreeSet<String> {
    let mut groups = snapshot
        .connections
        .values()
        .filter(|connection| connection.user == principal && !connection.revoked)
        .map(|connection| connection.group.clone())
        .collect::<BTreeSet<_>>();
    let mut pending = groups.iter().cloned().collect::<Vec<_>>();
    while let Some(group) = pending.pop() {
        if let Some(superior) = snapshot
            .groups
            .get(&group)
            .and_then(|profile| profile.superior_group.clone())
            && groups.insert(superior.clone())
        {
            pending.push(superior);
        }
    }
    groups
}

fn condition_matches(condition: &AccessCondition, environment: &AccessEnvironment) -> bool {
    condition
        .terminal
        .as_ref()
        .is_none_or(|value| environment.terminal.as_ref() == Some(value))
        && condition
            .console
            .as_ref()
            .is_none_or(|value| environment.console.as_ref() == Some(value))
        && condition
            .system
            .as_ref()
            .is_none_or(|value| environment.system.as_ref() == Some(value))
        && condition
            .application
            .as_ref()
            .is_none_or(|value| environment.application.as_ref() == Some(value))
        && condition
            .start_tick
            .is_none_or(|start| environment.tick >= start)
        && condition.end_tick.is_none_or(|end| environment.tick < end)
}

fn generic_match(pattern: &str, value: &str) -> bool {
    let pattern = pattern.split('.').collect::<Vec<_>>();
    let value = value.split('.').collect::<Vec<_>>();
    match_parts(&pattern, &value)
}

fn match_parts(pattern: &[&str], value: &[&str]) -> bool {
    if pattern.is_empty() {
        return value.is_empty();
    }
    if pattern[0] == "**" {
        return (0..=value.len()).any(|skip| match_parts(&pattern[1..], &value[skip..]));
    }
    !value.is_empty() && qualifier(pattern[0], value[0]) && match_parts(&pattern[1..], &value[1..])
}

fn qualifier(pattern: &str, value: &str) -> bool {
    pattern == "*"
        || pattern.len() == value.len()
            && pattern
                .bytes()
                .zip(value.bytes())
                .all(|(pattern, value)| pattern == b'%' || pattern == value)
}

fn specificity(pattern: &str) -> (usize, usize) {
    (
        pattern
            .bytes()
            .filter(|byte| !matches!(byte, b'*' | b'%'))
            .count(),
        usize::MAX - pattern.matches("**").count(),
    )
}

fn is_special(snapshot: &SecurityDatabaseSnapshot, principal: &str) -> bool {
    snapshot
        .principals
        .get(principal)
        .is_some_and(|profile| profile.attributes.contains("SPECIAL"))
}

fn is_auditor_or_special(snapshot: &SecurityDatabaseSnapshot, principal: &str) -> bool {
    snapshot.principals.get(principal).is_some_and(|profile| {
        profile.attributes.contains("SPECIAL") || profile.attributes.contains("AUDITOR")
    })
}

fn require_special(
    snapshot: &SecurityDatabaseSnapshot,
    principal: &str,
) -> Result<(), DecisionReason> {
    if is_special(snapshot, principal) {
        Ok(())
    } else {
        Err(DecisionReason::InsufficientAccess)
    }
}

fn acee_summary(acee: &Acee) -> AceeSummary {
    AceeSummary {
        id: acee.id.clone(),
        principal: acee.principal.clone(),
        default_group: acee.default_group.clone(),
        groups: acee.groups.clone(),
        parent: acee.parent.clone(),
        delegated_by: acee.delegated_by.clone(),
        token_ids: acee.token_ids.clone(),
        version: acee.version,
    }
}

fn token_metadata(token: &SecurityToken) -> TokenMetadata {
    TokenMetadata {
        id: token.id.clone(),
        kind: token.kind,
        owner: token.owner.clone(),
        issuer: token.issuer.clone(),
        token_digest: token.token_digest.clone(),
        scopes: token.scopes.clone(),
        issued_tick: token.issued_tick,
        expires_tick: token.expires_tick,
        state: token.state,
        version: token.version,
    }
}

fn redact_fields(
    mut fields: BTreeMap<String, AuditFieldValue>,
) -> BTreeMap<String, AuditFieldValue> {
    for (name, value) in &mut fields {
        let upper = name.to_ascii_uppercase();
        if [
            "PASSWORD",
            "PHRASE",
            "CREDENTIAL",
            "SECRET",
            "TOKEN",
            "KEY",
            "CERTIFICATE",
        ]
        .iter()
        .any(|marker| upper.contains(marker))
        {
            *value = AuditFieldValue::Redacted;
        }
    }
    fields
}

fn request_digest(context: &SafRequestContext, request: &RacrouteRequest) -> String {
    let mut digest = Sha256::new();
    digest.update(b"mainframe-env.racroute-request@1\0");
    digest.update(context.caller().as_str().as_bytes());
    digest.update(context.idempotency_key().as_bytes());
    digest.update(format!("{:?}", request.request_type()).as_bytes());
    format!("sha256:{:x}", digest.finalize())
}

fn next_id(prefix: &str, generation: u64, count: usize) -> String {
    format!("{prefix}{generation:020}{count:06}")
}

fn normalized_principal(value: &str) -> Result<String, DecisionReason> {
    normalized(value, 8, false)
}

fn normalized_class(value: &str) -> Result<String, DecisionReason> {
    normalized(value, 32, false)
}

fn normalized_profile(value: &str) -> Result<String, DecisionReason> {
    normalized(value, 246, true)
}

fn normalized(value: &str, max: usize, generic: bool) -> Result<String, DecisionReason> {
    let value = value.to_ascii_uppercase();
    if value.is_empty()
        || value.len() > max
        || value.bytes().any(|byte| {
            !(byte.is_ascii_alphanumeric()
                || matches!(byte, b'@' | b'#' | b'$' | b'.' | b'-' | b'_')
                || generic && matches!(byte, b'*' | b'%'))
        })
    {
        Err(DecisionReason::MalformedRequest)
    } else {
        Ok(value)
    }
}

fn normalized_id(value: String, max: usize) -> Result<String, HostProblem> {
    let value = value.to_ascii_uppercase();
    if value.is_empty()
        || value.len() > max
        || value.bytes().any(|byte| {
            !(byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b':' | b'-' | b'_'))
        })
    {
        Err(HostProblem::Malformed)
    } else {
        Ok(value)
    }
}

fn normalized_text(value: &str, max: usize) -> Result<String, DecisionReason> {
    if value.is_empty() || value.len() > max || value.chars().any(char::is_control) {
        Err(DecisionReason::MalformedRequest)
    } else {
        Ok(value.into())
    }
}

fn normalized_reference(value: &str) -> Result<String, DecisionReason> {
    if value.is_empty()
        || value.len() > 4096
        || !value.contains(':')
        || value.contains(char::is_whitespace)
    {
        Err(DecisionReason::MalformedRequest)
    } else {
        Ok(value.into())
    }
}

fn normalized_digest(value: &str) -> Result<String, DecisionReason> {
    if value.len() == 71
        && value.starts_with("sha256:")
        && value[7..].bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        Ok(value.to_ascii_lowercase())
    } else {
        Err(DecisionReason::MalformedRequest)
    }
}

impl From<HostProblem> for DecisionReason {
    fn from(problem: HostProblem) -> Self {
        match problem {
            HostProblem::ResourceExhausted => Self::ResourceExhausted,
            HostProblem::NotFound => Self::ProfileNotFound,
            HostProblem::Unauthorized => Self::InsufficientAccess,
            _ => Self::MalformedRequest,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CommandContext, MemorySecretResolver};
    use mainframe_env_execution_api::InvocationLimits;
    use mainframe_env_store::MemoryStore;
    use mainframe_env_store_api::ProviderStateStore;
    use std::sync::Arc;

    fn setup() -> (Arc<RacfService>, Arc<MemorySecretResolver>, PrincipalId) {
        let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
        let resolver = Arc::new(MemorySecretResolver::default());
        resolver.insert("secret:admin", b"ADMIN-PASSWORD".to_vec());
        resolver.insert("secret:user1", b"USER-PASSWORD".to_vec());
        let service = RacfService::open(store, resolver.clone(), Default::default()).unwrap();
        service
            .bootstrap_administrator(
                "RACFADM",
                &SecretRef::new("secret:admin", Default::default()).unwrap(),
            )
            .unwrap();
        let admin = PrincipalId::new("RACFADM", InvocationLimits::default()).unwrap();
        (service, resolver, admin)
    }

    fn command_context(actor: &PrincipalId, id: &str, tick: u64) -> CommandContext {
        CommandContext::new(actor.clone(), id, "SAF-SETUP", tick).unwrap()
    }

    fn saf_context(
        caller: &PrincipalId,
        acee: Option<&str>,
        id: &str,
        tick: u64,
    ) -> SafRequestContext {
        SafRequestContext::new(
            caller.clone(),
            acee.map(str::to_string),
            None,
            id,
            "SAF-TEST",
            tick,
        )
        .unwrap()
    }

    fn extract_acee(outcome: &RacrouteOutcome) -> String {
        match outcome.result.as_ref().unwrap() {
            RacrouteResult::Verified {
                acee: Some(acee), ..
            }
            | RacrouteResult::SignedOn(acee) => acee.id.clone(),
            other => panic!("expected ACEE result, got {other:?}"),
        }
    }

    #[test]
    fn generated_registry_and_all_fourteen_state_machines_execute() {
        let (service, _, admin) = setup();
        assert_eq!(racroute_descriptors().len(), 14);
        assert_eq!(
            racroute_descriptors()
                .iter()
                .map(|descriptor| descriptor.request_type())
                .collect::<BTreeSet<_>>()
                .len(),
            14
        );
        for (index, command) in [
            "ADDUSER USER1 PASSWORD('USER-PASSWORD')",
            "RDEFINE FACILITY APP.** OWNER(RACFADM) UACC(ALTER)",
            "RDEFINE RRSFDATA DIRECT.NODE1 OWNER(RACFADM) UACC(READ)",
            "SETROPTS CLASSACT(RRSFDATA) RACLIST(FACILITY)",
        ]
        .into_iter()
        .enumerate()
        {
            service
                .execute_command(
                    &command_context(&admin, &format!("SETUP-{index}"), index as u64 + 1),
                    command,
                )
                .unwrap();
        }

        let verify = service
            .racroute(
                &saf_context(&admin, None, "VERIFY-1", 10),
                RacrouteRequest::Verify {
                    user: admin.clone(),
                    credential_reference: SecretRef::new("secret:admin", Default::default())
                        .unwrap(),
                    action: SafVerifyAction::CreateAcee,
                    acee_id: None,
                },
            )
            .unwrap();
        let admin_acee = extract_acee(&verify);
        let acee_context = |id: &str, tick: u64| saf_context(&admin, Some(&admin_acee), id, tick);
        let digest = |byte: char| format!("sha256:{}", byte.to_string().repeat(64));
        let mut reached = BTreeSet::from([verify.request_type]);

        let requests = vec![
            (
                saf_context(&admin, None, "AUDIT-1", 11),
                RacrouteRequest::Audit {
                    action: "SAF.TEST".into(),
                    resource_digest: digest('1'),
                    decision: DecisionOutcome::Allow,
                    fields: BTreeMap::from([(
                        "PASSWORD".into(),
                        AuditFieldValue::Text("NEVER-STORED".into()),
                    )]),
                },
            ),
            (
                saf_context(&admin, None, "AUTH-1", 12),
                RacrouteRequest::Auth {
                    class: "FACILITY".into(),
                    resource: "APP.ONE".into(),
                    access: AccessLevel::Read,
                    environment: Default::default(),
                },
            ),
            (
                acee_context("DEFINE-1", 13),
                RacrouteRequest::Define {
                    action: SafDefineAction::Add,
                    class: "FACILITY".into(),
                    resource: "SAF.DEFINED".into(),
                    owner: "RACFADM".into(),
                    uacc: AccessLevel::Read,
                    generic: false,
                },
            ),
            (
                acee_context("DIRAUTH-1", 14),
                RacrouteRequest::Dirauth {
                    node: "NODE1".into(),
                },
            ),
            (
                acee_context("EXTRACT-1", 15),
                RacrouteRequest::Extract {
                    kind: SafExtractKind::User,
                    class: None,
                    name: "RACFADM".into(),
                },
            ),
            (
                acee_context("FASTAUTH-1", 16),
                RacrouteRequest::Fastauth {
                    class: "FACILITY".into(),
                    resource: "APP.ONE".into(),
                    access: AccessLevel::Read,
                    environment: Default::default(),
                },
            ),
            (
                acee_context("LIST-1", 17),
                RacrouteRequest::List {
                    class: "FACILITY".into(),
                    global: true,
                    refresh: true,
                },
            ),
            (
                saf_context(&admin, None, "SIGNON-1", 18),
                RacrouteRequest::Signon {
                    user: PrincipalId::new("USER1", InvocationLimits::default()).unwrap(),
                    credential_reference: SecretRef::new("secret:user1", Default::default())
                        .unwrap(),
                },
            ),
            (
                acee_context("STAT-1", 19),
                RacrouteRequest::Stat { class: None },
            ),
            (
                acee_context("TOKENBLD-1", 20),
                RacrouteRequest::Tokenbld {
                    owner: admin.clone(),
                    kind: TokenKind::SafIdentity,
                    token_reference: "secret:token-one".into(),
                    token_digest: digest('2'),
                    scopes: BTreeSet::from(["FACILITY".into()]),
                    expires_tick: Some(100),
                },
            ),
            (
                saf_context(&admin, None, "TOKENMAP-1", 21),
                RacrouteRequest::Tokenmap {
                    token_digest: digest('2'),
                },
            ),
            (
                acee_context("TOKENXTR-1", 22),
                RacrouteRequest::Tokenxtr {
                    token_id: "TOKEN00000000000000000008000000".into(),
                },
            ),
            (
                SafRequestContext::new(
                    admin.clone(),
                    None,
                    Some(admin.clone()),
                    "VERIFYX-1",
                    "SAF-TEST",
                    23,
                )
                .unwrap(),
                RacrouteRequest::Verifyx {
                    user: admin.clone(),
                    credential_reference: SecretRef::new("secret:admin", Default::default())
                        .unwrap(),
                    mfa_reference: None,
                    action: SafVerifyAction::CreateAcee,
                    acee_id: None,
                    parent_acee: Some(admin_acee.clone()),
                },
            ),
        ];
        let mut token_id = None;
        for (context, request) in requests {
            let outcome = service.racroute(&context, request).unwrap();
            let mut final_reason = outcome.status.reason;
            assert!(matches!(
                outcome.states.first(),
                Some(RacrouteState::Received)
            ));
            if let Some(RacrouteResult::TokenBuilt(token)) = &outcome.result {
                token_id = Some(token.id.clone());
                assert!(!format!("{token:?}").contains("secret:token-one"));
            }
            if outcome.request_type == RacrouteRequestType::Verifyx {
                assert!(matches!(
                    outcome.result.as_ref(),
                    Some(RacrouteResult::Verified {
                        acee: Some(AceeSummary {
                            delegated_by: Some(value),
                            ..
                        }),
                        ..
                    }) if value == "RACFADM"
                ));
            }
            if outcome.request_type == RacrouteRequestType::Tokenxtr
                && outcome.status.reason == DecisionReason::TokenInvalid
            {
                let retry = service
                    .racroute(
                        &acee_context("TOKENXTR-2", 24),
                        RacrouteRequest::Tokenxtr {
                            token_id: token_id.clone().unwrap(),
                        },
                    )
                    .unwrap();
                assert_eq!(retry.status.reason, DecisionReason::Granted);
                final_reason = retry.status.reason;
            }
            assert_eq!(final_reason, DecisionReason::Granted);
            reached.insert(outcome.request_type);
        }
        assert_eq!(reached.len(), 14);
        assert_eq!(
            service
                .database
                .read()
                .unwrap()
                .audits
                .iter()
                .find(|audit| audit.action == "SAF.TEST")
                .unwrap()
                .fields["PASSWORD"],
            AuditFieldValue::Redacted
        );
    }

    #[test]
    fn mfa_proofs_and_signoff_sessions_are_durable_and_redacted() {
        let (service, resolver, admin) = setup();
        resolver.insert("secret:mfa-enrolled", b"123456".to_vec());
        resolver.insert("secret:mfa-good", b"123456".to_vec());
        resolver.insert("secret:mfa-bad", b"654321".to_vec());
        resolver.insert("secret:user2", b"SECOND-PASSWORD".to_vec());
        for (index, command) in [
            "ADDUSER USER1 PASSWORD('USER-PASSWORD') MFA(FACTOR1 REF secret:mfa-enrolled TYPE TOTP)",
            "ADDUSER USER2 PASSWORD('SECOND-PASSWORD')",
        ]
        .into_iter()
        .enumerate()
        {
            service
                .execute_command(
                    &command_context(&admin, &format!("MFA-SETUP-{index}"), index as u64 + 1),
                    command,
                )
                .unwrap();
        }
        let user1 = PrincipalId::new("USER1", InvocationLimits::default()).unwrap();
        let verifyx = |id: &str, reference: &str, tick: u64| {
            service
                .racroute(
                    &saf_context(&admin, None, id, tick),
                    RacrouteRequest::Verifyx {
                        user: user1.clone(),
                        credential_reference: SecretRef::new("secret:user1", Default::default())
                            .unwrap(),
                        mfa_reference: Some(SecretRef::new(reference, Default::default()).unwrap()),
                        action: SafVerifyAction::CreateAcee,
                        acee_id: None,
                        parent_acee: None,
                    },
                )
                .unwrap()
        };
        let granted = verifyx("MFA-GOOD", "secret:mfa-good", 10);
        assert_eq!(granted.status.reason, DecisionReason::Granted);
        let denied = verifyx("MFA-BAD", "secret:mfa-bad", 11);
        assert_eq!(denied.status.reason, DecisionReason::MfaInvalid);
        assert!(matches!(denied.states.last(), Some(RacrouteState::Denied)));

        let user2 = PrincipalId::new("USER2", InvocationLimits::default()).unwrap();
        let signed_on = service
            .racroute(
                &saf_context(&admin, None, "SESSION-SIGNON", 12),
                RacrouteRequest::Signon {
                    user: user2,
                    credential_reference: SecretRef::new("secret:user2", Default::default())
                        .unwrap(),
                },
            )
            .unwrap();
        let acee_id = extract_acee(&signed_on);
        let listed = service
            .execute_command(
                &command_context(&admin, "SESSION-LIST", 13),
                "SIGNOFF USER(USER2) LIST",
            )
            .unwrap();
        assert!(matches!(
            listed.records.as_slice(),
            [crate::CommandRecord::Session { active: true, .. }]
        ));
        let listed_snapshot = service.database.read().unwrap();
        assert!(
            listed_snapshot
                .signon_sessions
                .values()
                .all(|session| session.state == SignonSessionState::Active)
        );
        assert_eq!(listed_snapshot.acees[&acee_id].state, AceeState::Active);
        let result = service
            .execute_command(
                &command_context(&admin, "SESSION-SIGNOFF", 14),
                "SIGNOFF USER(USER2)",
            )
            .unwrap();
        assert!(matches!(
            result.records.as_slice(),
            [crate::CommandRecord::Session { active: false, .. }]
        ));
        let snapshot = service.database.read().unwrap();
        assert!(
            snapshot
                .signon_sessions
                .values()
                .all(|session| session.state == SignonSessionState::SignedOff)
        );
        assert_eq!(snapshot.acees[&acee_id].state, AceeState::Deleted);
        let audits = format!("{:?}", snapshot.audits);
        for reference in [
            "secret:mfa-enrolled",
            "secret:mfa-good",
            "secret:mfa-bad",
            "secret:user2",
        ] {
            assert!(!audits.contains(reference));
        }
    }

    #[test]
    fn conditional_group_cache_and_deleted_acee_paths_fail_closed() {
        let (service, resolver, admin) = setup();
        for (index, command) in [
            "ADDGROUP PARENT",
            "ADDGROUP CHILD SUPGROUP(PARENT)",
            "ADDUSER USER1 DFLTGRP(CHILD) PASSWORD('USER-PASSWORD')",
            "RDEFINE FACILITY COND.** OWNER(RACFADM) UACC(NONE)",
            "PERMIT 'COND.**' CLASS(FACILITY) ID(PARENT) ACCESS(READ)",
        ]
        .into_iter()
        .enumerate()
        {
            service
                .execute_command(
                    &command_context(&admin, &format!("COND-{index}"), index as u64 + 1),
                    command,
                )
                .unwrap();
        }
        service
            .database
            .mutate(|snapshot| {
                snapshot
                    .profiles
                    .get_mut(&profile_key("FACILITY", "COND.**"))
                    .unwrap()
                    .access_list
                    .iter_mut()
                    .find(|entry| entry.principal == "PARENT")
                    .unwrap()
                    .when = Some(AccessCondition {
                    terminal: Some("TERM1".into()),
                    console: None,
                    system: None,
                    application: None,
                    start_tick: Some(5),
                    end_tick: Some(10),
                });
                Ok(())
            })
            .unwrap();
        let user = PrincipalId::new("USER1", InvocationLimits::default()).unwrap();
        let denied = service
            .racroute(
                &saf_context(&user, None, "COND-DENY", 6),
                RacrouteRequest::Auth {
                    class: "FACILITY".into(),
                    resource: "COND.ONE".into(),
                    access: AccessLevel::Read,
                    environment: AccessEnvironment {
                        terminal: Some("OTHER".into()),
                        tick: 6,
                        ..Default::default()
                    },
                },
            )
            .unwrap();
        assert_eq!(denied.status.reason, DecisionReason::ConditionNotSatisfied);
        assert_eq!(
            (
                denied.status.saf_return_code,
                denied.status.racf_return_code,
                denied.status.racf_reason_code,
            ),
            (8, 8, 4)
        );
        let allowed = service
            .racroute(
                &saf_context(&user, None, "COND-ALLOW", 6),
                RacrouteRequest::Auth {
                    class: "FACILITY".into(),
                    resource: "COND.ONE".into(),
                    access: AccessLevel::Read,
                    environment: AccessEnvironment {
                        terminal: Some("TERM1".into()),
                        tick: 6,
                        ..Default::default()
                    },
                },
            )
            .unwrap();
        assert_eq!(allowed.status.reason, DecisionReason::Granted);
        let verify = service
            .racroute(
                &saf_context(&user, None, "COND-VERIFY", 6),
                RacrouteRequest::Verify {
                    user: user.clone(),
                    credential_reference: SecretRef::new("secret:user1", Default::default())
                        .unwrap(),
                    action: SafVerifyAction::CreateAcee,
                    acee_id: None,
                },
            )
            .unwrap();
        let acee = extract_acee(&verify);
        let no_cache = service
            .racroute(
                &saf_context(&user, Some(&acee), "COND-FAST-DENY", 6),
                RacrouteRequest::Fastauth {
                    class: "FACILITY".into(),
                    resource: "COND.ONE".into(),
                    access: AccessLevel::Read,
                    environment: AccessEnvironment {
                        terminal: Some("TERM1".into()),
                        tick: 6,
                        ..Default::default()
                    },
                },
            )
            .unwrap();
        assert_eq!(no_cache.status.reason, DecisionReason::PolicyUnavailable);
        assert_eq!(
            (
                no_cache.status.saf_return_code,
                no_cache.status.racf_return_code,
                no_cache.status.racf_reason_code,
            ),
            (12, 12, 20)
        );
        service
            .execute_command(
                &command_context(&admin, "COND-CACHE", 7),
                "SETROPTS RACLIST(FACILITY)",
            )
            .unwrap();
        let cached = service
            .racroute(
                &saf_context(&user, Some(&acee), "COND-FAST-ALLOW", 7),
                RacrouteRequest::Fastauth {
                    class: "FACILITY".into(),
                    resource: "COND.ONE".into(),
                    access: AccessLevel::Read,
                    environment: AccessEnvironment {
                        terminal: Some("TERM1".into()),
                        tick: 7,
                        ..Default::default()
                    },
                },
            )
            .unwrap();
        assert_eq!(cached.status.reason, DecisionReason::Granted);
        service
            .racroute(
                &saf_context(&user, None, "COND-DELETE", 8),
                RacrouteRequest::Verify {
                    user: user.clone(),
                    credential_reference: SecretRef::new("secret:user1", Default::default())
                        .unwrap(),
                    action: SafVerifyAction::DeleteAcee,
                    acee_id: Some(acee.clone()),
                },
            )
            .unwrap();
        let deleted = service
            .racroute(
                &saf_context(&user, Some(&acee), "COND-DELETED-AUTH", 9),
                RacrouteRequest::Auth {
                    class: "FACILITY".into(),
                    resource: "COND.ONE".into(),
                    access: AccessLevel::Read,
                    environment: Default::default(),
                },
            )
            .unwrap();
        assert_eq!(deleted.status.reason, DecisionReason::AceeInvalid);
        assert_eq!(deleted.status.racf_reason_code, 12);
        drop(resolver);
    }
}
