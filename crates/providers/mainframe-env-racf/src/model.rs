use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const SECURITY_DATABASE_SCHEMA: &str = "mainframe-env.racf-database@2";
pub const SECURITY_PROFILE_SCHEMA: &str = "mainframe-env.racf-profile@2";
pub const SECURITY_TRANSACTION_SCHEMA: &str = "mainframe-env.racf-transaction@1";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SecurityDatabaseLimits {
    pub max_database_bytes: usize,
    pub max_name_bytes: usize,
    pub max_value_bytes: usize,
    pub max_principals: usize,
    pub max_groups: usize,
    pub max_connections: usize,
    pub max_classes: usize,
    pub max_templates: usize,
    pub max_profiles: usize,
    pub max_segments_per_profile: usize,
    pub max_fields_per_segment: usize,
    pub max_access_entries: usize,
    pub max_conditions: usize,
    pub max_acees: usize,
    pub max_tokens: usize,
    pub max_certificates: usize,
    pub max_keyrings: usize,
    pub max_audits: usize,
    pub max_transactions: usize,
    pub max_recovery_records: usize,
    pub max_migrations: usize,
}

impl Default for SecurityDatabaseLimits {
    fn default() -> Self {
        Self {
            max_database_bytes: 16 * 1024 * 1024,
            max_name_bytes: 246,
            max_value_bytes: 4096,
            max_principals: 4096,
            max_groups: 4096,
            max_connections: 65_536,
            max_classes: 1024,
            max_templates: 1024,
            max_profiles: 65_536,
            max_segments_per_profile: 64,
            max_fields_per_segment: 256,
            max_access_entries: 1024,
            max_conditions: 256,
            max_acees: 65_536,
            max_tokens: 65_536,
            max_certificates: 65_536,
            max_keyrings: 4096,
            max_audits: 65_536,
            max_transactions: 65_536,
            max_recovery_records: 4096,
            max_migrations: 64,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PrincipalKind {
    User,
    Group,
    StartedTask,
    Undefined,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum GroupAuthority {
    Use,
    Create,
    Connect,
    Join,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum AccessLevel {
    None,
    Execute,
    Read,
    Update,
    Control,
    Alter,
}

impl AccessLevel {
    #[must_use]
    pub const fn rank(self) -> u8 {
        match self {
            Self::None => 0,
            Self::Execute => 1,
            Self::Read => 2,
            Self::Update => 3,
            Self::Control => 4,
            Self::Alter => 5,
        }
    }

    #[must_use]
    pub const fn permits(self, requested: Self) -> bool {
        self.rank() >= requested.rank()
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum AuditPolicy {
    None,
    Failures,
    Successes,
    All,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PrincipalState {
    Active,
    PasswordExpired,
    Revoked,
    Suspended,
    Locked,
}

#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CredentialVerifier {
    pub algorithm: String,
    pub encoded_verifier: String,
    pub changed_tick: u64,
    pub history_digests: Vec<String>,
}

#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PrincipalProfile {
    pub id: String,
    pub kind: PrincipalKind,
    pub owner: String,
    pub default_group: Option<String>,
    pub state: PrincipalState,
    pub(crate) credential: Option<CredentialVerifier>,
    pub security_level: u32,
    pub security_label: Option<String>,
    pub categories: BTreeSet<String>,
    pub attributes: BTreeSet<String>,
    pub version: u64,
}

impl PrincipalProfile {
    #[must_use]
    pub const fn has_credential(&self) -> bool {
        self.credential.is_some()
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GroupProfile {
    pub name: String,
    pub owner: String,
    pub superior_group: Option<String>,
    pub universal: bool,
    pub version: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GroupConnection {
    pub user: String,
    pub group: String,
    pub authority: GroupAuthority,
    pub special: bool,
    pub operations: bool,
    pub auditor: bool,
    pub revoked: bool,
    pub version: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case", tag = "type", content = "value")]
pub enum SegmentValue {
    Boolean(bool),
    Unsigned(u64),
    Text(String),
    Name(String),
    Names(BTreeSet<String>),
    SecretReference(String),
    CertificateReference(String),
    KeyReference(String),
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SegmentFieldKind {
    Boolean,
    Unsigned,
    Text,
    Name,
    Names,
    SecretReference,
    CertificateReference,
    KeyReference,
}

impl SegmentValue {
    #[must_use]
    pub const fn kind(&self) -> SegmentFieldKind {
        match self {
            Self::Boolean(_) => SegmentFieldKind::Boolean,
            Self::Unsigned(_) => SegmentFieldKind::Unsigned,
            Self::Text(_) => SegmentFieldKind::Text,
            Self::Name(_) => SegmentFieldKind::Name,
            Self::Names(_) => SegmentFieldKind::Names,
            Self::SecretReference(_) => SegmentFieldKind::SecretReference,
            Self::CertificateReference(_) => SegmentFieldKind::CertificateReference,
            Self::KeyReference(_) => SegmentFieldKind::KeyReference,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SegmentFieldSchema {
    pub kind: SegmentFieldKind,
    pub required: bool,
    pub max_bytes: usize,
    pub max_items: usize,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SegmentTemplate {
    pub name: String,
    pub version: u32,
    pub fields: BTreeMap<String, SegmentFieldSchema>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileTemplate {
    pub id: String,
    pub version: u32,
    pub profile_kind: PrincipalKind,
    pub required_segments: BTreeSet<String>,
    pub segments: BTreeMap<String, SegmentTemplate>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileSegment {
    pub template: String,
    pub template_version: u32,
    pub fields: BTreeMap<String, SegmentValue>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ClassDescriptor {
    pub name: String,
    pub supplied: bool,
    pub active: bool,
    pub generic_allowed: bool,
    pub discrete_allowed: bool,
    pub raclist: bool,
    pub default_uacc: AccessLevel,
    pub max_profile_name_bytes: usize,
    pub posit: Option<u16>,
    pub member_class: Option<String>,
    pub grouping_class: Option<String>,
    pub profile_template: String,
    pub version: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AccessCondition {
    pub terminal: Option<String>,
    pub console: Option<String>,
    pub system: Option<String>,
    pub application: Option<String>,
    pub start_tick: Option<u64>,
    pub end_tick: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AccessControlEntry {
    pub principal: String,
    pub access: AccessLevel,
    pub when: Option<AccessCondition>,
    pub audit: AuditPolicy,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceProfile {
    pub class: String,
    pub name: String,
    pub generic: bool,
    pub owner: String,
    pub uacc: AccessLevel,
    pub audit: AuditPolicy,
    pub security_level: u32,
    pub security_label: Option<String>,
    pub categories: BTreeSet<String>,
    pub access_list: Vec<AccessControlEntry>,
    pub segments: BTreeMap<String, ProfileSegment>,
    pub version: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum AceeState {
    Active,
    Deleted,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Acee {
    pub id: String,
    pub principal: String,
    pub default_group: Option<String>,
    pub groups: BTreeSet<String>,
    pub parent: Option<String>,
    pub delegated_by: Option<String>,
    pub security_level: u32,
    pub security_label: Option<String>,
    pub categories: BTreeSet<String>,
    pub token_ids: BTreeSet<String>,
    pub created_tick: u64,
    pub state: AceeState,
    pub version: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TokenKind {
    SafIdentity,
    PassTicket,
    JwtReference,
    Custom,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TokenState {
    Active,
    Revoked,
    Expired,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SecurityToken {
    pub id: String,
    pub kind: TokenKind,
    pub owner: String,
    pub issuer: String,
    pub audience: Option<String>,
    pub token_reference: String,
    pub token_digest: String,
    pub scopes: BTreeSet<String>,
    pub issued_tick: u64,
    pub expires_tick: Option<u64>,
    pub state: TokenState,
    pub version: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CertificateReference {
    pub id: String,
    pub owner: String,
    pub label: String,
    pub certificate_reference: String,
    pub fingerprint_sha256: String,
    pub trusted: bool,
    pub not_before_tick: Option<u64>,
    pub not_after_tick: Option<u64>,
    pub version: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct KeyReference {
    pub id: String,
    pub owner: String,
    pub key_reference: String,
    pub algorithm: String,
    pub exportable: bool,
    pub version: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct KeyRing {
    pub owner: String,
    pub name: String,
    pub certificates: BTreeSet<String>,
    pub default_certificate: Option<String>,
    pub version: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum DecisionOutcome {
    Allow,
    Deny,
    NoDecision,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum DecisionReason {
    Granted,
    DefaultDeny,
    ClassInactive,
    ProfileNotFound,
    PrincipalNotFound,
    PrincipalInactive,
    InsufficientAccess,
    SecurityLabelMismatch,
    ConditionNotSatisfied,
    TokenInvalid,
    AceeInvalid,
    CredentialInvalid,
    PolicyUnavailable,
    StoreUnavailable,
    MalformedRequest,
    UnsupportedRequest,
    ResourceExhausted,
    Cancelled,
    TimedOut,
    RecoveryRequired,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SafStatus {
    pub saf_return_code: u32,
    pub racf_return_code: u32,
    pub racf_reason_code: u32,
    pub reason: DecisionReason,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SafDecision {
    pub outcome: DecisionOutcome,
    pub status: SafStatus,
    pub granted_access: AccessLevel,
    pub matched_profile: Option<String>,
    pub audit_id: Option<String>,
    pub cache_generation: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case", tag = "type", content = "value")]
pub enum AuditFieldValue {
    Text(String),
    Redacted,
    Digest(String),
    Reference(String),
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SecurityAuditRecord {
    pub id: String,
    pub correlation: String,
    pub actor: String,
    pub action: String,
    pub class: Option<String>,
    pub resource_digest: Option<String>,
    pub decision: DecisionOutcome,
    pub status: SafStatus,
    pub fields: BTreeMap<String, AuditFieldValue>,
    pub tick: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TransactionState {
    Intent,
    Committed,
    RolledBack,
    UnknownOutcome,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SecurityTransaction {
    pub id: String,
    pub idempotency_key: String,
    pub actor: String,
    pub operation: String,
    pub request_digest: String,
    pub state: TransactionState,
    pub base_generation: u64,
    pub final_generation: Option<u64>,
    pub status: SafStatus,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RecoveryState {
    Required,
    Replaying,
    Reconciled,
    Failed,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveryRecord {
    pub id: String,
    pub transaction_id: String,
    pub state: RecoveryState,
    pub attempt: u32,
    pub last_error: Option<DecisionReason>,
    pub version: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum MigrationState {
    Planned,
    Applied,
    RolledBack,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SecurityMigration {
    pub id: String,
    pub from_schema: String,
    pub to_schema: String,
    pub state: MigrationState,
    pub source_digest: String,
    pub result_digest: Option<String>,
}

#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SecurityDatabaseSnapshot {
    pub schema_version: String,
    pub generation: u64,
    pub classes: BTreeMap<String, ClassDescriptor>,
    pub templates: BTreeMap<String, ProfileTemplate>,
    pub principals: BTreeMap<String, PrincipalProfile>,
    pub groups: BTreeMap<String, GroupProfile>,
    pub connections: BTreeMap<String, GroupConnection>,
    pub profiles: BTreeMap<String, ResourceProfile>,
    pub acees: BTreeMap<String, Acee>,
    pub tokens: BTreeMap<String, SecurityToken>,
    pub certificates: BTreeMap<String, CertificateReference>,
    pub keys: BTreeMap<String, KeyReference>,
    pub keyrings: BTreeMap<String, KeyRing>,
    pub audits: Vec<SecurityAuditRecord>,
    pub transactions: BTreeMap<String, SecurityTransaction>,
    pub recovery: BTreeMap<String, RecoveryRecord>,
    pub migrations: Vec<SecurityMigration>,
}

impl Default for SecurityDatabaseSnapshot {
    fn default() -> Self {
        Self {
            schema_version: SECURITY_DATABASE_SCHEMA.to_string(),
            generation: 1,
            classes: BTreeMap::new(),
            templates: BTreeMap::new(),
            principals: BTreeMap::new(),
            groups: BTreeMap::new(),
            connections: BTreeMap::new(),
            profiles: BTreeMap::new(),
            acees: BTreeMap::new(),
            tokens: BTreeMap::new(),
            certificates: BTreeMap::new(),
            keys: BTreeMap::new(),
            keyrings: BTreeMap::new(),
            audits: Vec::new(),
            transactions: BTreeMap::new(),
            recovery: BTreeMap::new(),
            migrations: Vec::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SecuritySchemaProblem {
    IncompatibleVersion,
    Malformed,
    LimitExceeded,
    MissingReference,
    Duplicate,
    Cycle,
    SecretMaterial,
}

impl SecurityDatabaseSnapshot {
    pub fn validate(&self, limits: SecurityDatabaseLimits) -> Result<(), SecuritySchemaProblem> {
        if self.schema_version != SECURITY_DATABASE_SCHEMA || self.generation == 0 {
            return Err(SecuritySchemaProblem::IncompatibleVersion);
        }
        for (len, max) in [
            (self.principals.len(), limits.max_principals),
            (self.groups.len(), limits.max_groups),
            (self.connections.len(), limits.max_connections),
            (self.classes.len(), limits.max_classes),
            (self.templates.len(), limits.max_templates),
            (self.profiles.len(), limits.max_profiles),
            (self.acees.len(), limits.max_acees),
            (self.tokens.len(), limits.max_tokens),
            (self.certificates.len(), limits.max_certificates),
            (self.keyrings.len(), limits.max_keyrings),
            (self.audits.len(), limits.max_audits),
            (self.transactions.len(), limits.max_transactions),
            (self.recovery.len(), limits.max_recovery_records),
            (self.migrations.len(), limits.max_migrations),
        ] {
            if len > max {
                return Err(SecuritySchemaProblem::LimitExceeded);
            }
        }
        self.validate_templates(limits)?;
        self.validate_principals(limits)?;
        self.validate_classes(limits)?;
        self.validate_profiles(limits)?;
        self.validate_runtime_records(limits)
    }

    fn validate_templates(
        &self,
        limits: SecurityDatabaseLimits,
    ) -> Result<(), SecuritySchemaProblem> {
        for (id, template) in &self.templates {
            identifier(id, limits.max_name_bytes)?;
            if template.id != *id || template.version == 0 {
                return Err(SecuritySchemaProblem::Malformed);
            }
            for required in &template.required_segments {
                if !template.segments.contains_key(required) {
                    return Err(SecuritySchemaProblem::MissingReference);
                }
            }
            for (name, segment) in &template.segments {
                identifier(name, limits.max_name_bytes)?;
                if segment.name != *name
                    || segment.version == 0
                    || segment.fields.len() > limits.max_fields_per_segment
                {
                    return Err(SecuritySchemaProblem::Malformed);
                }
                for (field, schema) in &segment.fields {
                    identifier(field, limits.max_name_bytes)?;
                    if schema.max_bytes == 0
                        || schema.max_bytes > limits.max_value_bytes
                        || schema.max_items > limits.max_fields_per_segment
                    {
                        return Err(SecuritySchemaProblem::LimitExceeded);
                    }
                }
            }
        }
        Ok(())
    }

    fn validate_principals(
        &self,
        limits: SecurityDatabaseLimits,
    ) -> Result<(), SecuritySchemaProblem> {
        for (id, principal) in &self.principals {
            principal_name(id)?;
            if principal.id != *id || principal.version == 0 {
                return Err(SecuritySchemaProblem::Malformed);
            }
            principal_name(&principal.owner)?;
            if !self.principals.contains_key(&principal.owner)
                && !self.groups.contains_key(&principal.owner)
                && principal.owner != principal.id
            {
                return Err(SecuritySchemaProblem::MissingReference);
            }
            if let Some(default_group) = &principal.default_group
                && !self.groups.contains_key(default_group)
            {
                return Err(SecuritySchemaProblem::MissingReference);
            }
            if let Some(credential) = &principal.credential {
                bounded(&credential.algorithm, 64)?;
                bounded(&credential.encoded_verifier, limits.max_value_bytes)?;
                for digest in &credential.history_digests {
                    digest_sha256(digest)?;
                }
            }
        }
        for (name, group) in &self.groups {
            principal_name(name)?;
            if group.name != *name || group.version == 0 {
                return Err(SecuritySchemaProblem::Malformed);
            }
            if let Some(superior) = &group.superior_group {
                if superior == name || !self.groups.contains_key(superior) {
                    return Err(SecuritySchemaProblem::MissingReference);
                }
                let mut seen = BTreeSet::new();
                let mut current = Some(superior.as_str());
                while let Some(candidate) = current {
                    if !seen.insert(candidate) || candidate == name {
                        return Err(SecuritySchemaProblem::Cycle);
                    }
                    current = self
                        .groups
                        .get(candidate)
                        .and_then(|value| value.superior_group.as_deref());
                }
            }
        }
        for (key, connection) in &self.connections {
            if *key != connection_key(&connection.user, &connection.group)
                || connection.version == 0
                || !self.principals.contains_key(&connection.user)
                || !self.groups.contains_key(&connection.group)
            {
                return Err(SecuritySchemaProblem::MissingReference);
            }
        }
        Ok(())
    }

    fn validate_classes(
        &self,
        limits: SecurityDatabaseLimits,
    ) -> Result<(), SecuritySchemaProblem> {
        for (name, class) in &self.classes {
            class_name(name)?;
            if class.name != *name
                || class.version == 0
                || class.max_profile_name_bytes == 0
                || class.max_profile_name_bytes > limits.max_name_bytes
                || !self.templates.contains_key(&class.profile_template)
            {
                return Err(SecuritySchemaProblem::MissingReference);
            }
            for related in [class.member_class.as_ref(), class.grouping_class.as_ref()]
                .into_iter()
                .flatten()
            {
                if related == name || !self.classes.contains_key(related) {
                    return Err(SecuritySchemaProblem::MissingReference);
                }
            }
        }
        Ok(())
    }

    fn validate_profiles(
        &self,
        limits: SecurityDatabaseLimits,
    ) -> Result<(), SecuritySchemaProblem> {
        for (key, profile) in &self.profiles {
            if *key != profile_key(&profile.class, &profile.name) || profile.version == 0 {
                return Err(SecuritySchemaProblem::Malformed);
            }
            let class = self
                .classes
                .get(&profile.class)
                .ok_or(SecuritySchemaProblem::MissingReference)?;
            profile_name(&profile.name, class.max_profile_name_bytes, profile.generic)?;
            if profile.generic && !class.generic_allowed
                || !profile.generic && !class.discrete_allowed
                || !self.principals.contains_key(&profile.owner)
                    && !self.groups.contains_key(&profile.owner)
                || profile.access_list.len() > limits.max_access_entries
                || profile.segments.len() > limits.max_segments_per_profile
            {
                return Err(SecuritySchemaProblem::Malformed);
            }
            let template = self
                .templates
                .get(&class.profile_template)
                .ok_or(SecuritySchemaProblem::MissingReference)?;
            for required in &template.required_segments {
                if !profile.segments.contains_key(required) {
                    return Err(SecuritySchemaProblem::MissingReference);
                }
            }
            for (name, segment) in &profile.segments {
                let segment_schema = template
                    .segments
                    .get(name)
                    .ok_or(SecuritySchemaProblem::MissingReference)?;
                if segment.template != *name
                    || segment.template_version != segment_schema.version
                    || segment.fields.len() > limits.max_fields_per_segment
                {
                    return Err(SecuritySchemaProblem::IncompatibleVersion);
                }
                for (field, field_schema) in &segment_schema.fields {
                    if field_schema.required && !segment.fields.contains_key(field) {
                        return Err(SecuritySchemaProblem::MissingReference);
                    }
                }
                for (field, value) in &segment.fields {
                    let field_schema = segment_schema
                        .fields
                        .get(field)
                        .ok_or(SecuritySchemaProblem::MissingReference)?;
                    if value.kind() != field_schema.kind {
                        return Err(SecuritySchemaProblem::Malformed);
                    }
                    validate_segment_value(value, field_schema, limits)?;
                }
            }
            for ace in &profile.access_list {
                if !self.principals.contains_key(&ace.principal)
                    && !self.groups.contains_key(&ace.principal)
                {
                    return Err(SecuritySchemaProblem::MissingReference);
                }
                if let Some(condition) = &ace.when {
                    validate_condition(condition, limits)?;
                }
            }
        }
        Ok(())
    }

    fn validate_runtime_records(
        &self,
        limits: SecurityDatabaseLimits,
    ) -> Result<(), SecuritySchemaProblem> {
        for (id, acee) in &self.acees {
            identifier(id, limits.max_name_bytes)?;
            if acee.id != *id
                || acee.version == 0
                || !self.principals.contains_key(&acee.principal)
                || acee
                    .parent
                    .as_ref()
                    .is_some_and(|parent| parent == id || !self.acees.contains_key(parent))
                || acee
                    .token_ids
                    .iter()
                    .any(|token| !self.tokens.contains_key(token))
            {
                return Err(SecuritySchemaProblem::MissingReference);
            }
        }
        for (id, token) in &self.tokens {
            identifier(id, limits.max_name_bytes)?;
            if token.id != *id || token.version == 0 || !self.principals.contains_key(&token.owner)
            {
                return Err(SecuritySchemaProblem::MissingReference);
            }
            secret_reference(&token.token_reference, limits.max_value_bytes)?;
            digest_sha256(&token.token_digest)?;
            if token
                .expires_tick
                .is_some_and(|expiry| expiry <= token.issued_tick)
            {
                return Err(SecuritySchemaProblem::Malformed);
            }
        }
        for (id, certificate) in &self.certificates {
            identifier(id, limits.max_name_bytes)?;
            if certificate.id != *id
                || certificate.version == 0
                || !self.principals.contains_key(&certificate.owner)
            {
                return Err(SecuritySchemaProblem::MissingReference);
            }
            secret_reference(&certificate.certificate_reference, limits.max_value_bytes)?;
            digest_sha256(&certificate.fingerprint_sha256)?;
        }
        for (id, key) in &self.keys {
            identifier(id, limits.max_name_bytes)?;
            if key.id != *id || key.version == 0 || !self.principals.contains_key(&key.owner) {
                return Err(SecuritySchemaProblem::MissingReference);
            }
            secret_reference(&key.key_reference, limits.max_value_bytes)?;
        }
        for (id, keyring) in &self.keyrings {
            if *id != keyring_key(&keyring.owner, &keyring.name)
                || keyring.version == 0
                || !self.principals.contains_key(&keyring.owner)
                || keyring
                    .certificates
                    .iter()
                    .any(|certificate| !self.certificates.contains_key(certificate))
                || keyring
                    .default_certificate
                    .as_ref()
                    .is_some_and(|certificate| !keyring.certificates.contains(certificate))
            {
                return Err(SecuritySchemaProblem::MissingReference);
            }
        }
        for audit in &self.audits {
            identifier(&audit.id, limits.max_name_bytes)?;
            bounded(&audit.correlation, limits.max_value_bytes)?;
            bounded(&audit.action, limits.max_value_bytes)?;
            if audit.fields.len() > limits.max_fields_per_segment {
                return Err(SecuritySchemaProblem::LimitExceeded);
            }
        }
        for (id, transaction) in &self.transactions {
            identifier(id, limits.max_name_bytes)?;
            if transaction.id != *id {
                return Err(SecuritySchemaProblem::Malformed);
            }
            digest_sha256(&transaction.request_digest)?;
        }
        for (id, recovery) in &self.recovery {
            identifier(id, limits.max_name_bytes)?;
            if recovery.id != *id || !self.transactions.contains_key(&recovery.transaction_id) {
                return Err(SecuritySchemaProblem::MissingReference);
            }
        }
        Ok(())
    }
}

fn validate_segment_value(
    value: &SegmentValue,
    schema: &SegmentFieldSchema,
    limits: SecurityDatabaseLimits,
) -> Result<(), SecuritySchemaProblem> {
    match value {
        SegmentValue::Boolean(_) | SegmentValue::Unsigned(_) => Ok(()),
        SegmentValue::Text(value) => bounded(value, schema.max_bytes),
        SegmentValue::Name(value) => identifier(value, schema.max_bytes),
        SegmentValue::Names(values) => {
            if values.len() > schema.max_items {
                return Err(SecuritySchemaProblem::LimitExceeded);
            }
            for value in values {
                identifier(value, schema.max_bytes)?;
            }
            Ok(())
        }
        SegmentValue::SecretReference(value)
        | SegmentValue::CertificateReference(value)
        | SegmentValue::KeyReference(value) => {
            secret_reference(value, schema.max_bytes.min(limits.max_value_bytes))
        }
    }
}

fn validate_condition(
    condition: &AccessCondition,
    limits: SecurityDatabaseLimits,
) -> Result<(), SecuritySchemaProblem> {
    if condition
        .start_tick
        .zip(condition.end_tick)
        .is_some_and(|(start, end)| start >= end)
    {
        return Err(SecuritySchemaProblem::Malformed);
    }
    for value in [
        condition.terminal.as_ref(),
        condition.console.as_ref(),
        condition.system.as_ref(),
        condition.application.as_ref(),
    ]
    .into_iter()
    .flatten()
    {
        identifier(value, limits.max_name_bytes)?;
    }
    Ok(())
}

pub(crate) fn connection_key(user: &str, group: &str) -> String {
    format!("{user}:{group}")
}

pub(crate) fn profile_key(class: &str, name: &str) -> String {
    format!("{class}:{name}")
}

pub(crate) fn keyring_key(owner: &str, name: &str) -> String {
    format!("{owner}:{name}")
}

pub(crate) fn principal_name(value: &str) -> Result<(), SecuritySchemaProblem> {
    identifier_with(value, 8, false)
}

pub(crate) fn class_name(value: &str) -> Result<(), SecuritySchemaProblem> {
    identifier_with(value, 32, false)
}

pub(crate) fn profile_name(
    value: &str,
    max: usize,
    generic: bool,
) -> Result<(), SecuritySchemaProblem> {
    identifier_with(value, max, generic)
}

fn identifier(value: &str, max: usize) -> Result<(), SecuritySchemaProblem> {
    identifier_with(value, max, false)
}

fn identifier_with(value: &str, max: usize, generic: bool) -> Result<(), SecuritySchemaProblem> {
    if value.is_empty()
        || value.len() > max
        || value.bytes().any(|byte| {
            !(byte.is_ascii_uppercase()
                || byte.is_ascii_digit()
                || matches!(byte, b'@' | b'#' | b'$' | b'-' | b'_' | b'.' | b':' | b'/')
                || generic && matches!(byte, b'*' | b'%'))
        })
    {
        Err(SecuritySchemaProblem::Malformed)
    } else {
        Ok(())
    }
}

fn bounded(value: &str, max: usize) -> Result<(), SecuritySchemaProblem> {
    if value.is_empty() || value.len() > max || value.chars().any(char::is_control) {
        Err(SecuritySchemaProblem::Malformed)
    } else {
        Ok(())
    }
}

fn secret_reference(value: &str, max: usize) -> Result<(), SecuritySchemaProblem> {
    bounded(value, max)?;
    if value.contains(char::is_whitespace)
        || !value.contains(':')
        || value.to_ascii_uppercase().contains("BEGIN ")
    {
        Err(SecuritySchemaProblem::SecretMaterial)
    } else {
        Ok(())
    }
}

fn digest_sha256(value: &str) -> Result<(), SecuritySchemaProblem> {
    if value.len() == 71
        && value.starts_with("sha256:")
        && value[7..].bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        Ok(())
    } else {
        Err(SecuritySchemaProblem::Malformed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn compiled_schema(name: &str) -> jsonschema::Validator {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../conformance/0.5/schemas")
            .join(name);
        let schema: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        jsonschema::draft202012::options()
            .offline()
            .build(&schema)
            .unwrap()
    }

    fn principal(id: &str) -> PrincipalProfile {
        PrincipalProfile {
            id: id.into(),
            kind: PrincipalKind::User,
            owner: id.into(),
            default_group: None,
            state: PrincipalState::Active,
            credential: None,
            security_level: 0,
            security_label: None,
            categories: BTreeSet::new(),
            attributes: BTreeSet::new(),
            version: 1,
        }
    }

    #[test]
    fn access_order_includes_none_and_alter() {
        assert!(AccessLevel::Alter.permits(AccessLevel::Control));
        assert!(AccessLevel::Read.permits(AccessLevel::Execute));
        assert!(!AccessLevel::None.permits(AccessLevel::Execute));
    }

    #[test]
    fn snapshot_rejects_missing_template_and_plain_secret_material() {
        let mut snapshot = SecurityDatabaseSnapshot::default();
        snapshot
            .principals
            .insert("IBMUSER".into(), principal("IBMUSER"));
        snapshot.classes.insert(
            "DATASET".into(),
            ClassDescriptor {
                name: "DATASET".into(),
                supplied: true,
                active: true,
                generic_allowed: true,
                discrete_allowed: true,
                raclist: false,
                default_uacc: AccessLevel::None,
                max_profile_name_bytes: 44,
                posit: None,
                member_class: None,
                grouping_class: None,
                profile_template: "RESOURCE".into(),
                version: 1,
            },
        );
        assert_eq!(
            snapshot.validate(SecurityDatabaseLimits::default()),
            Err(SecuritySchemaProblem::MissingReference)
        );
        assert_eq!(
            secret_reference("-----BEGIN PRIVATE KEY-----", 128),
            Err(SecuritySchemaProblem::SecretMaterial)
        );
    }

    #[test]
    fn group_hierarchy_cycles_are_rejected() {
        let mut snapshot = SecurityDatabaseSnapshot::default();
        for (name, superior) in [("A", Some("B")), ("B", Some("A"))] {
            snapshot.groups.insert(
                name.into(),
                GroupProfile {
                    name: name.into(),
                    owner: name.into(),
                    superior_group: superior.map(str::to_string),
                    universal: false,
                    version: 1,
                },
            );
        }
        assert_eq!(
            snapshot.validate(SecurityDatabaseLimits::default()),
            Err(SecuritySchemaProblem::Cycle)
        );
    }

    #[test]
    fn durable_and_public_json_projections_match_compiled_schemas() {
        let snapshot = serde_json::to_value(SecurityDatabaseSnapshot::default()).unwrap();
        compiled_schema("security-database.schema.json")
            .validate(&snapshot)
            .unwrap();
        let decision = SafDecision {
            outcome: DecisionOutcome::Deny,
            status: SafStatus {
                saf_return_code: 8,
                racf_return_code: 8,
                racf_reason_code: 4,
                reason: DecisionReason::DefaultDeny,
            },
            granted_access: AccessLevel::None,
            matched_profile: None,
            audit_id: None,
            cache_generation: None,
        };
        compiled_schema("security-decision.schema.json")
            .validate(&serde_json::to_value(decision).unwrap())
            .unwrap();
    }
}
