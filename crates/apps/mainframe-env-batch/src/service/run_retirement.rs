//! One shared existing Job retirement body, with opt-in lock-free host calls.
use super::*;

impl BatchService {
    pub(super) fn retire_contained_run(
        &self,
        invocation: &(impl RunInput + ?Sized),
        job: &mut Job,
        outcome: Result<i32, HostProblem>,
    ) -> Result<JobSnapshot, HostProblem> {
        invocation.check()?;
        if let Err(problem) = &outcome
            && matches!(
                problem,
                HostProblem::UnknownOutcome
                    | HostProblem::Cancelled
                    | HostProblem::TimedOut
                    | HostProblem::InfrastructureFailure
                    | HostProblem::IdempotencyConflict
                    | HostProblem::ResourceExhausted
            )
        {
            return Err(invocation.poison(problem.clone()));
        }
        let cleanup = self.cleanup_job_temporary_datasets(invocation, job);
        invocation.check()?;
        let outcome = match (outcome, cleanup) {
            (Ok(code), Ok(())) => Ok(code),
            (Err(problem), Ok(())) | (_, Err(problem)) => Err(problem),
        };
        let current = self
            .lock()?
            .jobs
            .get(&job.id)
            .cloned()
            .ok_or(HostProblem::NotFound)?;
        if current.version != job.version || current.state != JobState::Running {
            return Err(invocation.poison(HostProblem::IdempotencyConflict));
        }
        self.retire_job(invocation, job, current, outcome, |next, expected| {
            invocation.check()?;
            {
                let mut state = self.lock()?;
                let current = state.jobs.get(&next.id).ok_or(HostProblem::NotFound)?;
                if current.version != expected
                    || current.owner != next.owner
                    || !matches!(current.state, JobState::Running | JobState::Output)
                {
                    return Err(invocation.poison(HostProblem::IdempotencyConflict));
                }
                self.persist_job(next, Some(expected))?;
                state.jobs.insert(next.id.clone(), next.clone());
                invocation.published_job(job_record(next)?);
            }
            invocation.check()?;
            Ok(())
        })
    }

    pub(super) fn retire_job(
        &self,
        invocation: &(impl RunInput + ?Sized),
        job: &mut Job,
        current: Job,
        outcome: Result<i32, HostProblem>,
        mut publish: impl FnMut(&Job, u64) -> Result<(), HostProblem>,
    ) -> Result<JobSnapshot, HostProblem> {
        invocation.check()?;
        if current.state == JobState::Cancelled {
            return Ok(snapshot(&current));
        }
        if current.state != JobState::Running {
            return Err(HostProblem::UnknownOutcome);
        }
        ensure_job_event_capacity(
            job,
            self.limits.max_events,
            if outcome.is_ok() { 2 } else { 1 },
        )?;
        let terminal_version = next_job_version(current.version)?;
        let completed_version = outcome
            .is_ok()
            .then(|| next_job_version(terminal_version))
            .transpose()?;
        job.version = terminal_version;
        let terminal_step = job.active_step.clone();
        job.active_step = None;
        match outcome {
            Ok(return_code) => {
                job.return_code = Some(return_code);
                job.state = JobState::Output;
                push_job_event(job, self.limits.max_events, "output")?;
                self.append_spool_records(
                    invocation,
                    job,
                    None,
                    None,
                    "JESMSGLG",
                    vec![format!("ENDED RC={return_code:04}").into_bytes()],
                )?;
                self.complete_output(invocation, job)?;
                publish(job, current.version)?;
                let output_version = job.version;
                job.version = completed_version.ok_or(HostProblem::InfrastructureFailure)?;
                job.state = JobState::Completed;
                job.initiator = None;
                push_job_event(job, self.limits.max_events, "completed")?;
                publish(job, output_version)?;
                let result = snapshot(job);
                invocation.check()?;
                self.persist_checkpoint(job, job.effect_sequence)?;
                invocation.check()?;
                return Ok(result);
            }
            Err(problem) => {
                if let HostProblem::Condition { name, .. } = &problem
                    && let Some(code) = name.strip_prefix("ABEND:")
                {
                    job.abend_code = Some(code.to_string());
                }
                let cancelled = problem == HostProblem::Cancelled;
                job.state = if cancelled {
                    JobState::Cancelled
                } else {
                    JobState::Failed
                };
                job.initiator = None;
                if let Some(cancellation) = job.cancellation.as_mut() {
                    cancellation.state = CancellationState::Completed;
                } else if cancelled {
                    job.cancellation = Some(JesCancellation {
                        id: format!("{}-CANCEL-{}", job.id, job.version),
                        requested_by: invocation.original().principal.id().as_str().into(),
                        reason: "execution cancellation".into(),
                        requested_tick: 0,
                        state: CancellationState::Completed,
                    });
                }
                push_job_event(
                    job,
                    self.limits.max_events,
                    if cancelled {
                        String::from("cancelled:execution")
                    } else {
                        format!("failed:{problem:?}")
                    },
                )?;
                if let Some(step) = terminal_step {
                    self.append_spool_records(
                        invocation,
                        job,
                        Some(&step),
                        None,
                        "JOBLOG",
                        vec![
                            format!(
                                "{step} {} {problem:?}",
                                if cancelled { "CANCELLED" } else { "FAILED" }
                            )
                            .into_bytes(),
                        ],
                    )?;
                }
                self.append_spool_records(
                    invocation,
                    job,
                    None,
                    None,
                    "JESMSGLG",
                    vec![
                        format!(
                            "{} {problem:?}",
                            if cancelled { "CANCELLED" } else { "FAILED" }
                        )
                        .into_bytes(),
                    ],
                )?;
                if cancelled {
                    self.cancel_output(invocation, job)?;
                } else {
                    self.complete_output(invocation, job)?;
                }
            }
        }
        publish(job, current.version)?;
        let result = snapshot(job);
        invocation.check()?;
        self.persist_checkpoint(job, job.effect_sequence)?;
        invocation.check()?;
        Ok(result)
    }
}
