use crate::RacfService;
use crate::command::{
    CommandDiagnostic, CommandDiagnosticCode, CommandFamily, CommandLanguageLimits, ParsedCommand,
    ParsedOperand, diagnostic, parse_command,
};
use crate::model::{
    AccessControlEntry, AccessLevel, AuditFieldValue, AuditPolicy, ClassDescriptor,
    DecisionOutcome, DecisionReason, GroupAuthority, GroupConnection, GroupProfile, PrincipalKind,
    PrincipalProfile, PrincipalState, ProfileSegment, ProfileTemplate, ResourceProfile, SafStatus,
    SecurityAuditRecord, SecurityDatabaseSnapshot, SecurityTransaction, SegmentFieldKind,
    SegmentFieldSchema, SegmentTemplate, SegmentValue, TransactionState, connection_key,
    profile_key,
};
use mainframe_env_execution_api::PrincipalId;
use mainframe_env_host_api::HostProblem;
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CommandObjectKind {
    User,
    Group,
    Connection,
    DatasetProfile,
    ResourceProfile,
    Database,
}

#[derive(Clone, Debug, Eq, PartialEq)]
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

pub(crate) fn execute(
    service: &RacfService,
    context: &CommandContext,
    input: &str,
) -> Result<CommandResult, CommandDiagnostic> {
    let parsed = parse_command(input, CommandLanguageLimits::default())?;
    if parsed.descriptor.work_package() != "SEC-502" {
        return Err(diagnostic(CommandDiagnosticCode::UnsupportedFamily, 0));
    }
    let request_digest = format!("sha256:{:x}", Sha256::digest(input.as_bytes()));
    let family = parsed.descriptor.family();
    let ((outcome, replayed, status), generation) = service
        .database
        .mutate_retry(|snapshot| {
            if parsed.descriptor.mutating()
                && let Some(existing) = snapshot.transactions.get(context.idempotency_key())
            {
                if existing.request_digest != request_digest {
                    append_audit(
                        snapshot,
                        context,
                        &parsed,
                        DecisionOutcome::Deny,
                        status_for(DecisionReason::MalformedRequest),
                        &request_digest,
                    )?;
                    return Ok((
                        (
                            ExecutionOutcome::Failure(SemanticProblem::Conflict),
                            false,
                            status_for(DecisionReason::MalformedRequest),
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
                    let status = status_for(DecisionReason::Granted);
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
                    let status = status_for(reason_for_problem(&problem));
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

fn apply_mutation(
    service: &RacfService,
    snapshot: &mut SecurityDatabaseSnapshot,
    context: &CommandContext,
    command: &ParsedCommand,
) -> Result<Vec<CommandRecord>, SemanticProblem> {
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
        _ => Err(SemanticProblem::Invalid(0)),
    }
}

fn apply_query(
    snapshot: &SecurityDatabaseSnapshot,
    context: &CommandContext,
    command: &ParsedCommand,
) -> Result<Vec<CommandRecord>, SemanticProblem> {
    require_active(snapshot, context)?;
    match command.descriptor.family() {
        CommandFamily::Display => Ok(vec![CommandRecord::Summary {
            users: snapshot.principals.len(),
            groups: snapshot.groups.len(),
            profiles: snapshot.profiles.len(),
            generation: snapshot.generation,
        }]),
        CommandFamily::ListUser => list_users(snapshot, context, command),
        CommandFamily::ListGrp => list_groups(snapshot, command),
        CommandFamily::ListDsd => list_profiles(snapshot, "DATASET", command, 0),
        CommandFamily::Rlist => {
            let class = command.positional(0).ok_or(SemanticProblem::Invalid(0))?;
            list_profiles(snapshot, &upper_class(class)?, command, 1)
        }
        CommandFamily::Search => search(snapshot, command),
        _ => Err(SemanticProblem::Invalid(0)),
    }
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
    let mut principal = if let Some(secret) = secret_operand(command)? {
        service
            .password_principal_from_bytes(&user, secret.as_bytes())
            .map_err(|_| SemanticProblem::Invalid(secret_offset(command)))?
    } else {
        PrincipalProfile {
            id: user.clone(),
            kind: PrincipalKind::User,
            owner: user.clone(),
            default_group: None,
            state: PrincipalState::Active,
            credential: None,
            profile_template: None,
            segments: BTreeMap::new(),
            security_level: 0,
            security_label: None,
            categories: BTreeSet::new(),
            attributes: BTreeSet::new(),
            version: 1,
        }
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
    if !is_special(snapshot, context)
        && current.owner != context.actor().as_str()
        && user != context.actor().as_str()
    {
        return Err(SemanticProblem::Unauthorized);
    }
    let mut next = current;
    if let Some(owner) = operand_principal(command, "OWNER")? {
        require_principal_or_group(snapshot, &owner)?;
        next.owner = owner;
    }
    if let Some(group) = operand_principal(command, "DFLTGRP")? {
        require_group(snapshot, &group)?;
        next.default_group = Some(group);
    }
    if let Some(secret) = secret_operand(command)? {
        next.credential = service
            .password_principal_from_bytes(&user, secret.as_bytes())
            .map_err(|_| SemanticProblem::Invalid(secret_offset(command)))?
            .credential;
        next.state = PrincipalState::Active;
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
            || snapshot.acees.values().any(|acee| acee.principal == user)
            || snapshot.tokens.values().any(|token| token.owner == user)
        {
            return Err(SemanticProblem::Conflict);
        }
        snapshot.principals.remove(&user);
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
            access_list: vec![AccessControlEntry {
                principal: context.actor().as_str().into(),
                access: AccessLevel::Alter,
                when: None,
                audit: AuditPolicy::None,
            }],
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
                .retain(|entry| entry.principal != *id || entry.when.is_some());
            if !command.has_operand("DELETE") {
                next.access_list.push(AccessControlEntry {
                    principal: id.clone(),
                    access: access.expect("validated access"),
                    when: None,
                    audit: AuditPolicy::None,
                });
            }
        }
        next.access_list
            .sort_by(|left, right| left.principal.cmp(&right.principal));
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
    snapshot
        .classes
        .entry(class.into())
        .or_insert(ClassDescriptor {
            name: class.into(),
            supplied: matches!(class, "DATASET" | "FACILITY" | "PROGRAM"),
            active: true,
            generic_allowed: true,
            discrete_allowed: true,
            raclist: false,
            default_uacc: AccessLevel::None,
            max_profile_name_bytes: 246,
            posit: None,
            member_class: None,
            grouping_class: None,
            profile_template: "RESOURCE".into(),
            version: 1,
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
        fields: BTreeMap::from([
            (
                "COMMAND_FAMILY".into(),
                AuditFieldValue::Text(command.descriptor.keyword().into()),
            ),
            (
                "OFFICIAL_ROW".into(),
                AuditFieldValue::Text(command.descriptor.row_id().into()),
            ),
        ]),
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

fn status_for(reason: DecisionReason) -> SafStatus {
    match reason {
        DecisionReason::Granted => SafStatus {
            saf_return_code: 0,
            racf_return_code: 0,
            racf_reason_code: 0,
            reason,
        },
        DecisionReason::DefaultDeny | DecisionReason::InsufficientAccess => SafStatus {
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
        _ => SafStatus {
            saf_return_code: 8,
            racf_return_code: 8,
            racf_reason_code: 12,
            reason,
        },
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
    let code = match problem {
        HostProblem::Unauthorized => CommandDiagnosticCode::Unauthorized,
        HostProblem::NotFound => CommandDiagnosticCode::NotFound,
        HostProblem::IdempotencyConflict => CommandDiagnosticCode::Conflict,
        HostProblem::ResourceExhausted => CommandDiagnosticCode::ResourceExhausted,
        _ => CommandDiagnosticCode::ProviderFailure,
    };
    diagnostic(code, 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MemorySecretResolver, RacfLimits};
    use mainframe_env_execution_api::InvocationLimits;
    use mainframe_env_host_api::SecretRef;
    use mainframe_env_store::MemoryStore;
    use mainframe_env_store_api::ProviderStateStore;
    use std::sync::Arc;

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
    fn later_package_families_are_recognized_but_not_executed_by_sec_502() {
        let (service, context) = setup();
        for form in ["SETROPTS LIST", "RACDCERT LIST", "TARGET LIST"] {
            assert_ne!(
                crate::recognize_command(form, Default::default()).unwrap(),
                CommandFamily::AddUser
            );
            assert_eq!(
                service.execute_command(&context, form).unwrap_err().code,
                CommandDiagnosticCode::UnsupportedFamily
            );
        }
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
}
