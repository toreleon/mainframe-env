//! Existing application batch-controller input projection.

use super::*;

pub(super) fn decode_application_batch_controller(
    package: &ApplicationPackageV2,
    controller: &ApplicationBatchController,
) -> Result<BatchControllerDefinition, HostProblem> {
    let launcher = controller_property(controller, "launcher")?;
    let program = controller_property(controller, "selector-program")?.to_string();
    let selector = match launcher {
        "tso-run" => BatchControllerSelector::TsoRun { program },
        "ims-controller" => BatchControllerSelector::ImsController {
            mode: controller_property(controller, "selector-mode")?.to_string(),
            program,
            qualifier: controller.properties.get("selector-qualifier").cloned(),
        },
        _ => return Err(HostProblem::Malformed),
    };
    let behavior = controller_property(controller, "behavior")?;
    let artifact = package
        .base
        .manifest
        .entries
        .iter()
        .find(|entry| entry.kind == EntryKind::Program && entry.path == controller.program)
        .ok_or(HostProblem::Malformed)?;
    let artifact_program = artifact
        .path
        .rsplit('/')
        .next()
        .filter(|name| name.eq_ignore_ascii_case(selector.program()))
        .ok_or(HostProblem::Malformed)?;
    if artifact_program.is_empty() {
        return Err(HostProblem::Malformed);
    }
    let plan = match behavior {
        "program-call" if controller.kind == BatchControllerKind::CobolProgram => {
            validate_controller_properties(
                controller,
                &["behavior", "launcher", "selector-program"],
                &[],
            )?;
            BatchControllerPlan::ProgramCall
        }
        "ims-load" if controller.kind == BatchControllerKind::ImsMessageProcessing => {
            validate_controller_properties(
                controller,
                &[
                    "behavior",
                    "launcher",
                    "selector-program",
                    "selector-mode",
                    "selector-qualifier",
                    "database",
                    "root-dd",
                    "child-dd",
                    "root-record-bytes",
                    "child-record-bytes",
                    "parent-key-bytes",
                ],
                &[],
            )?;
            BatchControllerPlan::ImsLoad {
                database: controller_property(controller, "database")?.into(),
                root_dd: controller_property(controller, "root-dd")?.into(),
                child_dd: controller_property(controller, "child-dd")?.into(),
                root_record_bytes: controller_usize(controller, "root-record-bytes")?,
                child_record_bytes: controller_usize(controller, "child-record-bytes")?,
                parent_key_bytes: controller_usize(controller, "parent-key-bytes")?,
            }
        }
        "ims-unload"
            if matches!(
                controller.kind,
                BatchControllerKind::ImsMessageProcessing | BatchControllerKind::DeclarativeUtility
            ) =>
        {
            validate_controller_properties(
                controller,
                &[
                    "behavior",
                    "launcher",
                    "selector-program",
                    "selector-mode",
                    "selector-qualifier",
                    "database",
                    "root-segment",
                    "child-segment",
                ],
                &["root-output-dd", "child-output-dd", "combined-output-dd"],
            )?;
            BatchControllerPlan::ImsUnload {
                database: controller_property(controller, "database")?.into(),
                root_segment: controller_property(controller, "root-segment")?.into(),
                child_segment: controller_property(controller, "child-segment")?.into(),
                root_output_dd: controller.properties.get("root-output-dd").cloned(),
                child_output_dd: controller.properties.get("child-output-dd").cloned(),
                combined_output_dd: controller.properties.get("combined-output-dd").cloned(),
            }
        }
        "ims-purge" if controller.kind == BatchControllerKind::ImsMessageProcessing => {
            validate_controller_properties(
                controller,
                &[
                    "behavior",
                    "launcher",
                    "selector-program",
                    "selector-mode",
                    "selector-qualifier",
                    "psb",
                    "root-segment",
                    "child-segment",
                    "control-dd",
                    "required-expiry-days",
                    "checkpoint-prefix",
                    "summary-field",
                ],
                &[],
            )?;
            BatchControllerPlan::ImsPurge {
                psb: controller_property(controller, "psb")?.into(),
                root_segment: controller_property(controller, "root-segment")?.into(),
                child_segment: controller_property(controller, "child-segment")?.into(),
                control_dd: controller_property(controller, "control-dd")?.into(),
                required_expiry_days: controller_property(controller, "required-expiry-days")?
                    .into(),
                checkpoint_prefix: controller_property(controller, "checkpoint-prefix")?.into(),
                summary_field: controller_property(controller, "summary-field")?.into(),
            }
        }
        _ => return Err(HostProblem::Malformed),
    };
    Ok(BatchControllerDefinition {
        name: controller.name.clone(),
        selector,
        program: BatchControllerProgram {
            path: artifact.path.clone(),
            identity: artifact.sha256.clone(),
        },
        plan,
    })
}

fn controller_property<'a>(
    controller: &'a ApplicationBatchController,
    name: &str,
) -> Result<&'a str, HostProblem> {
    controller
        .properties
        .get(name)
        .map(String::as_str)
        .filter(|value| !value.is_empty())
        .ok_or(HostProblem::Malformed)
}

fn controller_usize(
    controller: &ApplicationBatchController,
    name: &str,
) -> Result<usize, HostProblem> {
    controller_property(controller, name)?
        .parse()
        .map_err(|_| HostProblem::Malformed)
}

fn validate_controller_properties(
    controller: &ApplicationBatchController,
    required: &[&str],
    optional: &[&str],
) -> Result<(), HostProblem> {
    let accepted = required
        .iter()
        .chain(optional)
        .copied()
        .collect::<BTreeSet<_>>();
    if required
        .iter()
        .any(|name| !controller.properties.contains_key(*name))
        || controller
            .properties
            .keys()
            .any(|name| !accepted.contains(name.as_str()))
    {
        Err(HostProblem::Malformed)
    } else {
        Ok(())
    }
}

pub(super) struct ApplicationPublicationPlan {
    pub(super) controllers: BatchControllerGeneration,
    pub(super) db2: Option<Db2CatalogGeneration>,
}

impl ProductServer {
    pub(super) fn publication_write(&self) -> Result<RwLockWriteGuard<'_, ()>, HostProblem> {
        self.application_publication
            .try_write()
            .map_err(|error| match error {
                TryLockError::WouldBlock => HostProblem::IdempotencyConflict,
                TryLockError::Poisoned(_) => HostProblem::InfrastructureFailure,
            })
    }

    pub(super) fn decode_application_publication(
        &self,
        record: &ProviderStateRecord,
    ) -> Result<ApplicationPublicationState, HostProblem> {
        let state = ApplicationPublicationState::from_payload(&record.payload)?;
        if record.namespace != APPLICATION_PUBLICATION_NAMESPACE
            || record.version == 0
            || record.key != state.package.to_ascii_uppercase()
        {
            return Err(HostProblem::InfrastructureFailure);
        }
        Ok(state)
    }

    pub(super) fn require_complete_application_publication(
        &self,
        selected: &SelectedApplicationGeneration,
    ) -> Result<(), HostProblem> {
        let expected = selected.record();
        let row = self
            .store
            .get_provider_state(
                APPLICATION_PUBLICATION_NAMESPACE,
                &expected.package.to_ascii_uppercase(),
            )
            .map_err(store_error)?
            .ok_or(HostProblem::NotFound)?;
        let state = self.decode_application_publication(&row)?;
        let package = selected.package();
        let sql_applicable = !package.sections.sql_tables.is_empty()
            || !package.sections.sql_rows.is_empty()
            || package
                .base
                .manifest
                .entries
                .iter()
                .any(|entry| entry.kind == EntryKind::Data && entry.path == "data/db2/catalog");
        if !state.matches_complete(&expected.package, expected.generation, &expected.identity)
            || sql_applicable && state.db2 != PublicationSectionState::Applied
            || !package.sections.batch_controllers.is_empty()
                && state.controllers != PublicationSectionState::Applied
            || (package.sections.ims_metadata.is_some() || package.sections.ims_tm.is_some())
                && state.ims != PublicationSectionState::Applied
        {
            return Err(HostProblem::NotFound);
        }
        Ok(())
    }

    pub(super) fn prevalidate_application_publication(
        &self,
        selected: &SelectedApplicationGeneration,
        action: PublicationAction,
        completed: bool,
    ) -> Result<ApplicationPublicationPlan, HostProblem> {
        let package = selected.package();
        let controllers = package
            .sections
            .batch_controllers
            .iter()
            .map(|controller| decode_application_batch_controller(package, controller))
            .collect::<Result<Vec<_>, _>>()?;
        for controller in &controllers {
            if matches!(controller.plan, BatchControllerPlan::ProgramCall)
                && self
                    .program
                    .has_registered_program(controller.selector.program())
            {
                return Err(HostProblem::IdempotencyConflict);
            }
        }
        let controllers = BatchControllerGeneration {
            schema_version: BATCH_CONTROLLER_REGISTRY_CONTRACT.into(),
            application: package.base.manifest.name.clone(),
            generation: package.generation,
            identity: selected.record().identity.clone(),
            controllers,
        };
        match action {
            PublicationAction::Install => self.batch.validate_controller_install(&controllers)?,
            PublicationAction::Rollback => self.batch.validate_controller_rollback(
                &controllers.application,
                controllers.generation,
                &controllers.identity,
            )?,
        }
        let db2 = self.decode_application_db2_catalog(selected)?;
        if let Some(catalog) = &db2 {
            catalog.validate(Db2Limits::default())?;
            // Completed replay performs no provider install. Preserve its readiness
            // while still checking the signed shape and exact catalog closure.
            if !completed {
                match action {
                    PublicationAction::Install => self.db2.validate_catalog_install(catalog)?,
                    PublicationAction::Rollback => self.db2.validate_catalog_rollback(
                        &catalog.application,
                        catalog.generation,
                        &catalog.identity,
                    )?,
                }
            }
        }
        if let Some(metadata) = &package.sections.ims_metadata {
            mainframe_env_host_api::validate_ims_metadata(
                metadata,
                mainframe_env_host_api::ImsMetadataLimits::default(),
            )
            .map_err(|_| HostProblem::Malformed)?;
            self.ims.validate_secondary_metadata(metadata)?;
        }
        if let Some(definitions) = &package.sections.ims_tm {
            definitions.validate(TmLimits::default())?;
        }
        Ok(ApplicationPublicationPlan { controllers, db2 })
    }
    pub(super) fn decode_application_db2_catalog(
        &self,
        selected: &SelectedApplicationGeneration,
    ) -> Result<Option<Db2CatalogGeneration>, HostProblem> {
        let package = selected.package();
        let catalog_entry = package
            .base
            .manifest
            .entries
            .iter()
            .find(|entry| entry.kind == EntryKind::Data && entry.path == "data/db2/catalog");
        let Some(catalog_entry) = catalog_entry else {
            return if package.sections.sql_tables.is_empty() && package.sections.sql_rows.is_empty()
            {
                Ok(None)
            } else {
                Err(HostProblem::Malformed)
            };
        };
        let catalog_blob = package
            .base
            .blobs
            .get(&catalog_entry.sha256)
            .ok_or(HostProblem::Malformed)?;
        let tables = decode_table_definitions_bounded(catalog_blob, Db2Limits::default())?;
        let declared = package
            .sections
            .sql_tables
            .iter()
            .map(|table| {
                (
                    table.name.to_ascii_uppercase(),
                    table
                        .columns
                        .iter()
                        .map(|column| (column.name.to_ascii_uppercase(), column.nullable))
                        .collect::<Vec<_>>(),
                    table
                        .primary_key
                        .iter()
                        .map(|column| column.to_ascii_uppercase())
                        .collect::<Vec<_>>(),
                )
            })
            .collect::<BTreeSet<_>>();
        let signed = tables
            .iter()
            .map(|table| {
                (
                    table.name.to_ascii_uppercase(),
                    table
                        .columns
                        .iter()
                        .map(|column| (column.name.to_ascii_uppercase(), column.nullable))
                        .collect::<Vec<_>>(),
                    table
                        .primary_key
                        .iter()
                        .map(|column| column.to_ascii_uppercase())
                        .collect::<Vec<_>>(),
                )
            })
            .collect::<BTreeSet<_>>();
        if declared != signed {
            return Err(HostProblem::Malformed);
        }
        let rows = package
            .sections
            .sql_rows
            .iter()
            .map(|row| Db2SeedRow {
                table: row.table.clone(),
                values: row
                    .values
                    .iter()
                    .map(|(name, value)| (name.clone(), value.as_bytes().to_vec()))
                    .collect(),
            })
            .collect();
        Ok(Some(Db2CatalogGeneration {
            application: package.base.manifest.name.clone(),
            generation: package.generation,
            identity: selected.record().identity.clone(),
            tables,
            rows,
        }))
    }
}
