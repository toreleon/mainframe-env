use crate::names::DatasetName;
use crate::request::{
    DatasetAttributes, DatasetOrganization, HostLimits, HostProblem, RecordFormat,
};
use mainframe_env_execution_api::PrincipalId;

pub const DATASET_DEFINITION_CONTRACT: &str = "mainframe-env.dataset-definition@1";
pub const DATASET_REQUEST_CONTRACT: &str = "mainframe-env.host.dataset-request@2";
pub const DATASET_RESULT_CONTRACT: &str = "mainframe-env.host.dataset-result@2";
pub const DATASET_PROVIDER_CAPABILITY_CONTRACT: &str =
    "mainframe-env.dataset-provider-capabilities@1";
pub const DATASET_STATE_SCHEMA_VERSION: u16 = 6;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SpaceUnit {
    Tracks,
    Cylinders,
    Blocks,
    Kilobytes,
    Megabytes,
    Records,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AllocationSpace {
    pub unit: SpaceUnit,
    pub primary: u64,
    pub secondary: u64,
    pub directory_blocks: u32,
    pub release_unused: bool,
    pub contiguous: bool,
    pub round_to_cylinder: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DcbOptions {
    pub block_size: u32,
    pub buffer_count: u16,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VolumeKind {
    Abstract,
    PhysicalDisk,
    Tape,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VolumeSelection {
    pub kind: VolumeKind,
    pub volume_ids: Vec<String>,
    pub device_type: Option<String>,
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

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SmsClasses {
    pub data_class: Option<String>,
    pub management_class: Option<String>,
    pub storage_class: Option<String>,
    pub acs_routine: Option<String>,
    pub guaranteed_space: bool,
    pub extended_format: bool,
    pub extended_addressable: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompressionMode {
    None,
    Generic,
    Tailored,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BufferingMode {
    System,
    NonsharedResources,
    LocalSharedResources,
    GlobalSharedResources,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VsamAccessMode {
    NonRls,
    Rls,
    Tvs,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DatasetLockMode {
    Shared,
    Update,
    Exclusive,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DatasetLockTarget {
    Dataset,
    Record(Vec<u8>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DatasetLockReceipt {
    pub lock_id: String,
    pub dataset: DatasetName,
    pub target: DatasetLockTarget,
    pub owner: PrincipalId,
    pub mode: DatasetLockMode,
    pub expires_at: u64,
    pub transaction: Option<String>,
    pub version: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TvsRecordOperation {
    Insert {
        dataset: DatasetName,
        record: Vec<u8>,
    },
    Rewrite {
        dataset: DatasetName,
        key: Vec<u8>,
        record: Vec<u8>,
    },
    Delete {
        dataset: DatasetName,
        key: Vec<u8>,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TvsUnitOfWorkState {
    Active,
    Committed,
    RolledBack,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TvsUnitOfWorkReceipt {
    pub transaction: String,
    pub owner: PrincipalId,
    pub state: TvsUnitOfWorkState,
    pub staged_operations: u32,
    pub version: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DatasetShareOptions {
    pub cross_region: u8,
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VsamAttributes {
    pub control_interval_size: Option<u32>,
    pub control_area_size: Option<u64>,
    pub share_options: DatasetShareOptions,
    pub access_mode: VsamAccessMode,
    pub spanned: bool,
    pub reuse: bool,
    pub speed: bool,
    pub write_check: bool,
    pub erase_on_delete: bool,
    pub buffering: BufferingMode,
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DataSecurity {
    pub encryption_key_label: Option<String>,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CatalogEntryKind {
    Dataset,
    AlternateIndex,
    Path,
    Alias,
    GenerationDataGroup,
    UserCatalog,
    MasterCatalog,
    Library,
    Volume,
    PageSpace,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CatalogKind {
    Master,
    User,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CatalogResolution {
    pub requested: DatasetName,
    pub resolved: DatasetName,
    pub catalog: Option<DatasetName>,
    pub alias_chain: Vec<DatasetName>,
    pub version: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CatalogMetadata {
    pub entry_kind: CatalogEntryKind,
    pub catalog: Option<DatasetName>,
    pub owner: Option<String>,
    pub creation_date: Option<u32>,
    pub expiration_date: Option<u32>,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DatasetLifecycleState {
    Allocated,
    Cataloged,
    Open,
    Closed,
    Migrated,
    RecallPending,
    RecoveryRequired,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LifecycleMetadata {
    pub state: DatasetLifecycleState,
    pub migration_level: u8,
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
pub struct DatasetProviderCapabilities {
    pub schema_version: u16,
    pub abstract_volumes: bool,
    pub allocation_extents: bool,
    pub buffering: bool,
    pub catalog_metadata: bool,
    pub catalog_routing: bool,
    pub control_intervals: bool,
    pub extended_format: bool,
    pub physical_volumes: bool,
    pub tape: bool,
    pub sms_acs: bool,
    pub encryption: bool,
    pub compression: bool,
    pub striping: bool,
    pub migration_recall: bool,
    pub rls: bool,
    pub sharing: bool,
    pub sms_classes: bool,
    pub tvs: bool,
    pub vsam_data_options: bool,
}

impl DatasetProviderCapabilities {
    #[must_use]
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DatasetDefinition {
    pub attributes: DatasetAttributes,
    pub dcb: DcbOptions,
    pub allocation: AllocationSpace,
    pub volumes: VolumeSelection,
    pub sms: SmsClasses,
    pub vsam: VsamAttributes,
    pub security: DataSecurity,
    pub catalog: CatalogMetadata,
    pub lifecycle: LifecycleMetadata,
}

impl DatasetDefinition {
    #[must_use]
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
            || self.volumes.unit_count == 0
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
            if !(1900001..=9999366).contains(&date) || date % 1000 == 0 || date % 1000 > 366 {
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
pub struct DatasetDescription {
    pub definition: DatasetDefinition,
    pub version: u64,
    pub allocated_bytes: u64,
    pub used_bytes: u64,
    pub control_intervals: u64,
    pub control_areas: u64,
    pub high_used_rba: u64,
    pub max_rba: u64,
    pub extents: Vec<DatasetExtent>,
    pub buffer_bytes: u64,
    pub abstract_placement: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DatasetExtent {
    pub ordinal: u32,
    pub start: u64,
    pub length: u64,
    pub volume_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DatasetDiagnostic {
    pub code: String,
    pub field: Option<String>,
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
    }
}
