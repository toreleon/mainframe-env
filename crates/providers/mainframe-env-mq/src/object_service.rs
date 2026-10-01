//! Adapter between the typed object authority and the queue-only host route.

use crate::object::{
    MqObjectCapability, MqObjectCatalog, MqObjectDefinition, MqObjectError, MqObjectLimits,
    MqObjectLookup, MqObjectName, MqQueueManagerDefinition, MqResolvedTarget,
};
use crate::service::{MqLimits, MqQueueDefinition};
use mainframe_env_host_api::HostProblem;

const LEGACY_MANAGER_NAME: &str = "DEFAULT.QM";

pub(crate) fn catalog_limits(limits: MqLimits) -> MqObjectLimits {
    MqObjectLimits {
        max_dynamic_instances: limits.max_queues,
        max_persisted_bytes: limits
            .max_state_bytes
            .min(MqObjectLimits::default().max_persisted_bytes),
        ..MqObjectLimits::default()
    }
}

pub(crate) fn legacy_catalog(
    definitions: &[MqQueueDefinition],
    limits: MqLimits,
) -> Result<MqObjectCatalog, HostProblem> {
    MqObjectCatalog::from_queue_definitions(
        MqQueueManagerDefinition {
            name: MqObjectName::new(LEGACY_MANAGER_NAME).expect("static MQ manager name"),
            default_transmission_queue: None,
        },
        definitions,
        catalog_limits(limits),
    )
    .map_err(object_problem)
}

pub(crate) fn object_problem(problem: MqObjectError) -> HostProblem {
    match problem {
        MqObjectError::InvalidName | MqObjectError::InvalidDynamicPattern => HostProblem::Malformed,
        MqObjectError::UnknownObject | MqObjectError::MissingReference => HostProblem::NotFound,
        MqObjectError::ResourceExhausted => HostProblem::ResourceExhausted,
        MqObjectError::NotAuthorized => HostProblem::Unauthorized,
        _ => HostProblem::ProviderFailure,
    }
}

/// The compatibility route can execute only local queues. Remote routing needs
/// a channel adapter; model opens need an MQOD dynamic-name pattern and close mode.
pub(crate) fn local_target(
    catalog: &MqObjectCatalog,
    name: &str,
    capability: MqObjectCapability,
) -> Result<(String, Vec<String>), HostProblem> {
    let name = MqObjectName::new(name).map_err(object_problem)?;
    let resolution = catalog
        .resolve(&MqObjectLookup::Queue(name), capability)
        .map_err(object_problem)?;
    match resolution.target {
        MqResolvedTarget::Queue { name, .. } => Ok((
            name.as_str().into(),
            resolution
                .path
                .into_iter()
                .map(|identity| identity.name.as_str().into())
                .collect(),
        )),
        MqResolvedTarget::Model { .. }
        | MqResolvedTarget::Remote(_)
        | MqResolvedTarget::Topic { .. }
        | MqResolvedTarget::Definition { .. } => Err(HostProblem::ProviderFailure),
    }
}

pub(crate) fn local_definitions(
    catalog: &MqObjectCatalog,
) -> impl Iterator<Item = (&MqObjectName, &Option<MqObjectName>)> {
    catalog
        .definitions()
        .filter_map(|definition| match definition {
            MqObjectDefinition::LocalQueue {
                name,
                trigger_process,
                ..
            } => Some((name, trigger_process)),
            _ => None,
        })
}
