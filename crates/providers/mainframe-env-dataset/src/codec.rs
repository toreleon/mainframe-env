use mainframe_env_host_api::{
    AllocationSpace, BufferingMode, CatalogEntryKind, CatalogMetadata, CompressionMode,
    DataSecurity, DatasetAttributes, DatasetDefinition, DatasetLifecycleState, DatasetName,
    DatasetOrganization, DatasetShareOptions, DcbOptions, LifecycleMetadata, RecordFormat,
    SmsClasses, SpaceUnit, VolumeKind, VolumeSelection, VsamAccessMode, VsamAttributes,
};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Entry {
    pub attributes: DatasetAttributes,
    pub dcb: DcbOptions,
    pub allocation: AllocationSpace,
    pub volumes: VolumeSelection,
    pub sms: SmsClasses,
    pub vsam: VsamAttributes,
    pub security: DataSecurity,
    pub catalog: CatalogMetadata,
    pub lifecycle: LifecycleMetadata,
    pub version: u64,
    pub records: Vec<Vec<u8>>,
    pub members: BTreeMap<String, Vec<Vec<u8>>>,
    pub relative_records: BTreeMap<u64, Vec<u8>>,
    pub member_generations: BTreeMap<String, Vec<MemberGeneration>>,
    pub member_aliases: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct MemberGeneration {
    pub generation: u64,
    pub program_object: bool,
    pub records: Vec<Vec<u8>>,
}

impl Entry {
    pub(crate) fn from_definition(definition: DatasetDefinition, version: u64) -> Self {
        Self {
            attributes: definition.attributes,
            dcb: definition.dcb,
            allocation: definition.allocation,
            volumes: definition.volumes,
            sms: definition.sms,
            vsam: definition.vsam,
            security: definition.security,
            catalog: definition.catalog,
            lifecycle: definition.lifecycle,
            version,
            records: Vec::new(),
            members: BTreeMap::new(),
            relative_records: BTreeMap::new(),
            member_generations: BTreeMap::new(),
            member_aliases: BTreeMap::new(),
        }
    }

    pub(crate) fn definition(&self) -> DatasetDefinition {
        DatasetDefinition {
            attributes: self.attributes.clone(),
            dcb: self.dcb.clone(),
            allocation: self.allocation.clone(),
            volumes: self.volumes.clone(),
            sms: self.sms.clone(),
            vsam: self.vsam.clone(),
            security: self.security.clone(),
            catalog: self.catalog.clone(),
            lifecycle: self.lifecycle.clone(),
        }
    }

    pub(crate) fn replace_definition(&mut self, definition: DatasetDefinition) {
        self.attributes = definition.attributes;
        self.dcb = definition.dcb;
        self.allocation = definition.allocation;
        self.volumes = definition.volumes;
        self.sms = definition.sms;
        self.vsam = definition.vsam;
        self.security = definition.security;
        self.catalog = definition.catalog;
        self.lifecycle = definition.lifecycle;
    }
}

pub(crate) fn encode(entry: &Entry) -> Result<Vec<u8>, ()> {
    encode_state(entry, b"MEDS6", true, true)
}

fn encode_state(
    entry: &Entry,
    schema: &[u8; 5],
    include_access_mode: bool,
    include_creation_date: bool,
) -> Result<Vec<u8>, ()> {
    let mut out = schema.to_vec();
    out.push(org(entry.attributes.organization));
    out.push(recfm(entry.attributes.record_format));
    u32v(&mut out, entry.attributes.logical_record_length);
    optional_u32(&mut out, entry.attributes.key_offset);
    optional_u32(&mut out, entry.attributes.key_length);
    optional_u16(&mut out, entry.attributes.ccsid);
    u64v(&mut out, entry.version);
    encode_metadata(&mut out, entry, include_access_mode, include_creation_date)?;
    records(&mut out, &entry.records)?;
    u32v(
        &mut out,
        u32::try_from(entry.members.len()).map_err(|_| ())?,
    );
    for (name, value) in &entry.members {
        bytes(&mut out, name.as_bytes())?;
        records(&mut out, value)?;
    }
    u32v(
        &mut out,
        u32::try_from(entry.relative_records.len()).map_err(|_| ())?,
    );
    for (number, value) in &entry.relative_records {
        u64v(&mut out, *number);
        bytes(&mut out, value)?;
    }
    u32v(
        &mut out,
        u32::try_from(entry.member_generations.len()).map_err(|_| ())?,
    );
    for (member, generations) in &entry.member_generations {
        bytes(&mut out, member.as_bytes())?;
        u32v(&mut out, u32::try_from(generations.len()).map_err(|_| ())?);
        for generation in generations {
            u64v(&mut out, generation.generation);
            boolv(&mut out, generation.program_object);
            records(&mut out, &generation.records)?;
        }
    }
    u32v(
        &mut out,
        u32::try_from(entry.member_aliases.len()).map_err(|_| ())?,
    );
    for (alias, target) in &entry.member_aliases {
        bytes(&mut out, alias.as_bytes())?;
        bytes(&mut out, target.as_bytes())?;
    }
    Ok(out)
}

pub(crate) fn encode_definition_digest_v2(definition: &DatasetDefinition) -> Result<Vec<u8>, ()> {
    let entry = Entry::from_definition(definition.clone(), 0);
    let mut out = b"MEDS3".to_vec();
    out.push(org(entry.attributes.organization));
    out.push(recfm(entry.attributes.record_format));
    u32v(&mut out, entry.attributes.logical_record_length);
    optional_u32(&mut out, entry.attributes.key_offset);
    optional_u32(&mut out, entry.attributes.key_length);
    optional_u16(&mut out, entry.attributes.ccsid);
    u64v(&mut out, 0);
    encode_metadata(&mut out, &entry, false, false)?;
    records(&mut out, &[])?;
    u32v(&mut out, 0);
    u32v(&mut out, 0);
    Ok(out)
}

pub(crate) fn encode_definition_digest_v3(definition: &DatasetDefinition) -> Result<Vec<u8>, ()> {
    let mut out = encode_definition_digest_v2(definition)?;
    if let Some(creation_date) = definition.catalog.creation_date {
        out.extend_from_slice(b"MECRD1");
        u32v(&mut out, creation_date);
    }
    Ok(out)
}
pub(crate) fn decode(
    bytes_in: &[u8],
    max_records: usize,
    max_record: usize,
    max_members: usize,
) -> Result<Entry, ()> {
    let mut r = Reader {
        bytes: bytes_in,
        at: 0,
    };
    let schema = r.take(5)?;
    if !matches!(
        schema,
        b"MEDS1" | b"MEDS2" | b"MEDS3" | b"MEDS4" | b"MEDS5" | b"MEDS6"
    ) {
        return Err(());
    }
    let organization = org_back(r.byte()?)?;
    let record_format = recfm_back(r.byte()?)?;
    let logical_record_length = r.u32()?;
    let key_offset = r.optional_u32()?;
    let key_length = r.optional_u32()?;
    let ccsid = r.optional_u16()?;
    let version = r.u64()?;
    let mut definition = DatasetDefinition::compatibility(DatasetAttributes {
        organization,
        record_format,
        logical_record_length,
        key_offset,
        key_length,
        ccsid,
    });
    if matches!(schema, b"MEDS3" | b"MEDS4" | b"MEDS5" | b"MEDS6") {
        decode_metadata(
            &mut r,
            &mut definition,
            max_records,
            matches!(schema, b"MEDS5" | b"MEDS6"),
            schema == b"MEDS6",
        )?;
    }
    let records = r.records(max_records, max_record)?;
    let count = usize::try_from(r.u32()?).map_err(|_| ())?;
    if count > max_members {
        return Err(());
    }
    let mut members = BTreeMap::new();
    for _ in 0..count {
        let name = String::from_utf8(r.bytes(128)?).map_err(|_| ())?;
        if members
            .insert(name, r.records(max_records, max_record)?)
            .is_some()
        {
            return Err(());
        }
    }
    let mut relative_records = BTreeMap::new();
    if matches!(schema, b"MEDS2" | b"MEDS3" | b"MEDS4" | b"MEDS5" | b"MEDS6") {
        let count = usize::try_from(r.u32()?).map_err(|_| ())?;
        if count > max_records {
            return Err(());
        }
        for _ in 0..count {
            let number = r.u64()?;
            if number == 0
                || relative_records
                    .insert(number, r.bytes(max_record)?)
                    .is_some()
            {
                return Err(());
            }
        }
    }
    if !matches!(schema, b"MEDS4" | b"MEDS5" | b"MEDS6") && r.at != bytes_in.len() {
        return Err(());
    }
    let mut entry = Entry::from_definition(definition, version);
    entry.records = records;
    entry.members = members;
    entry.relative_records = relative_records;
    if matches!(schema, b"MEDS4" | b"MEDS5" | b"MEDS6") {
        let member_count = usize::try_from(r.u32()?).map_err(|_| ())?;
        if member_count > max_members {
            return Err(());
        }
        for _ in 0..member_count {
            let member = r.string(128)?;
            let generation_count = usize::try_from(r.u32()?).map_err(|_| ())?;
            if generation_count == 0 || generation_count > max_records {
                return Err(());
            }
            let mut generations = Vec::with_capacity(generation_count);
            let mut previous = 0u64;
            for _ in 0..generation_count {
                let generation = r.u64()?;
                if generation == 0 || generation <= previous {
                    return Err(());
                }
                previous = generation;
                generations.push(MemberGeneration {
                    generation,
                    program_object: r.bool()?,
                    records: r.records(max_records, max_record)?,
                });
            }
            if entry
                .member_generations
                .insert(member, generations)
                .is_some()
            {
                return Err(());
            }
        }
        let alias_count = usize::try_from(r.u32()?).map_err(|_| ())?;
        if alias_count > max_members {
            return Err(());
        }
        for _ in 0..alias_count {
            let alias = r.string(128)?;
            let target = r.string(128)?;
            if entry.member_aliases.insert(alias, target).is_some() {
                return Err(());
            }
        }
    }
    if r.at != bytes_in.len() {
        return Err(());
    }
    Ok(entry)
}
fn org(value: DatasetOrganization) -> u8 {
    match value {
        DatasetOrganization::Sequential => 0,
        DatasetOrganization::Partitioned => 1,
        DatasetOrganization::PartitionedExtended => 5,
        DatasetOrganization::KeySequenced => 2,
        DatasetOrganization::EntrySequenced => 3,
        DatasetOrganization::Relative => 4,
        DatasetOrganization::VariableRelative => 6,
        DatasetOrganization::Linear => 7,
    }
}
fn org_back(value: u8) -> Result<DatasetOrganization, ()> {
    match value {
        0 => Ok(DatasetOrganization::Sequential),
        1 => Ok(DatasetOrganization::Partitioned),
        2 => Ok(DatasetOrganization::KeySequenced),
        3 => Ok(DatasetOrganization::EntrySequenced),
        4 => Ok(DatasetOrganization::Relative),
        5 => Ok(DatasetOrganization::PartitionedExtended),
        6 => Ok(DatasetOrganization::VariableRelative),
        7 => Ok(DatasetOrganization::Linear),
        _ => Err(()),
    }
}
fn recfm(value: RecordFormat) -> u8 {
    match value {
        RecordFormat::Fixed => 0,
        RecordFormat::FixedBlocked => 1,
        RecordFormat::FixedBlockedStandard => 6,
        RecordFormat::Variable => 2,
        RecordFormat::VariableBlocked => 3,
        RecordFormat::VariableSpanned => 7,
        RecordFormat::VariableBlockedSpanned => 8,
        RecordFormat::Undefined => 4,
        RecordFormat::Line => 5,
    }
}
fn recfm_back(value: u8) -> Result<RecordFormat, ()> {
    match value {
        0 => Ok(RecordFormat::Fixed),
        1 => Ok(RecordFormat::FixedBlocked),
        2 => Ok(RecordFormat::Variable),
        3 => Ok(RecordFormat::VariableBlocked),
        4 => Ok(RecordFormat::Undefined),
        5 => Ok(RecordFormat::Line),
        6 => Ok(RecordFormat::FixedBlockedStandard),
        7 => Ok(RecordFormat::VariableSpanned),
        8 => Ok(RecordFormat::VariableBlockedSpanned),
        _ => Err(()),
    }
}

fn encode_metadata(
    out: &mut Vec<u8>,
    entry: &Entry,
    include_access_mode: bool,
    include_creation_date: bool,
) -> Result<(), ()> {
    u32v(out, entry.dcb.block_size);
    u16v(out, entry.dcb.buffer_count);
    optional_u32(out, entry.dcb.buffer_size);
    out.push(space_unit(entry.allocation.unit));
    u64v(out, entry.allocation.primary);
    u64v(out, entry.allocation.secondary);
    u32v(out, entry.allocation.directory_blocks);
    boolv(out, entry.allocation.release_unused);
    boolv(out, entry.allocation.contiguous);
    boolv(out, entry.allocation.round_to_cylinder);

    out.push(volume_kind(entry.volumes.kind));
    u32v(
        out,
        u32::try_from(entry.volumes.volume_ids.len()).map_err(|_| ())?,
    );
    for volume in &entry.volumes.volume_ids {
        bytes(out, volume.as_bytes())?;
    }
    optional_string(out, entry.volumes.device_type.as_deref())?;
    u16v(out, entry.volumes.unit_count);

    optional_string(out, entry.sms.data_class.as_deref())?;
    optional_string(out, entry.sms.management_class.as_deref())?;
    optional_string(out, entry.sms.storage_class.as_deref())?;
    optional_string(out, entry.sms.acs_routine.as_deref())?;
    boolv(out, entry.sms.guaranteed_space);
    boolv(out, entry.sms.extended_format);
    boolv(out, entry.sms.extended_addressable);

    optional_u32(out, entry.vsam.control_interval_size);
    optional_u64(out, entry.vsam.control_area_size);
    out.push(entry.vsam.share_options.cross_region);
    out.push(entry.vsam.share_options.cross_system);
    if include_access_mode {
        out.push(vsam_access_mode(entry.vsam.access_mode));
    }
    boolv(out, entry.vsam.spanned);
    boolv(out, entry.vsam.reuse);
    boolv(out, entry.vsam.speed);
    boolv(out, entry.vsam.write_check);
    boolv(out, entry.vsam.erase_on_delete);
    out.push(buffering(entry.vsam.buffering));
    u16v(out, entry.vsam.stripe_count);

    optional_string(out, entry.security.encryption_key_label.as_deref())?;
    out.push(compression(entry.security.compression));

    out.push(catalog_kind(entry.catalog.entry_kind));
    optional_string(out, entry.catalog.catalog.as_ref().map(DatasetName::as_str))?;
    optional_string(out, entry.catalog.owner.as_deref())?;
    if include_creation_date {
        optional_u32(out, entry.catalog.creation_date);
    }
    optional_u32(out, entry.catalog.expiration_date);
    optional_u16(out, entry.catalog.retention_days);

    out.push(lifecycle(entry.lifecycle.state));
    out.push(entry.lifecycle.migration_level);
    u64v(out, entry.lifecycle.backup_generation);
    Ok(())
}

fn decode_metadata(
    reader: &mut Reader<'_>,
    definition: &mut DatasetDefinition,
    max_items: usize,
    has_access_mode: bool,
    has_creation_date: bool,
) -> Result<(), ()> {
    definition.dcb = DcbOptions {
        block_size: reader.u32()?,
        buffer_count: reader.u16()?,
        buffer_size: reader.optional_u32()?,
    };
    definition.allocation = AllocationSpace {
        unit: space_unit_back(reader.byte()?)?,
        primary: reader.u64()?,
        secondary: reader.u64()?,
        directory_blocks: reader.u32()?,
        release_unused: reader.bool()?,
        contiguous: reader.bool()?,
        round_to_cylinder: reader.bool()?,
    };
    let kind = volume_kind_back(reader.byte()?)?;
    let volume_count = usize::try_from(reader.u32()?).map_err(|_| ())?;
    if volume_count == 0 || volume_count > max_items {
        return Err(());
    }
    let mut volume_ids = Vec::with_capacity(volume_count);
    for _ in 0..volume_count {
        volume_ids.push(reader.string(128)?);
    }
    definition.volumes = VolumeSelection {
        kind,
        volume_ids,
        device_type: reader.optional_string(128)?,
        unit_count: reader.u16()?,
    };
    definition.sms = SmsClasses {
        data_class: reader.optional_string(128)?,
        management_class: reader.optional_string(128)?,
        storage_class: reader.optional_string(128)?,
        acs_routine: reader.optional_string(128)?,
        guaranteed_space: reader.bool()?,
        extended_format: reader.bool()?,
        extended_addressable: reader.bool()?,
    };
    definition.vsam = VsamAttributes {
        control_interval_size: reader.optional_u32()?,
        control_area_size: reader.optional_u64()?,
        share_options: DatasetShareOptions {
            cross_region: reader.byte()?,
            cross_system: reader.byte()?,
        },
        access_mode: if has_access_mode {
            vsam_access_mode_back(reader.byte()?)?
        } else {
            VsamAccessMode::NonRls
        },
        spanned: reader.bool()?,
        reuse: reader.bool()?,
        speed: reader.bool()?,
        write_check: reader.bool()?,
        erase_on_delete: reader.bool()?,
        buffering: buffering_back(reader.byte()?)?,
        stripe_count: reader.u16()?,
    };
    definition.security = DataSecurity {
        encryption_key_label: reader.optional_string(128)?,
        compression: compression_back(reader.byte()?)?,
    };
    definition.catalog = CatalogMetadata {
        entry_kind: catalog_kind_back(reader.byte()?)?,
        catalog: reader
            .optional_string(128)?
            .map(|name| DatasetName::new(name, 128))
            .transpose()
            .map_err(|_| ())?,
        owner: reader.optional_string(128)?,
        creation_date: if has_creation_date {
            reader.optional_u32()?
        } else {
            None
        },
        expiration_date: reader.optional_u32()?,
        retention_days: reader.optional_u16()?,
    };
    definition.lifecycle = LifecycleMetadata {
        state: lifecycle_back(reader.byte()?)?,
        migration_level: reader.byte()?,
        backup_generation: reader.u64()?,
    };
    Ok(())
}

fn space_unit(value: SpaceUnit) -> u8 {
    match value {
        SpaceUnit::Tracks => 0,
        SpaceUnit::Cylinders => 1,
        SpaceUnit::Blocks => 2,
        SpaceUnit::Kilobytes => 3,
        SpaceUnit::Megabytes => 4,
        SpaceUnit::Records => 5,
    }
}

fn space_unit_back(value: u8) -> Result<SpaceUnit, ()> {
    match value {
        0 => Ok(SpaceUnit::Tracks),
        1 => Ok(SpaceUnit::Cylinders),
        2 => Ok(SpaceUnit::Blocks),
        3 => Ok(SpaceUnit::Kilobytes),
        4 => Ok(SpaceUnit::Megabytes),
        5 => Ok(SpaceUnit::Records),
        _ => Err(()),
    }
}

fn volume_kind(value: VolumeKind) -> u8 {
    match value {
        VolumeKind::Abstract => 0,
        VolumeKind::PhysicalDisk => 1,
        VolumeKind::Tape => 2,
    }
}

fn volume_kind_back(value: u8) -> Result<VolumeKind, ()> {
    match value {
        0 => Ok(VolumeKind::Abstract),
        1 => Ok(VolumeKind::PhysicalDisk),
        2 => Ok(VolumeKind::Tape),
        _ => Err(()),
    }
}

fn buffering(value: BufferingMode) -> u8 {
    match value {
        BufferingMode::System => 0,
        BufferingMode::NonsharedResources => 1,
        BufferingMode::LocalSharedResources => 2,
        BufferingMode::GlobalSharedResources => 3,
    }
}

fn buffering_back(value: u8) -> Result<BufferingMode, ()> {
    match value {
        0 => Ok(BufferingMode::System),
        1 => Ok(BufferingMode::NonsharedResources),
        2 => Ok(BufferingMode::LocalSharedResources),
        3 => Ok(BufferingMode::GlobalSharedResources),
        _ => Err(()),
    }
}

fn vsam_access_mode(value: VsamAccessMode) -> u8 {
    match value {
        VsamAccessMode::NonRls => 0,
        VsamAccessMode::Rls => 1,
        VsamAccessMode::Tvs => 2,
    }
}

fn vsam_access_mode_back(value: u8) -> Result<VsamAccessMode, ()> {
    match value {
        0 => Ok(VsamAccessMode::NonRls),
        1 => Ok(VsamAccessMode::Rls),
        2 => Ok(VsamAccessMode::Tvs),
        _ => Err(()),
    }
}

fn compression(value: CompressionMode) -> u8 {
    match value {
        CompressionMode::None => 0,
        CompressionMode::Generic => 1,
        CompressionMode::Tailored => 2,
    }
}

fn compression_back(value: u8) -> Result<CompressionMode, ()> {
    match value {
        0 => Ok(CompressionMode::None),
        1 => Ok(CompressionMode::Generic),
        2 => Ok(CompressionMode::Tailored),
        _ => Err(()),
    }
}

fn catalog_kind(value: CatalogEntryKind) -> u8 {
    match value {
        CatalogEntryKind::Dataset => 0,
        CatalogEntryKind::AlternateIndex => 1,
        CatalogEntryKind::Path => 2,
        CatalogEntryKind::Alias => 3,
        CatalogEntryKind::GenerationDataGroup => 4,
        CatalogEntryKind::UserCatalog => 5,
        CatalogEntryKind::MasterCatalog => 6,
        CatalogEntryKind::Library => 7,
        CatalogEntryKind::Volume => 8,
        CatalogEntryKind::PageSpace => 9,
    }
}

fn catalog_kind_back(value: u8) -> Result<CatalogEntryKind, ()> {
    match value {
        0 => Ok(CatalogEntryKind::Dataset),
        1 => Ok(CatalogEntryKind::AlternateIndex),
        2 => Ok(CatalogEntryKind::Path),
        3 => Ok(CatalogEntryKind::Alias),
        4 => Ok(CatalogEntryKind::GenerationDataGroup),
        5 => Ok(CatalogEntryKind::UserCatalog),
        6 => Ok(CatalogEntryKind::MasterCatalog),
        7 => Ok(CatalogEntryKind::Library),
        8 => Ok(CatalogEntryKind::Volume),
        9 => Ok(CatalogEntryKind::PageSpace),
        _ => Err(()),
    }
}

fn lifecycle(value: DatasetLifecycleState) -> u8 {
    match value {
        DatasetLifecycleState::Allocated => 0,
        DatasetLifecycleState::Cataloged => 1,
        DatasetLifecycleState::Open => 2,
        DatasetLifecycleState::Closed => 3,
        DatasetLifecycleState::Migrated => 4,
        DatasetLifecycleState::RecallPending => 5,
        DatasetLifecycleState::RecoveryRequired => 6,
    }
}

fn lifecycle_back(value: u8) -> Result<DatasetLifecycleState, ()> {
    match value {
        0 => Ok(DatasetLifecycleState::Allocated),
        1 => Ok(DatasetLifecycleState::Cataloged),
        2 => Ok(DatasetLifecycleState::Open),
        3 => Ok(DatasetLifecycleState::Closed),
        4 => Ok(DatasetLifecycleState::Migrated),
        5 => Ok(DatasetLifecycleState::RecallPending),
        6 => Ok(DatasetLifecycleState::RecoveryRequired),
        _ => Err(()),
    }
}
fn u16v(out: &mut Vec<u8>, v: u16) {
    out.extend_from_slice(&v.to_be_bytes())
}
fn u32v(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_be_bytes())
}
fn u64v(out: &mut Vec<u8>, v: u64) {
    out.extend_from_slice(&v.to_be_bytes())
}
fn boolv(out: &mut Vec<u8>, value: bool) {
    out.push(u8::from(value));
}
fn optional_u32(out: &mut Vec<u8>, v: Option<u32>) {
    out.push(u8::from(v.is_some()));
    if let Some(v) = v {
        u32v(out, v)
    }
}
fn optional_u16(out: &mut Vec<u8>, v: Option<u16>) {
    out.push(u8::from(v.is_some()));
    if let Some(v) = v {
        u16v(out, v)
    }
}
fn optional_u64(out: &mut Vec<u8>, v: Option<u64>) {
    out.push(u8::from(v.is_some()));
    if let Some(v) = v {
        u64v(out, v)
    }
}
fn optional_string(out: &mut Vec<u8>, value: Option<&str>) -> Result<(), ()> {
    out.push(u8::from(value.is_some()));
    if let Some(value) = value {
        bytes(out, value.as_bytes())?;
    }
    Ok(())
}
fn bytes(out: &mut Vec<u8>, value: &[u8]) -> Result<(), ()> {
    u32v(out, u32::try_from(value.len()).map_err(|_| ())?);
    out.extend_from_slice(value);
    Ok(())
}
fn records(out: &mut Vec<u8>, values: &[Vec<u8>]) -> Result<(), ()> {
    u32v(out, u32::try_from(values.len()).map_err(|_| ())?);
    for value in values {
        bytes(out, value)?;
    }
    Ok(())
}
struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}
impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], ()> {
        let end = self.at.checked_add(n).ok_or(())?;
        let out = self.bytes.get(self.at..end).ok_or(())?;
        self.at = end;
        Ok(out)
    }
    fn byte(&mut self) -> Result<u8, ()> {
        Ok(self.take(1)?[0])
    }
    fn bool(&mut self) -> Result<bool, ()> {
        match self.byte()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(()),
        }
    }
    fn u16(&mut self) -> Result<u16, ()> {
        Ok(u16::from_be_bytes(
            self.take(2)?.try_into().map_err(|_| ())?,
        ))
    }
    fn u32(&mut self) -> Result<u32, ()> {
        Ok(u32::from_be_bytes(
            self.take(4)?.try_into().map_err(|_| ())?,
        ))
    }
    fn u64(&mut self) -> Result<u64, ()> {
        Ok(u64::from_be_bytes(
            self.take(8)?.try_into().map_err(|_| ())?,
        ))
    }
    fn optional_u32(&mut self) -> Result<Option<u32>, ()> {
        match self.byte()? {
            0 => Ok(None),
            1 => Ok(Some(self.u32()?)),
            _ => Err(()),
        }
    }
    fn optional_u16(&mut self) -> Result<Option<u16>, ()> {
        match self.byte()? {
            0 => Ok(None),
            1 => Ok(Some(self.u16()?)),
            _ => Err(()),
        }
    }
    fn optional_u64(&mut self) -> Result<Option<u64>, ()> {
        match self.byte()? {
            0 => Ok(None),
            1 => Ok(Some(self.u64()?)),
            _ => Err(()),
        }
    }
    fn bytes(&mut self, max: usize) -> Result<Vec<u8>, ()> {
        let n = usize::try_from(self.u32()?).map_err(|_| ())?;
        if n > max {
            return Err(());
        }
        Ok(self.take(n)?.to_vec())
    }
    fn string(&mut self, max: usize) -> Result<String, ()> {
        String::from_utf8(self.bytes(max)?).map_err(|_| ())
    }
    fn optional_string(&mut self, max: usize) -> Result<Option<String>, ()> {
        match self.byte()? {
            0 => Ok(None),
            1 => Ok(Some(self.string(max)?)),
            _ => Err(()),
        }
    }
    fn records(&mut self, max_count: usize, max_record: usize) -> Result<Vec<Vec<u8>>, ()> {
        let count = usize::try_from(self.u32()?).map_err(|_| ())?;
        if count > max_count {
            return Err(());
        }
        let mut out = Vec::with_capacity(count);
        for _ in 0..count {
            out.push(self.bytes(max_record)?);
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meds2_state_upgrades_to_the_typed_meds6_definition() {
        let mut legacy = b"MEDS2".to_vec();
        legacy.extend_from_slice(&[0, 0]);
        legacy.extend_from_slice(&4u32.to_be_bytes());
        legacy.extend_from_slice(&[0, 0, 1]);
        legacy.extend_from_slice(&37u16.to_be_bytes());
        legacy.extend_from_slice(&2u64.to_be_bytes());
        legacy.extend_from_slice(&1u32.to_be_bytes());
        legacy.extend_from_slice(&4u32.to_be_bytes());
        legacy.extend_from_slice(b"ABCD");
        legacy.extend_from_slice(&0u32.to_be_bytes());
        legacy.extend_from_slice(&0u32.to_be_bytes());

        let decoded = decode(&legacy, 8, 80, 8).unwrap();
        assert_eq!(decoded.version, 2);
        assert_eq!(decoded.records, [b"ABCD".to_vec()]);
        assert_eq!(
            decoded.definition(),
            DatasetDefinition::compatibility(DatasetAttributes {
                organization: DatasetOrganization::Sequential,
                record_format: RecordFormat::Fixed,
                logical_record_length: 4,
                key_offset: None,
                key_length: None,
                ccsid: Some(37),
            })
        );
        let upgraded = encode(&decoded).unwrap();
        assert_eq!(&upgraded[..5], b"MEDS6");
        assert_eq!(decode(&upgraded, 8, 80, 8).unwrap(), decoded);
    }

    #[test]
    fn meds3_definition_upgrades_to_meds6_with_empty_pdse_directory() {
        let definition = DatasetDefinition::compatibility(DatasetAttributes {
            organization: DatasetOrganization::PartitionedExtended,
            record_format: RecordFormat::Fixed,
            logical_record_length: 80,
            key_offset: None,
            key_length: None,
            ccsid: Some(1047),
        });
        let legacy = encode_definition_digest_v2(&definition).unwrap();
        assert_eq!(&legacy[..5], b"MEDS3");
        let decoded = decode(&legacy, 8, 80, 8).unwrap();
        assert!(decoded.member_generations.is_empty());
        assert!(decoded.member_aliases.is_empty());
        let upgraded = encode(&decoded).unwrap();
        assert_eq!(&upgraded[..5], b"MEDS6");
        assert_eq!(decode(&upgraded, 8, 80, 8).unwrap(), decoded);
    }

    #[test]
    fn meds4_state_defaults_to_non_rls_and_upgrades_without_data_loss() {
        let definition = DatasetDefinition::compatibility(DatasetAttributes {
            organization: DatasetOrganization::KeySequenced,
            record_format: RecordFormat::Fixed,
            logical_record_length: 4,
            key_offset: Some(0),
            key_length: Some(2),
            ccsid: Some(37),
        });
        let mut original = Entry::from_definition(definition, 7);
        original.records = vec![b"AA11".to_vec(), b"BB22".to_vec()];
        let legacy = encode_state(&original, b"MEDS4", false, false).unwrap();
        let decoded = decode(&legacy, 8, 80, 8).unwrap();
        assert_eq!(decoded.vsam.access_mode, VsamAccessMode::NonRls);
        assert_eq!(decoded.records, original.records);
        assert_eq!(decoded.version, 7);
        let upgraded = encode(&decoded).unwrap();
        assert_eq!(&upgraded[..5], b"MEDS6");
        assert_eq!(decode(&upgraded, 8, 80, 8).unwrap(), decoded);
    }

    #[test]
    fn meds6_persists_catalog_creation_and_retention_dates() {
        let mut definition = DatasetDefinition::compatibility(DatasetAttributes {
            organization: DatasetOrganization::Sequential,
            record_format: RecordFormat::Fixed,
            logical_record_length: 4,
            key_offset: None,
            key_length: None,
            ccsid: Some(37),
        });
        definition.catalog.owner = Some("OWNER1".into());
        definition.catalog.creation_date = Some(2_026_001);
        definition.catalog.retention_days = Some(30);
        let entry = Entry::from_definition(definition, 3);
        let encoded = encode(&entry).unwrap();
        assert_eq!(&encoded[..5], b"MEDS6");
        assert_eq!(decode(&encoded, 8, 80, 8).unwrap(), entry);
    }

    #[test]
    fn definition_digest_v3_adds_creation_date_without_drifting_legacy_no_date_bytes() {
        let mut definition = DatasetDefinition::compatibility(DatasetAttributes {
            organization: DatasetOrganization::Sequential,
            record_format: RecordFormat::Fixed,
            logical_record_length: 4,
            key_offset: None,
            key_length: None,
            ccsid: Some(37),
        });
        assert_eq!(
            encode_definition_digest_v3(&definition).unwrap(),
            encode_definition_digest_v2(&definition).unwrap()
        );
        definition.catalog.creation_date = Some(2_026_001);
        let first = encode_definition_digest_v3(&definition).unwrap();
        definition.catalog.creation_date = Some(2_026_002);
        let second = encode_definition_digest_v3(&definition).unwrap();
        assert_ne!(first, second);
        assert_eq!(&first[..5], b"MEDS3");
        assert_eq!(&second[..5], b"MEDS3");
    }
}
