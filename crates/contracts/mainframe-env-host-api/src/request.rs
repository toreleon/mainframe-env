use crate::clock::ClockRequest;
use crate::dataset::{
    CatalogKind, CatalogListEntry, CatalogResolution, DatasetDefinition, DatasetDescription,
    DatasetDiagnostic, DatasetLifecycleState, DatasetLockMode, DatasetLockReceipt,
    DatasetLockTarget, DatasetProviderCapabilities, DatasetSnapshot, TvsRecordOperation,
    TvsUnitOfWorkReceipt,
};
use crate::{DatasetName, JobName, MemberName, ResourceName, RuntimeServiceName, SessionId};
use mainframe_env_execution_api::{
    BoundedPayload, CapabilityId, IdempotencyKey, InvocationLimits, PrincipalId, RunUnitId,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;

mod dataset;
pub use dataset::{DatasetRequest, DatasetResult};

mod db2;
pub use db2::{Db2HostVariable, Db2Operation, Db2Request, Db2Result, Db2Row};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Resource ceilings for host request and result validation.
/// Byte limits count encoded bytes, not characters; providers may impose tighter limits.
pub struct HostLimits {
    /// Maximum encoded byte length for bounded names and identifiers.
    pub max_name_bytes: usize,
    /// Maximum bytes in one record, key, message or comparable record payload.
    pub max_record_bytes: usize,
    /// Maximum record or bounded listing count admitted by host validation.
    pub max_records: usize,
    /// Maximum count of fields, operands or qualifiers where the contract checks a field bound.
    pub max_fields: usize,
    /// Maximum number of supplemental audit key/value pairs.
    pub max_audit_fields: usize,
    /// Maximum bytes for state values and other payloads checked against the state byte ceiling.
    pub max_state_bytes: usize,
}
impl Default for HostLimits {
    fn default() -> Self {
        Self {
            max_name_bytes: 128,
            max_record_bytes: 1024 * 1024,
            max_records: 4096,
            max_fields: 512,
            max_audit_fields: 128,
            max_state_bytes: 4 * 1024 * 1024,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
/// Storage organization carried by the dataset contract.
/// A variant identifies the requested layout; provider capabilities determine availability.
pub enum DatasetOrganization {
    /// Sequential record dataset.
    Sequential,
    /// Directory of named members in a partitioned dataset.
    Partitioned,
    /// Extended partitioned dataset with member generations.
    PartitionedExtended,
    /// Records addressed by an embedded key.
    KeySequenced,
    /// Records addressed by entry position or relative byte address.
    EntrySequenced,
    /// Records addressed by relative record number.
    Relative,
    /// Relative records with variable-length payloads.
    VariableRelative,
    /// Byte-addressed linear dataset.
    Linear,
}
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
/// Logical record representation carried in dataset attributes.
/// These tags do not themselves implement physical blocking or spanning.
pub enum RecordFormat {
    /// Fixed-length logical records.
    Fixed,
    /// Fixed-length records grouped into blocks.
    FixedBlocked,
    /// Standard blocked fixed-length format tag.
    FixedBlockedStandard,
    /// Variable-length logical records.
    Variable,
    /// Variable-length records grouped into blocks.
    VariableBlocked,
    /// Variable-length records that may span blocks.
    VariableSpanned,
    /// Blocked variable-length format permitting spanning.
    VariableBlockedSpanned,
    /// Record structure supplied by the access method or caller.
    Undefined,
    /// Line-oriented record representation.
    Line,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Comparison used to position a keyed dataset browse relative to supplied key bytes.
pub enum KeyRelation {
    /// Select an exact key match.
    Equal,
    /// Select a key strictly greater than the supplied key.
    Greater,
    /// Select an equal key or the first greater key.
    GreaterOrEqual,
    /// Select a key strictly less than the supplied key.
    Less,
    /// Select an equal key or the first lesser key.
    LessOrEqual,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
/// Requested read-lock behavior, subject to the dataset provider and access mode.
pub enum DatasetReadLockMode {
    #[default]
    /// Use the provider-selected default lock behavior.
    Default,
    /// Request a read lock.
    Lock,
    /// Request a lock retained beyond the read.
    KeptLock,
    /// Request a read without acquiring a lock.
    NoLock,
    /// Request the provider-supported ignore-lock mode.
    IgnoreLock,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
/// Optional lock and wait controls accompanying a dataset read.
pub struct DatasetReadControl {
    /// Requested lock behavior for this read.
    pub lock: DatasetReadLockMode,
    /// Optional wait choice; `None` leaves selection to the provider.
    pub wait: Option<bool>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Selects the reel or unit form of a dataset close request.
pub enum DatasetReelUnit {
    /// Select reel-oriented close handling.
    Reel,
    /// Select unit-oriented close handling.
    Unit,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
/// Close options passed to the dataset provider for applicability checks.
pub struct DatasetCloseControl {
    /// Optional reel or unit close form.
    pub reel_or_unit: Option<DatasetReelUnit>,
    /// Request close without rewinding.
    pub no_rewind: bool,
    /// Request media removal handling.
    pub removal: bool,
    /// Request the provider's close-lock behavior.
    pub lock: bool,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Requested access level for a resource authorization decision.
pub enum AccessIntent {
    /// Read resource contents.
    Read,
    /// Execute the named resource.
    Execute,
    /// Update existing resource contents.
    Update,
    /// Request control-level resource access.
    Control,
    /// Request alter-level resource access.
    Alter,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Owned reference to a credential resolved by the security provider.
/// The string identifies a secret; callers should not place credential bytes in it.
pub struct SecretRef(String);

impl SecretRef {
    /// Own a nonempty credential reference within `max_name_bytes`.
    /// Returns `Malformed` for an empty, oversized or whitespace-containing value.
    pub fn new(value: impl Into<String>, limits: HostLimits) -> Result<Self, HostProblem> {
        let value = value.into();
        if value.is_empty()
            || value.len() > limits.max_name_bytes
            || value.contains(char::is_whitespace)
        {
            Err(HostProblem::Malformed)
        } else {
            Ok(Self(value))
        }
    }
    #[must_use]
    /// Borrow the retained reference string without resolving the secret.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
/// Logical dataset layout and optional character-encoding metadata.
/// Keyed layouts require a nonempty key wholly within the logical record.
pub struct DatasetAttributes {
    /// Requested record-addressing organization.
    pub organization: DatasetOrganization,
    /// Requested logical record representation.
    pub record_format: RecordFormat,
    /// Nonzero logical record length in bytes, bounded by `max_record_bytes`.
    pub logical_record_length: u32,
    /// Optional zero-based byte offset of the embedded key.
    pub key_offset: Option<u32>,
    /// Optional nonzero key length in bytes; supplied together with `key_offset`.
    pub key_length: Option<u32>,
    /// Optional nonzero coded character set identifier; absence carries no explicit encoding
    /// choice.
    pub ccsid: Option<u16>,
}

impl DatasetAttributes {
    /// Check record size, paired key bounds and nonzero optional CCSID.
    /// A key is required exactly for the key-sequenced organization; invalid layouts return
    /// `Malformed`.
    pub fn validate(&self, limits: HostLimits) -> Result<(), HostProblem> {
        if self.logical_record_length == 0
            || self.logical_record_length as usize > limits.max_record_bytes
            || self
                .key_offset
                .zip(self.key_length)
                .is_some_and(|(offset, length)| {
                    length == 0
                        || offset
                            .checked_add(length)
                            .is_none_or(|end| end > self.logical_record_length)
                })
            || self.key_offset.is_some() != self.key_length.is_some()
            || (self.organization == DatasetOrganization::KeySequenced) != self.key_offset.is_some()
            || self.ccsid == Some(0)
        {
            Err(HostProblem::Malformed)
        } else {
            Ok(())
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Replay identity for a host mutation, with an optional transaction association.
/// The outer effect must carry the same sequence and idempotency key.
pub struct Mutation {
    /// Nonzero effect sequence used to correlate and validate replay metadata.
    pub sequence: u64,
    /// Stable identity for replay protection of this mutation.
    pub idempotency_key: IdempotencyKey,
    /// Optional nonempty transaction identity bounded by `max_name_bytes`.
    pub transaction: Option<String>,
}

impl Mutation {
    /// Check the nonzero sequence and optional transaction-name bound.
    /// Outer key/sequence agreement is checked by `EffectRequest::validate`.
    pub fn validate(&self, limits: HostLimits) -> Result<(), HostProblem> {
        if self.sequence == 0
            || self
                .transaction
                .as_ref()
                .is_some_and(|value| value.is_empty() || value.len() > limits.max_name_bytes)
        {
            Err(HostProblem::Malformed)
        } else {
            Ok(())
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
/// Namespace used when resolving a named, versioned runtime service.
pub enum RuntimeServiceKind {
    /// Language Environment runtime-service namespace.
    LanguageEnvironment,
    /// Site or host extension runtime-service namespace.
    HostExtension,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Exact runtime-service lookup key, including a nonzero ABI version.
pub struct RuntimeServiceSelector {
    /// Runtime-service namespace used in the exact lookup key.
    pub kind: RuntimeServiceKind,
    /// Validated runtime-service name within the selected namespace.
    pub name: RuntimeServiceName,
    /// Nonzero ABI version selected explicitly; lookup does not choose a newer version.
    pub abi_version: u16,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Job-scoped spool operations using owned record bytes and explicit replay metadata.
pub enum SpoolRequest {
    /// Append bounded records to a job-owned spool file.
    Append {
        /// Validated job identity owning the spool file or artifacts.
        job: JobName,
        /// Nonempty job-relative spool file identifier bounded by `max_name_bytes`.
        file: String,
        /// Owned logical record bytes; count and individual lengths are host-bounded.
        records: Vec<Vec<u8>>,
        /// Replay sequence/key and optional transaction for this state change.
        mutation: Mutation,
    },
    /// List spool files owned by a job.
    List {
        /// Validated job identity owning the spool file or artifacts.
        job: JobName,
    },
    /// Read a bounded range of spool records.
    Read {
        /// Validated job identity owning the spool file or artifacts.
        job: JobName,
        /// Nonempty job-relative spool file identifier bounded by `max_name_bytes`.
        file: String,
        /// Zero-based starting record position for the requested spool range.
        start: u64,
        /// Positive requested record ceiling, at most `max_records`.
        max_records: u32,
    },
    /// Seal a spool file against further append operations.
    Seal {
        /// Validated job identity owning the spool file or artifacts.
        job: JobName,
        /// Nonempty job-relative spool file identifier bounded by `max_name_bytes`.
        file: String,
        /// Replay sequence/key and optional transaction for this state change.
        mutation: Mutation,
    },
    /// Request deletion of a job's spool artifacts.
    Purge {
        /// Validated job identity owning the spool file or artifacts.
        job: JobName,
        /// Replay sequence/key and optional transaction for this state change.
        mutation: Mutation,
    },
}

/// Version identifier for the typed spool request contract.
pub const SPOOL_REQUEST_CONTRACT: &str = "mainframe-env.spool-request@2";
/// Version identifier for spool results including incomplete purge reporting.
pub const SPOOL_RESULT_CONTRACT: &str = "mainframe-env.spool-result@2";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
/// Bounded metadata for one job-owned spool file.
pub struct SpoolFileSummary {
    /// Nonempty job-relative spool file identifier bounded by `max_name_bytes`.
    pub file: String,
    /// Total records reported for this spool file.
    pub record_count: u64,
    /// Total payload bytes reported for this spool file.
    pub byte_count: u64,
    /// Whether further append operations are prohibited for this file.
    pub sealed: bool,
    /// Provider-observed version associated with the returned data or mutation.
    pub version: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
/// Spool replies distinguishing applied mutations, reads and pending artifact deletion.
pub enum SpoolResult {
    /// Applied or replayed spool mutation with its version.
    Mutated {
        /// Provider-observed version associated with the returned data or mutation.
        version: u64,
        /// Whether the provider recognized a previously applied mutation identity.
        replayed: bool,
    },
    /// Spool file summaries for the selected job.
    Files {
        /// Bounded file summaries returned for the selected job.
        files: Vec<SpoolFileSummary>,
    },
    /// One bounded page of spool record bytes.
    Records {
        /// Owned logical record bytes; count and individual lengths are host-bounded.
        records: Vec<Vec<u8>>,
        /// Whether additional entries remain after this page.
        more: bool,
        /// Provider-observed version associated with the returned data or mutation.
        version: u64,
    },
    /// Deletion is incomplete and artifacts remain.
    PurgePending {
        /// Positive count of artifacts still awaiting deletion.
        remaining_artifacts: u64,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Owned terminal field metadata and uninterpreted display or input bytes.
/// Host validation bounds field count, name and value length; the terminal provider checks layout.
pub struct TerminalField {
    /// Nonempty field identifier bounded by `max_name_bytes`.
    pub name: String,
    /// Row coordinate interpreted by the terminal provider.
    pub row: u16,
    /// Column coordinate interpreted by the terminal provider.
    pub column: u16,
    /// Field capacity in bytes; validation rejects longer values.
    pub length: u16,
    /// Whether the field is marked as modified input.
    pub modified: bool,
    /// Whether the terminal should treat the field value as sensitive.
    pub secret: bool,
    /// Owned field bytes no longer than the declared capacity or `max_record_bytes`.
    pub value: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Session-scoped terminal operations carrying explicit geometry, input and display fields.
pub enum TerminalRequest {
    /// Open a session with explicit screen geometry.
    Open {
        /// Validated terminal session identity.
        session: SessionId,
        /// Requested terminal row count.
        rows: u16,
        /// Requested terminal column count.
        columns: u16,
    },
    /// Write display fields and optional cursor placement.
    Write {
        /// Validated terminal session identity.
        session: SessionId,
        /// Whether to clear the existing display before applying fields.
        erase: bool,
        /// Optional row/column cursor position interpreted by the terminal provider.
        cursor: Option<(u16, u16)>,
        /// Owned fields subject to the host field-count and field-value bounds.
        fields: Vec<TerminalField>,
    },
    /// Read the session's current input payload.
    Read {
        /// Validated terminal session identity.
        session: SessionId,
    },
    /// Supply an attention identifier and modified field values.
    Input {
        /// Validated terminal session identity.
        session: SessionId,
        /// Raw attention identifier byte accompanying terminal input.
        aid: u8,
        /// Owned fields subject to the host field-count and field-value bounds.
        fields: Vec<TerminalField>,
    },
    /// Release a terminal session.
    Release {
        /// Validated terminal session identity.
        session: SessionId,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Typed security operations that separate credential resolution, identity checks and
/// authorization.
pub enum SecurityRequest {
    /// Authenticate a principal through a secret reference.
    Authenticate {
        /// Principal identity to authenticate.
        user: PrincipalId,
        /// Reference resolved by the security provider to obtain credentials.
        credential_reference: SecretRef,
    },
    /// Validate a non-login execution identity from durable security state.
    /// This request never carries or resolves a credential.
    /// It is distinct from credential authentication.
    ValidatePrincipal {
        /// Principal identity whose retained validity or access is checked.
        principal: PrincipalId,
    },
    /// Check a principal's access to a named resource.
    Authorize {
        /// Principal identity whose retained validity or access is checked.
        principal: PrincipalId,
        /// Resource class presented to the security provider.
        class: String,
        /// Validated resource name for the authorization check.
        resource: ResourceName,
        /// Requested resource access level.
        intent: AccessIntent,
    },
    /// Submit a typed audit event.
    Audit(AuditEvent),
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Security-provider disposition without exposing credential material.
pub enum SecurityDecision {
    /// The requested security check succeeded.
    Allow,
    /// The requested access was denied.
    Deny,
    /// The referenced security identity or resource was not found.
    NotFound,
    /// Credential verification failed.
    InvalidCredentials,
    /// The checked credential or identity has expired.
    Expired,
    /// The checked identity or authority was revoked.
    Revoked,
    /// The checked identity is locked.
    Locked,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Owned audit metadata with a hashed resource identity and bounded field count.
pub struct AuditEvent {
    /// Audit action identifier supplied by the caller.
    pub action: String,
    /// Hashed resource identity for the audit record, rather than raw resource contents.
    pub resource_hash: String,
    /// Audit decision label supplied by the caller.
    pub decision: String,
    /// Supplemental audit pairs; the count is bounded by `max_audit_fields`.
    pub fields: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Version-aware operations on host-owned opaque state bytes.
pub enum StateRequest {
    /// Read an optional value and its state version.
    Get {
        /// Owned key identifying the host state value.
        key: String,
    },
    /// Store owned bytes with an optional version precondition.
    Put {
        /// Owned key identifying the host state value.
        key: String,
        /// Owned opaque state bytes bounded by `max_state_bytes`.
        value: Vec<u8>,
        /// Optional compare-and-update precondition on the current version.
        expected_version: Option<u64>,
        /// Replay sequence/key and optional transaction for this state change.
        mutation: Mutation,
    },
    /// Remove a state value with an optional version precondition.
    Delete {
        /// Owned key identifying the host state value.
        key: String,
        /// Optional compare-and-update precondition on the current version.
        expected_version: Option<u64>,
        /// Replay sequence/key and optional transaction for this state change.
        mutation: Mutation,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Typed operation selector for the IMS host request contract.
/// Provider applicability and scheduling rules are separate from this selector.
pub enum ImsOperation {
    /// Schedule the selected program specification block.
    Schedule,
    /// Terminate the scheduled context.
    Terminate,
    /// Retrieve a segment using the supplied path and qualifiers.
    GetUnique,
    /// Retrieve the next segment in sequence.
    GetNext,
    /// Retrieve the next segment within the parent context.
    GetNextParent,
    /// Retrieve a selected segment with hold intent.
    GetHoldUnique,
    /// Retrieve the next segment with hold intent.
    GetHoldNext,
    /// Retrieve the next segment under a parent with hold intent.
    GetHoldNextParent,
    /// Insert supplied segment bytes.
    Insert,
    /// Replace segment bytes in the selected context.
    Replace,
    /// Delete a selected segment.
    Delete,
    /// Request a checkpoint with an optional identifier.
    Checkpoint,
    /// Load the requested IMS context.
    Load,
    /// Unload the requested IMS context.
    Unload,
    /// Commit the current unit of work.
    Commit,
    /// Roll back the current unit of work.
    Rollback,
    /// Dispatch the attached typed system-service request.
    System,
}

impl ImsOperation {
    #[must_use]
    /// Whether this selector requires mutation replay metadata.
    /// Only `Unload` is classified as non-mutating; read selectors also retain replay metadata.
    pub const fn is_mutating(self) -> bool {
        !matches!(self, Self::Unload)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Segment/field selector carrying an owned comparison value.
pub struct ImsQualifier {
    /// Nonempty segment name bounded by `max_name_bytes`.
    pub segment: String,
    /// Nonempty field name bounded by `max_name_bytes`.
    pub field: String,
    /// Owned comparison bytes bounded by `max_record_bytes`.
    pub value: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Owned IMS call operands with explicit segment bounds and optional replay metadata.
/// Non-system calls require a nonzero PCB selector; only system calls carry `system`.
pub struct ImsRequest {
    /// Typed IMS call to dispatch.
    pub operation: ImsOperation,
    /// Optional program specification block name, bounded by `max_name_bytes`.
    pub psb: Option<String>,
    /// PCB selector; host validation requires it to be nonzero for non-system calls.
    pub pcb: u16,
    /// Ordered segment names identifying the requested path, bounded by `max_fields`.
    pub segments: Vec<String>,
    /// Owned segment bytes bounded by `max_record_bytes`, without implicit text decoding.
    pub data: Vec<u8>,
    /// Field/value qualifiers, bounded by `max_fields`.
    pub qualifiers: Vec<ImsQualifier>,
    /// Optional nonempty checkpoint identifier bounded by `max_name_bytes`.
    pub checkpoint_id: Option<String>,
    /// Positive returned-segment ceiling, at most `max_records`.
    pub max_segments: u32,
    /// Replay metadata required when the operation is classified as mutating.
    pub mutation: Option<Mutation>,
    /// Typed system-call operands. Present only for `ImsOperation::System`.
    pub system: Option<crate::ImsSystemRequest>,
    /// Q/LOCKCLASS reservation requested by a database Get call.
    pub q_class: Option<crate::ImsQClass>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// One returned segment with optional parent identity and uninterpreted record bytes.
pub struct ImsSegment {
    /// Nonempty returned segment name bounded by `max_name_bytes`.
    pub name: String,
    /// Optional parent identity bytes bounded by `max_record_bytes`.
    pub parent_key: Option<Vec<u8>>,
    /// Owned segment bytes bounded by `max_record_bytes`, without implicit text decoding.
    pub data: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// IMS reply with a two-byte status string and bounded returned segments.
pub struct ImsResult {
    /// Two-byte provider status string; interpreted according to the IMS call.
    pub status: String,
    /// Returned segment records bounded by `max_records`.
    pub segments: Vec<ImsSegment>,
    /// Optional nonempty checkpoint identifier bounded by `max_name_bytes`.
    pub checkpoint_id: Option<String>,
    /// Provider-reported count of affected segments.
    pub affected_segments: u64,
    /// Typed output for a system call; absent for existing database/TM calls.
    pub system: Option<crate::ImsSystemResult>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Operation selector for the replay-protected MQ host boundary.
pub enum MqOperation {
    /// Open a queue and obtain a provider handle.
    Open,
    /// Receive a bounded message through a handle.
    Get,
    /// Send a message through an open handle.
    Put,
    /// Send a message using a queue name without retaining a handle.
    PutOne,
    /// Close a provider-issued handle.
    Close,
    /// Commit the MQ unit of work.
    Commit,
    /// Roll back the MQ unit of work.
    Rollback,
}

impl MqOperation {
    #[must_use]
    /// Whether this selector requires mutation replay metadata.
    /// Every operation in this MQ boundary is classified as mutating.
    pub const fn is_mutating(self) -> bool {
        true
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Owned MQ operands with explicit receive bounds and replay metadata.
/// Host validation requires mutation metadata for every request in this contract.
pub struct MqRequest {
    /// Typed MQ call to dispatch.
    pub operation: MqOperation,
    /// Optional nonempty queue name bounded by `max_name_bytes`.
    pub queue: Option<String>,
    /// Optional handle issued by the provider for the selected context.
    pub handle: Option<u32>,
    /// Raw signed option bits interpreted for the selected MQ operation.
    pub options: i32,
    /// Owned message bytes bounded by `max_record_bytes`.
    pub message: Vec<u8>,
    /// Optional exact 24-byte message identity.
    pub message_id: Option<Vec<u8>>,
    /// Optional exact 24-byte correlation identity.
    pub correlation_id: Option<Vec<u8>>,
    /// Requested receive wait duration in logical ticks.
    pub wait_ticks: u64,
    /// Positive receive capacity in bytes, at most `max_record_bytes`.
    pub max_message_bytes: u32,
    /// Required replay sequence/key for this MQ request.
    pub mutation: Option<Mutation>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// MQ completion codes, optional handle and owned message data returned by a provider.
pub struct MqResult {
    /// Provider-returned signed MQ completion code.
    pub completion_code: i32,
    /// Provider-returned signed MQ reason code.
    pub reason_code: i32,
    /// Optional handle issued by the provider for the selected context.
    pub handle: Option<u32>,
    /// Owned message bytes bounded by `max_record_bytes`.
    pub message: Vec<u8>,
    /// Optional exact 24-byte message identity.
    pub message_id: Option<Vec<u8>>,
    /// Optional exact 24-byte correlation identity.
    pub correlation_id: Option<Vec<u8>>,
    /// Optional bounded program name supplied by trigger processing.
    pub trigger_program: Option<String>,
}

mod browse;
mod cics;
pub use cics::*;
mod program;
pub use program::*;

#[derive(Clone, Debug, Eq, PartialEq)]
/// Owned request dispatched under a built-in host capability.
/// Local shape validation does not establish provider support or resource authorization.
pub enum HostRequest {
    /// Dataset access and catalog operation.
    Dataset(DatasetRequest),
    /// Program control operation.
    Program(ProgramRequest),
    /// Job-owned spool operation.
    Spool(SpoolRequest),
    /// Terminal session operation.
    Terminal(TerminalRequest),
    /// Security identity, access or audit operation.
    Security(SecurityRequest),
    /// Clock value selection.
    Clock(ClockRequest),
    /// Opaque host state operation.
    State(StateRequest),
    /// Typed CICS command with its command-owned validation and replay identity.
    /// Mutation identity is checked before provider dispatch.
    Cics(CicsRequest),
    /// Relational statement or cursor operation.
    Db2(Db2Request),
    /// IMS segment or system-service operation.
    Ims(ImsRequest),
    /// MQ handle, message or transaction operation.
    Mq(MqRequest),
}

impl HostRequest {
    #[must_use]
    /// Return the built-in capability used for provider selection and invocation grants.
    /// This does not check resource-level authorization. Panics if `limits` rejects the built-in
    /// identity.
    pub fn required_capability(&self, limits: InvocationLimits) -> CapabilityId {
        let name = match self {
            Self::Dataset(
                DatasetRequest::Capabilities
                | DatasetRequest::List { .. }
                | DatasetRequest::Attributes { .. }
                | DatasetRequest::Describe { .. }
                | DatasetRequest::Diagnose { .. }
                | DatasetRequest::ResolveCatalog { .. }
                | DatasetRequest::ListCatalog { .. }
                | DatasetRequest::ListVolumes { .. }
                | DatasetRequest::ListLocks { .. }
                | DatasetRequest::TvsStatus { .. }
                | DatasetRequest::ListMembers { .. }
                | DatasetRequest::ReadMemberGeneration { .. }
                | DatasetRequest::Read { .. }
                | DatasetRequest::ReadGeneric { .. }
                | DatasetRequest::ReadConcatenation { .. }
                | DatasetRequest::ReadRelative { .. }
                | DatasetRequest::ReadRba { .. }
                | DatasetRequest::ReadSequential { .. }
                | DatasetRequest::Snapshot { .. }
                | DatasetRequest::ResolveGeneration { .. }
                | DatasetRequest::ReadNext { .. }
                | DatasetRequest::StartBrowse { .. }
                | DatasetRequest::ResetBrowse { .. }
                | DatasetRequest::EndBrowse { .. }
                | DatasetRequest::Close { .. },
            ) => "host.dataset.read",
            Self::Dataset(_) => "host.dataset.write",
            Self::Program(_) => "host.program.invoke",
            Self::Spool(SpoolRequest::List { .. } | SpoolRequest::Read { .. }) => "host.spool.read",
            Self::Spool(_) => "host.spool.write",
            Self::Terminal(_) => "host.terminal",
            Self::Security(SecurityRequest::Audit(_)) => "host.audit",
            Self::Security(_) => "host.security.authorize",
            Self::Clock(_) => "host.clock",
            Self::State(StateRequest::Get { .. }) => "host.state.read",
            Self::State(_) => "host.state.write",
            Self::Cics(_) => "host.cics.execute",
            Self::Db2(request) if request.operation.is_mutating() => "host.db2.write",
            Self::Db2(_) => "host.db2.read",
            Self::Ims(request) if request.operation.is_mutating() => "host.ims.write",
            Self::Ims(_) => "host.ims.read",
            Self::Mq(_) => "host.mq.write",
        };
        CapabilityId::new(name, limits).expect("built-in capability identities are valid")
    }

    #[must_use]
    /// Whether the request requires an outer idempotency key.
    /// Includes stateful program control and subsystem operations classified as mutations.
    pub fn is_mutating(&self) -> bool {
        matches!(
            self,
            Self::Dataset(
                DatasetRequest::Create { .. }
                    | DatasetRequest::Define { .. }
                    | DatasetRequest::Alter { .. }
                    | DatasetRequest::SetLifecycle { .. }
                    | DatasetRequest::RecordBackup { .. }
                    | DatasetRequest::Restore { .. }
                    | DatasetRequest::DefineCatalog { .. }
                    | DatasetRequest::SetCatalogConnection { .. }
                    | DatasetRequest::DefineAlias { .. }
                    | DatasetRequest::DefineMemberAlias { .. }
                    | DatasetRequest::WriteMemberGeneration { .. }
                    | DatasetRequest::DeleteMemberGeneration { .. }
                    | DatasetRequest::AcquireLock { .. }
                    | DatasetRequest::ReleaseLock { .. }
                    | DatasetRequest::BeginTvs { .. }
                    | DatasetRequest::StageTvs { .. }
                    | DatasetRequest::CompleteTvs { .. }
                    | DatasetRequest::ReconcileTvs { .. }
                    | DatasetRequest::Write { .. }
                    | DatasetRequest::Append { .. }
                    | DatasetRequest::Truncate { .. }
                    | DatasetRequest::RewriteRecord { .. }
                    | DatasetRequest::DeleteRecord { .. }
                    | DatasetRequest::DefineAlternateIndex { .. }
                    | DatasetRequest::BuildAlternateIndex { .. }
                    | DatasetRequest::DefinePath { .. }
                    | DatasetRequest::WriteRelative { .. }
                    | DatasetRequest::DeleteRelative { .. }
                    | DatasetRequest::WriteRba { .. }
                    | DatasetRequest::DefineGenerationGroup { .. }
                    | DatasetRequest::CreateGeneration { .. }
                    | DatasetRequest::Rename { .. }
                    | DatasetRequest::Delete { .. }
            ) | Self::Spool(
                SpoolRequest::Append { .. }
                    | SpoolRequest::Seal { .. }
                    | SpoolRequest::Purge { .. }
            ) | Self::Program(
                ProgramRequest::Call { .. }
                    | ProgramRequest::Invoke { .. }
                    | ProgramRequest::Link { .. }
                    | ProgramRequest::Xctl { .. }
                    | ProgramRequest::Return { .. }
                    | ProgramRequest::Cancel { .. }
                    | ProgramRequest::Abend { .. }
            ) | Self::State(StateRequest::Put { .. } | StateRequest::Delete { .. })
        ) || matches!(self, Self::Cics(request) if request.is_mutating())
            || matches!(self, Self::Db2(request) if request.operation.is_mutating())
            || matches!(self, Self::Ims(request) if request.operation.is_mutating())
            || matches!(self, Self::Mq(request) if request.operation.is_mutating())
    }

    #[must_use]
    /// Borrow payload-level replay metadata when this request carries it.
    /// Some mutating program requests have only the outer effect key and return `None` here.
    pub fn mutation(&self) -> Option<&Mutation> {
        match self {
            Self::Dataset(
                DatasetRequest::Create { mutation, .. }
                | DatasetRequest::Define { mutation, .. }
                | DatasetRequest::Alter { mutation, .. }
                | DatasetRequest::SetLifecycle { mutation, .. }
                | DatasetRequest::RecordBackup { mutation, .. }
                | DatasetRequest::Restore { mutation, .. }
                | DatasetRequest::DefineCatalog { mutation, .. }
                | DatasetRequest::SetCatalogConnection { mutation, .. }
                | DatasetRequest::DefineAlias { mutation, .. }
                | DatasetRequest::DefineMemberAlias { mutation, .. }
                | DatasetRequest::WriteMemberGeneration { mutation, .. }
                | DatasetRequest::DeleteMemberGeneration { mutation, .. }
                | DatasetRequest::AcquireLock { mutation, .. }
                | DatasetRequest::ReleaseLock { mutation, .. }
                | DatasetRequest::BeginTvs { mutation, .. }
                | DatasetRequest::StageTvs { mutation, .. }
                | DatasetRequest::CompleteTvs { mutation, .. }
                | DatasetRequest::ReconcileTvs { mutation, .. }
                | DatasetRequest::Write { mutation, .. }
                | DatasetRequest::Append { mutation, .. }
                | DatasetRequest::Truncate { mutation, .. }
                | DatasetRequest::RewriteRecord { mutation, .. }
                | DatasetRequest::DeleteRecord { mutation, .. }
                | DatasetRequest::DefineAlternateIndex { mutation, .. }
                | DatasetRequest::BuildAlternateIndex { mutation, .. }
                | DatasetRequest::DefinePath { mutation, .. }
                | DatasetRequest::WriteRelative { mutation, .. }
                | DatasetRequest::DeleteRelative { mutation, .. }
                | DatasetRequest::WriteRba { mutation, .. }
                | DatasetRequest::DefineGenerationGroup { mutation, .. }
                | DatasetRequest::CreateGeneration { mutation, .. }
                | DatasetRequest::Rename { mutation, .. }
                | DatasetRequest::Delete { mutation, .. },
            )
            | Self::Spool(
                SpoolRequest::Append { mutation, .. }
                | SpoolRequest::Seal { mutation, .. }
                | SpoolRequest::Purge { mutation, .. },
            )
            | Self::State(
                StateRequest::Put { mutation, .. } | StateRequest::Delete { mutation, .. },
            ) => Some(mutation),
            Self::Cics(request) => request.mutation.as_ref(),
            Self::Db2(request) => request.mutation.as_ref(),
            Self::Ims(request) => request.mutation.as_ref(),
            Self::Mq(request) => request.mutation.as_ref(),
            _ => None,
        }
    }

    /// Check locally enforced payload shapes and resource bounds.
    /// Provider-specific operand applicability, readiness and authorization are checked at
    /// dispatch.
    pub fn validate(&self, limits: HostLimits) -> Result<(), HostProblem> {
        match self {
            Self::Dataset(request) => validate_dataset(request, limits),
            Self::Program(ProgramRequest::Call {
                service: Some(service),
                ..
            }) if service.abi_version == 0 => Err(HostProblem::Malformed),
            Self::Program(ProgramRequest::Link {
                selection: Some(selection),
                ..
            }) if !selection.is_valid() => Err(HostProblem::Malformed),
            Self::Program(ProgramRequest::Cancel { programs })
                if programs.is_empty() || programs.len() > limits.max_fields =>
            {
                Err(HostProblem::ResourceExhausted)
            }
            Self::Spool(SpoolRequest::Append {
                file,
                records,
                mutation,
                ..
            }) => {
                validate_spool_file(file, limits)?;
                validate_records(records, limits)?;
                mutation.validate(limits)
            }
            Self::Spool(SpoolRequest::Read {
                file, max_records, ..
            }) => {
                validate_spool_file(file, limits)?;
                if *max_records == 0 || *max_records as usize > limits.max_records {
                    Err(HostProblem::ResourceExhausted)
                } else {
                    Ok(())
                }
            }
            Self::Spool(SpoolRequest::Seal { file, mutation, .. }) => {
                validate_spool_file(file, limits)?;
                mutation.validate(limits)
            }
            Self::Spool(SpoolRequest::Purge { mutation, .. }) => mutation.validate(limits),
            Self::Terminal(
                TerminalRequest::Write { fields, .. } | TerminalRequest::Input { fields, .. },
            ) => validate_fields(fields, limits),
            Self::Security(SecurityRequest::Audit(event)) => {
                if event.fields.len() > limits.max_audit_fields {
                    Err(HostProblem::ResourceExhausted)
                } else {
                    Ok(())
                }
            }
            Self::State(StateRequest::Put {
                value, mutation, ..
            }) => {
                if value.len() > limits.max_state_bytes {
                    return Err(HostProblem::ResourceExhausted);
                }
                mutation.validate(limits)
            }
            Self::State(StateRequest::Delete { mutation, .. }) => mutation.validate(limits),
            Self::Cics(request) => request.validate(limits),
            Self::Db2(request) => {
                if request.statement.len() > limits.max_state_bytes
                    || request.cursor.as_ref().is_some_and(|cursor| {
                        cursor.is_empty() || cursor.len() > limits.max_name_bytes
                    })
                    || request.inputs.len() > limits.max_fields
                    || request.outputs.len() > limits.max_fields
                    || request.max_rows as usize > limits.max_records
                    || request.inputs.iter().any(|(name, variable)| {
                        name.is_empty()
                            || name.len() > limits.max_name_bytes
                            || variable.value.len() > limits.max_record_bytes
                    })
                    || request
                        .outputs
                        .iter()
                        .any(|name| name.is_empty() || name.len() > limits.max_name_bytes)
                {
                    return Err(HostProblem::ResourceExhausted);
                }
                if request.operation.is_mutating() {
                    request
                        .mutation
                        .as_ref()
                        .ok_or(HostProblem::MissingIdempotency)?
                        .validate(limits)?;
                }
                Ok(())
            }
            Self::Ims(request) => {
                if (request.operation == ImsOperation::System) != request.system.is_some()
                    || request.q_class.is_some_and(|class| !class.is_valid())
                    || request.q_class.is_some()
                        && !matches!(
                            request.operation,
                            ImsOperation::GetUnique
                                | ImsOperation::GetNext
                                | ImsOperation::GetNextParent
                                | ImsOperation::GetHoldUnique
                                | ImsOperation::GetHoldNext
                                | ImsOperation::GetHoldNextParent
                        )
                {
                    return Err(HostProblem::Malformed);
                }
                if request.pcb == 0 && request.operation != ImsOperation::System
                    || request.segments.len() > limits.max_fields
                    || request.data.len() > limits.max_record_bytes
                    || request.qualifiers.len() > limits.max_fields
                    || request.max_segments == 0
                    || request.max_segments as usize > limits.max_records
                    || request
                        .psb
                        .as_ref()
                        .is_some_and(|name| name.is_empty() || name.len() > limits.max_name_bytes)
                    || request
                        .segments
                        .iter()
                        .any(|name| name.is_empty() || name.len() > limits.max_name_bytes)
                    || request.qualifiers.iter().any(|qualifier| {
                        qualifier.segment.is_empty()
                            || qualifier.segment.len() > limits.max_name_bytes
                            || qualifier.field.is_empty()
                            || qualifier.field.len() > limits.max_name_bytes
                            || qualifier.value.len() > limits.max_record_bytes
                    })
                    || request
                        .checkpoint_id
                        .as_ref()
                        .is_some_and(|id| id.is_empty() || id.len() > limits.max_name_bytes)
                {
                    return Err(HostProblem::ResourceExhausted);
                }
                if let Some(system) = &request.system {
                    system.validate(limits)?;
                }
                if request.operation.is_mutating() {
                    request
                        .mutation
                        .as_ref()
                        .ok_or(HostProblem::MissingIdempotency)?
                        .validate(limits)?;
                }
                Ok(())
            }
            Self::Mq(request) => {
                if request
                    .queue
                    .as_ref()
                    .is_some_and(|queue| queue.is_empty() || queue.len() > limits.max_name_bytes)
                    || request.message.len() > limits.max_record_bytes
                    || request
                        .message_id
                        .as_ref()
                        .is_some_and(|value| value.len() != 24)
                    || request
                        .correlation_id
                        .as_ref()
                        .is_some_and(|value| value.len() != 24)
                    || request.max_message_bytes == 0
                    || request.max_message_bytes as usize > limits.max_record_bytes
                {
                    return Err(HostProblem::ResourceExhausted);
                }
                request
                    .mutation
                    .as_ref()
                    .ok_or(HostProblem::MissingIdempotency)?
                    .validate(limits)
            }
            _ => Ok(()),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Typed successful host reply.
/// Provider failures and uncertain outcomes are carried by `EffectResult::outcome`.
pub enum HostResult {
    /// Dataset access and catalog reply.
    Dataset(DatasetResult),
    /// Program control schema-tagged bounded reply bytes.
    Program(BoundedPayload),
    /// Job-owned spool reply.
    Spool(SpoolResult),
    /// Terminal session schema-tagged bounded reply bytes.
    Terminal(BoundedPayload),
    /// Security decision.
    Security(SecurityDecision),
    /// Clock value formatted by the clock provider.
    Clock(String),
    /// Opaque host state value and version.
    State {
        /// Optional owned state bytes; absence represents no returned value.
        value: Option<Vec<u8>>,
        /// Provider-observed version associated with the returned data or mutation.
        version: u64,
    },
    /// CICS command response and control disposition.
    Cics(CicsResponse),
    /// Relational statement or cursor reply.
    Db2(Db2Result),
    /// IMS segment or system-service reply.
    Ims(ImsResult),
    /// MQ handle, message or transaction reply.
    Mq(MqResult),
}

impl HostResult {
    /// Check locally enforced reply bounds and structural relationships.
    /// Does not interpret subsystem status codes as execution success or failure.
    pub fn validate(&self, limits: HostLimits) -> Result<(), HostProblem> {
        match self {
            Self::Dataset(DatasetResult::Description(description)) => {
                description.definition.validate(
                    limits,
                    DatasetProviderCapabilities::all_contract_capabilities(),
                )?;
                if description.extents.is_empty()
                    || description.extents.len() > limits.max_records
                    || description.buffer_bytes == 0
                    || description.abstract_placement.is_empty()
                    || description.abstract_placement.len() > limits.max_name_bytes
                {
                    return Err(HostProblem::Malformed);
                }
                let mut next_start = 0u64;
                for (position, extent) in description.extents.iter().enumerate() {
                    if extent.ordinal != u32::try_from(position).unwrap_or(u32::MAX)
                        || extent.start != next_start
                        || extent.length == 0
                        || extent.volume_id.is_empty()
                        || extent.volume_id.len() > limits.max_name_bytes
                    {
                        return Err(HostProblem::Malformed);
                    }
                    next_start = next_start
                        .checked_add(extent.length)
                        .ok_or(HostProblem::ResourceExhausted)?;
                }
                if next_start != description.allocated_bytes
                    || description.high_used_rba > description.max_rba
                {
                    Err(HostProblem::Malformed)
                } else {
                    Ok(())
                }
            }
            Self::Dataset(DatasetResult::Catalog(resolution))
                if resolution.alias_chain.len() > limits.max_records || resolution.version == 0 =>
            {
                Err(HostProblem::ResourceExhausted)
            }
            Self::Dataset(DatasetResult::CatalogEntries { entries, .. })
                if entries.len() > limits.max_records
                    || entries.iter().any(|entry| entry.version == 0) =>
            {
                Err(HostProblem::ResourceExhausted)
            }
            Self::Dataset(DatasetResult::Volumes { volumes, .. }) => {
                if volumes.len() > limits.max_records {
                    return Err(HostProblem::ResourceExhausted);
                }
                let mut previous_volume = None;
                for volume in volumes {
                    if volume.volume_id.is_empty()
                        || volume.volume_id.len() > limits.max_name_bytes
                        || previous_volume
                            .is_some_and(|previous: &str| previous >= volume.volume_id.as_str())
                        || volume.extents.is_empty()
                        || volume.extents.len() > limits.max_records
                        || volume.used_bytes > volume.allocated_bytes
                    {
                        return Err(HostProblem::Malformed);
                    }
                    previous_volume = Some(&volume.volume_id);
                    let mut next_start = 0u64;
                    for extent in &volume.extents {
                        if extent.volume_start != next_start || extent.length == 0 {
                            return Err(HostProblem::Malformed);
                        }
                        next_start = next_start
                            .checked_add(extent.length)
                            .ok_or(HostProblem::ResourceExhausted)?;
                    }
                    if next_start != volume.allocated_bytes {
                        return Err(HostProblem::Malformed);
                    }
                }
                Ok(())
            }
            Self::Dataset(DatasetResult::Locks { locks })
                if locks.len() > limits.max_records
                    || locks.iter().any(|lock| {
                        lock.lock_id.is_empty()
                            || lock.lock_id.len() > limits.max_name_bytes
                            || lock.expires_at == 0
                            || lock.version == 0
                            || matches!(
                                &lock.target,
                                DatasetLockTarget::Record(identity)
                                    if identity.is_empty()
                                        || identity.len() > limits.max_record_bytes
                            )
                            || lock.transaction.as_ref().is_some_and(|transaction| {
                                transaction.is_empty() || transaction.len() > limits.max_name_bytes
                            })
                    }) =>
            {
                Err(HostProblem::ResourceExhausted)
            }
            Self::Dataset(DatasetResult::Tvs(receipt))
                if receipt.transaction.is_empty()
                    || receipt.transaction.len() > limits.max_name_bytes
                    || receipt.version == 0 =>
            {
                Err(HostProblem::Malformed)
            }
            Self::Dataset(DatasetResult::Snapshot { snapshot, version }) => {
                if *version == 0 {
                    Err(HostProblem::Malformed)
                } else {
                    validate_dataset_snapshot(snapshot, limits)
                }
            }
            Self::Dataset(DatasetResult::MemberGeneration { generation: 0, .. }) => {
                Err(HostProblem::Malformed)
            }
            Self::Dataset(DatasetResult::Diagnostics { diagnostics })
                if diagnostics.len() > limits.max_records
                    || diagnostics.iter().any(|diagnostic| {
                        diagnostic.code.is_empty()
                            || diagnostic.code.len() > limits.max_name_bytes
                            || diagnostic
                                .field
                                .as_ref()
                                .is_some_and(|field| field.len() > limits.max_name_bytes)
                            || diagnostic.detail.len() > limits.max_state_bytes
                    }) =>
            {
                Err(HostProblem::ResourceExhausted)
            }
            Self::Dataset(DatasetResult::Listed { names, .. })
                if names.len() > limits.max_records =>
            {
                Err(HostProblem::ResourceExhausted)
            }
            Self::Dataset(DatasetResult::Members { names, .. })
                if names.len() > limits.max_records =>
            {
                Err(HostProblem::ResourceExhausted)
            }
            Self::Dataset(DatasetResult::Records {
                records,
                identities,
                ..
            })
            | Self::Dataset(DatasetResult::MemberGeneration {
                records,
                identities,
                ..
            }) => {
                validate_records(records, limits)?;
                if identities.len() != records.len()
                    || identities
                        .iter()
                        .any(|identity| identity.len() > limits.max_record_bytes)
                {
                    Err(HostProblem::Malformed)
                } else {
                    Ok(())
                }
            }
            Self::Dataset(DatasetResult::Rba {
                data,
                rba,
                next_rba,
                ..
            }) if data.len() > limits.max_record_bytes
                || *next_rba < *rba
                || next_rba.saturating_sub(*rba)
                    != u64::try_from(data.len()).unwrap_or(u64::MAX) =>
            {
                Err(HostProblem::Malformed)
            }
            Self::Dataset(DatasetResult::Browse {
                record,
                identity,
                key,
                ..
            }) if record
                .as_ref()
                .is_some_and(|value| value.len() > limits.max_record_bytes)
                || identity
                    .as_ref()
                    .is_some_and(|value| value.len() > limits.max_record_bytes)
                || key
                    .as_ref()
                    .is_some_and(|value| value.len() > limits.max_record_bytes) =>
            {
                Err(HostProblem::ResourceExhausted)
            }
            Self::Dataset(DatasetResult::Browse {
                record,
                identity,
                key,
                ..
            }) if record.is_some() != identity.is_some() || record.is_some() != key.is_some() => {
                Err(HostProblem::Malformed)
            }
            Self::Spool(SpoolResult::Files { files })
                if files.len() > limits.max_records
                    || files.iter().any(|file| {
                        file.file.is_empty()
                            || file.file.len() > limits.max_name_bytes
                            || file.file.chars().any(char::is_control)
                            || usize::try_from(file.record_count)
                                .map_or(true, |count| count > limits.max_records)
                            || file.version == 0
                    }) =>
            {
                Err(HostProblem::ResourceExhausted)
            }
            Self::Spool(SpoolResult::Records {
                records, version, ..
            }) if *version == 0
                || records.len() > limits.max_records
                || records
                    .iter()
                    .any(|record| record.len() > limits.max_record_bytes) =>
            {
                Err(HostProblem::ResourceExhausted)
            }
            Self::Spool(SpoolResult::Mutated { version, .. }) if *version == 0 => {
                Err(HostProblem::Malformed)
            }
            Self::Spool(SpoolResult::PurgePending {
                remaining_artifacts,
            }) if *remaining_artifacts == 0 => Err(HostProblem::Malformed),
            Self::Program(payload) | Self::Terminal(payload)
                if payload.bytes().len() > limits.max_state_bytes =>
            {
                Err(HostProblem::ResourceExhausted)
            }
            Self::Cics(response)
                if response.condition.len() > limits.max_name_bytes
                    || response.applid.len() > limits.max_name_bytes
                    || response.sysid.len() > limits.max_name_bytes
                    || response.transaction.len() > limits.max_name_bytes
                    || response
                        .target
                        .as_ref()
                        .is_some_and(|value| value.len() > limits.max_name_bytes)
                    || response
                        .next_transaction
                        .as_ref()
                        .is_some_and(|value| value.len() > limits.max_name_bytes)
                    || response.payload.bytes().len() > limits.max_state_bytes
                    || response.outputs.len() > limits.max_fields
                    || response
                        .outputs
                        .values()
                        .any(|value| value.bytes().len() > limits.max_state_bytes)
                    || response
                        .outputs
                        .values()
                        .try_fold(0usize, |total, value| {
                            total.checked_add(value.bytes().len())
                        })
                        .is_none_or(|total| total > limits.max_state_bytes) =>
            {
                Err(HostProblem::ResourceExhausted)
            }
            Self::Db2(result)
                if result.sqlstate.len() != 5
                    || result.message.len() > limits.max_state_bytes
                    || result.rows.len() > limits.max_records
                    || result.rows.iter().any(|row| {
                        row.columns.len() > limits.max_fields
                            || row
                                .columns
                                .iter()
                                .any(|column| column.len() > limits.max_record_bytes)
                    }) =>
            {
                Err(HostProblem::ResourceExhausted)
            }
            Self::Ims(result)
                if result.status.len() != 2
                    || result.segments.len() > limits.max_records
                    || result
                        .checkpoint_id
                        .as_ref()
                        .is_some_and(|id| id.is_empty() || id.len() > limits.max_name_bytes)
                    || result.segments.iter().any(|segment| {
                        segment.name.is_empty()
                            || segment.name.len() > limits.max_name_bytes
                            || segment.data.len() > limits.max_record_bytes
                            || segment
                                .parent_key
                                .as_ref()
                                .is_some_and(|key| key.len() > limits.max_record_bytes)
                    }) =>
            {
                Err(HostProblem::ResourceExhausted)
            }
            Self::Ims(result)
                if result
                    .system
                    .as_ref()
                    .is_some_and(|system| system.validate(limits).is_err()) =>
            {
                Err(HostProblem::ResourceExhausted)
            }
            Self::Mq(result)
                if result.message.len() > limits.max_record_bytes
                    || result
                        .message_id
                        .as_ref()
                        .is_some_and(|value| value.len() != 24)
                    || result
                        .correlation_id
                        .as_ref()
                        .is_some_and(|value| value.len() != 24)
                    || result.trigger_program.as_ref().is_some_and(|program| {
                        program.is_empty() || program.len() > limits.max_name_bytes
                    }) =>
            {
                Err(HostProblem::ResourceExhausted)
            }
            Self::State {
                value: Some(value), ..
            } if value.len() > limits.max_state_bytes => Err(HostProblem::ResourceExhausted),
            Self::Clock(value) if value.len() > limits.max_name_bytes => {
                Err(HostProblem::ResourceExhausted)
            }
            _ => Ok(()),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Invocation-scoped effect envelope with a deadline and replay identity.
/// Mutating requests require an outer key even when their payload has no `Mutation` field.
pub struct EffectRequest {
    /// Run-unit identity that must match the active invocation before dispatch.
    pub run_unit: RunUnitId,
    /// Nonzero effect sequence used to correlate and validate replay metadata.
    pub sequence: u64,
    /// Nonzero logical deadline; dispatch rejects when the current tick reaches it.
    pub deadline_tick: u64,
    /// Outer replay key required for mutating requests; must match payload metadata when present.
    pub idempotency_key: Option<IdempotencyKey>,
    /// Owned typed request carried by this envelope.
    pub request: HostRequest,
}

impl EffectRequest {
    /// Require a nonzero sequence/deadline and validate the typed payload.
    /// For mutations, require an outer key and reject disagreement with payload replay metadata.
    pub fn validate(&self, limits: HostLimits) -> Result<(), HostProblem> {
        if self.sequence == 0 || self.deadline_tick == 0 {
            return Err(HostProblem::Malformed);
        }
        if self.request.is_mutating() {
            let key = self
                .idempotency_key
                .as_ref()
                .ok_or(HostProblem::MissingIdempotency)?;
            if let Some(mutation) = self.request.mutation()
                && (&mutation.idempotency_key != key || mutation.sequence != self.sequence)
            {
                return Err(HostProblem::IdempotencyConflict);
            }
        }
        self.request.validate(limits)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Sequence-bound reply that separates successful data from typed host failures.
pub struct EffectResult {
    /// Nonzero effect sequence used to correlate and validate replay metadata.
    pub sequence: u64,
    /// Successful typed reply or failure; uncertainty is preserved as `UnknownOutcome`.
    pub outcome: Result<HostResult, HostProblem>,
}

impl EffectResult {
    /// Require the expected nonzero sequence and validate either reply data or structured failure
    /// fields.
    pub fn validate(&self, expected_sequence: u64, limits: HostLimits) -> Result<(), HostProblem> {
        if self.sequence == 0 || self.sequence != expected_sequence {
            return Err(HostProblem::Malformed);
        }
        match &self.outcome {
            Ok(result) => result.validate(limits)?,
            Err(problem) => problem.validate(limits)?,
        }
        Ok(())
    }
}

fn validate_dataset(request: &DatasetRequest, limits: HostLimits) -> Result<(), HostProblem> {
    match request {
        DatasetRequest::Capabilities
        | DatasetRequest::Describe { .. }
        | DatasetRequest::Diagnose { .. }
        | DatasetRequest::ResolveCatalog { .. } => Ok(()),
        DatasetRequest::ListCatalog {
            pattern, max_items, ..
        } if pattern.len() > limits.max_name_bytes
            || *max_items == 0
            || *max_items as usize > limits.max_records =>
        {
            Err(HostProblem::ResourceExhausted)
        }
        DatasetRequest::ListVolumes { start, max_items }
            if *max_items == 0
                || *max_items as usize > limits.max_records
                || start.as_ref().is_some_and(|start| {
                    start.is_empty() || start.len() > limits.max_name_bytes
                }) =>
        {
            Err(HostProblem::ResourceExhausted)
        }
        DatasetRequest::ListLocks {
            now_tick,
            max_items,
            ..
        } => {
            if *now_tick == 0 {
                Err(HostProblem::Malformed)
            } else if *max_items == 0 || *max_items as usize > limits.max_records {
                Err(HostProblem::ResourceExhausted)
            } else {
                Ok(())
            }
        }
        DatasetRequest::TvsStatus { transaction, .. } => validate_transaction(transaction, limits),
        DatasetRequest::List {
            max_items, pattern, ..
        } if *max_items == 0
            || *max_items as usize > limits.max_records
            || pattern.len() > limits.max_name_bytes =>
        {
            Err(HostProblem::ResourceExhausted)
        }
        DatasetRequest::ListMembers { max_items, .. }
            if *max_items == 0 || *max_items as usize > limits.max_records =>
        {
            Err(HostProblem::ResourceExhausted)
        }
        DatasetRequest::ReadMemberGeneration { relative, .. } if *relative > 0 => {
            Err(HostProblem::Malformed)
        }
        DatasetRequest::ReadMemberGeneration { max_records, .. }
            if *max_records == 0 || *max_records as usize > limits.max_records =>
        {
            Err(HostProblem::ResourceExhausted)
        }
        DatasetRequest::Read {
            key, max_records, ..
        } if *max_records == 0
            || *max_records as usize > limits.max_records
            || key
                .as_ref()
                .is_some_and(|value| value.len() > limits.max_record_bytes) =>
        {
            Err(HostProblem::ResourceExhausted)
        }
        DatasetRequest::ReadGeneric {
            key_prefix,
            max_records,
            ..
        } if key_prefix.is_empty()
            || key_prefix.len() > limits.max_record_bytes
            || *max_records == 0
            || *max_records as usize > limits.max_records =>
        {
            Err(HostProblem::ResourceExhausted)
        }
        DatasetRequest::ReadConcatenation {
            datasets,
            max_records,
            ..
        } if datasets.is_empty()
            || datasets.len() > limits.max_records
            || *max_records == 0
            || *max_records as usize > limits.max_records =>
        {
            Err(HostProblem::ResourceExhausted)
        }
        DatasetRequest::ReadRelative { record_number, .. } if *record_number == 0 => {
            Err(HostProblem::Malformed)
        }
        DatasetRequest::ReadRba { max_bytes, .. }
            if *max_bytes == 0 || *max_bytes as usize > limits.max_record_bytes =>
        {
            Err(HostProblem::ResourceExhausted)
        }
        DatasetRequest::ReadSequential { max_records, .. }
            if *max_records == 0 || *max_records as usize > limits.max_records =>
        {
            Err(HostProblem::ResourceExhausted)
        }
        DatasetRequest::Snapshot {
            max_records,
            max_members,
            ..
        } if *max_records == 0
            || *max_members == 0
            || *max_records as usize > limits.max_records
            || *max_members as usize > limits.max_records =>
        {
            Err(HostProblem::ResourceExhausted)
        }
        DatasetRequest::Create {
            attributes,
            mutation,
            ..
        } => {
            attributes.validate(limits)?;
            mutation.validate(limits)
        }
        DatasetRequest::Define {
            definition,
            mutation,
            ..
        }
        | DatasetRequest::Alter {
            definition,
            mutation,
            ..
        } => {
            definition.validate(
                limits,
                DatasetProviderCapabilities::all_contract_capabilities(),
            )?;
            mutation.validate(limits)
        }
        DatasetRequest::SetLifecycle { mutation, .. }
        | DatasetRequest::RecordBackup { mutation, .. } => mutation.validate(limits),
        DatasetRequest::Restore {
            snapshot, mutation, ..
        } => {
            validate_dataset_snapshot(snapshot, limits)?;
            mutation.validate(limits)
        }
        DatasetRequest::DefineCatalog { mutation, .. }
        | DatasetRequest::SetCatalogConnection { mutation, .. }
        | DatasetRequest::DefineAlias { mutation, .. }
        | DatasetRequest::DefineMemberAlias { mutation, .. } => mutation.validate(limits),
        DatasetRequest::WriteMemberGeneration {
            records, mutation, ..
        } => {
            validate_records(records, limits)?;
            mutation.validate(limits)
        }
        DatasetRequest::DeleteMemberGeneration {
            generation,
            mutation,
            ..
        } => {
            if *generation == 0 {
                Err(HostProblem::Malformed)
            } else {
                mutation.validate(limits)
            }
        }
        DatasetRequest::AcquireLock {
            target,
            now_tick,
            lease_ticks,
            transaction,
            mutation,
            ..
        } => {
            if *now_tick == 0
                || *lease_ticks == 0
                || matches!(target, DatasetLockTarget::Record(identity) if identity.is_empty())
            {
                return Err(HostProblem::Malformed);
            }
            if matches!(target, DatasetLockTarget::Record(identity) if identity.len() > limits.max_record_bytes)
            {
                return Err(HostProblem::ResourceExhausted);
            }
            if let Some(transaction) = transaction {
                validate_transaction(transaction, limits)?;
            }
            mutation.validate(limits)
        }
        DatasetRequest::ReleaseLock {
            lock_id, mutation, ..
        } => {
            if lock_id.is_empty() || lock_id.len() > limits.max_name_bytes {
                Err(HostProblem::Malformed)
            } else {
                mutation.validate(limits)
            }
        }
        DatasetRequest::BeginTvs {
            transaction,
            mutation,
            ..
        }
        | DatasetRequest::CompleteTvs {
            transaction,
            mutation,
            ..
        }
        | DatasetRequest::ReconcileTvs {
            transaction,
            mutation,
            ..
        } => {
            validate_transaction(transaction, limits)?;
            mutation.validate(limits)
        }
        DatasetRequest::StageTvs {
            transaction,
            operation,
            mutation,
            ..
        } => {
            validate_transaction(transaction, limits)?;
            validate_tvs_operation(operation, limits)?;
            mutation.validate(limits)
        }
        DatasetRequest::Write {
            records, mutation, ..
        } => {
            validate_records(records, limits)?;
            mutation.validate(limits)
        }
        DatasetRequest::Append {
            records, mutation, ..
        } => {
            validate_records(records, limits)?;
            mutation.validate(limits)
        }
        DatasetRequest::Truncate { mutation, .. } => mutation.validate(limits),
        DatasetRequest::RewriteRecord {
            key,
            record,
            mutation,
            ..
        } if key.len() > limits.max_record_bytes || record.len() > limits.max_record_bytes => {
            Err(HostProblem::ResourceExhausted)
        }
        DatasetRequest::RewriteRecord { mutation, .. } => mutation.validate(limits),
        DatasetRequest::DeleteRecord { key, mutation, .. } => {
            if key.len() > limits.max_record_bytes {
                Err(HostProblem::ResourceExhausted)
            } else {
                mutation.validate(limits)
            }
        }
        DatasetRequest::DefineAlternateIndex {
            key_offset,
            key_length,
            mutation,
            ..
        } => {
            if *key_length == 0
                || key_offset
                    .checked_add(*key_length)
                    .is_none_or(|end| end as usize > limits.max_record_bytes)
            {
                Err(HostProblem::Malformed)
            } else {
                mutation.validate(limits)
            }
        }
        DatasetRequest::BuildAlternateIndex { mutation, .. } => mutation.validate(limits),
        DatasetRequest::DefinePath { mutation, .. } => mutation.validate(limits),
        DatasetRequest::WriteRelative {
            record_number,
            record,
            mutation,
            ..
        } => {
            if *record_number == 0 {
                Err(HostProblem::Malformed)
            } else if record.len() > limits.max_record_bytes {
                Err(HostProblem::ResourceExhausted)
            } else {
                mutation.validate(limits)
            }
        }
        DatasetRequest::DeleteRelative {
            record_number,
            mutation,
            ..
        } => {
            if *record_number == 0 {
                Err(HostProblem::Malformed)
            } else {
                mutation.validate(limits)
            }
        }
        DatasetRequest::WriteRba { data, mutation, .. } => {
            if data.is_empty() {
                Err(HostProblem::Malformed)
            } else if data.len() > limits.max_record_bytes {
                Err(HostProblem::ResourceExhausted)
            } else {
                mutation.validate(limits)
            }
        }
        DatasetRequest::DefineGenerationGroup {
            limit, mutation, ..
        } => {
            if *limit == 0 || *limit as usize > limits.max_records {
                Err(HostProblem::ResourceExhausted)
            } else {
                mutation.validate(limits)
            }
        }
        DatasetRequest::CreateGeneration {
            attributes,
            records,
            mutation,
            ..
        } => {
            attributes.validate(limits)?;
            validate_records(records, limits)?;
            mutation.validate(limits)
        }
        DatasetRequest::ResolveGeneration { relative, .. } if *relative > 0 => {
            Err(HostProblem::Malformed)
        }
        DatasetRequest::Rename { mutation, .. } => mutation.validate(limits),
        DatasetRequest::Delete {
            current_date,
            mutation,
            ..
        } => {
            if current_date.is_some_and(|date| !valid_julian_date(date)) {
                Err(HostProblem::Malformed)
            } else {
                mutation.validate(limits)
            }
        }
        // Browse key and cursor bounds share one validator.
        DatasetRequest::StartBrowse { .. }
        | DatasetRequest::ResetBrowse { .. }
        | DatasetRequest::ReadNext { .. }
        | DatasetRequest::EndBrowse { .. } => browse::validate(request, limits),
        DatasetRequest::Close {
            cursor, control, ..
        } if cursor
            .as_ref()
            .is_some_and(|cursor| cursor.is_empty() || cursor.len() > limits.max_name_bytes)
            || (control.lock
                && (control.reel_or_unit.is_some() || control.no_rewind || control.removal))
            || (control.removal && control.reel_or_unit.is_none()) =>
        {
            Err(HostProblem::Malformed)
        }
        _ => Ok(()),
    }
}

fn validate_records(records: &[Vec<u8>], limits: HostLimits) -> Result<(), HostProblem> {
    if records.len() > limits.max_records
        || records
            .iter()
            .any(|record| record.len() > limits.max_record_bytes)
    {
        Err(HostProblem::ResourceExhausted)
    } else {
        Ok(())
    }
}

fn validate_spool_file(file: &str, limits: HostLimits) -> Result<(), HostProblem> {
    if file.is_empty()
        || file.len() > limits.max_name_bytes
        || file
            .chars()
            .any(|character| character.is_control() || !character.is_ascii())
    {
        Err(HostProblem::Malformed)
    } else {
        Ok(())
    }
}

fn validate_dataset_snapshot(
    snapshot: &DatasetSnapshot,
    limits: HostLimits,
) -> Result<(), HostProblem> {
    snapshot.definition.validate(
        limits,
        DatasetProviderCapabilities::all_contract_capabilities(),
    )?;
    validate_records(&snapshot.records, limits)?;
    if snapshot.relative_records.len() > limits.max_records
        || snapshot.members.len() > limits.max_records
        || snapshot.linear_data.len() > limits.max_state_bytes
    {
        return Err(HostProblem::ResourceExhausted);
    }
    let mut previous_rrn = 0u64;
    for relative in &snapshot.relative_records {
        if relative.record_number == 0 || relative.record_number <= previous_rrn {
            return Err(HostProblem::Malformed);
        }
        previous_rrn = relative.record_number;
        validate_records(std::slice::from_ref(&relative.record), limits)?;
    }
    let mut total = snapshot
        .records
        .len()
        .checked_add(snapshot.relative_records.len())
        .ok_or(HostProblem::ResourceExhausted)?;
    let mut total_bytes = snapshot
        .records
        .iter()
        .chain(
            snapshot
                .relative_records
                .iter()
                .map(|relative| &relative.record),
        )
        .try_fold(snapshot.linear_data.len(), |total, record| {
            total
                .checked_add(record.len())
                .ok_or(HostProblem::ResourceExhausted)
        })?;
    let mut previous_member = None;
    for member in &snapshot.members {
        if previous_member.is_some_and(|previous: &str| previous >= member.name.as_str()) {
            return Err(HostProblem::Malformed);
        }
        previous_member = Some(member.name.as_str());
        validate_records(&member.records, limits)?;
        if member.generations.len() > limits.max_records
            || member.alias_of.is_some()
                && (!member.records.is_empty() || !member.generations.is_empty())
        {
            return Err(HostProblem::Malformed);
        }
        total = total
            .checked_add(member.records.len())
            .ok_or(HostProblem::ResourceExhausted)?;
        total_bytes = member
            .records
            .iter()
            .try_fold(total_bytes, |total, record| {
                total
                    .checked_add(record.len())
                    .ok_or(HostProblem::ResourceExhausted)
            })?;
        let mut previous = 0u64;
        for generation in &member.generations {
            if generation.generation == 0 || generation.generation <= previous {
                return Err(HostProblem::Malformed);
            }
            previous = generation.generation;
            validate_records(&generation.records, limits)?;
            total = total
                .checked_add(generation.records.len())
                .ok_or(HostProblem::ResourceExhausted)?;
            total_bytes = generation
                .records
                .iter()
                .try_fold(total_bytes, |total, record| {
                    total
                        .checked_add(record.len())
                        .ok_or(HostProblem::ResourceExhausted)
                })?;
        }
    }
    let shape_valid = match snapshot.definition.attributes.organization {
        DatasetOrganization::Sequential
        | DatasetOrganization::KeySequenced
        | DatasetOrganization::EntrySequenced => {
            snapshot.relative_records.is_empty()
                && snapshot.members.is_empty()
                && snapshot.linear_data.is_empty()
        }
        DatasetOrganization::Relative | DatasetOrganization::VariableRelative => {
            snapshot.records.is_empty()
                && snapshot.members.is_empty()
                && snapshot.linear_data.is_empty()
        }
        DatasetOrganization::Partitioned => {
            snapshot.records.is_empty()
                && snapshot.relative_records.is_empty()
                && snapshot.linear_data.is_empty()
                && snapshot
                    .members
                    .iter()
                    .all(|member| member.alias_of.is_none() && member.generations.is_empty())
        }
        DatasetOrganization::PartitionedExtended => {
            snapshot.records.is_empty()
                && snapshot.relative_records.is_empty()
                && snapshot.linear_data.is_empty()
                && snapshot.members.iter().all(|member| {
                    member.records.is_empty()
                        && member.alias_of.as_ref().map_or(
                            !member.generations.is_empty(),
                            |target| {
                                target != &member.name
                                    && snapshot.members.iter().any(|candidate| {
                                        candidate.name == *target && candidate.alias_of.is_none()
                                    })
                            },
                        )
                })
        }
        DatasetOrganization::Linear => {
            snapshot.records.is_empty()
                && snapshot.relative_records.is_empty()
                && snapshot.members.is_empty()
        }
    };
    if !shape_valid {
        Err(HostProblem::Malformed)
    } else if total > limits.max_records || total_bytes > limits.max_state_bytes {
        Err(HostProblem::ResourceExhausted)
    } else {
        Ok(())
    }
}
fn validate_transaction(transaction: &str, limits: HostLimits) -> Result<(), HostProblem> {
    if transaction.is_empty()
        || transaction.len() > limits.max_name_bytes
        || transaction.chars().any(char::is_control)
    {
        Err(HostProblem::Malformed)
    } else {
        Ok(())
    }
}
fn valid_julian_date(date: u32) -> bool {
    let year = date / 1000;
    let day = date % 1000;
    let leap = year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
    (1900..=9999).contains(&year) && day != 0 && day <= if leap { 366 } else { 365 }
}
fn validate_tvs_operation(
    operation: &TvsRecordOperation,
    limits: HostLimits,
) -> Result<(), HostProblem> {
    match operation {
        TvsRecordOperation::Insert { record, .. } => {
            if record.is_empty() {
                Err(HostProblem::Malformed)
            } else if record.len() > limits.max_record_bytes {
                Err(HostProblem::ResourceExhausted)
            } else {
                Ok(())
            }
        }
        TvsRecordOperation::Rewrite { key, record, .. } => {
            if key.is_empty() || record.is_empty() {
                Err(HostProblem::Malformed)
            } else if key.len() > limits.max_record_bytes || record.len() > limits.max_record_bytes
            {
                Err(HostProblem::ResourceExhausted)
            } else {
                Ok(())
            }
        }
        TvsRecordOperation::Delete { key, .. } => {
            if key.is_empty() {
                Err(HostProblem::Malformed)
            } else if key.len() > limits.max_record_bytes {
                Err(HostProblem::ResourceExhausted)
            } else {
                Ok(())
            }
        }
    }
}
fn validate_fields(fields: &[TerminalField], limits: HostLimits) -> Result<(), HostProblem> {
    if fields.len() > limits.max_fields
        || fields.iter().any(|field| {
            field.name.is_empty()
                || field.name.len() > limits.max_name_bytes
                || field.value.len() > limits.max_record_bytes
                || field.value.len() > usize::from(field.length)
        })
    {
        Err(HostProblem::ResourceExhausted)
    } else {
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Typed rejection, failure or uncertainty at the host boundary.
/// `UnknownOutcome` requires reconciliation rather than assuming the effect did not occur.
pub enum HostProblem {
    /// The envelope or payload violates its structural contract.
    Malformed,
    /// No supported implementation is available for the request.
    Unsupported,
    /// A specific provider capability is unavailable, with explanatory detail.
    UnsupportedCapability {
        /// Bounded nonempty identity of the unsupported capability.
        capability: String,
        /// Nonempty explanatory text bounded by `max_state_bytes`.
        detail: String,
    },
    /// The requested resource was not found.
    NotFound,
    /// A named subsystem condition with primary and secondary response codes.
    Condition {
        /// Nonempty condition name bounded by `max_name_bytes`.
        name: String,
        /// Primary signed subsystem response code.
        response: i32,
        /// Secondary signed subsystem response code.
        response2: i32,
    },
    /// The invocation lacks the required authority.
    Unauthorized,
    /// Cancellation prevented the requested operation.
    Cancelled,
    /// The logical deadline was reached.
    TimedOut,
    /// A request, reply or provider resource exceeds an admitted bound.
    ResourceExhausted,
    /// The selected provider is unavailable or fails its contract.
    ProviderFailure,
    /// Host infrastructure failed while dispatching the effect.
    InfrastructureFailure,
    /// A replay-protected operation has no required idempotency identity.
    MissingIdempotency,
    /// Replay metadata disagrees with an existing identity or outer envelope.
    IdempotencyConflict,
    /// Whether the effect took place cannot be established.
    /// Reconcile durable state before deciding whether a retry is safe.
    UnknownOutcome,
}
impl fmt::Display for HostProblem {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "host service failed: {self:?}")
    }
}
impl std::error::Error for HostProblem {}

impl HostProblem {
    fn validate(&self, limits: HostLimits) -> Result<(), HostProblem> {
        match self {
            Self::UnsupportedCapability { capability, detail }
                if capability.is_empty()
                    || capability.len() > limits.max_name_bytes
                    || detail.is_empty()
                    || detail.len() > limits.max_state_bytes =>
            {
                Err(HostProblem::Malformed)
            }
            Self::Condition { name, .. }
                if name.is_empty() || name.len() > limits.max_name_bytes =>
            {
                Err(HostProblem::Malformed)
            }
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mutating_effect_requires_idempotency() {
        let invocation = InvocationLimits::default();
        let host = HostLimits::default();
        let request = EffectRequest {
            run_unit: RunUnitId::new("run-1", invocation).unwrap(),
            sequence: 1,
            deadline_tick: 1,
            idempotency_key: None,
            request: HostRequest::State(StateRequest::Delete {
                key: "x".into(),
                expected_version: None,
                mutation: Mutation {
                    sequence: 1,
                    idempotency_key: IdempotencyKey::new("idem-1", invocation).unwrap(),
                    transaction: None,
                },
            }),
        };
        assert_eq!(request.validate(host), Err(HostProblem::MissingIdempotency));
    }

    #[test]
    fn token_producing_read_requires_mutation_replay_identity() {
        let token_target = BoundedPayload::new(
            "mainframe-env.cics.argument@1",
            b"TOKEN-X".to_vec(),
            InvocationLimits::default(),
        )
        .unwrap();
        let token_read = HostRequest::Cics(CicsRequest {
            operation: CicsOperation::Read,
            arguments: BTreeMap::from([("TOKEN".into(), token_target)]),
            condition_policy: CicsConditionPolicy::Default,
            mutation: None,
        });
        assert!(token_read.is_mutating());
        assert_eq!(
            token_read.validate(HostLimits::default()),
            Err(HostProblem::MissingIdempotency)
        );
        let plain_read = HostRequest::Cics(CicsRequest {
            operation: CicsOperation::Read,
            arguments: BTreeMap::new(),
            condition_policy: CicsConditionPolicy::Default,
            mutation: None,
        });
        assert!(!plain_read.is_mutating());
    }

    #[test]
    fn records_are_bounded() {
        let invocation = InvocationLimits::default();
        let limits = HostLimits {
            max_record_bytes: 1,
            ..HostLimits::default()
        };
        let mutation = Mutation {
            sequence: 1,
            idempotency_key: IdempotencyKey::new("i", invocation).unwrap(),
            transaction: None,
        };
        let request = HostRequest::Dataset(DatasetRequest::Write {
            dataset: DatasetName::new("USER.DATA", 44).unwrap(),
            member: None,
            records: vec![vec![1, 2]],
            expected_version: None,
            mutation,
        });
        assert_eq!(
            request.validate(limits),
            Err(HostProblem::ResourceExhausted)
        );
    }

    #[test]
    fn close_control_rejects_conflicting_lock_and_reel_dispositions() {
        let dataset = DatasetName::new("USER.DATA", 44).unwrap();
        let valid = HostRequest::Dataset(DatasetRequest::Close {
            dataset: dataset.clone(),
            cursor: None,
            control: DatasetCloseControl {
                reel_or_unit: Some(DatasetReelUnit::Reel),
                no_rewind: true,
                ..DatasetCloseControl::default()
            },
        });
        assert_eq!(valid.validate(HostLimits::default()), Ok(()));

        let invalid = HostRequest::Dataset(DatasetRequest::Close {
            dataset,
            cursor: None,
            control: DatasetCloseControl {
                reel_or_unit: Some(DatasetReelUnit::Unit),
                lock: true,
                ..DatasetCloseControl::default()
            },
        });
        assert_eq!(
            invalid.validate(HostLimits::default()),
            Err(HostProblem::Malformed)
        );
    }

    #[test]
    fn all_cics_runtime_operation_names_are_unique() {
        let forms = [
            CicsOperation::Abend,
            CicsOperation::AddSubevent,
            CicsOperation::Address,
            CicsOperation::AddressSet,
            CicsOperation::Asktime,
            CicsOperation::BifDeedit,
            CicsOperation::BifDigest,
            CicsOperation::AsktimeEib,
            CicsOperation::Assign,
            CicsOperation::ChangeTask,
            CicsOperation::Post,
            CicsOperation::WriteOperator,
            CicsOperation::ExtractCertificate,
            CicsOperation::ExtractTcpip,
            CicsOperation::Deq,
            CicsOperation::Delete,
            CicsOperation::DefineInputEvent,
            CicsOperation::DefineCompositeEvent,
            CicsOperation::DocumentCreate,
            CicsOperation::DocumentDelete,
            CicsOperation::DocumentInsert,
            CicsOperation::DocumentRetrieve,
            CicsOperation::DocumentSet,
            CicsOperation::DeleteTransientData,
            CicsOperation::DeleteTemporaryStorage,
            CicsOperation::ReadTemporaryStorage,
            CicsOperation::WriteTemporaryStorage,
            CicsOperation::Enq,
            CicsOperation::EndBrowse,
            CicsOperation::FormatTime,
            CicsOperation::ConvertTime,
            CicsOperation::Freemain,
            CicsOperation::Freemain64,
            CicsOperation::Getmain,
            CicsOperation::Getmain64,
            CicsOperation::HandleAbend,
            CicsOperation::HandleAid,
            CicsOperation::HandleCondition,
            CicsOperation::IgnoreCondition,
            CicsOperation::Inquire,
            CicsOperation::InvokeApplication,
            CicsOperation::Load,
            CicsOperation::Release,
            CicsOperation::Link,
            CicsOperation::PopHandle,
            CicsOperation::PushHandle,
            CicsOperation::PurgeMessage,
            CicsOperation::Read,
            CicsOperation::ReadNext,
            CicsOperation::ReadPrev,
            CicsOperation::ResetBrowse,
            CicsOperation::ReadTransientData,
            CicsOperation::RemoveSubevent,
            CicsOperation::DeleteEvent,
            CicsOperation::CheckTimer,
            CicsOperation::DefineTimer,
            CicsOperation::DeleteTimer,
            CicsOperation::ForceTimer,
            CicsOperation::RetrieveReattachEvent,
            CicsOperation::RetrieveSubevent,
            CicsOperation::TestEvent,
            CicsOperation::SignalEvent,
            CicsOperation::DefineCounter,
            CicsOperation::DefineDCounter,
            CicsOperation::DeleteCounter,
            CicsOperation::DeleteDCounter,
            CicsOperation::GetCounter,
            CicsOperation::GetDCounter,
            CicsOperation::QueryCounter,
            CicsOperation::QueryDCounter,
            CicsOperation::RewindCounter,
            CicsOperation::RewindDCounter,
            CicsOperation::UpdateCounter,
            CicsOperation::UpdateDCounter,
            CicsOperation::ReceiveMap,
            CicsOperation::ReceivePartn,
            CicsOperation::Retrieve,
            CicsOperation::Return,
            CicsOperation::Rewrite,
            CicsOperation::SendText,
            CicsOperation::SendMap,
            CicsOperation::SendControl,
            CicsOperation::SendPage,
            CicsOperation::SendPartnset,
            CicsOperation::SetAssociationUserCorrData,
            CicsOperation::SetFileStatus,
            CicsOperation::SpoolClose,
            CicsOperation::SpoolOpenInput,
            CicsOperation::SpoolOpenOutput,
            CicsOperation::SpoolRead,
            CicsOperation::SpoolWrite,
            CicsOperation::Start,
            CicsOperation::StartBrowse,
            CicsOperation::StartAttach,
            CicsOperation::Suspend,
            CicsOperation::WaitEvent,
            CicsOperation::WaitExternal,
            CicsOperation::WaitCics,
            CicsOperation::Syncpoint,
            CicsOperation::InvokeService,
            CicsOperation::SoapFaultAdd,
            CicsOperation::SoapFaultCreate,
            CicsOperation::SoapFaultDelete,
            CicsOperation::WsaContextBuild,
            CicsOperation::WsaContextDelete,
            CicsOperation::WsaContextGet,
            CicsOperation::WsaEprCreate,
            CicsOperation::TransformDataToJson,
            CicsOperation::TransformDataToXml,
            CicsOperation::TransformJsonToData,
            CicsOperation::TransformXmlToData,
            CicsOperation::WebParseUrl,
            CicsOperation::WebOpen,
            CicsOperation::WebClose,
            CicsOperation::WebExtract,
            CicsOperation::ExtractWeb,
            CicsOperation::WebRead,
            CicsOperation::WebStartBrowse,
            CicsOperation::WebReadNext,
            CicsOperation::WebEndBrowse,
            CicsOperation::WebWrite,
            CicsOperation::WebSend,
            CicsOperation::WebRetrieve,
            CicsOperation::WebReceive,
            CicsOperation::WebConverse,
            CicsOperation::Unlock,
            CicsOperation::Write,
            CicsOperation::WriteTransientData,
            CicsOperation::Xctl,
        ];
        assert_eq!(forms.len(), 129);
        let names = forms
            .iter()
            .map(|operation| operation.runtime_name())
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(names.len(), forms.len());
    }
}
