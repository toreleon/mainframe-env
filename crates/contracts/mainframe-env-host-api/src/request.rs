use crate::dataset::{
    CatalogKind, CatalogListEntry, CatalogResolution, DatasetDefinition, DatasetDescription,
    DatasetDiagnostic, DatasetLifecycleState, DatasetLockMode, DatasetLockReceipt,
    DatasetLockTarget, DatasetProviderCapabilities, DatasetSnapshot, TvsRecordOperation,
    TvsUnitOfWorkReceipt,
};
use crate::{
    ClassName, DatasetName, JobName, MemberName, MethodName, ProgramName, ResourceName,
    RuntimeServiceName, SessionId,
};
use mainframe_env_execution_api::{
    BoundedPayload, CapabilityId, IdempotencyKey, InvocationLimits, PrincipalId, RunUnitId,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HostLimits {
    pub max_name_bytes: usize,
    pub max_record_bytes: usize,
    pub max_records: usize,
    pub max_fields: usize,
    pub max_audit_fields: usize,
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
pub enum DatasetOrganization {
    Sequential,
    Partitioned,
    PartitionedExtended,
    KeySequenced,
    EntrySequenced,
    Relative,
    VariableRelative,
    Linear,
}
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RecordFormat {
    Fixed,
    FixedBlocked,
    FixedBlockedStandard,
    Variable,
    VariableBlocked,
    VariableSpanned,
    VariableBlockedSpanned,
    Undefined,
    Line,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KeyRelation {
    Equal,
    Greater,
    GreaterOrEqual,
    Less,
    LessOrEqual,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum DatasetReadLockMode {
    #[default]
    Default,
    Lock,
    KeptLock,
    NoLock,
    IgnoreLock,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DatasetReadControl {
    pub lock: DatasetReadLockMode,
    pub wait: Option<bool>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DatasetReelUnit {
    Reel,
    Unit,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DatasetCloseControl {
    pub reel_or_unit: Option<DatasetReelUnit>,
    pub no_rewind: bool,
    pub removal: bool,
    pub lock: bool,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AccessIntent {
    Read,
    Execute,
    Update,
    Control,
    Alter,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SecretRef(String);

impl SecretRef {
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
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DatasetAttributes {
    pub organization: DatasetOrganization,
    pub record_format: RecordFormat,
    pub logical_record_length: u32,
    pub key_offset: Option<u32>,
    pub key_length: Option<u32>,
    pub ccsid: Option<u16>,
}

impl DatasetAttributes {
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
pub struct Mutation {
    pub sequence: u64,
    pub idempotency_key: IdempotencyKey,
    pub transaction: Option<String>,
}

impl Mutation {
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DatasetRequest {
    Capabilities,
    List {
        pattern: String,
        start: Option<DatasetName>,
        max_items: u32,
    },
    Attributes {
        dataset: DatasetName,
    },
    Describe {
        dataset: DatasetName,
    },
    Diagnose {
        dataset: DatasetName,
    },
    ResolveCatalog {
        name: DatasetName,
    },
    ListCatalog {
        pattern: String,
        start: Option<DatasetName>,
        max_items: u32,
    },
    ListVolumes {
        start: Option<String>,
        max_items: u32,
    },
    ListLocks {
        dataset: DatasetName,
        now_tick: u64,
        max_items: u32,
    },
    TvsStatus {
        transaction: String,
        owner: PrincipalId,
    },
    ListMembers {
        dataset: DatasetName,
        start: Option<MemberName>,
        max_items: u32,
    },
    ReadMemberGeneration {
        dataset: DatasetName,
        member: MemberName,
        relative: i32,
        max_records: u32,
    },
    Read {
        dataset: DatasetName,
        member: Option<MemberName>,
        key: Option<Vec<u8>>,
        max_records: u32,
        control: DatasetReadControl,
    },
    ReadGeneric {
        dataset: DatasetName,
        key_prefix: Vec<u8>,
        max_records: u32,
    },
    ReadConcatenation {
        datasets: Vec<DatasetName>,
        member: Option<MemberName>,
        max_records: u32,
    },
    ReadRelative {
        dataset: DatasetName,
        record_number: u64,
    },
    ReadRba {
        dataset: DatasetName,
        rba: u64,
        max_bytes: u32,
    },
    ReadSequential {
        dataset: DatasetName,
        member: Option<MemberName>,
        start: Option<u64>,
        reverse: bool,
        max_records: u32,
    },
    Snapshot {
        dataset: DatasetName,
        max_records: u32,
        max_members: u32,
    },
    Create {
        dataset: DatasetName,
        attributes: DatasetAttributes,
        mutation: Mutation,
    },
    Define {
        dataset: DatasetName,
        definition: Box<DatasetDefinition>,
        mutation: Mutation,
    },
    Alter {
        dataset: DatasetName,
        definition: Box<DatasetDefinition>,
        expected_version: Option<u64>,
        mutation: Mutation,
    },
    SetLifecycle {
        dataset: DatasetName,
        state: DatasetLifecycleState,
        expected_version: Option<u64>,
        mutation: Mutation,
    },
    RecordBackup {
        dataset: DatasetName,
        expected_version: Option<u64>,
        mutation: Mutation,
    },
    Restore {
        dataset: DatasetName,
        snapshot: Box<DatasetSnapshot>,
        expected_version: Option<u64>,
        mutation: Mutation,
    },
    DefineCatalog {
        catalog: DatasetName,
        kind: CatalogKind,
        mutation: Mutation,
    },
    SetCatalogConnection {
        catalog: DatasetName,
        connected: bool,
        expected_version: Option<u64>,
        mutation: Mutation,
    },
    DefineAlias {
        alias: DatasetName,
        target: DatasetName,
        mutation: Mutation,
    },
    DefineMemberAlias {
        dataset: DatasetName,
        alias: MemberName,
        target: MemberName,
        expected_version: Option<u64>,
        mutation: Mutation,
    },
    WriteMemberGeneration {
        dataset: DatasetName,
        member: MemberName,
        records: Vec<Vec<u8>>,
        program_object: bool,
        expected_version: Option<u64>,
        mutation: Mutation,
    },
    DeleteMemberGeneration {
        dataset: DatasetName,
        member: MemberName,
        generation: u64,
        expected_version: Option<u64>,
        mutation: Mutation,
    },
    AcquireLock {
        dataset: DatasetName,
        target: DatasetLockTarget,
        owner: PrincipalId,
        mode: DatasetLockMode,
        now_tick: u64,
        lease_ticks: u64,
        transaction: Option<String>,
        mutation: Mutation,
    },
    ReleaseLock {
        dataset: DatasetName,
        lock_id: String,
        owner: PrincipalId,
        mutation: Mutation,
    },
    BeginTvs {
        transaction: String,
        owner: PrincipalId,
        mutation: Mutation,
    },
    StageTvs {
        transaction: String,
        owner: PrincipalId,
        operation: TvsRecordOperation,
        mutation: Mutation,
    },
    CompleteTvs {
        transaction: String,
        owner: PrincipalId,
        commit: bool,
        mutation: Mutation,
    },
    ReconcileTvs {
        transaction: String,
        owner: PrincipalId,
        committed: bool,
        mutation: Mutation,
    },
    Write {
        dataset: DatasetName,
        member: Option<MemberName>,
        records: Vec<Vec<u8>>,
        expected_version: Option<u64>,
        mutation: Mutation,
    },
    Append {
        dataset: DatasetName,
        member: Option<MemberName>,
        records: Vec<Vec<u8>>,
        expected_version: Option<u64>,
        mutation: Mutation,
    },
    Truncate {
        dataset: DatasetName,
        expected_version: Option<u64>,
        mutation: Mutation,
    },
    RewriteRecord {
        dataset: DatasetName,
        key: Vec<u8>,
        record: Vec<u8>,
        expected_version: Option<u64>,
        mutation: Mutation,
    },
    DeleteRecord {
        dataset: DatasetName,
        key: Vec<u8>,
        expected_version: Option<u64>,
        mutation: Mutation,
    },
    WriteRelative {
        dataset: DatasetName,
        record_number: u64,
        record: Vec<u8>,
        expected_version: Option<u64>,
        mutation: Mutation,
    },
    DeleteRelative {
        dataset: DatasetName,
        record_number: u64,
        expected_version: Option<u64>,
        mutation: Mutation,
    },
    WriteRba {
        dataset: DatasetName,
        rba: u64,
        data: Vec<u8>,
        expected_version: Option<u64>,
        mutation: Mutation,
    },
    DefineAlternateIndex {
        base: DatasetName,
        index: DatasetName,
        key_offset: u32,
        key_length: u32,
        allow_duplicates: bool,
        upgrade: bool,
        mutation: Mutation,
    },
    BuildAlternateIndex {
        base: DatasetName,
        index: DatasetName,
        mutation: Mutation,
    },
    DefinePath {
        path: DatasetName,
        index: DatasetName,
        mutation: Mutation,
    },
    DefineGenerationGroup {
        base: DatasetName,
        limit: u32,
        scratch: bool,
        empty: bool,
        mutation: Mutation,
    },
    CreateGeneration {
        base: DatasetName,
        attributes: DatasetAttributes,
        records: Vec<Vec<u8>>,
        mutation: Mutation,
    },
    ResolveGeneration {
        base: DatasetName,
        relative: i32,
    },
    Rename {
        from: DatasetName,
        to: DatasetName,
        mutation: Mutation,
    },
    Delete {
        dataset: DatasetName,
        member: Option<MemberName>,
        expected_version: Option<u64>,
        purge: bool,
        current_date: Option<u32>,
        mutation: Mutation,
    },
    StartBrowse {
        dataset: DatasetName,
        key: Vec<u8>,
        relation: KeyRelation,
    },
    ReadNext {
        dataset: DatasetName,
        cursor: String,
        reverse: bool,
        control: DatasetReadControl,
    },
    EndBrowse {
        dataset: DatasetName,
        cursor: String,
    },
    Close {
        dataset: DatasetName,
        cursor: Option<String>,
        control: DatasetCloseControl,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DatasetResult {
    Capabilities {
        capabilities: DatasetProviderCapabilities,
    },
    Listed {
        names: Vec<DatasetName>,
        more: bool,
    },
    Members {
        names: Vec<MemberName>,
        more: bool,
    },
    Attributes {
        attributes: DatasetAttributes,
        version: u64,
    },
    Description(Box<DatasetDescription>),
    Diagnostics {
        diagnostics: Vec<DatasetDiagnostic>,
    },
    Catalog(CatalogResolution),
    CatalogEntries {
        entries: Vec<CatalogListEntry>,
        more: bool,
    },
    Volumes {
        volumes: Vec<crate::DatasetVolumeDescription>,
        more: bool,
    },
    Locks {
        locks: Vec<DatasetLockReceipt>,
    },
    Tvs(TvsUnitOfWorkReceipt),
    Snapshot {
        snapshot: Box<DatasetSnapshot>,
        version: u64,
    },
    Records {
        records: Vec<Vec<u8>>,
        identities: Vec<Vec<u8>>,
        version: u64,
    },
    MemberGeneration {
        records: Vec<Vec<u8>>,
        identities: Vec<Vec<u8>>,
        generation: u64,
        program_object: bool,
        version: u64,
    },
    Rba {
        data: Vec<u8>,
        record: bool,
        rba: u64,
        next_rba: u64,
        version: u64,
    },
    Created {
        version: u64,
    },
    Mutated {
        version: u64,
    },
    Browse {
        cursor: String,
        record: Option<Vec<u8>>,
        identity: Option<Vec<u8>>,
        key: Option<Vec<u8>>,
    },
    Generation {
        dataset: DatasetName,
        absolute_generation: u32,
        version: u64,
    },
    Condition {
        name: String,
        status: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProgramRequest {
    Inquire {
        program: ProgramName,
    },
    Call {
        program: ProgramName,
        payload: BoundedPayload,
        service: Option<RuntimeServiceSelector>,
    },
    Invoke {
        class: ClassName,
        method: MethodName,
        receiver: BoundedPayload,
        payload: BoundedPayload,
    },
    Link {
        program: ProgramName,
        payload: BoundedPayload,
    },
    Xctl {
        program: ProgramName,
        payload: BoundedPayload,
    },
    Return {
        next_transaction: Option<String>,
        payload: BoundedPayload,
    },
    Cancel {
        programs: Vec<ProgramName>,
    },
    Abend {
        code: String,
    },
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum RuntimeServiceKind {
    LanguageEnvironment,
    HostExtension,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeServiceSelector {
    pub kind: RuntimeServiceKind,
    pub name: RuntimeServiceName,
    pub abi_version: u16,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SpoolRequest {
    Append {
        job: JobName,
        file: String,
        records: Vec<Vec<u8>>,
        mutation: Mutation,
    },
    List {
        job: JobName,
    },
    Read {
        job: JobName,
        file: String,
        start: u64,
        max_records: u32,
    },
    Seal {
        job: JobName,
        file: String,
        mutation: Mutation,
    },
    Purge {
        job: JobName,
        mutation: Mutation,
    },
}

pub const SPOOL_REQUEST_CONTRACT: &str = "mainframe-env.spool-request@2";
pub const SPOOL_RESULT_CONTRACT: &str = "mainframe-env.spool-result@2";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SpoolFileSummary {
    pub file: String,
    pub record_count: u64,
    pub byte_count: u64,
    pub sealed: bool,
    pub version: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum SpoolResult {
    Mutated {
        version: u64,
        replayed: bool,
    },
    Files {
        files: Vec<SpoolFileSummary>,
    },
    Records {
        records: Vec<Vec<u8>>,
        more: bool,
        version: u64,
    },
    PurgePending {
        remaining_artifacts: u64,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TerminalField {
    pub name: String,
    pub row: u16,
    pub column: u16,
    pub length: u16,
    pub modified: bool,
    pub secret: bool,
    pub value: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TerminalRequest {
    Open {
        session: SessionId,
        rows: u16,
        columns: u16,
    },
    Write {
        session: SessionId,
        erase: bool,
        cursor: Option<(u16, u16)>,
        fields: Vec<TerminalField>,
    },
    Read {
        session: SessionId,
    },
    Input {
        session: SessionId,
        aid: u8,
        fields: Vec<TerminalField>,
    },
    Release {
        session: SessionId,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SecurityRequest {
    Authenticate {
        user: PrincipalId,
        credential_reference: SecretRef,
    },
    Authorize {
        principal: PrincipalId,
        class: String,
        resource: ResourceName,
        intent: AccessIntent,
    },
    Audit(AuditEvent),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SecurityDecision {
    Allow,
    Deny,
    NotFound,
    InvalidCredentials,
    Expired,
    Revoked,
    Locked,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuditEvent {
    pub action: String,
    pub resource_hash: String,
    pub decision: String,
    pub fields: BTreeMap<String, String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClockRequest {
    UtcTimestamp,
    Date,
    Time,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StateRequest {
    Get {
        key: String,
    },
    Put {
        key: String,
        value: Vec<u8>,
        expected_version: Option<u64>,
        mutation: Mutation,
    },
    Delete {
        key: String,
        expected_version: Option<u64>,
        mutation: Mutation,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Db2Operation {
    ExecuteScript,
    FreePlans,
    Select,
    Insert,
    Update,
    Delete,
    Count,
    DeclareCursor,
    OpenCursor,
    FetchCursor,
    CloseCursor,
    Commit,
    Rollback,
    Extract,
}

impl Db2Operation {
    #[must_use]
    pub const fn is_mutating(self) -> bool {
        !matches!(self, Self::Select | Self::Count | Self::Extract)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Db2HostVariable {
    pub value: Vec<u8>,
    pub indicator: Option<i16>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Db2Request {
    pub operation: Db2Operation,
    pub statement: String,
    pub cursor: Option<String>,
    pub inputs: BTreeMap<String, Db2HostVariable>,
    pub outputs: Vec<String>,
    pub max_rows: u32,
    pub mutation: Option<Mutation>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Db2Row {
    pub columns: Vec<Vec<u8>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Db2Result {
    pub sqlcode: i32,
    pub sqlstate: String,
    pub message: String,
    pub rows: Vec<Db2Row>,
    pub affected_rows: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImsOperation {
    Schedule,
    Terminate,
    GetUnique,
    GetNext,
    GetNextParent,
    Insert,
    Replace,
    Delete,
    Checkpoint,
    Load,
    Unload,
    Commit,
    Rollback,
}

impl ImsOperation {
    #[must_use]
    pub const fn is_mutating(self) -> bool {
        !matches!(self, Self::Unload)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImsQualifier {
    pub segment: String,
    pub field: String,
    pub value: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImsRequest {
    pub operation: ImsOperation,
    pub psb: Option<String>,
    pub pcb: u16,
    pub segments: Vec<String>,
    pub data: Vec<u8>,
    pub qualifiers: Vec<ImsQualifier>,
    pub checkpoint_id: Option<String>,
    pub max_segments: u32,
    pub mutation: Option<Mutation>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImsSegment {
    pub name: String,
    pub parent_key: Option<Vec<u8>>,
    pub data: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImsResult {
    pub status: String,
    pub segments: Vec<ImsSegment>,
    pub checkpoint_id: Option<String>,
    pub affected_segments: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MqOperation {
    Open,
    Get,
    Put,
    PutOne,
    Close,
    Commit,
    Rollback,
}

impl MqOperation {
    #[must_use]
    pub const fn is_mutating(self) -> bool {
        true
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MqRequest {
    pub operation: MqOperation,
    pub queue: Option<String>,
    pub handle: Option<u32>,
    pub options: i32,
    pub message: Vec<u8>,
    pub message_id: Option<Vec<u8>>,
    pub correlation_id: Option<Vec<u8>>,
    pub wait_ticks: u64,
    pub max_message_bytes: u32,
    pub mutation: Option<Mutation>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MqResult {
    pub completion_code: i32,
    pub reason_code: i32,
    pub handle: Option<u32>,
    pub message: Vec<u8>,
    pub message_id: Option<Vec<u8>>,
    pub correlation_id: Option<Vec<u8>>,
    pub trigger_program: Option<String>,
}

mod cics;
pub use cics::*;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HostRequest {
    Dataset(DatasetRequest),
    Program(ProgramRequest),
    Spool(SpoolRequest),
    Terminal(TerminalRequest),
    Security(SecurityRequest),
    Clock(ClockRequest),
    State(StateRequest),
    Cics(CicsRequest),
    Db2(Db2Request),
    Ims(ImsRequest),
    Mq(MqRequest),
}

impl HostRequest {
    #[must_use]
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
        ) || matches!(self, Self::Cics(CicsRequest { operation, .. }) if operation.is_mutating())
            || matches!(self, Self::Db2(request) if request.operation.is_mutating())
            || matches!(self, Self::Ims(request) if request.operation.is_mutating())
            || matches!(self, Self::Mq(request) if request.operation.is_mutating())
    }

    #[must_use]
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

    pub fn validate(&self, limits: HostLimits) -> Result<(), HostProblem> {
        match self {
            Self::Dataset(request) => validate_dataset(request, limits),
            Self::Program(ProgramRequest::Call {
                service: Some(service),
                ..
            }) if service.abi_version == 0 => Err(HostProblem::Malformed),
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
            Self::Cics(request) => {
                if request.arguments.len() > limits.max_fields {
                    return Err(HostProblem::ResourceExhausted);
                }
                if self.is_mutating() {
                    request
                        .mutation
                        .as_ref()
                        .ok_or(HostProblem::MissingIdempotency)?
                        .validate(limits)?;
                }
                Ok(())
            }
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
                if request.pcb == 0
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
pub enum HostResult {
    Dataset(DatasetResult),
    Program(BoundedPayload),
    Spool(SpoolResult),
    Terminal(BoundedPayload),
    Security(SecurityDecision),
    Clock(String),
    State {
        value: Option<Vec<u8>>,
        version: u64,
    },
    Cics(CicsResponse),
    Db2(Db2Result),
    Ims(ImsResult),
    Mq(MqResult),
}

impl HostResult {
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
pub struct EffectRequest {
    pub run_unit: RunUnitId,
    pub sequence: u64,
    pub deadline_tick: u64,
    pub idempotency_key: Option<IdempotencyKey>,
    pub request: HostRequest,
}

impl EffectRequest {
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
pub struct EffectResult {
    pub sequence: u64,
    pub outcome: Result<HostResult, HostProblem>,
}

impl EffectResult {
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
        DatasetRequest::StartBrowse { key, .. } if key.len() > limits.max_record_bytes => {
            Err(HostProblem::ResourceExhausted)
        }
        DatasetRequest::ReadNext { cursor, .. } | DatasetRequest::EndBrowse { cursor, .. }
            if cursor.is_empty() || cursor.len() > limits.max_name_bytes =>
        {
            Err(HostProblem::Malformed)
        }
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
pub enum HostProblem {
    Malformed,
    Unsupported,
    UnsupportedCapability {
        capability: String,
        detail: String,
    },
    NotFound,
    Condition {
        name: String,
        response: i32,
        response2: i32,
    },
    Unauthorized,
    Cancelled,
    TimedOut,
    ResourceExhausted,
    ProviderFailure,
    InfrastructureFailure,
    MissingIdempotency,
    IdempotencyConflict,
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
            CicsOperation::Asktime,
            CicsOperation::Assign,
            CicsOperation::ChangeTask,
            CicsOperation::Deq,
            CicsOperation::Delete,
            CicsOperation::Enq,
            CicsOperation::EndBrowse,
            CicsOperation::FormatTime,
            CicsOperation::HandleAbend,
            CicsOperation::HandleCondition,
            CicsOperation::Inquire,
            CicsOperation::Link,
            CicsOperation::Read,
            CicsOperation::ReadNext,
            CicsOperation::ReadPrev,
            CicsOperation::ReceiveMap,
            CicsOperation::Retrieve,
            CicsOperation::Return,
            CicsOperation::Rewrite,
            CicsOperation::SendText,
            CicsOperation::SendMap,
            CicsOperation::SetFileStatus,
            CicsOperation::StartBrowse,
            CicsOperation::Suspend,
            CicsOperation::Syncpoint,
            CicsOperation::Write,
            CicsOperation::WriteTransientData,
            CicsOperation::Xctl,
        ];
        assert_eq!(forms.len(), 29);
        let names = forms
            .iter()
            .map(|operation| operation.runtime_name())
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(names.len(), forms.len());
    }
}
