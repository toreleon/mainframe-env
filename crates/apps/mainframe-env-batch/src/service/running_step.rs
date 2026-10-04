//! Volatile observation of the sole Batch runner's genuine Running step.
use super::*;
use std::sync::atomic::{AtomicBool, Ordering};

pub(super) type ProgramDispatch<'a> = dyn for<'step> FnMut(&RunningStepAdmission<'step>, &Invocation, EffectRequest) -> EffectResult
    + 'a;

struct Observation {
    active: AtomicBool,
    original: Invocation,
    row: ProviderStateRecord,
    store: Arc<dyn ProviderStateStore>,
    job_name: String,
    step_name: String,
    job_attempt: u32,
    step_attempt: u32,
    program: String,
    run: Option<Arc<run_stop::RunLifetime>>,
}

/// Borrowed only from the genuine runner callback; not a JES/core/MQ permit.
/// No constructor, Clone or serialization is provided. A snapshot/ID cannot mint it.
/// ```compile_fail
/// use mainframe_env_batch::RunningStepAdmission;
/// let admission = RunningStepAdmission {};
/// ```
/// A borrowed callback admission cannot be retained with a static lifetime:
/// ```compile_fail
/// use mainframe_env_batch::RunningStepAdmission;
/// fn escape<'a>(a: &'a RunningStepAdmission<'a>) -> &'static RunningStepAdmission<'static> { a }
/// ```
pub struct RunningStepAdmission<'step> {
    owner: &'step StepOwner,
}

/// Retained read-only view of the SAME volatile step owner, never a new owner.
/// Retirement/error/Drop irrevocably invalidates it. No constructor/Clone/Serde;
/// restored Job rows do not restore this view or grant dispatch permission.
/// ```compile_fail
/// use mainframe_env_batch::RunningStepView;
/// let view = RunningStepView {};
/// ```
/// ```compile_fail
/// use mainframe_env_batch::RunningStepView;
/// fn requires_clone<T: Clone>() {}
/// requires_clone::<RunningStepView>();
/// ```
pub struct RunningStepView {
    observation: Arc<Observation>,
}

impl RunningStepAdmission<'_> {
    /// Retain observations without extending the runner's executable lifetime.
    /// Only the actual borrowed callback admission can produce this view.
    pub fn retain_for_host(&self) -> RunningStepView {
        RunningStepView {
            observation: self.owner.observation.clone(),
        }
    }

    /// Check this actual runner and captured physical Job row remain live.
    /// This is not server claim, coordinator, SAF or physical-publication authority.
    pub fn check_live(&self) -> Result<(), HostProblem> {
        self.owner.observation.check_live()
    }
}

impl RunningStepView {
    /// Refuse irrevocably revoked views or changed/missing same-store Job rows.
    /// Performs observation only; does not dispatch, renew or settle work.
    pub fn check_live(&self) -> Result<(), HostProblem> {
        self.observation.check_live()
    }
    /// Exact original runner invocation; returned observations grant no permission.
    pub fn original(&self) -> &Invocation {
        &self.observation.original
    }
    /// Exact known-published Running Job row; no reconstructed semantic subset.
    pub fn job_row(&self) -> &ProviderStateRecord {
        &self.observation.row
    }
    /// Compare the actual physical Batch adapter without exposing its writer port.
    /// Equal rows in another adapter are not identity and this grants no authority.
    pub fn uses_store(&self, expected: &Arc<dyn ProviderStateStore>) -> bool {
        Arc::ptr_eq(&self.observation.store, expected)
    }
    /// Retained actual JOB name, not caller ProgramInput/selector text.
    pub fn job_name(&self) -> &str {
        &self.observation.job_name
    }
    /// Exact retained Running step name.
    pub fn step_name(&self) -> &str {
        &self.observation.step_name
    }
    /// Actual positive job attempt observed at the known Running publication.
    pub fn job_attempt(&self) -> u32 {
        self.observation.job_attempt
    }
    /// Actual positive step attempt observed at the known Running publication.
    pub fn step_attempt(&self) -> u32 {
        self.observation.step_attempt
    }
    /// Actual validated ProgramService registration for this step.
    pub fn program(&self) -> &str {
        &self.observation.program
    }
}

impl Observation {
    fn check_live(&self) -> Result<(), HostProblem> {
        if !self.active.load(Ordering::SeqCst) || self.run.as_ref().is_some_and(|run| !run.active())
        {
            return Err(HostProblem::Unauthorized);
        }
        let current = self
            .store
            .get_provider_state(&self.row.namespace, &self.row.key)
            .map_err(store_error);
        let current = match current {
            Ok(current) => current,
            Err(problem) => {
                if self.active.load(Ordering::SeqCst) {
                    if let Some(run) = &self.run {
                        return Err(run.poison(problem));
                    }
                }
                return Err(problem);
            }
        };
        if current.as_ref() != Some(&self.row)
            || !self.active.load(Ordering::SeqCst)
            || self.run.as_ref().is_some_and(|run| !run.active())
        {
            if self.active.load(Ordering::SeqCst) && current.as_ref() != Some(&self.row) {
                if let Some(run) = &self.run {
                    return Err(run.poison(HostProblem::Unauthorized));
                }
            }
            return Err(HostProblem::Unauthorized);
        }
        Ok(())
    }
}

pub(super) struct StepOwner {
    observation: Arc<Observation>,
}
impl StepOwner {
    // Called ONLY from the actual opt-in ProgramService controller branch after
    // successful Running step/checkpoint publication and registration validation.
    pub(super) fn admit(
        service: &BatchService,
        scope: &(impl RunInput + ?Sized),
        job: &Job,
        step: &StepPlan,
        program: &str,
    ) -> Result<Self, HostProblem> {
        scope.check()?;
        let original = scope.original();
        let execution = job
            .steps
            .iter()
            .find(|e| e.name == step.name)
            .ok_or(HostProblem::InfrastructureFailure)?;
        let registration = job
            .program_registrations
            .get(&step.name)
            .ok_or(HostProblem::InfrastructureFailure)?;
        if job.state != JobState::Running
            || job.attempt == 0
            || job.owner != original.principal.id().as_str()
            || job.active_step.as_deref() != Some(step.name.as_str())
            || execution.state != StepState::Running
            || execution.attempt == 0
            || registration.handler != RegisteredProgramHandler::ProgramService
            || registration.program != program
            || step.program != program
            || registration != &resolve_program_registration(program)?
        {
            return Err(HostProblem::Unauthorized);
        }
        let row = job_record(job)?;
        if service
            .store
            .get_provider_state(&row.namespace, &row.key)
            .map_err(store_error)?
            .as_ref()
            != Some(&row)
        {
            return Err(HostProblem::IdempotencyConflict);
        }
        if scope.contained() {
            let checkpoint = service
                .checkpoint(&job.id)?
                .ok_or(HostProblem::UnknownOutcome)?;
            if checkpoint.job_version != job.version
                || checkpoint.attempt != job.attempt
                || checkpoint.active_step != job.active_step
                || checkpoint.effect_sequence != job.effect_sequence
            {
                return Err(HostProblem::UnknownOutcome);
            }
        }
        let run = scope.admit_run(service, &row)?;
        Ok(Self {
            observation: Arc::new(Observation {
                active: AtomicBool::new(true),
                original: original.clone(),
                row,
                store: service.store.clone(),
                job_name: job.name.clone(),
                step_name: step.name.clone(),
                job_attempt: job.attempt,
                step_attempt: execution.attempt,
                program: program.into(),
                run,
            }),
        })
    }
    pub(super) fn borrow(&self) -> RunningStepAdmission<'_> {
        RunningStepAdmission { owner: self }
    }
}
impl Drop for StepOwner {
    fn drop(&mut self) {
        self.observation.active.store(false, Ordering::SeqCst);
    }
}

impl BatchService {
    /// Opt in to a synchronous callback only for genuine Running ProgramService
    /// steps. Uses the existing scheduler, CAS/registration and original request
    /// builder. The callback must enforce scoped host/core admission independently;
    /// this API creates no core Intent, server claim, JES context or MQ permission.
    /// Callback exit/error/panic revokes views before existing step cleanup.
    pub fn run_claimed_with_program_dispatch<D>(
        &self,
        invocation: &Invocation,
        id: &str,
        initiator: &str,
        cancelled: bool,
        dispatch: &mut D,
    ) -> Result<Option<JobSnapshot>, HostProblem>
    where
        D: for<'step> FnMut(
            &RunningStepAdmission<'step>,
            &Invocation,
            EffectRequest,
        ) -> EffectResult,
    {
        let topology = self.topology()?;
        let members = topology
            .members
            .values()
            .filter(|member| member.enabled && member.node == topology.local_node)
            .map(|member| member.name.clone())
            .collect::<Vec<_>>();
        if members.is_empty() {
            return Err(HostProblem::UnsupportedCapability {
                capability: "jes.mas.member".into(),
                detail: "no enabled local MAS member".into(),
            });
        }
        for member in members {
            if let Some(job) = self.run_on_member(
                invocation,
                &member,
                initiator,
                cancelled,
                Some(id),
                Some(dispatch),
            )? {
                return Ok(Some(job));
            }
        }
        Ok(None)
    }
}
