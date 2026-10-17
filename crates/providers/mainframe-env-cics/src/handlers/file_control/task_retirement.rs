//! End only confirmed task-owned file browses through the ordinary scoped host boundary.

use super::*;
use mainframe_env_host_api::AccessIntent;

pub(in crate::service) fn release_task(
    service: &CicsService,
    run: &mut Run,
) -> Result<(), HostProblem> {
    // Volatile restoration cannot fabricate the actor of a cursor it did not observe.
    if run.browses.iter().any(|(dataset, cursor)| {
        !run.file_updates
            .task_browses
            .contains_key(&(dataset.clone(), cursor.clone()))
    }) {
        return Err(HostProblem::UnknownOutcome);
    }
    let owners = run
        .file_updates
        .task_browses
        .keys()
        .cloned()
        .collect::<Vec<_>>();
    for (dataset, cursor) in owners {
        let owner = run.file_updates.task_browses[&(dataset.clone(), cursor.clone())].clone();
        if owner.retirement_unknown {
            return Err(HostProblem::UnknownOutcome);
        }
        let active = &run.current_program.effect_invocation;
        if owner.actor.run_unit_id != run.invocation.run_unit_id
            || owner.actor.attempt != run.invocation.attempt
            || owner.actor.principal != run.invocation.principal
            || owner.actor.provider_generations != run.invocation.provider_generations
        {
            return Err(HostProblem::Unauthorized);
        }
        if run.invocation.cancellation_requested()
            || active.cancellation_requested()
            || owner.actor.cancellation_requested()
        {
            return Err(HostProblem::Cancelled);
        }
        validate_generation(service, &owner.actor)?;
        let mut actor = owner.actor;
        // Retain the creating actor and any narrower task/active-actor deadline.
        actor.deadline_tick = actor
            .deadline_tick
            .min(active.deadline_tick)
            .min(run.invocation.deadline_tick);
        let selected = DatasetName::new(&dataset, 128).map_err(|_| HostProblem::Malformed)?;
        let previous = std::mem::replace(&mut run.current_program.effect_invocation, actor);
        let authorization =
            service.authorize(run, "DATASET", selected.as_str(), AccessIntent::Read);
        if let Err(problem) = authorization {
            run.current_program.effect_invocation = previous;
            return Err(problem);
        }
        let result = service.nested(
            run,
            HostRequest::Dataset(DatasetRequest::EndBrowse {
                dataset: selected,
                cursor: cursor.clone(),
            }),
        );
        run.current_program.effect_invocation = previous;
        match result {
            Ok(HostResult::Dataset(DatasetResult::Browse {
                cursor: ended,
                record: None,
                identity: None,
                key: None,
            })) if ended == cursor => {
                run.file_updates
                    .task_browses
                    .remove(&(dataset.clone(), cursor.clone()));
                file_tokens::invalidate_browse(run, &dataset, &cursor);
                if run.browses.get(&dataset) == Some(&cursor) {
                    run.browses.remove(&dataset);
                    run.initial_browse_positions.remove(&dataset);
                    run.current_records.remove(&dataset);
                    run.file_updates.current_record_values.remove(&dataset);
                }
            }
            Ok(_) => {
                mark_unknown(run, &dataset, &cursor);
                return Err(HostProblem::UnknownOutcome);
            }
            Err(problem) => {
                if ambiguous(&problem) {
                    mark_unknown(run, &dataset, &cursor);
                }
                return Err(problem);
            }
        }
    }
    Ok(())
}

pub(super) fn ambiguous(problem: &HostProblem) -> bool {
    matches!(
        problem,
        HostProblem::UnknownOutcome
            | HostProblem::Malformed
            | HostProblem::ProviderFailure
            | HostProblem::InfrastructureFailure
    )
}

/// The host reports a generation refusal as ProviderFailure, also used by admitted providers.
/// Check this known pre-dispatch refusal separately so it cannot latch retirement uncertainty.
pub(super) fn validate_generation(
    service: &CicsService,
    actor: &mainframe_env_execution_api::Invocation,
) -> Result<(), HostProblem> {
    let capability = mainframe_env_execution_api::CapabilityId::new(
        "host.dataset.read",
        mainframe_env_execution_api::InvocationLimits::default(),
    )
    .map_err(|_| HostProblem::InfrastructureFailure)?;
    if let Some(generation) = actor.provider_generations.get(&capability) {
        service
            .host
            .validate_provider_generations(&BTreeMap::from([(capability, generation.clone())]))?;
    }
    Ok(())
}

pub(super) fn mark_unknown(run: &mut Run, dataset: &str, cursor: &str) {
    if let Some(owner) = run
        .file_updates
        .task_browses
        .get_mut(&(dataset.into(), cursor.into()))
    {
        owner.retirement_unknown = true;
    }
}

#[cfg(test)]
mod tests;
