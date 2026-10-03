//! Existing application-section reference validation.

use super::*;

pub(super) fn validate_sections(
    package: &ApplicationPackageV2,
    limits: PackageLimits,
) -> Result<(), InstallProblem> {
    let sections = &package.sections;
    let program_paths = package
        .base
        .manifest
        .entries
        .iter()
        .filter(|entry| entry.kind == EntryKind::Program)
        .map(|entry| entry.path.as_str())
        .collect::<BTreeSet<_>>();
    let manifest_blobs = package
        .base
        .manifest
        .entries
        .iter()
        .map(|entry| entry.sha256.as_str())
        .collect::<BTreeSet<_>>();
    let mut abi_ids = BTreeSet::new();
    for library in &sections.host_abi_libraries {
        validate_text(&library.id)?;
        validate_text(&library.version)?;
        if !abi_ids.insert(library.id.to_ascii_uppercase()) || library.members.is_empty() {
            return Err(InstallProblem::DuplicateEntry);
        }
        let mut members = BTreeSet::new();
        for member in &library.members {
            validate_text(&member.name)?;
            validate_sha256(&member.blob_sha256)?;
            if !members.insert(member.name.to_ascii_uppercase())
                || !manifest_blobs.contains(member.blob_sha256.as_str())
            {
                return Err(InstallProblem::MissingReference);
            }
        }
    }
    let mut table_names = BTreeSet::new();
    let mut table_columns = BTreeMap::new();
    let mut required_columns = BTreeMap::new();
    for table in &sections.sql_tables {
        validate_text(&table.name)?;
        let normalized = table.name.to_ascii_uppercase();
        if !table_names.insert(normalized.clone()) || table.columns.is_empty() {
            return Err(InstallProblem::DuplicateEntry);
        }
        let mut columns = BTreeSet::new();
        let mut required = BTreeSet::new();
        for column in &table.columns {
            validate_text(&column.name)?;
            let column_name = column.name.to_ascii_uppercase();
            if !columns.insert(column_name.clone()) {
                return Err(InstallProblem::DuplicateEntry);
            }
            if !column.nullable {
                required.insert(column_name);
            }
        }
        if table.primary_key.is_empty()
            || table
                .primary_key
                .iter()
                .any(|key| !columns.contains(&key.to_ascii_uppercase()))
        {
            return Err(InstallProblem::MissingReference);
        }
        table_columns.insert(normalized.clone(), columns);
        required_columns.insert(normalized, required);
    }
    for row in &sections.sql_rows {
        let table = row.table.to_ascii_uppercase();
        validate_values(&row.values, limits)?;
        let columns = table_columns
            .get(&table)
            .ok_or(InstallProblem::MissingReference)?;
        let present = row
            .values
            .keys()
            .map(|column| column.to_ascii_uppercase())
            .collect::<BTreeSet<_>>();
        if !present.is_subset(columns)
            || !required_columns
                .get(&table)
                .is_some_and(|required| required.is_subset(&present))
        {
            return Err(InstallProblem::MissingReference);
        }
    }
    let mut ims = BTreeMap::new();
    for definition in &sections.ims_definitions {
        validate_text(&definition.name)?;
        if definition.segments.is_empty()
            || ims
                .insert(definition.name.to_ascii_uppercase(), &definition.segments)
                .is_some()
        {
            return Err(InstallProblem::DuplicateEntry);
        }
        for segment in &definition.segments {
            validate_text(segment)?;
        }
    }
    for row in &sections.ims_rows {
        validate_values(&row.values, limits)?;
        if !ims
            .get(&row.definition.to_ascii_uppercase())
            .is_some_and(|segments| segments.contains(&row.segment))
        {
            return Err(InstallProblem::MissingReference);
        }
    }
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
        let metadata = sections
            .ims_metadata
            .as_ref()
            .ok_or(InstallProblem::MissingReference)?;
        for transaction in &definitions.transactions {
            let psb = metadata
                .psbs
                .iter()
                .find(|psb| psb.name == transaction.psb)
                .ok_or(InstallProblem::MissingReference)?;
            if !package.base.manifest.entries.iter().any(|entry| {
                entry.kind == EntryKind::Program
                    && entry.path == transaction.program_selector
                    && entry.sha256 == transaction.artifact
            }) {
                return Err(InstallProblem::MissingReference);
            }
            for alternate in &transaction.alternate_pcbs {
                let matched = psb.pcbs.iter().any(|pcb| match pcb {
                    ImsPcbMetadata::AlternateTerminal(pcb) => {
                        pcb.name == alternate.name
                            && pcb.express == alternate.express
                            && match &alternate.destination {
                                TmDestination::Fixed(destination) => {
                                    !pcb.modifiable
                                        && pcb.destination.as_deref() == Some(destination)
                                }
                                TmDestination::Modifiable => pcb.modifiable,
                            }
                    }
                    ImsPcbMetadata::Database(_) => false,
                });
                if !matched {
                    return Err(InstallProblem::MissingReference);
                }
            }
        }
    }
    let controllers = sections
        .batch_controllers
        .iter()
        .map(|controller| controller.name.to_ascii_uppercase())
        .collect::<BTreeSet<_>>();
    if controllers.len() != sections.batch_controllers.len() {
        return Err(InstallProblem::DuplicateEntry);
    }
    for controller in &sections.batch_controllers {
        validate_text(&controller.name)?;
        validate_values(&controller.properties, limits)?;
        if !program_paths.contains(controller.program.as_str()) {
            return Err(InstallProblem::MissingReference);
        }
    }
    let resources = sections
        .mq_resources
        .iter()
        .map(|resource| resource.name.to_ascii_uppercase())
        .collect::<BTreeSet<_>>();
    if resources.len() != sections.mq_resources.len() {
        return Err(InstallProblem::DuplicateEntry);
    }
    for resource in &sections.mq_resources {
        validate_text(&resource.name)?;
        if resource
            .target
            .as_ref()
            .is_some_and(|target| !resources.contains(&target.to_ascii_uppercase()))
            || resource
                .controller
                .as_ref()
                .is_some_and(|controller| !controllers.contains(&controller.to_ascii_uppercase()))
        {
            return Err(InstallProblem::MissingReference);
        }
    }
    let mut security = BTreeSet::new();
    for resource in &sections.security_resources {
        validate_text(&resource.class)?;
        validate_text(&resource.profile)?;
        validate_text(&resource.owner)?;
        if !security.insert((
            resource.class.to_ascii_uppercase(),
            resource.profile.to_ascii_uppercase(),
        )) {
            return Err(InstallProblem::DuplicateEntry);
        }
    }
    Ok(())
}
