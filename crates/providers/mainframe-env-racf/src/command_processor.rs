use crate::RacfService;
use crate::authority::{CredentialPolicyProblem, trim_credential_history};
use crate::command::{
    CommandDiagnostic, CommandDiagnosticCode, CommandFamily, CommandLanguageLimits, ParsedCommand,
    ParsedOperand, diagnostic, diagnostic_with_host_problem, parse_command,
};
use crate::model::{
    AccessCondition, AccessControlEntry, AccessLevel, AssociationState, AuditFieldValue,
    AuditPolicy, CertificateReference, ClassDescriptor, CredentialVerifier, DatabaseSharingMode,
    DecisionOutcome, DecisionReason, GroupAuthority, GroupConnection, GroupProfile,
    IdentityMapping, KeyReference, KeyRing, MfaFactor, MfaFactorKind, PrincipalKind,
    PrincipalProfile, PrincipalState, ProfileSegment, ProfileTemplate, RaclistCache,
    ResourceProfile, RrsfNode, RrsfNodeState, SafStatus, SecurityAuditRecord,
    SecurityDatabaseSnapshot, SecurityPolicyOptions, SecurityRequestDigestFormat,
    SecurityTransaction, SegmentFieldKind, SegmentFieldSchema, SegmentTemplate, SegmentValue,
    SignonSessionState, TransactionState, UserAssociation, connection_key, keyring_key,
    profile_key,
};
use argon2::Argon2;
use argon2::password_hash::{PasswordVerifier, phc::PasswordHash};
use mainframe_env_execution_api::PrincipalId;
use mainframe_env_host_api::HostProblem;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommandContext {
    actor: PrincipalId,
    idempotency_key: String,
    correlation: String,
    tick: u64,
}

impl CommandContext {
    pub fn new(
        actor: PrincipalId,
        idempotency_key: impl Into<String>,
        correlation: impl Into<String>,
        tick: u64,
    ) -> Result<Self, CommandDiagnostic> {
        let idempotency_key = idempotency_key.into().to_ascii_uppercase();
        let correlation = correlation.into();
        if idempotency_key.is_empty()
            || idempotency_key.len() > 128
            || !idempotency_key.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b':' | b'-' | b'_')
            })
            || correlation.is_empty()
            || correlation.len() > 246
            || correlation.chars().any(char::is_control)
        {
            return Err(diagnostic(CommandDiagnosticCode::InvalidValue, 0));
        }
        Ok(Self {
            actor,
            idempotency_key,
            correlation,
            tick,
        })
    }

    #[must_use]
    pub fn actor(&self) -> &PrincipalId {
        &self.actor
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

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CommandObjectKind {
    User,
    Group,
    Connection,
    DatasetProfile,
    ResourceProfile,
    Database,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case", tag = "record_kind")]
pub enum CommandRecord {
    Name {
        kind: CommandObjectKind,
        name: String,
    },
    User {
        id: String,
        owner: String,
        default_group: Option<String>,
        state: PrincipalState,
        attributes: BTreeSet<String>,
        groups: BTreeSet<String>,
        segments: BTreeSet<String>,
        version: u64,
    },
    Group {
        name: String,
        owner: String,
        superior_group: Option<String>,
        universal: bool,
        members: BTreeSet<String>,
        segments: BTreeSet<String>,
        version: u64,
    },
    Profile {
        class: String,
        name: String,
        owner: String,
        generic: bool,
        uacc: AccessLevel,
        access_entries: usize,
        segments: BTreeSet<String>,
        version: u64,
    },
    Summary {
        users: usize,
        groups: usize,
        profiles: usize,
        generation: u64,
    },
    Class {
        name: String,
        supplied: bool,
        active: bool,
        generic_active: bool,
        raclist: bool,
        cache_generation: Option<u64>,
        version: u64,
    },
    Policy {
        add_creator: bool,
        program_control: bool,
        ml_active: bool,
        rules: bool,
        write_down: bool,
        subsystem_running: bool,
        database_active: bool,
        database_sharing: DatabaseSharingMode,
    },
    Certificate {
        id: String,
        owner: String,
        label: String,
        fingerprint_sha256: String,
        trusted: bool,
        active: bool,
        version: u64,
    },
    Keyring {
        owner: String,
        name: String,
        certificates: BTreeSet<String>,
        default_certificate: Option<String>,
        version: u64,
    },
    IdentityMapping {
        id: String,
        registry: String,
        distributed_identity: String,
        local_user: String,
        version: u64,
    },
    Association {
        id: String,
        local_user: String,
        node: String,
        remote_user: String,
        active: bool,
        version: u64,
    },
    RrsfNode {
        name: String,
        operative: bool,
        description: Option<String>,
        protocol: Option<String>,
        version: u64,
    },
    Session {
        id: String,
        user: String,
        node: Option<String>,
        active: bool,
        version: u64,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommandResult {
    pub family: CommandFamily,
    pub status: SafStatus,
    pub generation: u64,
    pub replayed: bool,
    pub records: Vec<CommandRecord>,
}

enum SemanticProblem {
    Unauthorized,
    NotFound,
    Conflict,
    Invalid(usize),
    Exhausted,
}

enum ExecutionOutcome {
    Success(Vec<CommandRecord>),
    Failure(SemanticProblem),
}

const RACF_COMMAND_DIGEST_DOMAIN: &[u8] = b"mainframe-env.racf-command@1\0";

pub(crate) fn execute(
    service: &RacfService,
    context: &CommandContext,
    input: &str,
) -> Result<CommandResult, CommandDiagnostic> {
    let parsed = parse_command(input, CommandLanguageLimits::default())?;
    if !matches!(
        parsed.descriptor.work_package(),
        "SEC-502" | "SEC-503" | "SEC-505"
    ) {
        return Err(diagnostic(CommandDiagnosticCode::UnsupportedFamily, 0));
    }
    let request_digest = command_request_digest(&parsed);
    let family = parsed.descriptor.family();
    let ((outcome, replayed, status), generation) = service
        .database
        .mutate_retry(|snapshot| {
            if parsed.descriptor.mutating()
                && let Some(existing) = snapshot.transactions.get(context.idempotency_key())
            {
                if existing.actor != context.actor().as_str()
                    || require_active(snapshot, context).is_err()
                {
                    let status = crate::saf::status_for_reason(DecisionReason::InsufficientAccess);
                    append_audit(
                        snapshot,
                        context,
                        &parsed,
                        DecisionOutcome::Deny,
                        status,
                        &request_digest,
                    )?;
                    return Ok((
                        (
                            ExecutionOutcome::Failure(SemanticProblem::Unauthorized),
                            false,
                            status,
                        ),
                        true,
                    ));
                }
                if matches!(
                    existing.request_digest_format,
                    SecurityRequestDigestFormat::LegacyUnversioned
                        | SecurityRequestDigestFormat::LegacyScrubbedV0
                ) {
                    return Err(HostProblem::UnknownOutcome);
                }
                if existing.request_digest_format
                    != SecurityRequestDigestFormat::RacfCommandCanonicalV1
                    || existing.request_digest != request_digest
                {
                    append_audit(
                        snapshot,
                        context,
                        &parsed,
                        DecisionOutcome::Deny,
                        crate::saf::status_for_reason(DecisionReason::MalformedRequest),
                        &request_digest,
                    )?;
                    return Ok((
                        (
                            ExecutionOutcome::Failure(SemanticProblem::Conflict),
                            false,
                            crate::saf::status_for_reason(DecisionReason::MalformedRequest),
                        ),
                        true,
                    ));
                }
                let outcome = if existing.state == TransactionState::Committed {
                    ExecutionOutcome::Success(Vec::new())
                } else {
                    ExecutionOutcome::Failure(problem_from_reason(existing.status.reason))
                };
                return Ok(((outcome, true, existing.status), false));
            }

            let mut staged = snapshot.clone();
            let applied = if parsed.descriptor.mutating() {
                apply_mutation(service, &mut staged, context, &parsed)
            } else {
                apply_query(&staged, context, &parsed)
            };
            match applied {
                Ok(records) => {
                    let status = crate::saf::status_for_reason(DecisionReason::Granted);
                    if parsed.descriptor.mutating() {
                        append_transaction(
                            &mut staged,
                            context,
                            &parsed,
                            &request_digest,
                            TransactionState::Committed,
                            status,
                        )?;
                    }
                    append_audit(
                        &mut staged,
                        context,
                        &parsed,
                        DecisionOutcome::Allow,
                        status,
                        &request_digest,
                    )?;
                    *snapshot = staged;
                    Ok(((ExecutionOutcome::Success(records), false, status), true))
                }
                Err(problem) => {
                    let status = crate::saf::status_for_reason(reason_for_problem(&problem));
                    if parsed.descriptor.mutating() {
                        append_transaction(
                            snapshot,
                            context,
                            &parsed,
                            &request_digest,
                            TransactionState::RolledBack,
                            status,
                        )?;
                    }
                    append_audit(
                        snapshot,
                        context,
                        &parsed,
                        DecisionOutcome::Deny,
                        status,
                        &request_digest,
                    )?;
                    Ok(((ExecutionOutcome::Failure(problem), false, status), true))
                }
            }
        })
        .map_err(host_diagnostic)?;
    match outcome {
        ExecutionOutcome::Success(records) => Ok(CommandResult {
            family,
            status,
            generation,
            replayed,
            records,
        }),
        ExecutionOutcome::Failure(problem) => Err(semantic_diagnostic(problem)),
    }
}

fn command_request_digest(command: &ParsedCommand) -> String {
    let mut digest = Sha256::new();
    digest.update(RACF_COMMAND_DIGEST_DOMAIN);
    digest_command_field(&mut digest, command.descriptor.keyword().as_bytes());
    digest_command_len(&mut digest, command.positionals.len());
    for positional in &command.positionals {
        digest_command_field(&mut digest, positional.as_bytes());
    }
    digest_command_len(&mut digest, command.operands.len());
    for operand in &command.operands {
        digest_command_field(&mut digest, operand.name.as_bytes());
        digest_command_len(&mut digest, operand.values.len());
        let credential = credential_operand(command, &operand.name);
        for value in &operand.values {
            digest.update([u8::from(credential)]);
            if !credential {
                digest_command_field(&mut digest, value.as_bytes());
            }
        }
    }
    format!("sha256:{:x}", digest.finalize())
}

fn credential_operand(command: &ParsedCommand, operand: &str) -> bool {
    matches!(
        command.descriptor.keyword(),
        "ADDUSER" | "ALTUSER" | "PASSWORD"
    ) && matches!(operand, "PASSWORD" | "PHRASE")
}

fn digest_command_len(digest: &mut Sha256, value: usize) {
    digest.update(u64::try_from(value).unwrap_or(u64::MAX).to_be_bytes());
}

fn digest_command_field(digest: &mut Sha256, value: &[u8]) {
    digest_command_len(digest, value.len());
    digest.update(value);
}

pub(crate) fn reconcile_legacy_replay(
    service: &RacfService,
    context: &CommandContext,
    expected_scrubbed_digest: &str,
    input: &str,
) -> Result<(), HostProblem> {
    if !valid_sha256(expected_scrubbed_digest) {
        return Err(HostProblem::Malformed);
    }
    let parsed = parse_command(input, CommandLanguageLimits::default())
        .map_err(|_| HostProblem::Malformed)?;
    if !parsed.descriptor.mutating()
        || !matches!(
            parsed.descriptor.work_package(),
            "SEC-502" | "SEC-503" | "SEC-505"
        )
    {
        return Err(HostProblem::Unsupported);
    }
    let canonical = command_request_digest(&parsed);
    service
        .database
        .mutate_if_changed(|snapshot| {
            let transaction = snapshot
                .transactions
                .get_mut(context.idempotency_key())
                .ok_or(HostProblem::NotFound)?;
            if transaction.actor != context.actor().as_str()
                || transaction.operation != parsed.descriptor.keyword()
            {
                return Err(HostProblem::IdempotencyConflict);
            }
            match transaction.request_digest_format {
                SecurityRequestDigestFormat::RacfCommandCanonicalV1 => {
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
                        SecurityRequestDigestFormat::RacfCommandCanonicalV1;
                    transaction.request_digest = canonical.clone();
                    Ok(((), true))
                }
                SecurityRequestDigestFormat::LegacyUnversioned => Err(HostProblem::UnknownOutcome),
                SecurityRequestDigestFormat::RacrouteCanonicalV1 => {
                    Err(HostProblem::IdempotencyConflict)
                }
            }
        })
        .map(|_| ())
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 71
        && value.starts_with("sha256:")
        && value[7..].bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn apply_mutation(
    service: &RacfService,
    snapshot: &mut SecurityDatabaseSnapshot,
    context: &CommandContext,
    command: &ParsedCommand,
) -> Result<Vec<CommandRecord>, SemanticProblem> {
    reject_unsupported_direction(command)?;
    require_active(snapshot, context)?;
    match command.descriptor.family() {
        CommandFamily::AddGroup => add_group(snapshot, context, command),
        CommandFamily::AddUser => add_user(service, snapshot, context, command),
        CommandFamily::AltGroup => alter_group(snapshot, context, command),
        CommandFamily::AltUser => alter_user(service, snapshot, context, command),
        CommandFamily::Connect => connect(snapshot, context, command),
        CommandFamily::Remove => remove(snapshot, context, command),
        CommandFamily::DelGroup => delete_groups(snapshot, context, command),
        CommandFamily::DelUser => delete_users(snapshot, context, command),
        CommandFamily::AddSd | CommandFamily::Rdefine => {
            define_resources(snapshot, context, command)
        }
        CommandFamily::AltSd | CommandFamily::Ralter => alter_resources(snapshot, context, command),
        CommandFamily::DelSd | CommandFamily::Rdelete => {
            delete_resources(snapshot, context, command)
        }
        CommandFamily::Permit => permit(snapshot, context, command),
        CommandFamily::Racpriv => racpriv(snapshot, context, command),
        CommandFamily::Restart => restart(snapshot, context),
        CommandFamily::Rvary => rvary(snapshot, context, command),
        CommandFamily::Set => set_operational_options(snapshot, context, command),
        CommandFamily::Setropts => setropts(snapshot, context, command),
        CommandFamily::Stop => stop(snapshot, context),
        CommandFamily::Password => password(service, snapshot, context, command),
        CommandFamily::Racdcert => racdcert(snapshot, context, command),
        CommandFamily::Raclink => raclink(snapshot, context, command),
        CommandFamily::Racmap => racmap(snapshot, context, command),
        CommandFamily::Signoff => signoff(snapshot, context, command),
        CommandFamily::Target => target(snapshot, context, command),
        _ => Err(SemanticProblem::Invalid(0)),
    }
}

fn apply_query(
    snapshot: &SecurityDatabaseSnapshot,
    context: &CommandContext,
    command: &ParsedCommand,
) -> Result<Vec<CommandRecord>, SemanticProblem> {
    reject_unsupported_direction(command)?;
    require_active(snapshot, context)?;
    match command.descriptor.family() {
        CommandFamily::Display => display(snapshot, context, command),
        CommandFamily::ListUser => list_users(snapshot, context, command),
        CommandFamily::ListGrp => list_groups(snapshot, command),
        CommandFamily::ListDsd => list_profiles(snapshot, "DATASET", command, 0),
        CommandFamily::Rlist => {
            let class = command.positional(0).ok_or(SemanticProblem::Invalid(0))?;
            list_profiles(snapshot, &upper_class(class)?, command, 1)
        }
        CommandFamily::Search => search(snapshot, command),
        CommandFamily::Racprmck => validate_parmlib_members(command),
        _ => Err(SemanticProblem::Invalid(0)),
    }
}

fn password(
    service: &RacfService,
    snapshot: &mut SecurityDatabaseSnapshot,
    context: &CommandContext,
    command: &ParsedCommand,
) -> Result<Vec<CommandRecord>, SemanticProblem> {
    if command.has_operand("PASSWORD") && command.has_operand("PHRASE") {
        return Err(SemanticProblem::Invalid(0));
    }
    let target =
        operand_principal(command, "USER")?.unwrap_or_else(|| context.actor().as_str().to_string());
    if target != context.actor().as_str() && !is_special(snapshot, context) {
        return Err(SemanticProblem::Unauthorized);
    }
    let operand = command
        .operand("PASSWORD")
        .or_else(|| command.operand("PHRASE"))
        .ok_or(SemanticProblem::Invalid(0))?;
    let values = operand.values().collect::<Vec<_>>();
    if values.is_empty() || values.len() > 2 {
        return Err(SemanticProblem::Invalid(operand.offset));
    }
    let current = snapshot
        .principals
        .get(&target)
        .cloned()
        .ok_or(SemanticProblem::NotFound)?;
    let changing_self = target == context.actor().as_str();
    if changing_self {
        let old = (values.len() == 2)
            .then_some(values[0])
            .ok_or(SemanticProblem::Invalid(operand.offset))?;
        let credential = current
            .credential
            .as_ref()
            .ok_or(SemanticProblem::Invalid(operand.offset))?;
        let parsed = PasswordHash::new(&credential.encoded_verifier)
            .map_err(|_| SemanticProblem::Conflict)?;
        if Argon2::default()
            .verify_password(old.as_bytes(), &parsed)
            .is_err()
        {
            return Err(SemanticProblem::Unauthorized);
        }
    }
    let new_secret = values[values.len() - 1];
    let mut next = current;
    next.credential = Some(replace_credential(
        service,
        snapshot,
        &target,
        next.credential.as_ref(),
        new_secret,
        command.has_operand("PHRASE"),
        operand.offset,
        context.tick(),
    )?);
    next.state = PrincipalState::Active;
    next.version = checked_version(next.version)?;
    snapshot.principals.insert(target.clone(), next);
    Ok(vec![CommandRecord::Name {
        kind: CommandObjectKind::User,
        name: target,
    }])
}

#[allow(clippy::too_many_arguments)]
fn replace_credential(
    service: &RacfService,
    snapshot: &SecurityDatabaseSnapshot,
    user: &str,
    current: Option<&CredentialVerifier>,
    secret: &str,
    phrase: bool,
    offset: usize,
    tick: u64,
) -> Result<CredentialVerifier, SemanticProblem> {
    service
        .credential_from_bytes(
            &snapshot.policy,
            user,
            current,
            secret.as_bytes(),
            phrase,
            tick,
        )
        .map_err(|problem| match problem {
            CredentialPolicyProblem::Invalid | CredentialPolicyProblem::Infrastructure => {
                SemanticProblem::Invalid(offset)
            }
            CredentialPolicyProblem::Reused => SemanticProblem::Conflict,
        })
}

fn racdcert(
    snapshot: &mut SecurityDatabaseSnapshot,
    context: &CommandContext,
    command: &ParsedCommand,
) -> Result<Vec<CommandRecord>, SemanticProblem> {
    let owner =
        operand_principal(command, "ID")?.unwrap_or_else(|| context.actor().as_str().to_string());
    if owner != context.actor().as_str() && !is_special(snapshot, context) {
        return Err(SemanticProblem::Unauthorized);
    }
    require_principal(snapshot, &owner)?;
    let actions = [
        "ADD",
        "ALTER",
        "CHECKCERT",
        "CONNECT",
        "DELETE",
        "DISCONNECT",
        "EXPORT",
        "GENCERT",
        "GENREQ",
        "IMPORT",
        "LIST",
        "LISTCHAIN",
        "START",
        "STOP",
    ]
    .into_iter()
    .filter_map(|name| command.operand(name).map(|operand| (name, operand)))
    .collect::<Vec<_>>();
    if actions.len() != 1 {
        return Err(SemanticProblem::Invalid(0));
    }
    let (action, operand) = actions[0];
    match action {
        "ADD" | "GENCERT" | "IMPORT" => {
            require_special(snapshot, context)?;
            let id = inner_first_name(operand)?;
            let certificate_reference =
                inner_value(operand, "CERTREF").ok_or(SemanticProblem::Invalid(operand.offset))?;
            let fingerprint = inner_value(operand, "FINGERPRINT")
                .ok_or(SemanticProblem::Invalid(operand.offset))?;
            let fingerprint = normalized_digest(fingerprint)?;
            let reference = normalized_reference(certificate_reference)?;
            let label = inner_value(operand, "LABEL").unwrap_or(&id).to_string();
            if snapshot.certificates.contains_key(&id) {
                return Err(SemanticProblem::Conflict);
            }
            snapshot.certificates.insert(
                id.clone(),
                CertificateReference {
                    id: id.clone(),
                    owner: owner.clone(),
                    label,
                    certificate_reference: reference,
                    fingerprint_sha256: fingerprint,
                    trusted: inner_flag(operand, "TRUST"),
                    active: true,
                    not_before_tick: None,
                    not_after_tick: None,
                    version: 1,
                },
            );
            if let (Some(key_id), Some(key_reference)) = (
                inner_value(operand, "KEYID"),
                inner_value(operand, "KEYREF"),
            ) {
                let key_id = bounded_upper(key_id, 246, false)?;
                if snapshot.keys.contains_key(&key_id) {
                    return Err(SemanticProblem::Conflict);
                }
                snapshot.keys.insert(
                    key_id.clone(),
                    KeyReference {
                        id: key_id,
                        owner: owner.clone(),
                        key_reference: normalized_reference(key_reference)?,
                        algorithm: inner_value(operand, "ALGORITHM")
                            .unwrap_or("REFERENCE")
                            .to_string(),
                        exportable: inner_flag(operand, "EXPORTABLE"),
                        version: 1,
                    },
                );
            }
            Ok(vec![certificate_record(&snapshot.certificates[&id])])
        }
        "DELETE" => {
            let id = inner_first_name(operand)?;
            let certificate = snapshot
                .certificates
                .get(&id)
                .ok_or(SemanticProblem::NotFound)?;
            if certificate.owner != owner && !is_special(snapshot, context) {
                return Err(SemanticProblem::Unauthorized);
            }
            if snapshot
                .keyrings
                .values()
                .any(|ring| ring.certificates.contains(&id))
            {
                return Err(SemanticProblem::Conflict);
            }
            snapshot.certificates.remove(&id);
            Ok(vec![CommandRecord::Name {
                kind: CommandObjectKind::ResourceProfile,
                name: id,
            }])
        }
        "CONNECT" | "DISCONNECT" => {
            let certificate_id = inner_first_name(operand)?;
            let ring_name = inner_value(operand, "RING")
                .or_else(|| command.operand("RING").and_then(ParsedOperand::first))
                .ok_or(SemanticProblem::Invalid(operand.offset))?;
            let ring_name = bounded_upper(ring_name, 246, false)?;
            if !snapshot.certificates.contains_key(&certificate_id) {
                return Err(SemanticProblem::NotFound);
            }
            let key = keyring_key(&owner, &ring_name);
            let ring = snapshot.keyrings.entry(key).or_insert(KeyRing {
                owner: owner.clone(),
                name: ring_name.clone(),
                certificates: BTreeSet::new(),
                default_certificate: None,
                version: 1,
            });
            if action == "CONNECT" {
                ring.certificates.insert(certificate_id.clone());
                if inner_flag(operand, "DEFAULT") {
                    ring.default_certificate = Some(certificate_id);
                }
            } else {
                ring.certificates.remove(&certificate_id);
                if ring.default_certificate.as_ref() == Some(&certificate_id) {
                    ring.default_certificate = None;
                }
            }
            ring.version = checked_version(ring.version)?;
            Ok(vec![keyring_record(ring)])
        }
        "START" | "STOP" | "ALTER" => {
            let id = inner_first_name(operand)?;
            let special = is_special(snapshot, context);
            let certificate = snapshot
                .certificates
                .get_mut(&id)
                .ok_or(SemanticProblem::NotFound)?;
            if certificate.owner != owner && !special {
                return Err(SemanticProblem::Unauthorized);
            }
            if action == "START" {
                certificate.active = true;
            } else if action == "STOP" {
                certificate.active = false;
            }
            if action == "ALTER" {
                if inner_flag(operand, "TRUST") {
                    certificate.trusted = true;
                }
                if inner_flag(operand, "NOTRUST") {
                    certificate.trusted = false;
                }
            }
            certificate.version = checked_version(certificate.version)?;
            Ok(vec![certificate_record(certificate)])
        }
        "CHECKCERT" | "EXPORT" | "GENREQ" => {
            let id = inner_first_name(operand)?;
            let certificate = snapshot
                .certificates
                .get(&id)
                .ok_or(SemanticProblem::NotFound)?;
            Ok(vec![certificate_record(certificate)])
        }
        "LIST" | "LISTCHAIN" => Ok(snapshot
            .certificates
            .values()
            .filter(|certificate| certificate.owner == owner)
            .map(certificate_record)
            .collect()),
        _ => Err(SemanticProblem::Invalid(operand.offset)),
    }
}

fn raclink(
    snapshot: &mut SecurityDatabaseSnapshot,
    context: &CommandContext,
    command: &ParsedCommand,
) -> Result<Vec<CommandRecord>, SemanticProblem> {
    let local_user = upper_principal(command.positional(0).ok_or(SemanticProblem::Invalid(0))?)?;
    if local_user != context.actor().as_str() && !is_special(snapshot, context) {
        return Err(SemanticProblem::Unauthorized);
    }
    require_principal(snapshot, &local_user)?;
    if let Some(operand) = command.operand("DEFINE") {
        let values = operand.values().collect::<Vec<_>>();
        if values.len() < 2 {
            return Err(SemanticProblem::Invalid(operand.offset));
        }
        let node = bounded_upper(values[0], 32, false)?;
        let remote_user = upper_principal(values[1])?;
        if !snapshot.rrsf_nodes.contains_key(&node) {
            return Err(SemanticProblem::NotFound);
        }
        let id = format!("{local_user}:{node}");
        if snapshot.user_associations.contains_key(&id) {
            return Err(SemanticProblem::Conflict);
        }
        let association = UserAssociation {
            id: id.clone(),
            local_user: local_user.clone(),
            node: node.clone(),
            remote_user: remote_user.clone(),
            peer: command.has_operand("PEER"),
            password_sync: command.has_operand("PWDONLY"),
            state: AssociationState::Active,
            version: 1,
        };
        snapshot.user_associations.insert(id, association.clone());
        return Ok(vec![association_record(&association)]);
    }
    if let Some(operand) = command.operand("UNDEFINE") {
        let node = bounded_upper(
            operand
                .first()
                .ok_or(SemanticProblem::Invalid(operand.offset))?,
            32,
            false,
        )?;
        let id = format!("{local_user}:{node}");
        if snapshot.user_associations.remove(&id).is_none() {
            return Err(SemanticProblem::NotFound);
        }
        return Ok(vec![CommandRecord::Name {
            kind: CommandObjectKind::Connection,
            name: id,
        }]);
    }
    if command.has_operand("LIST") {
        return Ok(snapshot
            .user_associations
            .values()
            .filter(|association| association.local_user == local_user)
            .map(association_record)
            .collect());
    }
    Err(SemanticProblem::Invalid(0))
}

fn racmap(
    snapshot: &mut SecurityDatabaseSnapshot,
    context: &CommandContext,
    command: &ParsedCommand,
) -> Result<Vec<CommandRecord>, SemanticProblem> {
    let local_user =
        operand_principal(command, "ID")?.unwrap_or_else(|| context.actor().as_str().to_string());
    if local_user != context.actor().as_str() && !is_special(snapshot, context) {
        return Err(SemanticProblem::Unauthorized);
    }
    require_principal(snapshot, &local_user)?;
    if let Some(operand) = command.operand("MAP") {
        let id = inner_first_name(operand)?;
        let registry =
            inner_value(operand, "REGISTRY").ok_or(SemanticProblem::Invalid(operand.offset))?;
        let distributed_identity =
            inner_value(operand, "NAME").ok_or(SemanticProblem::Invalid(operand.offset))?;
        let mapping = IdentityMapping {
            id: id.clone(),
            registry: normalized_text(registry, 4096)?,
            distributed_identity: normalized_text(distributed_identity, 4096)?,
            local_user,
            label: inner_value(operand, "LABEL").map(str::to_string),
            version: 1,
        };
        if snapshot
            .identity_mappings
            .insert(id, mapping.clone())
            .is_some()
        {
            return Err(SemanticProblem::Conflict);
        }
        return Ok(vec![mapping_record(&mapping)]);
    }
    for delete in ["DELAPPLE", "DELCERT", "DELDN", "DELNMAP", "DELREGISTRY"] {
        if let Some(operand) = command.operand(delete) {
            let id = inner_first_name(operand)?;
            if snapshot.identity_mappings.remove(&id).is_none() {
                return Err(SemanticProblem::NotFound);
            }
            return Ok(vec![CommandRecord::Name {
                kind: CommandObjectKind::ResourceProfile,
                name: id,
            }]);
        }
    }
    if command.has_operand("LIST") || command.has_operand("QUERY") {
        return Ok(snapshot
            .identity_mappings
            .values()
            .filter(|mapping| mapping.local_user == local_user)
            .map(mapping_record)
            .collect());
    }
    Err(SemanticProblem::Invalid(0))
}

fn signoff(
    snapshot: &mut SecurityDatabaseSnapshot,
    context: &CommandContext,
    command: &ParsedCommand,
) -> Result<Vec<CommandRecord>, SemanticProblem> {
    let users = operand_names(command, "USER", 8)?;
    let everyone = command.has_operand("EVERYONE");
    if everyone || users.iter().any(|user| user != context.actor().as_str()) {
        require_special(snapshot, context)?;
    }
    let node = operand_name(command, "AT", 32)?;
    if command.has_operand("LIST") {
        return Ok(snapshot
            .signon_sessions
            .values()
            .filter(|session| {
                session.state == SignonSessionState::Active
                    && (everyone
                        || if users.is_empty() {
                            session.user == context.actor().as_str()
                        } else {
                            users.contains(&session.user)
                        })
                    && node
                        .as_ref()
                        .is_none_or(|node| session.node.as_ref() == Some(node))
            })
            .map(session_record)
            .collect());
    }
    let mut records = Vec::new();
    let session_ids = snapshot
        .signon_sessions
        .values()
        .filter(|session| {
            session.state == SignonSessionState::Active
                && (everyone
                    || if users.is_empty() {
                        session.user == context.actor().as_str()
                    } else {
                        users.contains(&session.user)
                    })
                && node
                    .as_ref()
                    .is_none_or(|node| session.node.as_ref() == Some(node))
        })
        .map(|session| session.id.clone())
        .collect::<Vec<_>>();
    for session_id in session_ids {
        let (acee_id, record) = {
            let session = snapshot
                .signon_sessions
                .get_mut(&session_id)
                .ok_or(SemanticProblem::NotFound)?;
            let acee_id = session.acee_id.clone();
            session.state = SignonSessionState::SignedOff;
            session.version = checked_version(session.version)?;
            (acee_id, session_record(session))
        };
        if let Some(acee) = snapshot.acees.get_mut(&acee_id) {
            acee.state = crate::AceeState::Deleted;
            acee.version = checked_version(acee.version)?;
        }
        records.push(record);
    }
    if records.is_empty() {
        return Err(SemanticProblem::NotFound);
    }
    Ok(records)
}

fn target(
    snapshot: &mut SecurityDatabaseSnapshot,
    context: &CommandContext,
    command: &ParsedCommand,
) -> Result<Vec<CommandRecord>, SemanticProblem> {
    require_special(snapshot, context)?;
    if let Some(operand) = command.operand("DELETE") {
        let node = bounded_upper(
            operand
                .first()
                .ok_or(SemanticProblem::Invalid(operand.offset))?,
            32,
            false,
        )?;
        if snapshot
            .user_associations
            .values()
            .any(|association| association.node == node)
        {
            return Err(SemanticProblem::Conflict);
        }
        if snapshot.rrsf_nodes.remove(&node).is_none() {
            return Err(SemanticProblem::NotFound);
        }
        return Ok(vec![CommandRecord::Name {
            kind: CommandObjectKind::Database,
            name: node,
        }]);
    }
    if let Some(operand) = command.operand("NODE") {
        let node = bounded_upper(
            operand
                .first()
                .ok_or(SemanticProblem::Invalid(operand.offset))?,
            32,
            false,
        )?;
        let current_version = snapshot.rrsf_nodes.get(&node).map(|node| node.version);
        let record = RrsfNode {
            name: node.clone(),
            description: command
                .operand("DESCRIPTION")
                .and_then(ParsedOperand::first)
                .map(str::to_string),
            protocol: command
                .operand("PROTOCOL")
                .and_then(ParsedOperand::first)
                .map(str::to_ascii_uppercase),
            prefix: command
                .operand("PREFIX")
                .and_then(ParsedOperand::first)
                .map(str::to_ascii_uppercase),
            workspace_limit: command
                .operand("WORKSPACE")
                .and_then(ParsedOperand::first)
                .map_or(Ok(0), |value| {
                    value
                        .parse()
                        .map_err(|_| SemanticProblem::Invalid(operand.offset))
                })?,
            state: if command.has_operand("DORMANT") {
                RrsfNodeState::Dormant
            } else {
                RrsfNodeState::Operative
            },
            version: current_version.map_or(Ok(1), checked_version)?,
        };
        snapshot.rrsf_nodes.insert(node, record.clone());
        return Ok(vec![node_record(&record)]);
    }
    if command.has_operand("LIST") {
        return Ok(snapshot.rrsf_nodes.values().map(node_record).collect());
    }
    Err(SemanticProblem::Invalid(0))
}

fn display(
    snapshot: &SecurityDatabaseSnapshot,
    context: &CommandContext,
    command: &ParsedCommand,
) -> Result<Vec<CommandRecord>, SemanticProblem> {
    if command.has_operand("SIGNON") || command.has_operand("USER") {
        let users = operand_names(command, "USER", 8)?;
        let special = is_auditor_or_special(snapshot, context);
        let records = snapshot
            .signon_sessions
            .values()
            .filter(|session| {
                (special && (users.is_empty() || users.contains(&session.user))
                    || session.user == context.actor().as_str())
                    && session.state == SignonSessionState::Active
            })
            .map(session_record)
            .collect();
        return Ok(records);
    }
    Ok(vec![CommandRecord::Summary {
        users: snapshot.principals.len(),
        groups: snapshot.groups.len(),
        profiles: snapshot.profiles.len(),
        generation: snapshot.generation,
    }])
}

fn racpriv(
    snapshot: &mut SecurityDatabaseSnapshot,
    context: &CommandContext,
    command: &ParsedCommand,
) -> Result<Vec<CommandRecord>, SemanticProblem> {
    require_special(snapshot, context)?;
    // The publication gives one operand: `RACPRIV [WRITEDOWN [(ACTIVE | INACTIVE | RESET)]]`.
    // WRITEDOWN without a value, and RACPRIV without any keyword, list the current mode.
    if let Some(operand) = command.operand("WRITEDOWN") {
        let values = operand.values().collect::<Vec<_>>();
        match values.as_slice() {
            [] => {}
            ["ACTIVE"] => snapshot.policy.write_down = true,
            ["INACTIVE"] => snapshot.policy.write_down = false,
            // "Reset to the user's installation defined default." This model carries exactly one
            // such default, the initial PolicyState value.
            ["RESET"] => {
                snapshot.policy.write_down = SecurityPolicyOptions::default().write_down;
            }
            _ => return Err(SemanticProblem::Invalid(operand.offset)),
        }
    }
    Ok(vec![policy_record(snapshot)])
}

fn restart(
    snapshot: &mut SecurityDatabaseSnapshot,
    context: &CommandContext,
) -> Result<Vec<CommandRecord>, SemanticProblem> {
    require_special(snapshot, context)?;
    snapshot.subsystem.running = true;
    snapshot.subsystem.restart_generation = checked_version(snapshot.subsystem.restart_generation)?;
    Ok(vec![policy_record(snapshot)])
}

fn stop(
    snapshot: &mut SecurityDatabaseSnapshot,
    context: &CommandContext,
) -> Result<Vec<CommandRecord>, SemanticProblem> {
    require_special(snapshot, context)?;
    snapshot.subsystem.running = false;
    Ok(vec![policy_record(snapshot)])
}

fn rvary(
    snapshot: &mut SecurityDatabaseSnapshot,
    context: &CommandContext,
    command: &ParsedCommand,
) -> Result<Vec<CommandRecord>, SemanticProblem> {
    require_special(snapshot, context)?;
    let actions = [
        "ACTIVE",
        "INACTIVE",
        "SWITCH",
        "DATASHARE",
        "NODATASHARE",
        "LIST",
    ]
    .into_iter()
    .filter(|name| command.has_operand(name))
    .count();
    if actions != 1 {
        return Err(SemanticProblem::Invalid(0));
    }
    if command.has_operand("ACTIVE") {
        snapshot.database_status.active = true;
    } else if command.has_operand("INACTIVE") {
        snapshot.database_status.active = false;
    } else if command.has_operand("DATASHARE") {
        snapshot.database_status.sharing_mode = DatabaseSharingMode::DataSharing;
    } else if command.has_operand("NODATASHARE") {
        snapshot.database_status.sharing_mode = DatabaseSharingMode::NonDataSharing;
    } else if command.has_operand("SWITCH") {
        let dataset = operand_name(command, "DATASET", 246)?.ok_or(SemanticProblem::Invalid(0))?;
        snapshot.database_status.backup_dataset = snapshot.database_status.primary_dataset.take();
        snapshot.database_status.primary_dataset = Some(dataset);
        snapshot.database_status.switch_generation =
            checked_version(snapshot.database_status.switch_generation)?;
    }
    Ok(vec![policy_record(snapshot)])
}

fn set_operational_options(
    snapshot: &mut SecurityDatabaseSnapshot,
    context: &CommandContext,
    command: &ParsedCommand,
) -> Result<Vec<CommandRecord>, SemanticProblem> {
    require_special(snapshot, context)?;
    if command.operands.is_empty() {
        return Err(SemanticProblem::Invalid(0));
    }
    for (positive, negative, key) in [
        ("AUTOAPPL", "NOAUTOAPPL", "AUTOAPPL"),
        ("AUTODIRECT", "NOAUTODIRECT", "AUTODIRECT"),
        ("AUTOPWD", "NOAUTOPWD", "AUTOPWD"),
        ("AUTOSIGNON", "NOAUTOSIGNON", "AUTOSIGNON"),
    ] {
        if command.has_operand(positive) && command.has_operand(negative) {
            return Err(SemanticProblem::Conflict);
        }
        if command.has_operand(positive) {
            snapshot.policy.set_flags.insert(key.into(), true);
        }
        if command.has_operand(negative) {
            snapshot.policy.set_flags.insert(key.into(), false);
        }
    }
    if command.has_operand("TRACE") && command.has_operand("NOTRACE") {
        return Err(SemanticProblem::Conflict);
    }
    if command.has_operand("TRACE") {
        snapshot.subsystem.trace = true;
    }
    if command.has_operand("NOTRACE") {
        snapshot.subsystem.trace = false;
    }
    Ok(vec![policy_record(snapshot)])
}

fn setropts(
    snapshot: &mut SecurityDatabaseSnapshot,
    context: &CommandContext,
    command: &ParsedCommand,
) -> Result<Vec<CommandRecord>, SemanticProblem> {
    require_special(snapshot, context)?;
    if command.operands.is_empty() {
        return Err(SemanticProblem::Invalid(0));
    }
    for (positive, negative, target) in [
        ("ADDCREATOR", "NOADDCREATOR", "add_creator"),
        ("CMDVIOL", "NOCMDVIOL", "command_violations_audited"),
        ("JESBATCHALLRACF", "NOJESBATCHALLRACF", "jes_batch_all_racf"),
        ("MLACTIVE", "NOMLACTIVE", "ml_active"),
        ("PROGRAM", "NOPROGRAM", "program_control"),
        ("RULES", "NORULES", "rules"),
        ("SECLEVELAUDIT", "NOSECLEVELAUDIT", "security_level_audit"),
        ("SECLABELAUDIT", "NOSECLABELAUDIT", "security_label_audit"),
        ("WHENPROGRAM", "NOWHENPROGRAM", "when_program"),
    ] {
        if command.has_operand(positive) && command.has_operand(negative) {
            return Err(SemanticProblem::Conflict);
        }
        if command.has_operand(positive) {
            set_policy_boolean(snapshot, target, true);
        }
        if command.has_operand(negative) {
            set_policy_boolean(snapshot, target, false);
        }
    }

    for class in operand_names(command, "CLASSACT", 32)? {
        ensure_resource_class(snapshot, &class)?;
        let descriptor = snapshot
            .classes
            .get_mut(&class)
            .ok_or(SemanticProblem::NotFound)?;
        descriptor.active = true;
        descriptor.version = checked_version(descriptor.version)?;
    }
    for class in operand_names(command, "NOCLASSACT", 32)?
        .into_iter()
        .chain(operand_names(command, "INACTIVE", 32)?)
    {
        ensure_resource_class(snapshot, &class)?;
        let descriptor = snapshot
            .classes
            .get_mut(&class)
            .ok_or(SemanticProblem::NotFound)?;
        descriptor.active = false;
        descriptor.version = checked_version(descriptor.version)?;
        snapshot.raclist_caches.remove(&class);
    }
    for class in operand_names(command, "GENERIC", 32)? {
        ensure_resource_class(snapshot, &class)?;
        let descriptor = snapshot
            .classes
            .get_mut(&class)
            .ok_or(SemanticProblem::NotFound)?;
        if !descriptor.generic_allowed {
            return Err(SemanticProblem::Invalid(0));
        }
        descriptor.generic_active = true;
        descriptor.version = checked_version(descriptor.version)?;
    }
    for class in operand_names(command, "NOGENERIC", 32)? {
        ensure_resource_class(snapshot, &class)?;
        let descriptor = snapshot
            .classes
            .get_mut(&class)
            .ok_or(SemanticProblem::NotFound)?;
        descriptor.generic_active = false;
        descriptor.version = checked_version(descriptor.version)?;
    }
    let raclist = operand_names(command, "RACLIST", 32)?;
    for class in &raclist {
        ensure_resource_class(snapshot, class)?;
        snapshot
            .classes
            .get_mut(class)
            .ok_or(SemanticProblem::NotFound)?
            .raclist = true;
        refresh_raclist(snapshot, class)?;
    }
    for class in operand_names(command, "NORACLIST", 32)? {
        ensure_resource_class(snapshot, &class)?;
        let descriptor = snapshot
            .classes
            .get_mut(&class)
            .ok_or(SemanticProblem::NotFound)?;
        descriptor.raclist = false;
        descriptor.version = checked_version(descriptor.version)?;
        snapshot.raclist_caches.remove(&class);
    }
    if command.has_operand("REFRESH") && raclist.is_empty() {
        return Err(SemanticProblem::Invalid(0));
    }
    if let Some(operand) = command.operand("PASSWORD") {
        if let Some(value) = inner_value(operand, "MINIMUM") {
            snapshot.policy.password_minimum = value
                .parse()
                .map_err(|_| SemanticProblem::Invalid(operand.offset))?;
        }
        if let Some(value) = inner_value(operand, "MAXIMUM") {
            snapshot.policy.password_maximum = value
                .parse()
                .map_err(|_| SemanticProblem::Invalid(operand.offset))?;
        }
        if let Some(value) = inner_value(operand, "HISTORY") {
            snapshot.policy.password_history = value
                .parse()
                .map_err(|_| SemanticProblem::Invalid(operand.offset))?;
        }
    }
    if let Some(operand) = command.operand("PHRASE")
        && let Some(value) = inner_value(operand, "MINIMUM")
    {
        snapshot.policy.phrase_minimum = value
            .parse()
            .map_err(|_| SemanticProblem::Invalid(operand.offset))?;
    }
    if snapshot.policy.password_minimum == 0
        || snapshot.policy.password_minimum > snapshot.policy.password_maximum
        || snapshot.policy.phrase_minimum < snapshot.policy.password_minimum
        || snapshot.policy.phrase_minimum > snapshot.policy.password_maximum
        || snapshot.policy.password_history > 128
    {
        return Err(SemanticProblem::Invalid(0));
    }
    for credential in snapshot
        .principals
        .values_mut()
        .filter_map(|principal| principal.credential.as_mut())
    {
        trim_credential_history(
            &mut credential.history_digests,
            &mut credential.history_verifiers,
            snapshot.policy.password_history,
        );
    }
    let consumed = [
        "ADDCREATOR",
        "NOADDCREATOR",
        "CMDVIOL",
        "NOCMDVIOL",
        "JESBATCHALLRACF",
        "NOJESBATCHALLRACF",
        "MLACTIVE",
        "NOMLACTIVE",
        "PROGRAM",
        "NOPROGRAM",
        "RULES",
        "NORULES",
        "SECLEVELAUDIT",
        "NOSECLEVELAUDIT",
        "SECLABELAUDIT",
        "NOSECLABELAUDIT",
        "WHENPROGRAM",
        "NOWHENPROGRAM",
        "CLASSACT",
        "NOCLASSACT",
        "INACTIVE",
        "GENERIC",
        "NOGENERIC",
        "RACLIST",
        "NORACLIST",
        "REFRESH",
        "LIST",
    ];
    for operand in &command.operands {
        if !consumed.contains(&operand.name.as_str()) {
            snapshot
                .policy
                .values
                .insert(operand.name.clone(), operand_text(operand));
        }
    }
    let mut records = vec![policy_record(snapshot)];
    let requested_classes = [
        "CLASSACT",
        "NOCLASSACT",
        "INACTIVE",
        "GENERIC",
        "NOGENERIC",
        "RACLIST",
        "NORACLIST",
    ]
    .into_iter()
    .map(|name| operand_names(command, name, 32))
    .collect::<Result<Vec<_>, _>>()?
    .into_iter()
    .flatten()
    .collect::<BTreeSet<_>>();
    for class in requested_classes {
        records.push(class_record(snapshot, &class)?);
    }
    Ok(records)
}

fn validate_parmlib_members(
    command: &ParsedCommand,
) -> Result<Vec<CommandRecord>, SemanticProblem> {
    let members = operand_names(command, "MEMBER", 8)?;
    if members.is_empty() || members.len() > 3 {
        return Err(SemanticProblem::Invalid(0));
    }
    Ok(members
        .into_iter()
        .map(|name| CommandRecord::Name {
            kind: CommandObjectKind::Database,
            name,
        })
        .collect())
}

fn refresh_raclist(
    snapshot: &mut SecurityDatabaseSnapshot,
    class: &str,
) -> Result<(), SemanticProblem> {
    let profiles = snapshot
        .profiles
        .iter()
        .filter(|(_, profile)| profile.class == class)
        .map(|(key, profile)| (key.clone(), profile.clone()))
        .collect::<BTreeMap<_, _>>();
    let version = snapshot
        .raclist_caches
        .get(class)
        .map_or(Ok(1), |cache| checked_version(cache.version))?;
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

fn set_policy_boolean(snapshot: &mut SecurityDatabaseSnapshot, target: &str, value: bool) {
    match target {
        "add_creator" => snapshot.policy.add_creator = value,
        "command_violations_audited" => snapshot.policy.command_violations_audited = value,
        "jes_batch_all_racf" => snapshot.policy.jes_batch_all_racf = value,
        "ml_active" => snapshot.policy.ml_active = value,
        "program_control" => snapshot.policy.program_control = value,
        "rules" => snapshot.policy.rules = value,
        "security_level_audit" => snapshot.policy.security_level_audit = value,
        "security_label_audit" => snapshot.policy.security_label_audit = value,
        "when_program" => snapshot.policy.when_program = value,
        _ => {}
    }
}

fn policy_record(snapshot: &SecurityDatabaseSnapshot) -> CommandRecord {
    CommandRecord::Policy {
        add_creator: snapshot.policy.add_creator,
        program_control: snapshot.policy.program_control,
        ml_active: snapshot.policy.ml_active,
        rules: snapshot.policy.rules,
        write_down: snapshot.policy.write_down,
        subsystem_running: snapshot.subsystem.running,
        database_active: snapshot.database_status.active,
        database_sharing: snapshot.database_status.sharing_mode,
    }
}

fn class_record(
    snapshot: &SecurityDatabaseSnapshot,
    class: &str,
) -> Result<CommandRecord, SemanticProblem> {
    let descriptor = snapshot
        .classes
        .get(class)
        .ok_or(SemanticProblem::NotFound)?;
    Ok(CommandRecord::Class {
        name: class.into(),
        supplied: descriptor.supplied,
        active: descriptor.active,
        generic_active: descriptor.generic_active,
        raclist: descriptor.raclist,
        cache_generation: snapshot
            .raclist_caches
            .get(class)
            .map(|cache| cache.built_generation),
        version: descriptor.version,
    })
}

fn add_group(
    snapshot: &mut SecurityDatabaseSnapshot,
    context: &CommandContext,
    command: &ParsedCommand,
) -> Result<Vec<CommandRecord>, SemanticProblem> {
    require_special(snapshot, context)?;
    let name = upper_principal(command.positional(0).ok_or(SemanticProblem::Invalid(0))?)?;
    if snapshot.groups.contains_key(&name) {
        return Err(SemanticProblem::Conflict);
    }
    let owner =
        operand_principal(command, "OWNER")?.unwrap_or_else(|| context.actor().as_str().into());
    require_principal_or_group(snapshot, &owner)?;
    let superior_group = operand_principal(command, "SUPGROUP")?;
    if let Some(superior) = &superior_group {
        require_group(snapshot, superior)?;
    }
    let mut group = GroupProfile {
        name: name.clone(),
        owner,
        superior_group,
        universal: command.has_operand("UNIVERSAL"),
        profile_template: None,
        segments: BTreeMap::new(),
        version: 1,
    };
    capture_profile_operands(
        snapshot,
        PrincipalKind::Group,
        &mut group.profile_template,
        &mut group.segments,
        command,
        &["OWNER", "SUPGROUP", "UNIVERSAL"],
    )?;
    snapshot.groups.insert(name.clone(), group);
    Ok(vec![CommandRecord::Name {
        kind: CommandObjectKind::Group,
        name,
    }])
}

fn add_user(
    service: &RacfService,
    snapshot: &mut SecurityDatabaseSnapshot,
    context: &CommandContext,
    command: &ParsedCommand,
) -> Result<Vec<CommandRecord>, SemanticProblem> {
    require_special(snapshot, context)?;
    let user = upper_principal(command.positional(0).ok_or(SemanticProblem::Invalid(0))?)?;
    if snapshot.principals.contains_key(&user) {
        return Err(SemanticProblem::Conflict);
    }
    let secret = secret_operand(command)?;
    if secret.is_some() && command.has_operand("NOPASSWORD")
        || command.has_operand("PASSWORD") && command.has_operand("PHRASE")
    {
        return Err(SemanticProblem::Invalid(secret_offset(command)));
    }
    let mut principal = PrincipalProfile {
        id: user.clone(),
        kind: PrincipalKind::User,
        owner: user.clone(),
        default_group: None,
        state: PrincipalState::Active,
        credential: secret
            .map(|secret| {
                replace_credential(
                    service,
                    snapshot,
                    &user,
                    None,
                    secret,
                    command.has_operand("PHRASE"),
                    secret_offset(command),
                    context.tick(),
                )
            })
            .transpose()?,
        profile_template: None,
        segments: BTreeMap::new(),
        security_level: 0,
        security_label: None,
        categories: BTreeSet::new(),
        attributes: BTreeSet::new(),
        version: 1,
    };
    principal.owner =
        operand_principal(command, "OWNER")?.unwrap_or_else(|| context.actor().as_str().into());
    require_principal_or_group(snapshot, &principal.owner)?;
    principal.default_group = operand_principal(command, "DFLTGRP")?;
    if let Some(group) = &principal.default_group {
        require_group(snapshot, group)?;
    }
    update_principal_flags(&mut principal, command);
    capture_profile_operands(
        snapshot,
        PrincipalKind::User,
        &mut principal.profile_template,
        &mut principal.segments,
        command,
        &principal_consumed_operands(),
    )?;
    snapshot.principals.insert(user.clone(), principal);
    apply_mfa(snapshot, &user, command, context.tick())?;
    if let Some(group) = snapshot.principals[&user].default_group.clone() {
        snapshot.connections.insert(
            connection_key(&user, &group),
            GroupConnection {
                user: user.clone(),
                group,
                authority: GroupAuthority::Use,
                special: false,
                operations: false,
                auditor: false,
                revoked: false,
                version: 1,
            },
        );
    }
    Ok(vec![CommandRecord::Name {
        kind: CommandObjectKind::User,
        name: user,
    }])
}

fn alter_group(
    snapshot: &mut SecurityDatabaseSnapshot,
    context: &CommandContext,
    command: &ParsedCommand,
) -> Result<Vec<CommandRecord>, SemanticProblem> {
    let name = upper_principal(command.positional(0).ok_or(SemanticProblem::Invalid(0))?)?;
    let current = snapshot
        .groups
        .get(&name)
        .cloned()
        .ok_or(SemanticProblem::NotFound)?;
    if !is_special(snapshot, context) && current.owner != context.actor().as_str() {
        return Err(SemanticProblem::Unauthorized);
    }
    let mut next = current;
    if let Some(owner) = operand_principal(command, "OWNER")? {
        require_principal_or_group(snapshot, &owner)?;
        next.owner = owner;
    }
    if let Some(superior) = operand_principal(command, "SUPGROUP")? {
        require_group(snapshot, &superior)?;
        if superior == name || group_descends_from(snapshot, &superior, &name) {
            return Err(SemanticProblem::Conflict);
        }
        next.superior_group = Some(superior);
    }
    if command.has_operand("UNIVERSAL") {
        next.universal = true;
    }
    if command.has_operand("NOUNIVERSAL") {
        next.universal = false;
    }
    capture_profile_operands(
        snapshot,
        PrincipalKind::Group,
        &mut next.profile_template,
        &mut next.segments,
        command,
        &["OWNER", "SUPGROUP", "UNIVERSAL", "NOUNIVERSAL"],
    )?;
    next.version = checked_version(next.version)?;
    snapshot.groups.insert(name.clone(), next);
    Ok(vec![CommandRecord::Name {
        kind: CommandObjectKind::Group,
        name,
    }])
}

fn alter_user(
    service: &RacfService,
    snapshot: &mut SecurityDatabaseSnapshot,
    context: &CommandContext,
    command: &ParsedCommand,
) -> Result<Vec<CommandRecord>, SemanticProblem> {
    let user = upper_principal(command.positional(0).ok_or(SemanticProblem::Invalid(0))?)?;
    let current = snapshot
        .principals
        .get(&user)
        .cloned()
        .ok_or(SemanticProblem::NotFound)?;
    let special = is_special(snapshot, context);
    let administrative = command
        .operands
        .iter()
        .any(|operand| !matches!(operand.name.as_str(), "NAME" | "LANGUAGE"));
    if administrative && !special {
        return Err(SemanticProblem::Unauthorized);
    }
    if !special && current.owner != context.actor().as_str() && user != context.actor().as_str() {
        return Err(SemanticProblem::Unauthorized);
    }
    let mut next = current;
    if let Some(owner) = operand_principal(command, "OWNER")? {
        require_principal_or_group(snapshot, &owner)?;
        next.owner = owner;
    }
    if let Some(group) = operand_principal(command, "DFLTGRP")? {
        require_group(snapshot, &group)?;
        if !snapshot
            .connections
            .get(&connection_key(&user, &group))
            .is_some_and(|connection| !connection.revoked)
        {
            return Err(SemanticProblem::Conflict);
        }
        next.default_group = Some(group);
    }
    if let Some(secret) = secret_operand(command)? {
        next.credential = Some(replace_credential(
            service,
            snapshot,
            &user,
            next.credential.as_ref(),
            secret,
            command.has_operand("PHRASE"),
            secret_offset(command),
            context.tick(),
        )?);
        next.state = PrincipalState::Active;
    }
    if command.has_operand("NOPASSWORD") {
        next.credential = None;
    }
    if command.has_operand("REVOKE") {
        next.state = PrincipalState::Revoked;
    }
    if command.has_operand("RESUME") {
        next.state = PrincipalState::Active;
    }
    update_principal_flags(&mut next, command);
    capture_profile_operands(
        snapshot,
        PrincipalKind::User,
        &mut next.profile_template,
        &mut next.segments,
        command,
        &principal_consumed_operands(),
    )?;
    next.version = checked_version(next.version)?;
    snapshot.principals.insert(user.clone(), next);
    apply_mfa(snapshot, &user, command, context.tick())?;
    Ok(vec![CommandRecord::Name {
        kind: CommandObjectKind::User,
        name: user,
    }])
}

fn connect(
    snapshot: &mut SecurityDatabaseSnapshot,
    context: &CommandContext,
    command: &ParsedCommand,
) -> Result<Vec<CommandRecord>, SemanticProblem> {
    let group = operand_principal(command, "GROUP")?.ok_or(SemanticProblem::Invalid(0))?;
    let group_profile = snapshot
        .groups
        .get(&group)
        .ok_or(SemanticProblem::NotFound)?;
    if !is_special(snapshot, context) && group_profile.owner != context.actor().as_str() {
        return Err(SemanticProblem::Unauthorized);
    }
    let authority = command
        .operand("AUTHORITY")
        .and_then(ParsedOperand::first)
        .map(parse_group_authority)
        .transpose()?
        .unwrap_or(GroupAuthority::Use);
    let mut records = Vec::new();
    for raw_user in &command.positionals {
        let user = upper_principal(raw_user)?;
        require_principal(snapshot, &user)?;
        let key = connection_key(&user, &group);
        let version = snapshot
            .connections
            .get(&key)
            .map_or(Ok(1), |current| checked_version(current.version))?;
        snapshot.connections.insert(
            key,
            GroupConnection {
                user: user.clone(),
                group: group.clone(),
                authority,
                special: command.has_operand("SPECIAL"),
                operations: command.has_operand("OPERATIONS"),
                auditor: command.has_operand("AUDITOR"),
                revoked: command.has_operand("REVOKE"),
                version,
            },
        );
        records.push(CommandRecord::Name {
            kind: CommandObjectKind::Connection,
            name: connection_key(&user, &group),
        });
    }
    Ok(records)
}

fn remove(
    snapshot: &mut SecurityDatabaseSnapshot,
    context: &CommandContext,
    command: &ParsedCommand,
) -> Result<Vec<CommandRecord>, SemanticProblem> {
    let group = operand_principal(command, "GROUP")?.ok_or(SemanticProblem::Invalid(0))?;
    let group_profile = snapshot
        .groups
        .get(&group)
        .ok_or(SemanticProblem::NotFound)?;
    if !is_special(snapshot, context) && group_profile.owner != context.actor().as_str() {
        return Err(SemanticProblem::Unauthorized);
    }
    let mut records = Vec::new();
    for raw_user in &command.positionals {
        let user = upper_principal(raw_user)?;
        let key = connection_key(&user, &group);
        if snapshot.connections.remove(&key).is_none() {
            return Err(SemanticProblem::NotFound);
        }
        if snapshot
            .principals
            .get(&user)
            .and_then(|principal| principal.default_group.as_ref())
            == Some(&group)
        {
            return Err(SemanticProblem::Conflict);
        }
        records.push(CommandRecord::Name {
            kind: CommandObjectKind::Connection,
            name: key,
        });
    }
    Ok(records)
}

fn delete_groups(
    snapshot: &mut SecurityDatabaseSnapshot,
    context: &CommandContext,
    command: &ParsedCommand,
) -> Result<Vec<CommandRecord>, SemanticProblem> {
    let mut records = Vec::new();
    for raw_group in &command.positionals {
        let group = upper_principal(raw_group)?;
        let profile = snapshot
            .groups
            .get(&group)
            .ok_or(SemanticProblem::NotFound)?;
        if !is_special(snapshot, context) && profile.owner != context.actor().as_str() {
            return Err(SemanticProblem::Unauthorized);
        }
        if snapshot
            .connections
            .values()
            .any(|value| value.group == group)
            || snapshot
                .groups
                .values()
                .any(|value| value.superior_group.as_deref() == Some(&group))
            || snapshot
                .principals
                .values()
                .any(|value| value.default_group.as_deref() == Some(&group))
            || snapshot.profiles.values().any(|value| value.owner == group)
        {
            return Err(SemanticProblem::Conflict);
        }
        snapshot.groups.remove(&group);
        records.push(CommandRecord::Name {
            kind: CommandObjectKind::Group,
            name: group,
        });
    }
    Ok(records)
}

fn delete_users(
    snapshot: &mut SecurityDatabaseSnapshot,
    context: &CommandContext,
    command: &ParsedCommand,
) -> Result<Vec<CommandRecord>, SemanticProblem> {
    require_special(snapshot, context)?;
    let mut records = Vec::new();
    for raw_user in &command.positionals {
        let user = upper_principal(raw_user)?;
        require_principal(snapshot, &user)?;
        if snapshot
            .profiles
            .values()
            .any(|profile| profile.owner == user)
            || snapshot.groups.values().any(|group| group.owner == user)
            || snapshot
                .acees
                .values()
                .any(|acee| acee.principal == user && acee.state == crate::AceeState::Active)
            || snapshot.tokens.values().any(|token| token.owner == user)
            || snapshot
                .certificates
                .values()
                .any(|certificate| certificate.owner == user)
            || snapshot.keys.values().any(|key| key.owner == user)
            || snapshot
                .keyrings
                .values()
                .any(|keyring| keyring.owner == user)
            || snapshot
                .mfa_factors
                .values()
                .any(|factor| factor.owner == user)
            || snapshot
                .identity_mappings
                .values()
                .any(|mapping| mapping.local_user == user)
            || snapshot
                .user_associations
                .values()
                .any(|association| association.local_user == user)
            || snapshot
                .signon_sessions
                .values()
                .any(|session| session.user == user && session.state == SignonSessionState::Active)
        {
            return Err(SemanticProblem::Conflict);
        }
        snapshot.principals.remove(&user);
        let closed_acees = snapshot
            .acees
            .values()
            .filter(|acee| acee.principal == user && acee.state != crate::AceeState::Active)
            .map(|acee| acee.id.clone())
            .collect::<BTreeSet<_>>();
        snapshot.acees.retain(|id, _| !closed_acees.contains(id));
        snapshot.signon_sessions.retain(|_, session| {
            session.user != user || session.state == SignonSessionState::Active
        });
        snapshot
            .connections
            .retain(|_, connection| connection.user != user);
        for profile in snapshot.profiles.values_mut() {
            profile.access_list.retain(|entry| entry.principal != user);
        }
        records.push(CommandRecord::Name {
            kind: CommandObjectKind::User,
            name: user,
        });
    }
    Ok(records)
}

fn define_resources(
    snapshot: &mut SecurityDatabaseSnapshot,
    context: &CommandContext,
    command: &ParsedCommand,
) -> Result<Vec<CommandRecord>, SemanticProblem> {
    let (class, names): (String, Vec<&str>) = if command.descriptor.family() == CommandFamily::AddSd
    {
        (
            "DATASET".into(),
            command
                .positionals
                .iter()
                .map(|value| value.as_str())
                .collect(),
        )
    } else {
        (
            upper_class(command.positional(0).ok_or(SemanticProblem::Invalid(0))?)?,
            command
                .positionals
                .iter()
                .skip(1)
                .map(|value| value.as_str())
                .collect(),
        )
    };
    ensure_resource_class(snapshot, &class)?;
    let owner =
        operand_principal(command, "OWNER")?.unwrap_or_else(|| context.actor().as_str().into());
    require_principal_or_group(snapshot, &owner)?;
    let uacc = operand_access(command, "UACC")?.unwrap_or(AccessLevel::None);
    let mut records = Vec::new();
    for raw_name in names {
        let name = upper_profile(raw_name)?;
        if !can_define(snapshot, context, &class, &name) {
            return Err(SemanticProblem::Unauthorized);
        }
        let key = profile_key(&class, &name);
        if snapshot.profiles.contains_key(&key) {
            return Err(SemanticProblem::Conflict);
        }
        let mut profile = ResourceProfile {
            class: class.clone(),
            name: name.clone(),
            generic: contains_generic(&name) || command.has_operand("GENERIC"),
            owner: owner.clone(),
            uacc,
            audit: parse_audit(command)?,
            security_level: operand_u32(command, "LEVEL")?.unwrap_or(0),
            security_label: operand_name(command, "SECLABEL", 246)?,
            categories: operand_names(command, "ADDCATEGORY", 246)?,
            access_list: snapshot
                .policy
                .add_creator
                .then(|| AccessControlEntry {
                    principal: context.actor().as_str().into(),
                    access: AccessLevel::Alter,
                    when: None,
                    audit: AuditPolicy::None,
                })
                .into_iter()
                .collect(),
            segments: BTreeMap::new(),
            version: 1,
        };
        capture_resource_operands(
            snapshot,
            &mut profile,
            command,
            &resource_consumed_operands(),
        )?;
        snapshot.profiles.insert(key, profile);
        records.push(CommandRecord::Name {
            kind: if class == "DATASET" {
                CommandObjectKind::DatasetProfile
            } else {
                CommandObjectKind::ResourceProfile
            },
            name,
        });
    }
    Ok(records)
}

fn alter_resources(
    snapshot: &mut SecurityDatabaseSnapshot,
    context: &CommandContext,
    command: &ParsedCommand,
) -> Result<Vec<CommandRecord>, SemanticProblem> {
    let (class, names): (String, Vec<&str>) = if command.descriptor.family() == CommandFamily::AltSd
    {
        (
            "DATASET".into(),
            command
                .positionals
                .iter()
                .map(|value| value.as_str())
                .collect(),
        )
    } else {
        (
            upper_class(command.positional(0).ok_or(SemanticProblem::Invalid(0))?)?,
            command
                .positionals
                .iter()
                .skip(1)
                .map(|value| value.as_str())
                .collect(),
        )
    };
    let mut records = Vec::new();
    for raw_name in names {
        let name = upper_profile(raw_name)?;
        let key = profile_key(&class, &name);
        let current = snapshot
            .profiles
            .get(&key)
            .cloned()
            .ok_or(SemanticProblem::NotFound)?;
        if !is_special(snapshot, context) && current.owner != context.actor().as_str() {
            return Err(SemanticProblem::Unauthorized);
        }
        let mut next = current;
        if let Some(owner) = operand_principal(command, "OWNER")? {
            require_principal_or_group(snapshot, &owner)?;
            next.owner = owner;
        }
        if let Some(uacc) = operand_access(command, "UACC")? {
            next.uacc = uacc;
        }
        if command.has_operand("AUDIT") || command.has_operand("NOAUDIT") {
            next.audit = parse_audit(command)?;
        }
        if let Some(level) = operand_u32(command, "LEVEL")? {
            next.security_level = level;
        }
        if command.has_operand("NOLEVEL") {
            next.security_level = 0;
        }
        if let Some(label) = operand_name(command, "SECLABEL", 246)? {
            next.security_label = Some(label);
        }
        if command.has_operand("NOSECLABEL") {
            next.security_label = None;
        }
        next.categories
            .extend(operand_names(command, "ADDCATEGORY", 246)?);
        if command.has_operand("NOADDCATEGORY") {
            next.categories.clear();
        }
        capture_resource_operands(snapshot, &mut next, command, &resource_consumed_operands())?;
        next.version = checked_version(next.version)?;
        snapshot.profiles.insert(key, next);
        records.push(CommandRecord::Name {
            kind: if class == "DATASET" {
                CommandObjectKind::DatasetProfile
            } else {
                CommandObjectKind::ResourceProfile
            },
            name,
        });
    }
    Ok(records)
}

fn delete_resources(
    snapshot: &mut SecurityDatabaseSnapshot,
    context: &CommandContext,
    command: &ParsedCommand,
) -> Result<Vec<CommandRecord>, SemanticProblem> {
    let (class, names): (String, Vec<&str>) = if command.descriptor.family() == CommandFamily::DelSd
    {
        (
            "DATASET".into(),
            command
                .positionals
                .iter()
                .map(|value| value.as_str())
                .collect(),
        )
    } else {
        (
            upper_class(command.positional(0).ok_or(SemanticProblem::Invalid(0))?)?,
            command
                .positionals
                .iter()
                .skip(1)
                .map(|value| value.as_str())
                .collect(),
        )
    };
    let mut records = Vec::new();
    for raw_name in names {
        let name = upper_profile(raw_name)?;
        let key = profile_key(&class, &name);
        let profile = snapshot
            .profiles
            .get(&key)
            .ok_or(SemanticProblem::NotFound)?;
        if !is_special(snapshot, context) && profile.owner != context.actor().as_str() {
            return Err(SemanticProblem::Unauthorized);
        }
        snapshot.profiles.remove(&key);
        records.push(CommandRecord::Name {
            kind: if class == "DATASET" {
                CommandObjectKind::DatasetProfile
            } else {
                CommandObjectKind::ResourceProfile
            },
            name,
        });
    }
    Ok(records)
}

fn permit(
    snapshot: &mut SecurityDatabaseSnapshot,
    context: &CommandContext,
    command: &ParsedCommand,
) -> Result<Vec<CommandRecord>, SemanticProblem> {
    let class = operand_name(command, "CLASS", 32)?.unwrap_or_else(|| "DATASET".into());
    let class = upper_class(&class)?;
    let ids = operand_names(command, "ID", 8)?;
    if ids.is_empty() && !command.has_operand("RESET") {
        return Err(SemanticProblem::Invalid(0));
    }
    for id in &ids {
        require_principal_or_group(snapshot, id)?;
    }
    let access = operand_access(command, "ACCESS")?;
    if access.is_none() && !command.has_operand("DELETE") && !command.has_operand("RESET") {
        return Err(SemanticProblem::Invalid(0));
    }
    let when = parse_access_condition(command)?;
    let mut records = Vec::new();
    for raw_name in &command.positionals {
        let name = upper_profile(raw_name)?;
        let key = profile_key(&class, &name);
        let current = snapshot
            .profiles
            .get(&key)
            .cloned()
            .ok_or(SemanticProblem::NotFound)?;
        if !is_special(snapshot, context) && current.owner != context.actor().as_str() {
            return Err(SemanticProblem::Unauthorized);
        }
        let mut next = current;
        if command.has_operand("RESET") {
            next.access_list.clear();
        }
        for id in &ids {
            next.access_list
                .retain(|entry| entry.principal != *id || entry.when != when);
            if !command.has_operand("DELETE") {
                next.access_list.push(AccessControlEntry {
                    principal: id.clone(),
                    access: access.expect("validated access"),
                    when: when.clone(),
                    audit: AuditPolicy::None,
                });
            }
        }
        next.access_list.sort_by(|left, right| {
            (&left.principal, &left.when).cmp(&(&right.principal, &right.when))
        });
        next.version = checked_version(next.version)?;
        snapshot.profiles.insert(key, next);
        records.push(CommandRecord::Name {
            kind: if class == "DATASET" {
                CommandObjectKind::DatasetProfile
            } else {
                CommandObjectKind::ResourceProfile
            },
            name,
        });
    }
    Ok(records)
}

fn list_users(
    snapshot: &SecurityDatabaseSnapshot,
    context: &CommandContext,
    command: &ParsedCommand,
) -> Result<Vec<CommandRecord>, SemanticProblem> {
    let names = if command.positionals.is_empty() {
        vec![context.actor().as_str().to_string()]
    } else {
        command
            .positionals
            .iter()
            .map(|name| upper_principal(name))
            .collect::<Result<Vec<_>, _>>()?
    };
    if names.iter().any(|name| name != context.actor().as_str())
        && !is_auditor_or_special(snapshot, context)
    {
        return Err(SemanticProblem::Unauthorized);
    }
    names
        .into_iter()
        .map(|name| {
            let user = snapshot
                .principals
                .get(&name)
                .ok_or(SemanticProblem::NotFound)?;
            Ok(CommandRecord::User {
                id: user.id.clone(),
                owner: user.owner.clone(),
                default_group: user.default_group.clone(),
                state: user.state,
                attributes: user.attributes.clone(),
                groups: snapshot
                    .connections
                    .values()
                    .filter(|connection| connection.user == user.id && !connection.revoked)
                    .map(|connection| connection.group.clone())
                    .collect(),
                segments: user.segments.keys().cloned().collect(),
                version: user.version,
            })
        })
        .collect()
}

fn list_groups(
    snapshot: &SecurityDatabaseSnapshot,
    command: &ParsedCommand,
) -> Result<Vec<CommandRecord>, SemanticProblem> {
    let names = if command.positionals.is_empty() {
        snapshot.groups.keys().cloned().collect()
    } else {
        command
            .positionals
            .iter()
            .map(|name| upper_principal(name))
            .collect::<Result<Vec<_>, _>>()?
    };
    names
        .into_iter()
        .map(|name| {
            let group = snapshot
                .groups
                .get(&name)
                .ok_or(SemanticProblem::NotFound)?;
            Ok(CommandRecord::Group {
                name: group.name.clone(),
                owner: group.owner.clone(),
                superior_group: group.superior_group.clone(),
                universal: group.universal,
                members: snapshot
                    .connections
                    .values()
                    .filter(|connection| connection.group == group.name && !connection.revoked)
                    .map(|connection| connection.user.clone())
                    .collect(),
                segments: group.segments.keys().cloned().collect(),
                version: group.version,
            })
        })
        .collect()
}

fn list_profiles(
    snapshot: &SecurityDatabaseSnapshot,
    class: &str,
    command: &ParsedCommand,
    skip: usize,
) -> Result<Vec<CommandRecord>, SemanticProblem> {
    let requested = command
        .positionals
        .iter()
        .skip(skip)
        .map(|name| upper_profile(name))
        .collect::<Result<BTreeSet<_>, _>>()?;
    let mut records = Vec::new();
    for profile in snapshot.profiles.values().filter(|profile| {
        profile.class == class && (requested.is_empty() || requested.contains(&profile.name))
    }) {
        records.push(profile_record(profile));
    }
    if !requested.is_empty() && records.len() != requested.len() {
        return Err(SemanticProblem::NotFound);
    }
    Ok(records)
}

fn search(
    snapshot: &SecurityDatabaseSnapshot,
    command: &ParsedCommand,
) -> Result<Vec<CommandRecord>, SemanticProblem> {
    let class = operand_name(command, "CLASS", 32)?
        .map(|class| upper_class(&class))
        .transpose()?;
    let mask = operand_name(command, "MASK", 246)?.map(|value| value.to_ascii_uppercase());
    let mut records = Vec::new();
    if class.as_deref() == Some("USER") {
        for name in snapshot
            .principals
            .keys()
            .filter(|name| mask_match(name, mask.as_deref()))
        {
            records.push(CommandRecord::Name {
                kind: CommandObjectKind::User,
                name: name.clone(),
            });
        }
    } else if class.as_deref() == Some("GROUP") {
        for name in snapshot
            .groups
            .keys()
            .filter(|name| mask_match(name, mask.as_deref()))
        {
            records.push(CommandRecord::Name {
                kind: CommandObjectKind::Group,
                name: name.clone(),
            });
        }
    } else {
        for profile in snapshot.profiles.values().filter(|profile| {
            class.as_ref().is_none_or(|class| class == &profile.class)
                && mask_match(&profile.name, mask.as_deref())
        }) {
            records.push(profile_record(profile));
        }
    }
    Ok(records)
}

fn capture_profile_operands(
    snapshot: &mut SecurityDatabaseSnapshot,
    kind: PrincipalKind,
    profile_template: &mut Option<String>,
    segments: &mut BTreeMap<String, ProfileSegment>,
    command: &ParsedCommand,
    consumed: &[&str],
) -> Result<(), SemanticProblem> {
    let values = command
        .operands
        .iter()
        .filter(|operand| {
            !consumed.contains(&operand.name.as_str())
                && !matches!(operand.name.as_str(), "AT" | "ONLYAT")
        })
        .map(|operand| (operand.name.clone(), operand_text(operand)))
        .collect::<Vec<_>>();
    if values.is_empty() {
        return Ok(());
    }
    let template_name = match kind {
        PrincipalKind::User => "USER",
        PrincipalKind::Group => "GROUP",
        _ => return Err(SemanticProblem::Invalid(0)),
    };
    ensure_base_template(snapshot, template_name, kind, &values)?;
    *profile_template = Some(template_name.into());
    let segment = segments
        .entry("BASE".into())
        .or_insert_with(|| ProfileSegment {
            template: "BASE".into(),
            template_version: 1,
            fields: BTreeMap::new(),
        });
    for (name, value) in values {
        segment.fields.insert(name, SegmentValue::Text(value));
    }
    Ok(())
}

fn capture_resource_operands(
    snapshot: &mut SecurityDatabaseSnapshot,
    profile: &mut ResourceProfile,
    command: &ParsedCommand,
    consumed: &[&str],
) -> Result<(), SemanticProblem> {
    let values = command
        .operands
        .iter()
        .filter(|operand| {
            !consumed.contains(&operand.name.as_str())
                && !matches!(operand.name.as_str(), "AT" | "ONLYAT")
        })
        .map(|operand| (operand.name.clone(), operand_text(operand)))
        .collect::<Vec<_>>();
    if values.is_empty() {
        return Ok(());
    }
    ensure_base_template(snapshot, "RESOURCE", PrincipalKind::Undefined, &values)?;
    let segment = profile
        .segments
        .entry("BASE".into())
        .or_insert_with(|| ProfileSegment {
            template: "BASE".into(),
            template_version: 1,
            fields: BTreeMap::new(),
        });
    for (name, value) in values {
        segment.fields.insert(name, SegmentValue::Text(value));
    }
    Ok(())
}

fn ensure_base_template(
    snapshot: &mut SecurityDatabaseSnapshot,
    name: &str,
    kind: PrincipalKind,
    values: &[(String, String)],
) -> Result<(), SemanticProblem> {
    let template = snapshot
        .templates
        .entry(name.into())
        .or_insert_with(|| ProfileTemplate {
            id: name.into(),
            version: 1,
            profile_kind: kind,
            required_segments: BTreeSet::new(),
            segments: BTreeMap::from([(
                "BASE".into(),
                SegmentTemplate {
                    name: "BASE".into(),
                    version: 1,
                    fields: BTreeMap::new(),
                },
            )]),
        });
    if template.profile_kind != kind {
        return Err(SemanticProblem::Conflict);
    }
    let base = template
        .segments
        .get_mut("BASE")
        .ok_or(SemanticProblem::Conflict)?;
    for (field, _) in values {
        base.fields
            .entry(field.clone())
            .or_insert(SegmentFieldSchema {
                kind: SegmentFieldKind::Text,
                required: false,
                max_bytes: 4096,
                max_items: 0,
            });
    }
    Ok(())
}

fn ensure_resource_class(
    snapshot: &mut SecurityDatabaseSnapshot,
    class: &str,
) -> Result<(), SemanticProblem> {
    ensure_base_template(snapshot, "RESOURCE", PrincipalKind::Undefined, &[])?;
    snapshot.classes.entry(class.into()).or_insert_with(|| {
        let supplied = crate::supplied_class_descriptors()
            .iter()
            .find(|descriptor| descriptor.name == class);
        ClassDescriptor {
            name: class.into(),
            supplied: supplied.is_some(),
            active: supplied.is_none_or(|descriptor| descriptor.active),
            generic_allowed: supplied.is_none_or(|descriptor| descriptor.generic_allowed),
            generic_active: supplied.is_none_or(|descriptor| descriptor.generic_active),
            discrete_allowed: supplied.is_none_or(|descriptor| descriptor.discrete_allowed),
            raclist: supplied.is_some_and(|descriptor| descriptor.raclist),
            default_uacc: AccessLevel::None,
            max_profile_name_bytes: supplied
                .map_or(246, |descriptor| descriptor.max_profile_name_bytes),
            posit: supplied.and_then(|descriptor| descriptor.posit),
            member_class: None,
            grouping_class: None,
            profile_template: "RESOURCE".into(),
            version: 1,
        }
    });
    Ok(())
}

fn append_transaction(
    snapshot: &mut SecurityDatabaseSnapshot,
    context: &CommandContext,
    command: &ParsedCommand,
    request_digest: &str,
    state: TransactionState,
    status: SafStatus,
) -> Result<(), HostProblem> {
    if snapshot.transactions.len() >= 65_536 {
        return Err(HostProblem::ResourceExhausted);
    }
    snapshot.transactions.insert(
        context.idempotency_key().into(),
        SecurityTransaction {
            id: context.idempotency_key().into(),
            idempotency_key: context.idempotency_key().into(),
            actor: context.actor().as_str().into(),
            operation: command.descriptor.keyword().into(),
            request_digest_format: SecurityRequestDigestFormat::RacfCommandCanonicalV1,
            request_digest: request_digest.into(),
            state,
            base_generation: snapshot.generation,
            final_generation: Some(
                snapshot
                    .generation
                    .checked_add(1)
                    .ok_or(HostProblem::ResourceExhausted)?,
            ),
            status,
            terminal_result: None,
        },
    );
    Ok(())
}

fn append_audit(
    snapshot: &mut SecurityDatabaseSnapshot,
    context: &CommandContext,
    command: &ParsedCommand,
    decision: DecisionOutcome,
    status: SafStatus,
    request_digest: &str,
) -> Result<(), HostProblem> {
    if snapshot.audits.len() >= 65_536 {
        return Err(HostProblem::ResourceExhausted);
    }
    let id = format!(
        "AUDIT{:020}{:06}",
        snapshot.generation,
        snapshot.audits.len()
    );
    snapshot.audits.push(SecurityAuditRecord {
        id,
        correlation: context.correlation().into(),
        actor: context.actor().as_str().into(),
        action: command.descriptor.keyword().into(),
        class: None,
        resource_digest: Some(request_digest.into()),
        decision,
        status,
        fields: crate::audit::redact_fields(BTreeMap::from([
            (
                "COMMAND_FAMILY".into(),
                AuditFieldValue::Text(command.descriptor.keyword().into()),
            ),
            (
                "OFFICIAL_ROW".into(),
                AuditFieldValue::Text(command.descriptor.row_id().into()),
            ),
            (
                "REQUEST_DIGEST_FORMAT".into(),
                AuditFieldValue::Text(
                    SecurityRequestDigestFormat::RacfCommandCanonicalV1
                        .as_str()
                        .into(),
                ),
            ),
        ])),
        tick: context.tick(),
    });
    Ok(())
}

fn require_active(
    snapshot: &SecurityDatabaseSnapshot,
    context: &CommandContext,
) -> Result<(), SemanticProblem> {
    let principal = snapshot
        .principals
        .get(context.actor().as_str())
        .ok_or(SemanticProblem::Unauthorized)?;
    if matches!(
        principal.state,
        PrincipalState::Active | PrincipalState::PasswordExpired
    ) {
        Ok(())
    } else {
        Err(SemanticProblem::Unauthorized)
    }
}

fn require_special(
    snapshot: &SecurityDatabaseSnapshot,
    context: &CommandContext,
) -> Result<(), SemanticProblem> {
    if is_special(snapshot, context) {
        Ok(())
    } else {
        Err(SemanticProblem::Unauthorized)
    }
}

fn is_special(snapshot: &SecurityDatabaseSnapshot, context: &CommandContext) -> bool {
    snapshot
        .principals
        .get(context.actor().as_str())
        .is_some_and(|principal| principal.attributes.contains("SPECIAL"))
}

fn is_auditor_or_special(snapshot: &SecurityDatabaseSnapshot, context: &CommandContext) -> bool {
    snapshot
        .principals
        .get(context.actor().as_str())
        .is_some_and(|principal| {
            principal.attributes.contains("SPECIAL") || principal.attributes.contains("AUDITOR")
        })
}

fn can_define(
    snapshot: &SecurityDatabaseSnapshot,
    context: &CommandContext,
    class: &str,
    name: &str,
) -> bool {
    is_special(snapshot, context)
        || snapshot
            .principals
            .get(context.actor().as_str())
            .is_some_and(|principal| principal.attributes.contains(&format!("CLAUTH:{class}")))
        || class == "DATASET"
            && name
                .split('.')
                .next()
                .is_some_and(|qualifier| qualifier == context.actor().as_str())
}

fn require_principal(
    snapshot: &SecurityDatabaseSnapshot,
    value: &str,
) -> Result<(), SemanticProblem> {
    if snapshot.principals.contains_key(value) {
        Ok(())
    } else {
        Err(SemanticProblem::NotFound)
    }
}

fn require_group(snapshot: &SecurityDatabaseSnapshot, value: &str) -> Result<(), SemanticProblem> {
    if snapshot.groups.contains_key(value) {
        Ok(())
    } else {
        Err(SemanticProblem::NotFound)
    }
}

fn require_principal_or_group(
    snapshot: &SecurityDatabaseSnapshot,
    value: &str,
) -> Result<(), SemanticProblem> {
    if snapshot.principals.contains_key(value) || snapshot.groups.contains_key(value) {
        Ok(())
    } else {
        Err(SemanticProblem::NotFound)
    }
}

fn group_descends_from(snapshot: &SecurityDatabaseSnapshot, group: &str, ancestor: &str) -> bool {
    let mut current = Some(group);
    let mut seen = BTreeSet::new();
    while let Some(name) = current {
        if name == ancestor {
            return true;
        }
        if !seen.insert(name) {
            return true;
        }
        current = snapshot
            .groups
            .get(name)
            .and_then(|profile| profile.superior_group.as_deref());
    }
    false
}

fn update_principal_flags(principal: &mut PrincipalProfile, command: &ParsedCommand) {
    for (operand, attribute, enabled) in [
        ("SPECIAL", "SPECIAL", true),
        ("NOSPECIAL", "SPECIAL", false),
        ("AUDITOR", "AUDITOR", true),
        ("NOAUDITOR", "AUDITOR", false),
        ("OPERATIONS", "OPERATIONS", true),
        ("NOOPERATIONS", "OPERATIONS", false),
        ("RESTRICTED", "RESTRICTED", true),
        ("NORESTRICTED", "RESTRICTED", false),
    ] {
        if command.has_operand(operand) {
            if enabled {
                principal.attributes.insert(attribute.into());
            } else {
                principal.attributes.remove(attribute);
            }
        }
    }
}

fn reject_unsupported_direction(command: &ParsedCommand) -> Result<(), SemanticProblem> {
    command
        .operands
        .iter()
        .find(|operand| {
            matches!(operand.name.as_str(), "AT" | "ONLYAT")
                && !command
                    .descriptor
                    .operands()
                    .contains(&operand.name.as_str())
        })
        .map_or(Ok(()), |operand| {
            Err(SemanticProblem::Invalid(operand.offset))
        })
}

fn parse_access_condition(
    command: &ParsedCommand,
) -> Result<Option<AccessCondition>, SemanticProblem> {
    let Some(operand) = command.operand("WHEN") else {
        return Ok(None);
    };
    let values = operand.values().collect::<Vec<_>>();
    if values.is_empty() {
        return Err(SemanticProblem::Invalid(operand.offset));
    }
    let mut condition = AccessCondition {
        terminal: None,
        console: None,
        system: None,
        application: None,
        start_tick: None,
        end_tick: None,
    };
    let mut index = 0usize;
    while index < values.len() {
        let marker = values[index].to_ascii_uppercase();
        index += 1;
        match marker.as_str() {
            "TERMINAL" | "CONSOLE" | "SYSTEM" | "APPLICATION" | "APPL" => {
                let value = values
                    .get(index)
                    .ok_or(SemanticProblem::Invalid(operand.offset))?;
                index += 1;
                let value = normalized_text(value, 246)?.to_ascii_uppercase();
                let target = match marker.as_str() {
                    "TERMINAL" => &mut condition.terminal,
                    "CONSOLE" => &mut condition.console,
                    "SYSTEM" => &mut condition.system,
                    "APPLICATION" | "APPL" => &mut condition.application,
                    _ => unreachable!(),
                };
                if target.replace(value).is_some() {
                    return Err(SemanticProblem::Invalid(operand.offset));
                }
            }
            "TIME" => {
                let start = values
                    .get(index)
                    .and_then(|value| value.parse::<u64>().ok())
                    .ok_or(SemanticProblem::Invalid(operand.offset))?;
                let end = values
                    .get(index + 1)
                    .and_then(|value| value.parse::<u64>().ok())
                    .ok_or(SemanticProblem::Invalid(operand.offset))?;
                if condition.start_tick.replace(start).is_some()
                    || condition.end_tick.replace(end).is_some()
                {
                    return Err(SemanticProblem::Invalid(operand.offset));
                }
                index += 2;
            }
            "START" | "END" => {
                let value = values
                    .get(index)
                    .and_then(|value| value.parse::<u64>().ok())
                    .ok_or(SemanticProblem::Invalid(operand.offset))?;
                index += 1;
                let target = if marker == "START" {
                    &mut condition.start_tick
                } else {
                    &mut condition.end_tick
                };
                if target.replace(value).is_some() {
                    return Err(SemanticProblem::Invalid(operand.offset));
                }
            }
            _ => return Err(SemanticProblem::Invalid(operand.offset)),
        }
    }
    if condition
        .start_tick
        .zip(condition.end_tick)
        .is_some_and(|(start, end)| start > end)
    {
        return Err(SemanticProblem::Invalid(operand.offset));
    }
    Ok(Some(condition))
}

fn apply_mfa(
    snapshot: &mut SecurityDatabaseSnapshot,
    owner: &str,
    command: &ParsedCommand,
    tick: u64,
) -> Result<(), SemanticProblem> {
    let Some(operand) = command.operand("MFA") else {
        return Ok(());
    };
    let id = inner_first_name(operand)?;
    if inner_flag(operand, "DELETE") || inner_flag(operand, "INACTIVE") {
        let factor = snapshot
            .mfa_factors
            .get(&id)
            .ok_or(SemanticProblem::NotFound)?;
        if factor.owner != owner {
            return Err(SemanticProblem::Unauthorized);
        }
        if inner_flag(operand, "DELETE") {
            snapshot.mfa_factors.remove(&id);
        } else {
            let factor = snapshot
                .mfa_factors
                .get_mut(&id)
                .ok_or(SemanticProblem::NotFound)?;
            factor.active = false;
            factor.version = checked_version(factor.version)?;
        }
        return Ok(());
    }
    let reference = inner_value(operand, "REF")
        .or_else(|| inner_value(operand, "REFERENCE"))
        .ok_or(SemanticProblem::Invalid(operand.offset))?;
    let kind = match inner_value(operand, "TYPE")
        .unwrap_or("TOTP")
        .to_ascii_uppercase()
        .as_str()
    {
        "TOTP" => MfaFactorKind::Totp,
        "WEBAUTHN" => MfaFactorKind::Webauthn,
        "PASSCODE" => MfaFactorKind::Passcode,
        "CUSTOM" => MfaFactorKind::Custom,
        _ => return Err(SemanticProblem::Invalid(operand.offset)),
    };
    let version = match snapshot.mfa_factors.get(&id) {
        Some(factor) if factor.owner != owner => return Err(SemanticProblem::Conflict),
        Some(factor) => checked_version(factor.version)?,
        None => 1,
    };
    snapshot.mfa_factors.insert(
        id.clone(),
        MfaFactor {
            id,
            owner: owner.into(),
            kind,
            secret_reference: normalized_reference(reference)?,
            active: !inner_flag(operand, "DORMANT"),
            created_tick: tick,
            version,
        },
    );
    Ok(())
}

fn principal_consumed_operands() -> Vec<&'static str> {
    vec![
        "OWNER",
        "DFLTGRP",
        "PASSWORD",
        "PHRASE",
        "NOPASSWORD",
        "SPECIAL",
        "NOSPECIAL",
        "AUDITOR",
        "NOAUDITOR",
        "OPERATIONS",
        "NOOPERATIONS",
        "RESTRICTED",
        "NORESTRICTED",
        "REVOKE",
        "RESUME",
        "MFA",
    ]
}

fn resource_consumed_operands() -> Vec<&'static str> {
    vec![
        "OWNER",
        "UACC",
        "AUDIT",
        "NOAUDIT",
        "LEVEL",
        "NOLEVEL",
        "SECLABEL",
        "NOSECLABEL",
        "ADDCATEGORY",
        "NOADDCATEGORY",
        "GENERIC",
    ]
}

fn secret_operand(command: &ParsedCommand) -> Result<Option<&str>, SemanticProblem> {
    let password = command
        .operand("PASSWORD")
        .or_else(|| command.operand("PHRASE"));
    password
        .map(|operand| {
            operand
                .first()
                .ok_or(SemanticProblem::Invalid(operand.offset))
        })
        .transpose()
}

fn secret_offset(command: &ParsedCommand) -> usize {
    command
        .operand("PASSWORD")
        .or_else(|| command.operand("PHRASE"))
        .map_or(0, |operand| operand.offset)
}

fn operand_text(operand: &ParsedOperand) -> String {
    if operand.values.is_empty() {
        "TRUE".into()
    } else {
        operand.values().collect::<Vec<_>>().join(" ")
    }
}

fn normalized_digest(value: &str) -> Result<String, SemanticProblem> {
    if value.len() == 71
        && value.starts_with("sha256:")
        && value[7..].bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        Ok(value.to_ascii_lowercase())
    } else {
        Err(SemanticProblem::Invalid(0))
    }
}

fn normalized_reference(value: &str) -> Result<String, SemanticProblem> {
    if value.is_empty()
        || value.len() > 4096
        || !value.contains(':')
        || value.contains(char::is_whitespace)
        || value.to_ascii_uppercase().contains("BEGIN ")
    {
        Err(SemanticProblem::Invalid(0))
    } else {
        Ok(value.into())
    }
}

fn normalized_text(value: &str, max: usize) -> Result<String, SemanticProblem> {
    if value.is_empty() || value.len() > max || value.chars().any(char::is_control) {
        Err(SemanticProblem::Invalid(0))
    } else {
        Ok(value.into())
    }
}

fn inner_first_name(operand: &ParsedOperand) -> Result<String, SemanticProblem> {
    operand
        .first()
        .ok_or(SemanticProblem::Invalid(operand.offset))
        .and_then(|value| bounded_upper(value, 246, false))
}

fn inner_value<'a>(operand: &'a ParsedOperand, name: &str) -> Option<&'a str> {
    let values = operand.values().collect::<Vec<_>>();
    values
        .windows(2)
        .find_map(|pair| pair[0].eq_ignore_ascii_case(name).then_some(pair[1]))
}

fn inner_flag(operand: &ParsedOperand, name: &str) -> bool {
    operand
        .values()
        .any(|value| value.eq_ignore_ascii_case(name))
}

fn operand_principal(
    command: &ParsedCommand,
    name: &str,
) -> Result<Option<String>, SemanticProblem> {
    command
        .operand(name)
        .map(|operand| {
            operand
                .first()
                .ok_or(SemanticProblem::Invalid(operand.offset))
                .and_then(upper_principal)
        })
        .transpose()
}

fn operand_name(
    command: &ParsedCommand,
    name: &str,
    max: usize,
) -> Result<Option<String>, SemanticProblem> {
    command
        .operand(name)
        .map(|operand| {
            let value = operand
                .first()
                .ok_or(SemanticProblem::Invalid(operand.offset))?;
            bounded_upper(value, max, true)
        })
        .transpose()
}

fn operand_names(
    command: &ParsedCommand,
    name: &str,
    max: usize,
) -> Result<BTreeSet<String>, SemanticProblem> {
    command
        .operand(name)
        .map(|operand| {
            operand
                .values()
                .map(|value| bounded_upper(value, max, true))
                .collect()
        })
        .unwrap_or_else(|| Ok(BTreeSet::new()))
}

fn operand_access(
    command: &ParsedCommand,
    name: &str,
) -> Result<Option<AccessLevel>, SemanticProblem> {
    command
        .operand(name)
        .map(|operand| {
            operand
                .first()
                .ok_or(SemanticProblem::Invalid(operand.offset))
                .and_then(parse_access)
        })
        .transpose()
}

fn operand_u32(command: &ParsedCommand, name: &str) -> Result<Option<u32>, SemanticProblem> {
    command
        .operand(name)
        .map(|operand| {
            operand
                .first()
                .ok_or(SemanticProblem::Invalid(operand.offset))?
                .parse::<u32>()
                .map_err(|_| SemanticProblem::Invalid(operand.offset))
        })
        .transpose()
}

fn parse_access(value: &str) -> Result<AccessLevel, SemanticProblem> {
    match value.to_ascii_uppercase().as_str() {
        "NONE" => Ok(AccessLevel::None),
        "EXECUTE" => Ok(AccessLevel::Execute),
        "READ" => Ok(AccessLevel::Read),
        "UPDATE" => Ok(AccessLevel::Update),
        "CONTROL" => Ok(AccessLevel::Control),
        "ALTER" => Ok(AccessLevel::Alter),
        _ => Err(SemanticProblem::Invalid(0)),
    }
}

fn parse_group_authority(value: &str) -> Result<GroupAuthority, SemanticProblem> {
    match value.to_ascii_uppercase().as_str() {
        "USE" => Ok(GroupAuthority::Use),
        "CREATE" => Ok(GroupAuthority::Create),
        "CONNECT" => Ok(GroupAuthority::Connect),
        "JOIN" => Ok(GroupAuthority::Join),
        _ => Err(SemanticProblem::Invalid(0)),
    }
}

fn parse_audit(command: &ParsedCommand) -> Result<AuditPolicy, SemanticProblem> {
    if command.has_operand("NOAUDIT") {
        return Ok(AuditPolicy::None);
    }
    let Some(operand) = command.operand("AUDIT") else {
        return Ok(AuditPolicy::Failures);
    };
    let values = operand
        .values()
        .map(str::to_ascii_uppercase)
        .collect::<BTreeSet<_>>();
    if values.contains("ALL") || values.contains("SUCCESS") && values.contains("FAILURES") {
        Ok(AuditPolicy::All)
    } else if values.contains("SUCCESS") || values.contains("SUCCESSES") {
        Ok(AuditPolicy::Successes)
    } else {
        Ok(AuditPolicy::Failures)
    }
}

fn profile_record(profile: &ResourceProfile) -> CommandRecord {
    CommandRecord::Profile {
        class: profile.class.clone(),
        name: profile.name.clone(),
        owner: profile.owner.clone(),
        generic: profile.generic,
        uacc: profile.uacc,
        access_entries: profile.access_list.len(),
        segments: profile.segments.keys().cloned().collect(),
        version: profile.version,
    }
}

fn certificate_record(certificate: &CertificateReference) -> CommandRecord {
    CommandRecord::Certificate {
        id: certificate.id.clone(),
        owner: certificate.owner.clone(),
        label: certificate.label.clone(),
        fingerprint_sha256: certificate.fingerprint_sha256.clone(),
        trusted: certificate.trusted,
        active: certificate.active,
        version: certificate.version,
    }
}

fn keyring_record(keyring: &KeyRing) -> CommandRecord {
    CommandRecord::Keyring {
        owner: keyring.owner.clone(),
        name: keyring.name.clone(),
        certificates: keyring.certificates.clone(),
        default_certificate: keyring.default_certificate.clone(),
        version: keyring.version,
    }
}

fn mapping_record(mapping: &IdentityMapping) -> CommandRecord {
    CommandRecord::IdentityMapping {
        id: mapping.id.clone(),
        registry: mapping.registry.clone(),
        distributed_identity: mapping.distributed_identity.clone(),
        local_user: mapping.local_user.clone(),
        version: mapping.version,
    }
}

fn association_record(association: &UserAssociation) -> CommandRecord {
    CommandRecord::Association {
        id: association.id.clone(),
        local_user: association.local_user.clone(),
        node: association.node.clone(),
        remote_user: association.remote_user.clone(),
        active: association.state == AssociationState::Active,
        version: association.version,
    }
}

fn node_record(node: &RrsfNode) -> CommandRecord {
    CommandRecord::RrsfNode {
        name: node.name.clone(),
        operative: node.state == RrsfNodeState::Operative,
        description: node.description.clone(),
        protocol: node.protocol.clone(),
        version: node.version,
    }
}

fn session_record(session: &crate::model::SignonSession) -> CommandRecord {
    CommandRecord::Session {
        id: session.id.clone(),
        user: session.user.clone(),
        node: session.node.clone(),
        active: session.state == SignonSessionState::Active,
        version: session.version,
    }
}

fn mask_match(value: &str, mask: Option<&str>) -> bool {
    mask.is_none_or(|mask| value.starts_with(mask.trim_end_matches('*')))
}

fn upper_principal(value: &str) -> Result<String, SemanticProblem> {
    bounded_upper(value, 8, false)
}

fn upper_class(value: &str) -> Result<String, SemanticProblem> {
    bounded_upper(value, 32, false)
}

fn upper_profile(value: &str) -> Result<String, SemanticProblem> {
    bounded_upper(value, 246, true)
}

fn bounded_upper(value: &str, max: usize, generic: bool) -> Result<String, SemanticProblem> {
    let value = value.to_ascii_uppercase();
    if value.is_empty()
        || value.len() > max
        || value.bytes().any(|byte| {
            !(byte.is_ascii_alphanumeric()
                || matches!(byte, b'@' | b'#' | b'$' | b'.' | b'-' | b'_')
                || generic && matches!(byte, b'*' | b'%'))
        })
    {
        Err(SemanticProblem::Invalid(0))
    } else {
        Ok(value)
    }
}

fn contains_generic(value: &str) -> bool {
    value.bytes().any(|byte| matches!(byte, b'*' | b'%'))
}

fn checked_version(version: u64) -> Result<u64, SemanticProblem> {
    version.checked_add(1).ok_or(SemanticProblem::Exhausted)
}

fn reason_for_problem(problem: &SemanticProblem) -> DecisionReason {
    match problem {
        SemanticProblem::Unauthorized => DecisionReason::DefaultDeny,
        SemanticProblem::NotFound => DecisionReason::ProfileNotFound,
        SemanticProblem::Conflict => DecisionReason::MalformedRequest,
        SemanticProblem::Invalid(_) => DecisionReason::MalformedRequest,
        SemanticProblem::Exhausted => DecisionReason::ResourceExhausted,
    }
}

fn problem_from_reason(reason: DecisionReason) -> SemanticProblem {
    match reason {
        DecisionReason::DefaultDeny
        | DecisionReason::PrincipalInactive
        | DecisionReason::InsufficientAccess => SemanticProblem::Unauthorized,
        DecisionReason::ProfileNotFound | DecisionReason::PrincipalNotFound => {
            SemanticProblem::NotFound
        }
        DecisionReason::ResourceExhausted => SemanticProblem::Exhausted,
        _ => SemanticProblem::Conflict,
    }
}

fn semantic_diagnostic(problem: SemanticProblem) -> CommandDiagnostic {
    match problem {
        SemanticProblem::Unauthorized => diagnostic(CommandDiagnosticCode::Unauthorized, 0),
        SemanticProblem::NotFound => diagnostic(CommandDiagnosticCode::NotFound, 0),
        SemanticProblem::Conflict => diagnostic(CommandDiagnosticCode::Conflict, 0),
        SemanticProblem::Invalid(offset) => diagnostic(CommandDiagnosticCode::InvalidValue, offset),
        SemanticProblem::Exhausted => diagnostic(CommandDiagnosticCode::ResourceExhausted, 0),
    }
}

fn host_diagnostic(problem: HostProblem) -> CommandDiagnostic {
    let code = match &problem {
        HostProblem::Unauthorized => CommandDiagnosticCode::Unauthorized,
        HostProblem::NotFound => CommandDiagnosticCode::NotFound,
        HostProblem::IdempotencyConflict => CommandDiagnosticCode::Conflict,
        HostProblem::ResourceExhausted => CommandDiagnosticCode::ResourceExhausted,
        _ => CommandDiagnosticCode::ProviderFailure,
    };
    diagnostic_with_host_problem(code, 0, problem)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AccessEnvironment, MemorySecretResolver, RacfLimits, RacrouteRequest, SafRequestContext,
    };
    use mainframe_env_execution_api::InvocationLimits;
    use mainframe_env_host_api::{AccessIntent, ResourceName, SecretRef, SecurityDecision};
    use mainframe_env_store::MemoryStore;
    use mainframe_env_store_api::{
        ProviderStateMutation, ProviderStateRecord, ProviderStateStore, ProviderStateWrite,
        StoreError,
    };
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    struct UnknownOutcomeStore {
        inner: Arc<MemoryStore>,
        fail_after_next_put: AtomicBool,
    }

    impl UnknownOutcomeStore {
        fn new(inner: Arc<MemoryStore>) -> Self {
            Self {
                inner,
                fail_after_next_put: AtomicBool::new(false),
            }
        }

        fn arm(&self) {
            self.fail_after_next_put.store(true, Ordering::SeqCst);
        }
    }

    impl mainframe_env_store_api::AuditSink for UnknownOutcomeStore {
        fn record_audit(
            &self,
            record: mainframe_env_execution_api::AuditRecord,
        ) -> Result<(), StoreError> {
            self.inner.record_audit(record)
        }

        fn audit_records(
            &self,
            execution_id: &mainframe_env_execution_api::ExecutionId,
            start_effect_sequence: u64,
            max: usize,
        ) -> Result<Vec<mainframe_env_execution_api::AuditRecord>, StoreError> {
            self.inner
                .audit_records(execution_id, start_effect_sequence, max)
        }
    }

    impl ProviderStateStore for UnknownOutcomeStore {
        fn get_provider_state(
            &self,
            namespace: &str,
            key: &str,
        ) -> Result<Option<ProviderStateRecord>, StoreError> {
            self.inner.get_provider_state(namespace, key)
        }

        fn list_provider_state(
            &self,
            namespace: &str,
            max: usize,
        ) -> Result<Vec<ProviderStateRecord>, StoreError> {
            self.inner.list_provider_state(namespace, max)
        }

        fn put_provider_state(
            &self,
            record: ProviderStateRecord,
            expected_version: Option<u64>,
        ) -> Result<(), StoreError> {
            self.inner.put_provider_state(record, expected_version)?;
            if self.fail_after_next_put.swap(false, Ordering::SeqCst) {
                Err(StoreError::Infrastructure(
                    "injected unknown outcome".into(),
                ))
            } else {
                Ok(())
            }
        }

        fn delete_provider_state(
            &self,
            namespace: &str,
            key: &str,
            expected_version: u64,
        ) -> Result<(), StoreError> {
            self.inner
                .delete_provider_state(namespace, key, expected_version)
        }

        fn move_provider_state(
            &self,
            record: ProviderStateRecord,
            old_key: &str,
            expected_version: u64,
        ) -> Result<(), StoreError> {
            self.inner
                .move_provider_state(record, old_key, expected_version)
        }

        fn put_provider_states_atomic(
            &self,
            writes: Vec<ProviderStateWrite>,
        ) -> Result<(), StoreError> {
            self.inner.put_provider_states_atomic(writes)
        }

        fn mutate_provider_states_atomic(
            &self,
            mutations: Vec<ProviderStateMutation>,
        ) -> Result<(), StoreError> {
            self.inner.mutate_provider_states_atomic(mutations)
        }
    }

    fn setup() -> (Arc<RacfService>, CommandContext) {
        let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
        let secrets = Arc::new(MemorySecretResolver::default());
        secrets.insert("secret:admin", b"ADMIN-PASSWORD".to_vec());
        let service = RacfService::open(store, secrets, RacfLimits::default()).unwrap();
        service
            .bootstrap_administrator(
                "RACFADM",
                &SecretRef::new("secret:admin", Default::default()).unwrap(),
            )
            .unwrap();
        let context = CommandContext::new(
            PrincipalId::new("RACFADM", InvocationLimits::default()).unwrap(),
            "TX-0001",
            "TEST-CORRELATION",
            1,
        )
        .unwrap();
        (service, context)
    }

    fn next(context: &CommandContext, id: &str) -> CommandContext {
        CommandContext::new(
            context.actor().clone(),
            id,
            context.correlation(),
            context.tick() + 1,
        )
        .unwrap()
    }

    #[test]
    fn command_digest_is_golden_and_redacts_credential_values_before_storage() {
        let input = "ADDUSER USER1 PASSWORD('THIS-SECRET-MUST-NOT-BE-HASHED') OWNER(RACFADM)";
        let parsed = parse_command(input, Default::default()).unwrap();
        let digest = command_request_digest(&parsed);
        let alternate = command_request_digest(
            &parse_command(
                "ADDUSER USER1 PASSWORD('A-DIFFERENT-SECRET') OWNER(RACFADM)",
                Default::default(),
            )
            .unwrap(),
        );
        let changed_request = command_request_digest(
            &parse_command(
                "ADDUSER USER2 PASSWORD('THIS-SECRET-MUST-NOT-BE-HASHED') OWNER(RACFADM)",
                Default::default(),
            )
            .unwrap(),
        );
        assert_eq!(
            digest,
            "sha256:5b150c202f1af2c3d1f63a24875153e7055dcc894d28daa90de9f3eb5356035e"
        );
        assert_eq!(digest, alternate);
        assert_ne!(digest, changed_request);
        assert_ne!(
            digest,
            format!("sha256:{:x}", Sha256::digest(input.as_bytes()))
        );

        let (service, context) = setup();
        service.execute_command(&context, input).unwrap();
        let snapshot = service.database.read().unwrap();
        let transaction = &snapshot.transactions[context.idempotency_key()];
        assert_eq!(
            transaction.request_digest_format,
            SecurityRequestDigestFormat::RacfCommandCanonicalV1
        );
        assert_eq!(transaction.request_digest, digest);
        let audit = snapshot.audits.last().unwrap();
        assert_eq!(audit.resource_digest.as_deref(), Some(digest.as_str()));
        assert_eq!(
            audit.fields["REQUEST_DIGEST_FORMAT"],
            AuditFieldValue::Text(
                SecurityRequestDigestFormat::RacfCommandCanonicalV1
                    .as_str()
                    .into()
            )
        );
        let durable = serde_json::to_string(&snapshot).unwrap();
        assert!(!durable.contains("THIS-SECRET-MUST-NOT-BE-HASHED"));
        assert!(!durable.contains(&format!("{:x}", Sha256::digest(input.as_bytes()))));
    }

    #[test]
    fn legacy_command_digests_are_scrubbed_then_require_reviewed_reconciliation() {
        let store = Arc::new(MemoryStore::new(Default::default()));
        let secrets = Arc::new(MemorySecretResolver::default());
        secrets.insert("secret:admin", b"ADMIN-PASSWORD".to_vec());
        let provider_store: Arc<dyn ProviderStateStore> = store.clone();
        let service =
            RacfService::open(provider_store, secrets.clone(), Default::default()).unwrap();
        service
            .bootstrap_administrator(
                "RACFADM",
                &SecretRef::new("secret:admin", Default::default()).unwrap(),
            )
            .unwrap();
        let context = CommandContext::new(
            PrincipalId::new("RACFADM", InvocationLimits::default()).unwrap(),
            "LEGACY-COMMAND",
            "LEGACY-COMMAND",
            3,
        )
        .unwrap();
        let input = "ADDUSER USER1 PASSWORD('LEGACY-RAW-SECRET')";
        service.execute_command(&context, input).unwrap();
        let legacy_digest = format!("sha256:{:x}", Sha256::digest(input.as_bytes()));
        service
            .database
            .mutate(|snapshot| {
                let transaction = snapshot.transactions[context.idempotency_key()].clone();
                let mut encoded =
                    serde_json::to_value(transaction).map_err(|_| HostProblem::Malformed)?;
                let encoded = encoded.as_object_mut().ok_or(HostProblem::Malformed)?;
                encoded.remove("request_digest_format");
                encoded.insert(
                    "request_digest".into(),
                    serde_json::Value::String(legacy_digest.clone()),
                );
                let transaction: SecurityTransaction =
                    serde_json::from_value(serde_json::Value::Object(encoded.clone()))
                        .map_err(|_| HostProblem::Malformed)?;
                assert_eq!(
                    transaction.request_digest_format,
                    SecurityRequestDigestFormat::LegacyUnversioned
                );
                snapshot
                    .transactions
                    .insert(context.idempotency_key().into(), transaction);
                let audit = snapshot.audits.last_mut().ok_or(HostProblem::NotFound)?;
                audit.resource_digest = Some(legacy_digest.clone());
                audit.fields.remove("REQUEST_DIGEST_FORMAT");
                Ok(())
            })
            .unwrap();
        drop(service);

        let provider_store: Arc<dyn ProviderStateStore> = store.clone();
        let reopened =
            RacfService::open(provider_store, secrets.clone(), Default::default()).unwrap();
        let snapshot = reopened.database.read().unwrap();
        let transaction = &snapshot.transactions[context.idempotency_key()];
        assert_eq!(
            transaction.request_digest_format,
            SecurityRequestDigestFormat::LegacyScrubbedV0
        );
        assert_ne!(transaction.request_digest, legacy_digest);
        let scrubbed_digest = transaction.request_digest.clone();
        let audit = snapshot.audits.last().unwrap();
        assert_ne!(
            audit.resource_digest.as_deref(),
            Some(legacy_digest.as_str())
        );
        assert_eq!(
            audit.fields["REQUEST_DIGEST_FORMAT"],
            AuditFieldValue::Text(
                SecurityRequestDigestFormat::LegacyScrubbedV0
                    .as_str()
                    .into()
            )
        );
        drop(snapshot);

        let problem = reopened.execute_command(&context, input).unwrap_err();
        assert_eq!(problem.code, CommandDiagnosticCode::ProviderFailure);
        assert_eq!(problem.host_problem(), Some(&HostProblem::UnknownOutcome));
        assert_eq!(
            reopened.reconcile_legacy_command(
                &context,
                &format!("sha256:{}", "b".repeat(64)),
                input,
            ),
            Err(HostProblem::IdempotencyConflict)
        );
        reopened
            .reconcile_legacy_command(&context, &scrubbed_digest, input)
            .unwrap();
        let replay = reopened.execute_command(&context, input).unwrap();
        assert!(replay.replayed);
        assert_eq!(reopened.database.read().unwrap().principals.len(), 2);
        drop(reopened);

        let provider_store: Arc<dyn ProviderStateStore> = store;
        let restarted = RacfService::open(provider_store, secrets, Default::default()).unwrap();
        assert!(restarted.execute_command(&context, input).unwrap().replayed);
        assert_eq!(restarted.database.read().unwrap().principals.len(), 2);
    }

    #[test]
    fn user_group_resource_commands_are_atomic_idempotent_and_queryable() {
        let (service, context) = setup();
        service
            .execute_command(&context, "ADDGROUP OPER OWNER(RACFADM)")
            .unwrap();
        let add_user = next(&context, "TX-0002");
        service
            .execute_command(
                &add_user,
                "ADDUSER USER1 DFLTGRP(OPER) PASSWORD('USER PASSWORD') OMVS(UID(1001))",
            )
            .unwrap();
        let replay = service
            .execute_command(
                &add_user,
                "ADDUSER USER1 DFLTGRP(OPER) PASSWORD('USER PASSWORD') OMVS(UID(1001))",
            )
            .unwrap();
        assert!(replay.replayed);
        let define = next(&context, "TX-0003");
        service
            .execute_command(&define, "ADDSD 'USER1.**' GENERIC OWNER(USER1) UACC(NONE)")
            .unwrap();
        service
            .execute_command(
                &next(&context, "TX-0004"),
                "PERMIT 'USER1.**' CLASS(DATASET) ID(USER1) ACCESS(UPDATE)",
            )
            .unwrap();
        let list = service
            .execute_command(&next(&context, "QUERY-1"), "LISTUSER USER1 ALL")
            .unwrap();
        assert!(
            matches!(list.records.as_slice(), [CommandRecord::User { segments, .. }] if segments.contains("BASE"))
        );
        let resources = service
            .execute_command(&next(&context, "QUERY-2"), "LISTDSD 'USER1.**' ALL")
            .unwrap();
        assert!(matches!(
            resources.records.as_slice(),
            [CommandRecord::Profile {
                access_entries: 2,
                ..
            }]
        ));
    }

    #[test]
    fn denied_malformed_and_conflicting_commands_do_not_mutate_targets_but_are_audited() {
        let (service, context) = setup();
        service
            .execute_command(&context, "ADDGROUP OPER OWNER(RACFADM)")
            .unwrap();
        let before = service.database().summary().unwrap();
        let ordinary = CommandContext::new(
            PrincipalId::new("UNKNOWN", InvocationLimits::default()).unwrap(),
            "DENY-1",
            "DENY-CORRELATION",
            2,
        )
        .unwrap();
        assert_eq!(
            service
                .execute_command(&ordinary, "ADDGROUP NOAUTH")
                .unwrap_err()
                .code,
            CommandDiagnosticCode::Unauthorized
        );
        let after = service.database().summary().unwrap();
        assert_eq!(after.groups, before.groups);
        assert_eq!(after.audits, before.audits + 1);
        assert_eq!(
            service
                .execute_command(&next(&context, "BAD-1"), "ADDUSER USER1 UNKNOWN(x)")
                .unwrap_err()
                .code,
            CommandDiagnosticCode::UnknownOperand
        );
        assert_eq!(
            service.database().summary().unwrap().principals,
            before.principals
        );
    }

    #[test]
    fn deliberately_unimplemented_publication_operand_fails_before_any_effect() {
        let (service, context) = setup();
        let before = service.database().summary().unwrap();
        let problem = service
            .execute_command(&context, "ADDGROUP OPER AT(NODE1)")
            .unwrap_err();
        assert_eq!(problem.code, CommandDiagnosticCode::UnsupportedCapability);
        assert!(matches!(
            problem.host_problem(),
            Some(HostProblem::UnsupportedCapability { capability, detail })
                if capability == "racf-command-operand"
                    && detail == "ADDGROUP operand AT is not implemented by the RACF command processor"
        ));
        assert_eq!(service.database().summary().unwrap(), before);
    }

    #[test]
    fn ordinary_altuser_self_service_cannot_grant_or_use_privileged_authority() {
        let (service, admin) = setup();
        service
            .execute_command(&admin, "ADDUSER USER1 PASSWORD('USER-PASSWORD')")
            .unwrap();
        let user = PrincipalId::new("USER1", InvocationLimits::default()).unwrap();
        let before = service.database().summary().unwrap();
        for (index, operand) in [
            "SPECIAL",
            "AUDITOR",
            "OPERATIONS",
            "OWNER(RACFADM)",
            "REVOKE",
            "OMVS(UID(0))",
            "MFA(FACTOR1 REF secret:mfa TYPE TOTP)",
        ]
        .into_iter()
        .enumerate()
        {
            let context = CommandContext::new(
                user.clone(),
                format!("SELF-ESCALATE-{index}"),
                "SELF-ESCALATE",
                10 + index as u64,
            )
            .unwrap();
            assert_eq!(
                service
                    .execute_command(&context, &format!("ALTUSER USER1 {operand}"))
                    .unwrap_err()
                    .code,
                CommandDiagnosticCode::Unauthorized
            );
        }
        let snapshot = service.database.read().unwrap();
        assert!(
            ["SPECIAL", "AUDITOR", "OPERATIONS"]
                .into_iter()
                .all(|attribute| !snapshot.principals["USER1"].attributes.contains(attribute))
        );
        assert_eq!(snapshot.principals["USER1"].state, PrincipalState::Active);
        assert!(!snapshot.mfa_factors.contains_key("FACTOR1"));
        assert_eq!(snapshot.audits.len(), before.audits + 7);
        drop(snapshot);

        let denied =
            CommandContext::new(user.clone(), "SELF-USE-PRIVILEGE", "SELF-ESCALATE", 30).unwrap();
        assert_eq!(
            service
                .execute_command(&denied, "ADDGROUP ESCALATED")
                .unwrap_err()
                .code,
            CommandDiagnosticCode::Unauthorized
        );
        assert!(
            !service
                .database
                .read()
                .unwrap()
                .groups
                .contains_key("ESCALATED")
        );

        service
            .execute_command(
                &CommandContext::new(user, "SELF-NAME", "SELF-ESCALATE", 31).unwrap(),
                "ALTUSER USER1 NAME('ORDINARY USER')",
            )
            .unwrap();
    }

    #[test]
    fn permit_when_is_typed_evaluated_and_replaced_independently() {
        let (service, admin) = setup();
        for (index, command) in [
            "ADDUSER USER1 PASSWORD('USER-PASSWORD')",
            "RDEFINE FACILITY COND.** OWNER(RACFADM) UACC(NONE)",
            "PERMIT 'COND.**' CLASS(FACILITY) ID(USER1) ACCESS(READ) WHEN(TERMINAL(TERM1) TIME(5 10))",
        ]
        .into_iter()
        .enumerate()
        {
            service
                .execute_command(&next(&admin, &format!("WHEN-{index}")), command)
                .unwrap_or_else(|problem| panic!("{command}: {problem}"));
        }
        let user = PrincipalId::new("USER1", InvocationLimits::default()).unwrap();
        let authorize = |id: &str, terminal: &str, tick: u64| {
            service
                .racroute(
                    &SafRequestContext::new(user.clone(), None, None, id, "PERMIT-WHEN", tick)
                        .unwrap(),
                    RacrouteRequest::Auth {
                        class: "FACILITY".into(),
                        resource: "COND.ONE".into(),
                        access: AccessLevel::Read,
                        environment: AccessEnvironment {
                            terminal: Some(terminal.into()),
                            tick,
                            ..Default::default()
                        },
                    },
                )
                .unwrap()
        };
        assert_eq!(
            authorize("WHEN-ALLOW", "TERM1", 6).status.reason,
            DecisionReason::Granted
        );
        assert_eq!(
            authorize("WHEN-DENY-TERM", "OTHER", 6).status.reason,
            DecisionReason::ConditionNotSatisfied
        );
        assert_eq!(
            authorize("WHEN-DENY-TIME", "TERM1", 11).status.reason,
            DecisionReason::ConditionNotSatisfied
        );

        service
            .execute_command(
                &next(&admin, "WHEN-UNCONDITIONAL"),
                "PERMIT 'COND.**' CLASS(FACILITY) ID(USER1) ACCESS(UPDATE)",
            )
            .unwrap();
        assert_eq!(
            service.database.read().unwrap().profiles["FACILITY:COND.**"]
                .access_list
                .iter()
                .filter(|entry| entry.principal == "USER1")
                .count(),
            2
        );
        service
            .execute_command(
                &next(&admin, "WHEN-DELETE-UNCONDITIONAL"),
                "PERMIT 'COND.**' CLASS(FACILITY) ID(USER1) DELETE",
            )
            .unwrap();
        let snapshot = service.database.read().unwrap();
        let entries = snapshot.profiles["FACILITY:COND.**"]
            .access_list
            .iter()
            .filter(|entry| entry.principal == "USER1")
            .collect::<Vec<_>>();
        assert_eq!(entries.len(), 1);
        assert!(entries[0].when.is_some());
        drop(snapshot);

        service
            .execute_command(
                &next(&admin, "WHEN-REPLACE-CONDITIONAL"),
                "PERMIT 'COND.**' CLASS(FACILITY) ID(USER1) ACCESS(CONTROL) WHEN(TERMINAL(TERM1) TIME(5 10))",
            )
            .unwrap();
        let snapshot = service.database.read().unwrap();
        let entries = snapshot.profiles["FACILITY:COND.**"]
            .access_list
            .iter()
            .filter(|entry| entry.principal == "USER1")
            .collect::<Vec<_>>();
        assert_eq!(
            (entries.len(), entries[0].access),
            (1, AccessLevel::Control)
        );
        drop(snapshot);

        let before = service.database.read().unwrap();
        assert_eq!(
            service
                .execute_command(
                    &next(&admin, "WHEN-UNSUPPORTED"),
                    "PERMIT 'COND.**' CLASS(FACILITY) ID(USER1) ACCESS(READ) WHEN(EXIT(UNKNOWN))",
                )
                .unwrap_err()
                .code,
            CommandDiagnosticCode::InvalidValue
        );
        let after = service.database.read().unwrap();
        assert_eq!(
            after.profiles["FACILITY:COND.**"].access_list,
            before.profiles["FACILITY:COND.**"].access_list
        );
        assert_eq!(after.audits.len(), before.audits.len() + 1);
        drop(after);

        service
            .execute_command(
                &next(&admin, "WHEN-DELETE-CONDITIONAL"),
                "PERMIT 'COND.**' CLASS(FACILITY) ID(USER1) DELETE WHEN(TERMINAL(TERM1) TIME(5 10))",
            )
            .unwrap();
        assert!(
            service.database.read().unwrap().profiles["FACILITY:COND.**"]
                .access_list
                .iter()
                .all(|entry| entry.principal != "USER1")
        );
    }

    #[test]
    fn remote_only_direction_is_an_explicit_capability_failure_before_local_effects() {
        let (service, admin) = setup();
        for (index, command) in ["ADDGROUP GROUP1", "ADDUSER USER1"].into_iter().enumerate() {
            service
                .execute_command(&next(&admin, &format!("DIRECTION-SETUP-{index}")), command)
                .unwrap();
        }
        let before = service.database().summary().unwrap();
        for (index, command) in ["DELUSER USER1 ONLYAT(REMOTE)", "DELGROUP GROUP1 AT(REMOTE)"]
            .into_iter()
            .enumerate()
        {
            let problem = service
                .execute_command(&next(&admin, &format!("DIRECTION-DENY-{index}")), command)
                .unwrap_err();
            assert_eq!(problem.code, CommandDiagnosticCode::UnsupportedCapability);
            assert!(matches!(
                problem.host_problem(),
                Some(HostProblem::UnsupportedCapability { capability, .. })
                    if capability == "racf-command-operand"
            ));
        }
        let after = service.database().summary().unwrap();
        assert_eq!(
            (after.principals, after.groups),
            (before.principals, before.groups)
        );
        assert_eq!(after.audits, before.audits);
        assert!(
            service
                .database
                .read()
                .unwrap()
                .principals
                .contains_key("USER1")
        );
        assert!(
            service
                .database
                .read()
                .unwrap()
                .groups
                .contains_key("GROUP1")
        );
        service
            .execute_command(
                &next(&admin, "DIRECTION-LOCAL-SIGNOFF"),
                "SIGNOFF AT(REMOTE) LIST",
            )
            .unwrap();
    }

    #[test]
    fn add_alt_and_password_share_policy_history_and_nopassword_across_restart() {
        let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
        let secrets = Arc::new(MemorySecretResolver::default());
        secrets.insert("secret:admin", b"ADMIN-PASSWORD".to_vec());
        let service =
            RacfService::open(store.clone(), secrets.clone(), Default::default()).unwrap();
        service
            .bootstrap_administrator(
                "RACFADM",
                &SecretRef::new("secret:admin", Default::default()).unwrap(),
            )
            .unwrap();
        let admin = CommandContext::new(
            PrincipalId::new("RACFADM", InvocationLimits::default()).unwrap(),
            "POLICY-SET",
            "PASSWORD-POLICY",
            1,
        )
        .unwrap();
        service
            .execute_command(
                &admin,
                "SETROPTS PASSWORD(MINIMUM(10) MAXIMUM(20) HISTORY(2)) PHRASE(MINIMUM(15))",
            )
            .unwrap();
        for (id, command) in [
            ("ADD-SHORT", "ADDUSER SHORT PASSWORD('short')"),
            ("ADD-PHRASE-SHORT", "ADDUSER PHRASE1 PHRASE('short phrase')"),
        ] {
            assert_eq!(
                service
                    .execute_command(&next(&admin, id), command)
                    .unwrap_err()
                    .code,
                CommandDiagnosticCode::InvalidValue
            );
        }
        service
            .execute_command(
                &next(&admin, "ADD-VALID"),
                "ADDUSER USER1 PASSWORD('VALID-PASS1')",
            )
            .unwrap();
        service
            .execute_command(
                &next(&admin, "ADD-PHRASE-VALID"),
                "ADDUSER PHRASE1 PHRASE('VALID LONG PHRASE')",
            )
            .unwrap();
        for (id, command) in [
            ("ALT-SHORT", "ALTUSER USER1 PASSWORD('tiny')"),
            ("ALT-MAX", "ALTUSER USER1 PASSWORD('123456789012345678901')"),
        ] {
            assert_eq!(
                service
                    .execute_command(&next(&admin, id), command)
                    .unwrap_err()
                    .code,
                CommandDiagnosticCode::InvalidValue
            );
        }
        service
            .execute_command(
                &next(&admin, "ALT-VALID"),
                "ALTUSER USER1 PASSWORD('VALID-PASS2')",
            )
            .unwrap();
        let credential = service.database.read().unwrap().principals["USER1"]
            .credential
            .clone()
            .unwrap();
        assert_eq!(credential.history_verifiers.len(), 1);
        drop(service);

        let reopened = RacfService::open(store, secrets, Default::default()).unwrap();
        assert_eq!(
            reopened
                .execute_command(
                    &next(&admin, "ALT-REUSE-AFTER-RESTART"),
                    "ALTUSER USER1 PASSWORD('VALID-PASS1')",
                )
                .unwrap_err()
                .code,
            CommandDiagnosticCode::Conflict
        );
        assert_eq!(
            reopened
                .execute_command(
                    &next(&admin, "ALT-PHRASE-SHORT"),
                    "ALTUSER USER1 PHRASE('short phrase')",
                )
                .unwrap_err()
                .code,
            CommandDiagnosticCode::InvalidValue
        );
        reopened
            .execute_command(
                &next(&admin, "ALT-PHRASE-VALID"),
                "ALTUSER USER1 PHRASE('ANOTHER LONG PHRASE')",
            )
            .unwrap();
        reopened
            .execute_command(&next(&admin, "ALT-NOPASSWORD"), "ALTUSER USER1 NOPASSWORD")
            .unwrap();
        assert!(!reopened.database.read().unwrap().principals["USER1"].has_credential());

        reopened
            .execute_command(
                &next(&admin, "ADD-SELF-PASSWORD"),
                "ADDUSER USER2 PASSWORD('VALID-PASS1')",
            )
            .unwrap();
        let user = PrincipalId::new("USER2", InvocationLimits::default()).unwrap();
        assert_eq!(
            reopened
                .execute_command(
                    &CommandContext::new(
                        user.clone(),
                        "SELF-PASSWORD-MISSING-CURRENT",
                        "PASSWORD-POLICY",
                        50,
                    )
                    .unwrap(),
                    "PASSWORD PASSWORD('VALID-PASS2')",
                )
                .unwrap_err()
                .code,
            CommandDiagnosticCode::InvalidValue
        );
        reopened
            .execute_command(
                &CommandContext::new(user, "SELF-PASSWORD-WITH-CURRENT", "PASSWORD-POLICY", 51)
                    .unwrap(),
                "PASSWORD PASSWORD('VALID-PASS1' 'VALID-PASS2')",
            )
            .unwrap();
    }

    #[test]
    fn altuser_default_group_requires_connection_and_survives_restart() {
        let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
        let secrets = Arc::new(MemorySecretResolver::default());
        secrets.insert("secret:admin", b"ADMIN-PASSWORD".to_vec());
        let service =
            RacfService::open(store.clone(), secrets.clone(), Default::default()).unwrap();
        service
            .bootstrap_administrator(
                "RACFADM",
                &SecretRef::new("secret:admin", Default::default()).unwrap(),
            )
            .unwrap();
        let admin = CommandContext::new(
            PrincipalId::new("RACFADM", InvocationLimits::default()).unwrap(),
            "DFLT-1",
            "DEFAULT-GROUP",
            1,
        )
        .unwrap();
        for (index, command) in [
            "ADDGROUP GROUP1",
            "ADDGROUP GROUP2",
            "ADDUSER USER1 DFLTGRP(GROUP1)",
        ]
        .into_iter()
        .enumerate()
        {
            service
                .execute_command(&next(&admin, &format!("DFLT-{index}")), command)
                .unwrap();
        }
        assert_eq!(
            service
                .execute_command(
                    &next(&admin, "DFLT-NOT-CONNECTED"),
                    "ALTUSER USER1 DFLTGRP(GROUP2)",
                )
                .unwrap_err()
                .code,
            CommandDiagnosticCode::Conflict
        );
        assert_eq!(
            service.database.read().unwrap().principals["USER1"].default_group,
            Some("GROUP1".into())
        );
        for (id, command) in [
            ("DFLT-CONNECT", "CONNECT USER1 GROUP(GROUP2)"),
            ("DFLT-ALTER", "ALTUSER USER1 DFLTGRP(GROUP2)"),
            (
                "DFLT-PROFILE",
                "ADDSD 'GROUP2.**' OWNER(RACFADM) UACC(NONE)",
            ),
            (
                "DFLT-PERMIT",
                "PERMIT 'GROUP2.**' CLASS(DATASET) ID(GROUP2) ACCESS(READ)",
            ),
        ] {
            service.execute_command(&next(&admin, id), command).unwrap();
        }
        drop(service);

        let reopened = RacfService::open(store, secrets, Default::default()).unwrap();
        let snapshot = reopened.database.read().unwrap();
        assert_eq!(
            snapshot.principals["USER1"].default_group,
            Some("GROUP2".into())
        );
        assert!(
            snapshot
                .connections
                .get(&connection_key("USER1", "GROUP2"))
                .is_some_and(|connection| !connection.revoked)
        );
        drop(snapshot);
        assert_eq!(
            reopened
                .authorize(
                    &PrincipalId::new("USER1", InvocationLimits::default()).unwrap(),
                    "DATASET",
                    &ResourceName::new("GROUP2.DATA", 246).unwrap(),
                    AccessIntent::Read,
                )
                .unwrap(),
            SecurityDecision::Allow
        );
    }

    #[test]
    fn rollback_on_second_target_failure_preserves_first_target() {
        let (service, context) = setup();
        service
            .execute_command(&context, "ADDGROUP OPER OWNER(RACFADM)")
            .unwrap();
        assert_eq!(
            service
                .execute_command(&next(&context, "TX-ROLLBACK"), "DELGROUP OPER MISSING",)
                .unwrap_err()
                .code,
            CommandDiagnosticCode::NotFound
        );
        let listed = service
            .execute_command(&next(&context, "QUERY-GROUP"), "LISTGRP OPER")
            .unwrap();
        assert_eq!(listed.records.len(), 1);
    }

    #[test]
    fn command_replay_is_bound_to_the_active_actor_across_restart() {
        let store = Arc::new(MemoryStore::new(Default::default()));
        let secrets = Arc::new(MemorySecretResolver::default());
        secrets.insert("secret:admin", b"ADMIN-PASSWORD".to_vec());
        let service =
            RacfService::open(store.clone(), secrets.clone(), Default::default()).unwrap();
        service
            .bootstrap_administrator(
                "RACFADM",
                &SecretRef::new("secret:admin", Default::default()).unwrap(),
            )
            .unwrap();
        let admin = PrincipalId::new("RACFADM", InvocationLimits::default()).unwrap();
        let user = PrincipalId::new("USER1", InvocationLimits::default()).unwrap();
        let missing = PrincipalId::new("MISSING", InvocationLimits::default()).unwrap();
        service
            .execute_command(
                &CommandContext::new(admin.clone(), "ACTOR-SETUP", "ACTOR-REPLAY", 1).unwrap(),
                "ADDUSER USER1",
            )
            .unwrap();
        let original =
            CommandContext::new(admin.clone(), "ACTOR-BOUND", "ACTOR-REPLAY", 2).unwrap();
        service.execute_command(&original, "ADDGROUP OPER").unwrap();
        let before = service.database.read().unwrap();
        let transaction = before.transactions["ACTOR-BOUND"].clone();
        let audit_count = before.audits.len();
        let group_count = before.groups.len();
        drop(before);

        for (actor, command, tick) in [
            (user.clone(), "ADDGROUP OPER", 3),
            (missing, "DELGROUP OPER", 4),
        ] {
            let denied = service
                .execute_command(
                    &CommandContext::new(actor, "ACTOR-BOUND", "ACTOR-REPLAY", tick).unwrap(),
                    command,
                )
                .unwrap_err();
            assert_eq!(denied.code, CommandDiagnosticCode::Unauthorized);
        }
        let after_denials = service.database.read().unwrap();
        assert_eq!(after_denials.groups.len(), group_count);
        assert_eq!(after_denials.transactions["ACTOR-BOUND"], transaction);
        assert_eq!(after_denials.audits.len(), audit_count + 2);
        assert!(after_denials.audits[audit_count..].iter().all(|audit| {
            audit.decision == DecisionOutcome::Deny
                && audit.status.reason == DecisionReason::InsufficientAccess
        }));
        assert_eq!(after_denials.audits[audit_count].actor, "USER1");
        assert_eq!(after_denials.audits[audit_count + 1].actor, "MISSING");
        drop(after_denials);

        drop(service);
        let service = RacfService::open(store, secrets, Default::default()).unwrap();
        assert_eq!(
            service
                .execute_command(
                    &CommandContext::new(user, "ACTOR-BOUND", "ACTOR-REPLAY-RESTART", 5,).unwrap(),
                    "ADDGROUP OPER",
                )
                .unwrap_err()
                .code,
            CommandDiagnosticCode::Unauthorized
        );
        let audit_count_before_owner_replay = service.database.read().unwrap().audits.len();
        let replay = service.execute_command(&original, "ADDGROUP OPER").unwrap();
        assert!(replay.replayed);
        let after_restart = service.database.read().unwrap();
        assert_eq!(after_restart.groups.len(), group_count);
        assert_eq!(after_restart.transactions["ACTOR-BOUND"], transaction);
        assert_eq!(after_restart.audits.len(), audit_count_before_owner_replay);
    }

    #[test]
    fn every_sec_505_family_reaches_its_owned_handler_without_exposing_references() {
        let (service, context) = setup();
        let fingerprint = format!("sha256:{}", "a".repeat(64));
        let commands = [
            "ADDUSER USER1 PASSWORD('USER-PASSWORD') MFA(FACTOR1 REF secret:mfa TYPE TOTP)"
                .to_string(),
            "PASSWORD USER(USER1) PASSWORD('NEW-USER-PASSWORD')".into(),
            "TARGET NODE(NODE1) DESCRIPTION('REMOTE NODE') PROTOCOL(TCP)".into(),
            "RACLINK USER1 DEFINE(NODE1 REMOTE1)".into(),
            "RACMAP ID(USER1) MAP(MAP1 REGISTRY LDAP NAME user@example.com LABEL MAPONE)".into(),
            format!(
                "RACDCERT ID(USER1) ADD(CERT1 CERTREF secret:cert1 FINGERPRINT {fingerprint} LABEL CERTONE KEYID KEY1 KEYREF secret:key1)"
            ),
            "RACDCERT ID(USER1) CONNECT(CERT1 RING RING1 DEFAULT)".into(),
            "SIGNOFF LIST".into(),
        ];
        let mut reached = BTreeSet::new();
        for (index, command) in commands.iter().enumerate() {
            let result = service
                .execute_command(&next(&context, &format!("SEC505-{index}")), command)
                .unwrap_or_else(|problem| panic!("{command}: {problem}"));
            reached.insert(result.family);
            let public = format!("{:?}", result.records);
            assert!(!public.contains("secret:mfa"));
            assert!(!public.contains("secret:cert1"));
            assert!(!public.contains("secret:key1"));
        }
        assert_eq!(
            reached,
            BTreeSet::from([
                CommandFamily::AddUser,
                CommandFamily::Password,
                CommandFamily::Racdcert,
                CommandFamily::Raclink,
                CommandFamily::Racmap,
                CommandFamily::Signoff,
                CommandFamily::Target,
            ])
        );
        assert_eq!(
            service
                .execute_command(
                    &next(&context, "SEC505-HISTORY"),
                    "PASSWORD USER(USER1) PASSWORD('USER-PASSWORD')",
                )
                .unwrap_err()
                .code,
            CommandDiagnosticCode::Conflict
        );
        let snapshot = service.database.read().unwrap();
        let verifier = PasswordHash::new(
            &snapshot.principals["USER1"]
                .credential
                .as_ref()
                .unwrap()
                .encoded_verifier,
        )
        .unwrap();
        assert!(
            Argon2::default()
                .verify_password(b"NEW-USER-PASSWORD", &verifier)
                .is_ok()
        );
        assert!(
            Argon2::default()
                .verify_password(b"USER-PASSWORD", &verifier)
                .is_err()
        );
        assert_eq!(
            snapshot.mfa_factors["FACTOR1"].secret_reference,
            "secret:mfa"
        );
        assert_eq!(
            snapshot.certificates["CERT1"].certificate_reference,
            "secret:cert1"
        );
        assert_eq!(snapshot.keys["KEY1"].key_reference, "secret:key1");
        assert_eq!(
            snapshot.keyrings[&keyring_key("USER1", "RING1")].default_certificate,
            Some("CERT1".into())
        );
        let audits = format!("{:?}", snapshot.audits);
        assert!(!audits.contains("secret:mfa"));
        assert!(!audits.contains("secret:cert1"));
        assert!(!audits.contains("secret:key1"));
    }

    #[test]
    fn every_sec_502_family_reaches_its_owned_handler() {
        let (service, context) = setup();
        let commands = [
            "ADDGROUP OPER OWNER(RACFADM)",
            "ADDGROUP DEV SUPGROUP(OPER) OWNER(RACFADM)",
            "ALTGROUP DEV DATA('DEVELOPMENT GROUP')",
            "ADDUSER USER2 DFLTGRP(DEV)",
            "ALTUSER USER2 DATA('DEVELOPER')",
            "CONNECT USER2 GROUP(OPER) AUTHORITY(USE)",
            "REMOVE USER2 GROUP(OPER)",
            "ADDSD 'USER2.**' GENERIC OWNER(USER2) UACC(NONE)",
            "ALTDSD 'USER2.**' AUDIT(ALL) LEVEL(1)",
            "PERMIT 'USER2.**' CLASS(DATASET) ID(USER2) ACCESS(READ)",
            "LISTDSD 'USER2.**' ALL",
            "RDEFINE FACILITY DEV.RESOURCE OWNER(USER2) UACC(NONE)",
            "RALTER FACILITY DEV.RESOURCE AUDIT(FAILURES)",
            "RLIST FACILITY DEV.RESOURCE ALL",
            "SEARCH CLASS(FACILITY) MASK(DEV)",
            "LISTUSER USER2 ALL",
            "LISTGRP DEV ALL",
            "DISPLAY ALL",
            "RDELETE FACILITY DEV.RESOURCE",
            "DELDSD 'USER2.**' GENERIC",
            "DELUSER USER2",
            "DELGROUP DEV",
        ];
        let mut reached = BTreeSet::new();
        for (index, command) in commands.into_iter().enumerate() {
            let invocation = next(&context, &format!("MATRIX-{index:02}"));
            let result = service
                .execute_command(&invocation, command)
                .unwrap_or_else(|problem| panic!("redacted command matrix failure: {problem}"));
            reached.insert(result.family);
        }
        let expected = crate::command_descriptors()
            .iter()
            .filter(|descriptor| descriptor.work_package() == "SEC-502")
            .map(|descriptor| descriptor.family())
            .collect::<BTreeSet<_>>();
        assert_eq!(reached, expected);
        assert_eq!(expected.len(), 21);
    }

    #[test]
    fn concurrent_authority_instances_retry_cas_without_lost_updates() {
        let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
        let secrets = Arc::new(MemorySecretResolver::default());
        secrets.insert("secret:admin", b"ADMIN-PASSWORD".to_vec());
        let first = RacfService::open(store.clone(), secrets.clone(), Default::default()).unwrap();
        first
            .bootstrap_administrator(
                "RACFADM",
                &SecretRef::new("secret:admin", Default::default()).unwrap(),
            )
            .unwrap();
        let second = RacfService::open(store, secrets, Default::default()).unwrap();
        let barrier = Arc::new(std::sync::Barrier::new(3));
        let mut workers = Vec::new();
        for (service, group, transaction) in [
            (first.clone(), "GROUPA", "CONCURRENT-A"),
            (second, "GROUPB", "CONCURRENT-B"),
        ] {
            let barrier = barrier.clone();
            workers.push(std::thread::spawn(move || {
                let context = CommandContext::new(
                    PrincipalId::new("RACFADM", InvocationLimits::default()).unwrap(),
                    transaction,
                    transaction,
                    1,
                )
                .unwrap();
                barrier.wait();
                service.execute_command(&context, &format!("ADDGROUP {group}"))
            }));
        }
        barrier.wait();
        for worker in workers {
            worker.join().unwrap().unwrap();
        }
        let listed = first
            .execute_command(
                &CommandContext::new(
                    PrincipalId::new("RACFADM", InvocationLimits::default()).unwrap(),
                    "QUERY-CONCURRENT",
                    "QUERY-CONCURRENT",
                    2,
                )
                .unwrap(),
                "LISTGRP GROUPA GROUPB",
            )
            .unwrap();
        assert_eq!(listed.records.len(), 2);
    }

    #[test]
    fn committed_unknown_outcome_replays_after_restart_without_duplicate_mutation() {
        let backing = Arc::new(MemoryStore::new(Default::default()));
        let fault_store = Arc::new(UnknownOutcomeStore::new(backing.clone()));
        let secrets = Arc::new(MemorySecretResolver::default());
        secrets.insert("secret:admin", b"ADMIN-PASSWORD".to_vec());
        let provider_store: Arc<dyn ProviderStateStore> = fault_store.clone();
        let service =
            RacfService::open(provider_store, secrets.clone(), Default::default()).unwrap();
        service
            .bootstrap_administrator(
                "RACFADM",
                &SecretRef::new("secret:admin", Default::default()).unwrap(),
            )
            .unwrap();
        let context = CommandContext::new(
            PrincipalId::new("RACFADM", InvocationLimits::default()).unwrap(),
            "UNKNOWN-OUTCOME",
            "UNKNOWN-OUTCOME",
            1,
        )
        .unwrap();
        fault_store.arm();
        assert_eq!(
            service
                .execute_command(&context, "ADDGROUP GROUP1")
                .unwrap_err()
                .code,
            CommandDiagnosticCode::ProviderFailure
        );
        drop(service);
        let provider_store: Arc<dyn ProviderStateStore> = backing;
        let reopened = RacfService::open(provider_store, secrets, Default::default()).unwrap();
        let replay = reopened
            .execute_command(&context, "ADDGROUP GROUP1")
            .unwrap();
        assert!(replay.replayed);
        let listed = reopened
            .execute_command(&next(&context, "UNKNOWN-LIST"), "LISTGRP GROUP1")
            .unwrap();
        assert_eq!(listed.records.len(), 1);
    }

    #[test]
    fn setropts_raclist_refresh_class_and_generic_policy_are_exact() {
        use mainframe_env_host_api::{AccessIntent, ResourceName, SecurityDecision};

        let (service, context) = setup();
        service.execute_command(&context, "ADDUSER USER1").unwrap();
        service
            .execute_command(
                &next(&context, "POLICY-DEFINE"),
                "ADDSD 'USER1.**' GENERIC OWNER(RACFADM) UACC(NONE)",
            )
            .unwrap();
        service
            .execute_command(
                &next(&context, "POLICY-PERMIT-READ"),
                "PERMIT 'USER1.**' CLASS(DATASET) ID(USER1) ACCESS(READ)",
            )
            .unwrap();
        let user = PrincipalId::new("USER1", InvocationLimits::default()).unwrap();
        let resource = ResourceName::new("USER1.DATA", 246).unwrap();
        assert_eq!(
            service
                .authorize(&user, "DATASET", &resource, AccessIntent::Read)
                .unwrap(),
            SecurityDecision::Allow
        );
        service
            .execute_command(
                &next(&context, "POLICY-RACLIST"),
                "SETROPTS RACLIST(DATASET)",
            )
            .unwrap();
        service
            .execute_command(
                &next(&context, "POLICY-PERMIT-UPDATE"),
                "PERMIT 'USER1.**' CLASS(DATASET) ID(USER1) ACCESS(UPDATE)",
            )
            .unwrap();
        assert_eq!(
            service
                .authorize(&user, "DATASET", &resource, AccessIntent::Update)
                .unwrap(),
            SecurityDecision::Deny,
            "RACLIST keeps the owned cached generation until refresh"
        );
        service
            .execute_command(
                &next(&context, "POLICY-REFRESH"),
                "SETROPTS RACLIST(DATASET) REFRESH",
            )
            .unwrap();
        assert_eq!(
            service
                .authorize(&user, "DATASET", &resource, AccessIntent::Update)
                .unwrap(),
            SecurityDecision::Allow
        );
        service
            .execute_command(
                &next(&context, "POLICY-INACTIVE"),
                "SETROPTS INACTIVE(DATASET)",
            )
            .unwrap();
        assert_eq!(
            service
                .authorize(&user, "DATASET", &resource, AccessIntent::Read)
                .unwrap(),
            SecurityDecision::Deny
        );
        service
            .execute_command(
                &next(&context, "POLICY-ACTIVE"),
                "SETROPTS CLASSACT(DATASET) RACLIST(DATASET) REFRESH",
            )
            .unwrap();
        service
            .execute_command(
                &next(&context, "POLICY-NOGENERIC"),
                "SETROPTS NOGENERIC(DATASET)",
            )
            .unwrap();
        assert_eq!(
            service
                .authorize(&user, "DATASET", &resource, AccessIntent::Read)
                .unwrap(),
            SecurityDecision::Deny
        );
        service
            .execute_command(
                &next(&context, "POLICY-GENERIC"),
                "SETROPTS GENERIC(DATASET)",
            )
            .unwrap();
        assert_eq!(
            service
                .authorize(&user, "DATASET", &resource, AccessIntent::Read)
                .unwrap(),
            SecurityDecision::Allow
        );
        service
            .execute_command(
                &next(&context, "POLICY-CREDENTIALS"),
                "SETROPTS PASSWORD(MINIMUM(10) MAXIMUM(80) HISTORY(4)) PHRASE(MINIMUM(20))",
            )
            .unwrap();
        let snapshot = service.database.read().unwrap();
        let policy = &snapshot.policy;
        assert_eq!(policy.password_minimum, 10);
        assert_eq!(policy.password_maximum, 80);
        assert_eq!(policy.phrase_minimum, 20);
        assert_eq!(policy.password_history, 4);
        assert_eq!(
            service
                .execute_command(
                    &next(&context, "POLICY-CREDENTIALS-INVALID"),
                    "SETROPTS PASSWORD(MINIMUM(90) MAXIMUM(20))",
                )
                .unwrap_err()
                .code,
            CommandDiagnosticCode::InvalidValue
        );
        let after = service.database.read().unwrap();
        assert_eq!(after.policy.password_minimum, 10);
        assert_eq!(after.policy.password_maximum, 80);
    }

    #[test]
    fn operations_program_control_and_all_seven_sec_503_families_execute() {
        use mainframe_env_host_api::{AccessIntent, ResourceName, SecurityDecision};

        let (service, context) = setup();
        service
            .execute_command(
                &context,
                "RDEFINE PROGRAM APP.LOAD OWNER(RACFADM) UACC(EXECUTE)",
            )
            .unwrap();
        let admin = context.actor().clone();
        let program = ResourceName::new("APP.LOAD", 246).unwrap();
        assert_eq!(
            service
                .authorize(&admin, "PROGRAM", &program, AccessIntent::Execute)
                .unwrap(),
            SecurityDecision::Deny
        );
        let commands = [
            "RACPRIV WRITEDOWN(ACTIVE)",
            "RACPRMCK MEMBER(IRROPT01 IRROPT02)",
            "SET TRACE AUTOAPPL",
            "SETROPTS PROGRAM RULES",
            "RVARY LIST",
            "STOP",
            "RESTART",
        ];
        let mut reached = BTreeSet::new();
        for (index, command) in commands.into_iter().enumerate() {
            let result = service
                .execute_command(&next(&context, &format!("SEC503-{index:02}")), command)
                .unwrap();
            reached.insert(result.family);
        }
        let expected = crate::command_descriptors()
            .iter()
            .filter(|descriptor| descriptor.work_package() == "SEC-503")
            .map(|descriptor| descriptor.family())
            .collect::<BTreeSet<_>>();
        assert_eq!(reached, expected);
        assert_eq!(expected.len(), 7);
        assert_eq!(
            service
                .authorize(&admin, "PROGRAM", &program, AccessIntent::Execute)
                .unwrap(),
            SecurityDecision::Allow
        );
        service
            .execute_command(&next(&context, "RVARY-INACTIVE"), "RVARY INACTIVE")
            .unwrap();
        assert_eq!(
            service
                .authorize(&admin, "PROGRAM", &program, AccessIntent::Execute)
                .unwrap(),
            SecurityDecision::Deny
        );
        service
            .execute_command(&next(&context, "RVARY-ACTIVE"), "RVARY ACTIVE")
            .unwrap();
        assert_eq!(
            service
                .execute_command(&next(&context, "PARMLIB-LIMIT"), "RACPRMCK MEMBER(A B C D)",)
                .unwrap_err()
                .code,
            CommandDiagnosticCode::InvalidValue
        );
        service
            .execute_command(
                &next(&context, "CUSTOM-DEFINE"),
                "RDEFINE CUSTOMCLS CUSTOM.RESOURCE OWNER(RACFADM)",
            )
            .unwrap();
        let class_result = service
            .execute_command(
                &next(&context, "CUSTOM-INACTIVE"),
                "SETROPTS INACTIVE(CUSTOMCLS)",
            )
            .unwrap();
        assert!(matches!(
            class_result.records.as_slice(),
            [
                CommandRecord::Policy { .. },
                CommandRecord::Class {
                    supplied: false,
                    active: false,
                    ..
                }
            ]
        ));
    }
}
