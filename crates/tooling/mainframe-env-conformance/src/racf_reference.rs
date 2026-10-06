//! Independent bounded RACF/SAF development reference simulation.
//!
//! This module deliberately has no dependency on the provider under test. Its
//! tables are reviewed against the frozen catalogs and documented RACF/SAF
//! precedence and status-code rules. It is not licensed IBM differential
//! evidence and cannot emit an oracle receipt.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[cfg(test)]
const COMMAND_CATALOG: &str =
    include_str!("../../../../conformance/subsystems/racf/racf/command-language.json");
#[cfg(test)]
const REQUEST_CATALOG: &str =
    include_str!("../../../../conformance/subsystems/racf/racf/racroute.json");
const UNKNOWN_CASES: [&str; 4] = [
    "exact-cryptographic-material",
    "installation-exits",
    "rrsf-transport-timing",
    "undocumented-database-internals",
];

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct ReferenceObservation {
    pub surface: String,
    pub keyword: String,
    pub obligation: String,
    pub gate: String,
    pub outcome: String,
    pub status: Option<StatusObservation>,
    pub changed_domains: BTreeSet<String>,
    pub identity: Option<String>,
    pub audit: String,
    pub recovery: String,
    pub replay: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct StatusObservation {
    pub saf: u32,
    pub racf: u32,
    pub reason_code: u32,
    pub reason: String,
}

pub(crate) fn compare_observations(
    expected: &ReferenceObservation,
    actual: &ReferenceObservation,
) -> Result<(), String> {
    if expected == actual {
        Ok(())
    } else {
        Err(format!(
            "independent RACF observation mismatch: expected={expected:?} actual={actual:?}"
        ))
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum Access {
    None,
    Execute,
    Read,
    Update,
    Control,
    Alter,
}

impl Access {
    const ALL: [Self; 6] = [
        Self::None,
        Self::Execute,
        Self::Read,
        Self::Update,
        Self::Control,
        Self::Alter,
    ];
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Reason {
    Granted,
    DefaultDeny,
    ClassInactive,
    ProfileNotFound,
    PrincipalNotFound,
    PrincipalInactive,
    InsufficientAccess,
    LabelMismatch,
    ConditionNotSatisfied,
    TokenInvalid,
    AceeInvalid,
    CredentialInvalid,
    MfaInvalid,
    PolicyUnavailable,
    MalformedRequest,
    ResourceExhausted,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Status {
    saf: u32,
    racf: u32,
    reason_code: u32,
    reason: Reason,
}

fn status(reason: Reason) -> Status {
    let (saf, racf, reason_code) = match reason {
        Reason::Granted => (0, 0, 0),
        Reason::DefaultDeny
        | Reason::InsufficientAccess
        | Reason::ConditionNotSatisfied
        | Reason::LabelMismatch => (8, 8, 4),
        Reason::ProfileNotFound | Reason::PrincipalNotFound => (8, 8, 8),
        Reason::ResourceExhausted => (12, 12, 16),
        Reason::PolicyUnavailable => (12, 12, 20),
        Reason::ClassInactive
        | Reason::PrincipalInactive
        | Reason::TokenInvalid
        | Reason::AceeInvalid
        | Reason::CredentialInvalid
        | Reason::MfaInvalid
        | Reason::MalformedRequest => (8, 8, 12),
    };
    Status {
        saf,
        racf,
        reason_code,
        reason,
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct AccessFixture {
    policy_available: bool,
    subsystem_running: bool,
    database_active: bool,
    principal_exists: bool,
    principal_active: bool,
    class_active: bool,
    raclisted: bool,
    cache_present: bool,
    profile_present: bool,
    label_matches: bool,
    user_entry: Option<(Access, bool)>,
    group_entries: Vec<(Access, bool)>,
    uacc: Access,
    requested: Access,
}

impl Default for AccessFixture {
    fn default() -> Self {
        Self {
            policy_available: true,
            subsystem_running: true,
            database_active: true,
            principal_exists: true,
            principal_active: true,
            class_active: true,
            raclisted: false,
            cache_present: true,
            profile_present: true,
            label_matches: true,
            user_entry: Some((Access::Read, true)),
            group_entries: vec![(Access::Update, true)],
            uacc: Access::Execute,
            requested: Access::Read,
        }
    }
}

fn decide_access(input: &AccessFixture) -> Status {
    if !input.policy_available || !input.subsystem_running || !input.database_active {
        return status(Reason::PolicyUnavailable);
    }
    if !input.principal_exists {
        return status(Reason::PrincipalNotFound);
    }
    if !input.principal_active {
        return status(Reason::PrincipalInactive);
    }
    if !input.class_active {
        return status(Reason::ClassInactive);
    }
    if input.raclisted && !input.cache_present {
        return status(Reason::PolicyUnavailable);
    }
    if !input.profile_present {
        return status(Reason::ProfileNotFound);
    }
    if !input.label_matches {
        return status(Reason::LabelMismatch);
    }
    let selected = if let Some((access, condition)) = input.user_entry {
        if !condition {
            return status(Reason::ConditionNotSatisfied);
        }
        access
    } else {
        let mut selected = None;
        let mut conditional_miss = false;
        for (access, condition) in &input.group_entries {
            if *condition {
                selected = Some(selected.map_or(*access, |current: Access| current.max(*access)));
            } else {
                conditional_miss = true;
            }
        }
        match selected {
            Some(access) => access,
            None if conditional_miss => return status(Reason::ConditionNotSatisfied),
            None => input.uacc,
        }
    };
    if selected >= input.requested {
        status(Reason::Granted)
    } else {
        status(Reason::InsufficientAccess)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Authority {
    Active,
    SelfOrSpecial,
    OwnerOrSpecial,
    Special,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Transition {
    Query,
    CreateGroup,
    DeleteGroup,
    UpdateGroup,
    CreateUser,
    DeleteUser,
    UpdateUser,
    AddConnection,
    RemoveConnection,
    CreateProfile,
    DeleteProfile,
    UpdateProfile,
    Grant,
    Credential,
    Identity,
    Policy,
    Start,
    Stop,
    Operation,
}

#[derive(Clone, Copy, Debug)]
struct CommandCase {
    keyword: &'static str,
    authority: Authority,
    transition: Transition,
}

const COMMANDS: [CommandCase; 34] = [
    command("ADDGROUP", Authority::Special, Transition::CreateGroup),
    command(
        "ADDSD",
        Authority::OwnerOrSpecial,
        Transition::CreateProfile,
    ),
    command("ADDUSER", Authority::Special, Transition::CreateUser),
    command(
        "ALTDSD",
        Authority::OwnerOrSpecial,
        Transition::UpdateProfile,
    ),
    command(
        "ALTGROUP",
        Authority::OwnerOrSpecial,
        Transition::UpdateGroup,
    ),
    command("ALTUSER", Authority::SelfOrSpecial, Transition::UpdateUser),
    command(
        "CONNECT",
        Authority::OwnerOrSpecial,
        Transition::AddConnection,
    ),
    command(
        "DELDSD",
        Authority::OwnerOrSpecial,
        Transition::DeleteProfile,
    ),
    command("DELGROUP", Authority::Special, Transition::DeleteGroup),
    command("DELUSER", Authority::Special, Transition::DeleteUser),
    command("DISPLAY", Authority::Active, Transition::Query),
    command("LISTDSD", Authority::Active, Transition::Query),
    command("LISTGRP", Authority::Active, Transition::Query),
    command("LISTUSER", Authority::Active, Transition::Query),
    command("PASSWORD", Authority::SelfOrSpecial, Transition::Credential),
    command("PERMIT", Authority::OwnerOrSpecial, Transition::Grant),
    command("RACDCERT", Authority::SelfOrSpecial, Transition::Identity),
    command("RACLINK", Authority::SelfOrSpecial, Transition::Identity),
    command("RACMAP", Authority::SelfOrSpecial, Transition::Identity),
    command("RACPRIV", Authority::Special, Transition::Policy),
    command("RACPRMCK", Authority::Special, Transition::Query),
    command(
        "RALTER",
        Authority::OwnerOrSpecial,
        Transition::UpdateProfile,
    ),
    command(
        "RDEFINE",
        Authority::OwnerOrSpecial,
        Transition::CreateProfile,
    ),
    command(
        "RDELETE",
        Authority::OwnerOrSpecial,
        Transition::DeleteProfile,
    ),
    command(
        "REMOVE",
        Authority::OwnerOrSpecial,
        Transition::RemoveConnection,
    ),
    command("RESTART", Authority::Special, Transition::Start),
    command("RLIST", Authority::Active, Transition::Query),
    command("RVARY", Authority::Special, Transition::Operation),
    command("SEARCH", Authority::Active, Transition::Query),
    command("SET", Authority::Special, Transition::Policy),
    command("SETROPTS", Authority::Special, Transition::Policy),
    command("SIGNOFF", Authority::SelfOrSpecial, Transition::Identity),
    command("STOP", Authority::Special, Transition::Stop),
    command("TARGET", Authority::Special, Transition::Identity),
];

const fn command(
    keyword: &'static str,
    authority: Authority,
    transition: Transition,
) -> CommandCase {
    CommandCase {
        keyword,
        authority,
        transition,
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct RefState {
    generation: u64,
    users: u32,
    groups: u32,
    profiles: u32,
    connections: u32,
    permits: u32,
    credentials: u32,
    identities: u32,
    acees: u32,
    tokens: u32,
    sessions: u32,
    mfa_required: bool,
    mfa_valid: bool,
    cache_generation: u64,
    policy_generation: u64,
    running: bool,
    audits: Vec<BTreeMap<String, String>>,
}

impl Default for RefState {
    fn default() -> Self {
        Self {
            generation: 1,
            users: 8,
            groups: 8,
            profiles: 8,
            connections: 8,
            permits: 8,
            credentials: 8,
            identities: 8,
            acees: 2,
            tokens: 2,
            sessions: 1,
            mfa_required: false,
            mfa_valid: true,
            cache_generation: 1,
            policy_generation: 1,
            running: true,
            audits: Vec::new(),
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Actor {
    exists: bool,
    active: bool,
    self_profile: bool,
    owns_target: bool,
    special: bool,
}

impl Actor {
    const AUTHORIZED: Self = Self {
        exists: true,
        active: true,
        self_profile: true,
        owns_target: true,
        special: true,
    };
    const MISSING: Self = Self {
        exists: false,
        active: false,
        self_profile: false,
        owns_target: false,
        special: false,
    };
}

fn execute_command(
    state: &mut RefState,
    case: CommandCase,
    actor: Actor,
    malformed: bool,
    fields: BTreeMap<String, String>,
) -> Status {
    if malformed {
        return status(Reason::MalformedRequest);
    }
    let before = protected_shape(state);
    let authorized = actor.exists
        && actor.active
        && match case.authority {
            Authority::Active => true,
            Authority::SelfOrSpecial => actor.self_profile || actor.special,
            Authority::OwnerOrSpecial => actor.owns_target || actor.special,
            Authority::Special => actor.special,
        };
    if !authorized {
        append_audit(state, case.keyword, false, fields);
        debug_assert_eq!(before, protected_shape(state));
        return status(if actor.exists {
            Reason::DefaultDeny
        } else {
            Reason::PrincipalNotFound
        });
    }
    apply_transition(state, case.transition);
    state.generation += 1;
    append_audit(state, case.keyword, true, fields);
    status(Reason::Granted)
}

fn apply_transition(state: &mut RefState, transition: Transition) {
    match transition {
        Transition::Query => {}
        Transition::CreateGroup => state.groups += 1,
        Transition::DeleteGroup => state.groups = state.groups.saturating_sub(1),
        Transition::UpdateGroup => state.policy_generation += 1,
        Transition::CreateUser => state.users += 1,
        Transition::DeleteUser => state.users = state.users.saturating_sub(1),
        Transition::UpdateUser => state.policy_generation += 1,
        Transition::AddConnection => state.connections += 1,
        Transition::RemoveConnection => {
            state.connections = state.connections.saturating_sub(1);
        }
        Transition::CreateProfile => state.profiles += 1,
        Transition::DeleteProfile => state.profiles = state.profiles.saturating_sub(1),
        Transition::UpdateProfile => state.policy_generation += 1,
        Transition::Grant => state.permits += 1,
        Transition::Credential => state.credentials += 1,
        Transition::Identity => state.identities += 1,
        Transition::Policy | Transition::Operation => state.policy_generation += 1,
        Transition::Start => state.running = true,
        Transition::Stop => state.running = false,
    }
}

#[derive(Clone, Copy, Debug)]
enum RequestTransition {
    Audit,
    Access,
    Define,
    Dirauth,
    Extract,
    List,
    Signon,
    Stat,
    TokenBuild,
    TokenMap,
    TokenExtract,
    Verify,
    Verifyx,
}

#[derive(Clone, Copy, Debug)]
struct RequestCase {
    keyword: &'static str,
    requires_acee: bool,
    transition: RequestTransition,
}

const REQUESTS: [RequestCase; 14] = [
    request("AUDIT", false, RequestTransition::Audit),
    request("AUTH", false, RequestTransition::Access),
    request("DEFINE", true, RequestTransition::Define),
    request("DIRAUTH", true, RequestTransition::Dirauth),
    request("EXTRACT", true, RequestTransition::Extract),
    request("FASTAUTH", true, RequestTransition::Access),
    request("LIST", true, RequestTransition::List),
    request("SIGNON", false, RequestTransition::Signon),
    request("STAT", true, RequestTransition::Stat),
    request("TOKENBLD", true, RequestTransition::TokenBuild),
    request("TOKENMAP", false, RequestTransition::TokenMap),
    request("TOKENXTR", true, RequestTransition::TokenExtract),
    request("VERIFY", false, RequestTransition::Verify),
    request("VERIFYX", false, RequestTransition::Verifyx),
];

const fn request(
    keyword: &'static str,
    requires_acee: bool,
    transition: RequestTransition,
) -> RequestCase {
    RequestCase {
        keyword,
        requires_acee,
        transition,
    }
}

fn execute_request(
    state: &mut RefState,
    case: RequestCase,
    actor: Actor,
    acee_valid: bool,
    malformed: bool,
    fields: BTreeMap<String, String>,
) -> Status {
    if malformed {
        append_audit(state, case.keyword, false, fields);
        return status(Reason::MalformedRequest);
    }
    let before = protected_shape(state);
    if !actor.exists {
        append_audit(state, case.keyword, false, fields);
        debug_assert_eq!(before, protected_shape(state));
        return status(Reason::PrincipalNotFound);
    }
    if !actor.active {
        append_audit(state, case.keyword, false, fields);
        return status(Reason::PrincipalInactive);
    }
    if case.requires_acee && !acee_valid {
        append_audit(state, case.keyword, false, fields);
        return status(Reason::AceeInvalid);
    }
    let result = match case.transition {
        RequestTransition::Access | RequestTransition::Dirauth => {
            decide_access(&AccessFixture::default())
        }
        RequestTransition::TokenMap | RequestTransition::TokenExtract if state.tokens == 0 => {
            status(Reason::TokenInvalid)
        }
        RequestTransition::Define => {
            state.profiles += 1;
            status(Reason::Granted)
        }
        RequestTransition::List => {
            state.cache_generation += 1;
            status(Reason::Granted)
        }
        RequestTransition::Signon => {
            if state.credentials == 0 {
                status(Reason::CredentialInvalid)
            } else {
                state.acees += 1;
                state.sessions += 1;
                status(Reason::Granted)
            }
        }
        RequestTransition::TokenBuild => {
            state.tokens += 1;
            status(Reason::Granted)
        }
        RequestTransition::TokenMap => status(Reason::Granted),
        RequestTransition::Verifyx => {
            if state.credentials == 0 {
                status(Reason::CredentialInvalid)
            } else if state.mfa_required && !state.mfa_valid {
                status(Reason::MfaInvalid)
            } else {
                state.acees += 1;
                status(Reason::Granted)
            }
        }
        RequestTransition::Audit
        | RequestTransition::Extract
        | RequestTransition::Stat
        | RequestTransition::TokenExtract => status(Reason::Granted),
        RequestTransition::Verify if state.credentials == 0 => status(Reason::CredentialInvalid),
        RequestTransition::Verify => status(Reason::Granted),
    };
    append_audit(
        state,
        case.keyword,
        result.reason == Reason::Granted,
        fields,
    );
    result
}

fn append_audit(
    state: &mut RefState,
    action: &str,
    allowed: bool,
    mut fields: BTreeMap<String, String>,
) {
    for (name, value) in &mut fields {
        if sensitive_name(name) || sensitive_value(value) {
            *value = "<redacted>".into();
        }
    }
    fields.insert("ACTION".into(), action.into());
    fields.insert(
        "DECISION".into(),
        if allowed { "ALLOW" } else { "DENY" }.into(),
    );
    state.audits.push(fields);
}

fn sensitive_name(name: &str) -> bool {
    let name = name.to_ascii_uppercase();
    [
        "PASSWORD",
        "PHRASE",
        "CREDENTIAL",
        "SECRET",
        "TOKEN",
        "KEY",
        "CERTIFICATE",
        "PASSCODE",
        "MFA",
        "ASSERTION",
    ]
    .iter()
    .any(|marker| name.contains(marker))
}

fn sensitive_value(value: &str) -> bool {
    let upper = value.to_ascii_uppercase();
    value.starts_with("secret:")
        || value.starts_with("vault:")
        || value.starts_with("$argon2")
        || upper.contains("BEGIN PRIVATE KEY")
        || upper.contains("BEGIN CERTIFICATE")
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ProtectedShape {
    users: u32,
    groups: u32,
    profiles: u32,
    connections: u32,
    permits: u32,
    credentials: u32,
    identities: u32,
    acees: u32,
    tokens: u32,
    sessions: u32,
    mfa_required: bool,
    mfa_valid: bool,
    cache_generation: u64,
    policy_generation: u64,
    running: bool,
}

fn protected_shape(state: &RefState) -> ProtectedShape {
    ProtectedShape {
        users: state.users,
        groups: state.groups,
        profiles: state.profiles,
        connections: state.connections,
        permits: state.permits,
        credentials: state.credentials,
        identities: state.identities,
        acees: state.acees,
        tokens: state.tokens,
        sessions: state.sessions,
        mfa_required: state.mfa_required,
        mfa_valid: state.mfa_valid,
        cache_generation: state.cache_generation,
        policy_generation: state.policy_generation,
        running: state.running,
    }
}

fn safe_fields() -> BTreeMap<String, String> {
    BTreeMap::from([
        ("PASSWORD".into(), "never-retain".into()),
        ("NOTE".into(), "secret:opaque".into()),
        ("TERMINAL".into(), "L7001".into()),
    ])
}

pub(crate) fn verify_binding(
    surface: &str,
    row_id: &str,
    keyword: &str,
    obligation: &str,
    gate: &str,
) -> Result<(), String> {
    debug_assert_eq!(UNKNOWN_CASES.len(), 4);
    debug_assert_eq!(Access::ALL.len(), 6);
    debug_assert_eq!(status(Reason::ResourceExhausted).saf, 12);
    if gate == "differential" {
        return Err("reference simulation cannot satisfy licensed differential".into());
    }
    match surface {
        "command" => {
            let (index, case) = COMMANDS
                .iter()
                .enumerate()
                .find(|(_, case)| case.keyword == keyword)
                .ok_or_else(|| format!("reference command is missing: {keyword}"))?;
            require_row(row_id, "racf-command-families", index + 1)?;
            verify_command_obligation(*case, obligation)
        }
        "racroute" => {
            let (index, case) = REQUESTS
                .iter()
                .enumerate()
                .find(|(_, case)| case.keyword == keyword)
                .ok_or_else(|| format!("reference request is missing: {keyword}"))?;
            require_row(row_id, "racroute-request-types", index + 1)?;
            verify_request_obligation(*case, obligation)
        }
        _ => Err("reference surface is unknown".into()),
    }
}

pub(crate) fn expected_observation(
    surface: &str,
    row_id: &str,
    keyword: &str,
    obligation: &str,
    gate: &str,
) -> Result<ReferenceObservation, String> {
    verify_binding(surface, row_id, keyword, obligation, gate)?;
    match surface {
        "command" => expected_command_observation(keyword, obligation, gate),
        "racroute" => expected_request_observation(keyword, obligation, gate),
        _ => Err("reference observation surface is unknown".into()),
    }
}

fn expected_command_observation(
    keyword: &str,
    obligation: &str,
    gate: &str,
) -> Result<ReferenceObservation, String> {
    let (outcome, status, audit, recovery, replay, semantic) = match obligation {
        "syntax" if gate == "recognized" => {
            ("recognized", None, "none", "none", "not-applicable", false)
        }
        "syntax" if gate == "validated" => {
            ("validated", None, "none", "none", "not-applicable", false)
        }
        "authorized" => (
            "granted",
            Some(status_observation(status(Reason::Granted))),
            "allow-redacted",
            "none",
            "not-applicable",
            true,
        ),
        "unauthorized" => (
            "denied",
            Some(status_observation(status(Reason::DefaultDeny))),
            "deny-redacted",
            "none",
            "not-applicable",
            false,
        ),
        "malformed" => (
            "diagnostic:MERSEC1012E",
            None,
            "none",
            "none",
            "not-applicable",
            false,
        ),
        "bounded-limit" => (
            "diagnostic:MERSEC1002E",
            None,
            "none",
            "none",
            "not-applicable",
            false,
        ),
        "audit-redaction" => (
            "granted",
            Some(status_observation(status(Reason::Granted))),
            "allow-redacted",
            "none",
            "not-applicable",
            true,
        ),
        "atomic-retry" => (
            "granted",
            Some(status_observation(status(Reason::Granted))),
            "allow-redacted",
            "atomic-retry-stable",
            "not-applicable",
            true,
        ),
        "restart-recovery" => (
            "granted",
            Some(status_observation(status(Reason::Granted))),
            "allow-redacted",
            "restart-stable",
            if command_mutating(keyword) {
                "state-and-audit-stable"
            } else {
                "not-applicable"
            },
            true,
        ),
        _ => {
            return Err(format!(
                "unsupported reference command observation {obligation}/{gate}"
            ));
        }
    };
    Ok(ReferenceObservation {
        surface: "command".into(),
        keyword: keyword.into(),
        obligation: obligation.into(),
        gate: gate.into(),
        outcome: outcome.into(),
        status,
        changed_domains: if semantic {
            command_domains(keyword)
        } else {
            BTreeSet::new()
        },
        identity: semantic.then(|| command_identity(keyword).to_string()),
        audit: audit.into(),
        recovery: recovery.into(),
        replay: replay.into(),
    })
}

fn expected_request_observation(
    keyword: &str,
    obligation: &str,
    gate: &str,
) -> Result<ReferenceObservation, String> {
    let credential_negative = matches!(keyword, "SIGNON" | "VERIFY" | "VERIFYX");
    let (outcome, status, audit, recovery, replay, semantic) = match obligation {
        "syntax" if gate == "recognized" => {
            ("recognized", None, "none", "none", "not-applicable", false)
        }
        "syntax" if gate == "validated" => {
            ("validated", None, "none", "none", "not-applicable", false)
        }
        "authorized" => (
            "granted",
            Some(status_observation(status(Reason::Granted))),
            "allow-redacted",
            "none",
            if request_mutating(keyword) {
                "exact-terminal-no-duplicate"
            } else {
                "not-applicable"
            },
            true,
        ),
        "unauthorized" => (
            "denied",
            Some(status_observation(status(Reason::PrincipalNotFound))),
            "deny-redacted",
            "none",
            if request_mutating(keyword) {
                "terminal-denial-no-duplicate"
            } else {
                "not-applicable"
            },
            false,
        ),
        "malformed" | "bounded-limit" => (
            if credential_negative {
                "credential-invalid"
            } else {
                "malformed-request"
            },
            Some(status_observation(status(if credential_negative {
                Reason::CredentialInvalid
            } else {
                Reason::MalformedRequest
            }))),
            "deny-redacted",
            "none",
            if request_mutating(keyword) {
                "terminal-denial-no-duplicate"
            } else {
                "not-applicable"
            },
            false,
        ),
        "audit-redaction" => (
            "granted",
            Some(status_observation(status(Reason::Granted))),
            "allow-redacted",
            "none",
            if request_mutating(keyword) {
                "exact-terminal-no-duplicate"
            } else {
                "not-applicable"
            },
            true,
        ),
        "atomic-retry" => (
            "granted",
            Some(status_observation(status(Reason::Granted))),
            "allow-redacted",
            "atomic-retry-stable",
            if request_mutating(keyword) {
                "exact-terminal-no-duplicate"
            } else {
                "not-applicable"
            },
            true,
        ),
        "restart-recovery" => (
            "granted",
            Some(status_observation(status(Reason::Granted))),
            "allow-redacted",
            "restart-stable",
            if request_mutating(keyword) {
                "exact-terminal-no-duplicate"
            } else {
                "not-applicable"
            },
            true,
        ),
        _ => {
            return Err(format!(
                "unsupported reference request observation {obligation}/{gate}"
            ));
        }
    };
    Ok(ReferenceObservation {
        surface: "racroute".into(),
        keyword: keyword.into(),
        obligation: obligation.into(),
        gate: gate.into(),
        outcome: outcome.into(),
        status,
        changed_domains: if semantic {
            request_domains(keyword)
        } else {
            BTreeSet::new()
        },
        identity: semantic.then(|| request_identity(keyword).to_string()),
        audit: audit.into(),
        recovery: recovery.into(),
        replay: replay.into(),
    })
}

fn status_observation(value: Status) -> StatusObservation {
    StatusObservation {
        saf: value.saf,
        racf: value.racf,
        reason_code: value.reason_code,
        reason: reason_slug(value.reason).into(),
    }
}

fn reason_slug(reason: Reason) -> &'static str {
    match reason {
        Reason::Granted => "granted",
        Reason::DefaultDeny => "default-deny",
        Reason::ClassInactive => "class-inactive",
        Reason::ProfileNotFound => "profile-not-found",
        Reason::PrincipalNotFound => "principal-not-found",
        Reason::PrincipalInactive => "principal-inactive",
        Reason::InsufficientAccess => "insufficient-access",
        Reason::LabelMismatch => "security-label-mismatch",
        Reason::ConditionNotSatisfied => "condition-not-satisfied",
        Reason::TokenInvalid => "token-invalid",
        Reason::AceeInvalid => "acee-invalid",
        Reason::CredentialInvalid => "credential-invalid",
        Reason::MfaInvalid => "mfa-invalid",
        Reason::PolicyUnavailable => "policy-unavailable",
        Reason::MalformedRequest => "malformed-request",
        Reason::ResourceExhausted => "resource-exhausted",
    }
}

fn command_domains(keyword: &str) -> BTreeSet<String> {
    let domains: &[&str] = match keyword {
        "ADDGROUP" | "DELGROUP" => &["groups"],
        "ALTGROUP" => &["classes", "groups"],
        "ADDUSER" | "DELUSER" => &["connections", "principals"],
        "ALTUSER" => &["classes", "principals"],
        "CONNECT" | "REMOVE" => &["connections"],
        "ADDSD" | "ALTDSD" | "DELDSD" | "PERMIT" | "RALTER" | "RDEFINE" | "RDELETE" => {
            &["profiles"]
        }
        "PASSWORD" => &["principals"],
        "RACDCERT" => &["certificates"],
        "RACLINK" => &["user-associations"],
        "RACMAP" => &["identity-mappings"],
        "RACPRIV" | "RESTART" | "SET" | "SETROPTS" | "STOP" => &["policy"],
        "TARGET" => &["rrsf-nodes"],
        _ => &[],
    };
    domains.iter().map(|domain| (*domain).into()).collect()
}

fn command_identity(keyword: &str) -> &'static str {
    match keyword {
        "ADDGROUP" | "ALTGROUP" | "DELGROUP" => "group",
        "ADDSD" | "ALTDSD" | "DELDSD" | "PERMIT" => "dataset-profile",
        "ADDUSER" | "ALTUSER" | "DELUSER" | "PASSWORD" => "user",
        "CONNECT" | "REMOVE" => "connection",
        "DISPLAY" => "summary",
        "LISTDSD" | "RLIST" | "SEARCH" => "profile",
        "LISTGRP" => "group-record",
        "LISTUSER" => "user-record",
        "RACDCERT" => "certificate",
        "RACLINK" => "association",
        "RACMAP" => "identity-mapping",
        "RACPRIV" | "RESTART" | "RVARY" | "SET" | "SETROPTS" | "STOP" => "policy",
        "RACPRMCK" => "database",
        "RALTER" | "RDEFINE" | "RDELETE" => "resource-profile",
        "SIGNOFF" => "none",
        "TARGET" => "rrsf-node",
        _ => "none",
    }
}

fn command_mutating(keyword: &str) -> bool {
    !matches!(
        keyword,
        "DISPLAY" | "LISTDSD" | "LISTGRP" | "LISTUSER" | "RACPRMCK" | "RLIST" | "SEARCH"
    )
}

fn request_domains(keyword: &str) -> BTreeSet<String> {
    let domains: &[&str] = match keyword {
        "DEFINE" => &["profiles"],
        "LIST" => &["caches", "classes"],
        "SIGNON" => &["acees", "sessions"],
        "TOKENBLD" => &["tokens"],
        "TOKENMAP" | "VERIFYX" => &["acees"],
        _ => &[],
    };
    domains.iter().map(|domain| (*domain).into()).collect()
}

fn request_identity(keyword: &str) -> &'static str {
    match keyword {
        "AUDIT" => "audit",
        "AUTH" | "DIRAUTH" | "FASTAUTH" => "decision",
        "DEFINE" => "defined",
        "VERIFY" | "VERIFYX" => "verified",
        "EXTRACT" => "extracted-user",
        "LIST" => "listed",
        "SIGNON" => "signed-on",
        "STAT" => "statistics",
        "TOKENBLD" => "token-built",
        "TOKENMAP" => "token-mapped",
        "TOKENXTR" => "token-extracted",
        _ => "none",
    }
}

fn request_mutating(keyword: &str) -> bool {
    matches!(
        keyword,
        "AUDIT" | "DEFINE" | "LIST" | "SIGNON" | "TOKENBLD" | "TOKENMAP" | "VERIFY" | "VERIFYX"
    )
}

fn verify_command_obligation(case: CommandCase, obligation: &str) -> Result<(), String> {
    let mut state = RefState::default();
    let before = state.clone();
    match obligation {
        "syntax" => Ok(()),
        "authorized" => require_granted(execute_command(
            &mut state,
            case,
            Actor::AUTHORIZED,
            false,
            BTreeMap::new(),
        )),
        "unauthorized" => {
            let outcome = execute_command(&mut state, case, Actor::MISSING, false, BTreeMap::new());
            if outcome.reason == Reason::Granted
                || protected_shape(&state) != protected_shape(&before)
                || state.audits.len() != 1
            {
                Err("reference command authorization bypassed denial".into())
            } else {
                Ok(())
            }
        }
        "malformed" | "bounded-limit" => {
            let outcome =
                execute_command(&mut state, case, Actor::AUTHORIZED, true, BTreeMap::new());
            if outcome.reason == Reason::MalformedRequest
                && protected_shape(&state) == protected_shape(&before)
            {
                Ok(())
            } else {
                Err("reference command malformed/limit path mutated state".into())
            }
        }
        "audit-redaction" => {
            require_granted(execute_command(
                &mut state,
                case,
                Actor::AUTHORIZED,
                false,
                safe_fields(),
            ))?;
            require_redacted(&state)
        }
        "restart-recovery" => {
            require_granted(execute_command(
                &mut state,
                case,
                Actor::AUTHORIZED,
                false,
                BTreeMap::new(),
            ))?;
            let restarted = restart_state(&state)?;
            if restarted == state {
                Ok(())
            } else {
                Err("reference command restart drifted".into())
            }
        }
        "atomic-retry" => require_command_commutes(case),
        _ => Err(format!(
            "reference command obligation is unknown: {obligation}"
        )),
    }
}

fn verify_request_obligation(case: RequestCase, obligation: &str) -> Result<(), String> {
    let mut state = RefState::default();
    let before = state.clone();
    match obligation {
        "syntax" => Ok(()),
        "authorized" => require_granted(execute_request(
            &mut state,
            case,
            Actor::AUTHORIZED,
            true,
            false,
            BTreeMap::new(),
        )),
        "unauthorized" => {
            let outcome = execute_request(
                &mut state,
                case,
                Actor::MISSING,
                false,
                false,
                BTreeMap::new(),
            );
            if outcome.reason == Reason::Granted
                || protected_shape(&state) != protected_shape(&before)
                || state.audits.len() != 1
            {
                Err("reference request authorization bypassed denial".into())
            } else {
                Ok(())
            }
        }
        "malformed" | "bounded-limit" => {
            if matches!(
                case.transition,
                RequestTransition::Signon | RequestTransition::Verify | RequestTransition::Verifyx
            ) {
                state.credentials = 0;
            }
            let credential_negative = state.credentials == 0;
            let negative_before = protected_shape(&state);
            let outcome = execute_request(
                &mut state,
                case,
                Actor::AUTHORIZED,
                true,
                !credential_negative,
                BTreeMap::new(),
            );
            if outcome.reason
                == if credential_negative {
                    Reason::CredentialInvalid
                } else {
                    Reason::MalformedRequest
                }
                && protected_shape(&state) == negative_before
            {
                Ok(())
            } else {
                Err("reference request malformed/limit path mutated state".into())
            }
        }
        "audit-redaction" => {
            require_granted(execute_request(
                &mut state,
                case,
                Actor::AUTHORIZED,
                true,
                false,
                safe_fields(),
            ))?;
            require_redacted(&state)
        }
        "restart-recovery" => {
            require_granted(execute_request(
                &mut state,
                case,
                Actor::AUTHORIZED,
                true,
                false,
                BTreeMap::new(),
            ))?;
            let restarted = restart_state(&state)?;
            if restarted == state {
                Ok(())
            } else {
                Err("reference request restart drifted".into())
            }
        }
        "atomic-retry" => require_request_commutes(case),
        _ => Err(format!(
            "reference request obligation is unknown: {obligation}"
        )),
    }
}

fn require_granted(outcome: Status) -> Result<(), String> {
    if outcome == status(Reason::Granted) {
        Ok(())
    } else {
        Err(format!("reference outcome was not granted: {outcome:?}"))
    }
}

fn restart_state(state: &RefState) -> Result<RefState, String> {
    let durable = serde_json::to_vec(state)
        .map_err(|error| format!("reference restart encode failed: {error}"))?;
    serde_json::from_slice(&durable)
        .map_err(|error| format!("reference restart decode failed: {error}"))
}

fn require_redacted(state: &RefState) -> Result<(), String> {
    let audit = state
        .audits
        .last()
        .ok_or("reference audit was not emitted")?;
    if audit.get("PASSWORD").map(String::as_str) == Some("<redacted>")
        && audit.get("NOTE").map(String::as_str) == Some("<redacted>")
        && audit.get("TERMINAL").map(String::as_str) == Some("L7001")
    {
        Ok(())
    } else {
        Err("reference audit retained sensitive material".into())
    }
}

fn require_command_commutes(case: CommandCase) -> Result<(), String> {
    let mut left = RefState::default();
    let mut right = left.clone();
    require_granted(execute_command(
        &mut left,
        case,
        Actor::AUTHORIZED,
        false,
        BTreeMap::new(),
    ))?;
    left.groups += 1;
    right.groups += 1;
    require_granted(execute_command(
        &mut right,
        case,
        Actor::AUTHORIZED,
        false,
        BTreeMap::new(),
    ))?;
    if protected_shape(&left) == protected_shape(&right) {
        Ok(())
    } else {
        Err("reference command transitions do not commute under retry".into())
    }
}

fn require_request_commutes(case: RequestCase) -> Result<(), String> {
    let mut left = RefState::default();
    let mut right = left.clone();
    require_granted(execute_request(
        &mut left,
        case,
        Actor::AUTHORIZED,
        true,
        false,
        BTreeMap::new(),
    ))?;
    left.groups += 1;
    right.groups += 1;
    require_granted(execute_request(
        &mut right,
        case,
        Actor::AUTHORIZED,
        true,
        false,
        BTreeMap::new(),
    ))?;
    if protected_shape(&left) == protected_shape(&right) {
        Ok(())
    } else {
        Err("reference request transitions do not commute under retry".into())
    }
}

fn require_row(row_id: &str, family: &str, sequence: usize) -> Result<(), String> {
    let suffix = format!(":{family}:{sequence:04}");
    if row_id == format!("ibm-zos-3.2-racf-saf-2026{suffix}") {
        Ok(())
    } else {
        Err(format!("reference row identity drifted: {row_id}"))
    }
}

#[cfg(test)]
#[derive(Deserialize)]
struct CommandCatalog {
    families: Vec<CatalogCommand>,
}

#[cfg(test)]
#[derive(Deserialize)]
struct CatalogCommand {
    row_id: String,
    keyword: String,
}

#[cfg(test)]
#[derive(Deserialize)]
struct RequestCatalog {
    requests: Vec<CatalogRequest>,
}

#[cfg(test)]
#[derive(Deserialize)]
struct CatalogRequest {
    row_id: String,
    keyword: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn independent_tables_are_exact_against_frozen_catalogs() {
        let commands: CommandCatalog = serde_json::from_str(COMMAND_CATALOG).unwrap();
        let requests: RequestCatalog = serde_json::from_str(REQUEST_CATALOG).unwrap();
        assert_eq!(commands.families.len(), COMMANDS.len());
        assert_eq!(requests.requests.len(), REQUESTS.len());
        for (index, (catalog, reference)) in commands.families.iter().zip(COMMANDS).enumerate() {
            assert_eq!(catalog.keyword, reference.keyword);
            require_row(&catalog.row_id, "racf-command-families", index + 1).unwrap();
        }
        for (index, (catalog, reference)) in requests.requests.iter().zip(REQUESTS).enumerate() {
            assert_eq!(catalog.keyword, reference.keyword);
            require_row(&catalog.row_id, "racroute-request-types", index + 1).unwrap();
        }
    }

    #[test]
    fn precedence_properties_cover_user_group_uacc_conditions_and_labels() {
        for user in Access::ALL {
            for group in Access::ALL {
                for uacc in Access::ALL {
                    for requested in Access::ALL {
                        let direct = AccessFixture {
                            user_entry: Some((user, true)),
                            group_entries: vec![(group, true)],
                            uacc,
                            requested,
                            ..Default::default()
                        };
                        assert_eq!(
                            decide_access(&direct).reason == Reason::Granted,
                            user >= requested,
                            "a direct user entry must override group and UACC"
                        );
                        let group_only = AccessFixture {
                            user_entry: None,
                            group_entries: vec![(group, true)],
                            uacc,
                            requested,
                            ..Default::default()
                        };
                        assert_eq!(
                            decide_access(&group_only).reason == Reason::Granted,
                            group >= requested,
                            "group access must override UACC"
                        );
                    }
                }
            }
        }
        let conditional = AccessFixture {
            user_entry: Some((Access::Alter, false)),
            ..Default::default()
        };
        assert_eq!(
            decide_access(&conditional),
            status(Reason::ConditionNotSatisfied)
        );
        let label = AccessFixture {
            label_matches: false,
            ..Default::default()
        };
        assert_eq!(decide_access(&label), status(Reason::LabelMismatch));
    }

    #[test]
    fn fail_closed_precedence_and_code_table_are_exact() {
        let cases = [
            (
                AccessFixture {
                    policy_available: false,
                    ..Default::default()
                },
                Reason::PolicyUnavailable,
            ),
            (
                AccessFixture {
                    principal_exists: false,
                    ..Default::default()
                },
                Reason::PrincipalNotFound,
            ),
            (
                AccessFixture {
                    class_active: false,
                    ..Default::default()
                },
                Reason::ClassInactive,
            ),
            (
                AccessFixture {
                    raclisted: true,
                    cache_present: false,
                    ..Default::default()
                },
                Reason::PolicyUnavailable,
            ),
            (
                AccessFixture {
                    profile_present: false,
                    ..Default::default()
                },
                Reason::ProfileNotFound,
            ),
        ];
        for (fixture, reason) in cases {
            assert_eq!(decide_access(&fixture), status(reason));
        }
        assert_eq!(
            status(Reason::Granted),
            Status {
                saf: 0,
                racf: 0,
                reason_code: 0,
                reason: Reason::Granted
            }
        );
        assert_eq!(status(Reason::InsufficientAccess).reason_code, 4);
        assert_eq!(status(Reason::PrincipalNotFound).reason_code, 8);
        assert_eq!(status(Reason::MalformedRequest).reason_code, 12);
        assert_eq!(status(Reason::ResourceExhausted).saf, 12);
        assert_eq!(status(Reason::PolicyUnavailable).reason_code, 20);
    }

    #[test]
    fn token_acee_mfa_and_cache_invariants_fail_closed() {
        let mut no_token = RefState {
            tokens: 0,
            ..Default::default()
        };
        assert_eq!(
            execute_request(
                &mut no_token,
                REQUESTS[10],
                Actor::AUTHORIZED,
                true,
                false,
                BTreeMap::new(),
            ),
            status(Reason::TokenInvalid)
        );

        let mut bad_acee = RefState::default();
        assert_eq!(
            execute_request(
                &mut bad_acee,
                REQUESTS[2],
                Actor::AUTHORIZED,
                false,
                false,
                BTreeMap::new(),
            ),
            status(Reason::AceeInvalid)
        );

        let mut bad_mfa = RefState {
            mfa_required: true,
            mfa_valid: false,
            ..Default::default()
        };
        let acees = bad_mfa.acees;
        assert_eq!(
            execute_request(
                &mut bad_mfa,
                REQUESTS[13],
                Actor::AUTHORIZED,
                true,
                false,
                BTreeMap::new(),
            ),
            status(Reason::MfaInvalid)
        );
        assert_eq!(bad_mfa.acees, acees);

        let stale_cache = AccessFixture {
            raclisted: true,
            cache_present: false,
            ..Default::default()
        };
        assert_eq!(
            decide_access(&stale_cache),
            status(Reason::PolicyUnavailable)
        );
    }

    #[test]
    fn every_nonlicensed_binding_is_exercised_by_the_reference_model() {
        for (index, case) in COMMANDS.iter().enumerate() {
            let row = format!(
                "ibm-zos-3.2-racf-saf-2026:racf-command-families:{:04}",
                index + 1
            );
            for obligation in [
                "syntax",
                "authorized",
                "unauthorized",
                "malformed",
                "bounded-limit",
                "audit-redaction",
                "atomic-retry",
                "restart-recovery",
            ] {
                verify_binding("command", &row, case.keyword, obligation, "executed").unwrap();
            }
        }
        for (index, case) in REQUESTS.iter().enumerate() {
            let row = format!(
                "ibm-zos-3.2-racf-saf-2026:racroute-request-types:{:04}",
                index + 1
            );
            for obligation in [
                "syntax",
                "authorized",
                "unauthorized",
                "malformed",
                "bounded-limit",
                "audit-redaction",
                "atomic-retry",
                "restart-recovery",
            ] {
                verify_binding("racroute", &row, case.keyword, obligation, "executed").unwrap();
            }
        }
    }

    #[test]
    fn representative_mutants_are_rejected() {
        let generic_success = status(Reason::Granted);
        let bypassed = execute_command(
            &mut RefState::default(),
            COMMANDS[0],
            Actor::MISSING,
            false,
            BTreeMap::new(),
        );
        assert_ne!(bypassed, generic_success, "generic success mutant survived");

        let wrong_precedence = AccessFixture {
            user_entry: Some((Access::None, true)),
            group_entries: vec![(Access::Alter, true)],
            requested: Access::Read,
            ..Default::default()
        };
        assert_eq!(
            decide_access(&wrong_precedence).reason,
            Reason::InsufficientAccess,
            "wrong precedence mutant survived"
        );
        assert_ne!(
            status(Reason::PrincipalNotFound).reason_code,
            status(Reason::InsufficientAccess).reason_code,
            "wrong code mutant survived"
        );

        let mut state = RefState::default();
        execute_command(
            &mut state,
            COMMANDS[0],
            Actor::AUTHORIZED,
            false,
            safe_fields(),
        );
        assert_eq!(state.audits.len(), 1, "missing audit mutant survived");
        require_redacted(&state).expect("unsafe projection mutant survived");

        let source = include_str!("racf_reference.rs");
        for forbidden in [
            ["mainframe", "_env_", "racf"].concat(),
            ["command", "_processor"].concat(),
            ["evaluate", "_access"].concat(),
            ["apply", "_request"].concat(),
            ["apply", "_mutation"].concat(),
            ["command", "_oracle", "_observation"].concat(),
            ["execute", "_racroute", "_case"].concat(),
        ] {
            assert!(
                !source.contains(&forbidden),
                "shared implementation reuse mutant survived: {forbidden}"
            );
        }

        let expected = expected_observation(
            "command",
            "ibm-zos-3.2-racf-saf-2026:racf-command-families:0001",
            "ADDGROUP",
            "authorized",
            "executed",
        )
        .unwrap();
        let mut generic_success = expected.clone();
        generic_success.status = None;
        assert!(compare_observations(&expected, &generic_success).is_err());
        let mut no_op = expected.clone();
        no_op.changed_domains.clear();
        assert!(compare_observations(&expected, &no_op).is_err());
        let mut wrong_transition = expected.clone();
        wrong_transition.changed_domains = BTreeSet::from(["profiles".into()]);
        assert!(compare_observations(&expected, &wrong_transition).is_err());
        let mut wrong_status = expected.clone();
        wrong_status.status.as_mut().unwrap().reason = "default-deny".into();
        assert!(compare_observations(&expected, &wrong_status).is_err());
        let mut missing_audit = expected.clone();
        missing_audit.audit = "none".into();
        assert!(compare_observations(&expected, &missing_audit).is_err());

        let denied = expected_observation(
            "command",
            "ibm-zos-3.2-racf-saf-2026:racf-command-families:0001",
            "ADDGROUP",
            "unauthorized",
            "executed",
        )
        .unwrap();
        let mut authorization_bypass = denied.clone();
        authorization_bypass.outcome = "granted".into();
        authorization_bypass.status = Some(status_observation(status(Reason::Granted)));
        authorization_bypass.changed_domains = BTreeSet::from(["groups".into()]);
        assert!(compare_observations(&denied, &authorization_bypass).is_err());

        let replay = expected_observation(
            "racroute",
            "ibm-zos-3.2-racf-saf-2026:racroute-request-types:0010",
            "TOKENBLD",
            "authorized",
            "executed",
        )
        .unwrap();
        let mut replay_duplication = replay.clone();
        replay_duplication.replay = "duplicate-effect".into();
        assert!(compare_observations(&replay, &replay_duplication).is_err());
    }

    #[test]
    fn documented_unknowns_remain_explicitly_outside_the_model() {
        let unknowns = UNKNOWN_CASES.into_iter().collect::<BTreeSet<_>>();
        assert_eq!(unknowns.len(), 4);
    }
}
