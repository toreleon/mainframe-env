//! Retire known task-owned file browses through the original scoped host authority.

use super::*;

pub(in crate::service::handlers) fn check_cleanup_deadline(
    run: &Run,
    now_tick: u64,
) -> Result<(), HostProblem> {
    if !run.browses.is_empty() && now_tick >= run.current_program.effect_invocation.deadline_tick {
        Err(HostProblem::TimedOut)
    } else {
        Ok(())
    }
}

pub(in crate::service::handlers) fn release_task(
    service: &CicsService,
    run: &mut Run,
) -> Result<(), HostProblem> {
    for (name, cursor) in run.browses.clone() {
        let actor = &run.current_program.effect_invocation;
        if actor.cancellation_requested() {
            return Err(HostProblem::Cancelled);
        }
        if let Some(clock) = &service.replay_clock
            && clock.now_tick()? >= actor.deadline_tick
        {
            return Err(HostProblem::TimedOut);
        }
        let dataset = DatasetName::new(&name, 128).map_err(|_| HostProblem::Malformed)?;
        service.authorize(
            run,
            "DATASET",
            &name,
            crate::service::access_for(CicsOperation::EndBrowse),
        )?;
        if let Some(clock) = &service.replay_clock
            && clock.now_tick()? >= run.current_program.effect_invocation.deadline_tick
        {
            return Err(HostProblem::TimedOut);
        }
        let result = service.nested(
            run,
            HostRequest::Dataset(DatasetRequest::EndBrowse {
                dataset,
                cursor: cursor.clone(),
            }),
        );
        match result {
            Ok(HostResult::Dataset(DatasetResult::Browse {
                cursor: ended,
                record: None,
                identity: None,
                key: None,
            })) if ended == cursor => {
                run.browses.remove(&name);
                run.initial_browse_positions.remove(&name);
                run.current_records.remove(&name);
                run.file_updates.current_record_values.remove(&name);
                file_tokens::invalidate_browse(run, &name, &cursor);
            }
            Err(problem) if problem != HostProblem::UnknownOutcome => return Err(problem),
            _ => {
                // An uncertain delegate must retain its owner and block fresh task admission.
                service
                    .lock()?
                    .task_dispatch
                    .mark_uncertain_session(&run.session);
                return Err(HostProblem::UnknownOutcome);
            }
        }
        // Public lifecycle cleanup works on a clone; retain each known retirement even
        // if a later cursor or another task resource refuses cleanup. Command loans
        // instead own the mutable run directly and restore it on every exit.
        let mut state = service.lock()?;
        if let Some(saved) = state.runs.get_mut(&run.invocation.run_unit_id) {
            if saved.session != run.session
                || saved.current_program.effect_invocation != run.current_program.effect_invocation
                || saved.browses.get(&name) != Some(&cursor)
            {
                return Err(HostProblem::IdempotencyConflict);
            }
            saved.browses.remove(&name);
            saved.initial_browse_positions.remove(&name);
            saved.current_records.remove(&name);
            saved.file_updates.current_record_values.remove(&name);
            file_tokens::invalidate_browse(saved, &name, &cursor);
            saved.host_sequence = run.host_sequence;
        }
    }
    Ok(())
}
