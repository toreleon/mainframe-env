// Frozen exhaustive host wire schema: every field or variant requires protocol review.
use super::*;

impl Canonical for SpaceUnit {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        match self {
            Self::Tracks => out.variant("SpaceUnit", "Tracks", 0),
            Self::Cylinders => out.variant("SpaceUnit", "Cylinders", 0),
            Self::Blocks => out.variant("SpaceUnit", "Blocks", 0),
            Self::Kilobytes => out.variant("SpaceUnit", "Kilobytes", 0),
            Self::Megabytes => out.variant("SpaceUnit", "Megabytes", 0),
            Self::Records => out.variant("SpaceUnit", "Records", 0),
        }
    }
}

impl Canonical for AllocationSpace {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        let Self {
            contiguous,
            directory_blocks,
            primary,
            release_unused,
            round_to_cylinder,
            secondary,
            unit,
        } = self;
        out.object("AllocationSpace", 7)?;
        out.text("contiguous")?;
        contiguous.encode(out)?;
        out.text("directory_blocks")?;
        directory_blocks.encode(out)?;
        out.text("primary")?;
        primary.encode(out)?;
        out.text("release_unused")?;
        release_unused.encode(out)?;
        out.text("round_to_cylinder")?;
        round_to_cylinder.encode(out)?;
        out.text("secondary")?;
        secondary.encode(out)?;
        out.text("unit")?;
        unit.encode(out)?;
        Ok(())
    }
}

impl Canonical for DcbOptions {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        let Self {
            block_size,
            buffer_count,
            buffer_size,
        } = self;
        out.object("DcbOptions", 3)?;
        out.text("block_size")?;
        block_size.encode(out)?;
        out.text("buffer_count")?;
        buffer_count.encode(out)?;
        out.text("buffer_size")?;
        buffer_size.encode(out)?;
        Ok(())
    }
}

impl Canonical for VolumeKind {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        match self {
            Self::Abstract => out.variant("VolumeKind", "Abstract", 0),
            Self::PhysicalDisk => out.variant("VolumeKind", "PhysicalDisk", 0),
            Self::Tape => out.variant("VolumeKind", "Tape", 0),
        }
    }
}

impl Canonical for VolumeSelection {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        let Self {
            device_type,
            kind,
            unit_count,
            volume_ids,
        } = self;
        out.object("VolumeSelection", 4)?;
        out.text("device_type")?;
        device_type.encode(out)?;
        out.text("kind")?;
        kind.encode(out)?;
        out.text("unit_count")?;
        unit_count.encode(out)?;
        out.text("volume_ids")?;
        volume_ids.encode(out)?;
        Ok(())
    }
}

impl Canonical for SmsClasses {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        let Self {
            acs_routine,
            data_class,
            extended_addressable,
            extended_format,
            guaranteed_space,
            management_class,
            storage_class,
        } = self;
        out.object("SmsClasses", 7)?;
        out.text("acs_routine")?;
        acs_routine.encode(out)?;
        out.text("data_class")?;
        data_class.encode(out)?;
        out.text("extended_addressable")?;
        extended_addressable.encode(out)?;
        out.text("extended_format")?;
        extended_format.encode(out)?;
        out.text("guaranteed_space")?;
        guaranteed_space.encode(out)?;
        out.text("management_class")?;
        management_class.encode(out)?;
        out.text("storage_class")?;
        storage_class.encode(out)?;
        Ok(())
    }
}

impl Canonical for CompressionMode {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        match self {
            Self::None => out.variant("CompressionMode", "None", 0),
            Self::Generic => out.variant("CompressionMode", "Generic", 0),
            Self::Tailored => out.variant("CompressionMode", "Tailored", 0),
        }
    }
}

impl Canonical for BufferingMode {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        match self {
            Self::System => out.variant("BufferingMode", "System", 0),
            Self::NonsharedResources => out.variant("BufferingMode", "NonsharedResources", 0),
            Self::LocalSharedResources => out.variant("BufferingMode", "LocalSharedResources", 0),
            Self::GlobalSharedResources => out.variant("BufferingMode", "GlobalSharedResources", 0),
        }
    }
}

impl Canonical for VsamAccessMode {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        match self {
            Self::NonRls => out.variant("VsamAccessMode", "NonRls", 0),
            Self::Rls => out.variant("VsamAccessMode", "Rls", 0),
            Self::Tvs => out.variant("VsamAccessMode", "Tvs", 0),
        }
    }
}

impl Canonical for DatasetLockMode {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        match self {
            Self::Shared => out.variant("DatasetLockMode", "Shared", 0),
            Self::Update => out.variant("DatasetLockMode", "Update", 0),
            Self::Exclusive => out.variant("DatasetLockMode", "Exclusive", 0),
        }
    }
}

impl Canonical for DatasetLockTarget {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        match self {
            Self::Dataset => out.variant("DatasetLockTarget", "Dataset", 0),
            Self::Record(v0) => {
                out.variant("DatasetLockTarget", "Record", 1)?;
                out.text("0")?;
                v0.encode(out)?;
                Ok(())
            }
        }
    }
}

impl Canonical for DatasetLockReceipt {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        let Self {
            dataset,
            expires_at,
            lock_id,
            mode,
            owner,
            target,
            transaction,
            version,
        } = self;
        out.object("DatasetLockReceipt", 8)?;
        out.text("dataset")?;
        dataset.encode(out)?;
        out.text("expires_at")?;
        expires_at.encode(out)?;
        out.text("lock_id")?;
        lock_id.encode(out)?;
        out.text("mode")?;
        mode.encode(out)?;
        out.text("owner")?;
        owner.encode(out)?;
        out.text("target")?;
        target.encode(out)?;
        out.text("transaction")?;
        transaction.encode(out)?;
        out.text("version")?;
        version.encode(out)?;
        Ok(())
    }
}

impl Canonical for TvsRecordOperation {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        match self {
            Self::Insert { dataset, record } => {
                out.variant("TvsRecordOperation", "Insert", 2)?;
                out.text("dataset")?;
                dataset.encode(out)?;
                out.text("record")?;
                record.encode(out)?;
                Ok(())
            }
            Self::Rewrite {
                dataset,
                key,
                record,
            } => {
                out.variant("TvsRecordOperation", "Rewrite", 3)?;
                out.text("dataset")?;
                dataset.encode(out)?;
                out.text("key")?;
                key.encode(out)?;
                out.text("record")?;
                record.encode(out)?;
                Ok(())
            }
            Self::Delete { dataset, key } => {
                out.variant("TvsRecordOperation", "Delete", 2)?;
                out.text("dataset")?;
                dataset.encode(out)?;
                out.text("key")?;
                key.encode(out)?;
                Ok(())
            }
        }
    }
}

impl Canonical for TvsUnitOfWorkState {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        match self {
            Self::Active => out.variant("TvsUnitOfWorkState", "Active", 0),
            Self::Committed => out.variant("TvsUnitOfWorkState", "Committed", 0),
            Self::RolledBack => out.variant("TvsUnitOfWorkState", "RolledBack", 0),
            Self::Unknown => out.variant("TvsUnitOfWorkState", "Unknown", 0),
        }
    }
}

impl Canonical for TvsUnitOfWorkReceipt {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        let Self {
            owner,
            staged_operations,
            state,
            transaction,
            version,
        } = self;
        out.object("TvsUnitOfWorkReceipt", 5)?;
        out.text("owner")?;
        owner.encode(out)?;
        out.text("staged_operations")?;
        staged_operations.encode(out)?;
        out.text("state")?;
        state.encode(out)?;
        out.text("transaction")?;
        transaction.encode(out)?;
        out.text("version")?;
        version.encode(out)?;
        Ok(())
    }
}

impl Canonical for DatasetShareOptions {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        let Self {
            cross_region,
            cross_system,
        } = self;
        out.object("DatasetShareOptions", 2)?;
        out.text("cross_region")?;
        cross_region.encode(out)?;
        out.text("cross_system")?;
        cross_system.encode(out)?;
        Ok(())
    }
}

impl Canonical for VsamAttributes {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        let Self {
            access_mode,
            buffering,
            control_area_size,
            control_interval_size,
            erase_on_delete,
            reuse,
            share_options,
            spanned,
            speed,
            stripe_count,
            write_check,
        } = self;
        out.object("VsamAttributes", 11)?;
        out.text("access_mode")?;
        access_mode.encode(out)?;
        out.text("buffering")?;
        buffering.encode(out)?;
        out.text("control_area_size")?;
        control_area_size.encode(out)?;
        out.text("control_interval_size")?;
        control_interval_size.encode(out)?;
        out.text("erase_on_delete")?;
        erase_on_delete.encode(out)?;
        out.text("reuse")?;
        reuse.encode(out)?;
        out.text("share_options")?;
        share_options.encode(out)?;
        out.text("spanned")?;
        spanned.encode(out)?;
        out.text("speed")?;
        speed.encode(out)?;
        out.text("stripe_count")?;
        stripe_count.encode(out)?;
        out.text("write_check")?;
        write_check.encode(out)?;
        Ok(())
    }
}

impl Canonical for DataSecurity {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        let Self {
            compression,
            encryption_key_label,
        } = self;
        out.object("DataSecurity", 2)?;
        out.text("compression")?;
        compression.encode(out)?;
        out.text("encryption_key_label")?;
        encryption_key_label.encode(out)?;
        Ok(())
    }
}

impl Canonical for CatalogEntryKind {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        match self {
            Self::Dataset => out.variant("CatalogEntryKind", "Dataset", 0),
            Self::AlternateIndex => out.variant("CatalogEntryKind", "AlternateIndex", 0),
            Self::Path => out.variant("CatalogEntryKind", "Path", 0),
            Self::Alias => out.variant("CatalogEntryKind", "Alias", 0),
            Self::GenerationDataGroup => out.variant("CatalogEntryKind", "GenerationDataGroup", 0),
            Self::UserCatalog => out.variant("CatalogEntryKind", "UserCatalog", 0),
            Self::MasterCatalog => out.variant("CatalogEntryKind", "MasterCatalog", 0),
            Self::Library => out.variant("CatalogEntryKind", "Library", 0),
            Self::Volume => out.variant("CatalogEntryKind", "Volume", 0),
            Self::PageSpace => out.variant("CatalogEntryKind", "PageSpace", 0),
        }
    }
}

impl Canonical for CatalogKind {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        match self {
            Self::Master => out.variant("CatalogKind", "Master", 0),
            Self::User => out.variant("CatalogKind", "User", 0),
        }
    }
}

impl Canonical for CatalogResolution {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        let Self {
            alias_chain,
            catalog,
            requested,
            resolved,
            version,
        } = self;
        out.object("CatalogResolution", 5)?;
        out.text("alias_chain")?;
        alias_chain.encode(out)?;
        out.text("catalog")?;
        catalog.encode(out)?;
        out.text("requested")?;
        requested.encode(out)?;
        out.text("resolved")?;
        resolved.encode(out)?;
        out.text("version")?;
        version.encode(out)?;
        Ok(())
    }
}

impl Canonical for CatalogListEntry {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        let Self {
            kind,
            name,
            related,
            version,
        } = self;
        out.object("CatalogListEntry", 4)?;
        out.text("kind")?;
        kind.encode(out)?;
        out.text("name")?;
        name.encode(out)?;
        out.text("related")?;
        related.encode(out)?;
        out.text("version")?;
        version.encode(out)?;
        Ok(())
    }
}

impl Canonical for CatalogMetadata {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        let Self {
            catalog,
            creation_date,
            entry_kind,
            expiration_date,
            owner,
            retention_days,
        } = self;
        out.object("CatalogMetadata", 6)?;
        out.text("catalog")?;
        catalog.encode(out)?;
        out.text("creation_date")?;
        creation_date.encode(out)?;
        out.text("entry_kind")?;
        entry_kind.encode(out)?;
        out.text("expiration_date")?;
        expiration_date.encode(out)?;
        out.text("owner")?;
        owner.encode(out)?;
        out.text("retention_days")?;
        retention_days.encode(out)?;
        Ok(())
    }
}

impl Canonical for DatasetLifecycleState {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        match self {
            Self::Allocated => out.variant("DatasetLifecycleState", "Allocated", 0),
            Self::Cataloged => out.variant("DatasetLifecycleState", "Cataloged", 0),
            Self::Open => out.variant("DatasetLifecycleState", "Open", 0),
            Self::Closed => out.variant("DatasetLifecycleState", "Closed", 0),
            Self::Migrated => out.variant("DatasetLifecycleState", "Migrated", 0),
            Self::RecallPending => out.variant("DatasetLifecycleState", "RecallPending", 0),
            Self::RecoveryRequired => out.variant("DatasetLifecycleState", "RecoveryRequired", 0),
        }
    }
}

impl Canonical for LifecycleMetadata {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        let Self {
            backup_generation,
            migration_level,
            state,
        } = self;
        out.object("LifecycleMetadata", 3)?;
        out.text("backup_generation")?;
        backup_generation.encode(out)?;
        out.text("migration_level")?;
        migration_level.encode(out)?;
        out.text("state")?;
        state.encode(out)?;
        Ok(())
    }
}

impl Canonical for DatasetProviderCapabilities {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        let Self {
            abstract_volumes,
            allocation_extents,
            buffering,
            catalog_metadata,
            catalog_routing,
            compression,
            control_intervals,
            encryption,
            extended_format,
            migration_recall,
            physical_volumes,
            rls,
            schema_version,
            sharing,
            sms_acs,
            sms_classes,
            striping,
            tape,
            tvs,
            vsam_data_options,
        } = self;
        out.object("DatasetProviderCapabilities", 20)?;
        out.text("abstract_volumes")?;
        abstract_volumes.encode(out)?;
        out.text("allocation_extents")?;
        allocation_extents.encode(out)?;
        out.text("buffering")?;
        buffering.encode(out)?;
        out.text("catalog_metadata")?;
        catalog_metadata.encode(out)?;
        out.text("catalog_routing")?;
        catalog_routing.encode(out)?;
        out.text("compression")?;
        compression.encode(out)?;
        out.text("control_intervals")?;
        control_intervals.encode(out)?;
        out.text("encryption")?;
        encryption.encode(out)?;
        out.text("extended_format")?;
        extended_format.encode(out)?;
        out.text("migration_recall")?;
        migration_recall.encode(out)?;
        out.text("physical_volumes")?;
        physical_volumes.encode(out)?;
        out.text("rls")?;
        rls.encode(out)?;
        out.text("schema_version")?;
        schema_version.encode(out)?;
        out.text("sharing")?;
        sharing.encode(out)?;
        out.text("sms_acs")?;
        sms_acs.encode(out)?;
        out.text("sms_classes")?;
        sms_classes.encode(out)?;
        out.text("striping")?;
        striping.encode(out)?;
        out.text("tape")?;
        tape.encode(out)?;
        out.text("tvs")?;
        tvs.encode(out)?;
        out.text("vsam_data_options")?;
        vsam_data_options.encode(out)?;
        Ok(())
    }
}

impl Canonical for DatasetDefinition {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        let Self {
            allocation,
            attributes,
            catalog,
            dcb,
            lifecycle,
            security,
            sms,
            volumes,
            vsam,
        } = self;
        out.object("DatasetDefinition", 9)?;
        out.text("allocation")?;
        allocation.encode(out)?;
        out.text("attributes")?;
        attributes.encode(out)?;
        out.text("catalog")?;
        catalog.encode(out)?;
        out.text("dcb")?;
        dcb.encode(out)?;
        out.text("lifecycle")?;
        lifecycle.encode(out)?;
        out.text("security")?;
        security.encode(out)?;
        out.text("sms")?;
        sms.encode(out)?;
        out.text("volumes")?;
        volumes.encode(out)?;
        out.text("vsam")?;
        vsam.encode(out)?;
        Ok(())
    }
}

impl Canonical for DatasetDescription {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        let Self {
            abstract_placement,
            allocated_bytes,
            buffer_bytes,
            control_areas,
            control_intervals,
            definition,
            extents,
            high_used_rba,
            max_rba,
            used_bytes,
            version,
        } = self;
        out.object("DatasetDescription", 11)?;
        out.text("abstract_placement")?;
        abstract_placement.encode(out)?;
        out.text("allocated_bytes")?;
        allocated_bytes.encode(out)?;
        out.text("buffer_bytes")?;
        buffer_bytes.encode(out)?;
        out.text("control_areas")?;
        control_areas.encode(out)?;
        out.text("control_intervals")?;
        control_intervals.encode(out)?;
        out.text("definition")?;
        definition.encode(out)?;
        out.text("extents")?;
        extents.encode(out)?;
        out.text("high_used_rba")?;
        high_used_rba.encode(out)?;
        out.text("max_rba")?;
        max_rba.encode(out)?;
        out.text("used_bytes")?;
        used_bytes.encode(out)?;
        out.text("version")?;
        version.encode(out)?;
        Ok(())
    }
}

impl Canonical for DatasetExtent {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        let Self {
            length,
            ordinal,
            start,
            volume_id,
            volume_start,
        } = self;
        out.object("DatasetExtent", 5)?;
        out.text("length")?;
        length.encode(out)?;
        out.text("ordinal")?;
        ordinal.encode(out)?;
        out.text("start")?;
        start.encode(out)?;
        out.text("volume_id")?;
        volume_id.encode(out)?;
        out.text("volume_start")?;
        volume_start.encode(out)?;
        Ok(())
    }
}

impl Canonical for DatasetVolumeExtent {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        let Self {
            dataset,
            dataset_extent_ordinal,
            length,
            logical_start,
            volume_start,
        } = self;
        out.object("DatasetVolumeExtent", 5)?;
        out.text("dataset")?;
        dataset.encode(out)?;
        out.text("dataset_extent_ordinal")?;
        dataset_extent_ordinal.encode(out)?;
        out.text("length")?;
        length.encode(out)?;
        out.text("logical_start")?;
        logical_start.encode(out)?;
        out.text("volume_start")?;
        volume_start.encode(out)?;
        Ok(())
    }
}

impl Canonical for DatasetVolumeDescription {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        let Self {
            allocated_bytes,
            extents,
            used_bytes,
            volume_id,
        } = self;
        out.object("DatasetVolumeDescription", 4)?;
        out.text("allocated_bytes")?;
        allocated_bytes.encode(out)?;
        out.text("extents")?;
        extents.encode(out)?;
        out.text("used_bytes")?;
        used_bytes.encode(out)?;
        out.text("volume_id")?;
        volume_id.encode(out)?;
        Ok(())
    }
}

impl Canonical for DatasetSnapshot {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        let Self {
            definition,
            linear_data,
            members,
            records,
            relative_records,
        } = self;
        out.object("DatasetSnapshot", 5)?;
        out.text("definition")?;
        definition.encode(out)?;
        out.text("linear_data")?;
        linear_data.encode(out)?;
        out.text("members")?;
        members.encode(out)?;
        out.text("records")?;
        records.encode(out)?;
        out.text("relative_records")?;
        relative_records.encode(out)?;
        Ok(())
    }
}

impl Canonical for DatasetRelativeRecordSnapshot {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        let Self {
            record,
            record_number,
        } = self;
        out.object("DatasetRelativeRecordSnapshot", 2)?;
        out.text("record")?;
        record.encode(out)?;
        out.text("record_number")?;
        record_number.encode(out)?;
        Ok(())
    }
}

impl Canonical for DatasetMemberSnapshot {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        let Self {
            alias_of,
            generations,
            name,
            records,
        } = self;
        out.object("DatasetMemberSnapshot", 4)?;
        out.text("alias_of")?;
        alias_of.encode(out)?;
        out.text("generations")?;
        generations.encode(out)?;
        out.text("name")?;
        name.encode(out)?;
        out.text("records")?;
        records.encode(out)?;
        Ok(())
    }
}

impl Canonical for DatasetMemberGenerationSnapshot {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        let Self {
            generation,
            program_object,
            records,
        } = self;
        out.object("DatasetMemberGenerationSnapshot", 3)?;
        out.text("generation")?;
        generation.encode(out)?;
        out.text("program_object")?;
        program_object.encode(out)?;
        out.text("records")?;
        records.encode(out)?;
        Ok(())
    }
}

impl Canonical for DatasetDiagnostic {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        let Self {
            code,
            detail,
            field,
        } = self;
        out.object("DatasetDiagnostic", 3)?;
        out.text("code")?;
        code.encode(out)?;
        out.text("detail")?;
        detail.encode(out)?;
        out.text("field")?;
        field.encode(out)?;
        Ok(())
    }
}

impl Canonical for HostLimits {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        let Self {
            max_audit_fields,
            max_fields,
            max_name_bytes,
            max_record_bytes,
            max_records,
            max_state_bytes,
        } = self;
        out.object("HostLimits", 6)?;
        out.text("max_audit_fields")?;
        max_audit_fields.encode(out)?;
        out.text("max_fields")?;
        max_fields.encode(out)?;
        out.text("max_name_bytes")?;
        max_name_bytes.encode(out)?;
        out.text("max_record_bytes")?;
        max_record_bytes.encode(out)?;
        out.text("max_records")?;
        max_records.encode(out)?;
        out.text("max_state_bytes")?;
        max_state_bytes.encode(out)?;
        Ok(())
    }
}

impl Canonical for DatasetOrganization {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        match self {
            Self::Sequential => out.variant("DatasetOrganization", "Sequential", 0),
            Self::Partitioned => out.variant("DatasetOrganization", "Partitioned", 0),
            Self::PartitionedExtended => {
                out.variant("DatasetOrganization", "PartitionedExtended", 0)
            }
            Self::KeySequenced => out.variant("DatasetOrganization", "KeySequenced", 0),
            Self::EntrySequenced => out.variant("DatasetOrganization", "EntrySequenced", 0),
            Self::Relative => out.variant("DatasetOrganization", "Relative", 0),
            Self::VariableRelative => out.variant("DatasetOrganization", "VariableRelative", 0),
            Self::Linear => out.variant("DatasetOrganization", "Linear", 0),
        }
    }
}

impl Canonical for RecordFormat {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        match self {
            Self::Fixed => out.variant("RecordFormat", "Fixed", 0),
            Self::FixedBlocked => out.variant("RecordFormat", "FixedBlocked", 0),
            Self::FixedBlockedStandard => out.variant("RecordFormat", "FixedBlockedStandard", 0),
            Self::Variable => out.variant("RecordFormat", "Variable", 0),
            Self::VariableBlocked => out.variant("RecordFormat", "VariableBlocked", 0),
            Self::VariableSpanned => out.variant("RecordFormat", "VariableSpanned", 0),
            Self::VariableBlockedSpanned => {
                out.variant("RecordFormat", "VariableBlockedSpanned", 0)
            }
            Self::Undefined => out.variant("RecordFormat", "Undefined", 0),
            Self::Line => out.variant("RecordFormat", "Line", 0),
        }
    }
}

impl Canonical for KeyRelation {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        match self {
            Self::Equal => out.variant("KeyRelation", "Equal", 0),
            Self::Greater => out.variant("KeyRelation", "Greater", 0),
            Self::GreaterOrEqual => out.variant("KeyRelation", "GreaterOrEqual", 0),
            Self::Less => out.variant("KeyRelation", "Less", 0),
            Self::LessOrEqual => out.variant("KeyRelation", "LessOrEqual", 0),
        }
    }
}

impl Canonical for DatasetReadLockMode {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        match self {
            Self::Default => out.variant("DatasetReadLockMode", "Default", 0),
            Self::Lock => out.variant("DatasetReadLockMode", "Lock", 0),
            Self::KeptLock => out.variant("DatasetReadLockMode", "KeptLock", 0),
            Self::NoLock => out.variant("DatasetReadLockMode", "NoLock", 0),
            Self::IgnoreLock => out.variant("DatasetReadLockMode", "IgnoreLock", 0),
        }
    }
}

impl Canonical for DatasetReadControl {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        let Self { lock, wait } = self;
        out.object("DatasetReadControl", 2)?;
        out.text("lock")?;
        lock.encode(out)?;
        out.text("wait")?;
        wait.encode(out)?;
        Ok(())
    }
}

impl Canonical for DatasetReelUnit {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        match self {
            Self::Reel => out.variant("DatasetReelUnit", "Reel", 0),
            Self::Unit => out.variant("DatasetReelUnit", "Unit", 0),
        }
    }
}

impl Canonical for DatasetCloseControl {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        let Self {
            lock,
            no_rewind,
            reel_or_unit,
            removal,
        } = self;
        out.object("DatasetCloseControl", 4)?;
        out.text("lock")?;
        lock.encode(out)?;
        out.text("no_rewind")?;
        no_rewind.encode(out)?;
        out.text("reel_or_unit")?;
        reel_or_unit.encode(out)?;
        out.text("removal")?;
        removal.encode(out)?;
        Ok(())
    }
}

impl Canonical for AccessIntent {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        match self {
            Self::Read => out.variant("AccessIntent", "Read", 0),
            Self::Execute => out.variant("AccessIntent", "Execute", 0),
            Self::Update => out.variant("AccessIntent", "Update", 0),
            Self::Control => out.variant("AccessIntent", "Control", 0),
            Self::Alter => out.variant("AccessIntent", "Alter", 0),
        }
    }
}

impl Canonical for DatasetAttributes {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        let Self {
            ccsid,
            key_length,
            key_offset,
            logical_record_length,
            organization,
            record_format,
        } = self;
        out.object("DatasetAttributes", 6)?;
        out.text("ccsid")?;
        ccsid.encode(out)?;
        out.text("key_length")?;
        key_length.encode(out)?;
        out.text("key_offset")?;
        key_offset.encode(out)?;
        out.text("logical_record_length")?;
        logical_record_length.encode(out)?;
        out.text("organization")?;
        organization.encode(out)?;
        out.text("record_format")?;
        record_format.encode(out)?;
        Ok(())
    }
}

impl Canonical for Mutation {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        let Self {
            idempotency_key,
            sequence,
            transaction,
        } = self;
        out.object("Mutation", 3)?;
        out.text("idempotency_key")?;
        idempotency_key.encode(out)?;
        out.text("sequence")?;
        sequence.encode(out)?;
        out.text("transaction")?;
        transaction.encode(out)?;
        Ok(())
    }
}

impl Canonical for DatasetRequest {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        match self {
            Self::Capabilities => out.variant("DatasetRequest", "Capabilities", 0),
            Self::List {
                max_items,
                pattern,
                start,
            } => {
                out.variant("DatasetRequest", "List", 3)?;
                out.text("max_items")?;
                max_items.encode(out)?;
                out.text("pattern")?;
                pattern.encode(out)?;
                out.text("start")?;
                start.encode(out)?;
                Ok(())
            }
            Self::Attributes { dataset } => {
                out.variant("DatasetRequest", "Attributes", 1)?;
                out.text("dataset")?;
                dataset.encode(out)?;
                Ok(())
            }
            Self::Describe { dataset } => {
                out.variant("DatasetRequest", "Describe", 1)?;
                out.text("dataset")?;
                dataset.encode(out)?;
                Ok(())
            }
            Self::Diagnose { dataset } => {
                out.variant("DatasetRequest", "Diagnose", 1)?;
                out.text("dataset")?;
                dataset.encode(out)?;
                Ok(())
            }
            Self::ResolveCatalog { name } => {
                out.variant("DatasetRequest", "ResolveCatalog", 1)?;
                out.text("name")?;
                name.encode(out)?;
                Ok(())
            }
            Self::ListCatalog {
                max_items,
                pattern,
                start,
            } => {
                out.variant("DatasetRequest", "ListCatalog", 3)?;
                out.text("max_items")?;
                max_items.encode(out)?;
                out.text("pattern")?;
                pattern.encode(out)?;
                out.text("start")?;
                start.encode(out)?;
                Ok(())
            }
            Self::ListVolumes { max_items, start } => {
                out.variant("DatasetRequest", "ListVolumes", 2)?;
                out.text("max_items")?;
                max_items.encode(out)?;
                out.text("start")?;
                start.encode(out)?;
                Ok(())
            }
            Self::ListLocks {
                dataset,
                max_items,
                now_tick,
            } => {
                out.variant("DatasetRequest", "ListLocks", 3)?;
                out.text("dataset")?;
                dataset.encode(out)?;
                out.text("max_items")?;
                max_items.encode(out)?;
                out.text("now_tick")?;
                now_tick.encode(out)?;
                Ok(())
            }
            Self::TvsStatus { owner, transaction } => {
                out.variant("DatasetRequest", "TvsStatus", 2)?;
                out.text("owner")?;
                owner.encode(out)?;
                out.text("transaction")?;
                transaction.encode(out)?;
                Ok(())
            }
            Self::ListMembers {
                dataset,
                max_items,
                start,
            } => {
                out.variant("DatasetRequest", "ListMembers", 3)?;
                out.text("dataset")?;
                dataset.encode(out)?;
                out.text("max_items")?;
                max_items.encode(out)?;
                out.text("start")?;
                start.encode(out)?;
                Ok(())
            }
            Self::ReadMemberGeneration {
                dataset,
                max_records,
                member,
                relative,
            } => {
                out.variant("DatasetRequest", "ReadMemberGeneration", 4)?;
                out.text("dataset")?;
                dataset.encode(out)?;
                out.text("max_records")?;
                max_records.encode(out)?;
                out.text("member")?;
                member.encode(out)?;
                out.text("relative")?;
                relative.encode(out)?;
                Ok(())
            }
            Self::Read {
                control,
                dataset,
                key,
                max_records,
                member,
            } => {
                out.variant("DatasetRequest", "Read", 5)?;
                out.text("control")?;
                control.encode(out)?;
                out.text("dataset")?;
                dataset.encode(out)?;
                out.text("key")?;
                key.encode(out)?;
                out.text("max_records")?;
                max_records.encode(out)?;
                out.text("member")?;
                member.encode(out)?;
                Ok(())
            }
            Self::ReadGeneric {
                dataset,
                key_prefix,
                max_records,
            } => {
                out.variant("DatasetRequest", "ReadGeneric", 3)?;
                out.text("dataset")?;
                dataset.encode(out)?;
                out.text("key_prefix")?;
                key_prefix.encode(out)?;
                out.text("max_records")?;
                max_records.encode(out)?;
                Ok(())
            }
            Self::ReadConcatenation {
                datasets,
                max_records,
                member,
            } => {
                out.variant("DatasetRequest", "ReadConcatenation", 3)?;
                out.text("datasets")?;
                datasets.encode(out)?;
                out.text("max_records")?;
                max_records.encode(out)?;
                out.text("member")?;
                member.encode(out)?;
                Ok(())
            }
            Self::ReadRelative {
                dataset,
                record_number,
            } => {
                out.variant("DatasetRequest", "ReadRelative", 2)?;
                out.text("dataset")?;
                dataset.encode(out)?;
                out.text("record_number")?;
                record_number.encode(out)?;
                Ok(())
            }
            Self::ReadRba {
                dataset,
                max_bytes,
                rba,
            } => {
                out.variant("DatasetRequest", "ReadRba", 3)?;
                out.text("dataset")?;
                dataset.encode(out)?;
                out.text("max_bytes")?;
                max_bytes.encode(out)?;
                out.text("rba")?;
                rba.encode(out)?;
                Ok(())
            }
            Self::ReadSequential {
                dataset,
                max_records,
                member,
                reverse,
                start,
            } => {
                out.variant("DatasetRequest", "ReadSequential", 5)?;
                out.text("dataset")?;
                dataset.encode(out)?;
                out.text("max_records")?;
                max_records.encode(out)?;
                out.text("member")?;
                member.encode(out)?;
                out.text("reverse")?;
                reverse.encode(out)?;
                out.text("start")?;
                start.encode(out)?;
                Ok(())
            }
            Self::Snapshot {
                dataset,
                max_members,
                max_records,
            } => {
                out.variant("DatasetRequest", "Snapshot", 3)?;
                out.text("dataset")?;
                dataset.encode(out)?;
                out.text("max_members")?;
                max_members.encode(out)?;
                out.text("max_records")?;
                max_records.encode(out)?;
                Ok(())
            }
            Self::Create {
                attributes,
                dataset,
                mutation,
            } => {
                out.variant("DatasetRequest", "Create", 3)?;
                out.text("attributes")?;
                attributes.encode(out)?;
                out.text("dataset")?;
                dataset.encode(out)?;
                out.text("mutation")?;
                mutation.encode(out)?;
                Ok(())
            }
            Self::Define {
                dataset,
                definition,
                mutation,
            } => {
                out.variant("DatasetRequest", "Define", 3)?;
                out.text("dataset")?;
                dataset.encode(out)?;
                out.text("definition")?;
                definition.encode(out)?;
                out.text("mutation")?;
                mutation.encode(out)?;
                Ok(())
            }
            Self::Alter {
                dataset,
                definition,
                expected_version,
                mutation,
            } => {
                out.variant("DatasetRequest", "Alter", 4)?;
                out.text("dataset")?;
                dataset.encode(out)?;
                out.text("definition")?;
                definition.encode(out)?;
                out.text("expected_version")?;
                expected_version.encode(out)?;
                out.text("mutation")?;
                mutation.encode(out)?;
                Ok(())
            }
            Self::SetLifecycle {
                dataset,
                expected_version,
                mutation,
                state,
            } => {
                out.variant("DatasetRequest", "SetLifecycle", 4)?;
                out.text("dataset")?;
                dataset.encode(out)?;
                out.text("expected_version")?;
                expected_version.encode(out)?;
                out.text("mutation")?;
                mutation.encode(out)?;
                out.text("state")?;
                state.encode(out)?;
                Ok(())
            }
            Self::RecordBackup {
                dataset,
                expected_version,
                mutation,
            } => {
                out.variant("DatasetRequest", "RecordBackup", 3)?;
                out.text("dataset")?;
                dataset.encode(out)?;
                out.text("expected_version")?;
                expected_version.encode(out)?;
                out.text("mutation")?;
                mutation.encode(out)?;
                Ok(())
            }
            Self::Restore {
                dataset,
                expected_version,
                mutation,
                snapshot,
            } => {
                out.variant("DatasetRequest", "Restore", 4)?;
                out.text("dataset")?;
                dataset.encode(out)?;
                out.text("expected_version")?;
                expected_version.encode(out)?;
                out.text("mutation")?;
                mutation.encode(out)?;
                out.text("snapshot")?;
                snapshot.encode(out)?;
                Ok(())
            }
            Self::DefineCatalog {
                catalog,
                kind,
                mutation,
            } => {
                out.variant("DatasetRequest", "DefineCatalog", 3)?;
                out.text("catalog")?;
                catalog.encode(out)?;
                out.text("kind")?;
                kind.encode(out)?;
                out.text("mutation")?;
                mutation.encode(out)?;
                Ok(())
            }
            Self::SetCatalogConnection {
                catalog,
                connected,
                expected_version,
                mutation,
            } => {
                out.variant("DatasetRequest", "SetCatalogConnection", 4)?;
                out.text("catalog")?;
                catalog.encode(out)?;
                out.text("connected")?;
                connected.encode(out)?;
                out.text("expected_version")?;
                expected_version.encode(out)?;
                out.text("mutation")?;
                mutation.encode(out)?;
                Ok(())
            }
            Self::DefineAlias {
                alias,
                mutation,
                target,
            } => {
                out.variant("DatasetRequest", "DefineAlias", 3)?;
                out.text("alias")?;
                alias.encode(out)?;
                out.text("mutation")?;
                mutation.encode(out)?;
                out.text("target")?;
                target.encode(out)?;
                Ok(())
            }
            Self::DefineMemberAlias {
                alias,
                dataset,
                expected_version,
                mutation,
                target,
            } => {
                out.variant("DatasetRequest", "DefineMemberAlias", 5)?;
                out.text("alias")?;
                alias.encode(out)?;
                out.text("dataset")?;
                dataset.encode(out)?;
                out.text("expected_version")?;
                expected_version.encode(out)?;
                out.text("mutation")?;
                mutation.encode(out)?;
                out.text("target")?;
                target.encode(out)?;
                Ok(())
            }
            Self::WriteMemberGeneration {
                dataset,
                expected_version,
                member,
                mutation,
                program_object,
                records,
            } => {
                out.variant("DatasetRequest", "WriteMemberGeneration", 6)?;
                out.text("dataset")?;
                dataset.encode(out)?;
                out.text("expected_version")?;
                expected_version.encode(out)?;
                out.text("member")?;
                member.encode(out)?;
                out.text("mutation")?;
                mutation.encode(out)?;
                out.text("program_object")?;
                program_object.encode(out)?;
                out.text("records")?;
                records.encode(out)?;
                Ok(())
            }
            Self::DeleteMemberGeneration {
                dataset,
                expected_version,
                generation,
                member,
                mutation,
            } => {
                out.variant("DatasetRequest", "DeleteMemberGeneration", 5)?;
                out.text("dataset")?;
                dataset.encode(out)?;
                out.text("expected_version")?;
                expected_version.encode(out)?;
                out.text("generation")?;
                generation.encode(out)?;
                out.text("member")?;
                member.encode(out)?;
                out.text("mutation")?;
                mutation.encode(out)?;
                Ok(())
            }
            Self::AcquireLock {
                dataset,
                lease_ticks,
                mode,
                mutation,
                now_tick,
                owner,
                target,
                transaction,
            } => {
                out.variant("DatasetRequest", "AcquireLock", 8)?;
                out.text("dataset")?;
                dataset.encode(out)?;
                out.text("lease_ticks")?;
                lease_ticks.encode(out)?;
                out.text("mode")?;
                mode.encode(out)?;
                out.text("mutation")?;
                mutation.encode(out)?;
                out.text("now_tick")?;
                now_tick.encode(out)?;
                out.text("owner")?;
                owner.encode(out)?;
                out.text("target")?;
                target.encode(out)?;
                out.text("transaction")?;
                transaction.encode(out)?;
                Ok(())
            }
            Self::ReleaseLock {
                dataset,
                lock_id,
                mutation,
                owner,
            } => {
                out.variant("DatasetRequest", "ReleaseLock", 4)?;
                out.text("dataset")?;
                dataset.encode(out)?;
                out.text("lock_id")?;
                lock_id.encode(out)?;
                out.text("mutation")?;
                mutation.encode(out)?;
                out.text("owner")?;
                owner.encode(out)?;
                Ok(())
            }
            Self::BeginTvs {
                mutation,
                owner,
                transaction,
            } => {
                out.variant("DatasetRequest", "BeginTvs", 3)?;
                out.text("mutation")?;
                mutation.encode(out)?;
                out.text("owner")?;
                owner.encode(out)?;
                out.text("transaction")?;
                transaction.encode(out)?;
                Ok(())
            }
            Self::StageTvs {
                mutation,
                operation,
                owner,
                transaction,
            } => {
                out.variant("DatasetRequest", "StageTvs", 4)?;
                out.text("mutation")?;
                mutation.encode(out)?;
                out.text("operation")?;
                operation.encode(out)?;
                out.text("owner")?;
                owner.encode(out)?;
                out.text("transaction")?;
                transaction.encode(out)?;
                Ok(())
            }
            Self::CompleteTvs {
                commit,
                mutation,
                owner,
                transaction,
            } => {
                out.variant("DatasetRequest", "CompleteTvs", 4)?;
                out.text("commit")?;
                commit.encode(out)?;
                out.text("mutation")?;
                mutation.encode(out)?;
                out.text("owner")?;
                owner.encode(out)?;
                out.text("transaction")?;
                transaction.encode(out)?;
                Ok(())
            }
            Self::ReconcileTvs {
                committed,
                mutation,
                owner,
                transaction,
            } => {
                out.variant("DatasetRequest", "ReconcileTvs", 4)?;
                out.text("committed")?;
                committed.encode(out)?;
                out.text("mutation")?;
                mutation.encode(out)?;
                out.text("owner")?;
                owner.encode(out)?;
                out.text("transaction")?;
                transaction.encode(out)?;
                Ok(())
            }
            Self::Write {
                dataset,
                expected_version,
                member,
                mutation,
                records,
            } => {
                out.variant("DatasetRequest", "Write", 5)?;
                out.text("dataset")?;
                dataset.encode(out)?;
                out.text("expected_version")?;
                expected_version.encode(out)?;
                out.text("member")?;
                member.encode(out)?;
                out.text("mutation")?;
                mutation.encode(out)?;
                out.text("records")?;
                records.encode(out)?;
                Ok(())
            }
            Self::Append {
                dataset,
                expected_version,
                member,
                mutation,
                records,
            } => {
                out.variant("DatasetRequest", "Append", 5)?;
                out.text("dataset")?;
                dataset.encode(out)?;
                out.text("expected_version")?;
                expected_version.encode(out)?;
                out.text("member")?;
                member.encode(out)?;
                out.text("mutation")?;
                mutation.encode(out)?;
                out.text("records")?;
                records.encode(out)?;
                Ok(())
            }
            Self::Truncate {
                dataset,
                expected_version,
                mutation,
            } => {
                out.variant("DatasetRequest", "Truncate", 3)?;
                out.text("dataset")?;
                dataset.encode(out)?;
                out.text("expected_version")?;
                expected_version.encode(out)?;
                out.text("mutation")?;
                mutation.encode(out)?;
                Ok(())
            }
            Self::RewriteRecord {
                dataset,
                expected_version,
                key,
                mutation,
                record,
            } => {
                out.variant("DatasetRequest", "RewriteRecord", 5)?;
                out.text("dataset")?;
                dataset.encode(out)?;
                out.text("expected_version")?;
                expected_version.encode(out)?;
                out.text("key")?;
                key.encode(out)?;
                out.text("mutation")?;
                mutation.encode(out)?;
                out.text("record")?;
                record.encode(out)?;
                Ok(())
            }
            Self::DeleteRecord {
                dataset,
                expected_version,
                key,
                mutation,
            } => {
                out.variant("DatasetRequest", "DeleteRecord", 4)?;
                out.text("dataset")?;
                dataset.encode(out)?;
                out.text("expected_version")?;
                expected_version.encode(out)?;
                out.text("key")?;
                key.encode(out)?;
                out.text("mutation")?;
                mutation.encode(out)?;
                Ok(())
            }
            Self::WriteRelative {
                dataset,
                expected_version,
                mutation,
                record,
                record_number,
            } => {
                out.variant("DatasetRequest", "WriteRelative", 5)?;
                out.text("dataset")?;
                dataset.encode(out)?;
                out.text("expected_version")?;
                expected_version.encode(out)?;
                out.text("mutation")?;
                mutation.encode(out)?;
                out.text("record")?;
                record.encode(out)?;
                out.text("record_number")?;
                record_number.encode(out)?;
                Ok(())
            }
            Self::DeleteRelative {
                dataset,
                expected_version,
                mutation,
                record_number,
            } => {
                out.variant("DatasetRequest", "DeleteRelative", 4)?;
                out.text("dataset")?;
                dataset.encode(out)?;
                out.text("expected_version")?;
                expected_version.encode(out)?;
                out.text("mutation")?;
                mutation.encode(out)?;
                out.text("record_number")?;
                record_number.encode(out)?;
                Ok(())
            }
            Self::WriteRba {
                data,
                dataset,
                expected_version,
                mutation,
                rba,
            } => {
                out.variant("DatasetRequest", "WriteRba", 5)?;
                out.text("data")?;
                data.encode(out)?;
                out.text("dataset")?;
                dataset.encode(out)?;
                out.text("expected_version")?;
                expected_version.encode(out)?;
                out.text("mutation")?;
                mutation.encode(out)?;
                out.text("rba")?;
                rba.encode(out)?;
                Ok(())
            }
            Self::DefineAlternateIndex {
                allow_duplicates,
                base,
                index,
                key_length,
                key_offset,
                mutation,
                upgrade,
            } => {
                out.variant("DatasetRequest", "DefineAlternateIndex", 7)?;
                out.text("allow_duplicates")?;
                allow_duplicates.encode(out)?;
                out.text("base")?;
                base.encode(out)?;
                out.text("index")?;
                index.encode(out)?;
                out.text("key_length")?;
                key_length.encode(out)?;
                out.text("key_offset")?;
                key_offset.encode(out)?;
                out.text("mutation")?;
                mutation.encode(out)?;
                out.text("upgrade")?;
                upgrade.encode(out)?;
                Ok(())
            }
            Self::BuildAlternateIndex {
                base,
                index,
                mutation,
            } => {
                out.variant("DatasetRequest", "BuildAlternateIndex", 3)?;
                out.text("base")?;
                base.encode(out)?;
                out.text("index")?;
                index.encode(out)?;
                out.text("mutation")?;
                mutation.encode(out)?;
                Ok(())
            }
            Self::DefinePath {
                index,
                mutation,
                path,
            } => {
                out.variant("DatasetRequest", "DefinePath", 3)?;
                out.text("index")?;
                index.encode(out)?;
                out.text("mutation")?;
                mutation.encode(out)?;
                out.text("path")?;
                path.encode(out)?;
                Ok(())
            }
            Self::DefineGenerationGroup {
                base,
                empty,
                limit,
                mutation,
                scratch,
            } => {
                out.variant("DatasetRequest", "DefineGenerationGroup", 5)?;
                out.text("base")?;
                base.encode(out)?;
                out.text("empty")?;
                empty.encode(out)?;
                out.text("limit")?;
                limit.encode(out)?;
                out.text("mutation")?;
                mutation.encode(out)?;
                out.text("scratch")?;
                scratch.encode(out)?;
                Ok(())
            }
            Self::CreateGeneration {
                attributes,
                base,
                mutation,
                records,
            } => {
                out.variant("DatasetRequest", "CreateGeneration", 4)?;
                out.text("attributes")?;
                attributes.encode(out)?;
                out.text("base")?;
                base.encode(out)?;
                out.text("mutation")?;
                mutation.encode(out)?;
                out.text("records")?;
                records.encode(out)?;
                Ok(())
            }
            Self::ResolveGeneration { base, relative } => {
                out.variant("DatasetRequest", "ResolveGeneration", 2)?;
                out.text("base")?;
                base.encode(out)?;
                out.text("relative")?;
                relative.encode(out)?;
                Ok(())
            }
            Self::Rename { from, mutation, to } => {
                out.variant("DatasetRequest", "Rename", 3)?;
                out.text("from")?;
                from.encode(out)?;
                out.text("mutation")?;
                mutation.encode(out)?;
                out.text("to")?;
                to.encode(out)?;
                Ok(())
            }
            Self::Delete {
                current_date,
                dataset,
                expected_version,
                member,
                mutation,
                purge,
            } => {
                out.variant("DatasetRequest", "Delete", 6)?;
                out.text("current_date")?;
                current_date.encode(out)?;
                out.text("dataset")?;
                dataset.encode(out)?;
                out.text("expected_version")?;
                expected_version.encode(out)?;
                out.text("member")?;
                member.encode(out)?;
                out.text("mutation")?;
                mutation.encode(out)?;
                out.text("purge")?;
                purge.encode(out)?;
                Ok(())
            }
            Self::StartBrowse {
                dataset,
                key,
                relation,
            } => {
                out.variant("DatasetRequest", "StartBrowse", 3)?;
                out.text("dataset")?;
                dataset.encode(out)?;
                out.text("key")?;
                key.encode(out)?;
                out.text("relation")?;
                relation.encode(out)
            }
            Self::ResetBrowse { .. } => browse::encode_browse_request(self, out),
            Self::ReadNext {
                control,
                cursor,
                dataset,
                reverse,
            } => {
                out.variant("DatasetRequest", "ReadNext", 4)?;
                out.text("control")?;
                control.encode(out)?;
                out.text("cursor")?;
                cursor.encode(out)?;
                out.text("dataset")?;
                dataset.encode(out)?;
                out.text("reverse")?;
                reverse.encode(out)?;
                Ok(())
            }
            Self::EndBrowse { cursor, dataset } => {
                out.variant("DatasetRequest", "EndBrowse", 2)?;
                out.text("cursor")?;
                cursor.encode(out)?;
                out.text("dataset")?;
                dataset.encode(out)?;
                Ok(())
            }
            Self::Close {
                control,
                cursor,
                dataset,
            } => {
                out.variant("DatasetRequest", "Close", 3)?;
                out.text("control")?;
                control.encode(out)?;
                out.text("cursor")?;
                cursor.encode(out)?;
                out.text("dataset")?;
                dataset.encode(out)?;
                Ok(())
            }
        }
    }
}

impl Canonical for DatasetResult {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        match self {
            Self::Capabilities { capabilities } => {
                out.variant("DatasetResult", "Capabilities", 1)?;
                out.text("capabilities")?;
                capabilities.encode(out)?;
                Ok(())
            }
            Self::Listed { more, names } => {
                out.variant("DatasetResult", "Listed", 2)?;
                out.text("more")?;
                more.encode(out)?;
                out.text("names")?;
                names.encode(out)?;
                Ok(())
            }
            Self::Members { more, names } => {
                out.variant("DatasetResult", "Members", 2)?;
                out.text("more")?;
                more.encode(out)?;
                out.text("names")?;
                names.encode(out)?;
                Ok(())
            }
            Self::Attributes {
                attributes,
                version,
            } => {
                out.variant("DatasetResult", "Attributes", 2)?;
                out.text("attributes")?;
                attributes.encode(out)?;
                out.text("version")?;
                version.encode(out)?;
                Ok(())
            }
            Self::Description(v0) => {
                out.variant("DatasetResult", "Description", 1)?;
                out.text("0")?;
                v0.encode(out)?;
                Ok(())
            }
            Self::Diagnostics { diagnostics } => {
                out.variant("DatasetResult", "Diagnostics", 1)?;
                out.text("diagnostics")?;
                diagnostics.encode(out)?;
                Ok(())
            }
            Self::Catalog(v0) => {
                out.variant("DatasetResult", "Catalog", 1)?;
                out.text("0")?;
                v0.encode(out)?;
                Ok(())
            }
            Self::CatalogEntries { entries, more } => {
                out.variant("DatasetResult", "CatalogEntries", 2)?;
                out.text("entries")?;
                entries.encode(out)?;
                out.text("more")?;
                more.encode(out)?;
                Ok(())
            }
            Self::Volumes { more, volumes } => {
                out.variant("DatasetResult", "Volumes", 2)?;
                out.text("more")?;
                more.encode(out)?;
                out.text("volumes")?;
                volumes.encode(out)?;
                Ok(())
            }
            Self::Locks { locks } => {
                out.variant("DatasetResult", "Locks", 1)?;
                out.text("locks")?;
                locks.encode(out)?;
                Ok(())
            }
            Self::Tvs(v0) => {
                out.variant("DatasetResult", "Tvs", 1)?;
                out.text("0")?;
                v0.encode(out)?;
                Ok(())
            }
            Self::Snapshot { snapshot, version } => {
                out.variant("DatasetResult", "Snapshot", 2)?;
                out.text("snapshot")?;
                snapshot.encode(out)?;
                out.text("version")?;
                version.encode(out)?;
                Ok(())
            }
            Self::Records {
                identities,
                records,
                version,
            } => {
                out.variant("DatasetResult", "Records", 3)?;
                out.text("identities")?;
                identities.encode(out)?;
                out.text("records")?;
                records.encode(out)?;
                out.text("version")?;
                version.encode(out)?;
                Ok(())
            }
            Self::MemberGeneration {
                generation,
                identities,
                program_object,
                records,
                version,
            } => {
                out.variant("DatasetResult", "MemberGeneration", 5)?;
                out.text("generation")?;
                generation.encode(out)?;
                out.text("identities")?;
                identities.encode(out)?;
                out.text("program_object")?;
                program_object.encode(out)?;
                out.text("records")?;
                records.encode(out)?;
                out.text("version")?;
                version.encode(out)?;
                Ok(())
            }
            Self::Rba {
                data,
                next_rba,
                rba,
                record,
                version,
            } => {
                out.variant("DatasetResult", "Rba", 5)?;
                out.text("data")?;
                data.encode(out)?;
                out.text("next_rba")?;
                next_rba.encode(out)?;
                out.text("rba")?;
                rba.encode(out)?;
                out.text("record")?;
                record.encode(out)?;
                out.text("version")?;
                version.encode(out)?;
                Ok(())
            }
            Self::Created { version } => {
                out.variant("DatasetResult", "Created", 1)?;
                out.text("version")?;
                version.encode(out)?;
                Ok(())
            }
            Self::Mutated { version } => {
                out.variant("DatasetResult", "Mutated", 1)?;
                out.text("version")?;
                version.encode(out)?;
                Ok(())
            }
            Self::Browse {
                cursor,
                identity,
                key,
                record,
            } => {
                out.variant("DatasetResult", "Browse", 4)?;
                out.text("cursor")?;
                cursor.encode(out)?;
                out.text("identity")?;
                identity.encode(out)?;
                out.text("key")?;
                key.encode(out)?;
                out.text("record")?;
                record.encode(out)?;
                Ok(())
            }
            Self::Generation {
                absolute_generation,
                dataset,
                version,
            } => {
                out.variant("DatasetResult", "Generation", 3)?;
                out.text("absolute_generation")?;
                absolute_generation.encode(out)?;
                out.text("dataset")?;
                dataset.encode(out)?;
                out.text("version")?;
                version.encode(out)?;
                Ok(())
            }
            Self::Condition { name, status } => {
                out.variant("DatasetResult", "Condition", 2)?;
                out.text("name")?;
                name.encode(out)?;
                out.text("status")?;
                status.encode(out)?;
                Ok(())
            }
        }
    }
}

impl Canonical for ProgramRequest {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        match self {
            Self::Inquire { program } => {
                out.variant("ProgramRequest", "Inquire", 1)?;
                out.text("program")?;
                program.encode(out)?;
                Ok(())
            }
            Self::Call {
                payload,
                program,
                service,
            } => {
                out.variant("ProgramRequest", "Call", 3)?;
                out.text("payload")?;
                payload.encode(out)?;
                out.text("program")?;
                program.encode(out)?;
                out.text("service")?;
                service.encode(out)?;
                Ok(())
            }
            Self::Invoke {
                class,
                method,
                payload,
                receiver,
            } => {
                out.variant("ProgramRequest", "Invoke", 4)?;
                out.text("class")?;
                class.encode(out)?;
                out.text("method")?;
                method.encode(out)?;
                out.text("payload")?;
                payload.encode(out)?;
                out.text("receiver")?;
                receiver.encode(out)?;
                Ok(())
            }
            Self::Link {
                payload,
                program,
                selection,
            } => {
                let selection = selection.as_ref();
                encode_program_link(out, payload, program, selection)
            }
            Self::Xctl { payload, program } => {
                out.variant("ProgramRequest", "Xctl", 2)?;
                out.text("payload")?;
                payload.encode(out)?;
                out.text("program")?;
                program.encode(out)?;
                Ok(())
            }
            Self::Return {
                next_transaction,
                payload,
            } => {
                out.variant("ProgramRequest", "Return", 2)?;
                out.text("next_transaction")?;
                next_transaction.encode(out)?;
                out.text("payload")?;
                payload.encode(out)?;
                Ok(())
            }
            Self::Cancel { programs } => {
                out.variant("ProgramRequest", "Cancel", 1)?;
                out.text("programs")?;
                programs.encode(out)?;
                Ok(())
            }
            Self::Abend { code } => {
                out.variant("ProgramRequest", "Abend", 1)?;
                out.text("code")?;
                code.encode(out)?;
                Ok(())
            }
        }
    }
}

impl Canonical for RuntimeServiceKind {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        match self {
            Self::LanguageEnvironment => {
                out.variant("RuntimeServiceKind", "LanguageEnvironment", 0)
            }
            Self::HostExtension => out.variant("RuntimeServiceKind", "HostExtension", 0),
        }
    }
}

impl Canonical for RuntimeServiceSelector {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        let Self {
            abi_version,
            kind,
            name,
        } = self;
        out.object("RuntimeServiceSelector", 3)?;
        out.text("abi_version")?;
        abi_version.encode(out)?;
        out.text("kind")?;
        kind.encode(out)?;
        out.text("name")?;
        name.encode(out)?;
        Ok(())
    }
}

impl Canonical for SpoolRequest {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        match self {
            Self::Append {
                file,
                job,
                mutation,
                records,
            } => {
                out.variant("SpoolRequest", "Append", 4)?;
                out.text("file")?;
                file.encode(out)?;
                out.text("job")?;
                job.encode(out)?;
                out.text("mutation")?;
                mutation.encode(out)?;
                out.text("records")?;
                records.encode(out)?;
                Ok(())
            }
            Self::List { job } => {
                out.variant("SpoolRequest", "List", 1)?;
                out.text("job")?;
                job.encode(out)?;
                Ok(())
            }
            Self::Read {
                file,
                job,
                max_records,
                start,
            } => {
                out.variant("SpoolRequest", "Read", 4)?;
                out.text("file")?;
                file.encode(out)?;
                out.text("job")?;
                job.encode(out)?;
                out.text("max_records")?;
                max_records.encode(out)?;
                out.text("start")?;
                start.encode(out)?;
                Ok(())
            }
            Self::Seal {
                file,
                job,
                mutation,
            } => {
                out.variant("SpoolRequest", "Seal", 3)?;
                out.text("file")?;
                file.encode(out)?;
                out.text("job")?;
                job.encode(out)?;
                out.text("mutation")?;
                mutation.encode(out)?;
                Ok(())
            }
            Self::Purge { job, mutation } => {
                out.variant("SpoolRequest", "Purge", 2)?;
                out.text("job")?;
                job.encode(out)?;
                out.text("mutation")?;
                mutation.encode(out)?;
                Ok(())
            }
        }
    }
}

impl Canonical for SpoolFileSummary {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        let Self {
            byte_count,
            file,
            record_count,
            sealed,
            version,
        } = self;
        out.object("SpoolFileSummary", 5)?;
        out.text("byte_count")?;
        byte_count.encode(out)?;
        out.text("file")?;
        file.encode(out)?;
        out.text("record_count")?;
        record_count.encode(out)?;
        out.text("sealed")?;
        sealed.encode(out)?;
        out.text("version")?;
        version.encode(out)?;
        Ok(())
    }
}

impl Canonical for SpoolResult {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        match self {
            Self::Mutated { replayed, version } => {
                out.variant("SpoolResult", "Mutated", 2)?;
                out.text("replayed")?;
                replayed.encode(out)?;
                out.text("version")?;
                version.encode(out)?;
                Ok(())
            }
            Self::Files { files } => {
                out.variant("SpoolResult", "Files", 1)?;
                out.text("files")?;
                files.encode(out)?;
                Ok(())
            }
            Self::Records {
                more,
                records,
                version,
            } => {
                out.variant("SpoolResult", "Records", 3)?;
                out.text("more")?;
                more.encode(out)?;
                out.text("records")?;
                records.encode(out)?;
                out.text("version")?;
                version.encode(out)?;
                Ok(())
            }
            Self::PurgePending {
                remaining_artifacts,
            } => {
                out.variant("SpoolResult", "PurgePending", 1)?;
                out.text("remaining_artifacts")?;
                remaining_artifacts.encode(out)?;
                Ok(())
            }
        }
    }
}

impl Canonical for TerminalField {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        let Self {
            column,
            length,
            modified,
            name,
            row,
            secret,
            value,
        } = self;
        out.object("TerminalField", 7)?;
        out.text("column")?;
        column.encode(out)?;
        out.text("length")?;
        length.encode(out)?;
        out.text("modified")?;
        modified.encode(out)?;
        out.text("name")?;
        name.encode(out)?;
        out.text("row")?;
        row.encode(out)?;
        out.text("secret")?;
        secret.encode(out)?;
        out.text("value")?;
        value.encode(out)?;
        Ok(())
    }
}

impl Canonical for TerminalRequest {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        match self {
            Self::Open {
                columns,
                rows,
                session,
            } => {
                out.variant("TerminalRequest", "Open", 3)?;
                out.text("columns")?;
                columns.encode(out)?;
                out.text("rows")?;
                rows.encode(out)?;
                out.text("session")?;
                session.encode(out)?;
                Ok(())
            }
            Self::Write {
                cursor,
                erase,
                fields,
                session,
            } => {
                out.variant("TerminalRequest", "Write", 4)?;
                out.text("cursor")?;
                cursor.encode(out)?;
                out.text("erase")?;
                erase.encode(out)?;
                out.text("fields")?;
                fields.encode(out)?;
                out.text("session")?;
                session.encode(out)?;
                Ok(())
            }
            Self::Read { session } => {
                out.variant("TerminalRequest", "Read", 1)?;
                out.text("session")?;
                session.encode(out)?;
                Ok(())
            }
            Self::Input {
                aid,
                fields,
                session,
            } => {
                out.variant("TerminalRequest", "Input", 3)?;
                out.text("aid")?;
                aid.encode(out)?;
                out.text("fields")?;
                fields.encode(out)?;
                out.text("session")?;
                session.encode(out)?;
                Ok(())
            }
            Self::Release { session } => {
                out.variant("TerminalRequest", "Release", 1)?;
                out.text("session")?;
                session.encode(out)?;
                Ok(())
            }
        }
    }
}

impl Canonical for SecurityRequest {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        match self {
            Self::Authenticate {
                credential_reference,
                user,
            } => {
                out.variant("SecurityRequest", "Authenticate", 2)?;
                out.text("credential_reference")?;
                credential_reference.encode(out)?;
                out.text("user")?;
                user.encode(out)?;
                Ok(())
            }
            Self::Authorize {
                class,
                intent,
                principal,
                resource,
            } => {
                out.variant("SecurityRequest", "Authorize", 4)?;
                out.text("class")?;
                class.encode(out)?;
                out.text("intent")?;
                intent.encode(out)?;
                out.text("principal")?;
                principal.encode(out)?;
                out.text("resource")?;
                resource.encode(out)?;
                Ok(())
            }
            Self::ValidatePrincipal { principal } => encode_principal_validation(out, principal),
            Self::Audit(v0) => {
                out.variant("SecurityRequest", "Audit", 1)?;
                out.text("0")?;
                v0.encode(out)?;
                Ok(())
            }
        }
    }
}

impl Canonical for SecurityDecision {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        match self {
            Self::Allow => out.variant("SecurityDecision", "Allow", 0),
            Self::Deny => out.variant("SecurityDecision", "Deny", 0),
            Self::NotFound => out.variant("SecurityDecision", "NotFound", 0),
            Self::InvalidCredentials => out.variant("SecurityDecision", "InvalidCredentials", 0),
            Self::Expired => out.variant("SecurityDecision", "Expired", 0),
            Self::Revoked => out.variant("SecurityDecision", "Revoked", 0),
            Self::Locked => out.variant("SecurityDecision", "Locked", 0),
        }
    }
}

impl Canonical for AuditEvent {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        let Self {
            action,
            decision,
            fields,
            resource_hash,
        } = self;
        out.object("AuditEvent", 4)?;
        out.text("action")?;
        action.encode(out)?;
        out.text("decision")?;
        decision.encode(out)?;
        out.text("fields")?;
        fields.encode(out)?;
        out.text("resource_hash")?;
        resource_hash.encode(out)?;
        Ok(())
    }
}

impl Canonical for ClockRequest {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        match self {
            Self::UtcTimestamp => out.variant("ClockRequest", "UtcTimestamp", 0),
            Self::Date => out.variant("ClockRequest", "Date", 0),
            Self::Time => out.variant("ClockRequest", "Time", 0),
        }
    }
}

impl Canonical for StateRequest {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        match self {
            Self::Get { key } => {
                out.variant("StateRequest", "Get", 1)?;
                out.text("key")?;
                key.encode(out)?;
                Ok(())
            }
            Self::Put {
                expected_version,
                key,
                mutation,
                value,
            } => {
                out.variant("StateRequest", "Put", 4)?;
                out.text("expected_version")?;
                expected_version.encode(out)?;
                out.text("key")?;
                key.encode(out)?;
                out.text("mutation")?;
                mutation.encode(out)?;
                out.text("value")?;
                value.encode(out)?;
                Ok(())
            }
            Self::Delete {
                expected_version,
                key,
                mutation,
            } => {
                out.variant("StateRequest", "Delete", 3)?;
                out.text("expected_version")?;
                expected_version.encode(out)?;
                out.text("key")?;
                key.encode(out)?;
                out.text("mutation")?;
                mutation.encode(out)?;
                Ok(())
            }
        }
    }
}

impl Canonical for Db2Operation {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        match self {
            Self::ExecuteScript => out.variant("Db2Operation", "ExecuteScript", 0),
            Self::FreePlans => out.variant("Db2Operation", "FreePlans", 0),
            Self::Select => out.variant("Db2Operation", "Select", 0),
            Self::Insert => out.variant("Db2Operation", "Insert", 0),
            Self::Update => out.variant("Db2Operation", "Update", 0),
            Self::Delete => out.variant("Db2Operation", "Delete", 0),
            Self::Count => out.variant("Db2Operation", "Count", 0),
            Self::DeclareCursor => out.variant("Db2Operation", "DeclareCursor", 0),
            Self::OpenCursor => out.variant("Db2Operation", "OpenCursor", 0),
            Self::FetchCursor => out.variant("Db2Operation", "FetchCursor", 0),
            Self::CloseCursor => out.variant("Db2Operation", "CloseCursor", 0),
            Self::Commit => out.variant("Db2Operation", "Commit", 0),
            Self::Rollback => out.variant("Db2Operation", "Rollback", 0),
            Self::Extract => out.variant("Db2Operation", "Extract", 0),
        }
    }
}

impl Canonical for Db2HostVariable {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        let Self { indicator, value } = self;
        out.object("Db2HostVariable", 2)?;
        out.text("indicator")?;
        indicator.encode(out)?;
        out.text("value")?;
        value.encode(out)?;
        Ok(())
    }
}

impl Canonical for Db2Request {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        let Self {
            cursor,
            inputs,
            max_rows,
            mutation,
            operation,
            outputs,
            statement,
        } = self;
        out.object("Db2Request", 7)?;
        out.text("cursor")?;
        cursor.encode(out)?;
        out.text("inputs")?;
        inputs.encode(out)?;
        out.text("max_rows")?;
        max_rows.encode(out)?;
        out.text("mutation")?;
        mutation.encode(out)?;
        out.text("operation")?;
        operation.encode(out)?;
        out.text("outputs")?;
        outputs.encode(out)?;
        out.text("statement")?;
        statement.encode(out)?;
        Ok(())
    }
}

impl Canonical for Db2Row {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        let Self { columns } = self;
        out.object("Db2Row", 1)?;
        out.text("columns")?;
        columns.encode(out)?;
        Ok(())
    }
}

impl Canonical for Db2Result {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        let Self {
            affected_rows,
            message,
            rows,
            sqlcode,
            sqlstate,
        } = self;
        out.object("Db2Result", 5)?;
        out.text("affected_rows")?;
        affected_rows.encode(out)?;
        out.text("message")?;
        message.encode(out)?;
        out.text("rows")?;
        rows.encode(out)?;
        out.text("sqlcode")?;
        sqlcode.encode(out)?;
        out.text("sqlstate")?;
        sqlstate.encode(out)?;
        Ok(())
    }
}

impl Canonical for ImsOperation {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        match self {
            Self::Schedule => out.variant("ImsOperation", "Schedule", 0),
            Self::Terminate => out.variant("ImsOperation", "Terminate", 0),
            Self::GetUnique => out.variant("ImsOperation", "GetUnique", 0),
            Self::GetNext => out.variant("ImsOperation", "GetNext", 0),
            Self::GetNextParent => out.variant("ImsOperation", "GetNextParent", 0),
            Self::GetHoldUnique => out.variant("ImsOperation", "GetHoldUnique", 0),
            Self::GetHoldNext => out.variant("ImsOperation", "GetHoldNext", 0),
            Self::GetHoldNextParent => out.variant("ImsOperation", "GetHoldNextParent", 0),
            Self::Insert => out.variant("ImsOperation", "Insert", 0),
            Self::Replace => out.variant("ImsOperation", "Replace", 0),
            Self::Delete => out.variant("ImsOperation", "Delete", 0),
            Self::Checkpoint => out.variant("ImsOperation", "Checkpoint", 0),
            Self::Load => out.variant("ImsOperation", "Load", 0),
            Self::Unload => out.variant("ImsOperation", "Unload", 0),
            Self::Commit => out.variant("ImsOperation", "Commit", 0),
            Self::Rollback => out.variant("ImsOperation", "Rollback", 0),
        }
    }
}

impl Canonical for ImsQualifier {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        let Self {
            field,
            segment,
            value,
        } = self;
        out.object("ImsQualifier", 3)?;
        out.text("field")?;
        field.encode(out)?;
        out.text("segment")?;
        segment.encode(out)?;
        out.text("value")?;
        value.encode(out)?;
        Ok(())
    }
}

impl Canonical for ImsRequest {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        let Self {
            checkpoint_id,
            data,
            max_segments,
            mutation,
            operation,
            pcb,
            psb,
            qualifiers,
            segments,
        } = self;
        out.object("ImsRequest", 9)?;
        out.text("checkpoint_id")?;
        checkpoint_id.encode(out)?;
        out.text("data")?;
        data.encode(out)?;
        out.text("max_segments")?;
        max_segments.encode(out)?;
        out.text("mutation")?;
        mutation.encode(out)?;
        out.text("operation")?;
        operation.encode(out)?;
        out.text("pcb")?;
        pcb.encode(out)?;
        out.text("psb")?;
        psb.encode(out)?;
        out.text("qualifiers")?;
        qualifiers.encode(out)?;
        out.text("segments")?;
        segments.encode(out)?;
        Ok(())
    }
}

impl Canonical for ImsSegment {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        let Self {
            data,
            name,
            parent_key,
        } = self;
        out.object("ImsSegment", 3)?;
        out.text("data")?;
        data.encode(out)?;
        out.text("name")?;
        name.encode(out)?;
        out.text("parent_key")?;
        parent_key.encode(out)?;
        Ok(())
    }
}

impl Canonical for ImsResult {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        let Self {
            affected_segments,
            checkpoint_id,
            segments,
            status,
        } = self;
        out.object("ImsResult", 4)?;
        out.text("affected_segments")?;
        affected_segments.encode(out)?;
        out.text("checkpoint_id")?;
        checkpoint_id.encode(out)?;
        out.text("segments")?;
        segments.encode(out)?;
        out.text("status")?;
        status.encode(out)?;
        Ok(())
    }
}

impl Canonical for MqOperation {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        match self {
            Self::Open => out.variant("MqOperation", "Open", 0),
            Self::Get => out.variant("MqOperation", "Get", 0),
            Self::Put => out.variant("MqOperation", "Put", 0),
            Self::PutOne => out.variant("MqOperation", "PutOne", 0),
            Self::Close => out.variant("MqOperation", "Close", 0),
            Self::Commit => out.variant("MqOperation", "Commit", 0),
            Self::Rollback => out.variant("MqOperation", "Rollback", 0),
        }
    }
}

impl Canonical for MqRequest {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        let Self {
            correlation_id,
            handle,
            max_message_bytes,
            message,
            message_id,
            mutation,
            operation,
            options,
            queue,
            wait_ticks,
        } = self;
        out.object("MqRequest", 10)?;
        out.text("correlation_id")?;
        correlation_id.encode(out)?;
        out.text("handle")?;
        handle.encode(out)?;
        out.text("max_message_bytes")?;
        max_message_bytes.encode(out)?;
        out.text("message")?;
        message.encode(out)?;
        out.text("message_id")?;
        message_id.encode(out)?;
        out.text("mutation")?;
        mutation.encode(out)?;
        out.text("operation")?;
        operation.encode(out)?;
        out.text("options")?;
        options.encode(out)?;
        out.text("queue")?;
        queue.encode(out)?;
        out.text("wait_ticks")?;
        wait_ticks.encode(out)?;
        Ok(())
    }
}

impl Canonical for MqResult {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        let Self {
            completion_code,
            correlation_id,
            handle,
            message,
            message_id,
            reason_code,
            trigger_program,
        } = self;
        out.object("MqResult", 7)?;
        out.text("completion_code")?;
        completion_code.encode(out)?;
        out.text("correlation_id")?;
        correlation_id.encode(out)?;
        out.text("handle")?;
        handle.encode(out)?;
        out.text("message")?;
        message.encode(out)?;
        out.text("message_id")?;
        message_id.encode(out)?;
        out.text("reason_code")?;
        reason_code.encode(out)?;
        out.text("trigger_program")?;
        trigger_program.encode(out)?;
        Ok(())
    }
}

impl Canonical for HostRequest {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        match self {
            Self::Dataset(v0) => {
                out.variant("HostRequest", "Dataset", 1)?;
                out.text("0")?;
                v0.encode(out)?;
                Ok(())
            }
            Self::Program(v0) => {
                out.variant("HostRequest", "Program", 1)?;
                out.text("0")?;
                v0.encode(out)?;
                Ok(())
            }
            Self::Spool(v0) => {
                out.variant("HostRequest", "Spool", 1)?;
                out.text("0")?;
                v0.encode(out)?;
                Ok(())
            }
            Self::Terminal(v0) => {
                out.variant("HostRequest", "Terminal", 1)?;
                out.text("0")?;
                v0.encode(out)?;
                Ok(())
            }
            Self::Security(v0) => {
                out.variant("HostRequest", "Security", 1)?;
                out.text("0")?;
                v0.encode(out)?;
                Ok(())
            }
            Self::Clock(v0) => {
                out.variant("HostRequest", "Clock", 1)?;
                out.text("0")?;
                v0.encode(out)?;
                Ok(())
            }
            Self::State(v0) => {
                out.variant("HostRequest", "State", 1)?;
                out.text("0")?;
                v0.encode(out)?;
                Ok(())
            }
            Self::Cics(v0) => {
                out.variant("HostRequest", "Cics", 1)?;
                out.text("0")?;
                v0.encode(out)?;
                Ok(())
            }
            Self::Db2(v0) => {
                out.variant("HostRequest", "Db2", 1)?;
                out.text("0")?;
                v0.encode(out)?;
                Ok(())
            }
            Self::Ims(v0) => {
                out.variant("HostRequest", "Ims", 1)?;
                out.text("0")?;
                v0.encode(out)?;
                Ok(())
            }
            Self::Mq(v0) => {
                out.variant("HostRequest", "Mq", 1)?;
                out.text("0")?;
                v0.encode(out)?;
                Ok(())
            }
        }
    }
}

impl Canonical for HostResult {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        match self {
            Self::Dataset(v0) => {
                out.variant("HostResult", "Dataset", 1)?;
                out.text("0")?;
                v0.encode(out)?;
                Ok(())
            }
            Self::Program(v0) => {
                out.variant("HostResult", "Program", 1)?;
                out.text("0")?;
                v0.encode(out)?;
                Ok(())
            }
            Self::Spool(v0) => {
                out.variant("HostResult", "Spool", 1)?;
                out.text("0")?;
                v0.encode(out)?;
                Ok(())
            }
            Self::Terminal(v0) => {
                out.variant("HostResult", "Terminal", 1)?;
                out.text("0")?;
                v0.encode(out)?;
                Ok(())
            }
            Self::Security(v0) => {
                out.variant("HostResult", "Security", 1)?;
                out.text("0")?;
                v0.encode(out)?;
                Ok(())
            }
            Self::Clock(v0) => {
                out.variant("HostResult", "Clock", 1)?;
                out.text("0")?;
                v0.encode(out)?;
                Ok(())
            }
            Self::State { value, version } => {
                out.variant("HostResult", "State", 2)?;
                out.text("value")?;
                value.encode(out)?;
                out.text("version")?;
                version.encode(out)?;
                Ok(())
            }
            Self::Cics(v0) => {
                out.variant("HostResult", "Cics", 1)?;
                out.text("0")?;
                v0.encode(out)?;
                Ok(())
            }
            Self::Db2(v0) => {
                out.variant("HostResult", "Db2", 1)?;
                out.text("0")?;
                v0.encode(out)?;
                Ok(())
            }
            Self::Ims(v0) => {
                out.variant("HostResult", "Ims", 1)?;
                out.text("0")?;
                v0.encode(out)?;
                Ok(())
            }
            Self::Mq(v0) => {
                out.variant("HostResult", "Mq", 1)?;
                out.text("0")?;
                v0.encode(out)?;
                Ok(())
            }
        }
    }
}

impl Canonical for EffectRequest {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        let Self {
            deadline_tick,
            idempotency_key,
            request,
            run_unit,
            sequence,
        } = self;
        out.object("EffectRequest", 5)?;
        out.text("deadline_tick")?;
        deadline_tick.encode(out)?;
        out.text("idempotency_key")?;
        idempotency_key.encode(out)?;
        out.text("request")?;
        request.encode(out)?;
        out.text("run_unit")?;
        run_unit.encode(out)?;
        out.text("sequence")?;
        sequence.encode(out)?;
        Ok(())
    }
}

impl Canonical for EffectResult {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        let Self { outcome, sequence } = self;
        out.object("EffectResult", 2)?;
        out.text("outcome")?;
        outcome.encode(out)?;
        out.text("sequence")?;
        sequence.encode(out)?;
        Ok(())
    }
}

impl Canonical for HostProblem {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        match self {
            Self::Malformed => out.variant("HostProblem", "Malformed", 0),
            Self::Unsupported => out.variant("HostProblem", "Unsupported", 0),
            Self::UnsupportedCapability { capability, detail } => {
                out.variant("HostProblem", "UnsupportedCapability", 2)?;
                out.text("capability")?;
                capability.encode(out)?;
                out.text("detail")?;
                detail.encode(out)?;
                Ok(())
            }
            Self::NotFound => out.variant("HostProblem", "NotFound", 0),
            Self::Condition {
                name,
                response,
                response2,
            } => {
                out.variant("HostProblem", "Condition", 3)?;
                out.text("name")?;
                name.encode(out)?;
                out.text("response")?;
                response.encode(out)?;
                out.text("response2")?;
                response2.encode(out)?;
                Ok(())
            }
            Self::Unauthorized => out.variant("HostProblem", "Unauthorized", 0),
            Self::Cancelled => out.variant("HostProblem", "Cancelled", 0),
            Self::TimedOut => out.variant("HostProblem", "TimedOut", 0),
            Self::ResourceExhausted => out.variant("HostProblem", "ResourceExhausted", 0),
            Self::ProviderFailure => out.variant("HostProblem", "ProviderFailure", 0),
            Self::InfrastructureFailure => out.variant("HostProblem", "InfrastructureFailure", 0),
            Self::MissingIdempotency => out.variant("HostProblem", "MissingIdempotency", 0),
            Self::IdempotencyConflict => out.variant("HostProblem", "IdempotencyConflict", 0),
            Self::UnknownOutcome => out.variant("HostProblem", "UnknownOutcome", 0),
        }
    }
}
