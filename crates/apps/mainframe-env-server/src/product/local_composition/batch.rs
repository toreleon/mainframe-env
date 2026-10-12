//! Existing typed controller decoder plus a signed exact full controller-definition Resource.
use super::*;
use mainframe_env_batch::BatchControllerProjection;

pub(super) fn bind_controllers(
    server: &ProductServer,
    selected: &SelectedApplicationGeneration,
    used: &mut BTreeSet<String>,
) -> Result<(Option<BatchControllerGeneration>, usize), HostProblem> {
    let package = selected.package();
    if package.sections.batch_controllers.is_empty() {
        return Ok((None, 0));
    }
    // Old typed controller-only offers are outside this explicit signed local profile.
    if !package.base.manifest.entries.iter().any(|entry| {
        entry.kind == EntryKind::Resource && entry.path == "resource/batch-controllers"
    }) {
        return Err(HostProblem::Unsupported);
    }
    let mut controllers = package
        .sections
        .batch_controllers
        .iter()
        .map(|item| {
            super::super::batch_controller::decode_application_batch_controller(package, item)
        })
        .collect::<Result<Vec<_>, _>>()?;
    controllers.sort_by(|a, b| a.name.cmp(&b.name));
    if controllers.iter().any(|item| {
        !matches!(
            (&item.selector, &item.plan),
            (
                BatchControllerSelector::TsoRun { .. },
                BatchControllerPlan::ProgramCall
            )
        ) || server
            .program
            .has_registered_program(item.selector.program())
    }) {
        return Err(HostProblem::Unsupported);
    }
    let raw = serde_json::to_vec(&controllers).map_err(|_| HostProblem::InfrastructureFailure)?;
    if raw.len() > MAX_PROVIDER_COMPOSITION_BYTES / 8 {
        return Err(HostProblem::ResourceExhausted);
    }
    bind_entry(
        package,
        EntryKind::Resource,
        "resource/batch-controllers",
        &raw,
        used,
    )?;
    Ok((
        Some(BatchControllerGeneration {
            schema_version: mainframe_env_batch::BATCH_CONTROLLER_REGISTRY_CONTRACT.into(),
            application: selected.record().package.to_ascii_uppercase(),
            generation: selected.record().generation,
            identity: selected.record().identity.clone(),
            controllers,
        }),
        raw.len(),
    ))
}
pub(super) fn valid_projection(
    value: &BatchControllerProjection,
    selected: &LocalSelection,
) -> bool {
    value.generation.application == selected.package
        && value.generation.generation == selected.generation
        && value.generation.identity == selected.identity
        && value.content_payload().is_ok()
}
