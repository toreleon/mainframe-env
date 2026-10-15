//! Signed finite queue/process/trigger graph admission; launching and completion stay separate.
use super::*;
use mainframe_env_application::MqResourceKind;
use mainframe_env_batch::BatchControllerGeneration;
#[path = "mq/graph.rs"]
mod graph;
#[path = "mq/launch.rs"]
mod launch;
const GRAPH_PROFILE: &[u8] = b"mainframe-env.local-mq-process-trigger-graph@1";
use mainframe_env_mq::{MqCatalogProjection, MqQueueDefinition};

pub(in crate::product) fn bind_catalog(
    package: &ApplicationPackageV2,
    controllers: Option<&BatchControllerGeneration>,
    used: &mut BTreeSet<String>,
) -> Result<(Option<Vec<MqQueueDefinition>>, usize), HostProblem> {
    if package.sections.mq_resources.is_empty() {
        return Ok((None, 0));
    }
    if package.sections.mq_resources.len() > mainframe_env_mq::MqLimits::default().max_queues {
        return Err(HostProblem::ResourceExhausted);
    }
    // The legacy queue-only profile does not interpret graph edges. Keep its
    // Unsupported refusal until the package explicitly offers the graph marker;
    // the existing binding below still authenticates the marker's full bytes.
    let requires_graph = package.sections.mq_resources.iter().any(|resource| {
        resource.kind != MqResourceKind::Queue
            || resource.target.is_some()
            || resource.controller.is_some()
    });
    if requires_graph
        && !package
            .base
            .manifest
            .entries
            .iter()
            .any(|entry| entry.path == "resource/mq/graph-profile")
    {
        return Err(HostProblem::Unsupported);
    }
    let definitions = graph::definitions(&package.sections.mq_resources, controllers)?;
    let mut bytes = 0usize;
    if requires_graph {
        bind_entry(
            package,
            EntryKind::Resource,
            "resource/mq/graph-profile",
            GRAPH_PROFILE,
            used,
        )?;
        bytes = GRAPH_PROFILE.len();
    }
    for resource in &package.sections.mq_resources {
        let raw = serde_json::to_vec(resource).map_err(|_| HostProblem::InfrastructureFailure)?;
        bytes = bytes
            .checked_add(raw.len())
            .ok_or(HostProblem::ResourceExhausted)?;
        if bytes > MAX_PROVIDER_COMPOSITION_BYTES / 8 {
            return Err(HostProblem::ResourceExhausted);
        }
        bind_entry(
            package,
            EntryKind::Resource,
            &format!("resource/mq/{}", resource.name),
            &raw,
            used,
        )?;
    }
    bytes = bytes
        .checked_add(launch::bind_plans(package, controllers, used)?)
        .ok_or(HostProblem::ResourceExhausted)?;
    if bytes > MAX_PROVIDER_COMPOSITION_BYTES / 8 {
        return Err(HostProblem::ResourceExhausted);
    }
    Ok((Some(definitions), bytes))
}
pub(super) fn valid_projection(value: &MqCatalogProjection, selected: &LocalSelection) -> bool {
    value.application == selected.package
        && value.generation == selected.generation
        && value.package_identity == selected.identity
        && value.content_payload().is_ok()
}

// Reopen validates the retained signed graph against both actual selected provider projections.
// Neither this check nor graph admission supplies the original Complete/Applied launch authority.
pub(super) fn matches_graph(
    package: &ApplicationPackageV2,
    selected: &LocalSelection,
) -> Result<(), HostProblem> {
    let controllers = selected.batch.as_ref().map(|value| &value.generation);
    let (definitions, _) = bind_catalog(package, controllers, &mut BTreeSet::new())?;
    if definitions.as_ref() != selected.mq.as_ref().map(|value| &value.definitions) {
        return Err(HostProblem::IdempotencyConflict);
    }
    Ok(())
}
