//! Full retained/published provider-state comparators under the existing V3 owner.
use super::*;

pub(super) fn controller_generation(
    package: &ApplicationPackageV2,
    expected: &Inputs,
) -> Result<BatchControllerGeneration, CorpusProblem> {
    let controllers = expected
        .controller_templates
        .iter()
        .map(|row| {
            let entry = package
                .base
                .manifest
                .entries
                .iter()
                .find(|entry| entry.path == row.program_path && entry.kind == EntryKind::Program)
                .ok_or_else(|| drift("expected controller artifact entry missing"))?;
            Ok(BatchControllerDefinition {
                name: row.name.clone(),
                selector: row.selector.clone(),
                plan: row.plan.clone(),
                program: BatchControllerProgram {
                    path: row.program_path.clone(),
                    identity: entry.sha256.clone(),
                },
            })
        })
        .collect::<Result<_, CorpusProblem>>()?;
    Ok(BatchControllerGeneration {
        schema_version: "mainframe-env.batch-controller-registry@1".into(),
        application: APP.into(),
        generation: package.generation,
        identity: package_generation_identity(package).map_err(package_problem)?,
        controllers,
    })
}
pub(super) fn controllers_match(
    controller: &ControllerState,
    published: &[BatchControllerGeneration],
    selected: u64,
) -> bool {
    controller.schema_version == "mainframe-env.batch-controller-state@1"
        && controller.applications.len() == 1
        && controller.applications[0].application == APP
        && controller.applications[0].selected == selected
        && controller.applications[0].generations == published
}
pub(super) fn raw_rows(
    store: &dyn ProviderStateStore,
) -> Result<BTreeMap<String, Vec<ProviderStateRecord>>, CorpusProblem> {
    ROW_BOUNDS
        .iter()
        .map(|(namespace, bound)| {
            let rows = store
                .list_provider_state(namespace, bound + 1)
                .map_err(package_problem)?;
            let keys = rows
                .iter()
                .map(|row| row.key.as_str())
                .collect::<BTreeSet<_>>();
            refuse(
                rows.len() > *bound
                    || rows.len() != keys.len()
                    || rows
                        .iter()
                        .any(|row| row.namespace != *namespace || row.version == 0),
                "V3 provider namespace bounds/identities differ",
            )?;
            Ok(((*namespace).into(), rows))
        })
        .collect()
}
fn one<'a>(
    rows: &'a BTreeMap<String, Vec<ProviderStateRecord>>,
    namespace: &str,
    key: &str,
) -> Result<&'a ProviderStateRecord, CorpusProblem> {
    let rows = rows
        .get(namespace)
        .ok_or_else(|| drift("V3 required namespace missing"))?;
    refuse(
        rows.len() != 1 || rows[0].key != key,
        "V3 required provider row differs",
    )?;
    Ok(&rows[0])
}
pub(super) fn registry(
    store: &dyn ProviderStateStore,
) -> Result<ApplicationInstallerV2, CorpusProblem> {
    let row = store
        .get_provider_state("application-package-v2", "registry")
        .map_err(package_problem)?
        .ok_or_else(|| drift("actual trusted V3 registry missing"))?;
    refuse(
        row.namespace != "application-package-v2" || row.key != "registry" || row.version == 0,
        "actual trusted V3 registry identity differs",
    )?;
    ApplicationInstallerV2::from_state_payload(
        "0.2.0",
        PackageLimits::default(),
        carddemo_package_trust()?,
        &row.payload,
    )
    .map_err(package_problem)
}
pub(super) fn publication_matches(
    publication: &ApplicationPublicationState,
    selected: &ApplicationGenerationRecord,
    action: PublicationAction,
) -> bool {
    publication.matches_complete(APP, selected.generation, &selected.identity)
        && publication.action == action
        && publication.controllers == PublicationSectionState::Applied
        && publication.db2 == PublicationSectionState::Applied
        && publication.ims == PublicationSectionState::Applied
}
pub(super) fn db2_rows(
    expected: &Inputs,
    packages: &BTreeMap<u64, ApplicationPackageV2>,
    staged: &BTreeSet<u64>,
    selected: u64,
    action: PublicationAction,
) -> Result<BTreeMap<String, Vec<(String, serde_json::Value)>>, CorpusProblem> {
    let mut output = ROW_BOUNDS
        .iter()
        .filter(|(namespace, _)| namespace.starts_with("db2-"))
        .map(|(name, _)| ((*name).into(), Vec::new()))
        .collect::<BTreeMap<String, Vec<(String, serde_json::Value)>>>();
    let schemas = expected
        .local_tables
        .iter()
        .map(|table| (table.name.clone(), serde_json::json!(table)))
        .collect::<BTreeMap<_, _>>();
    let tables = expected
        .local_tables
        .iter()
        .map(|table| {
            (
                table.name.clone(),
                serde_json::json!({"columns":table.columns.len(),"rows":{}}),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let names = schemas.keys().cloned().collect::<Vec<_>>();
    let selected_package = packages
        .get(&selected)
        .ok_or_else(|| drift("expected selected Db2 package missing"))?;
    let identity = package_generation_identity(selected_package).map_err(package_problem)?;
    let object = |key: &str, value: serde_json::Value| serde_json::json!({"schema_version":"mainframe-env.db2-object-row@1","object_key":key,"value":value});
    for name in &names {
        output
            .get_mut("db2-v1-table")
            .ok_or_else(|| drift("expected table namespace missing"))?
            .push((name.clone(), object(name, tables[name].clone())));
        output
            .get_mut("db2-v1-schema")
            .ok_or_else(|| drift("expected schema namespace missing"))?
            .push((name.clone(), object(name, schemas[name].clone())));
        output
            .get_mut("db2-v1-table-provenance")
            .ok_or_else(|| drift("expected provenance namespace missing"))?
            .push((
                name.clone(),
                object(
                    name,
                    serde_json::json!({"Application":{"owner":APP,"generation":1}}),
                ),
            ));
    }
    output
        .get_mut("db2-v1-installation")
        .ok_or_else(|| drift("expected installation namespace missing"))?
        .push((
            APP.into(),
            object(
                APP,
                serde_json::json!({"generation":selected,"identity":identity,"tables":names}),
            ),
        ));
    // This exact owner performs one initial install, one g2 install and one
    // retained rollback. Replay/staging/refusal/reopen do not advance it.
    let catalog_version = if action == PublicationAction::Rollback {
        3
    } else {
        selected
    };
    output.get_mut("db2-state").ok_or_else(||drift("expected manifest namespace missing"))?.push(("catalog".into(),serde_json::json!({"schema_version":"mainframe-env.db2-row-store@1","catalog_version":catalog_version})));
    for (generation, package) in packages
        .iter()
        .filter(|(generation, _)| !staged.contains(generation))
    {
        let key = format!("{APP}|{generation:020}");
        let identity = package_generation_identity(package).map_err(package_problem)?;
        output.get_mut("db2-v1-catalog-generation").ok_or_else(||drift("expected generation namespace missing"))?.push((key.clone(),object(&key,serde_json::json!({"application":APP,"snapshot":{"generation":generation,"identity":identity,"schemas":schemas,"tables":tables}}))));
    }
    Ok(output)
}
pub(super) fn db2_matches(
    actual: &BTreeMap<String, Vec<ProviderStateRecord>>,
    expected: &BTreeMap<String, Vec<(String, serde_json::Value)>>,
) -> bool {
    expected.iter().all(|(namespace, pins)| {
        actual.get(namespace).is_some_and(|rows| {
            rows.len() == pins.len()
                && rows.iter().zip(pins).all(|(row, (key, value))| {
                    row.namespace == *namespace
                        && row.key == *key
                        && row.version > 0
                        && serde_json::from_slice::<serde_json::Value>(&row.payload)
                            .ok()
                            .as_ref()
                            == Some(value)
                })
        })
    })
}
pub(super) fn capture(
    owner: &Owner,
    backend: &Backend,
    server: &ProductServer,
    expected: &Inputs,
    definition: &BatchProgramDefinition,
    packages: &BTreeMap<u64, ApplicationPackageV2>,
    staged: &BTreeSet<u64>,
    selection: (u64, PublicationAction),
) -> Result<Snapshot, CorpusProblem> {
    let (selected, action) = selection;
    backend.verify(owner)?;
    let rows = raw_rows(backend.store.as_ref())?;
    let installer = registry(backend.store.as_ref())?;
    let state: serde_json::Value =
        serde_json::from_slice(&one(&rows, "application-package-v2", "registry")?.payload)
            .map_err(package_problem)?;
    let applications = state["applications"]
        .as_array()
        .ok_or_else(|| drift("actual registry applications missing"))?;
    refuse(
        applications.len() != 1
            || applications[0]["application"] != APP
            || applications[0]["selected"].as_u64() != Some(selected),
        "complete actual registry applications/selection differ",
    )?;
    let actual_generations = applications[0]["generations"]
        .as_array()
        .ok_or_else(|| drift("actual registry retained generations missing"))?;
    refuse(
        actual_generations.len() != packages.len()
            || actual_generations
                .iter()
                .map(|row| row["package"]["generation"].as_u64())
                .ne(packages.keys().copied().map(Some)),
        "complete retained package generation set/order differs",
    )?;
    let selected_owner = installer
        .selected_generation(APP)
        .map_err(package_problem)?
        .ok_or_else(|| drift("actual selected V3 owner missing"))?;
    for (generation, package) in packages {
        let identity = package_generation_identity(package).map_err(package_problem)?;
        let actual = installer
            .generation(APP, *generation, &identity)
            .map_err(package_problem)?
            .ok_or_else(|| drift("actual complete retained package missing"))?;
        refuse(
            actual.package() != package
                || actual.record().package != APP
                || actual.record().version != package.base.manifest.version
                || actual.record().generation != *generation
                || actual.record().identity != identity
                || actual.record().state
                    != if staged.contains(generation) {
                        InstallState::Staged
                    } else {
                        InstallState::Ready
                    },
            "actual retained manifest/sections/blobs/signature or record differs",
        )?;
    }
    let selected_record = selected_owner.record().clone();
    refuse(
        selected_record.generation != selected
            || selected_record.state != InstallState::Ready
            || selected_owner.package()
                != packages
                    .get(&selected)
                    .ok_or_else(|| drift("offered selected package missing"))?,
        "actual selected Ready package differs",
    )?;
    let publication = ApplicationPublicationState::from_payload(
        &one(&rows, "application-publication-v2", APP)?.payload,
    )
    .map_err(terminal_problem)?;
    refuse(
        !publication_matches(&publication, &selected_record, action),
        "actual publication and selected owner differ",
    )?;
    let controller: ControllerState =
        serde_json::from_slice(&one(&rows, "batch-controller-state", "registry")?.payload)
            .map_err(package_problem)?;
    let published = packages
        .iter()
        .filter(|(generation, _)| !staged.contains(generation))
        .map(|(_, package)| controller_generation(package, expected))
        .collect::<Result<Vec<_>, _>>()?;
    refuse(
        !controllers_match(&controller, &published, selected),
        "actual complete retained/selected controller state differs",
    )?;
    let ims: serde_json::Value =
        serde_json::from_slice(&one(&rows, "ims-v1-metadata-selection", APP)?.payload)
            .map_err(package_problem)?;
    refuse(
        ims != serde_json::json!({"schema_version":"mainframe-env.ims-metadata-selection@1","application":APP,"generation":selected,"package_identity":selected_record.identity,"metadata_identity":null}),
        "actual empty-section IMS selected binding differs",
    )?;
    refuse(
        !db2_matches(
            &rows,
            &db2_rows(expected, packages, staged, selected, action)?,
        ),
        "actual complete Db2 keys/schema/table/installation/generations/provenance/empty namespace values differ",
    )?;
    let mut tables = Vec::new();
    for expected_table in &expected.local_tables {
        let actual = server
            .db2_service()
            .table_definition(&expected_table.name)
            .map_err(terminal_problem)?;
        let data = server
            .db2_service()
            .table_rows(&expected_table.name)
            .map_err(terminal_problem)?;
        refuse(
            actual != *expected_table || !data.is_empty(),
            "actual local Db2 complete metadata or declared empty rows differ",
        )?;
        tables.push((actual, data));
    }
    let artifact_expected = publisher(definition)?;
    let program = one(&rows, "batch-program", "COBTUPDT")?;
    refuse(
        program.version != 1 || program.payload != definition.artifact.as_str().as_bytes(),
        "actual compiled batch program binding differs",
    )?;
    backend.verify(owner)?;
    let artifacts = LocalArtifactStore::open(&backend.config.artifact_root, 64 * 1024 * 1024)
        .map_err(package_problem)?;
    let artifact = artifacts
        .get_artifact(&definition.artifact)
        .map_err(package_problem)?
        .ok_or_else(|| drift("actual compiled artifact missing"))?;
    refuse(
        artifact != artifact_expected,
        "actual full publisher/stored executable artifact differs",
    )?;
    backend.verify(owner)?;
    Ok(Snapshot {
        rows,
        selected: selected_record,
        packages: packages.clone(),
        staged: staged.clone(),
        tables,
        artifact,
    })
}
pub(super) fn same_providers(before: &Snapshot, after: &Snapshot) -> bool {
    before
        .rows
        .iter()
        .filter(|(name, _)| name.as_str() != "application-package-v2")
        .eq(after
            .rows
            .iter()
            .filter(|(name, _)| name.as_str() != "application-package-v2"))
        && before.tables == after.tables
        && before.artifact == after.artifact
        && before.selected == after.selected
}
pub(super) fn reinstalled(before: &Snapshot, after: &Snapshot) -> bool {
    let (Ok(before_row), Ok(after_row)) = (
        one(&before.rows, "application-package-v2", "registry"),
        one(&after.rows, "application-package-v2", "registry"),
    ) else {
        return false;
    };
    same_providers(before, after)
        && before.packages == after.packages
        && before.staged == after.staged
        && before_row.namespace == after_row.namespace
        && before_row.key == after_row.key
        && before_row.payload == after_row.payload
        && before_row.version.checked_add(1) == Some(after_row.version)
}
pub(super) fn failure_state(
    owner: &Owner,
    report: &mut serde_json::Value,
    backend: &Backend,
) -> Result<(), CorpusProblem> {
    // Retain the actual caller-owned Memory rows while that backend still lives;
    // the failure remains a refusal, never an accepted phase or issue witness.
    let actual = raw_rows(backend.store.as_ref());
    report["scopes"][&backend.name]["failed_provider_observation"] = match actual {
        Ok(rows) => state_json(&rows),
        Err(error) => serde_json::json!({"unavailable":error.code}),
    };
    owner.save(report)
}
pub(super) fn snapshot_json(snapshot: &Snapshot) -> serde_json::Value {
    let metadata = snapshot.artifact.executable.as_ref();
    serde_json::json!({"providers":state_json(&snapshot.rows),"selected":snapshot.selected,"retained_packages":snapshot.packages,"staged_generations":snapshot.staged,
        "local_tables":snapshot.tables,"artifact":{"reference":snapshot.artifact.artifact.as_str(),"media_type":snapshot.artifact.media_type,"payload_digest":snapshot.artifact.payload_digest,"payload_base64":base64::engine::general_purpose::STANDARD.encode(&snapshot.artifact.payload),"executable":metadata.map(|row|serde_json::json!({"artifact_contract":row.artifact_contract,"compatibility_profile":row.compatibility_profile,"compiler_generation":row.compiler_generation,"target":row.target,"options":row.options,"host_interfaces":row.host_interfaces,"ir_contract":row.ir_contract,"dialect_contracts":row.dialect_contracts,"semantic_identity":row.semantic_identity,"manifest_payload_digest":row.manifest_payload_digest}))}})
}
