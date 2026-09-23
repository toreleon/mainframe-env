mod credential;
pub use credential::{CredentialDetails, CredentialFailure, CredentialKind};
use credential::{build_mfa_proof, verify_credential};

use crate::RacfService;
use crate::command::{RacrouteRequestType, racroute_descriptors};
use crate::model::{
    AccessCondition, AccessLevel, Acee, AceeState, AuditFieldValue, AuditPolicy, DecisionOutcome,
    DecisionReason, PrincipalState, RaclistCache, ResourceProfile, SafDecision, SafStatus,
    SecurityAuditRecord, SecurityDatabaseSnapshot, SecurityRequestDigestFormat, SecurityToken,
    SecurityTransaction, SignonSession, SignonSessionState, TokenKind, TokenState,
    TransactionState, profile_key,
};
use argon2::Argon2;
use argon2::password_hash::{PasswordVerifier, phc::PasswordHash};
use mainframe_env_execution_api::PrincipalId;
use mainframe_env_host_api::{HostProblem, SecretRef};
use serde::{Deserialize, Serialize};
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

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct AccessEnvironment {
    pub terminal: Option<String>,
    pub console: Option<String>,
    pub system: Option<String>,
    pub application: Option<String>,
    pub tick: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub enum SafDefineAction {
    Add,
    Alter,
    Delete,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub enum SafVerifyAction {
    AuthenticateOnly,
    CreateAcee,
    DeleteAcee,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub enum SafExtractKind {
    User,
    Group,
    Profile,
    Acee,
    Token,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RacrouteRequest {
    VerifyCredential {
        user: PrincipalId,
        credential_reference: SecretRef,
        kind: CredentialKind,
        group: Option<String>,
        binding_digest: [u8; 32],
    },
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
            Self::VerifyCredential { .. } => RacrouteRequestType::Verify,
            Self::Verifyx { .. } => RacrouteRequestType::Verifyx,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RacrouteState {
    Received,
    Validated,
    AceeResolved,
    PolicyResolved,
    Committed,
    Denied,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
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

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
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

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case", tag = "record_kind")]
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

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case", tag = "result_kind", content = "result")]
pub enum RacrouteResult {
    CredentialVerified {
        decision: SafDecision,
        failure: Option<CredentialFailure>,
        details: Option<CredentialDetails>,
    },
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

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct RacrouteTerminal {
    status: SafStatus,
    states: Vec<RacrouteState>,
    result: Option<RacrouteResult>,
}

enum SafExecution {
    Terminal {
        terminal: Box<RacrouteTerminal>,
        original_generation: Option<u64>,
    },
    Conflict,
    UnknownOutcome,
}

struct MfaProof {
    factor_id: String,
    factor_reference: String,
    valid: bool,
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
    let request_digest = request_digest(context, &request);
    let initial = service.database.read()?;
    let mut preflight = validate_request_shape(&request).err();
    if preflight.is_none()
        && is_authentication_request(&request)
        && (!initial.subsystem.running || !initial.database_status.active)
    {
        return reject_unavailable_authentication(
            service,
            context,
            descriptor.keyword(),
            &request,
            &request_digest,
        );
    }
    if preflight.is_none() && !initial.principals.contains_key(context.caller().as_str()) {
        preflight = Some(DecisionReason::PrincipalNotFound);
    }
    if descriptor.mutating()
        && service
            .database
            .transaction_for_replay(&initial, context.idempotency_key())?
            .is_some()
    {
        return execute_existing_transaction(
            service,
            context,
            descriptor.keyword(),
            request.request_type(),
            &request_digest,
        );
    }
    let secret = if preflight.is_none() {
        match &request {
            RacrouteRequest::Signon {
                credential_reference,
                ..
            }
            | RacrouteRequest::Verify {
                credential_reference,
                ..
            }
            | RacrouteRequest::VerifyCredential {
                credential_reference,
                ..
            }
            | RacrouteRequest::Verifyx {
                credential_reference,
                ..
            } => match service.secrets.resolve(credential_reference) {
                Ok(secret) => Some(secret),
                Err(_) => {
                    preflight = Some(DecisionReason::CredentialInvalid);
                    None
                }
            },
            _ => None,
        }
    } else {
        None
    };
    let mfa_proof = if preflight.is_none() {
        match &request {
            RacrouteRequest::Verifyx {
                user,
                mfa_reference,
                ..
            } => match build_mfa_proof(service, user.as_str(), mfa_reference.as_ref()) {
                Ok(proof) => proof,
                Err(_) => {
                    preflight = Some(DecisionReason::CredentialInvalid);
                    None
                }
            },
            _ => None,
        }
    } else {
        None
    };
    let ((execution, mut states), generation) = service.database.mutate_retry(|snapshot| {
        let mut states = vec![RacrouteState::Received, RacrouteState::Validated];
        let existing = descriptor
            .mutating()
            .then(|| {
                service
                    .database
                    .transaction_for_replay(snapshot, context.idempotency_key())
            })
            .transpose()?
            .flatten();
        if let Some(existing) = existing.as_ref() {
            if existing.request_digest_format != SecurityRequestDigestFormat::RacrouteCanonicalV1 {
                return Ok(((SafExecution::UnknownOutcome, states), false));
            }
            if existing.request_digest != request_digest {
                let status = status_for_reason(DecisionReason::MalformedRequest);
                let _ = append_audit(
                    snapshot,
                    context,
                    descriptor.keyword(),
                    status,
                    &request_digest,
                )?;
                states.push(RacrouteState::Denied);
                return Ok(((SafExecution::Conflict, states), true));
            }
            let execution = replay_execution(existing)?;
            let replay_states = match &execution {
                SafExecution::Terminal { terminal, .. } => terminal.states.clone(),
                SafExecution::UnknownOutcome => states,
                SafExecution::Conflict => unreachable!(),
            };
            return Ok(((execution, replay_states), false));
        }
        if let Some(reason) = preflight {
            let status = status_for_reason(reason);
            let audit_id = append_audit(
                snapshot,
                context,
                descriptor.keyword(),
                status,
                &request_digest,
            )?;
            let mut result = normalized_preflight_result(&request, reason);
            if let Some(result) = &mut result {
                attach_audit(result, audit_id);
            }
            states.push(RacrouteState::Denied);
            let terminal = RacrouteTerminal {
                status,
                states: states.clone(),
                result,
            };
            if descriptor.mutating() {
                append_racroute_transaction(
                    snapshot,
                    context,
                    descriptor.keyword(),
                    &request_digest,
                    TransactionState::RolledBack,
                    &terminal,
                )?;
            }
            return Ok((
                (
                    SafExecution::Terminal {
                        terminal: Box::new(terminal),
                        original_generation: None,
                    },
                    states,
                ),
                true,
            ));
        }
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
                let terminal = RacrouteTerminal {
                    status,
                    states: states.clone(),
                    result: Some(result),
                };
                if descriptor.mutating() {
                    append_racroute_transaction(
                        &mut staged,
                        context,
                        descriptor.keyword(),
                        &request_digest,
                        if status.reason == DecisionReason::Granted {
                            TransactionState::Committed
                        } else {
                            TransactionState::RolledBack
                        },
                        &terminal,
                    )?;
                }
                *snapshot = staged;
                Ok((
                    (
                        SafExecution::Terminal {
                            terminal: Box::new(terminal),
                            original_generation: None,
                        },
                        states,
                    ),
                    true,
                ))
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
                let terminal = RacrouteTerminal {
                    status,
                    states: states.clone(),
                    result: None,
                };
                if descriptor.mutating() {
                    append_racroute_transaction(
                        snapshot,
                        context,
                        descriptor.keyword(),
                        &request_digest,
                        TransactionState::RolledBack,
                        &terminal,
                    )?;
                }
                Ok((
                    (
                        SafExecution::Terminal {
                            terminal: Box::new(terminal),
                            original_generation: None,
                        },
                        states,
                    ),
                    true,
                ))
            }
        }
    })?;
    states.shrink_to_fit();
    match execution {
        SafExecution::Terminal {
            terminal,
            original_generation,
        } => {
            let terminal = *terminal;
            Ok(RacrouteOutcome {
                request_type: request.request_type(),
                status: terminal.status,
                states: terminal.states,
                result: terminal.result,
                generation: original_generation.unwrap_or(generation),
            })
        }
        SafExecution::Conflict => Err(HostProblem::IdempotencyConflict),
        SafExecution::UnknownOutcome => Err(HostProblem::UnknownOutcome),
    }
}

pub(crate) fn reconcile_legacy_replay(
    service: &RacfService,
    context: &SafRequestContext,
    expected_scrubbed_digest: &str,
    request: &RacrouteRequest,
) -> Result<(), HostProblem> {
    normalized_digest(expected_scrubbed_digest).map_err(|_| HostProblem::Malformed)?;
    let descriptor = racroute_descriptors()
        .iter()
        .copied()
        .find(|descriptor| descriptor.request_type() == request.request_type())
        .filter(|descriptor| descriptor.mutating())
        .ok_or(HostProblem::Unsupported)?;
    validate_request_shape(request).map_err(|_| HostProblem::Malformed)?;
    let canonical = request_digest(context, request);
    service
        .database
        .mutate_if_changed(|snapshot| {
            let transaction = snapshot
                .transactions
                .get_mut(context.idempotency_key())
                .ok_or(HostProblem::NotFound)?;
            if transaction.actor != context.caller().as_str()
                || transaction.operation != format!("RACROUTE-{}", descriptor.keyword())
            {
                return Err(HostProblem::IdempotencyConflict);
            }
            match transaction.request_digest_format {
                SecurityRequestDigestFormat::RacrouteCanonicalV1 => {
                    if transaction.request_digest == canonical {
                        Ok(((), false))
                    } else {
                        Err(HostProblem::IdempotencyConflict)
                    }
                }
                SecurityRequestDigestFormat::LegacyScrubbedV0 => {
                    if transaction.request_digest != expected_scrubbed_digest {
                        return Err(HostProblem::IdempotencyConflict);
                    }
                    transaction.request_digest_format =
                        SecurityRequestDigestFormat::RacrouteCanonicalV1;
                    transaction.request_digest = canonical.clone();
                    Ok(((), true))
                }
                SecurityRequestDigestFormat::LegacyUnversioned => Err(HostProblem::UnknownOutcome),
                SecurityRequestDigestFormat::RacfCommandCanonicalV1 => {
                    Err(HostProblem::IdempotencyConflict)
                }
            }
        })
        .map(|_| ())
}

fn is_authentication_request(request: &RacrouteRequest) -> bool {
    matches!(
        request,
        RacrouteRequest::Signon { .. }
            | RacrouteRequest::Verify { .. }
            | RacrouteRequest::VerifyCredential { .. }
            | RacrouteRequest::Verifyx { .. }
    )
}

fn reject_unavailable_authentication(
    service: &RacfService,
    context: &SafRequestContext,
    keyword: &str,
    request: &RacrouteRequest,
    request_digest: &str,
) -> Result<RacrouteOutcome, HostProblem> {
    let status = status_for_reason(DecisionReason::PolicyUnavailable);
    let (result, generation) = service.database.mutate_retry(|snapshot| {
        let audit_id = append_audit(snapshot, context, keyword, status, request_digest)?;
        let mut result = normalized_preflight_result(request, DecisionReason::PolicyUnavailable);
        if let Some(result) = &mut result {
            attach_audit(result, audit_id);
        }
        Ok((result, true))
    })?;
    Ok(RacrouteOutcome {
        request_type: request.request_type(),
        status,
        states: vec![
            RacrouteState::Received,
            RacrouteState::Validated,
            RacrouteState::Denied,
        ],
        result,
        generation,
    })
}

fn execute_existing_transaction(
    service: &RacfService,
    context: &SafRequestContext,
    keyword: &str,
    request_type: RacrouteRequestType,
    request_digest: &str,
) -> Result<RacrouteOutcome, HostProblem> {
    let (execution, generation) = service.database.mutate_retry(|snapshot| {
        let existing = service
            .database
            .transaction_for_replay(snapshot, context.idempotency_key())?
            .ok_or(HostProblem::UnknownOutcome)?;
        if existing.request_digest_format != SecurityRequestDigestFormat::RacrouteCanonicalV1 {
            return Ok((SafExecution::UnknownOutcome, false));
        }
        if existing.request_digest != request_digest {
            let status = status_for_reason(DecisionReason::MalformedRequest);
            let _ = append_audit(snapshot, context, keyword, status, request_digest)?;
            return Ok((SafExecution::Conflict, true));
        }
        Ok((replay_execution(&existing)?, false))
    })?;
    match execution {
        SafExecution::Terminal {
            terminal,
            original_generation,
        } => {
            let terminal = *terminal;
            Ok(RacrouteOutcome {
                request_type,
                status: terminal.status,
                states: terminal.states,
                result: terminal.result,
                generation: original_generation.unwrap_or(generation),
            })
        }
        SafExecution::Conflict => Err(HostProblem::IdempotencyConflict),
        SafExecution::UnknownOutcome => Err(HostProblem::UnknownOutcome),
    }
}

fn replay_execution(transaction: &SecurityTransaction) -> Result<SafExecution, HostProblem> {
    if matches!(
        transaction.state,
        TransactionState::Intent | TransactionState::UnknownOutcome
    ) {
        return Ok(SafExecution::UnknownOutcome);
    }
    let terminal = transaction
        .terminal_result
        .as_deref()
        .ok_or(HostProblem::UnknownOutcome)
        .and_then(|value| {
            serde_json::from_str::<RacrouteTerminal>(value)
                .map_err(|_| HostProblem::InfrastructureFailure)
        })?;
    if terminal.status != transaction.status {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(SafExecution::Terminal {
        terminal: Box::new(terminal),
        original_generation: transaction.final_generation,
    })
}

fn append_racroute_transaction(
    snapshot: &mut SecurityDatabaseSnapshot,
    context: &SafRequestContext,
    keyword: &str,
    request_digest: &str,
    state: TransactionState,
    terminal: &RacrouteTerminal,
) -> Result<(), HostProblem> {
    if snapshot.transactions.len() >= 65_536
        && !snapshot
            .transactions
            .contains_key(context.idempotency_key())
    {
        return Err(HostProblem::ResourceExhausted);
    }
    let terminal_result =
        serde_json::to_string(terminal).map_err(|_| HostProblem::InfrastructureFailure)?;
    if terminal_result.len() > 65_536 {
        return Err(HostProblem::ResourceExhausted);
    }
    let terminal_tick = snapshot
        .observe_retention_tick(context.tick())
        .ok_or(HostProblem::ResourceExhausted)?;
    snapshot.transactions.insert(
        context.idempotency_key().into(),
        SecurityTransaction {
            id: context.idempotency_key().into(),
            idempotency_key: context.idempotency_key().into(),
            actor: context.caller().as_str().into(),
            operation: format!("RACROUTE-{keyword}"),
            request_digest_format: SecurityRequestDigestFormat::RacrouteCanonicalV1,
            request_digest: request_digest.into(),
            state,
            base_generation: snapshot.generation,
            final_generation: Some(
                snapshot
                    .generation
                    .checked_add(1)
                    .ok_or(HostProblem::ResourceExhausted)?,
            ),
            status: terminal.status,
            terminal_result: Some(terminal_result),
            terminal_tick: Some(terminal_tick),
        },
    );
    Ok(())
}

fn validate_request_shape(request: &RacrouteRequest) -> Result<(), DecisionReason> {
    match request {
        RacrouteRequest::Audit {
            action,
            resource_digest,
            fields,
            ..
        } => {
            normalized_text(action, 246)?;
            normalized_digest(resource_digest)?;
            if fields.len() > 256 {
                return Err(DecisionReason::ResourceExhausted);
            }
        }
        RacrouteRequest::Auth {
            class,
            resource,
            environment,
            ..
        }
        | RacrouteRequest::Fastauth {
            class,
            resource,
            environment,
            ..
        } => {
            normalized_class(class)?;
            normalized_profile(resource)?;
            validate_environment(environment)?;
        }
        RacrouteRequest::Define {
            class,
            resource,
            owner,
            ..
        } => {
            normalized_class(class)?;
            normalized_profile(resource)?;
            normalized_principal(owner)?;
        }
        RacrouteRequest::Dirauth { node } => {
            normalized_id(node.clone(), 246).map_err(DecisionReason::from)?;
        }
        RacrouteRequest::Extract { kind, class, name } => match kind {
            SafExtractKind::Profile => {
                normalized_class(class.as_deref().ok_or(DecisionReason::MalformedRequest)?)?;
                normalized_profile(name)?;
            }
            SafExtractKind::User | SafExtractKind::Group => {
                normalized_principal(name)?;
            }
            SafExtractKind::Acee | SafExtractKind::Token => {
                normalized_id(name.clone(), 246).map_err(DecisionReason::from)?;
            }
        },
        RacrouteRequest::List { class, .. } => {
            normalized_class(class)?;
        }
        RacrouteRequest::Signon { .. } => {}
        RacrouteRequest::Stat { class } => {
            if let Some(class) = class {
                normalized_class(class)?;
            }
        }
        RacrouteRequest::Tokenbld {
            token_reference,
            token_digest,
            scopes,
            ..
        } => {
            normalized_reference(token_reference)?;
            normalized_digest(token_digest)?;
            if scopes.len() > 256 {
                return Err(DecisionReason::ResourceExhausted);
            }
            for scope in scopes {
                normalized_text(scope, 246)?;
            }
        }
        RacrouteRequest::Tokenmap { token_digest } => {
            normalized_digest(token_digest)?;
        }
        RacrouteRequest::Tokenxtr { token_id } => {
            normalized_id(token_id.clone(), 246).map_err(DecisionReason::from)?;
        }
        RacrouteRequest::Verify { acee_id, .. } => {
            if let Some(acee_id) = acee_id {
                normalized_id(acee_id.clone(), 246).map_err(DecisionReason::from)?;
            }
        }
        RacrouteRequest::VerifyCredential { group, .. } => {
            if let Some(group) = group {
                normalized_principal(group)?;
            }
        }
        RacrouteRequest::Verifyx {
            acee_id,
            parent_acee,
            ..
        } => {
            for id in [acee_id, parent_acee].into_iter().flatten() {
                normalized_id(id.clone(), 246).map_err(DecisionReason::from)?;
            }
        }
    }
    Ok(())
}

fn normalized_preflight_result(
    request: &RacrouteRequest,
    reason: DecisionReason,
) -> Option<RacrouteResult> {
    if !matches!(
        reason,
        DecisionReason::CredentialInvalid | DecisionReason::PolicyUnavailable
    ) || !matches!(
        request,
        RacrouteRequest::Signon { .. }
            | RacrouteRequest::Verify { .. }
            | RacrouteRequest::VerifyCredential { .. }
            | RacrouteRequest::Verifyx { .. }
    ) {
        return None;
    }
    if matches!(request, RacrouteRequest::VerifyCredential { .. }) {
        Some(RacrouteResult::CredentialVerified {
            decision: decision(reason, AccessLevel::None, None, None),
            failure: Some(CredentialFailure::PolicyUnavailable),
            details: None,
        })
    } else {
        Some(RacrouteResult::Verified {
            decision: decision(reason, AccessLevel::None, None, None),
            acee: None,
        })
    }
}

fn validate_environment(environment: &AccessEnvironment) -> Result<(), DecisionReason> {
    for value in [
        &environment.terminal,
        &environment.console,
        &environment.system,
        &environment.application,
    ]
    .into_iter()
    .flatten()
    {
        normalized_text(value, 246)?;
    }
    Ok(())
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
            let fields = crate::audit::redact_fields(fields.clone());
            let tick = snapshot
                .observe_retention_tick(context.tick())
                .ok_or(DecisionReason::ResourceExhausted)?;
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
                tick,
                retention_observed_tick: Some(tick),
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
        RacrouteRequest::VerifyCredential {
            user, kind, group, ..
        } => credential::verify_cics_request(
            snapshot,
            context,
            user,
            secret,
            *kind,
            group.as_deref(),
            states,
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
        SafExtractKind::Acee => {
            let acee = snapshot
                .acees
                .get(&normalized_id(name.to_string(), 246)?)
                .ok_or(DecisionReason::AceeInvalid)?;
            if actor != acee.principal && !is_auditor_or_special(snapshot, actor) {
                return Err(DecisionReason::InsufficientAccess);
            }
            Ok(ExtractedSecurityRecord::Acee(acee_summary(acee)))
        }
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
    let tick = snapshot
        .observe_retention_tick(context.tick())
        .ok_or(HostProblem::ResourceExhausted)?;
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
        fields: crate::audit::redact_fields(BTreeMap::from([
            (
                "ACEE".into(),
                context.acee_id().map_or(AuditFieldValue::Redacted, |id| {
                    AuditFieldValue::Reference(format!("acee:{id}"))
                }),
            ),
            (
                "REQUEST_DIGEST_FORMAT".into(),
                AuditFieldValue::Text(
                    SecurityRequestDigestFormat::RacrouteCanonicalV1
                        .as_str()
                        .into(),
                ),
            ),
        ])),
        tick,
        retention_observed_tick: Some(tick),
    });
    Ok(id)
}

fn attach_audit(result: &mut RacrouteResult, audit_id: String) {
    match result {
        RacrouteResult::Decision(decision)
        | RacrouteResult::Verified { decision, .. }
        | RacrouteResult::CredentialVerified { decision, .. } => decision.audit_id = Some(audit_id),
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

fn request_digest(context: &SafRequestContext, request: &RacrouteRequest) -> String {
    let mut digest = Sha256::new();
    digest.update(b"mainframe-env.racroute-request@1\0");
    digest_saf_field(&mut digest, context.caller().as_str().as_bytes());
    digest_saf_optional(&mut digest, context.acee_id());
    digest_saf_optional(&mut digest, context.delegated_by().map(PrincipalId::as_str));
    digest_saf_tag(&mut digest, racroute_request_tag(request.request_type()));
    match request {
        RacrouteRequest::Audit {
            action,
            resource_digest,
            decision,
            fields,
        } => {
            digest_saf_field(&mut digest, action.as_bytes());
            digest_saf_field(&mut digest, resource_digest.as_bytes());
            digest_saf_tag(&mut digest, decision_tag(*decision));
            let fields = crate::audit::redact_fields(fields.clone());
            digest_saf_len(&mut digest, fields.len());
            for (name, value) in &fields {
                digest_saf_field(&mut digest, name.as_bytes());
                digest_audit_field(&mut digest, value);
            }
        }
        RacrouteRequest::Auth {
            class,
            resource,
            access,
            environment,
        }
        | RacrouteRequest::Fastauth {
            class,
            resource,
            access,
            environment,
        } => {
            digest_saf_field(&mut digest, class.as_bytes());
            digest_saf_field(&mut digest, resource.as_bytes());
            digest_saf_tag(&mut digest, access_tag(*access));
            digest_access_environment(&mut digest, environment);
        }
        RacrouteRequest::Define {
            action,
            class,
            resource,
            owner,
            uacc,
            generic,
        } => {
            digest_saf_tag(&mut digest, define_action_tag(*action));
            digest_saf_field(&mut digest, class.as_bytes());
            digest_saf_field(&mut digest, resource.as_bytes());
            digest_saf_field(&mut digest, owner.as_bytes());
            digest_saf_tag(&mut digest, access_tag(*uacc));
            digest.update([u8::from(*generic)]);
        }
        RacrouteRequest::Dirauth { node } => digest_saf_field(&mut digest, node.as_bytes()),
        RacrouteRequest::Extract { kind, class, name } => {
            digest_saf_tag(&mut digest, extract_kind_tag(*kind));
            digest_saf_optional(&mut digest, class.as_deref());
            digest_saf_field(&mut digest, name.as_bytes());
        }
        RacrouteRequest::List {
            class,
            global,
            refresh,
        } => {
            digest_saf_field(&mut digest, class.as_bytes());
            digest.update([u8::from(*global), u8::from(*refresh)]);
        }
        RacrouteRequest::Signon {
            user,
            credential_reference,
        } => {
            digest_saf_field(&mut digest, user.as_str().as_bytes());
            digest_saf_field(&mut digest, credential_reference.as_str().as_bytes());
        }
        RacrouteRequest::Stat { class } => digest_saf_optional(&mut digest, class.as_deref()),
        RacrouteRequest::Tokenbld {
            owner,
            kind,
            token_reference,
            token_digest,
            scopes,
            expires_tick,
        } => {
            digest_saf_field(&mut digest, owner.as_str().as_bytes());
            digest_saf_tag(&mut digest, token_kind_tag(*kind));
            digest_saf_field(&mut digest, token_reference.as_bytes());
            digest_saf_field(&mut digest, token_digest.as_bytes());
            digest_saf_len(&mut digest, scopes.len());
            for scope in scopes {
                digest_saf_field(&mut digest, scope.as_bytes());
            }
            digest_saf_u64(&mut digest, *expires_tick);
        }
        RacrouteRequest::Tokenmap { token_digest } => {
            digest_saf_field(&mut digest, token_digest.as_bytes());
        }
        RacrouteRequest::Tokenxtr { token_id } => {
            digest_saf_field(&mut digest, token_id.as_bytes());
        }
        RacrouteRequest::Verify {
            user,
            credential_reference,
            action,
            acee_id,
        } => {
            digest_saf_field(&mut digest, user.as_str().as_bytes());
            digest_saf_field(&mut digest, credential_reference.as_str().as_bytes());
            digest_saf_tag(&mut digest, verify_action_tag(*action));
            digest_saf_optional(&mut digest, acee_id.as_deref());
        }
        RacrouteRequest::VerifyCredential {
            user,
            credential_reference,
            kind,
            group,
            binding_digest,
        } => {
            digest_saf_tag(&mut digest, 0xc1);
            digest_saf_field(&mut digest, user.as_str().as_bytes());
            digest_saf_field(&mut digest, credential_reference.as_str().as_bytes());
            digest_saf_tag(
                &mut digest,
                match kind {
                    CredentialKind::Password => 1,
                    CredentialKind::Phrase => 2,
                },
            );
            digest_saf_optional(&mut digest, group.as_deref());
            digest_saf_field(&mut digest, binding_digest);
        }
        RacrouteRequest::Verifyx {
            user,
            credential_reference,
            mfa_reference,
            action,
            acee_id,
            parent_acee,
        } => {
            digest_saf_field(&mut digest, user.as_str().as_bytes());
            digest_saf_field(&mut digest, credential_reference.as_str().as_bytes());
            digest_saf_optional(&mut digest, mfa_reference.as_ref().map(SecretRef::as_str));
            digest_saf_tag(&mut digest, verify_action_tag(*action));
            digest_saf_optional(&mut digest, acee_id.as_deref());
            digest_saf_optional(&mut digest, parent_acee.as_deref());
        }
    }
    format!("sha256:{:x}", digest.finalize())
}

const fn racroute_request_tag(value: RacrouteRequestType) -> u8 {
    match value {
        RacrouteRequestType::Audit => 1,
        RacrouteRequestType::Auth => 2,
        RacrouteRequestType::Define => 3,
        RacrouteRequestType::Dirauth => 4,
        RacrouteRequestType::Extract => 5,
        RacrouteRequestType::Fastauth => 6,
        RacrouteRequestType::List => 7,
        RacrouteRequestType::Signon => 8,
        RacrouteRequestType::Stat => 9,
        RacrouteRequestType::Tokenbld => 10,
        RacrouteRequestType::Tokenmap => 11,
        RacrouteRequestType::Tokenxtr => 12,
        RacrouteRequestType::Verify => 13,
        RacrouteRequestType::Verifyx => 14,
    }
}

const fn decision_tag(value: DecisionOutcome) -> u8 {
    match value {
        DecisionOutcome::Allow => 1,
        DecisionOutcome::Deny => 2,
        DecisionOutcome::NoDecision => 3,
    }
}

const fn access_tag(value: AccessLevel) -> u8 {
    match value {
        AccessLevel::None => 1,
        AccessLevel::Execute => 2,
        AccessLevel::Read => 3,
        AccessLevel::Update => 4,
        AccessLevel::Control => 5,
        AccessLevel::Alter => 6,
    }
}

const fn define_action_tag(value: SafDefineAction) -> u8 {
    match value {
        SafDefineAction::Add => 1,
        SafDefineAction::Alter => 2,
        SafDefineAction::Delete => 3,
    }
}

const fn extract_kind_tag(value: SafExtractKind) -> u8 {
    match value {
        SafExtractKind::User => 1,
        SafExtractKind::Group => 2,
        SafExtractKind::Profile => 3,
        SafExtractKind::Acee => 4,
        SafExtractKind::Token => 5,
    }
}

const fn token_kind_tag(value: TokenKind) -> u8 {
    match value {
        TokenKind::SafIdentity => 1,
        TokenKind::PassTicket => 2,
        TokenKind::JwtReference => 3,
        TokenKind::Custom => 4,
    }
}

const fn verify_action_tag(value: SafVerifyAction) -> u8 {
    match value {
        SafVerifyAction::AuthenticateOnly => 1,
        SafVerifyAction::CreateAcee => 2,
        SafVerifyAction::DeleteAcee => 3,
    }
}

fn digest_audit_field(digest: &mut Sha256, value: &AuditFieldValue) {
    match value {
        AuditFieldValue::Text(value) => {
            digest_saf_tag(digest, 1);
            digest_saf_field(digest, value.as_bytes());
        }
        AuditFieldValue::Redacted => digest_saf_tag(digest, 2),
        AuditFieldValue::Digest(value) => {
            digest_saf_tag(digest, 3);
            digest_saf_field(digest, value.as_bytes());
        }
        AuditFieldValue::Reference(value) => {
            digest_saf_tag(digest, 4);
            digest_saf_field(digest, value.as_bytes());
        }
    }
}

fn digest_access_environment(digest: &mut Sha256, environment: &AccessEnvironment) {
    digest_saf_optional(digest, environment.terminal.as_deref());
    digest_saf_optional(digest, environment.console.as_deref());
    digest_saf_optional(digest, environment.system.as_deref());
    digest_saf_optional(digest, environment.application.as_deref());
    digest.update(environment.tick.to_be_bytes());
}

fn digest_saf_tag(digest: &mut Sha256, value: u8) {
    digest.update([value]);
}

fn digest_saf_len(digest: &mut Sha256, value: usize) {
    digest.update(u64::try_from(value).unwrap_or(u64::MAX).to_be_bytes());
}

fn digest_saf_optional(digest: &mut Sha256, value: Option<&str>) {
    match value {
        Some(value) => {
            digest.update([1]);
            digest_saf_field(digest, value.as_bytes());
        }
        None => digest.update([0]),
    }
}

fn digest_saf_u64(digest: &mut Sha256, value: Option<u64>) {
    match value {
        Some(value) => {
            digest.update([1]);
            digest.update(value.to_be_bytes());
        }
        None => digest.update([0]),
    }
}

fn digest_saf_field(digest: &mut Sha256, value: &[u8]) {
    digest.update(u64::try_from(value.len()).unwrap_or(u64::MAX).to_be_bytes());
    digest.update(value);
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
    use mainframe_env_store::{MemoryStore, SqliteStateStore};
    use mainframe_env_store_api::ProviderStateStore;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct CountingResolver {
        inner: MemorySecretResolver,
        calls: AtomicUsize,
    }

    impl CountingResolver {
        fn new() -> Self {
            Self {
                inner: MemorySecretResolver::default(),
                calls: AtomicUsize::new(0),
            }
        }

        fn insert(&self, reference: &str, value: &[u8]) {
            self.inner.insert(reference, value.to_vec());
        }

        fn calls(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    impl crate::SecretResolver for CountingResolver {
        fn resolve(&self, reference: &SecretRef) -> Result<crate::ResolvedSecret, HostProblem> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.inner.resolve(reference)
        }
    }

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

    fn assert_policy_unavailable(outcome: &RacrouteOutcome) {
        assert_eq!(outcome.status.reason, DecisionReason::PolicyUnavailable);
        assert_eq!(
            outcome.states,
            [
                RacrouteState::Received,
                RacrouteState::Validated,
                RacrouteState::Denied,
            ]
        );
        assert!(matches!(
            outcome.result.as_ref(),
            Some(RacrouteResult::Verified {
                decision: SafDecision {
                    outcome: DecisionOutcome::Deny,
                    status: SafStatus {
                        reason: DecisionReason::PolicyUnavailable,
                        ..
                    },
                    ..
                },
                acee: None,
            })
        ));
    }

    #[test]
    fn cics_credential_verify_replays_once_and_preserves_distinct_phrase_slot() {
        let (service, resolver, admin) = setup();
        resolver.insert("secret:cics-password", b"PASSWORD".to_vec());
        resolver.insert("secret:cics-wrong", b"WRONG123".to_vec());
        service
            .add_user(
                "IBMUSER",
                &SecretRef::new("secret:cics-password", Default::default()).unwrap(),
            )
            .unwrap();
        let user = PrincipalId::new("IBMUSER", InvocationLimits::default()).unwrap();
        let valid = RacrouteRequest::VerifyCredential {
            user: user.clone(),
            credential_reference: SecretRef::new("secret:cics-password", Default::default())
                .unwrap(),
            kind: CredentialKind::Password,
            group: None,
            binding_digest: [1; 32],
        };
        let result = service
            .racroute(
                &saf_context(&admin, None, "CICS-VERIFY-1", 5),
                valid.clone(),
            )
            .unwrap();
        assert_eq!(result.status.reason, DecisionReason::Granted);
        assert!(matches!(
            result.result,
            Some(RacrouteResult::CredentialVerified {
                failure: None,
                details: Some(CredentialDetails {
                    invalid_count: 255,
                    last_use_tick: 0,
                    ..
                }),
                ..
            })
        ));
        let wrong = RacrouteRequest::VerifyCredential {
            user: user.clone(),
            credential_reference: SecretRef::new("secret:cics-wrong", Default::default()).unwrap(),
            kind: CredentialKind::Password,
            group: None,
            binding_digest: [2; 32],
        };
        let context = saf_context(&admin, None, "CICS-VERIFY-2", 6);
        let denied = service.racroute(&context, wrong.clone()).unwrap();
        assert_eq!(denied.status.reason, DecisionReason::CredentialInvalid);
        assert!(matches!(
            denied.result.as_ref(),
            Some(RacrouteResult::CredentialVerified {
                failure: Some(CredentialFailure::InvalidCredential),
                details: None,
                ..
            })
        ));
        let before = service.database.read().unwrap();
        assert_eq!(before.principals["IBMUSER"].invalid_count, Some(1));
        assert_eq!(service.racroute(&context, wrong.clone()).unwrap(), denied);
        assert_eq!(
            service.database.read().unwrap().audits.len(),
            before.audits.len()
        );
        let changed = RacrouteRequest::VerifyCredential {
            user: user.clone(),
            credential_reference: SecretRef::new("secret:cics-wrong", Default::default()).unwrap(),
            kind: CredentialKind::Password,
            group: None,
            binding_digest: [3; 32],
        };
        assert_eq!(
            service.racroute(&context, changed),
            Err(HostProblem::IdempotencyConflict)
        );

        let phrase = b"LONG-PHRASE-1234";
        service
            .database
            .mutate(|snapshot| {
                let verifier = service
                    .credential_from_bytes(&snapshot.policy, "IBMUSER", None, phrase, true, 7)
                    .map_err(|_| HostProblem::Malformed)?;
                snapshot
                    .principals
                    .get_mut("IBMUSER")
                    .unwrap()
                    .phrase_credential = Some(verifier);
                Ok(())
            })
            .unwrap();
        resolver.insert("secret:cics-phrase", phrase.to_vec());
        let phrase_result = service
            .racroute(
                &saf_context(&admin, None, "CICS-VERIFY-3", 8),
                RacrouteRequest::VerifyCredential {
                    user,
                    credential_reference: SecretRef::new("secret:cics-phrase", Default::default())
                        .unwrap(),
                    kind: CredentialKind::Phrase,
                    group: None,
                    binding_digest: [4; 32],
                },
            )
            .unwrap();
        assert_eq!(phrase_result.status.reason, DecisionReason::Granted);
        assert!(
            service.database.read().unwrap().principals["IBMUSER"]
                .credential
                .is_some()
        );
    }

    #[test]
    fn sqlite_restart_preserves_cics_verify_failure_and_replay_binding() {
        let directory = std::env::temp_dir().join(format!(
            "mainframe-env-cics-verify-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("racf.db");
        let url = format!("sqlite://{}?mode=rwc", path.display());
        let resolver = Arc::new(MemorySecretResolver::default());
        resolver.insert("secret:admin", b"ADMIN-PASSWORD".to_vec());
        resolver.insert("secret:user", b"PASSWORD".to_vec());
        resolver.insert("secret:wrong", b"WRONG123".to_vec());
        let admin = PrincipalId::new("RACFADM", InvocationLimits::default()).unwrap();
        let user = PrincipalId::new("IBMUSER", InvocationLimits::default()).unwrap();
        let context = saf_context(&admin, None, "CICS-VERIFY-SQLITE", 9);
        let request = RacrouteRequest::VerifyCredential {
            user,
            credential_reference: SecretRef::new("secret:wrong", Default::default()).unwrap(),
            kind: CredentialKind::Password,
            group: None,
            binding_digest: [9; 32],
        };
        let first = {
            let store: Arc<dyn ProviderStateStore> =
                Arc::new(SqliteStateStore::open(&url, 32 * 1024 * 1024, 65_536).unwrap());
            let service = RacfService::open(store, resolver.clone(), Default::default()).unwrap();
            service
                .bootstrap_administrator(
                    "RACFADM",
                    &SecretRef::new("secret:admin", Default::default()).unwrap(),
                )
                .unwrap();
            service
                .add_user(
                    "IBMUSER",
                    &SecretRef::new("secret:user", Default::default()).unwrap(),
                )
                .unwrap();
            service.racroute(&context, request.clone()).unwrap()
        };
        {
            let store: Arc<dyn ProviderStateStore> =
                Arc::new(SqliteStateStore::open(&url, 32 * 1024 * 1024, 65_536).unwrap());
            let service = RacfService::open(store, resolver, Default::default()).unwrap();
            assert_eq!(
                service.database.read().unwrap().principals["IBMUSER"].invalid_count,
                Some(1)
            );
            assert_eq!(service.racroute(&context, request).unwrap(), first);
            assert_eq!(
                service.database.read().unwrap().principals["IBMUSER"].invalid_count,
                Some(1)
            );
        }
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_dir(directory);
    }

    #[test]
    fn racroute_canonical_digest_has_a_stable_golden_identity() {
        let admin = PrincipalId::new("RACFADM", InvocationLimits::default()).unwrap();
        let context = saf_context(&admin, Some("ACEE0001"), "DIGEST-GOLDEN", 17);
        let request = RacrouteRequest::Auth {
            class: "FACILITY".into(),
            resource: "APP.ONE".into(),
            access: AccessLevel::Read,
            environment: AccessEnvironment {
                terminal: Some("LUTERM1".into()),
                console: None,
                system: Some("SYS1".into()),
                application: Some("APPL1".into()),
                tick: 19,
            },
        };
        assert_eq!(
            request_digest(&context, &request),
            "sha256:f12a7fce354c0f3c1a42b54376597d9230b956d729186932208c9f519824f503"
        );
        let changed = RacrouteRequest::Auth {
            class: "FACILITY".into(),
            resource: "APP.ONE".into(),
            access: AccessLevel::Update,
            environment: AccessEnvironment {
                terminal: Some("LUTERM1".into()),
                console: None,
                system: Some("SYS1".into()),
                application: Some("APPL1".into()),
                tick: 19,
            },
        };
        assert_ne!(
            request_digest(&context, &changed),
            "sha256:f12a7fce354c0f3c1a42b54376597d9230b956d729186932208c9f519824f503"
        );
        let audit = |secret: &str| RacrouteRequest::Audit {
            action: "SIGNON".into(),
            resource_digest: format!("sha256:{}", "1".repeat(64)),
            decision: DecisionOutcome::Deny,
            fields: BTreeMap::from([("PASSWORD".into(), AuditFieldValue::Text(secret.into()))]),
        };
        assert_eq!(
            request_digest(&context, &audit("FIRST-SECRET")),
            request_digest(&context, &audit("SECOND-SECRET"))
        );
    }

    #[test]
    fn legacy_racroute_replay_requires_attested_migration_without_redispatch() {
        let store = Arc::new(MemoryStore::new(Default::default()));
        let resolver = Arc::new(MemorySecretResolver::default());
        resolver.insert("secret:admin", b"ADMIN-PASSWORD".to_vec());
        let service =
            RacfService::open(store.clone(), resolver.clone(), Default::default()).unwrap();
        service
            .bootstrap_administrator(
                "RACFADM",
                &SecretRef::new("secret:admin", Default::default()).unwrap(),
            )
            .unwrap();
        let admin = PrincipalId::new("RACFADM", InvocationLimits::default()).unwrap();
        let context = saf_context(&admin, None, "LEGACY-RACROUTE", 21);
        let request = RacrouteRequest::Audit {
            action: "LEGACY.TEST".into(),
            resource_digest: format!("sha256:{}", "1".repeat(64)),
            decision: DecisionOutcome::Allow,
            fields: BTreeMap::new(),
        };
        let original = service.racroute(&context, request.clone()).unwrap();
        let audit_count = service.audits().len();
        let legacy_digest = format!("sha256:{}", "a".repeat(64));
        service
            .database
            .mutate(|snapshot| {
                let transaction = snapshot
                    .transactions
                    .get(context.idempotency_key())
                    .cloned()
                    .ok_or(HostProblem::NotFound)?;
                let mut encoded =
                    serde_json::to_value(transaction).map_err(|_| HostProblem::Malformed)?;
                let encoded = encoded.as_object_mut().ok_or(HostProblem::Malformed)?;
                encoded.remove("request_digest_format");
                encoded.insert(
                    "request_digest".into(),
                    serde_json::Value::String(legacy_digest.clone()),
                );
                let transaction: SecurityTransaction =
                    serde_json::from_value(encoded.clone().into())
                        .map_err(|_| HostProblem::Malformed)?;
                assert_eq!(
                    transaction.request_digest_format,
                    SecurityRequestDigestFormat::LegacyUnversioned
                );
                snapshot
                    .transactions
                    .insert(context.idempotency_key().into(), transaction);
                Ok(())
            })
            .unwrap();
        drop(service);

        let reopened =
            RacfService::open(store.clone(), resolver.clone(), Default::default()).unwrap();
        let scrubbed_digest = {
            let snapshot = reopened.database.read().unwrap();
            let transaction = &snapshot.transactions[context.idempotency_key()];
            assert_eq!(
                transaction.request_digest_format,
                SecurityRequestDigestFormat::LegacyScrubbedV0
            );
            assert_ne!(transaction.request_digest, legacy_digest);
            transaction.request_digest.clone()
        };
        assert_eq!(
            reopened.racroute(&context, request.clone()),
            Err(HostProblem::UnknownOutcome)
        );
        assert_eq!(reopened.audits().len(), audit_count);
        assert_eq!(
            reopened.reconcile_legacy_racroute(
                &context,
                &format!("sha256:{}", "b".repeat(64)),
                &request,
            ),
            Err(HostProblem::IdempotencyConflict)
        );
        reopened
            .reconcile_legacy_racroute(&context, &scrubbed_digest, &request)
            .unwrap();
        assert_eq!(
            reopened.racroute(&context, request.clone()),
            Ok(original.clone())
        );
        assert_eq!(reopened.audits().len(), audit_count);
        drop(reopened);

        let restarted = RacfService::open(store, resolver, Default::default()).unwrap();
        assert_eq!(restarted.racroute(&context, request), Ok(original));
        assert_eq!(restarted.audits().len(), audit_count);
    }

    #[test]
    fn stopped_or_inactive_racf_rejects_authentication_until_reactivated_across_restart() {
        use mainframe_env_host_api::SecurityDecision;

        let store = Arc::new(MemoryStore::new(Default::default()));
        let resolver = Arc::new(CountingResolver::new());
        resolver.insert("secret:admin", b"ADMIN-PASSWORD");
        let credential = SecretRef::new("secret:admin", Default::default()).unwrap();
        let admin = PrincipalId::new("RACFADM", InvocationLimits::default()).unwrap();
        let service =
            RacfService::open(store.clone(), resolver.clone(), Default::default()).unwrap();
        service
            .bootstrap_administrator("RACFADM", &credential)
            .unwrap();

        service
            .execute_command(&command_context(&admin, "STOP-AUTH", 1), "STOP")
            .unwrap();
        let calls_before_stop_checks = resolver.calls();
        assert_eq!(
            service.authenticate(&admin, &credential).unwrap(),
            SecurityDecision::Deny
        );
        let stopped_signon = service
            .racroute(
                &saf_context(&admin, None, "STOPPED-SIGNON", 2),
                RacrouteRequest::Signon {
                    user: admin.clone(),
                    credential_reference: credential.clone(),
                },
            )
            .unwrap();
        assert_policy_unavailable(&stopped_signon);
        let stopped_verify = service
            .racroute(
                &saf_context(&admin, None, "STOPPED-VERIFY", 3),
                RacrouteRequest::Verify {
                    user: admin.clone(),
                    credential_reference: credential.clone(),
                    action: SafVerifyAction::AuthenticateOnly,
                    acee_id: None,
                },
            )
            .unwrap();
        assert_policy_unavailable(&stopped_verify);
        assert_eq!(resolver.calls(), calls_before_stop_checks);
        {
            let snapshot = service.database.read().unwrap();
            assert!(!snapshot.subsystem.running);
            assert!(snapshot.acees.is_empty() && snapshot.signon_sessions.is_empty());
            assert!(!snapshot.transactions.contains_key("STOPPED-SIGNON"));
            assert!(!snapshot.transactions.contains_key("STOPPED-VERIFY"));
            assert_eq!(
                snapshot.audits.last().unwrap().status.reason,
                DecisionReason::PolicyUnavailable
            );
        }

        drop(service);
        let service =
            RacfService::open(store.clone(), resolver.clone(), Default::default()).unwrap();
        let calls_before_restart_check = resolver.calls();
        assert_eq!(
            service.authenticate(&admin, &credential).unwrap(),
            SecurityDecision::Deny
        );
        assert_eq!(resolver.calls(), calls_before_restart_check);
        service
            .execute_command(&command_context(&admin, "RESTART-AUTH", 4), "RESTART")
            .unwrap();
        assert_eq!(
            service.authenticate(&admin, &credential).unwrap(),
            SecurityDecision::Allow
        );
        let active_signon = service
            .racroute(
                &saf_context(&admin, None, "ACTIVE-SIGNON", 5),
                RacrouteRequest::Signon {
                    user: admin.clone(),
                    credential_reference: credential.clone(),
                },
            )
            .unwrap();
        assert_eq!(active_signon.status.reason, DecisionReason::Granted);

        service
            .execute_command(
                &command_context(&admin, "INACTIVE-AUTH", 6),
                "RVARY INACTIVE",
            )
            .unwrap();
        let calls_before_inactive_checks = resolver.calls();
        assert_eq!(
            service.authenticate(&admin, &credential).unwrap(),
            SecurityDecision::Deny
        );
        let inactive_signon = service
            .racroute(
                &saf_context(&admin, None, "INACTIVE-SIGNON", 7),
                RacrouteRequest::Signon {
                    user: admin.clone(),
                    credential_reference: credential.clone(),
                },
            )
            .unwrap();
        assert_policy_unavailable(&inactive_signon);
        let inactive_verify = service
            .racroute(
                &saf_context(&admin, None, "INACTIVE-VERIFY", 8),
                RacrouteRequest::Verify {
                    user: admin.clone(),
                    credential_reference: credential.clone(),
                    action: SafVerifyAction::AuthenticateOnly,
                    acee_id: None,
                },
            )
            .unwrap();
        assert_policy_unavailable(&inactive_verify);
        assert_eq!(resolver.calls(), calls_before_inactive_checks);

        drop(service);
        let service = RacfService::open(store, resolver.clone(), Default::default()).unwrap();
        let calls_before_active = resolver.calls();
        assert_eq!(
            service.authenticate(&admin, &credential).unwrap(),
            SecurityDecision::Deny
        );
        assert_eq!(resolver.calls(), calls_before_active);
        service
            .execute_command(&command_context(&admin, "ACTIVE-AUTH", 9), "RVARY ACTIVE")
            .unwrap();
        let active_verify = service
            .racroute(
                &saf_context(&admin, None, "REACTIVATED-VERIFY", 10),
                RacrouteRequest::Verify {
                    user: admin,
                    credential_reference: credential,
                    action: SafVerifyAction::AuthenticateOnly,
                    acee_id: None,
                },
            )
            .unwrap();
        assert_eq!(active_verify.status.reason, DecisionReason::Granted);
        assert!(resolver.calls() > calls_before_active);
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
    fn every_mutating_racroute_replays_exact_terminal_result_without_duplicate_effects() {
        let (service, _, admin) = setup();
        service
            .execute_command(
                &command_context(&admin, "IDEM-USER", 1),
                "ADDUSER USER1 PASSWORD('USER-PASSWORD')",
            )
            .unwrap();
        service
            .execute_command(
                &command_context(&admin, "IDEM-PROFILE", 2),
                "RDEFINE FACILITY IDEM.** OWNER(RACFADM) UACC(READ)",
            )
            .unwrap();
        let admin_acee = extract_acee(
            &service
                .racroute(
                    &saf_context(&admin, None, "IDEM-PREP-ACEE", 3),
                    RacrouteRequest::Verify {
                        user: admin.clone(),
                        credential_reference: SecretRef::new("secret:admin", Default::default())
                            .unwrap(),
                        action: SafVerifyAction::CreateAcee,
                        acee_id: None,
                    },
                )
                .unwrap(),
        );
        let token_digest = format!("sha256:{}", "a".repeat(64));
        service
            .racroute(
                &saf_context(&admin, Some(&admin_acee), "IDEM-PREP-TOKEN", 4),
                RacrouteRequest::Tokenbld {
                    owner: admin.clone(),
                    kind: TokenKind::SafIdentity,
                    token_reference: "secret:prep-token".into(),
                    token_digest: token_digest.clone(),
                    scopes: BTreeSet::from(["FACILITY".into()]),
                    expires_tick: Some(100),
                },
            )
            .unwrap();
        let user = PrincipalId::new("USER1", InvocationLimits::default()).unwrap();
        let cases = vec![
            (
                saf_context(&admin, None, "IDEM-AUDIT", 10),
                RacrouteRequest::Audit {
                    action: "IDEM.AUDIT".into(),
                    resource_digest: format!("sha256:{}", "1".repeat(64)),
                    decision: DecisionOutcome::Allow,
                    fields: BTreeMap::new(),
                },
            ),
            (
                saf_context(&admin, Some(&admin_acee), "IDEM-DEFINE", 11),
                RacrouteRequest::Define {
                    action: SafDefineAction::Add,
                    class: "FACILITY".into(),
                    resource: "IDEM.DEFINED".into(),
                    owner: "RACFADM".into(),
                    uacc: AccessLevel::Read,
                    generic: false,
                },
            ),
            (
                saf_context(&admin, Some(&admin_acee), "IDEM-LIST", 12),
                RacrouteRequest::List {
                    class: "FACILITY".into(),
                    global: true,
                    refresh: true,
                },
            ),
            (
                saf_context(&admin, None, "IDEM-SIGNON", 13),
                RacrouteRequest::Signon {
                    user,
                    credential_reference: SecretRef::new("secret:user1", Default::default())
                        .unwrap(),
                },
            ),
            (
                saf_context(&admin, Some(&admin_acee), "IDEM-TOKENBLD", 14),
                RacrouteRequest::Tokenbld {
                    owner: admin.clone(),
                    kind: TokenKind::SafIdentity,
                    token_reference: "secret:replay-token".into(),
                    token_digest: format!("sha256:{}", "b".repeat(64)),
                    scopes: BTreeSet::from(["FACILITY".into()]),
                    expires_tick: Some(101),
                },
            ),
            (
                saf_context(&admin, None, "IDEM-TOKENMAP", 15),
                RacrouteRequest::Tokenmap {
                    token_digest: token_digest.clone(),
                },
            ),
            (
                saf_context(&admin, None, "IDEM-VERIFY", 16),
                RacrouteRequest::Verify {
                    user: admin.clone(),
                    credential_reference: SecretRef::new("secret:admin", Default::default())
                        .unwrap(),
                    action: SafVerifyAction::CreateAcee,
                    acee_id: None,
                },
            ),
            (
                saf_context(&admin, None, "IDEM-VERIFYX", 17),
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
        for (context, request) in cases {
            let first = service.racroute(&context, request.clone()).unwrap();
            let after_first = service.database().summary().unwrap();
            let replay = service.racroute(&context, request).unwrap();
            let after_replay = service.database().summary().unwrap();
            assert_eq!(
                replay, first,
                "{:?} did not replay exactly",
                first.request_type
            );
            assert_eq!(
                after_replay, after_first,
                "{:?} replay duplicated state",
                first.request_type
            );
        }

        let context = saf_context(&admin, Some(&admin_acee), "IDEM-CONFLICT", 30);
        service
            .racroute(
                &context,
                RacrouteRequest::List {
                    class: "FACILITY".into(),
                    global: true,
                    refresh: true,
                },
            )
            .unwrap();
        let before_conflict = service.database().summary().unwrap();
        assert_eq!(
            service.racroute(
                &context,
                RacrouteRequest::List {
                    class: "DATASET".into(),
                    global: true,
                    refresh: true,
                },
            ),
            Err(HostProblem::IdempotencyConflict)
        );
        let after_conflict = service.database().summary().unwrap();
        assert_eq!(
            after_conflict.raclist_caches,
            before_conflict.raclist_caches
        );
        assert_eq!(after_conflict.transactions, before_conflict.transactions);
        assert_eq!(after_conflict.audits, before_conflict.audits + 1);
    }

    #[test]
    fn caller_and_shape_are_checked_before_secret_resolution_and_failures_are_normalized() {
        let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
        let resolver = Arc::new(CountingResolver::new());
        resolver.insert("secret:admin", b"ADMIN-PASSWORD");
        resolver.insert("secret:wrong", b"WRONG-PASSWORD");
        let service = RacfService::open(store, resolver.clone(), Default::default()).unwrap();
        service
            .bootstrap_administrator(
                "RACFADM",
                &SecretRef::new("secret:admin", Default::default()).unwrap(),
            )
            .unwrap();
        let calls_after_bootstrap = resolver.calls();
        let missing = PrincipalId::new("MISSING", InvocationLimits::default()).unwrap();
        let admin = PrincipalId::new("RACFADM", InvocationLimits::default()).unwrap();
        let unknown_existing = service
            .racroute(
                &saf_context(&missing, None, "PREFLIGHT-UNKNOWN-EXISTING", 1),
                RacrouteRequest::Verify {
                    user: admin.clone(),
                    credential_reference: SecretRef::new("secret:admin", Default::default())
                        .unwrap(),
                    action: SafVerifyAction::AuthenticateOnly,
                    acee_id: None,
                },
            )
            .unwrap();
        let unknown_missing = service
            .racroute(
                &saf_context(&missing, None, "PREFLIGHT-UNKNOWN-MISSING", 2),
                RacrouteRequest::Verify {
                    user: admin.clone(),
                    credential_reference: SecretRef::new(
                        "secret:not-installed",
                        Default::default(),
                    )
                    .unwrap(),
                    action: SafVerifyAction::AuthenticateOnly,
                    acee_id: None,
                },
            )
            .unwrap();
        assert_eq!(resolver.calls(), calls_after_bootstrap);
        assert_eq!(unknown_existing.status, unknown_missing.status);
        assert_eq!(unknown_existing.states, unknown_missing.states);
        assert_eq!(unknown_existing.result, unknown_missing.result);
        assert_eq!(
            unknown_existing.status.reason,
            DecisionReason::PrincipalNotFound
        );

        let wrong = service
            .racroute(
                &saf_context(&admin, None, "PREFLIGHT-WRONG", 3),
                RacrouteRequest::Verify {
                    user: admin.clone(),
                    credential_reference: SecretRef::new("secret:wrong", Default::default())
                        .unwrap(),
                    action: SafVerifyAction::AuthenticateOnly,
                    acee_id: None,
                },
            )
            .unwrap();
        let absent = service
            .racroute(
                &saf_context(&admin, None, "PREFLIGHT-ABSENT", 4),
                RacrouteRequest::Verify {
                    user: admin,
                    credential_reference: SecretRef::new(
                        "secret:not-installed",
                        Default::default(),
                    )
                    .unwrap(),
                    action: SafVerifyAction::AuthenticateOnly,
                    acee_id: None,
                },
            )
            .unwrap();
        assert_eq!(wrong.status, absent.status);
        assert_eq!(wrong.states, absent.states);
        assert_eq!(wrong.status.reason, DecisionReason::CredentialInvalid);
        assert!(matches!(
            (&wrong.result, &absent.result),
            (
                Some(RacrouteResult::Verified { acee: None, .. }),
                Some(RacrouteResult::Verified { acee: None, .. })
            )
        ));
        let audits = service.database.read().unwrap().audits;
        assert_eq!(
            audits
                .iter()
                .filter(|audit| audit.action == "VERIFY")
                .count(),
            4
        );
        assert!(
            audits
                .iter()
                .rev()
                .take(4)
                .all(|audit| audit.decision == DecisionOutcome::Deny)
        );
    }

    #[test]
    fn racroute_unknown_outcome_reconciles_once_and_replays_original_identity_after_restart() {
        let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
        let resolver = Arc::new(MemorySecretResolver::default());
        resolver.insert("secret:admin", b"ADMIN-PASSWORD".to_vec());
        let admin = PrincipalId::new("RACFADM", InvocationLimits::default()).unwrap();
        let context;
        let request;
        let first;
        {
            let service =
                RacfService::open(store.clone(), resolver.clone(), Default::default()).unwrap();
            service
                .bootstrap_administrator(
                    "RACFADM",
                    &SecretRef::new("secret:admin", Default::default()).unwrap(),
                )
                .unwrap();
            let acee = extract_acee(
                &service
                    .racroute(
                        &saf_context(&admin, None, "UNKNOWN-PREP", 1),
                        RacrouteRequest::Verify {
                            user: admin.clone(),
                            credential_reference: SecretRef::new(
                                "secret:admin",
                                Default::default(),
                            )
                            .unwrap(),
                            action: SafVerifyAction::CreateAcee,
                            acee_id: None,
                        },
                    )
                    .unwrap(),
            );
            context = saf_context(&admin, Some(&acee), "UNKNOWN-TOKEN", 2);
            request = RacrouteRequest::Tokenbld {
                owner: admin.clone(),
                kind: TokenKind::SafIdentity,
                token_reference: "secret:unknown-token".into(),
                token_digest: format!("sha256:{}", "c".repeat(64)),
                scopes: BTreeSet::from(["FACILITY".into()]),
                expires_tick: Some(100),
            };
            first = service.racroute(&context, request.clone()).unwrap();
            service
                .database
                .mutate(|snapshot| {
                    let transaction = snapshot.transactions.get_mut("UNKNOWN-TOKEN").unwrap();
                    transaction.state = TransactionState::UnknownOutcome;
                    transaction.terminal_tick = None;
                    Ok(())
                })
                .unwrap();
        }
        let reopened =
            RacfService::open(store.clone(), resolver.clone(), Default::default()).unwrap();
        let reconciled = reopened.database.read().unwrap();
        assert_eq!(
            reconciled.transactions["UNKNOWN-TOKEN"].state,
            TransactionState::Committed
        );
        assert_eq!(reconciled.recovery.len(), 1);
        assert_eq!(reconciled.recovery.values().next().unwrap().attempt, 1);
        drop(reconciled);
        let before_replay = reopened.database().summary().unwrap();
        assert_eq!(reopened.racroute(&context, request.clone()).unwrap(), first);
        assert_eq!(reopened.database().summary().unwrap(), before_replay);
        drop(reopened);

        let reopened_again = RacfService::open(store, resolver, Default::default()).unwrap();
        let stable = reopened_again.database().summary().unwrap();
        assert_eq!(stable.recovery_records, 1);
        assert_eq!(reopened_again.racroute(&context, request).unwrap(), first);
        assert_eq!(reopened_again.database().summary().unwrap(), stable);
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
    fn acee_extract_requires_owner_auditor_or_special_and_audits_both_paths() {
        let (service, resolver, admin) = setup();
        resolver.insert("secret:user2", b"SECOND-PASSWORD".to_vec());
        for (index, command) in [
            "ADDUSER USER1 PASSWORD('USER-PASSWORD')",
            "ADDUSER USER2 PASSWORD('SECOND-PASSWORD')",
        ]
        .into_iter()
        .enumerate()
        {
            service
                .execute_command(
                    &command_context(&admin, &format!("EXTRACT-SETUP-{index}"), index as u64 + 1),
                    command,
                )
                .unwrap();
        }
        let user1 = PrincipalId::new("USER1", InvocationLimits::default()).unwrap();
        let user2 = PrincipalId::new("USER2", InvocationLimits::default()).unwrap();
        let user1_acee = extract_acee(
            &service
                .racroute(
                    &saf_context(&admin, None, "EXTRACT-VERIFY-1", 10),
                    RacrouteRequest::Verify {
                        user: user1.clone(),
                        credential_reference: SecretRef::new("secret:user1", Default::default())
                            .unwrap(),
                        action: SafVerifyAction::CreateAcee,
                        acee_id: None,
                    },
                )
                .unwrap(),
        );
        let user2_acee = extract_acee(
            &service
                .racroute(
                    &saf_context(&admin, None, "EXTRACT-VERIFY-2", 11),
                    RacrouteRequest::Verify {
                        user: user2.clone(),
                        credential_reference: SecretRef::new("secret:user2", Default::default())
                            .unwrap(),
                        action: SafVerifyAction::CreateAcee,
                        acee_id: None,
                    },
                )
                .unwrap(),
        );
        let admin_acee = extract_acee(
            &service
                .racroute(
                    &saf_context(&admin, None, "EXTRACT-VERIFY-ADMIN", 12),
                    RacrouteRequest::Verify {
                        user: admin.clone(),
                        credential_reference: SecretRef::new("secret:admin", Default::default())
                            .unwrap(),
                        action: SafVerifyAction::CreateAcee,
                        acee_id: None,
                    },
                )
                .unwrap(),
        );
        let before = service.database.read().unwrap().audits.len();
        let denied = service
            .racroute(
                &saf_context(&user2, Some(&user2_acee), "EXTRACT-DENIED", 13),
                RacrouteRequest::Extract {
                    kind: SafExtractKind::Acee,
                    class: None,
                    name: user1_acee.clone(),
                },
            )
            .unwrap();
        assert_eq!(denied.status.reason, DecisionReason::InsufficientAccess);
        assert!(denied.result.is_none());
        let owner = service
            .racroute(
                &saf_context(&user1, Some(&user1_acee), "EXTRACT-OWNER", 14),
                RacrouteRequest::Extract {
                    kind: SafExtractKind::Acee,
                    class: None,
                    name: user1_acee.clone(),
                },
            )
            .unwrap();
        assert_eq!(owner.status.reason, DecisionReason::Granted);
        assert!(matches!(
            owner.result,
            Some(RacrouteResult::Extracted(ExtractedSecurityRecord::Acee(AceeSummary {
                principal,
                ..
            }))) if principal == "USER1"
        ));
        let privileged = service
            .racroute(
                &saf_context(&admin, Some(&admin_acee), "EXTRACT-SPECIAL", 15),
                RacrouteRequest::Extract {
                    kind: SafExtractKind::Acee,
                    class: None,
                    name: user1_acee,
                },
            )
            .unwrap();
        assert_eq!(privileged.status.reason, DecisionReason::Granted);
        let audits = service.database.read().unwrap().audits;
        assert_eq!(audits.len(), before + 3);
        assert_eq!(audits[audits.len() - 3].decision, DecisionOutcome::Deny);
        assert!(
            audits
                .iter()
                .rev()
                .take(2)
                .all(|audit| audit.decision == DecisionOutcome::Allow)
        );
    }

    #[test]
    fn closed_verify_and_signon_acees_do_not_block_deluser_after_restart() {
        let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
        let resolver = Arc::new(MemorySecretResolver::default());
        resolver.insert("secret:admin", b"ADMIN-PASSWORD".to_vec());
        resolver.insert("secret:user1", b"USER-PASSWORD".to_vec());
        let admin = PrincipalId::new("RACFADM", InvocationLimits::default()).unwrap();
        let user = PrincipalId::new("USER1", InvocationLimits::default()).unwrap();
        {
            let service =
                RacfService::open(store.clone(), resolver.clone(), Default::default()).unwrap();
            service
                .bootstrap_administrator(
                    "RACFADM",
                    &SecretRef::new("secret:admin", Default::default()).unwrap(),
                )
                .unwrap();
            service
                .execute_command(
                    &command_context(&admin, "DELETE-SETUP", 1),
                    "ADDUSER USER1 PASSWORD('USER-PASSWORD')",
                )
                .unwrap();
            let verified = service
                .racroute(
                    &saf_context(&admin, None, "DELETE-VERIFY-CREATE", 2),
                    RacrouteRequest::Verify {
                        user: user.clone(),
                        credential_reference: SecretRef::new("secret:user1", Default::default())
                            .unwrap(),
                        action: SafVerifyAction::CreateAcee,
                        acee_id: None,
                    },
                )
                .unwrap();
            let verified_acee = extract_acee(&verified);
            service
                .racroute(
                    &saf_context(&user, Some(&verified_acee), "DELETE-VERIFY-CLOSE", 3),
                    RacrouteRequest::Verify {
                        user: user.clone(),
                        credential_reference: SecretRef::new("secret:user1", Default::default())
                            .unwrap(),
                        action: SafVerifyAction::DeleteAcee,
                        acee_id: Some(verified_acee),
                    },
                )
                .unwrap();
            let signed_on = service
                .racroute(
                    &saf_context(&admin, None, "DELETE-SIGNON", 4),
                    RacrouteRequest::Signon {
                        user: user.clone(),
                        credential_reference: SecretRef::new("secret:user1", Default::default())
                            .unwrap(),
                    },
                )
                .unwrap();
            let signon_acee = extract_acee(&signed_on);
            service
                .execute_command(
                    &command_context(&admin, "DELETE-SIGNOFF", 5),
                    "SIGNOFF USER(USER1)",
                )
                .unwrap();
            assert_eq!(
                service.database.read().unwrap().acees[&signon_acee].state,
                AceeState::Deleted
            );
        }
        let reopened = RacfService::open(store, resolver, Default::default()).unwrap();
        reopened
            .execute_command(&command_context(&admin, "DELETE-USER", 6), "DELUSER USER1")
            .unwrap();
        let snapshot = reopened.database.read().unwrap();
        assert!(!snapshot.principals.contains_key("USER1"));
        assert!(
            snapshot
                .acees
                .values()
                .all(|acee| acee.principal != "USER1")
        );
        assert!(
            snapshot
                .signon_sessions
                .values()
                .all(|session| session.user != "USER1")
        );
    }

    #[test]
    fn sqlite_restart_preserves_sec_505_identity_state_and_sessions() {
        let directory = std::env::temp_dir().join(format!(
            "mainframe-env-racf-sec505-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("racf-sec505.db");
        let url = format!("sqlite://{}?mode=rwc", path.display());
        let resolver = Arc::new(MemorySecretResolver::default());
        for (reference, value) in [
            ("secret:admin", b"ADMIN-PASSWORD".as_slice()),
            ("secret:user1", b"USER-PASSWORD".as_slice()),
            ("secret:user1-new", b"NEW-USER-PASSWORD".as_slice()),
            ("secret:user2", b"SECOND-PASSWORD".as_slice()),
            ("secret:mfa", b"123456".as_slice()),
            ("secret:mfa-proof", b"123456".as_slice()),
        ] {
            resolver.insert(reference, value.to_vec());
        }
        let admin = PrincipalId::new("RACFADM", InvocationLimits::default()).unwrap();
        let generation = {
            let store: Arc<dyn ProviderStateStore> =
                Arc::new(SqliteStateStore::open(&url, 32 * 1024 * 1024, 65_536).unwrap());
            let service = RacfService::open(store, resolver.clone(), Default::default()).unwrap();
            service
                .bootstrap_administrator(
                    "RACFADM",
                    &SecretRef::new("secret:admin", Default::default()).unwrap(),
                )
                .unwrap();
            let fingerprint = format!("sha256:{}", "a".repeat(64));
            for (index, command) in [
                "ADDUSER USER1 PASSWORD('USER-PASSWORD') MFA(FACTOR1 REF secret:mfa TYPE TOTP)"
                    .to_string(),
                "PASSWORD USER(USER1) PASSWORD('NEW-USER-PASSWORD')".into(),
                "ADDUSER USER2 PASSWORD('SECOND-PASSWORD')".into(),
                "TARGET NODE(NODE1) PROTOCOL(TCP)".into(),
                "RACLINK USER1 DEFINE(NODE1 REMOTE1)".into(),
                "RACMAP ID(USER1) MAP(MAP1 REGISTRY LDAP NAME user1@example.com)".into(),
                format!(
                    "RACDCERT ID(USER1) ADD(CERT1 CERTREF secret:cert1 FINGERPRINT {fingerprint} KEYID KEY1 KEYREF secret:key1)"
                ),
                "RACDCERT ID(USER1) CONNECT(CERT1 RING RING1 DEFAULT)".into(),
            ]
            .iter()
            .enumerate()
            {
                service
                    .execute_command(
                        &command_context(&admin, &format!("RESTART-{index}"), index as u64 + 1),
                        command,
                    )
                    .unwrap();
            }
            let user2 = PrincipalId::new("USER2", InvocationLimits::default()).unwrap();
            service
                .racroute(
                    &saf_context(&admin, None, "RESTART-SIGNON", 20),
                    RacrouteRequest::Signon {
                        user: user2,
                        credential_reference: SecretRef::new("secret:user2", Default::default())
                            .unwrap(),
                    },
                )
                .unwrap();
            service.database.summary().unwrap().generation
        };
        {
            let store: Arc<dyn ProviderStateStore> =
                Arc::new(SqliteStateStore::open(&url, 32 * 1024 * 1024, 65_536).unwrap());
            let service = RacfService::open(store, resolver, Default::default()).unwrap();
            let summary = service.database.summary().unwrap();
            assert_eq!(summary.generation, generation);
            assert_eq!(
                (
                    summary.certificates,
                    summary.keys,
                    summary.keyrings,
                    summary.mfa_factors,
                    summary.identity_mappings,
                    summary.user_associations,
                    summary.rrsf_nodes,
                    summary.signon_sessions,
                ),
                (1, 1, 1, 1, 1, 1, 1, 1)
            );
            let user1 = PrincipalId::new("USER1", InvocationLimits::default()).unwrap();
            let verified = service
                .racroute(
                    &saf_context(&admin, None, "RESTART-VERIFYX", 21),
                    RacrouteRequest::Verifyx {
                        user: user1,
                        credential_reference: SecretRef::new(
                            "secret:user1-new",
                            Default::default(),
                        )
                        .unwrap(),
                        mfa_reference: Some(
                            SecretRef::new("secret:mfa-proof", Default::default()).unwrap(),
                        ),
                        action: SafVerifyAction::AuthenticateOnly,
                        acee_id: None,
                        parent_acee: None,
                    },
                )
                .unwrap();
            assert_eq!(verified.status.reason, DecisionReason::Granted);
        }
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_dir(directory);
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
