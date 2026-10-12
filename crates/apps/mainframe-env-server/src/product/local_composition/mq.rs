//! Exact signed queue-only declarations; native MQI and trigger/controller mapping stay separate.
use super::*;
use mainframe_env_application::MqResourceKind;
use mainframe_env_mq::{MqCatalogProjection, MqQueueDefinition};

pub(super) fn bind_catalog(
    package: &ApplicationPackageV2,
    used: &mut BTreeSet<String>,
) -> Result<(Option<Vec<MqQueueDefinition>>, usize), HostProblem> {
    if package.sections.mq_resources.is_empty() {
        return Ok((None, 0));
    }
    if package.sections.mq_resources.len() > mainframe_env_mq::MqLimits::default().max_queues {
        return Err(HostProblem::ResourceExhausted);
    }
    let mut definitions = BTreeMap::new();
    let mut bytes = 0usize;
    for resource in &package.sections.mq_resources {
        if resource.kind != MqResourceKind::Queue
            || resource.target.is_some()
            || resource.controller.is_some()
        {
            return Err(HostProblem::Unsupported);
        }
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
        if definitions
            .insert(
                resource.name.clone(),
                MqQueueDefinition {
                    name: resource.name.clone(),
                    trigger_program: None,
                },
            )
            .is_some()
        {
            return Err(HostProblem::IdempotencyConflict);
        }
    }
    Ok((Some(definitions.into_values().collect()), bytes))
}
pub(super) fn valid_projection(value: &MqCatalogProjection, selected: &LocalSelection) -> bool {
    value.application == selected.package
        && value.generation == selected.generation
        && value.package_identity == selected.identity
        && value.content_payload().is_ok()
}
