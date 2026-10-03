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
