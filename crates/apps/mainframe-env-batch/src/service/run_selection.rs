//! Existing selection and drive, sharing one private run context.
use super::*;

impl BatchService {
    pub(super) fn run_on_member(
        &self,
        invocation: &(impl RunInput + ?Sized),
        member: &str,
        initiator: &str,
        cancelled: bool,
        requested_id: Option<&str>,
        dispatch: Option<&mut ProgramDispatch<'_>>,
    ) -> Result<Option<JobSnapshot>, HostProblem> {
        invocation.check()?;
        if self.limits.max_active == 0 {
            return Err(HostProblem::ResourceExhausted);
        }
        let member = member.to_ascii_uppercase();
        let topology = self.topology()?;
        let member_definition = topology
            .members
            .get(&member)
            .filter(|member| member.enabled && topology.node_available(&member.node))
            .cloned()
            .ok_or(HostProblem::UnsupportedCapability {
                capability: "jes.mas.member".into(),
                detail: format!("MAS member {member} is unavailable"),
            })?;
        let scheduler = self
            .scheduler
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)?
            .configuration
            .clone();
        let id = {
            let state = self.lock()?;
            let mut class_active = BTreeMap::<char, usize>::new();
            let mut initiator_active = 0usize;
            let mut member_active = 0usize;
            for job in state
                .jobs
                .values()
                .filter(|job| matches!(job.state, JobState::Selected | JobState::Running))
            {
                *class_active.entry(job.class).or_default() += 1;
                if job.initiator.as_deref() == Some(initiator) {
                    initiator_active += 1;
                }
                if job.route.owner_member.as_deref() == Some(member.as_str()) {
                    member_active += 1;
                }
            }
            if member_active >= member_definition.max_active {
                return Ok(None);
            }
            let candidates = state
                .jobs
                .values()
                .filter(|job| job.owner == invocation.original().principal.id().as_str())
                .filter(|job| requested_id.is_none_or(|id| job.id == id))
                .filter(|job| {
                    job.route.execution_node == member_definition.node
                        && job
                            .route
                            .owner_member
                            .as_deref()
                            .is_none_or(|owner| owner == member)
                })
                .map(|job| JobSelectionCandidate {
                    id: &job.id,
                    class: job.class,
                    priority: job.priority,
                    state: job.state,
                });
            select_job(
                &scheduler,
                initiator,
                initiator_active,
                &class_active,
                candidates,
            )?
            .map(str::to_string)
        };
        let Some(id) = id else {
            return Ok(None);
        };
        let authorized = if invocation.contained() {
            let current = self
                .lock()?
                .jobs
                .get(&id)
                .cloned()
                .ok_or(HostProblem::NotFound)?;
            if current.state != JobState::Queued
                || current.attempt != 0
                || current.steps.iter().any(|step| step.attempt != 0)
                || !current.spool.is_empty()
                || current.program_registrations.is_empty()
                || current.program_registrations.values().any(|registration| {
                    registration.handler != RegisteredProgramHandler::ProgramService
                })
            {
                return Err(HostProblem::Unsupported);
            }
            self.authorize(
                invocation,
                "JESJOBS",
                &format!("JOB.{}", current.name),
                AccessIntent::Execute,
                2,
            )?;
            invocation.check()?;
            Some(current)
        } else {
            None
        };
        self.ensure_spool_migrated(invocation, &id)?;
        if cancelled || invocation.original().cancellation_requested() {
            return self.cancel(invocation.original(), &id).map(Some);
        }
        let mut job = {
            let mut state = self.lock()?;
            let current = state.jobs.get(&id).cloned().ok_or(HostProblem::NotFound)?;
            if current.owner != invocation.original().principal.id().as_str() {
                return Err(HostProblem::Unauthorized);
            }
            if let Some(authorized) = &authorized {
                if job_record(authorized)? != job_record(&current)?
                    || self
                        .store
                        .get_provider_state("jes-job", &id)
                        .map_err(store_error)?
                        .as_ref()
                        != Some(&job_record(&current)?)
                {
                    return Err(invocation.poison(HostProblem::IdempotencyConflict));
                }
            } else {
                self.authorize(
                    invocation,
                    "JESJOBS",
                    &format!("JOB.{}", current.name),
                    AccessIntent::Execute,
                    2,
                )?;
            }
            ensure_job_event_capacity(&current, self.limits.max_events, 4)?;
            let selected_version = next_job_version(current.version)?;
            let running_version = next_job_version(selected_version)?;
            let running_attempt = current
                .attempt
                .checked_add(1)
                .filter(|attempt| *attempt <= self.limits.max_attempts)
                .ok_or(HostProblem::ResourceExhausted)?;
            let mut selected = current.clone();
            selected.version = selected_version;
            selected.state = JobState::Selected;
            selected.initiator = Some(initiator.into());
            selected.route.owner_member = Some(member.clone());
            push_job_event(
                &mut selected,
                self.limits.max_events,
                format!("selected:{initiator}"),
            )?;
            self.persist_job(&selected, Some(current.version))?;
            state.jobs.insert(id.clone(), selected.clone());
            let mut running = selected.clone();
            running.version = running_version;
            running.attempt = running_attempt;
            running.state = JobState::Running;
            running.initiator = Some(initiator.into());
            running.route.owner_member = Some(member);
            push_job_event(&mut running, self.limits.max_events, "running")?;
            self.persist_job(&running, Some(selected.version))?;
            state.jobs.insert(id.clone(), running.clone());
            running
        };
        let outcome = self.execute(invocation, &mut job, dispatch);
        invocation.revoke_run();
        if invocation.contained() {
            invocation.check()?;
            return self
                .retire_contained_run(invocation, &mut job, outcome)
                .map(Some);
        }
        if outcome == Err(HostProblem::UnknownOutcome) {
            return Err(HostProblem::UnknownOutcome);
        }
        let cleanup = self.cleanup_job_temporary_datasets(invocation, &mut job);
        let outcome = match (outcome, cleanup) {
            (Ok(return_code), Ok(())) => Ok(return_code),
            (Err(problem), Ok(())) | (_, Err(problem)) => Err(problem),
        };
        let mut state = self.lock()?;
        let current = state.jobs.get(&id).cloned().ok_or(HostProblem::NotFound)?;
        self.retire_job(invocation, &mut job, current, outcome, |next, expected| {
            self.persist_job(next, Some(expected))?;
            state.jobs.insert(id.clone(), next.clone());
            Ok(())
        })
        .map(Some)
    }
}
