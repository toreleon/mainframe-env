//! Package resource preflight and existing optional semantic validation.
use super::*;

pub(super) fn validate_aggregate_bounds(
    package: &ApplicationPackageV2,
    limits: PackageLimits,
) -> Result<PackageFootprint, InstallProblem> {
    let footprint = validate_identity_bounds(package, limits)?;
    let sections = &package.sections;
    if let Some(metadata) = &sections.ims_metadata {
        validate_ims_metadata(metadata, ImsMetadataLimits::default()).map_err(|problem| {
            if problem == ImsMetadataProblem::LimitExceeded {
                InstallProblem::LimitExceeded
            } else {
                InstallProblem::MissingReference
            }
        })?;
    }
    if let Some(definitions) = &sections.ims_tm {
        definitions
            .validate(TmLimits::default())
            .map_err(|problem| {
                if problem == mainframe_env_host_api::HostProblem::ResourceExhausted {
                    InstallProblem::LimitExceeded
                } else {
                    InstallProblem::MissingReference
                }
            })?;
    }
    Ok(footprint)
}

/// Resource preflight for identity producers; graph validity remains admission-owned.
pub(super) fn validate_identity_bounds(
    package: &ApplicationPackageV2,
    limits: PackageLimits,
) -> Result<PackageFootprint, InstallProblem> {
    let sections = &package.sections;
    let section_counts = [
        sections.host_abi_libraries.len(),
        sections.sql_tables.len(),
        sections.sql_rows.len(),
        sections.ims_definitions.len(),
        sections.ims_rows.len(),
        usize::from(sections.ims_metadata.is_some()),
        usize::from(sections.ims_tm.is_some()),
        sections.mq_resources.len(),
        sections.batch_controllers.len(),
        sections.security_resources.len(),
    ];
    if limits.max_sections < APPLICATION_SECTION_COUNT
        || section_counts
            .iter()
            .any(|count| *count > limits.max_items_per_section)
        || bounded_sum(section_counts, limits.max_total_nested_items).is_err()
    {
        return Err(InstallProblem::LimitExceeded);
    }
    if package.base.manifest.entries.len() > limits.max_manifest_entries
        || package.base.blobs.len() > limits.max_manifest_entries
        || package
            .base
            .manifest
            .entries
            .iter()
            .any(|entry| entry.depends_on.len() > limits.max_dependencies_per_entry)
    {
        return Err(InstallProblem::LimitExceeded);
    }
    validate_preflight_text(package, limits)?;
    let blob_bytes = bounded_sum(
        package.base.blobs.values().map(Vec::len),
        limits.max_total_blob_bytes,
    )?;
    if sections
        .host_abi_libraries
        .iter()
        .any(|library| library.members.len() > limits.max_members_per_abi_library)
        || sections.sql_tables.iter().any(|table| {
            table.columns.len() > limits.max_columns_per_sql_table
                || table.primary_key.len() > limits.max_key_columns_per_sql_table
        })
        || sections
            .ims_definitions
            .iter()
            .any(|definition| definition.segments.len() > limits.max_segments_per_ims_definition)
        || sections
            .batch_controllers
            .iter()
            .any(|controller| controller.properties.len() > limits.max_properties_per_controller)
    {
        return Err(InstallProblem::LimitExceeded);
    }
    let nested_items = bounded_sum(
        package
            .base
            .manifest
            .entries
            .iter()
            .map(|entry| entry.depends_on.len())
            .chain(
                sections
                    .host_abi_libraries
                    .iter()
                    .map(|item| item.members.len()),
            )
            .chain(
                sections
                    .sql_tables
                    .iter()
                    .flat_map(|item| [item.columns.len(), item.primary_key.len()]),
            )
            .chain(sections.sql_rows.iter().map(|item| item.values.len()))
            .chain(
                sections
                    .ims_definitions
                    .iter()
                    .map(|item| item.segments.len()),
            )
            .chain(sections.ims_rows.iter().map(|item| item.values.len()))
            .chain(std::iter::once(metadata_nested_items(
                sections.ims_metadata.as_ref(),
            )?))
            .chain(std::iter::once(tm_nested_items(sections.ims_tm.as_ref())?))
            .chain(sections.mq_resources.iter().map(|_| 3))
            .chain(
                sections
                    .batch_controllers
                    .iter()
                    .map(|item| item.properties.len()),
            )
            .chain(sections.security_resources.iter().map(|_| 3))
            .chain(
                section_counts
                    .into_iter()
                    .enumerate()
                    .filter_map(|(index, count)| {
                        (!matches!(index, 5 | 6 | 7 | 9)).then_some(count)
                    }),
            )
            .chain(std::iter::once(package.base.manifest.entries.len()))
            .chain(std::iter::once(package.base.blobs.len()))
            .chain(std::iter::once(16)),
        limits.max_total_nested_items,
    )?;
    validate_optional_text(package)?;
    let text_bytes = bounded_sum(
        section_text_lengths(package),
        limits.max_total_section_bytes,
    )?;
    let metadata_bytes = sections.ims_metadata.as_ref().map_or(Ok(0), |metadata| {
        super::bounded_codec::json_size(metadata, limits.max_total_section_bytes - text_bytes)
    })?;
    let tm_bytes = sections.ims_tm.as_ref().map_or(Ok(0), |definitions| {
        super::bounded_codec::json_size(
            definitions,
            limits.max_total_section_bytes - text_bytes - metadata_bytes,
        )
    })?;
    let section_bytes = bounded_sum(
        [text_bytes, metadata_bytes, tm_bytes],
        limits.max_total_section_bytes,
    )?;
    let structural_bytes = nested_items
        .checked_mul(256)
        .ok_or(InstallProblem::LimitExceeded)?;
    let bytes = blob_bytes
        .checked_add(section_bytes)
        .and_then(|bytes| bytes.checked_add(structural_bytes))
        .ok_or(InstallProblem::LimitExceeded)?;
    if bytes
        > limits
            .max_retained_package_bytes
            .min(limits.max_total_retained_package_bytes)
        || nested_items
            > limits
                .max_retained_nested_items
                .min(limits.max_total_retained_nested_items)
    {
        return Err(InstallProblem::LimitExceeded);
    }
    let mut sql_rows = BTreeMap::<String, usize>::new();
    for row in &sections.sql_rows {
        let count = sql_rows.entry(row.table.to_ascii_uppercase()).or_default();
        *count = count.checked_add(1).ok_or(InstallProblem::LimitExceeded)?;
        if *count > limits.max_rows_per_sql_table {
            return Err(InstallProblem::LimitExceeded);
        }
    }
    let mut ims_rows = BTreeMap::<String, usize>::new();
    for row in &sections.ims_rows {
        let count = ims_rows
            .entry(row.definition.to_ascii_uppercase())
            .or_default();
        *count = count.checked_add(1).ok_or(InstallProblem::LimitExceeded)?;
        if *count > limits.max_rows_per_ims_definition {
            return Err(InstallProblem::LimitExceeded);
        }
    }
    Ok(PackageFootprint {
        bytes,
        items: nested_items,
    })
}

pub(super) fn validate_preflight_text(
    package: &ApplicationPackageV2,
    limits: PackageLimits,
) -> Result<(), InstallProblem> {
    for value in [
        package.base.manifest.name.as_str(),
        package.base.manifest.version.as_str(),
        package.base.manifest.target_product.as_str(),
        package.sections.schema_version.as_str(),
        package.signature.algorithm.as_str(),
        package.signature.key_id.as_str(),
        package.signature.value.as_str(),
    ] {
        bounded_text(value, 256)?;
    }
    for digest in package.base.blobs.keys() {
        bounded_text(digest, 71)?;
    }
    for entry in &package.base.manifest.entries {
        bounded_text(&entry.path, 4_096)?;
        bounded_text(&entry.sha256, 71)?;
        for dependency in &entry.depends_on {
            bounded_text(dependency, 4_096)?;
        }
    }
    for library in &package.sections.host_abi_libraries {
        bounded_text(&library.id, 256)?;
        bounded_text(&library.version, 256)?;
        for member in &library.members {
            bounded_text(&member.name, 256)?;
            bounded_text(&member.blob_sha256, 71)?;
        }
    }
    for table in &package.sections.sql_tables {
        bounded_text(&table.name, 256)?;
        for column in &table.columns {
            bounded_text(&column.name, 256)?;
        }
        for key in &table.primary_key {
            bounded_text(key, 256)?;
        }
    }
    for row in &package.sections.sql_rows {
        bounded_text(&row.table, 256)?;
        bounded_values(&row.values, limits)?;
    }
    for definition in &package.sections.ims_definitions {
        bounded_text(&definition.name, 256)?;
        for segment in &definition.segments {
            bounded_text(segment, 256)?;
        }
    }
    for row in &package.sections.ims_rows {
        bounded_text(&row.definition, 256)?;
        bounded_text(&row.segment, 256)?;
        bounded_values(&row.values, limits)?;
    }
    for resource in &package.sections.mq_resources {
        bounded_text(&resource.name, 256)?;
        if let Some(target) = &resource.target {
            bounded_text(target, 256)?;
        }
        if let Some(controller) = &resource.controller {
            bounded_text(controller, 256)?;
        }
    }
    for controller in &package.sections.batch_controllers {
        bounded_text(&controller.name, 256)?;
        bounded_text(&controller.program, 4_096)?;
        bounded_values(&controller.properties, limits)?;
    }
    for resource in &package.sections.security_resources {
        bounded_text(&resource.class, 256)?;
        bounded_text(&resource.profile, 256)?;
        bounded_text(&resource.owner, 256)?;
    }
    Ok(())
}

pub(super) fn metadata_nested_items(
    metadata: Option<&ImsMetadataCatalog>,
) -> Result<usize, InstallProblem> {
    let Some(metadata) = metadata else {
        return Ok(0);
    };
    let database_items = metadata
        .databases
        .iter()
        .try_fold(0usize, |total, database| {
            let segment_items = database
                .segments
                .iter()
                .try_fold(0usize, |total, segment| {
                    total
                        .checked_add(1)
                        .and_then(|total| total.checked_add(segment.fields.len()))
                        .ok_or(InstallProblem::LimitExceeded)
                })?;
            total
                .checked_add(1)
                .and_then(|total| total.checked_add(segment_items))
                .and_then(|total| total.checked_add(database.secondary_indexes.len()))
                .and_then(|total| total.checked_add(database.logical_relationships.len()))
                .ok_or(InstallProblem::LimitExceeded)
        })?;
    let psb_items = metadata.psbs.iter().try_fold(0usize, |total, psb| {
        let pcb_items = psb.pcbs.iter().try_fold(0usize, |total, pcb| {
            let sensitive = match pcb {
                mainframe_env_host_api::ImsPcbMetadata::Database(pcb) => {
                    pcb.sensitive_segments.len()
                }
                mainframe_env_host_api::ImsPcbMetadata::AlternateTerminal(_) => 0,
            };
            total
                .checked_add(1)
                .and_then(|total| total.checked_add(sensitive))
                .ok_or(InstallProblem::LimitExceeded)
        })?;
        total
            .checked_add(1)
            .and_then(|total| total.checked_add(pcb_items))
            .ok_or(InstallProblem::LimitExceeded)
    })?;
    database_items
        .checked_add(psb_items)
        .and_then(|total| total.checked_add(1))
        .ok_or(InstallProblem::LimitExceeded)
}

pub(super) fn bounded_values(
    values: &BTreeMap<String, String>,
    limits: PackageLimits,
) -> Result<(), InstallProblem> {
    if values.len() > limits.max_fields_per_record {
        return Err(InstallProblem::LimitExceeded);
    }
    for (name, value) in values {
        bounded_text(name, 256)?;
        bounded_text(value, limits.max_value_bytes)?;
    }
    Ok(())
}

fn tm_nested_items(definitions: Option<&TmDefinitionSet>) -> Result<usize, InstallProblem> {
    let Some(definitions) = definitions else {
        return Ok(0);
    };
    let limits = TmLimits::default();
    if definitions.transactions.len() > limits.max_transactions
        || definitions
            .transactions
            .iter()
            .any(|item| item.alternate_pcbs.len() > limits.max_alternate_pcbs)
    {
        return Err(InstallProblem::LimitExceeded);
    }
    bounded_sum(
        std::iter::once(1).chain(
            definitions
                .transactions
                .iter()
                .flat_map(|item| [1, item.alternate_pcbs.len()]),
        ),
        usize::MAX,
    )
}

fn validate_optional_text(package: &ApplicationPackageV2) -> Result<(), InstallProblem> {
    if let Some(metadata) = &package.sections.ims_metadata {
        bounded_text(&metadata.schema_version, 256)?;
        for database in &metadata.databases {
            bounded_text(&database.name, 256)?;
            for segment in &database.segments {
                bounded_text(&segment.name, 256)?;
                for text in segment.parent.iter().chain(
                    segment
                        .fields
                        .iter()
                        .filter_map(|field| field.name.as_ref()),
                ) {
                    bounded_text(text, 256)?;
                }
            }
            for index in &database.secondary_indexes {
                for text in [&index.name, &index.target_segment, &index.source_segment]
                    .into_iter()
                    .chain(index.source_fields.iter())
                {
                    bounded_text(text, 256)?;
                }
            }
            for relationship in &database.logical_relationships {
                for text in [
                    &relationship.parent_database,
                    &relationship.parent_segment,
                    &relationship.child_database,
                    &relationship.child_segment,
                ] {
                    bounded_text(text, 256)?;
                }
            }
        }
        for psb in &metadata.psbs {
            bounded_text(&psb.name, 256)?;
            for pcb in &psb.pcbs {
                match pcb {
                    ImsPcbMetadata::Database(pcb) => {
                        for text in [&pcb.name, &pcb.database, &pcb.processing_options]
                            .into_iter()
                            .chain(pcb.secondary_index.iter())
                        {
                            bounded_text(text, 256)?;
                        }
                        for segment in &pcb.sensitive_segments {
                            for text in std::iter::once(&segment.name)
                                .chain(segment.parent.iter())
                                .chain(segment.processing_options.iter())
                            {
                                bounded_text(text, 256)?;
                            }
                        }
                    }
                    ImsPcbMetadata::AlternateTerminal(pcb) => {
                        for text in std::iter::once(&pcb.name).chain(pcb.destination.iter()) {
                            bounded_text(text, 256)?;
                        }
                    }
                }
            }
        }
    }
    if let Some(definitions) = &package.sections.ims_tm {
        for transaction in &definitions.transactions {
            for text in [
                &transaction.code,
                &transaction.psb,
                &transaction.program_selector,
                &transaction.artifact,
                &transaction.required_generation,
            ] {
                bounded_text(text, 256)?;
            }
            for pcb in &transaction.alternate_pcbs {
                bounded_text(&pcb.name, 256)?;
                if let TmDestination::Fixed(destination) = &pcb.destination {
                    bounded_text(destination, 256)?;
                }
            }
        }
    }
    Ok(())
}

pub(super) fn bounded_text(value: &str, maximum: usize) -> Result<(), InstallProblem> {
    if value.len() > maximum {
        Err(InstallProblem::LimitExceeded)
    } else {
        Ok(())
    }
}

pub(super) fn section_text_lengths(
    package: &ApplicationPackageV2,
) -> impl Iterator<Item = usize> + '_ {
    let sections = &package.sections;
    [
        package.base.manifest.name.len(),
        package.base.manifest.version.len(),
        package.base.manifest.target_product.len(),
        sections.schema_version.len(),
        package.signature.algorithm.len(),
        package.signature.key_id.len(),
        package.signature.value.len(),
    ]
    .into_iter()
    .chain(package.base.blobs.keys().map(String::len))
    .chain(package.base.manifest.entries.iter().flat_map(|entry| {
        std::iter::once(entry.path.len())
            .chain(std::iter::once(entry.sha256.len()))
            .chain(entry.depends_on.iter().map(String::len))
    }))
    .chain(sections.host_abi_libraries.iter().flat_map(|library| {
        [library.id.len(), library.version.len()].into_iter().chain(
            library
                .members
                .iter()
                .flat_map(|member| [member.name.len(), member.blob_sha256.len()]),
        )
    }))
    .chain(sections.sql_tables.iter().flat_map(|table| {
        std::iter::once(table.name.len())
            .chain(table.columns.iter().map(|column| column.name.len()))
            .chain(table.primary_key.iter().map(String::len))
    }))
    .chain(sections.sql_rows.iter().flat_map(|row| {
        std::iter::once(row.table.len()).chain(
            row.values
                .iter()
                .flat_map(|(name, value)| [name.len(), value.len()]),
        )
    }))
    .chain(sections.ims_definitions.iter().flat_map(|definition| {
        std::iter::once(definition.name.len()).chain(definition.segments.iter().map(String::len))
    }))
    .chain(sections.ims_rows.iter().flat_map(|row| {
        [row.definition.len(), row.segment.len()].into_iter().chain(
            row.values
                .iter()
                .flat_map(|(name, value)| [name.len(), value.len()]),
        )
    }))
    .chain(sections.mq_resources.iter().flat_map(|resource| {
        std::iter::once(resource.name.len())
            .chain(resource.target.iter().map(String::len))
            .chain(resource.controller.iter().map(String::len))
    }))
    .chain(sections.batch_controllers.iter().flat_map(|controller| {
        [controller.name.len(), controller.program.len()]
            .into_iter()
            .chain(
                controller
                    .properties
                    .iter()
                    .flat_map(|(name, value)| [name.len(), value.len()]),
            )
    }))
    .chain(sections.security_resources.iter().flat_map(|resource| {
        [
            resource.class.len(),
            resource.profile.len(),
            resource.owner.len(),
        ]
    }))
}

pub(super) fn bounded_sum(
    values: impl IntoIterator<Item = usize>,
    maximum: usize,
) -> Result<usize, InstallProblem> {
    let mut total = 0usize;
    for value in values {
        total = total
            .checked_add(value)
            .filter(|total| *total <= maximum)
            .ok_or(InstallProblem::LimitExceeded)?;
    }
    Ok(total)
}
