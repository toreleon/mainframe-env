mod credential;
use argon2::password_hash::phc::PasswordHash;
pub(crate) use credential::CredentialVerifier;
use credential::{default_password_maximum, default_password_minimum};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const SECURITY_DATABASE_SCHEMA: &str = "mainframe-env.racf-database@2";
pub const SECURITY_PROFILE_SCHEMA: &str = "mainframe-env.racf-profile@2";
pub const SECURITY_TRANSACTION_SCHEMA: &str = "mainframe-env.racf-transaction@2";

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
    pub max_mfa_factors: usize,
    pub max_identity_mappings: usize,
    pub max_user_associations: usize,
    pub max_rrsf_nodes: usize,
    pub max_signon_sessions: usize,
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
            max_mfa_factors: 16_384,
            max_identity_mappings: 65_536,
            max_user_associations: 65_536,
            max_rrsf_nodes: 4096,
            max_signon_sessions: 65_536,
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
pub struct PrincipalProfile {
    pub id: String,
    pub kind: PrincipalKind,
    pub owner: String,
    pub default_group: Option<String>,
    pub state: PrincipalState,
    pub(crate) credential: Option<CredentialVerifier>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) phrase_credential: Option<CredentialVerifier>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) invalid_count: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) last_use_tick: Option<u64>,
    #[serde(default)]
    pub profile_template: Option<String>,
    #[serde(default)]
    pub segments: BTreeMap<String, ProfileSegment>,
    pub security_level: u32,
    pub security_label: Option<String>,
    pub categories: BTreeSet<String>,
    pub attributes: BTreeSet<String>,
    pub version: u64,
}

impl PrincipalProfile {
    #[must_use]
    pub const fn has_credential(&self) -> bool {
        self.credential.is_some() || self.phrase_credential.is_some()
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GroupProfile {
    pub name: String,
    pub owner: String,
    pub superior_group: Option<String>,
    pub universal: bool,
    #[serde(default)]
    pub profile_template: Option<String>,
    #[serde(default)]
    pub segments: BTreeMap<String, ProfileSegment>,
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
    #[serde(default = "default_true")]
    pub generic_active: bool,
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
pub struct RaclistCache {
    pub class: String,
    pub built_generation: u64,
    pub profiles: BTreeMap<String, ResourceProfile>,
    pub version: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum DatabaseSharingMode {
    NonDataSharing,
    DataSharing,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RacfDatabaseStatus {
    pub active: bool,
    pub primary_dataset: Option<String>,
    pub backup_dataset: Option<String>,
    pub sharing_mode: DatabaseSharingMode,
    pub switch_generation: u64,
}

impl Default for RacfDatabaseStatus {
    fn default() -> Self {
        Self {
            active: true,
            primary_dataset: None,
            backup_dataset: None,
            sharing_mode: DatabaseSharingMode::NonDataSharing,
            switch_generation: 1,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SecurityPolicyOptions {
    pub add_creator: bool,
    pub command_violations_audited: bool,
    pub jes_batch_all_racf: bool,
    pub ml_active: bool,
    pub program_control: bool,
    pub rules: bool,
    pub security_level_audit: bool,
    pub security_label_audit: bool,
    pub when_program: bool,
    pub write_down: bool,
    pub set_flags: BTreeMap<String, bool>,
    #[serde(default)]
    pub values: BTreeMap<String, String>,
    #[serde(default = "default_password_minimum")]
    pub password_minimum: usize,
    #[serde(default = "default_password_maximum")]
    pub password_maximum: usize,
    #[serde(default = "default_phrase_minimum")]
    pub phrase_minimum: usize,
    #[serde(default = "default_password_history")]
    pub password_history: usize,
}

impl Default for SecurityPolicyOptions {
    fn default() -> Self {
        Self {
            add_creator: true,
            command_violations_audited: true,
            jes_batch_all_racf: false,
            ml_active: false,
            program_control: false,
            rules: false,
            security_level_audit: false,
            security_label_audit: false,
            when_program: false,
            write_down: false,
            set_flags: BTreeMap::new(),
            values: BTreeMap::new(),
            password_minimum: default_password_minimum(),
            password_maximum: default_password_maximum(),
            phrase_minimum: default_phrase_minimum(),
            password_history: default_password_history(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RacfSubsystemState {
    pub running: bool,
    pub trace: bool,
    pub restart_generation: u64,
}

impl Default for RacfSubsystemState {
    fn default() -> Self {
        Self {
            running: true,
            trace: false,
            restart_generation: 1,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
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
    #[serde(default = "default_true")]
    pub active: bool,
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
pub enum MfaFactorKind {
    Totp,
    Webauthn,
    Passcode,
    Custom,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MfaFactor {
    pub id: String,
    pub owner: String,
    pub kind: MfaFactorKind,
    pub secret_reference: String,
    pub active: bool,
    pub created_tick: u64,
    pub version: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct IdentityMapping {
    pub id: String,
    pub registry: String,
    pub distributed_identity: String,
    pub local_user: String,
    pub label: Option<String>,
    pub version: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum AssociationState {
    Active,
    Dormant,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct UserAssociation {
    pub id: String,
    pub local_user: String,
    pub node: String,
    pub remote_user: String,
    pub peer: bool,
    pub password_sync: bool,
    pub state: AssociationState,
    pub version: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RrsfNodeState {
    Operative,
    Dormant,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RrsfNode {
    pub name: String,
    pub description: Option<String>,
    pub protocol: Option<String>,
    pub prefix: Option<String>,
    pub workspace_limit: u64,
    pub state: RrsfNodeState,
    pub version: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SignonSessionState {
    Active,
    SignedOff,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SignonSession {
    pub id: String,
    pub user: String,
    pub node: Option<String>,
    pub acee_id: String,
    pub created_tick: u64,
    pub state: SignonSessionState,
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
    MfaInvalid,
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
    /// Original event tick, retained unchanged for evidence and projections.
    pub tick: u64,
    /// Provider-clock observation used only when a legacy event has `tick == 0`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retention_observed_tick: Option<u64>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TransactionState {
    Intent,
    Committed,
    RolledBack,
    UnknownOutcome,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub enum SecurityRequestDigestFormat {
    #[default]
    #[serde(rename = "legacy-unversioned@0")]
    LegacyUnversioned,
    #[serde(rename = "mainframe-env.legacy-replay-redacted@0")]
    LegacyScrubbedV0,
    #[serde(rename = "mainframe-env.racf-command@1")]
    RacfCommandCanonicalV1,
    #[serde(rename = "mainframe-env.racroute-request@1")]
    RacrouteCanonicalV1,
}

impl SecurityRequestDigestFormat {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::LegacyUnversioned => "legacy-unversioned@0",
            Self::LegacyScrubbedV0 => "mainframe-env.legacy-replay-redacted@0",
            Self::RacfCommandCanonicalV1 => "mainframe-env.racf-command@1",
            Self::RacrouteCanonicalV1 => "mainframe-env.racroute-request@1",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SecurityTransaction {
    pub id: String,
    pub idempotency_key: String,
    pub actor: String,
    pub operation: String,
    #[serde(default)]
    pub request_digest_format: SecurityRequestDigestFormat,
    pub request_digest: String,
    pub state: TransactionState,
    pub base_generation: u64,
    pub final_generation: Option<u64>,
    pub status: SafStatus,
    #[serde(default)]
    pub terminal_result: Option<String>,
    /// Logical tick at which a terminal state was observed by a trusted caller.
    ///
    /// Legacy terminal rows retain `None` until an explicit conservative
    /// observation and are never interpreted as having occurred at tick zero.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal_tick: Option<u64>,
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
    /// Logical tick at which a terminal recovery state was observed.
    ///
    /// `None` is the conservative representation for legacy records whose age
    /// has not yet been observed in the provider clock domain.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal_tick: Option<u64>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum MigrationState {
    Planned,
    Applied,
    RolledBack,
}

/// Exact bounded legacy provider row retained inside the v2 migration record for downgrade review.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SecurityMigrationSourceRow {
    /// Original provider namespace.
    pub namespace: String,
    /// Original provider key.
    pub key: String,
    /// Original positive CAS version.
    pub version: u64,
    /// Exact original payload bytes.
    pub payload: Vec<u8>,
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
    /// Whether post-bootstrap schema installation has been included in `result_digest`.
    #[serde(default)]
    pub baseline_finalized: bool,
    /// Exact pre-upgrade rows retained without consuming live provider-row capacity.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_rows: Vec<SecurityMigrationSourceRow>,
}

#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SecurityDatabaseSnapshot {
    pub schema_version: String,
    pub generation: u64,
    /// Provider-local monotonic clock shared by every retainable RACF record.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub retention_tick: u64,
    /// CAS version of the atomically paired retention archive, or zero before creation.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub retention_archive_version: u64,
    pub classes: BTreeMap<String, ClassDescriptor>,
    pub templates: BTreeMap<String, ProfileTemplate>,
    pub principals: BTreeMap<String, PrincipalProfile>,
    pub groups: BTreeMap<String, GroupProfile>,
    pub connections: BTreeMap<String, GroupConnection>,
    pub profiles: BTreeMap<String, ResourceProfile>,
    #[serde(default)]
    pub raclist_caches: BTreeMap<String, RaclistCache>,
    #[serde(default)]
    pub policy: SecurityPolicyOptions,
    #[serde(default)]
    pub database_status: RacfDatabaseStatus,
    #[serde(default)]
    pub subsystem: RacfSubsystemState,
    pub acees: BTreeMap<String, Acee>,
    pub tokens: BTreeMap<String, SecurityToken>,
    pub certificates: BTreeMap<String, CertificateReference>,
    pub keys: BTreeMap<String, KeyReference>,
    pub keyrings: BTreeMap<String, KeyRing>,
    #[serde(default)]
    pub mfa_factors: BTreeMap<String, MfaFactor>,
    #[serde(default)]
    pub identity_mappings: BTreeMap<String, IdentityMapping>,
    #[serde(default)]
    pub user_associations: BTreeMap<String, UserAssociation>,
    #[serde(default)]
    pub rrsf_nodes: BTreeMap<String, RrsfNode>,
    #[serde(default)]
    pub signon_sessions: BTreeMap<String, SignonSession>,
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
            retention_tick: 0,
            retention_archive_version: 0,
            classes: BTreeMap::new(),
            templates: BTreeMap::new(),
            principals: BTreeMap::new(),
            groups: BTreeMap::new(),
            connections: BTreeMap::new(),
            profiles: BTreeMap::new(),
            raclist_caches: BTreeMap::new(),
            policy: SecurityPolicyOptions::default(),
            database_status: RacfDatabaseStatus::default(),
            subsystem: RacfSubsystemState::default(),
            acees: BTreeMap::new(),
            tokens: BTreeMap::new(),
            certificates: BTreeMap::new(),
            keys: BTreeMap::new(),
            keyrings: BTreeMap::new(),
            mfa_factors: BTreeMap::new(),
            identity_mappings: BTreeMap::new(),
            user_associations: BTreeMap::new(),
            rrsf_nodes: BTreeMap::new(),
            signon_sessions: BTreeMap::new(),
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
    pub(crate) fn observe_retention_tick(&mut self, supplied_tick: u64) -> Option<u64> {
        let tick = self.retention_tick.max(supplied_tick);
        self.retention_tick = tick;
        Some(tick)
    }

    pub(crate) fn max_record_retention_tick(&self) -> u64 {
        self.audits
            .iter()
            .map(|audit| audit.retention_observed_tick.unwrap_or(audit.tick))
            .chain(
                self.transactions
                    .values()
                    .filter_map(|transaction| transaction.terminal_tick),
            )
            .chain(
                self.recovery
                    .values()
                    .filter_map(|recovery| recovery.terminal_tick),
            )
            .max()
            .unwrap_or(0)
    }

    pub fn validate(&self, limits: SecurityDatabaseLimits) -> Result<(), SecuritySchemaProblem> {
        if self.schema_version != SECURITY_DATABASE_SCHEMA || self.generation == 0 {
            return Err(SecuritySchemaProblem::IncompatibleVersion);
        }
        if self.retention_tick != 0 && self.max_record_retention_tick() > self.retention_tick {
            return Err(SecuritySchemaProblem::Malformed);
        }
        for (len, max) in [
            (self.principals.len(), limits.max_principals),
            (self.groups.len(), limits.max_groups),
            (self.connections.len(), limits.max_connections),
            (self.classes.len(), limits.max_classes),
            (self.templates.len(), limits.max_templates),
            (self.profiles.len(), limits.max_profiles),
            (self.raclist_caches.len(), limits.max_classes),
            (self.acees.len(), limits.max_acees),
            (self.tokens.len(), limits.max_tokens),
            (self.certificates.len(), limits.max_certificates),
            (self.keyrings.len(), limits.max_keyrings),
            (self.mfa_factors.len(), limits.max_mfa_factors),
            (self.identity_mappings.len(), limits.max_identity_mappings),
            (self.user_associations.len(), limits.max_user_associations),
            (self.rrsf_nodes.len(), limits.max_rrsf_nodes),
            (self.signon_sessions.len(), limits.max_signon_sessions),
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
        self.validate_policy_state(limits)?;
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
            if principal
                .phrase_credential
                .as_ref()
                .is_some_and(|credential| !credential.is_phrase)
            {
                return Err(SecuritySchemaProblem::Malformed);
            }
            for credential in principal
                .credential
                .iter()
                .chain(principal.phrase_credential.iter())
            {
                bounded(&credential.algorithm, 64)?;
                bounded(&credential.encoded_verifier, limits.max_value_bytes)?;
                if credential
                    .history_digests
                    .len()
                    .saturating_add(credential.history_verifiers.len())
                    > 128
                {
                    return Err(SecuritySchemaProblem::LimitExceeded);
                }
                for digest in &credential.history_digests {
                    digest_sha256(digest)?;
                }
                for verifier in &credential.history_verifiers {
                    bounded(verifier, limits.max_value_bytes)?;
                    if PasswordHash::new(verifier).is_err() {
                        return Err(SecuritySchemaProblem::Malformed);
                    }
                }
            }
            self.validate_profile_segments(
                principal.profile_template.as_deref(),
                &principal.segments,
                PrincipalKind::User,
                limits,
            )?;
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
            self.validate_profile_segments(
                group.profile_template.as_deref(),
                &group.segments,
                PrincipalKind::Group,
                limits,
            )?;
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
            self.validate_profile_segments(
                Some(&class.profile_template),
                &profile.segments,
                PrincipalKind::Undefined,
                limits,
            )?;
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

    fn validate_profile_segments(
        &self,
        template_name: Option<&str>,
        segments: &BTreeMap<String, ProfileSegment>,
        expected_kind: PrincipalKind,
        limits: SecurityDatabaseLimits,
    ) -> Result<(), SecuritySchemaProblem> {
        let Some(template_name) = template_name else {
            return if segments.is_empty() {
                Ok(())
            } else {
                Err(SecuritySchemaProblem::MissingReference)
            };
        };
        let template = self
            .templates
            .get(template_name)
            .ok_or(SecuritySchemaProblem::MissingReference)?;
        if template.profile_kind != expected_kind
            || segments.len() > limits.max_segments_per_profile
        {
            return Err(SecuritySchemaProblem::Malformed);
        }
        for required in &template.required_segments {
            if !segments.contains_key(required) {
                return Err(SecuritySchemaProblem::MissingReference);
            }
        }
        for (name, segment) in segments {
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
                    .default_group
                    .as_ref()
                    .is_some_and(|group| !self.groups.contains_key(group))
                || acee
                    .groups
                    .iter()
                    .any(|group| !self.groups.contains_key(group))
                || acee
                    .delegated_by
                    .as_ref()
                    .is_some_and(|principal| !self.principals.contains_key(principal))
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
            let mut seen = BTreeSet::new();
            let mut parent = acee.parent.as_deref();
            while let Some(candidate) = parent {
                if candidate == id || !seen.insert(candidate) {
                    return Err(SecuritySchemaProblem::Cycle);
                }
                parent = self
                    .acees
                    .get(candidate)
                    .and_then(|parent| parent.parent.as_deref());
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
            if certificate
                .not_before_tick
                .zip(certificate.not_after_tick)
                .is_some_and(|(start, end)| start >= end)
            {
                return Err(SecuritySchemaProblem::Malformed);
            }
            bounded(&certificate.label, limits.max_value_bytes)?;
            secret_reference(&certificate.certificate_reference, limits.max_value_bytes)?;
            digest_sha256(&certificate.fingerprint_sha256)?;
        }
        for (id, key) in &self.keys {
            identifier(id, limits.max_name_bytes)?;
            if key.id != *id || key.version == 0 || !self.principals.contains_key(&key.owner) {
                return Err(SecuritySchemaProblem::MissingReference);
            }
            secret_reference(&key.key_reference, limits.max_value_bytes)?;
            bounded(&key.algorithm, limits.max_value_bytes)?;
        }
        for (id, keyring) in &self.keyrings {
            identifier(id, limits.max_name_bytes)?;
            principal_name(&keyring.owner)?;
            identifier(&keyring.name, limits.max_name_bytes)?;
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
        for (id, factor) in &self.mfa_factors {
            identifier(id, limits.max_name_bytes)?;
            if factor.id != *id
                || factor.version == 0
                || !self.principals.contains_key(&factor.owner)
            {
                return Err(SecuritySchemaProblem::MissingReference);
            }
            secret_reference(&factor.secret_reference, limits.max_value_bytes)?;
        }
        for (id, mapping) in &self.identity_mappings {
            identifier(id, limits.max_name_bytes)?;
            if mapping.id != *id
                || mapping.version == 0
                || !self.principals.contains_key(&mapping.local_user)
            {
                return Err(SecuritySchemaProblem::MissingReference);
            }
            bounded(&mapping.registry, limits.max_value_bytes)?;
            bounded(&mapping.distributed_identity, limits.max_value_bytes)?;
            if let Some(label) = &mapping.label {
                bounded(label, limits.max_value_bytes)?;
            }
        }
        for (id, association) in &self.user_associations {
            identifier(id, limits.max_name_bytes)?;
            if association.id != *id
                || association.version == 0
                || !self.principals.contains_key(&association.local_user)
                || !self.rrsf_nodes.contains_key(&association.node)
            {
                return Err(SecuritySchemaProblem::MissingReference);
            }
            principal_name(&association.remote_user)?;
        }
        for (name, node) in &self.rrsf_nodes {
            identifier(name, limits.max_name_bytes)?;
            if node.name != *name || node.version == 0 {
                return Err(SecuritySchemaProblem::Malformed);
            }
            if let Some(value) = &node.description {
                bounded(value, limits.max_value_bytes)?;
            }
            if let Some(value) = &node.protocol {
                identifier(value, limits.max_name_bytes)?;
            }
            if let Some(value) = &node.prefix {
                identifier(value, limits.max_name_bytes)?;
            }
        }
        for (id, session) in &self.signon_sessions {
            identifier(id, limits.max_name_bytes)?;
            if session.id != *id
                || session.version == 0
                || !self.principals.contains_key(&session.user)
                || !self.acees.contains_key(&session.acee_id)
                || session
                    .node
                    .as_ref()
                    .is_some_and(|node| !self.rrsf_nodes.contains_key(node))
            {
                return Err(SecuritySchemaProblem::MissingReference);
            }
        }
        let mut audit_ids = BTreeSet::new();
        for audit in &self.audits {
            identifier(&audit.id, limits.max_name_bytes)?;
            if !audit_ids.insert(audit.id.as_str()) {
                return Err(SecuritySchemaProblem::Duplicate);
            }
            if audit.retention_observed_tick == Some(0)
                || (audit.tick != 0
                    && audit
                        .retention_observed_tick
                        .is_some_and(|observed| observed != audit.tick))
            {
                return Err(SecuritySchemaProblem::Malformed);
            }
            principal_name(&audit.actor)?;
            bounded(&audit.correlation, limits.max_value_bytes)?;
            bounded(&audit.action, limits.max_value_bytes)?;
            if let Some(class) = &audit.class {
                class_name(class)?;
            }
            if let Some(resource) = &audit.resource_digest {
                bounded(resource, limits.max_value_bytes)?;
            }
            if audit.fields.len() > limits.max_fields_per_segment {
                return Err(SecuritySchemaProblem::LimitExceeded);
            }
            if crate::audit::redact_fields(audit.fields.clone()) != audit.fields {
                return Err(SecuritySchemaProblem::SecretMaterial);
            }
            for (name, value) in &audit.fields {
                bounded(name, limits.max_name_bytes)?;
                match value {
                    AuditFieldValue::Text(value) => bounded(value, limits.max_value_bytes)?,
                    AuditFieldValue::Digest(value) => digest_sha256(value)?,
                    AuditFieldValue::Reference(value) => {
                        secret_reference(value, limits.max_value_bytes)?;
                    }
                    AuditFieldValue::Redacted => {}
                }
            }
        }
        for (id, transaction) in &self.transactions {
            identifier(id, limits.max_name_bytes)?;
            identifier(&transaction.idempotency_key, limits.max_name_bytes)?;
            principal_name(&transaction.actor)?;
            identifier(&transaction.operation, limits.max_name_bytes)?;
            let terminal = matches!(
                transaction.state,
                TransactionState::Committed | TransactionState::RolledBack
            );
            if transaction.id != *id
                || transaction.idempotency_key != *id
                || transaction.base_generation == 0
                || transaction.terminal_tick == Some(0)
                || (!terminal && transaction.terminal_tick.is_some())
                || transaction
                    .final_generation
                    .is_some_and(|generation| generation <= transaction.base_generation)
            {
                return Err(SecuritySchemaProblem::Malformed);
            }
            digest_sha256(&transaction.request_digest)?;
            let racroute = transaction.operation.starts_with("RACROUTE-");
            if matches!(
                (racroute, transaction.request_digest_format),
                (true, SecurityRequestDigestFormat::RacfCommandCanonicalV1)
                    | (false, SecurityRequestDigestFormat::RacrouteCanonicalV1)
            ) {
                return Err(SecuritySchemaProblem::Malformed);
            }
            if let Some(result) = &transaction.terminal_result {
                bounded(result, 65_536)?;
                if crate::audit::sensitive_value(result) {
                    return Err(SecuritySchemaProblem::SecretMaterial);
                }
            }
        }
        for (id, recovery) in &self.recovery {
            identifier(id, limits.max_name_bytes)?;
            let terminal = matches!(
                recovery.state,
                RecoveryState::Reconciled | RecoveryState::Failed
            );
            if recovery.terminal_tick == Some(0) || (!terminal && recovery.terminal_tick.is_some())
            {
                return Err(SecuritySchemaProblem::Malformed);
            }
            if recovery.id != *id
                || recovery.version == 0
                || !self.transactions.contains_key(&recovery.transaction_id)
            {
                return Err(SecuritySchemaProblem::MissingReference);
            }
        }
        let mut migration_ids = BTreeSet::new();
        for migration in &self.migrations {
            identifier(&migration.id, limits.max_name_bytes)?;
            if !migration_ids.insert(migration.id.as_str()) {
                return Err(SecuritySchemaProblem::Duplicate);
            }
            bounded(&migration.from_schema, limits.max_value_bytes)?;
            bounded(&migration.to_schema, limits.max_value_bytes)?;
            digest_sha256(&migration.source_digest)?;
            if let Some(result) = &migration.result_digest {
                digest_sha256(result)?;
            }
            let max_sources = limits
                .max_principals
                .checked_add(limits.max_groups)
                .and_then(|value| value.checked_add(limits.max_profiles))
                .and_then(|value| value.checked_add(limits.max_audits))
                .ok_or(SecuritySchemaProblem::LimitExceeded)?;
            if migration.source_rows.len() > max_sources {
                return Err(SecuritySchemaProblem::LimitExceeded);
            }
            for row in &migration.source_rows {
                if !matches!(
                    row.namespace.as_str(),
                    "racf-user" | "racf-group" | "racf-profile" | "racf-audit"
                ) || row.key.is_empty()
                    || row.key.len() > 1_024
                    || row.version == 0
                    || row.payload.len() > limits.max_database_bytes
                {
                    return Err(SecuritySchemaProblem::Malformed);
                }
            }
        }
        Ok(())
    }

    fn validate_policy_state(
        &self,
        limits: SecurityDatabaseLimits,
    ) -> Result<(), SecuritySchemaProblem> {
        if self.database_status.switch_generation == 0 || self.subsystem.restart_generation == 0 {
            return Err(SecuritySchemaProblem::Malformed);
        }
        if self.policy.password_minimum == 0
            || self.policy.password_minimum > self.policy.password_maximum
            || self.policy.password_maximum > limits.max_value_bytes
            || self.policy.phrase_minimum < self.policy.password_minimum
            || self.policy.phrase_minimum > self.policy.password_maximum
            || self.policy.password_history > 128
        {
            return Err(SecuritySchemaProblem::LimitExceeded);
        }
        for name in self.policy.set_flags.keys() {
            identifier(name, limits.max_name_bytes)?;
        }
        for (name, value) in &self.policy.values {
            identifier(name, limits.max_name_bytes)?;
            bounded(value, limits.max_value_bytes)?;
        }
        for (cache_class, cache) in &self.raclist_caches {
            class_name(cache_class)?;
            let class = self
                .classes
                .get(cache_class)
                .ok_or(SecuritySchemaProblem::MissingReference)?;
            if cache.class != *cache_class
                || cache.version == 0
                || cache.built_generation == 0
                || !class.raclist
                || cache.profiles.len() > limits.max_profiles
                || cache
                    .profiles
                    .values()
                    .any(|profile| profile.class != *cache_class)
            {
                return Err(SecuritySchemaProblem::Malformed);
            }
        }
        Ok(())
    }
}

const fn default_true() -> bool {
    true
}

const fn is_zero(value: &u64) -> bool {
    *value == 0
}

const fn default_phrase_minimum() -> usize {
    14
}

const fn default_password_history() -> usize {
    8
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
            .join("../../../conformance/subsystems/racf/schemas")
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
            phrase_credential: None,
            invalid_count: None,
            last_use_tick: None,
            profile_template: None,
            segments: BTreeMap::new(),
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
                generic_active: true,
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
                    profile_template: None,
                    segments: BTreeMap::new(),
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
    fn nonterminal_transaction_and_recovery_cannot_carry_forged_retention_age() {
        let mut snapshot = SecurityDatabaseSnapshot {
            retention_tick: 1,
            ..SecurityDatabaseSnapshot::default()
        };
        snapshot.transactions.insert(
            "ACTIVE-TX".into(),
            SecurityTransaction {
                id: "ACTIVE-TX".into(),
                idempotency_key: "ACTIVE-TX".into(),
                actor: "SYSTEM".into(),
                operation: "RECOVERY".into(),
                request_digest_format: SecurityRequestDigestFormat::RacfCommandCanonicalV1,
                request_digest: format!("sha256:{}", "a".repeat(64)),
                state: TransactionState::Intent,
                base_generation: 1,
                final_generation: Some(2),
                status: SafStatus {
                    saf_return_code: 8,
                    racf_return_code: 8,
                    racf_reason_code: 4,
                    reason: DecisionReason::RecoveryRequired,
                },
                terminal_result: None,
                terminal_tick: Some(1),
            },
        );
        assert_eq!(
            snapshot.validate(Default::default()),
            Err(SecuritySchemaProblem::Malformed)
        );

        let transaction = snapshot.transactions.get_mut("ACTIVE-TX").unwrap();
        transaction.state = TransactionState::Committed;
        snapshot.recovery.insert(
            "ACTIVE-RECOVERY".into(),
            RecoveryRecord {
                id: "ACTIVE-RECOVERY".into(),
                transaction_id: "ACTIVE-TX".into(),
                state: RecoveryState::Replaying,
                attempt: 1,
                last_error: None,
                version: 1,
                terminal_tick: Some(1),
            },
        );
        assert_eq!(
            snapshot.validate(Default::default()),
            Err(SecuritySchemaProblem::Malformed)
        );
    }

    #[test]
    fn principal_segments_are_bound_to_typed_profile_templates() {
        let mut snapshot = SecurityDatabaseSnapshot::default();
        snapshot.templates.insert(
            "USER".into(),
            ProfileTemplate {
                id: "USER".into(),
                version: 1,
                profile_kind: PrincipalKind::User,
                required_segments: BTreeSet::from(["OMVS".into()]),
                segments: BTreeMap::from([(
                    "OMVS".into(),
                    SegmentTemplate {
                        name: "OMVS".into(),
                        version: 1,
                        fields: BTreeMap::from([(
                            "UID".into(),
                            SegmentFieldSchema {
                                kind: SegmentFieldKind::Unsigned,
                                required: true,
                                max_bytes: 20,
                                max_items: 0,
                            },
                        )]),
                    },
                )]),
            },
        );
        let mut user = principal("IBMUSER");
        user.profile_template = Some("USER".into());
        user.segments.insert(
            "OMVS".into(),
            ProfileSegment {
                template: "OMVS".into(),
                template_version: 1,
                fields: BTreeMap::from([("UID".into(), SegmentValue::Unsigned(1000))]),
            },
        );
        snapshot.principals.insert("IBMUSER".into(), user);
        snapshot.validate(Default::default()).unwrap();
        snapshot
            .principals
            .get_mut("IBMUSER")
            .unwrap()
            .segments
            .get_mut("OMVS")
            .unwrap()
            .fields
            .insert("UID".into(), SegmentValue::Text("1000".into()));
        assert_eq!(
            snapshot.validate(Default::default()),
            Err(SecuritySchemaProblem::Malformed)
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
