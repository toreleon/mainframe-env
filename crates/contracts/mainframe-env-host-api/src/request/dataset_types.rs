//! Dataset request/result records behind their unchanged host reexports.

use super::{DatasetAttributes, DatasetCloseControl, DatasetReadControl, KeyRelation, Mutation};
use crate::dataset::{
    CatalogKind, CatalogListEntry, CatalogResolution, DatasetDefinition, DatasetDescription,
    DatasetDiagnostic, DatasetLifecycleState, DatasetLockMode, DatasetLockReceipt,
    DatasetLockTarget, DatasetProviderCapabilities, DatasetSnapshot, TvsRecordOperation,
    TvsUnitOfWorkReceipt,
};
use crate::{DatasetName, MemberName};
use mainframe_env_execution_api::PrincipalId;

#[derive(Clone, Debug, Eq, PartialEq)]
/// Owned dataset operations. Structural validation bounds inputs; the provider checks capability, state, permissions and versions.
pub enum DatasetRequest {
    /// Observe declared dataset operand-family capabilities.
    Capabilities,
    /// List matching dataset names within an explicit page bound.
    List {
        /// Provider dataset-name listing pattern, bounded in bytes rather than executed as a language expression.
        pattern: String,
        /// Optional provider listing/traversal continuation position.
        start: Option<DatasetName>,
        /// Positive requested listing bound, no greater than HostLimits.max_records.
        max_items: u32,
    },
    /// Observe logical dataset attributes and version.
    Attributes {
        /// Validated dataset name whose provider state is addressed; the name grants no access.
        dataset: DatasetName,
    },
    /// Observe full definition, allocation and placement metadata.
    Describe {
        /// Validated dataset name whose provider state is addressed; the name grants no access.
        dataset: DatasetName,
    },
    /// Observe provider diagnostics without automatically repairing state.
    Diagnose {
        /// Validated dataset name whose provider state is addressed; the name grants no access.
        dataset: DatasetName,
    },
    /// Resolve aliases/catalog routing while preserving the requested name.
    ResolveCatalog {
        /// Catalog name to resolve; resolving it does not authorize the resulting dataset.
        name: DatasetName,
    },
    /// List catalog records within an explicit page bound.
    ListCatalog {
        /// Provider dataset-name listing pattern, bounded in bytes rather than executed as a language expression.
        pattern: String,
        /// Optional provider listing/traversal continuation position.
        start: Option<DatasetName>,
        /// Positive requested listing bound, no greater than HostLimits.max_records.
        max_items: u32,
    },
    /// Observe bounded provider volume allocation metadata.
    ListVolumes {
        /// Optional provider listing/traversal continuation position.
        start: Option<String>,
        /// Positive requested listing bound, no greater than HostLimits.max_records.
        max_items: u32,
    },
    /// Observe bounded locks at the supplied logical tick.
    ListLocks {
        /// Validated dataset name whose provider state is addressed; the name grants no access.
        dataset: DatasetName,
        /// Positive observed logical clock tick, in the provider's clock domain rather than wall-clock seconds.
        now_tick: u64,
        /// Positive requested listing bound, no greater than HostLimits.max_records.
        max_items: u32,
    },
    /// Observe the named owner's TVS unit without resolving it.
    TvsStatus {
        /// Transaction correlation retained verbatim; naming one does not establish ownership or commit it.
        transaction: String,
        /// Claimed principal retained for provider ownership checks; not an authentication credential.
        owner: PrincipalId,
    },
    /// List bounded member names within a library.
    ListMembers {
        /// Validated dataset name whose provider state is addressed; the name grants no access.
        dataset: DatasetName,
        /// Optional provider listing/traversal continuation position.
        start: Option<MemberName>,
        /// Positive requested listing bound, no greater than HostLimits.max_records.
        max_items: u32,
    },
    /// Read a current or older library generation; positive relative selectors are malformed.
    ReadMemberGeneration {
        /// Validated dataset name whose provider state is addressed; the name grants no access.
        dataset: DatasetName,
        /// Selected library member; absence in optional forms selects the nonmember form.
        member: MemberName,
        /// Nonpositive relative generation selector; positive future generations are rejected.
        relative: i32,
        /// Requested record/listing ceiling bounded by HostLimits.max_records.
        max_records: u32,
    },
    /// Read bounded records, retaining explicit key and lock/wait choices.
    Read {
        /// Validated dataset name whose provider state is addressed; the name grants no access.
        dataset: DatasetName,
        /// Selected library member; absence in optional forms selects the nonmember form.
        member: Option<MemberName>,
        /// Exact binary lookup identity, not a normalized text name.
        key: Option<Vec<u8>>,
        /// Requested record/listing ceiling bounded by HostLimits.max_records.
        max_records: u32,
        /// Explicit read or close policy, retained for provider legality checks.
        control: DatasetReadControl,
    },
    /// Select records by a nonempty binary key prefix.
    ReadGeneric {
        /// Validated dataset name whose provider state is addressed; the name grants no access.
        dataset: DatasetName,
        /// Nonempty exact binary prefix for bounded generic-key selection.
        key_prefix: Vec<u8>,
        /// Requested record/listing ceiling bounded by HostLimits.max_records.
        max_records: u32,
    },
    /// Read an ordered nonempty dataset concatenation.
    ReadConcatenation {
        /// Nonempty ordered concatenation members, bounded by max_records.
        datasets: Vec<DatasetName>,
        /// Selected library member; absence in optional forms selects the nonmember form.
        member: Option<MemberName>,
        /// Requested record/listing ceiling bounded by HostLimits.max_records.
        max_records: u32,
    },
    /// Read one positive relative-record slot.
    ReadRelative {
        /// Validated dataset name whose provider state is addressed; the name grants no access.
        dataset: DatasetName,
        /// Positive one-based relative record number.
        record_number: u64,
    },
    /// Read a bounded logical byte range or addressed record.
    ReadRba {
        /// Validated dataset name whose provider state is addressed; the name grants no access.
        dataset: DatasetName,
        /// Relative byte address within the dataset's logical byte space.
        rba: u64,
        /// Positive read byte ceiling, no greater than HostLimits.max_record_bytes.
        max_bytes: u32,
    },
    /// Traverse records from an optional starting position in either direction.
    ReadSequential {
        /// Validated dataset name whose provider state is addressed; the name grants no access.
        dataset: DatasetName,
        /// Selected library member; absence in optional forms selects the nonmember form.
        member: Option<MemberName>,
        /// Optional provider listing/traversal continuation position.
        start: Option<u64>,
        /// Select reverse traversal rather than reversing the returned bytes.
        reverse: bool,
        /// Requested record/listing ceiling bounded by HostLimits.max_records.
        max_records: u32,
    },
    /// Capture bounded organization-specific restore content.
    Snapshot {
        /// Validated dataset name whose provider state is addressed; the name grants no access.
        dataset: DatasetName,
        /// Requested record/listing ceiling bounded by HostLimits.max_records.
        max_records: u32,
        /// Positive requested member bound, no greater than HostLimits.max_records.
        max_members: u32,
    },
    /// Create using compatibility attributes and original mutation identity.
    Create {
        /// Validated dataset name whose provider state is addressed; the name grants no access.
        dataset: DatasetName,
        /// Logical record/organization definition validated before installation or mutation.
        attributes: DatasetAttributes,
        /// Original replay identity required before mutation dispatch; it does not confer permission.
        mutation: Mutation,
    },
    /// Create from a full structurally admitted definition.
    Define {
        /// Validated dataset name whose provider state is addressed; the name grants no access.
        dataset: DatasetName,
        /// Full owned definition; structural admission does not advertise installed provider support.
        definition: Box<DatasetDefinition>,
        /// Original replay identity required before mutation dispatch; it does not confer permission.
        mutation: Mutation,
    },
    /// Request a version-conditioned definition replacement.
    Alter {
        /// Validated dataset name whose provider state is addressed; the name grants no access.
        dataset: DatasetName,
        /// Full owned definition; structural admission does not advertise installed provider support.
        definition: Box<DatasetDefinition>,
        /// Optional provider version precondition; interpretation and conflict handling belong to the provider.
        expected_version: Option<u64>,
        /// Original replay identity required before mutation dispatch; it does not confer permission.
        mutation: Mutation,
    },
    /// Request a version-conditioned lifecycle transition; legality is provider-owned.
    SetLifecycle {
        /// Validated dataset name whose provider state is addressed; the name grants no access.
        dataset: DatasetName,
        /// Requested lifecycle state; transition and recovery legality remain provider-owned.
        state: DatasetLifecycleState,
        /// Optional provider version precondition; interpretation and conflict handling belong to the provider.
        expected_version: Option<u64>,
        /// Original replay identity required before mutation dispatch; it does not confer permission.
        mutation: Mutation,
    },
    /// Record a backup generation under the selected version precondition.
    RecordBackup {
        /// Validated dataset name whose provider state is addressed; the name grants no access.
        dataset: DatasetName,
        /// Optional provider version precondition; interpretation and conflict handling belong to the provider.
        expected_version: Option<u64>,
        /// Original replay identity required before mutation dispatch; it does not confer permission.
        mutation: Mutation,
    },
    /// Request bounded snapshot restoration under the selected version precondition.
    Restore {
        /// Validated dataset name whose provider state is addressed; the name grants no access.
        dataset: DatasetName,
        /// Owned restore/snapshot content, validated for organization-specific shape and aggregate bounds.
        snapshot: Box<DatasetSnapshot>,
        /// Optional provider version precondition; interpretation and conflict handling belong to the provider.
        expected_version: Option<u64>,
        /// Original replay identity required before mutation dispatch; it does not confer permission.
        mutation: Mutation,
    },
    /// Install a named master or user catalog.
    DefineCatalog {
        /// Catalog routing identity whose installation/connection is provider-owned.
        catalog: DatasetName,
        /// Master or user catalog classification.
        kind: CatalogKind,
        /// Original replay identity required before mutation dispatch; it does not confer permission.
        mutation: Mutation,
    },
    /// Request a catalog connection-state transition.
    SetCatalogConnection {
        /// Catalog routing identity whose installation/connection is provider-owned.
        catalog: DatasetName,
        /// Requested catalog connection state; not evidence of an existing connection.
        connected: bool,
        /// Optional provider version precondition; interpretation and conflict handling belong to the provider.
        expected_version: Option<u64>,
        /// Original replay identity required before mutation dispatch; it does not confer permission.
        mutation: Mutation,
    },
    /// Install a dataset/catalog alias relationship.
    DefineAlias {
        /// New alias name, separate from its destination.
        alias: DatasetName,
        /// Provider lock scope or alias destination, as selected by the enclosing operation.
        target: DatasetName,
        /// Original replay identity required before mutation dispatch; it does not confer permission.
        mutation: Mutation,
    },
    /// Install a library member alias relationship.
    DefineMemberAlias {
        /// Validated dataset name whose provider state is addressed; the name grants no access.
        dataset: DatasetName,
        /// New alias name, separate from its destination.
        alias: MemberName,
        /// Provider lock scope or alias destination, as selected by the enclosing operation.
        target: MemberName,
        /// Optional provider version precondition; interpretation and conflict handling belong to the provider.
        expected_version: Option<u64>,
        /// Original replay identity required before mutation dispatch; it does not confer permission.
        mutation: Mutation,
    },
    /// Create a new library member generation preserving content classification.
    WriteMemberGeneration {
        /// Validated dataset name whose provider state is addressed; the name grants no access.
        dataset: DatasetName,
        /// Selected library member; absence in optional forms selects the nonmember form.
        member: MemberName,
        /// Owned record bytes retained in order without text decoding or padding changes.
        records: Vec<Vec<u8>>,
        /// Retain program-object classification separately from member record bytes.
        program_object: bool,
        /// Optional provider version precondition; interpretation and conflict handling belong to the provider.
        expected_version: Option<u64>,
        /// Original replay identity required before mutation dispatch; it does not confer permission.
        mutation: Mutation,
    },
    /// Delete one positive absolute member generation.
    DeleteMemberGeneration {
        /// Validated dataset name whose provider state is addressed; the name grants no access.
        dataset: DatasetName,
        /// Selected library member; absence in optional forms selects the nonmember form.
        member: MemberName,
        /// Selected/observed generation identity; it is not the current mutable catalog version.
        generation: u64,
        /// Optional provider version precondition; interpretation and conflict handling belong to the provider.
        expected_version: Option<u64>,
        /// Original replay identity required before mutation dispatch; it does not confer permission.
        mutation: Mutation,
    },
    /// Request a provider-owned positive-duration lock with explicit owner and observed tick.
    AcquireLock {
        /// Validated dataset name whose provider state is addressed; the name grants no access.
        dataset: DatasetName,
        /// Provider lock scope or alias destination, as selected by the enclosing operation.
        target: DatasetLockTarget,
        /// Claimed principal retained for provider ownership checks; not an authentication credential.
        owner: PrincipalId,
        /// Requested shared/update/exclusive lock compatibility class.
        mode: DatasetLockMode,
        /// Positive observed logical clock tick, in the provider's clock domain rather than wall-clock seconds.
        now_tick: u64,
        /// Positive lease duration in the same clock domain as now_tick.
        lease_ticks: u64,
        /// Transaction correlation retained verbatim; naming one does not establish ownership or commit it.
        transaction: Option<String>,
        /// Original replay identity required before mutation dispatch; it does not confer permission.
        mutation: Mutation,
    },
    /// Release a lock only through its issued identity and owner checks.
    ReleaseLock {
        /// Validated dataset name whose provider state is addressed; the name grants no access.
        dataset: DatasetName,
        /// Provider-issued lock identity; release still requires ownership validation.
        lock_id: String,
        /// Claimed principal retained for provider ownership checks; not an authentication credential.
        owner: PrincipalId,
        /// Original replay identity required before mutation dispatch; it does not confer permission.
        mutation: Mutation,
    },
    /// Request a new owner-bound TVS staging unit.
    BeginTvs {
        /// Transaction correlation retained verbatim; naming one does not establish ownership or commit it.
        transaction: String,
        /// Claimed principal retained for provider ownership checks; not an authentication credential.
        owner: PrincipalId,
        /// Original replay identity required before mutation dispatch; it does not confer permission.
        mutation: Mutation,
    },
    /// Stage a record mutation without publishing it yet.
    StageTvs {
        /// Transaction correlation retained verbatim; naming one does not establish ownership or commit it.
        transaction: String,
        /// Claimed principal retained for provider ownership checks; not an authentication credential.
        owner: PrincipalId,
        /// Staged TVS mutation; it is not published by constructing the request.
        operation: TvsRecordOperation,
        /// Original replay identity required before mutation dispatch; it does not confer permission.
        mutation: Mutation,
    },
    /// Request commit or rollback of the named TVS unit.
    CompleteTvs {
        /// Transaction correlation retained verbatim; naming one does not establish ownership or commit it.
        transaction: String,
        /// Claimed principal retained for provider ownership checks; not an authentication credential.
        owner: PrincipalId,
        /// Select TVS commit versus rollback; the provider must resolve the actual outcome.
        commit: bool,
        /// Original replay identity required before mutation dispatch; it does not confer permission.
        mutation: Mutation,
    },
    /// Resolve a retained unknown TVS outcome using a known external decision.
    ReconcileTvs {
        /// Transaction correlation retained verbatim; naming one does not establish ownership or commit it.
        transaction: String,
        /// Claimed principal retained for provider ownership checks; not an authentication credential.
        owner: PrincipalId,
        /// Known resolution supplied to reconcile an Unknown TVS outcome; not a fresh commit instruction.
        committed: bool,
        /// Original replay identity required before mutation dispatch; it does not confer permission.
        mutation: Mutation,
    },
    /// Replace dataset/member records under an optional version precondition.
    Write {
        /// Validated dataset name whose provider state is addressed; the name grants no access.
        dataset: DatasetName,
        /// Selected library member; absence in optional forms selects the nonmember form.
        member: Option<MemberName>,
        /// Owned record bytes retained in order without text decoding or padding changes.
        records: Vec<Vec<u8>>,
        /// Optional provider version precondition; interpretation and conflict handling belong to the provider.
        expected_version: Option<u64>,
        /// Original replay identity required before mutation dispatch; it does not confer permission.
        mutation: Mutation,
    },
    /// Append records under an optional version precondition.
    Append {
        /// Validated dataset name whose provider state is addressed; the name grants no access.
        dataset: DatasetName,
        /// Selected library member; absence in optional forms selects the nonmember form.
        member: Option<MemberName>,
        /// Owned record bytes retained in order without text decoding or padding changes.
        records: Vec<Vec<u8>>,
        /// Optional provider version precondition; interpretation and conflict handling belong to the provider.
        expected_version: Option<u64>,
        /// Original replay identity required before mutation dispatch; it does not confer permission.
        mutation: Mutation,
    },
    /// Request content truncation under an optional version precondition.
    Truncate {
        /// Validated dataset name whose provider state is addressed; the name grants no access.
        dataset: DatasetName,
        /// Optional provider version precondition; interpretation and conflict handling belong to the provider.
        expected_version: Option<u64>,
        /// Original replay identity required before mutation dispatch; it does not confer permission.
        mutation: Mutation,
    },
    /// Replace the record selected by an exact binary key.
    RewriteRecord {
        /// Validated dataset name whose provider state is addressed; the name grants no access.
        dataset: DatasetName,
        /// Exact binary lookup identity, not a normalized text name.
        key: Vec<u8>,
        /// Complete owned record bytes; framing and mutation legality belong to the dataset provider.
        record: Vec<u8>,
        /// Optional provider version precondition; interpretation and conflict handling belong to the provider.
        expected_version: Option<u64>,
        /// Original replay identity required before mutation dispatch; it does not confer permission.
        mutation: Mutation,
    },
    /// Delete the record selected by an exact binary key.
    DeleteRecord {
        /// Validated dataset name whose provider state is addressed; the name grants no access.
        dataset: DatasetName,
        /// Exact binary lookup identity, not a normalized text name.
        key: Vec<u8>,
        /// Optional provider version precondition; interpretation and conflict handling belong to the provider.
        expected_version: Option<u64>,
        /// Original replay identity required before mutation dispatch; it does not confer permission.
        mutation: Mutation,
    },
    /// Replace a positive relative-record slot.
    WriteRelative {
        /// Validated dataset name whose provider state is addressed; the name grants no access.
        dataset: DatasetName,
        /// Positive one-based relative record number.
        record_number: u64,
        /// Complete owned record bytes; framing and mutation legality belong to the dataset provider.
        record: Vec<u8>,
        /// Optional provider version precondition; interpretation and conflict handling belong to the provider.
        expected_version: Option<u64>,
        /// Original replay identity required before mutation dispatch; it does not confer permission.
        mutation: Mutation,
    },
    /// Delete a positive relative-record slot.
    DeleteRelative {
        /// Validated dataset name whose provider state is addressed; the name grants no access.
        dataset: DatasetName,
        /// Positive one-based relative record number.
        record_number: u64,
        /// Optional provider version precondition; interpretation and conflict handling belong to the provider.
        expected_version: Option<u64>,
        /// Original replay identity required before mutation dispatch; it does not confer permission.
        mutation: Mutation,
    },
    /// Write nonempty bytes at a logical relative byte address.
    WriteRba {
        /// Validated dataset name whose provider state is addressed; the name grants no access.
        dataset: DatasetName,
        /// Relative byte address within the dataset's logical byte space.
        rba: u64,
        /// Exact nonempty logical-byte write payload bounded by max_record_bytes.
        data: Vec<u8>,
        /// Optional provider version precondition; interpretation and conflict handling belong to the provider.
        expected_version: Option<u64>,
        /// Original replay identity required before mutation dispatch; it does not confer permission.
        mutation: Mutation,
    },
    /// Define a checked key slice and duplicate/upgrade policy.
    DefineAlternateIndex {
        /// Base dataset/group identity for an index or generation relationship.
        base: DatasetName,
        /// Named alternate index participating in this operation.
        index: DatasetName,
        /// Zero-based index-key byte offset with checked range.
        key_offset: u32,
        /// Positive index-key byte length; checked end is bounded by max_record_bytes.
        key_length: u32,
        /// Declare whether duplicate alternate keys are admitted by the index.
        allow_duplicates: bool,
        /// Declare automatic maintenance of this alternate index on base changes.
        upgrade: bool,
        /// Original replay identity required before mutation dispatch; it does not confer permission.
        mutation: Mutation,
    },
    /// Request population of a defined alternate index from its base.
    BuildAlternateIndex {
        /// Base dataset/group identity for an index or generation relationship.
        base: DatasetName,
        /// Named alternate index participating in this operation.
        index: DatasetName,
        /// Original replay identity required before mutation dispatch; it does not confer permission.
        mutation: Mutation,
    },
    /// Install a named route to an alternate index.
    DefinePath {
        /// New path name referring to the selected alternate index.
        path: DatasetName,
        /// Named alternate index participating in this operation.
        index: DatasetName,
        /// Original replay identity required before mutation dispatch; it does not confer permission.
        mutation: Mutation,
    },
    /// Define bounded generation retention and rollover policies.
    DefineGenerationGroup {
        /// Base dataset/group identity for an index or generation relationship.
        base: DatasetName,
        /// Positive retained-generation count, bounded by max_records.
        limit: u32,
        /// Request deletion of rolled-off generation content rather than only uncataloging.
        scratch: bool,
        /// Request removal of all older generations at rollover rather than just the oldest.
        empty: bool,
        /// Original replay identity required before mutation dispatch; it does not confer permission.
        mutation: Mutation,
    },
    /// Create a generation with validated attributes and bounded records.
    CreateGeneration {
        /// Base dataset/group identity for an index or generation relationship.
        base: DatasetName,
        /// Logical record/organization definition validated before installation or mutation.
        attributes: DatasetAttributes,
        /// Owned record bytes retained in order without text decoding or padding changes.
        records: Vec<Vec<u8>>,
        /// Original replay identity required before mutation dispatch; it does not confer permission.
        mutation: Mutation,
    },
    /// Resolve a current/older generation without allocating a future generation.
    ResolveGeneration {
        /// Base dataset/group identity for an index or generation relationship.
        base: DatasetName,
        /// Nonpositive relative generation selector; positive future generations are rejected.
        relative: i32,
    },
    /// Request a dataset rename retaining original mutation identity.
    Rename {
        /// Existing source dataset identity for rename.
        from: DatasetName,
        /// Requested destination dataset identity for rename.
        to: DatasetName,
        /// Original replay identity required before mutation dispatch; it does not confer permission.
        mutation: Mutation,
    },
    /// Request dataset/member deletion with explicit retention inputs.
    Delete {
        /// Validated dataset name whose provider state is addressed; the name grants no access.
        dataset: DatasetName,
        /// Selected library member; absence in optional forms selects the nonmember form.
        member: Option<MemberName>,
        /// Optional provider version precondition; interpretation and conflict handling belong to the provider.
        expected_version: Option<u64>,
        /// Explicit retention-bypass request; permission and actual deletion remain provider-owned.
        purge: bool,
        /// Optional valid YYYYDDD date used for retention checks, never read from an implicit wall clock.
        current_date: Option<u32>,
        /// Original replay identity required before mutation dispatch; it does not confer permission.
        mutation: Mutation,
    },
    /// Create a provider cursor positioned by binary key relation.
    StartBrowse {
        /// Validated dataset name whose provider state is addressed; the name grants no access.
        dataset: DatasetName,
        /// Exact binary lookup identity, not a normalized text name.
        key: Vec<u8>,
        /// Binary-key comparison used to position the browse.
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
    /// Advance an existing browse and retain read-lock controls.
    ReadNext {
        /// Validated dataset name whose provider state is addressed; the name grants no access.
        dataset: DatasetName,
        /// Provider browse/session identity; it is not reconstructed from a numeric offset.
        cursor: String,
        /// Select reverse traversal rather than reversing the returned bytes.
        reverse: bool,
        /// Explicit read or close policy, retained for provider legality checks.
        control: DatasetReadControl,
    },
    /// Release the addressed provider browse cursor.
    EndBrowse {
        /// Validated dataset name whose provider state is addressed; the name grants no access.
        dataset: DatasetName,
        /// Provider browse/session identity; it is not reconstructed from a numeric offset.
        cursor: String,
    },
    /// Close a dataset or addressed cursor with validated disposition controls.
    Close {
        /// Validated dataset name whose provider state is addressed; the name grants no access.
        dataset: DatasetName,
        /// Provider browse/session identity; it is not reconstructed from a numeric offset.
        cursor: Option<String>,
        /// Explicit read or close policy, retained for provider legality checks.
        control: DatasetCloseControl,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Owned dataset observations and receipts. Variant presence is not proof of a supported provider or successful commit.
pub enum DatasetResult {
    /// Declared operand-family support, without execution credit.
    Capabilities {
        /// Provider operand-family declaration, not execution evidence.
        capabilities: DatasetProviderCapabilities,
    },
    /// Bounded dataset-name page.
    Listed {
        /// Bounded ordered names returned by the selected listing operation.
        names: Vec<DatasetName>,
        /// True when the provider reports additional entries beyond this bounded page.
        more: bool,
    },
    /// Bounded member-name page.
    Members {
        /// Bounded ordered names returned by the selected listing operation.
        names: Vec<MemberName>,
        /// True when the provider reports additional entries beyond this bounded page.
        more: bool,
    },
    /// Logical attribute observation and provider version.
    Attributes {
        /// Logical record/organization definition validated before installation or mutation.
        attributes: DatasetAttributes,
        /// Observed provider revision, distinct from a schema version or execution sequence.
        version: u64,
    },
    /// Full definition/allocation observation.
    Description(Box<DatasetDescription>),
    /// Bounded diagnostic observations without repair.
    Diagnostics {
        /// Bounded provider diagnostics without implicit repair action.
        diagnostics: Vec<DatasetDiagnostic>,
    },
    /// Resolved routing and alias traversal observation.
    Catalog(CatalogResolution),
    /// Bounded versioned catalog page.
    CatalogEntries {
        /// Versioned catalog listing entries.
        entries: Vec<CatalogListEntry>,
        /// True when the provider reports additional entries beyond this bounded page.
        more: bool,
    },
    /// Bounded ordered volume observations.
    Volumes {
        /// Sorted volume observations with contiguous positive extents.
        volumes: Vec<crate::DatasetVolumeDescription>,
        /// True when the provider reports additional entries beyond this bounded page.
        more: bool,
    },
    /// Observed provider lock receipts.
    Locks {
        /// Provider lock receipts, bounded by max_records.
        locks: Vec<DatasetLockReceipt>,
    },
    /// Retained TVS state and staged count, including uncertainty.
    Tvs(TvsUnitOfWorkReceipt),
    /// Owned restore content and its observed version.
    Snapshot {
        /// Owned restore/snapshot content, validated for organization-specific shape and aggregate bounds.
        snapshot: Box<DatasetSnapshot>,
        /// Observed provider revision, distinct from a schema version or execution sequence.
        version: u64,
    },
    /// Exact record/identity pairs with a provider version.
    Records {
        /// Owned record bytes retained in order without text decoding or padding changes.
        records: Vec<Vec<u8>>,
        /// One exact binary provider record identity per returned record, in the same order.
        identities: Vec<Vec<u8>>,
        /// Observed provider revision, distinct from a schema version or execution sequence.
        version: u64,
    },
    /// Exact member content and generation classification.
    MemberGeneration {
        /// Owned record bytes retained in order without text decoding or padding changes.
        records: Vec<Vec<u8>>,
        /// One exact binary provider record identity per returned record, in the same order.
        identities: Vec<Vec<u8>>,
        /// Selected/observed generation identity; it is not the current mutable catalog version.
        generation: u64,
        /// Retain program-object classification separately from member record bytes.
        program_object: bool,
        /// Observed provider revision, distinct from a schema version or execution sequence.
        version: u64,
    },
    /// Logical byte-range/record observation with a checked next address.
    Rba {
        /// Exact bytes returned for the requested logical range.
        data: Vec<u8>,
        /// Whether the RBA observation represents a record rather than a byte slice.
        record: bool,
        /// Relative byte address within the dataset's logical byte space.
        rba: u64,
        /// Next logical byte address; validated as rba plus returned byte length.
        next_rba: u64,
        /// Observed provider revision, distinct from a schema version or execution sequence.
        version: u64,
    },
    /// Provider receipt for newly created state.
    Created {
        /// Observed provider revision, distinct from a schema version or execution sequence.
        version: u64,
    },
    /// Provider receipt for a completed mutation.
    Mutated {
        /// Observed provider revision, distinct from a schema version or execution sequence.
        version: u64,
    },
    /// Cursor observation with all-or-none record/identity/key fields.
    Browse {
        /// Provider browse/session identity; it is not reconstructed from a numeric offset.
        cursor: String,
        /// Optional exact record bytes; presence must match identity and key.
        record: Option<Vec<u8>>,
        /// Optional exact record identity; presence must match record and key.
        identity: Option<Vec<u8>>,
        /// Optional exact browse key; presence must match record and identity.
        key: Option<Vec<u8>>,
    },
    /// Resolved absolute generation and dataset name.
    Generation {
        /// Validated dataset name whose provider state is addressed; the name grants no access.
        dataset: DatasetName,
        /// Absolute generation number resolved from the group selection.
        absolute_generation: u32,
        /// Observed provider revision, distinct from a schema version or execution sequence.
        version: u64,
    },
    /// Application condition/status text retained distinctly.
    Condition {
        /// Provider condition spelling; distinct from its returned status text.
        name: String,
        /// Provider condition status text, not normalized to a generic success.
        status: String,
    },
}
