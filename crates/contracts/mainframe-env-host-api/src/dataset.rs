use crate::names::{DatasetName, MemberName};
use crate::request::{
    DatasetAttributes, DatasetOrganization, HostLimits, HostProblem, RecordFormat,
};
use mainframe_env_execution_api::PrincipalId;
use serde::{Deserialize, Serialize};

/// Stable owned dataset-definition schema identity.
pub const DATASET_DEFINITION_CONTRACT: &str = "mainframe-env.dataset-definition@1";
/// Stable dataset request identity used by host framing.
pub const DATASET_REQUEST_CONTRACT: &str = "mainframe-env.host.dataset-request@2";
/// Stable dataset result identity used by host framing.
pub const DATASET_RESULT_CONTRACT: &str = "mainframe-env.host.dataset-result@2";
/// Stable provider operand-family capability descriptor identity.
pub const DATASET_PROVIDER_CAPABILITY_CONTRACT: &str =
    "mainframe-env.dataset-provider-capabilities@1";
/// Current dataset provider-state schema revision; not a dataset CAS version.
pub const DATASET_STATE_SCHEMA_VERSION: u16 = 6;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
/// Declared allocation quantity unit; abstract placement is not a physical device allocation claim.
pub enum SpaceUnit {
    /// Quantities expressed as tracks.
    Tracks,
    /// Quantities expressed as cylinders.
    Cylinders,
    /// Quantities expressed as blocks.
    Blocks,
    /// Quantities expressed as kilobytes.
    Kilobytes,
    /// Quantities expressed as megabytes.
    Megabytes,
    /// Quantities expressed as record slots.
    Records,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
/// Allocation requests with a positive primary quantity and optional provider-gated extent policies.
pub struct AllocationSpace {
    /// Unit in which primary and secondary quantities are expressed.
    pub unit: SpaceUnit,
    /// Positive initial allocation quantity in the selected unit.
    pub primary: u64,
    /// Requested additional extent quantity; nonzero requires allocation-extents capability.
    pub secondary: u64,
    /// Directory block count; nonzero exactly for partitioned organizations.
    pub directory_blocks: u32,
    /// Request release of unused allocation; requires allocation-extents capability.
    pub release_unused: bool,
    /// Request contiguous allocation; requires allocation-extents capability.
    pub contiguous: bool,
    /// Request cylinder rounding; requires allocation-extents capability.
    pub round_to_cylinder: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
/// Record blocking and buffering controls; zero block size leaves selection to the provider.
pub struct DcbOptions {
    /// Block byte count; zero leaves selection unspecified, fixed blocks must contain whole logical records.
    pub block_size: u32,
    /// Positive buffer count; nondefault selection requires buffering capability.
    pub buffer_count: u16,
    /// Optional positive per-buffer byte capacity, bounded by max_record_bytes.
    pub buffer_size: Option<u32>,
}

impl Default for DcbOptions {
    fn default() -> Self {
        Self {
            block_size: 0,
            buffer_count: 5,
            buffer_size: None,
        }
    }
}

impl Default for AllocationSpace {
    fn default() -> Self {
        Self {
            unit: SpaceUnit::Records,
            primary: 1,
            secondary: 0,
            directory_blocks: 0,
            release_unused: false,
            contiguous: false,
            round_to_cylinder: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
/// Placement domain used for explicit provider capability admission.
pub enum VolumeKind {
    /// Deterministic abstract placement.
    Abstract,
    /// Explicit disk placement requiring physical-volumes capability.
    PhysicalDisk,
    /// Explicit tape placement requiring tape capability.
    Tape,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
/// Nonempty distinct bounded volume labels and device selection; physical/tape operands require capability support.
pub struct VolumeSelection {
    /// Abstract, disk or tape selection checked against provider support.
    pub kind: VolumeKind,
    /// Nonempty distinct bounded labels; they are not native device handles.
    pub volume_ids: Vec<String>,
    /// Optional bounded device label requiring physical/tape capability.
    pub device_type: Option<String>,
    /// Positive requested device unit count, without allocating a device here.
    pub unit_count: u16,
}

impl Default for VolumeSelection {
    fn default() -> Self {
        Self {
            kind: VolumeKind::Abstract,
            volume_ids: vec!["MENV00".into()],
            device_type: None,
            unit_count: 1,
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
/// Optional SMS placement metadata; names preserve requests without implying an installed ACS implementation.
pub struct SmsClasses {
    /// Optional bounded data-class label requiring SMS support.
    pub data_class: Option<String>,
    /// Optional bounded management-class label requiring SMS support.
    pub management_class: Option<String>,
    /// Optional bounded storage-class label requiring SMS support.
    pub storage_class: Option<String>,
    /// Optional ACS routine label requiring explicit ACS capability.
    pub acs_routine: Option<String>,
    /// Guaranteed-space request requiring SMS capability.
    pub guaranteed_space: bool,
    /// Extended-format request requiring explicit capability.
    pub extended_format: bool,
    /// Extended-addressing request requiring explicit capability.
    pub extended_addressable: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
/// Requested compression family; non-None modes require explicit provider capability.
pub enum CompressionMode {
    /// No compression requested.
    None,
    /// Generic compression request requiring capability.
    Generic,
    /// Tailored compression request requiring capability.
    Tailored,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
/// Requested VSAM resource-sharing strategy; nondefault buffering is provider-gated.
pub enum BufferingMode {
    /// Default provider-selected buffering.
    System,
    /// Request nonshared buffering resources.
    NonsharedResources,
    /// Request local shared buffering resources.
    LocalSharedResources,
    /// Request global shared buffering resources.
    GlobalSharedResources,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
/// VSAM access mode. RLS and TVS need explicit capabilities; TVS is restricted to key-sequenced definitions.
pub enum VsamAccessMode {
    /// Ordinary non-RLS access selection.
    NonRls,
    /// Record-level sharing selection requiring RLS capability.
    Rls,
    /// Transactional VSAM selection requiring TVS capability and key-sequenced organization.
    Tvs,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Provider lock compatibility class, distinct from a security access intent.
pub enum DatasetLockMode {
    /// Shared read ownership class.
    Shared,
    /// Update ownership class.
    Update,
    /// Exclusive ownership class.
    Exclusive,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Dataset-wide or binary record identity scope for a provider-owned lock.
pub enum DatasetLockTarget {
    /// Scope the lock to the complete dataset.
    Dataset,
    /// Scope the lock to one nonempty exact binary record identity.
    Record(Vec<u8>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Observed lock ownership, expiry and version; possession of this data is not an authorization grant.
pub struct DatasetLockReceipt {
    /// Provider-issued lock identity; release still requires ownership validation.
    pub lock_id: String,
    /// Validated dataset name whose provider state is addressed; the name grants no access.
    pub dataset: DatasetName,
    /// Dataset-wide or exact binary record lock scope.
    pub target: DatasetLockTarget,
    /// Claimed principal retained for provider ownership checks; not an authentication credential.
    pub owner: PrincipalId,
    /// Observed lock compatibility class.
    pub mode: DatasetLockMode,
    /// Positive absolute logical expiry tick, not a wall-clock timestamp.
    pub expires_at: u64,
    /// Transaction correlation retained verbatim; naming one does not establish ownership or commit it.
    pub transaction: Option<String>,
    /// Observed provider revision, distinct from a schema version or execution sequence.
    pub version: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Staged TVS record mutation; staging does not itself publish the dataset change.
pub enum TvsRecordOperation {
    /// Stage insertion of complete nonempty record bytes.
    Insert {
        /// Validated dataset name whose provider state is addressed; the name grants no access.
        dataset: DatasetName,
        /// Complete owned record bytes; framing and mutation legality belong to the dataset provider.
        record: Vec<u8>,
    },
    /// Stage replacement by nonempty exact key and record bytes.
    Rewrite {
        /// Validated dataset name whose provider state is addressed; the name grants no access.
        dataset: DatasetName,
        /// Exact nonempty key bytes for the staged record operation.
        key: Vec<u8>,
        /// Complete owned record bytes; framing and mutation legality belong to the dataset provider.
        record: Vec<u8>,
    },
    /// Stage deletion by a nonempty exact key.
    Delete {
        /// Validated dataset name whose provider state is addressed; the name grants no access.
        dataset: DatasetName,
        /// Exact nonempty key bytes for the staged record operation.
        key: Vec<u8>,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Retained TVS outcome, keeping unresolved publication distinct from known commit or rollback.
pub enum TvsUnitOfWorkState {
    /// Staging remains open; no finalized decision yet.
    Active,
    /// Provider retains the known commit outcome.
    Committed,
    /// Provider retains the known rollback outcome.
    RolledBack,
    /// The publication outcome is unresolved and requires reconciliation.
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// TVS owner and staged-operation observation at one version, without coordinator completion authority.
pub struct TvsUnitOfWorkReceipt {
    /// Transaction correlation retained verbatim; naming one does not establish ownership or commit it.
    pub transaction: String,
    /// Claimed principal retained for provider ownership checks; not an authentication credential.
    pub owner: PrincipalId,
    /// Retained active/finalized/unknown outcome; Unknown does not imply rollback.
    pub state: TvsUnitOfWorkState,
    /// Number of retained staged mutations, not yet-published row count.
    pub staged_operations: u32,
    /// Observed provider revision, distinct from a schema version or execution sequence.
    pub version: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
/// VSAM cross-region/system share selections with contract-validated numeric domains.
pub struct DatasetShareOptions {
    /// Validated cross-region share option in 1 through 4.
    pub cross_region: u8,
    /// Validated cross-system share option in 3 through 4.
    pub cross_system: u8,
}

impl Default for DatasetShareOptions {
    fn default() -> Self {
        Self {
            cross_region: 1,
            cross_system: 3,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
/// VSAM-only options validated against organization, record framing and advertised provider capabilities.
pub struct VsamAttributes {
    /// Optional interval bytes in 512 through 32768, in multiples of 512.
    pub control_interval_size: Option<u32>,
    /// Optional positive area bytes, at least one interval and a multiple of its size.
    pub control_area_size: Option<u64>,
    /// Validated sharing policy; nondefault selection requires sharing capability.
    pub share_options: DatasetShareOptions,
    /// Non-RLS, RLS or TVS admission choice; no ownership is created here.
    pub access_mode: VsamAccessMode,
    /// Must agree exactly with the selected spanned record format.
    pub spanned: bool,
    /// Requested reuse policy requiring VSAM-data-options capability.
    pub reuse: bool,
    /// Requested speed policy requiring VSAM-data-options capability.
    pub speed: bool,
    /// Requested write checking requiring VSAM-data-options capability.
    pub write_check: bool,
    /// Requested erase policy requiring VSAM-data-options capability.
    pub erase_on_delete: bool,
    /// Buffer sharing strategy; nondefault selection requires buffering capability.
    pub buffering: BufferingMode,
    /// Positive stripe count; values other than one require striping capability.
    pub stripe_count: u16,
}

impl Default for VsamAttributes {
    fn default() -> Self {
        Self {
            control_interval_size: None,
            control_area_size: None,
            share_options: DatasetShareOptions::default(),
            access_mode: VsamAccessMode::NonRls,
            spanned: false,
            reuse: false,
            speed: false,
            write_check: false,
            erase_on_delete: false,
            buffering: BufferingMode::System,
            stripe_count: 1,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
/// Requested key-label and compression metadata; it carries no key material or cryptographic execution proof.
pub struct DataSecurity {
    /// Optional bounded key locator requiring encryption capability; no key bytes are retained.
    pub encryption_key_label: Option<String>,
    /// Requested compression family; non-None requires explicit capability.
    pub compression: CompressionMode,
}

impl Default for DataSecurity {
    fn default() -> Self {
        Self {
            encryption_key_label: None,
            compression: CompressionMode::None,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
/// Catalog record classification; existence in this enum does not advertise provider support.
pub enum CatalogEntryKind {
    /// Ordinary dataset entry.
    Dataset,
    /// Alternate index relationship entry.
    AlternateIndex,
    /// Named alternate-index access path.
    Path,
    /// Name indirection entry.
    Alias,
    /// Generation-group definition.
    GenerationDataGroup,
    /// User catalog entry.
    UserCatalog,
    /// Master catalog entry.
    MasterCatalog,
    /// Library catalog classification.
    Library,
    /// Volume catalog classification.
    Volume,
    /// Page-space catalog classification.
    PageSpace,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Master or user catalog routing identity.
pub enum CatalogKind {
    /// Master catalog routing domain.
    Master,
    /// User catalog routing domain.
    User,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Requested-to-resolved catalog observation preserving alias traversal and positive version.
pub struct CatalogResolution {
    /// Original catalog name before alias resolution.
    pub requested: DatasetName,
    /// Final resolved dataset/catalog name.
    pub resolved: DatasetName,
    /// Optional catalog owning the resolution.
    pub catalog: Option<DatasetName>,
    /// Ordered traversed alias names, bounded by max_records.
    pub alias_chain: Vec<DatasetName>,
    /// Observed provider revision, distinct from a schema version or execution sequence.
    pub version: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// One versioned catalog listing item and its optional related object.
pub struct CatalogListEntry {
    /// Catalog item identity.
    pub name: DatasetName,
    /// Observed catalog record classification.
    pub kind: CatalogEntryKind,
    /// Optional related dataset/index/catalog identity.
    pub related: Option<DatasetName>,
    /// Observed provider revision, distinct from a schema version or execution sequence.
    pub version: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
/// Catalog labels and retention dates; date fields use YYYYDDD and are validated for leap-year boundaries.
pub struct CatalogMetadata {
    /// Requested catalog record classification.
    pub entry_kind: CatalogEntryKind,
    /// Optional explicit catalog routing name.
    pub catalog: Option<DatasetName>,
    /// Optional bounded catalog owner label, not an authenticated principal.
    pub owner: Option<String>,
    /// Optional valid YYYYDDD creation date in years 1900 through 9999.
    pub creation_date: Option<u32>,
    /// Optional valid YYYYDDD date at or after creation; mutually exclusive with retention_days.
    pub expiration_date: Option<u32>,
    /// Optional retention duration in days; requires creation_date and excludes expiration_date.
    pub retention_days: Option<u16>,
}

impl Default for CatalogMetadata {
    fn default() -> Self {
        Self {
            entry_kind: CatalogEntryKind::Dataset,
            catalog: None,
            owner: None,
            creation_date: None,
            expiration_date: None,
            retention_days: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
/// Dataset lifecycle observation; pending recall and recovery are not open/usable success states.
pub enum DatasetLifecycleState {
    /// Allocation exists before catalog/open use.
    Allocated,
    /// Cataloged state without an active open observation.
    Cataloged,
    /// Provider reports an open lifecycle state.
    Open,
    /// Provider reports closed lifecycle state.
    Closed,
    /// Content requires migration/recall handling before ordinary use.
    Migrated,
    /// Recall is requested but not completed.
    RecallPending,
    /// Recovery is required; ordinary completion must not be fabricated.
    RecoveryRequired,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
/// Retained lifecycle state and migration/backup metadata, not a recovery decision.
pub struct LifecycleMetadata {
    /// Retained lifecycle observation, including pending recall/recovery.
    pub state: DatasetLifecycleState,
    /// Modeled migration level; nonzero requires migration-recall capability.
    pub migration_level: u8,
    /// Retained backup generation counter, not a machine checkpoint identifier.
    pub backup_generation: u64,
}

impl Default for LifecycleMetadata {
    fn default() -> Self {
        Self {
            state: DatasetLifecycleState::Cataloged,
            migration_level: 0,
            backup_generation: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Operand-family support declaration checked during definition admission, not licensed execution evidence.
pub struct DatasetProviderCapabilities {
    /// Capability descriptor schema revision, distinct from provider state schema.
    pub schema_version: u16,
    /// Declare support for abstract placement; declaration alone is not execution evidence.
    pub abstract_volumes: bool,
    /// Declare support for secondary allocation, release, contiguity and rounding; declaration alone is not execution evidence.
    pub allocation_extents: bool,
    /// Declare support for nondefault buffer count/size and resource sharing; declaration alone is not execution evidence.
    pub buffering: bool,
    /// Declare support for nondefault catalog type, owner and retention; declaration alone is not execution evidence.
    pub catalog_metadata: bool,
    /// Declare support for explicit catalog routing; declaration alone is not execution evidence.
    pub catalog_routing: bool,
    /// Declare support for explicit VSAM interval/area sizes; declaration alone is not execution evidence.
    pub control_intervals: bool,
    /// Declare support for extended format/addressability; declaration alone is not execution evidence.
    pub extended_format: bool,
    /// Declare support for disk/device placement; declaration alone is not execution evidence.
    pub physical_volumes: bool,
    /// Declare support for tape placement; declaration alone is not execution evidence.
    pub tape: bool,
    /// Declare support for ACS routine selection; declaration alone is not execution evidence.
    pub sms_acs: bool,
    /// Declare support for key-label encryption; declaration alone is not execution evidence.
    pub encryption: bool,
    /// Declare support for non-None compression; declaration alone is not execution evidence.
    pub compression: bool,
    /// Declare support for multiple stripes; declaration alone is not execution evidence.
    pub striping: bool,
    /// Declare support for nondefault migration/lifecycle requests; declaration alone is not execution evidence.
    pub migration_recall: bool,
    /// Declare support for record-level sharing mode; declaration alone is not execution evidence.
    pub rls: bool,
    /// Declare support for nondefault share options; declaration alone is not execution evidence.
    pub sharing: bool,
    /// Declare support for SMS classes and guaranteed space; declaration alone is not execution evidence.
    pub sms_classes: bool,
    /// Declare support for transactional VSAM mode; declaration alone is not execution evidence.
    pub tvs: bool,
    /// Declare support for REUSE, SPEED, WRITECHECK and ERASE; declaration alone is not execution evidence.
    pub vsam_data_options: bool,
}

impl DatasetProviderCapabilities {
    #[must_use]
    /// Declare the minimal deterministic abstract profile; physical and deferred operand families remain false.
    pub const fn deterministic_abstract() -> Self {
        Self {
            schema_version: 1,
            abstract_volumes: true,
            allocation_extents: false,
            buffering: false,
            catalog_metadata: false,
            catalog_routing: true,
            control_intervals: true,
            extended_format: false,
            physical_volumes: false,
            tape: false,
            sms_acs: false,
            encryption: false,
            compression: false,
            striping: false,
            migration_recall: false,
            rls: false,
            sharing: false,
            sms_classes: false,
            tvs: false,
            vsam_data_options: false,
        }
    }

    #[must_use]
    /// Enable all represented operand families for structural validation; this does not assert any installed provider implements them.
    pub const fn all_contract_capabilities() -> Self {
        Self {
            schema_version: 1,
            abstract_volumes: true,
            allocation_extents: true,
            buffering: true,
            catalog_metadata: true,
            catalog_routing: true,
            control_intervals: true,
            extended_format: true,
            physical_volumes: true,
            tape: true,
            sms_acs: true,
            encryption: true,
            compression: true,
            striping: true,
            migration_recall: true,
            rls: true,
            sharing: true,
            sms_classes: true,
            tvs: true,
            vsam_data_options: true,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
/// Owned dataset definition whose structural and capability admission remains separate from installation and permissions.
pub struct DatasetDefinition {
    /// Logical record/organization definition validated before installation or mutation.
    pub attributes: DatasetAttributes,
    /// Record blocking and buffering selection.
    pub dcb: DcbOptions,
    /// Allocation quantities and extent policy.
    pub allocation: AllocationSpace,
    /// Requested placement labels/domain.
    pub volumes: VolumeSelection,
    /// Optional provider-gated SMS selection.
    pub sms: SmsClasses,
    /// Organization-dependent VSAM options.
    pub vsam: VsamAttributes,
    /// Key-label and compression requests, without key material.
    pub security: DataSecurity,
    /// Catalog classification, routing and retention.
    pub catalog: CatalogMetadata,
    /// Requested initial lifecycle metadata.
    pub lifecycle: LifecycleMetadata,
}

impl DatasetDefinition {
    #[must_use]
    /// Wrap attributes in current contract defaults, adding one directory block for partitioned organizations; no dataset is created.
    pub fn compatibility(attributes: DatasetAttributes) -> Self {
        let mut allocation = AllocationSpace::default();
        if matches!(
            attributes.organization,
            DatasetOrganization::Partitioned | DatasetOrganization::PartitionedExtended
        ) {
            allocation.directory_blocks = 1;
        }
        Self {
            attributes,
            dcb: DcbOptions::default(),
            allocation,
            volumes: VolumeSelection::default(),
            sms: SmsClasses::default(),
            vsam: VsamAttributes::default(),
            security: DataSecurity::default(),
            catalog: CatalogMetadata::default(),
            lifecycle: LifecycleMetadata::default(),
        }
    }

    /// Check record/allocation/date/VSAM shape, then reject requested unsupported families with UnsupportedCapability; no state or clock is mutated.
    pub fn validate(
        &self,
        limits: HostLimits,
        capabilities: DatasetProviderCapabilities,
    ) -> Result<(), HostProblem> {
        self.attributes.validate(limits)?;
        if self.dcb.block_size as usize > limits.max_record_bytes
            || self.dcb.buffer_count == 0
            || self
                .dcb
                .buffer_size
                .is_some_and(|size| size == 0 || size as usize > limits.max_record_bytes)
            || self.allocation.primary == 0
            || self.allocation.directory_blocks as usize > limits.max_records
            || self.volumes.volume_ids.is_empty()
            || self.volumes.volume_ids.len() > limits.max_records
            || self
                .volumes
                .volume_ids
                .iter()
                .enumerate()
                .any(|(position, volume)| self.volumes.volume_ids[..position].contains(volume))
            || self.volumes.unit_count == 0
            || self
                .catalog
                .catalog
                .as_ref()
                .is_some_and(|catalog| catalog.as_str().len() > limits.max_name_bytes)
        {
            return Err(HostProblem::Malformed);
        }
        if self.dcb.block_size != 0
            && matches!(
                self.attributes.record_format,
                RecordFormat::Fixed
                    | RecordFormat::FixedBlocked
                    | RecordFormat::FixedBlockedStandard
            )
            && (self.dcb.block_size < self.attributes.logical_record_length
                || !self
                    .dcb
                    .block_size
                    .is_multiple_of(self.attributes.logical_record_length))
        {
            return Err(HostProblem::Malformed);
        }
        for value in self
            .volumes
            .volume_ids
            .iter()
            .chain(self.volumes.device_type.iter())
            .chain(self.sms.data_class.iter())
            .chain(self.sms.management_class.iter())
            .chain(self.sms.storage_class.iter())
            .chain(self.sms.acs_routine.iter())
            .chain(self.security.encryption_key_label.iter())
            .chain(self.catalog.owner.iter())
        {
            validate_label(value, limits)?;
        }
        if self.catalog.expiration_date.is_some() && self.catalog.retention_days.is_some()
            || self.catalog.retention_days.is_some() && self.catalog.creation_date.is_none()
        {
            return Err(HostProblem::Malformed);
        }
        for date in [self.catalog.creation_date, self.catalog.expiration_date]
            .into_iter()
            .flatten()
        {
            if !valid_julian_date(date) {
                return Err(HostProblem::Malformed);
            }
        }
        if let (Some(created), Some(expires)) =
            (self.catalog.creation_date, self.catalog.expiration_date)
            && created > expires
        {
            return Err(HostProblem::Malformed);
        }
        if !matches!(self.vsam.share_options.cross_region, 1..=4)
            || !matches!(self.vsam.share_options.cross_system, 3..=4)
        {
            return Err(HostProblem::Malformed);
        }
        if let Some(size) = self.vsam.control_interval_size
            && (!(512..=32768).contains(&size) || size % 512 != 0)
        {
            return Err(HostProblem::Malformed);
        }
        if self.vsam.control_area_size == Some(0) || self.vsam.stripe_count == 0 {
            return Err(HostProblem::Malformed);
        }
        if let Some(area) = self.vsam.control_area_size {
            let interval = u64::from(self.vsam.control_interval_size.unwrap_or(4096));
            if area < interval || area % interval != 0 {
                return Err(HostProblem::Malformed);
            }
        }
        let is_vsam = matches!(
            self.attributes.organization,
            DatasetOrganization::KeySequenced
                | DatasetOrganization::EntrySequenced
                | DatasetOrganization::Relative
                | DatasetOrganization::VariableRelative
                | DatasetOrganization::Linear
        );
        let is_partitioned = matches!(
            self.attributes.organization,
            DatasetOrganization::Partitioned | DatasetOrganization::PartitionedExtended
        );
        if is_partitioned != (self.allocation.directory_blocks != 0) {
            return Err(HostProblem::Malformed);
        }
        if !is_vsam && self.vsam != VsamAttributes::default() {
            return Err(HostProblem::Malformed);
        }
        if self.attributes.organization == DatasetOrganization::Linear
            && (self.attributes.record_format != RecordFormat::Undefined
                || self.attributes.key_offset.is_some()
                || self.vsam.spanned)
        {
            return Err(HostProblem::Malformed);
        }
        if self.vsam.access_mode == VsamAccessMode::Tvs
            && self.attributes.organization != DatasetOrganization::KeySequenced
        {
            return Err(HostProblem::Malformed);
        }
        if self.attributes.organization == DatasetOrganization::VariableRelative
            && !matches!(
                self.attributes.record_format,
                RecordFormat::Variable
                    | RecordFormat::VariableBlocked
                    | RecordFormat::VariableSpanned
                    | RecordFormat::VariableBlockedSpanned
            )
        {
            return Err(HostProblem::Malformed);
        }
        if self.vsam.spanned
            != matches!(
                self.attributes.record_format,
                RecordFormat::VariableSpanned | RecordFormat::VariableBlockedSpanned
            )
        {
            return Err(HostProblem::Malformed);
        }
        require_capability(
            (self.allocation.secondary == 0
                && !self.allocation.release_unused
                && !self.allocation.contiguous
                && !self.allocation.round_to_cylinder)
                || capabilities.allocation_extents,
            "allocation-extents",
            "SECONDARY/RLSE/CONTIG/ROUND",
        )?;
        require_capability(
            (self.dcb.buffer_count == DcbOptions::default().buffer_count
                && self.dcb.buffer_size.is_none()
                && self.vsam.buffering == BufferingMode::System)
                || capabilities.buffering,
            "buffering",
            "BUFNO/BUFSIZE/BUFFERING",
        )?;
        require_capability(
            self.catalog.catalog.is_none() || capabilities.catalog_routing,
            "catalog-routing",
            "CATALOG",
        )?;
        require_capability(
            (self.catalog.entry_kind == CatalogEntryKind::Dataset
                && self.catalog.owner.is_none()
                && self.catalog.expiration_date.is_none()
                && self.catalog.retention_days.is_none())
                || capabilities.catalog_metadata,
            "catalog-metadata",
            "OWNER/EXPIRATION/RETPD/ENTRYTYPE",
        )?;
        require_capability(
            (self.vsam.control_interval_size.is_none() && self.vsam.control_area_size.is_none())
                || capabilities.control_intervals,
            "control-intervals",
            "CONTROLINTERVALSIZE/CONTROLAREASIZE",
        )?;
        require_capability(
            (!self.sms.extended_format && !self.sms.extended_addressable)
                || capabilities.extended_format,
            "extended-format",
            "EXTENDED/EXTENDEDADDRESSABLE",
        )?;
        require_capability(
            (self.sms.data_class.is_none()
                && self.sms.management_class.is_none()
                && self.sms.storage_class.is_none()
                && !self.sms.guaranteed_space)
                || capabilities.sms_classes,
            "sms-classes",
            "DATACLAS/MGMTCLAS/STORCLAS/GUARANTEEDSPACE",
        )?;
        require_capability(
            self.vsam.share_options == DatasetShareOptions::default() || capabilities.sharing,
            "sharing",
            "SHAREOPTIONS",
        )?;
        require_capability(
            self.vsam.access_mode != VsamAccessMode::Rls || capabilities.rls,
            "rls",
            "RLS",
        )?;
        require_capability(
            self.vsam.access_mode != VsamAccessMode::Tvs || capabilities.tvs,
            "tvs",
            "TVS",
        )?;
        require_capability(
            (self.lifecycle.migration_level == 0
                && !matches!(
                    self.lifecycle.state,
                    DatasetLifecycleState::Migrated | DatasetLifecycleState::RecallPending
                ))
                || capabilities.migration_recall,
            "migration-recall",
            "MIGRATE/RECALL",
        )?;
        require_capability(
            (!self.vsam.reuse
                && !self.vsam.speed
                && !self.vsam.write_check
                && !self.vsam.erase_on_delete)
                || capabilities.vsam_data_options,
            "vsam-data-options",
            "REUSE/SPEED/WRITECHECK/ERASE",
        )?;
        require_capability(
            self.volumes.kind != VolumeKind::PhysicalDisk || capabilities.physical_volumes,
            "physical-volumes",
            "VOLUME/UNIT",
        )?;
        require_capability(
            self.volumes.kind != VolumeKind::Tape || capabilities.tape,
            "tape",
            "VOLUME/UNIT",
        )?;
        require_capability(
            (self.volumes.kind == VolumeKind::Abstract && self.volumes.device_type.is_none())
                || capabilities.physical_volumes
                || capabilities.tape,
            "physical-volumes",
            "UNIT/DEVICE/UNITCOUNT",
        )?;
        require_capability(
            self.sms.acs_routine.is_none() || capabilities.sms_acs,
            "sms-acs",
            "ACSROUTINE",
        )?;
        require_capability(
            self.security.encryption_key_label.is_none() || capabilities.encryption,
            "encryption",
            "KEYLABEL",
        )?;
        require_capability(
            self.security.compression == CompressionMode::None || capabilities.compression,
            "compression",
            "COMPRESS",
        )?;
        require_capability(
            self.vsam.stripe_count == 1 || capabilities.striping,
            "striping",
            "STRIPECOUNT",
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Versioned definition, logical allocation and placement observations; byte counters do not imply a physical device.
pub struct DatasetDescription {
    /// Complete observed definition.
    pub definition: DatasetDefinition,
    /// Observed provider revision, distinct from a schema version or execution sequence.
    pub version: u64,
    /// Total logical allocation in bytes, matching the ordered extent sum.
    pub allocated_bytes: u64,
    /// Observed logical used byte count.
    pub used_bytes: u64,
    /// Modeled control-interval count.
    pub control_intervals: u64,
    /// Modeled control-area count.
    pub control_areas: u64,
    /// Highest used relative byte address, no greater than max_rba.
    pub high_used_rba: u64,
    /// Maximum logical relative byte address.
    pub max_rba: u64,
    /// Nonempty ordered contiguous positive logical extents.
    pub extents: Vec<DatasetExtent>,
    /// Positive modeled buffering allocation in bytes.
    pub buffer_bytes: u64,
    /// Nonempty bounded abstract placement identity; not a physical-device claim.
    pub abstract_placement: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Ordered positive logical extent and corresponding provider volume range, both measured in bytes.
pub struct DatasetExtent {
    /// Zero-based position in the ordered extent list.
    pub ordinal: u32,
    /// Logical byte start, contiguous with the preceding extent.
    pub start: u64,
    /// Corresponding volume-relative byte start.
    pub volume_start: u64,
    /// Positive extent length in bytes.
    pub length: u64,
    /// Bounded provider volume label.
    pub volume_id: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
/// One dataset extent mapped into an owned volume byte range.
pub struct DatasetVolumeExtent {
    /// Validated dataset name whose provider state is addressed; the name grants no access.
    pub dataset: DatasetName,
    /// Dataset extent ordinal mapped into this volume.
    pub dataset_extent_ordinal: u32,
    /// Dataset-relative logical byte start.
    pub logical_start: u64,
    /// Volume-relative byte start.
    pub volume_start: u64,
    /// Positive extent byte length.
    pub length: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
/// Volume allocation observation with ordered contiguous extents and used bytes bounded by allocation.
pub struct DatasetVolumeDescription {
    /// Bounded volume label, sorted uniquely in a volume listing.
    pub volume_id: String,
    /// Volume allocation bytes matching the contiguous extent sum.
    pub allocated_bytes: u64,
    /// Used bytes, no greater than allocated_bytes.
    pub used_bytes: u64,
    /// Nonempty contiguous positive volume extents.
    pub extents: Vec<DatasetVolumeExtent>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
/// Owned restore data partitioned by organization; snapshot shape does not grant restore permission.
pub struct DatasetSnapshot {
    /// Owned organization/definition restored with this content.
    pub definition: DatasetDefinition,
    /// Owned record bytes retained in order without text decoding or padding changes.
    pub records: Vec<Vec<u8>>,
    /// Strictly increasing positive relative-record slots.
    pub relative_records: Vec<DatasetRelativeRecordSnapshot>,
    /// Strictly ordered unique library members.
    pub members: Vec<DatasetMemberSnapshot>,
    /// Complete linear byte content; mutually exclusive with record/member forms.
    pub linear_data: Vec<u8>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
/// One positive relative record number and its exact record bytes.
pub struct DatasetRelativeRecordSnapshot {
    /// Positive one-based relative record number.
    pub record_number: u64,
    /// Complete owned record bytes; framing and mutation legality belong to the dataset provider.
    pub record: Vec<u8>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
/// Member content, PDSE generations or alias target; alias snapshots cannot also carry content.
pub struct DatasetMemberSnapshot {
    /// Member identity, sorted uniquely within a snapshot.
    pub name: MemberName,
    /// Owned record bytes retained in order without text decoding or padding changes.
    pub records: Vec<Vec<u8>>,
    /// Strictly increasing positive PDSE generations.
    pub generations: Vec<DatasetMemberGenerationSnapshot>,
    /// Optional distinct nonalias target in the same snapshot; alias content must be empty.
    pub alias_of: Option<MemberName>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
/// Positive ordered member generation preserving program-object classification and record bytes.
pub struct DatasetMemberGenerationSnapshot {
    /// Selected/observed generation identity; it is not the current mutable catalog version.
    pub generation: u64,
    /// Retain program-object classification separately from member record bytes.
    pub program_object: bool,
    /// Owned record bytes retained in order without text decoding or padding changes.
    pub records: Vec<Vec<u8>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Bounded provider diagnostic text; it is not a standardized machine status or repair receipt.
pub struct DatasetDiagnostic {
    /// Nonempty bounded provider diagnostic identifier.
    pub code: String,
    /// Optional bounded affected-field label.
    pub field: Option<String>,
    /// Bounded explanatory text, not an automatic repair action.
    pub detail: String,
}

fn validate_label(value: &str, limits: HostLimits) -> Result<(), HostProblem> {
    if value.is_empty()
        || value.len() > limits.max_name_bytes
        || value.chars().any(char::is_control)
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

fn require_capability(ok: bool, capability: &str, operand: &str) -> Result<(), HostProblem> {
    if ok {
        Ok(())
    } else {
        Err(HostProblem::UnsupportedCapability {
            capability: capability.into(),
            detail: format!("{operand} requires provider capability {capability}"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attributes(organization: DatasetOrganization) -> DatasetAttributes {
        DatasetAttributes {
            organization,
            record_format: if organization == DatasetOrganization::Linear {
                RecordFormat::Undefined
            } else {
                RecordFormat::Fixed
            },
            logical_record_length: 80,
            key_offset: (organization == DatasetOrganization::KeySequenced).then_some(0),
            key_length: (organization == DatasetOrganization::KeySequenced).then_some(8),
            ccsid: Some(1047),
        }
    }

    #[test]
    fn compatibility_definition_is_valid_for_every_data_organization() {
        for organization in [
            DatasetOrganization::Sequential,
            DatasetOrganization::Partitioned,
            DatasetOrganization::PartitionedExtended,
            DatasetOrganization::KeySequenced,
            DatasetOrganization::EntrySequenced,
            DatasetOrganization::Relative,
            DatasetOrganization::VariableRelative,
            DatasetOrganization::Linear,
        ] {
            let mut definition = DatasetDefinition::compatibility(attributes(organization));
            if organization == DatasetOrganization::VariableRelative {
                definition.attributes.record_format = RecordFormat::Variable;
            }
            assert_eq!(
                definition.validate(
                    HostLimits::default(),
                    DatasetProviderCapabilities::deterministic_abstract()
                ),
                Ok(())
            );
        }
    }

    #[test]
    fn physical_operands_fail_with_an_explicit_capability() {
        let mut definition =
            DatasetDefinition::compatibility(attributes(DatasetOrganization::Sequential));
        definition.volumes.kind = VolumeKind::Tape;
        assert_eq!(
            definition.validate(
                HostLimits::default(),
                DatasetProviderCapabilities::deterministic_abstract()
            ),
            Err(HostProblem::UnsupportedCapability {
                capability: "tape".into(),
                detail: "VOLUME/UNIT requires provider capability tape".into(),
            })
        );
    }

    #[test]
    fn every_deferred_operand_family_fails_explicitly() {
        let capabilities = DatasetProviderCapabilities::deterministic_abstract();
        let limits = HostLimits::default();
        macro_rules! rejected {
            ($definition:expr, $capability:literal) => {
                assert!(matches!(
                    $definition.validate(limits, capabilities),
                    Err(HostProblem::UnsupportedCapability { ref capability, .. })
                        if capability == $capability
                ));
            };
        }

        let mut definition =
            DatasetDefinition::compatibility(attributes(DatasetOrganization::Sequential));
        definition.allocation.secondary = 1;
        rejected!(definition, "allocation-extents");

        let mut definition =
            DatasetDefinition::compatibility(attributes(DatasetOrganization::Sequential));
        definition.dcb.buffer_count = 6;
        rejected!(definition, "buffering");

        let mut definition =
            DatasetDefinition::compatibility(attributes(DatasetOrganization::Sequential));
        definition.catalog.owner = Some("IBMUSER".into());
        rejected!(definition, "catalog-metadata");

        let mut definition =
            DatasetDefinition::compatibility(attributes(DatasetOrganization::KeySequenced));
        definition.vsam.control_interval_size = Some(4096);
        assert_eq!(definition.validate(limits, capabilities), Ok(()));

        let mut definition =
            DatasetDefinition::compatibility(attributes(DatasetOrganization::Sequential));
        definition.sms.extended_format = true;
        rejected!(definition, "extended-format");

        let mut definition =
            DatasetDefinition::compatibility(attributes(DatasetOrganization::Sequential));
        definition.sms.data_class = Some("STANDARD".into());
        rejected!(definition, "sms-classes");

        let mut definition =
            DatasetDefinition::compatibility(attributes(DatasetOrganization::KeySequenced));
        definition.vsam.share_options.cross_region = 2;
        rejected!(definition, "sharing");

        let mut definition =
            DatasetDefinition::compatibility(attributes(DatasetOrganization::KeySequenced));
        definition.vsam.reuse = true;
        rejected!(definition, "vsam-data-options");

        let mut definition =
            DatasetDefinition::compatibility(attributes(DatasetOrganization::Sequential));
        definition.volumes.device_type = Some("3390".into());
        rejected!(definition, "physical-volumes");

        let mut definition =
            DatasetDefinition::compatibility(attributes(DatasetOrganization::Sequential));
        definition.volumes.kind = VolumeKind::Tape;
        rejected!(definition, "tape");

        let mut definition =
            DatasetDefinition::compatibility(attributes(DatasetOrganization::Sequential));
        definition.sms.acs_routine = Some("STANDARD".into());
        rejected!(definition, "sms-acs");

        let mut definition =
            DatasetDefinition::compatibility(attributes(DatasetOrganization::Sequential));
        definition.security.encryption_key_label = Some("KEY.ONE".into());
        rejected!(definition, "encryption");

        let mut definition =
            DatasetDefinition::compatibility(attributes(DatasetOrganization::Sequential));
        definition.security.compression = CompressionMode::Generic;
        rejected!(definition, "compression");

        let mut definition =
            DatasetDefinition::compatibility(attributes(DatasetOrganization::KeySequenced));
        definition.vsam.stripe_count = 2;
        rejected!(definition, "striping");

        let mut definition =
            DatasetDefinition::compatibility(attributes(DatasetOrganization::Sequential));
        definition.lifecycle.state = DatasetLifecycleState::Migrated;
        definition.lifecycle.migration_level = 1;
        rejected!(definition, "migration-recall");

        let mut definition =
            DatasetDefinition::compatibility(attributes(DatasetOrganization::KeySequenced));
        definition.vsam.access_mode = VsamAccessMode::Rls;
        rejected!(definition, "rls");

        let mut definition =
            DatasetDefinition::compatibility(attributes(DatasetOrganization::KeySequenced));
        definition.vsam.access_mode = VsamAccessMode::Tvs;
        rejected!(definition, "tvs");
    }

    #[test]
    fn catalog_dates_validate_real_julian_leap_year_boundaries() {
        let mut definition =
            DatasetDefinition::compatibility(attributes(DatasetOrganization::Sequential));
        definition.catalog.creation_date = Some(2_024_366);
        assert!(
            definition
                .validate(
                    HostLimits::default(),
                    DatasetProviderCapabilities::all_contract_capabilities(),
                )
                .is_ok()
        );
        definition.catalog.creation_date = Some(2_025_366);
        assert_eq!(
            definition.validate(
                HostLimits::default(),
                DatasetProviderCapabilities::all_contract_capabilities(),
            ),
            Err(HostProblem::Malformed)
        );
    }
}
