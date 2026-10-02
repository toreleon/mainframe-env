//! Dataset request and result records behind the stable host request exports.
use super::*;

#[derive(Clone, Debug, Eq, PartialEq)]
/// Owned dataset operations with explicit read bounds and mutation identities.
/// Validate through `HostRequest`; support and access checks remain provider responsibilities.
pub enum DatasetRequest {
    /// Query the installed provider's dataset capability set.
    Capabilities,
    /// List dataset names matching a bounded pattern.
    List {
        /// Owned name-selection pattern, interpreted by the dataset provider.
        pattern: String,
        /// Optional continuation position supplied to the provider.
        start: Option<DatasetName>,
        /// Positive page-entry ceiling, at most `max_records`.
        max_items: u32,
    },
    /// Read logical layout attributes and their version.
    Attributes {
        /// Validated name of the dataset addressed by this operation.
        dataset: DatasetName,
    },
    /// Read a full dataset description, including allocation metadata.
    Describe {
        /// Validated name of the dataset addressed by this operation.
        dataset: DatasetName,
    },
    /// Request structured diagnostics for a dataset.
    Diagnose {
        /// Validated name of the dataset addressed by this operation.
        dataset: DatasetName,
    },
    /// Resolve a dataset or alias through the catalog.
    ResolveCatalog {
        /// Validated name to resolve through the catalog.
        name: DatasetName,
    },
    /// List matching catalog entries with a bounded page.
    ListCatalog {
        /// Owned name-selection pattern, interpreted by the dataset provider.
        pattern: String,
        /// Optional continuation position supplied to the provider.
        start: Option<DatasetName>,
        /// Positive page-entry ceiling, at most `max_records`.
        max_items: u32,
    },
    /// List volume descriptions with a bounded page.
    ListVolumes {
        /// Optional continuation position supplied to the provider.
        start: Option<String>,
        /// Positive page-entry ceiling, at most `max_records`.
        max_items: u32,
    },
    /// List dataset lock receipts at a supplied logical tick.
    ListLocks {
        /// Validated name of the dataset addressed by this operation.
        dataset: DatasetName,
        /// Nonzero caller-supplied logical tick used for lease or expiry evaluation.
        now_tick: u64,
        /// Positive page-entry ceiling, at most `max_records`.
        max_items: u32,
    },
    /// Read the unit-of-work receipt for a transaction and owner.
    TvsStatus {
        /// Nonempty transaction identity bounded by `max_name_bytes` and free of control
        /// characters.
        transaction: String,
        /// Principal identity owning the transaction or lock.
        owner: PrincipalId,
    },
    /// List named members in a dataset.
    ListMembers {
        /// Validated name of the dataset addressed by this operation.
        dataset: DatasetName,
        /// Optional continuation position supplied to the provider.
        start: Option<MemberName>,
        /// Positive page-entry ceiling, at most `max_records`.
        max_items: u32,
    },
    /// Read a member generation selected relative to the current generation.
    ReadMemberGeneration {
        /// Validated name of the dataset addressed by this operation.
        dataset: DatasetName,
        /// Member selector within the dataset.
        member: MemberName,
        /// Zero selects the current generation; negative values select older generations.
        relative: i32,
        /// Positive requested record ceiling, at most `max_records`.
        max_records: u32,
    },
    /// Read bounded records, optionally by member or exact key.
    Read {
        /// Validated name of the dataset addressed by this operation.
        dataset: DatasetName,
        /// Member selector within the dataset.
        member: Option<MemberName>,
        /// Owned record key bytes, bounded by `max_record_bytes`.
        key: Option<Vec<u8>>,
        /// Positive requested record ceiling, at most `max_records`.
        max_records: u32,
        /// Typed access controls passed to the provider.
        control: DatasetReadControl,
    },
    /// Read records matching a key prefix.
    ReadGeneric {
        /// Validated name of the dataset addressed by this operation.
        dataset: DatasetName,
        /// Nonempty initial key bytes bounded by `max_record_bytes`, used for generic selection.
        key_prefix: Vec<u8>,
        /// Positive requested record ceiling, at most `max_records`.
        max_records: u32,
    },
    /// Read records from an ordered dataset concatenation.
    ReadConcatenation {
        /// Nonempty ordered concatenation, containing at most `max_records` dataset names.
        datasets: Vec<DatasetName>,
        /// Member selector within the dataset.
        member: Option<MemberName>,
        /// Positive requested record ceiling, at most `max_records`.
        max_records: u32,
    },
    /// Read one relative record number.
    ReadRelative {
        /// Validated name of the dataset addressed by this operation.
        dataset: DatasetName,
        /// Nonzero relative record number supplied to the provider.
        record_number: u64,
    },
    /// Read bounded bytes from a relative byte address.
    ReadRba {
        /// Validated name of the dataset addressed by this operation.
        dataset: DatasetName,
        /// Starting relative byte address of the requested or returned range.
        rba: u64,
        /// Positive requested byte ceiling, at most `max_record_bytes`.
        max_bytes: u32,
    },
    /// Read a bounded sequential range, optionally in reverse.
    ReadSequential {
        /// Validated name of the dataset addressed by this operation.
        dataset: DatasetName,
        /// Member selector within the dataset.
        member: Option<MemberName>,
        /// Optional continuation position supplied to the provider.
        start: Option<u64>,
        /// Select reverse rather than forward traversal.
        reverse: bool,
        /// Positive requested record ceiling, at most `max_records`.
        max_records: u32,
    },
    /// Capture a bounded dataset snapshot for restoration.
    Snapshot {
        /// Validated name of the dataset addressed by this operation.
        dataset: DatasetName,
        /// Positive requested record ceiling, at most `max_records`.
        max_records: u32,
        /// Positive member-count ceiling, at most `max_records`.
        max_members: u32,
    },
    /// Create a dataset from logical layout attributes.
    Create {
        /// Validated name of the dataset addressed by this operation.
        dataset: DatasetName,
        /// Logical layout attributes for the dataset.
        attributes: DatasetAttributes,
        /// Replay sequence/key and optional transaction for this state change.
        mutation: Mutation,
    },
    /// Create a dataset from a complete typed definition.
    Define {
        /// Validated name of the dataset addressed by this operation.
        dataset: DatasetName,
        /// Owned complete dataset definition for provider validation.
        definition: Box<DatasetDefinition>,
        /// Replay sequence/key and optional transaction for this state change.
        mutation: Mutation,
    },
    /// Change a dataset definition with an optional version precondition.
    Alter {
        /// Validated name of the dataset addressed by this operation.
        dataset: DatasetName,
        /// Owned complete dataset definition for provider validation.
        definition: Box<DatasetDefinition>,
        /// Optional compare-and-update precondition on the current version.
        expected_version: Option<u64>,
        /// Replay sequence/key and optional transaction for this state change.
        mutation: Mutation,
    },
    /// Change the retained dataset lifecycle state.
    SetLifecycle {
        /// Validated name of the dataset addressed by this operation.
        dataset: DatasetName,
        /// Requested retained dataset lifecycle state.
        state: DatasetLifecycleState,
        /// Optional compare-and-update precondition on the current version.
        expected_version: Option<u64>,
        /// Replay sequence/key and optional transaction for this state change.
        mutation: Mutation,
    },
    /// Record a backup marker for a dataset version.
    RecordBackup {
        /// Validated name of the dataset addressed by this operation.
        dataset: DatasetName,
        /// Optional compare-and-update precondition on the current version.
        expected_version: Option<u64>,
        /// Replay sequence/key and optional transaction for this state change.
        mutation: Mutation,
    },
    /// Restore a dataset from an owned snapshot.
    Restore {
        /// Validated name of the dataset addressed by this operation.
        dataset: DatasetName,
        /// Owned dataset snapshot with layout and record contents.
        snapshot: Box<DatasetSnapshot>,
        /// Optional compare-and-update precondition on the current version.
        expected_version: Option<u64>,
        /// Replay sequence/key and optional transaction for this state change.
        mutation: Mutation,
    },
    /// Create a catalog with an explicit catalog kind.
    DefineCatalog {
        /// Validated name of the catalog being changed.
        catalog: DatasetName,
        /// Requested catalog kind.
        kind: CatalogKind,
        /// Replay sequence/key and optional transaction for this state change.
        mutation: Mutation,
    },
    /// Connect or disconnect a catalog.
    SetCatalogConnection {
        /// Validated name of the catalog being changed.
        catalog: DatasetName,
        /// Whether this catalog should be connected for resolution.
        connected: bool,
        /// Optional compare-and-update precondition on the current version.
        expected_version: Option<u64>,
        /// Replay sequence/key and optional transaction for this state change.
        mutation: Mutation,
    },
    /// Create a catalog alias for a named target.
    DefineAlias {
        /// Validated alias name to create.
        alias: DatasetName,
        /// Validated target name referenced by the alias.
        target: DatasetName,
        /// Replay sequence/key and optional transaction for this state change.
        mutation: Mutation,
    },
    /// Create an alias for a member in the same dataset.
    DefineMemberAlias {
        /// Validated name of the dataset addressed by this operation.
        dataset: DatasetName,
        /// Validated alias name to create.
        alias: MemberName,
        /// Validated target name referenced by the alias.
        target: MemberName,
        /// Optional compare-and-update precondition on the current version.
        expected_version: Option<u64>,
        /// Replay sequence/key and optional transaction for this state change.
        mutation: Mutation,
    },
    /// Write a new member generation with bounded record bytes.
    WriteMemberGeneration {
        /// Validated name of the dataset addressed by this operation.
        dataset: DatasetName,
        /// Member selector within the dataset.
        member: MemberName,
        /// Owned logical record bytes; count and individual lengths are host-bounded.
        records: Vec<Vec<u8>>,
        /// Whether this member generation is marked as a program object.
        program_object: bool,
        /// Optional compare-and-update precondition on the current version.
        expected_version: Option<u64>,
        /// Replay sequence/key and optional transaction for this state change.
        mutation: Mutation,
    },
    /// Delete one absolute member generation.
    DeleteMemberGeneration {
        /// Validated name of the dataset addressed by this operation.
        dataset: DatasetName,
        /// Member selector within the dataset.
        member: MemberName,
        /// Nonzero absolute member generation identity.
        generation: u64,
        /// Optional compare-and-update precondition on the current version.
        expected_version: Option<u64>,
        /// Replay sequence/key and optional transaction for this state change.
        mutation: Mutation,
    },
    /// Acquire a bounded logical lease for a dataset lock target.
    AcquireLock {
        /// Validated name of the dataset addressed by this operation.
        dataset: DatasetName,
        /// Dataset-wide or record-specific lock target; record identities must be nonempty and
        /// bounded.
        target: DatasetLockTarget,
        /// Principal identity owning the transaction or lock.
        owner: PrincipalId,
        /// Requested dataset lock compatibility mode.
        mode: DatasetLockMode,
        /// Nonzero caller-supplied logical tick used for lease or expiry evaluation.
        now_tick: u64,
        /// Positive lease duration in logical ticks.
        lease_ticks: u64,
        /// Optional bounded transaction identity; if present, must be nonempty and free of
        /// controls.
        transaction: Option<String>,
        /// Replay sequence/key and optional transaction for this state change.
        mutation: Mutation,
    },
    /// Release an owner-bound lock receipt.
    ReleaseLock {
        /// Validated name of the dataset addressed by this operation.
        dataset: DatasetName,
        /// Nonempty provider lock identity bounded by `max_name_bytes`.
        lock_id: String,
        /// Principal identity owning the transaction or lock.
        owner: PrincipalId,
        /// Replay sequence/key and optional transaction for this state change.
        mutation: Mutation,
    },
    /// Begin a dataset transactional unit of work for an owner.
    BeginTvs {
        /// Nonempty transaction identity bounded by `max_name_bytes` and free of control
        /// characters.
        transaction: String,
        /// Principal identity owning the transaction or lock.
        owner: PrincipalId,
        /// Replay sequence/key and optional transaction for this state change.
        mutation: Mutation,
    },
    /// Stage one typed record operation in a unit of work.
    StageTvs {
        /// Nonempty transaction identity bounded by `max_name_bytes` and free of control
        /// characters.
        transaction: String,
        /// Principal identity owning the transaction or lock.
        owner: PrincipalId,
        /// Owned typed record change to stage in the unit of work.
        operation: TvsRecordOperation,
        /// Replay sequence/key and optional transaction for this state change.
        mutation: Mutation,
    },
    /// Commit or roll back a dataset unit of work.
    CompleteTvs {
        /// Nonempty transaction identity bounded by `max_name_bytes` and free of control
        /// characters.
        transaction: String,
        /// Principal identity owning the transaction or lock.
        owner: PrincipalId,
        /// Choose commit (`true`) or rollback (`false`).
        commit: bool,
        /// Replay sequence/key and optional transaction for this state change.
        mutation: Mutation,
    },
    /// Record a known committed or rolled-back disposition.
    ReconcileTvs {
        /// Nonempty transaction identity bounded by `max_name_bytes` and free of control
        /// characters.
        transaction: String,
        /// Principal identity owning the transaction or lock.
        owner: PrincipalId,
        /// Known disposition supplied when reconciling the unit of work.
        committed: bool,
        /// Replay sequence/key and optional transaction for this state change.
        mutation: Mutation,
    },
    /// Write dataset or member record contents.
    Write {
        /// Validated name of the dataset addressed by this operation.
        dataset: DatasetName,
        /// Member selector within the dataset.
        member: Option<MemberName>,
        /// Owned logical record bytes; count and individual lengths are host-bounded.
        records: Vec<Vec<u8>>,
        /// Optional compare-and-update precondition on the current version.
        expected_version: Option<u64>,
        /// Replay sequence/key and optional transaction for this state change.
        mutation: Mutation,
    },
    /// Append records to existing dataset or member contents.
    Append {
        /// Validated name of the dataset addressed by this operation.
        dataset: DatasetName,
        /// Member selector within the dataset.
        member: Option<MemberName>,
        /// Owned logical record bytes; count and individual lengths are host-bounded.
        records: Vec<Vec<u8>>,
        /// Optional compare-and-update precondition on the current version.
        expected_version: Option<u64>,
        /// Replay sequence/key and optional transaction for this state change.
        mutation: Mutation,
    },
    /// Remove the dataset's current record contents.
    Truncate {
        /// Validated name of the dataset addressed by this operation.
        dataset: DatasetName,
        /// Optional compare-and-update precondition on the current version.
        expected_version: Option<u64>,
        /// Replay sequence/key and optional transaction for this state change.
        mutation: Mutation,
    },
    /// Replace a record selected by exact key bytes.
    RewriteRecord {
        /// Validated name of the dataset addressed by this operation.
        dataset: DatasetName,
        /// Owned record key bytes, bounded by `max_record_bytes`.
        key: Vec<u8>,
        /// Owned replacement record bytes, bounded by `max_record_bytes`.
        record: Vec<u8>,
        /// Optional compare-and-update precondition on the current version.
        expected_version: Option<u64>,
        /// Replay sequence/key and optional transaction for this state change.
        mutation: Mutation,
    },
    /// Delete a record selected by exact key bytes.
    DeleteRecord {
        /// Validated name of the dataset addressed by this operation.
        dataset: DatasetName,
        /// Owned record key bytes, bounded by `max_record_bytes`.
        key: Vec<u8>,
        /// Optional compare-and-update precondition on the current version.
        expected_version: Option<u64>,
        /// Replay sequence/key and optional transaction for this state change.
        mutation: Mutation,
    },
    /// Write a record at a relative record number.
    WriteRelative {
        /// Validated name of the dataset addressed by this operation.
        dataset: DatasetName,
        /// Nonzero relative record number supplied to the provider.
        record_number: u64,
        /// Owned replacement record bytes, bounded by `max_record_bytes`.
        record: Vec<u8>,
        /// Optional compare-and-update precondition on the current version.
        expected_version: Option<u64>,
        /// Replay sequence/key and optional transaction for this state change.
        mutation: Mutation,
    },
    /// Delete one relative record number.
    DeleteRelative {
        /// Validated name of the dataset addressed by this operation.
        dataset: DatasetName,
        /// Nonzero relative record number supplied to the provider.
        record_number: u64,
        /// Optional compare-and-update precondition on the current version.
        expected_version: Option<u64>,
        /// Replay sequence/key and optional transaction for this state change.
        mutation: Mutation,
    },
    /// Write bytes at a relative byte address.
    WriteRba {
        /// Validated name of the dataset addressed by this operation.
        dataset: DatasetName,
        /// Starting relative byte address of the requested or returned range.
        rba: u64,
        /// Owned segment bytes bounded by `max_record_bytes`, without implicit text decoding.
        data: Vec<u8>,
        /// Optional compare-and-update precondition on the current version.
        expected_version: Option<u64>,
        /// Replay sequence/key and optional transaction for this state change.
        mutation: Mutation,
    },
    /// Define an alternate key projection over a base dataset.
    DefineAlternateIndex {
        /// Validated name of the base dataset or generation group.
        base: DatasetName,
        /// Validated alternate-index dataset name.
        index: DatasetName,
        /// Zero-based alternate-key byte offset in a base record.
        key_offset: u32,
        /// Positive alternate-key byte length; offset plus length must fit `max_record_bytes`.
        key_length: u32,
        /// Whether the alternate key permits multiple base records.
        allow_duplicates: bool,
        /// Whether base mutations should maintain the alternate index.
        upgrade: bool,
        /// Replay sequence/key and optional transaction for this state change.
        mutation: Mutation,
    },
    /// Populate an alternate index from its base dataset.
    BuildAlternateIndex {
        /// Validated name of the base dataset or generation group.
        base: DatasetName,
        /// Validated alternate-index dataset name.
        index: DatasetName,
        /// Replay sequence/key and optional transaction for this state change.
        mutation: Mutation,
    },
    /// Define a named path through an alternate index.
    DefinePath {
        /// Validated name of the path through the alternate index.
        path: DatasetName,
        /// Validated alternate-index dataset name.
        index: DatasetName,
        /// Replay sequence/key and optional transaction for this state change.
        mutation: Mutation,
    },
    /// Define generation retention and rollover options.
    DefineGenerationGroup {
        /// Validated name of the base dataset or generation group.
        base: DatasetName,
        /// Positive retained-generation ceiling, at most `max_records`.
        limit: u32,
        /// Request removal of expired generation datasets on rollover.
        scratch: bool,
        /// Request emptying prior generations when the retention limit is reached.
        empty: bool,
        /// Replay sequence/key and optional transaction for this state change.
        mutation: Mutation,
    },
    /// Create a new dataset generation with owned initial records.
    CreateGeneration {
        /// Validated name of the base dataset or generation group.
        base: DatasetName,
        /// Logical layout attributes for the dataset.
        attributes: DatasetAttributes,
        /// Owned logical record bytes; count and individual lengths are host-bounded.
        records: Vec<Vec<u8>>,
        /// Replay sequence/key and optional transaction for this state change.
        mutation: Mutation,
    },
    /// Resolve a relative generation to an absolute dataset identity.
    ResolveGeneration {
        /// Validated name of the base dataset or generation group.
        base: DatasetName,
        /// Zero selects the current generation; negative values select older generations.
        relative: i32,
    },
    /// Rename a dataset with replay protection.
    Rename {
        /// Validated current dataset name.
        from: DatasetName,
        /// Validated replacement dataset name.
        to: DatasetName,
        /// Replay sequence/key and optional transaction for this state change.
        mutation: Mutation,
    },
    /// Delete a dataset or member with explicit retention controls.
    Delete {
        /// Validated name of the dataset addressed by this operation.
        dataset: DatasetName,
        /// Member selector within the dataset.
        member: Option<MemberName>,
        /// Optional compare-and-update precondition on the current version.
        expected_version: Option<u64>,
        /// Explicit request to override supported retention checks.
        purge: bool,
        /// Optional retention date in YYYYDDD form; year must be 1900–9999 and the day must exist.
        current_date: Option<u32>,
        /// Replay sequence/key and optional transaction for this state change.
        mutation: Mutation,
    },
    /// Create a browse positioned by key comparison.
    StartBrowse {
        /// Validated name of the dataset addressed by this operation.
        dataset: DatasetName,
        /// Owned record key bytes, bounded by `max_record_bytes`.
        key: Vec<u8>,
        /// Key comparison used to establish the browse position.
        relation: KeyRelation,
    },
    /// Reposition an existing browse cursor for one dataset.
    ResetBrowse {
        /// Dataset owning the cursor.
        dataset: DatasetName,
        /// Cursor identity returned by STARTBR.
        cursor: String,
        /// Target record key for the new browse position.
        key: Vec<u8>,
        /// Comparison used to select the new position.
        relation: KeyRelation,
    },
    /// Read and advance a provider-issued browse cursor.
    ReadNext {
        /// Validated name of the dataset addressed by this operation.
        dataset: DatasetName,
        /// Provider-issued browse identity associated with this dataset.
        cursor: String,
        /// Select reverse rather than forward traversal.
        reverse: bool,
        /// Typed access controls passed to the provider.
        control: DatasetReadControl,
    },
    /// Release a dataset browse cursor.
    EndBrowse {
        /// Validated name of the dataset addressed by this operation.
        dataset: DatasetName,
        /// Provider-issued browse identity associated with this dataset.
        cursor: String,
    },
    /// Close dataset access, optionally naming a browse cursor.
    Close {
        /// Validated name of the dataset addressed by this operation.
        dataset: DatasetName,
        /// Provider-issued browse identity associated with this dataset.
        cursor: Option<String>,
        /// Typed access controls passed to the provider.
        control: DatasetCloseControl,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Owned dataset replies, including versions, browse identities and structured conditions.
pub enum DatasetResult {
    /// Installed dataset provider feature set.
    Capabilities {
        /// Feature set reported by the installed dataset provider.
        capabilities: DatasetProviderCapabilities,
    },
    /// One page of dataset names.
    Listed {
        /// Validated names returned in this page.
        names: Vec<DatasetName>,
        /// Whether additional entries remain after this page.
        more: bool,
    },
    /// One page of member names.
    Members {
        /// Validated names returned in this page.
        names: Vec<MemberName>,
        /// Whether additional entries remain after this page.
        more: bool,
    },
    /// Logical layout and its observed version.
    Attributes {
        /// Logical layout attributes for the dataset.
        attributes: DatasetAttributes,
        /// Provider-observed version associated with the returned data or mutation.
        version: u64,
    },
    /// Full typed allocation and layout description.
    Description(Box<DatasetDescription>),
    /// Structured dataset diagnostic records.
    Diagnostics {
        /// Structured findings returned by dataset diagnosis.
        diagnostics: Vec<DatasetDiagnostic>,
    },
    /// Resolved catalog identity and target.
    Catalog(CatalogResolution),
    /// One page of catalog entries.
    CatalogEntries {
        /// Typed catalog entries in this bounded page.
        entries: Vec<CatalogListEntry>,
        /// Whether additional entries remain after this page.
        more: bool,
    },
    /// One page of volume descriptions.
    Volumes {
        /// Typed volume descriptions in this bounded page.
        volumes: Vec<crate::DatasetVolumeDescription>,
        /// Whether additional entries remain after this page.
        more: bool,
    },
    /// Owner-bound dataset lock receipts.
    Locks {
        /// Lock receipts returned for the selected dataset.
        locks: Vec<DatasetLockReceipt>,
    },
    /// Transactional unit-of-work receipt.
    Tvs(TvsUnitOfWorkReceipt),
    /// Owned snapshot and the version it represents.
    Snapshot {
        /// Owned dataset snapshot with layout and record contents.
        snapshot: Box<DatasetSnapshot>,
        /// Provider-observed version associated with the returned data or mutation.
        version: u64,
    },
    /// Record bytes paired with stable identities.
    Records {
        /// Owned logical record bytes; count and individual lengths are host-bounded.
        records: Vec<Vec<u8>>,
        /// Record identities paired one-for-one with `records`.
        identities: Vec<Vec<u8>>,
        /// Provider-observed version associated with the returned data or mutation.
        version: u64,
    },
    /// Record bytes and identities from one member generation.
    MemberGeneration {
        /// Owned logical record bytes; count and individual lengths are host-bounded.
        records: Vec<Vec<u8>>,
        /// Record identities paired one-for-one with `records`.
        identities: Vec<Vec<u8>>,
        /// Nonzero absolute member generation identity.
        generation: u64,
        /// Whether this member generation is marked as a program object.
        program_object: bool,
        /// Provider-observed version associated with the returned data or mutation.
        version: u64,
    },
    /// Returned byte range and its next relative byte address.
    Rba {
        /// Owned segment bytes bounded by `max_record_bytes`, without implicit text decoding.
        data: Vec<u8>,
        /// Whether the returned bytes represent a record rather than a raw byte range.
        record: bool,
        /// Starting relative byte address of the requested or returned range.
        rba: u64,
        /// Exclusive end address; host validation requires its distance from `rba` to equal the
        /// byte count.
        next_rba: u64,
        /// Provider-observed version associated with the returned data or mutation.
        version: u64,
    },
    /// Version assigned to a created dataset.
    Created {
        /// Provider-observed version associated with the returned data or mutation.
        version: u64,
    },
    /// Version observed after a dataset mutation.
    Mutated {
        /// Provider-observed version associated with the returned data or mutation.
        version: u64,
    },
    /// Provider cursor with an optional positioned record and its identity/key.
    Browse {
        /// Provider-issued browse identity associated with this dataset.
        cursor: String,
        /// Optional positioned record bytes; identity and key must be present together with it.
        record: Option<Vec<u8>>,
        /// Optional stable identity paired with the positioned record.
        identity: Option<Vec<u8>>,
        /// Optional key bytes for the positioned record.
        key: Option<Vec<u8>>,
    },
    /// Resolved absolute dataset generation.
    Generation {
        /// Validated name of the dataset addressed by this operation.
        dataset: DatasetName,
        /// Absolute generation number selected by catalog resolution.
        absolute_generation: u32,
        /// Provider-observed version associated with the returned data or mutation.
        version: u64,
    },
    /// Named dataset condition and provider status text.
    Condition {
        /// Named dataset condition returned by the provider.
        name: String,
        /// Provider status text accompanying the named dataset condition.
        status: String,
    },
}
