//! Explicit local static graph. No IBM trigger eligibility or controller execution is inferred.
use mainframe_env_application::{MqResource, MqResourceKind};
use mainframe_env_batch::{
    BatchControllerGeneration, BatchControllerPlan, BatchControllerSelector,
};
use mainframe_env_host_api::HostProblem;
use mainframe_env_mq::{MqLimits, MqObjectName, MqQueueDefinition};
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn definitions(
    resources: &[MqResource],
    controllers: Option<&BatchControllerGeneration>,
) -> Result<Vec<MqQueueDefinition>, HostProblem> {
    if resources.is_empty() || resources.len() > MqLimits::default().max_queues {
        return Err(HostProblem::ResourceExhausted);
    }
    let mut entries = BTreeMap::new();
    let mut folded = BTreeSet::new();
    for resource in resources {
        // Keep the existing case-sensitive MQ catalog rule and package's folded uniqueness rule.
        for name in [Some(resource.name.as_str()), resource.target.as_deref()]
            .into_iter()
            .flatten()
        {
            if MqObjectName::new(name)
                .map_err(|_| HostProblem::Malformed)?
                .as_str()
                != name
            {
                return Err(HostProblem::Malformed);
            }
        }
        if !folded.insert(resource.name.to_ascii_uppercase())
            || entries.insert(resource.name.as_str(), resource).is_some()
        {
            return Err(HostProblem::IdempotencyConflict);
        }
        match resource.kind {
            MqResourceKind::Queue if resource.controller.is_none() => {}
            MqResourceKind::Trigger
                if resource.target.is_some() && resource.controller.is_none() => {}
            MqResourceKind::Process
                if resource.target.is_none() && resource.controller.is_some() =>
            {
                let name = resource
                    .controller
                    .as_deref()
                    .ok_or(HostProblem::Malformed)?;
                if name.is_empty()
                    || name.len() > 128
                    || name != name.to_ascii_uppercase()
                    || !name
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
                {
                    return Err(HostProblem::Malformed);
                }
                let mut matching = controllers
                    .into_iter()
                    .flat_map(|generation| &generation.controllers)
                    .filter(|controller| controller.name == name);
                let controller = matching.next().ok_or(HostProblem::NotFound)?;
                if matching.next().is_some()
                    || !matches!(
                        (&controller.selector, &controller.plan),
                        (
                            BatchControllerSelector::TsoRun { .. },
                            BatchControllerPlan::ProgramCall
                        )
                    )
                {
                    return Err(HostProblem::Unsupported);
                }
            }
            _ => return Err(HostProblem::Unsupported),
        }
    }
    let mut definitions = Vec::new();
    let mut used_triggers = BTreeSet::new();
    let mut used_processes = BTreeSet::new();
    for queue in entries
        .values()
        .filter(|resource| resource.kind == MqResourceKind::Queue)
    {
        let process = if let Some(target) = queue.target.as_deref() {
            let trigger = entries.get(target).ok_or(HostProblem::NotFound)?;
            if trigger.kind != MqResourceKind::Trigger || !used_triggers.insert(target) {
                return Err(HostProblem::IdempotencyConflict);
            }
            let target = trigger.target.as_deref().ok_or(HostProblem::Malformed)?;
            let process = entries.get(target).ok_or(HostProblem::NotFound)?;
            if process.kind != MqResourceKind::Process {
                return Err(HostProblem::IdempotencyConflict);
            }
            used_processes.insert(target);
            Some(target.to_owned())
        } else {
            None
        };
        definitions.push(MqQueueDefinition {
            name: queue.name.clone(),
            trigger_program: process,
        });
    }
    if definitions.is_empty()
        || entries.values().any(|resource| match resource.kind {
            MqResourceKind::Queue => false,
            MqResourceKind::Trigger => !used_triggers.contains(resource.name.as_str()),
            MqResourceKind::Process => !used_processes.contains(resource.name.as_str()),
        })
    {
        return Err(HostProblem::IdempotencyConflict);
    }
    Ok(definitions)
}
