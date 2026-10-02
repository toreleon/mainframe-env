use crate::names::{DatasetName, MemberName};
use crate::request::{
    DatasetAttributes, DatasetOrganization, HostLimits, HostProblem, RecordFormat,
};
use mainframe_env_execution_api::PrincipalId;
use serde::{Deserialize, Serialize};

/// Version identity for owned dataset definitions, independent of provider implementation.
pub const DATASET_DEFINITION_CONTRACT: &str = "mainframe-env.dataset-definition@1";
/// Version identity for the bounded typed dataset request surface.
pub const DATASET_REQUEST_CONTRACT: &str = "mainframe-env.host.dataset-request@2";
/// Version identity for the bounded typed dataset result surface.
pub const DATASET_RESULT_CONTRACT: &str = "mainframe-env.host.dataset-result@2";
/// Version identity for explicit dataset provider capability declarations.
pub const DATASET_PROVIDER_CAPABILITY_CONTRACT: &str =
    "mainframe-env.dataset-provider-capabilities@1";
/// Current dataset state writer generation; retained readers belong to the provider owner.
pub const DATASET_STATE_SCHEMA_VERSION: u16 = 6;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
/// Requested allocation quantity unit; physical realization requires an admitted provider capability.
pub enum SpaceUnit {
    /// Allocation quantity expressed as requested tracks.
    Tracks,
    /// Allocation quantity expressed as requested cylinders.
    Cylinders,
    /// Allocation quantity expressed as requested blocks.
    Blocks,
    /// Allocation quantity expressed in the provider's kilobyte unit.
    Kilobytes,
    /// Allocation quantity expressed in the provider's megabyte unit.
    Megabytes,
    /// Allocation quantity expressed as logical record count.
    Records,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
/// Owned allocation operands; extended requests require explicit allocation capability support.
pub struct AllocationSpace {
    /// Unit applied to primary and secondary quantities; default Records.
    pub unit: SpaceUnit,
    /// Positive initial allocation quantity in unit; default one record.
    pub primary: u64,
    /// Additional extent quantity in unit; zero by default.
    pub secondary: u64,
    /// Directory block count; nonzero exactly for partitioned organizations.
    pub directory_blocks: u32,
    /// Request release of unused allocation; false by default.
    pub release_unused: bool,
    /// Request contiguous placement; false by default and capability-gated.
    pub contiguous: bool,
    /// Request cylinder rounding; false by default and capability-gated.
    pub round_to_cylinder: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
/// Owned record-buffer geometry; validation distinguishes defaults from capability-gated operands.
pub struct DcbOptions {
    /// Requested block size in bytes; zero leaves size unspecified.
    pub block_size: u32,
    /// Positive requested buffer count; default five.
    pub buffer_count: u16,
    /// Optional positive buffer capacity in bytes, bounded by max_record_bytes.
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
/// Requested placement category; a descriptor does not provide physical disk or tape execution.
pub enum VolumeKind {
    /// Deterministic abstract placement category used by the default definition.
    Abstract,
    /// Physical-disk placement request requiring provider support.
    PhysicalDisk,
    /// Tape placement request requiring provider support.
    Tape,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
/// Nonempty bounded volume selection with explicit placement category and optional device label.
pub struct VolumeSelection {
    /// Requested placement category; default Abstract.
    pub kind: VolumeKind,
    /// Distinct nonempty bounded volume labels; default contains MENV00.
    pub volume_ids: Vec<String>,
    /// Optional bounded device label; absence requests no device-specific identity.
    pub device_type: Option<String>,
    /// Positive requested unit count; default one.
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
/// Optional storage-management operands; absent labels and false flags are the default.
pub struct SmsClasses {
    /// Optional bounded data-class label, requiring SMS-class support.
    pub data_class: Option<String>,
    /// Optional bounded management-class label, requiring SMS-class support.
    pub management_class: Option<String>,
    /// Optional bounded storage-class label, requiring SMS-class support.
    pub storage_class: Option<String>,
    /// Optional bounded ACS routine label, requiring ACS support.
    pub acs_routine: Option<String>,
    /// Request guaranteed space through the SMS-class capability.
    pub guaranteed_space: bool,
    /// Request extended format through the extended-format capability.
    pub extended_format: bool,
    /// Request extended addressability through the extended-format capability.
    pub extended_addressable: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
/// Requested compression category; nondefault modes require explicit provider support.
pub enum CompressionMode {
    /// No compression requested; the default.
    None,
    /// Generic compression requested through the compression capability.
    Generic,
    /// Tailored compression requested through the compression capability.
    Tailored,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
/// Requested buffering category, retained as owned operands rather than a physical buffering guarantee.
pub enum BufferingMode {
    /// Default system buffering request.
    System,
    /// Nonshared buffering request requiring buffering support.
    NonsharedResources,
    /// Local shared buffering request requiring buffering support.
    LocalSharedResources,
    /// Global shared buffering request requiring buffering support.
    GlobalSharedResources,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
/// Access-mode selector whose nondefault modes require provider capabilities.
pub enum VsamAccessMode {
    /// Default non-RLS access request.
    NonRls,
    /// RLS access request requiring the rls capability.
    Rls,
    /// Transactional access request, admitted by definition validation only for keyed datasets.
    Tvs,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Typed requested lock mode; shared lock authority owns actual acquisition and fencing.
pub enum DatasetLockMode {
    /// Request a shared lock on the declared target.
    Shared,
    /// Request an update lock on the declared target.
    Update,
    /// Request an exclusive lock on the declared target.
    Exclusive,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Dataset-wide or exact-record lock identity.
pub enum DatasetLockTarget {
    /// Lock scope covers the named dataset.
    Dataset,
    /// Lock scope identifies one record by nonempty bounded opaque identity bytes.
    Record(Vec<u8>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Owned lock observation carrying owner, expiry and version; it is not reusable authorization.
pub struct DatasetLockReceipt {
    /// Provider-issued lock identity for release and observation.
    pub lock_id: String,
    /// Dataset resource owning the lock.
    pub dataset: DatasetName,
    /// Dataset-wide or record identity protected by the lock.
    pub target: DatasetLockTarget,
    /// Principal identity owning the lock.
    pub owner: PrincipalId,
    /// Granted lock-mode observation.
    pub mode: DatasetLockMode,
    /// Expiry on the provider's host logical-tick timeline.
    pub expires_at: u64,
    /// Optional transaction binding; None denotes no declared transaction association.
    pub transaction: Option<String>,
    /// Observed lock-state version used by the provider's fencing contract.
    pub version: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Bounded staged record mutation for the existing dataset transactional route.
pub enum TvsRecordOperation {
    /// Stage insertion of one nonempty record.
    Insert {
        /// Dataset resource affected by the staged operation.
        dataset: DatasetName,
        /// Nonempty record bytes bounded by max_record_bytes.
        record: Vec<u8>,
    },
    /// Stage replacement of a record identified by its key.
    Rewrite {
        /// Dataset resource affected by the staged operation.
        dataset: DatasetName,
        /// Nonempty exact key bytes bounded by max_record_bytes.
        key: Vec<u8>,
        /// Nonempty record bytes bounded by max_record_bytes.
        record: Vec<u8>,
    },
    /// Stage deletion of a record identified by its key.
    Delete {
        /// Dataset resource affected by the staged operation.
        dataset: DatasetName,
        /// Nonempty exact key bytes bounded by max_record_bytes.
        key: Vec<u8>,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Explicit transactional dataset outcome; unresolved completion remains Unknown.
pub enum TvsUnitOfWorkState {
    /// The unit of work remains active with staged operations.
    Active,
    /// The unit of work reports committed completion.
    Committed,
    /// The unit of work reports rolled-back completion.
    RolledBack,
    /// The authoritative completion outcome is unresolved.
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Owned transaction-state observation without a universal exactly-once claim.
pub struct TvsUnitOfWorkReceipt {
    /// Bounded transaction identity observed by the provider.
    pub transaction: String,
    /// Principal owning the unit of work.
    pub owner: PrincipalId,
    /// Active or explicit terminal/unknown state.
    pub state: TvsUnitOfWorkState,
    /// Number of staged record operations.
    pub staged_operations: u32,
    /// Observed unit-of-work state version.
    pub version: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
/// Declared cross-region/system sharing operands checked for ranges and provider support.
pub struct DatasetShareOptions {
    /// Requested cross-region class, validated in 1 through 4; default 1.
    pub cross_region: u8,
    /// Requested cross-system class, validated in 3 through 4; default 3.
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
/// Owned VSAM geometry and access operands; nondefault options require declared capabilities.
pub struct VsamAttributes {
    /// Optional interval size in bytes: 512 through 32,768 in 512-byte multiples.
    pub control_interval_size: Option<u32>,
    /// Optional positive area size in bytes, a multiple of the interval size.
    pub control_area_size: Option<u64>,
    /// Requested sharing classes; default cross-region 1 and cross-system 3.
    pub share_options: DatasetShareOptions,
    /// Requested access mode; default NonRls.
    pub access_mode: VsamAccessMode,
    /// Whether the record format is spanned; must agree with the selected format.
    pub spanned: bool,
    /// Request reusable storage through the VSAM-data-options capability.
    pub reuse: bool,
    /// Request the speed option through the VSAM-data-options capability.
    pub speed: bool,
    /// Request write checking through the VSAM-data-options capability.
    pub write_check: bool,
    /// Request erase-on-delete through the VSAM-data-options capability.
    pub erase_on_delete: bool,
    /// Requested buffering category; default System.
    pub buffering: BufferingMode,
    /// Positive requested stripe count; default one, larger values require striping support.
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
/// Owned encryption/compression requests, rejected when provider capabilities are absent.
pub struct DataSecurity {
    /// Optional bounded key label, requiring encryption support; never key material.
    pub encryption_key_label: Option<String>,
    /// Requested compression mode; default None.
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
/// Typed catalog-resource category; recognition alone does not implement every resource kind.
pub enum CatalogEntryKind {
    /// Ordinary dataset catalog entry.
    Dataset,
    /// Alternate-index catalog entry.
    AlternateIndex,
    /// Index-path catalog entry.
    Path,
    /// Name-alias catalog entry.
    Alias,
    /// Generation-group catalog entry.
    GenerationDataGroup,
    /// User-catalog entry.
    UserCatalog,
    /// Master-catalog entry.
    MasterCatalog,
    /// Library catalog entry.
    Library,
    /// Volume catalog entry.
    Volume,
    /// Page-space catalog entry.
    PageSpace,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Catalog routing category used by the typed define-catalog request.
pub enum CatalogKind {
    /// Master-catalog routing identity.
    Master,
    /// User-catalog routing identity.
    User,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Owned name-resolution observation with the traversed alias chain.
pub struct CatalogResolution {
    /// Original dataset name supplied for resolution.
    pub requested: DatasetName,
    /// Final resource name after catalog/alias resolution.
    pub resolved: DatasetName,
    /// Optional selected catalog identity; absent when no catalog identity is returned.
    pub catalog: Option<DatasetName>,
    /// Alias names traversed in resolution order.
    pub alias_chain: Vec<DatasetName>,
    /// Observed resolution-state version.
    pub version: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// One catalog listing observation and its optional related resource.
pub struct CatalogListEntry {
    /// Cataloged resource name.
    pub name: DatasetName,
    /// Typed category of this entry.
    pub kind: CatalogEntryKind,
    /// Optional related dataset/index/alias target identity.
    pub related: Option<DatasetName>,
    /// Observed entry version.
    pub version: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
/// Optional catalog ownership/date/retention operands validated before provider admission.
pub struct CatalogMetadata {
    /// Requested entry category; default Dataset.
    pub entry_kind: CatalogEntryKind,
    /// Optional routing catalog name; absent leaves routing to the existing authority.
    pub catalog: Option<DatasetName>,
    /// Optional bounded owner label, requiring catalog-metadata support.
    pub owner: Option<String>,
    /// Optional validated YYYYDDD creation date; absent records no date.
    pub creation_date: Option<u32>,
    /// Optional validated YYYYDDD expiry date; mutually exclusive with retention_days.
    pub expiration_date: Option<u32>,
    /// Optional retention duration in days; mutually exclusive with expiration_date.
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
/// Owned lifecycle vocabulary; migration and recall requests require explicit provider support.
pub enum DatasetLifecycleState {
    /// Resource is described as allocated.
    Allocated,
    /// Resource is described as cataloged; the default metadata state.
    Cataloged,
    /// Resource is described as open.
    Open,
    /// Resource is described as closed.
    Closed,
    /// Resource is described as migrated, subject to migration/recall capability.
    Migrated,
    /// Recall is described as pending, subject to migration/recall capability.
    RecallPending,
    /// Resource requires recovery rather than ordinary successful use.
    RecoveryRequired,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
/// Declared lifecycle state and retained migration/backup markers.
pub struct LifecycleMetadata {
    /// Declared lifecycle state; default Cataloged.
    pub state: DatasetLifecycleState,
    /// Migration marker, zero by default; nonzero requires migration/recall support.
    pub migration_level: u8,
    /// Retained backup-generation marker; zero by default.
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
/// Explicit provider support flags; capability presence is not execution or certification evidence.
pub struct DatasetProviderCapabilities {
    /// Capability descriptor generation; constructors emit version one.
    pub schema_version: u16,
    /// Whether deterministic abstract placement is supported.
    pub abstract_volumes: bool,
    /// Whether secondary/release/contiguous/round allocation operands are supported.
    pub allocation_extents: bool,
    /// Whether nondefault buffer geometry and buffering categories are supported.
    pub buffering: bool,
    /// Whether catalog owner, retention and entry-kind operands are supported.
    pub catalog_metadata: bool,
    /// Whether explicit catalog selection is supported.
    pub catalog_routing: bool,
    /// Whether explicit interval/area byte geometry is supported.
    pub control_intervals: bool,
    /// Whether extended-format and extended-addressability requests are supported.
    pub extended_format: bool,
    /// Whether physical-disk placement operands are supported.
    pub physical_volumes: bool,
    /// Whether tape placement operands are supported.
    pub tape: bool,
    /// Whether ACS routine selection is supported.
    pub sms_acs: bool,
    /// Whether encryption key-label requests are supported.
    pub encryption: bool,
    /// Whether nondefault compression requests are supported.
    pub compression: bool,
    /// Whether multiple stripes are supported.
    pub striping: bool,
    /// Whether migration markers and recall lifecycle states are supported.
    pub migration_recall: bool,
    /// Whether RLS access requests are supported.
    pub rls: bool,
    /// Whether nondefault sharing classes are supported.
    pub sharing: bool,
    /// Whether SMS-class labels and guaranteed-space requests are supported.
    pub sms_classes: bool,
    /// Whether transactional VSAM access requests are supported.
    pub tvs: bool,
    /// Whether reuse, speed, write-check and erase operands are supported.
    pub vsam_data_options: bool,
}

impl DatasetProviderCapabilities {
    #[must_use]
    /// Return the conservative abstract profile, leaving deferred capabilities false.
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
    /// Enable all flags for shape validation; this does not declare actual provider support.
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
/// Complete owned dataset operands; validate shape and actual provider capabilities before use.
pub struct DatasetDefinition {
    /// Record organization, format and key-layout attributes.
    pub attributes: DatasetAttributes,
    /// Requested block and buffer geometry.
    pub dcb: DcbOptions,
    /// Allocation quantity, directory and extent operands.
    pub allocation: AllocationSpace,
    /// Placement category and selected volume/device identities.
    pub volumes: VolumeSelection,
    /// Optional storage-management labels and flags.
    pub sms: SmsClasses,
    /// Requested VSAM geometry, sharing and access options.
    pub vsam: VsamAttributes,
    /// Optional key-label and compression operands.
    pub security: DataSecurity,
    /// Catalog routing, ownership and retention operands.
    pub catalog: CatalogMetadata,
    /// Declared lifecycle and retained migration/backup markers.
    pub lifecycle: LifecycleMetadata,
}

impl DatasetDefinition {
    #[must_use]
    /// Use supplied attributes with default operands and one directory block for partitioned datasets.
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

    /// Reject malformed geometry or unsupported operands using the supplied actual capability profile.
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
/// Provider-reported definition, geometry and utilization; abstract placement is explicit.
pub struct DatasetDescription {
    /// Effective owned definition observed by the provider.
    pub definition: DatasetDefinition,
    /// Observed dataset state version.
    pub version: u64,
    /// Reported allocated capacity in bytes.
    pub allocated_bytes: u64,
    /// Reported used capacity in bytes.
    pub used_bytes: u64,
    /// Reported control-interval count.
    pub control_intervals: u64,
    /// Reported control-area count.
    pub control_areas: u64,
    /// Reported high-used relative byte address.
    pub high_used_rba: u64,
    /// Reported maximum relative byte address.
    pub max_rba: u64,
    /// Ordered allocation extent observations.
    pub extents: Vec<DatasetExtent>,
    /// Reported total buffer capacity in bytes.
    pub buffer_bytes: u64,
    /// Explicit abstract placement description, without a physical-device claim.
    pub abstract_placement: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// One owned extent observation with logical and volume-relative coordinates.
pub struct DatasetExtent {
    /// Extent ordering identity supplied by the provider.
    pub ordinal: u32,
    /// Logical byte start within the dataset.
    pub start: u64,
    /// Byte start within the selected volume's modeled placement.
    pub volume_start: u64,
    /// Extent length in bytes.
    pub length: u64,
    /// Volume label owning the extent.
    pub volume_id: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
/// Volume-side extent observation identifying its dataset and logical range.
pub struct DatasetVolumeExtent {
    /// Dataset resource occupying this extent.
    pub dataset: DatasetName,
    /// Ordinal linking the extent to the dataset's extent list.
    pub dataset_extent_ordinal: u32,
    /// Logical byte start within the dataset.
    pub logical_start: u64,
    /// Byte start within the volume's modeled placement.
    pub volume_start: u64,
    /// Extent length in bytes.
    pub length: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
/// Owned modeled volume utilization and extent list.
pub struct DatasetVolumeDescription {
    /// Volume resource label.
    pub volume_id: String,
    /// Reported allocated volume capacity in bytes.
    pub allocated_bytes: u64,
    /// Reported used volume capacity in bytes.
    pub used_bytes: u64,
    /// Dataset extents occupying the modeled volume.
    pub extents: Vec<DatasetVolumeExtent>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
/// Bounded restore image; organization determines which record/member/linear collection may be populated.
pub struct DatasetSnapshot {
    /// Owned dataset definition restored with the data image.
    pub definition: DatasetDefinition,
    /// Ordered sequential/keyed records; empty for member, relative and linear organizations.
    pub records: Vec<Vec<u8>>,
    /// Strictly increasing positive relative-record identities and their bytes.
    pub relative_records: Vec<DatasetRelativeRecordSnapshot>,
    /// Strictly name-ordered library members with bounded generations or aliases.
    pub members: Vec<DatasetMemberSnapshot>,
    /// Linear byte image; empty for record/member organizations.
    pub linear_data: Vec<u8>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
/// One relative record in a restore image.
pub struct DatasetRelativeRecordSnapshot {
    /// Positive relative-record number, strictly increasing in its snapshot collection.
    pub record_number: u64,
    /// Record bytes bounded by max_record_bytes.
    pub record: Vec<u8>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
/// Member restore image; alias entries cannot also carry record or generation data.
pub struct DatasetMemberSnapshot {
    /// Member identity unique in the strictly ordered snapshot list.
    pub name: MemberName,
    /// Ordinary partitioned-member record bytes; empty for extended-library generations.
    pub records: Vec<Vec<u8>>,
    /// Strictly increasing positive extended-library generations.
    pub generations: Vec<DatasetMemberGenerationSnapshot>,
    /// Optional nonalias target member; when present records and generations must be empty.
    pub alias_of: Option<MemberName>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
/// One retained extended-library member generation.
pub struct DatasetMemberGenerationSnapshot {
    /// Positive absolute generation identity within the member snapshot.
    pub generation: u64,
    /// Whether the generation is marked as a program object.
    pub program_object: bool,
    /// Ordered bounded record bytes for this generation.
    pub records: Vec<Vec<u8>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Bounded diagnostic observation tied optionally to a definition operand.
pub struct DatasetDiagnostic {
    /// Machine-readable diagnostic identity supplied by the provider.
    pub code: String,
    /// Optional operand identity; absent for diagnostics applying to the whole definition.
    pub field: Option<String>,
    /// Diagnostic explanation bounded by the host result boundary.
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
