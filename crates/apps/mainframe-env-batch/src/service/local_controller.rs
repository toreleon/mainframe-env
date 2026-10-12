//! Opt-in exact controller submission binding; original legacy admission is unchanged.
use super::*;
use mainframe_env_store_api::{
    MAX_PROVIDER_COMPOSITION_BYTES, ProviderCompositionCommit, ProviderStateWrite,
};

pub const LOCAL_CONTROLLER_SUBMISSION_NAMESPACE: &str = "batch-local-controller-submission";
const SCHEMA: &str = "mainframe-env.local-controller-submission@1";
pub const LOCAL_CONTROLLER_KEY_PREFIX: &str = "local-mq:";

#[derive(Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct Binding {
    schema_version: String,
    application: String,
    generation: u64,
    identity: String,
    controller: String,
    program_identity: String,
    program: String,
    owner: String,
    plan_sha256: [u8; 32],
}

/// Genuine prepared registry/publication owner; copied rows cannot construct it.
pub struct PreparedLocalController<'a> {
    service: &'a BatchService,
    _publication: std::sync::RwLockReadGuard<'a, ()>,
    invocation: Invocation,
    bundle: JclBundle,
    key: IdempotencyKey,
    writes: Vec<ProviderStateWrite>,
}

impl BatchService {
    pub fn open(
        host: Arc<ScopedHostService>,
        store: Arc<dyn ProviderStateStore>,
        jcl_limits: JclLimits,
        limits: BatchLimits,
    ) -> Result<Arc<Self>, HostProblem> {
        Self::open_with_scheduler(
            host,
            store,
            jcl_limits,
            limits,
            JesSchedulerConfiguration::single_node(limits.max_active),
        )
    }

    pub fn prepare_local_controller_submission<'a>(
        &'a self,
        invocation: &Invocation,
        generation: &BatchControllerGeneration,
        controller_name: &str,
        bundle: &JclBundle,
        key: &IdempotencyKey,
    ) -> Result<PreparedLocalController<'a>, HostProblem> {
        if !key.as_str().starts_with(LOCAL_CONTROLLER_KEY_PREFIX) {
            return Err(HostProblem::Malformed);
        }
        let plan = self.plan(bundle)?;
        let controller = generation
            .controllers
            .iter()
            .find(|item| item.name == controller_name)
            .ok_or(HostProblem::NotFound)?;
        validate_local_controller_plan(&plan, controller)?;
        let (publication, resolved) = self.admit_controller(&controller.selector)?;
        let resolved = resolved.ok_or(HostProblem::NotFound)?;
        let selected = self
            .controllers
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)?
            .registry
            .composition_selected(&generation.application)?;
        if selected.as_ref() != Some(generation)
            || resolved.name != controller_name
            || resolved.application != generation.application
            || resolved.generation != generation.generation
            || resolved.identity != generation.identity
            || resolved.program != controller.program
            || self.verify_controller_program(&resolved.program)? != controller.selector.program()
        {
            return Err(HostProblem::IdempotencyConflict);
        }
        let binding = Binding {
            schema_version: SCHEMA.into(),
            application: generation.application.clone(),
            generation: generation.generation,
            identity: generation.identity.clone(),
            controller: controller.name.clone(),
            program_identity: controller.program.identity.clone(),
            program: controller.selector.program().into(),
            owner: invocation.principal.id().as_str().into(),
            plan_sha256: plan_digest(&plan)?,
        };
        let payload =
            serde_json::to_vec(&binding).map_err(|_| HostProblem::InfrastructureFailure)?;
        if payload.len() > MAX_PROVIDER_COMPOSITION_BYTES / 8 {
            return Err(HostProblem::ResourceExhausted);
        }
        if self
            .store
            .get_provider_state(LOCAL_CONTROLLER_SUBMISSION_NAMESPACE, key.as_str())
            .map_err(store_error)?
            .is_some()
        {
            return Err(HostProblem::IdempotencyConflict);
        }
        let writes = vec![ProviderStateWrite {
            expected_version: None,
            record: ProviderStateRecord {
                namespace: LOCAL_CONTROLLER_SUBMISSION_NAMESPACE.into(),
                key: key.as_str().into(),
                version: 1,
                payload,
            },
        }];
        Ok(PreparedLocalController {
            service: self,
            _publication: publication,
            invocation: invocation.clone(),
            bundle: bundle.clone(),
            key: key.clone(),
            writes,
        })
    }

    /// Called by the original TSO controller branch under its original Complete/Applied guard.
    pub(super) fn require_local_controller_binding(
        &self,
        job: &Job,
        controller: &ResolvedBatchController,
    ) -> Result<(), HostProblem> {
        if !job.submit_key.starts_with(LOCAL_CONTROLLER_KEY_PREFIX) {
            return Ok(());
        }
        let row = self
            .store
            .get_provider_state(LOCAL_CONTROLLER_SUBMISSION_NAMESPACE, &job.submit_key)
            .map_err(store_error)?
            .ok_or(HostProblem::NotFound)?;
        let binding = decode(&row)?;
        matches_binding(&binding, controller, &job.owner, &job.plan)
    }

    /// Run retirement is already complete; this keeps the actual job owner through the callback.
    /// No row/snapshot can construct this borrow and no callback result rewrites Job state.
    pub fn with_local_controller_completion<T>(
        &self,
        key: &IdempotencyKey,
        job_id: &str,
        inspect: impl FnOnce(&ProviderStateRecord, &JobSnapshot) -> Result<T, HostProblem>,
    ) -> Result<T, HostProblem> {
        let binding_row = self
            .store
            .get_provider_state(LOCAL_CONTROLLER_SUBMISSION_NAMESPACE, key.as_str())
            .map_err(store_error)?
            .ok_or(HostProblem::NotFound)?;
        let binding = decode(&binding_row)?;
        let selector = BatchControllerSelector::tso(&binding.program)?;
        let (_publication, controller) = self.admit_controller(&selector)?;
        let controller = controller.ok_or(HostProblem::NotFound)?;
        let state = self.lock()?;
        let job = state.jobs.get(job_id).ok_or(HostProblem::NotFound)?;
        matches_binding(&binding, &controller, &job.owner, &job.plan)?;
        if job.submit_key != key.as_str()
            || job.state != JobState::Completed
            || job.attempt == 0
            || job.return_code != Some(0)
            || job.abend_code.is_some()
            || job.active_step.is_some()
            || job.steps.len() != 1
            || job.steps[0].state != StepState::Completed
            || job.steps[0].attempt == 0
            || job.steps[0].termination != Some(StepTermination::ReturnCode { code: 0 })
        {
            return Err(HostProblem::UnknownOutcome);
        }
        let row = job_record(job)?;
        if self
            .store
            .get_provider_state(&row.namespace, &row.key)
            .map_err(store_error)?
            .as_ref()
            != Some(&row)
        {
            return Err(HostProblem::UnknownOutcome);
        }
        let checkpoints = self
            .checkpoint_store
            .as_ref()
            .ok_or(HostProblem::Unsupported)?;
        if Arc::as_ptr(checkpoints).cast::<()>() != Arc::as_ptr(&self.store).cast::<()>() {
            return Err(HostProblem::Unsupported);
        }
        // The public checkpoint() method acquires the same job mutex. Read
        // through the real checkpoint store while retaining this original owner.
        let record = checkpoints
            .get_checkpoint(&checkpoint_execution_id(job_id)?)
            .map_err(store_error)?
            .ok_or(HostProblem::UnknownOutcome)?;
        validate_checkpoint_record(job, &record)?;
        let checkpoint: JesCheckpoint = serde_json::from_slice(&record.payload)
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        if checkpoint.job_version != job.version
            || checkpoint.attempt != job.attempt
            || checkpoint.active_step != job.active_step
            || checkpoint.effect_sequence != job.effect_sequence
        {
            return Err(HostProblem::UnknownOutcome);
        }
        inspect(&row, &snapshot(job))
    }
}

impl PreparedLocalController<'_> {
    pub fn writes(&self) -> &[ProviderStateWrite] {
        &self.writes
    }
    pub fn accepts(&self, commit: &ProviderCompositionCommit) -> bool {
        commit.covers(&self.service.store, &self.writes)
    }
    pub fn submit(self, commit: &ProviderCompositionCommit) -> Result<JobSnapshot, HostProblem> {
        if !self.accepts(commit) {
            return Err(HostProblem::UnknownOutcome);
        }
        self.service
            .submit(&self.invocation, &self.bundle, &self.key, false)
    }
}

pub fn validate_local_controller_plan(
    plan: &JobPlan,
    controller: &crate::BatchControllerDefinition,
) -> Result<(), HostProblem> {
    if !matches!(
        (&controller.selector, &controller.plan),
        (
            BatchControllerSelector::TsoRun { .. },
            BatchControllerPlan::ProgramCall
        )
    ) || plan.restart_step.is_some()
        || plan.steps.len() != 1
    {
        return Err(HostProblem::Unsupported);
    }
    let step = &plan.steps[0];
    if step.program != "IKJEFT01" || step.condition != crate::StepCondition::Always {
        return Err(HostProblem::Unsupported);
    }
    let controls = step
        .dds
        .iter()
        .filter(|dd| dd.name == "SYSTSIN")
        .collect::<Vec<_>>();
    if controls.len() != 1 || controls[0].dataset.is_some() || controls[0].concatenation {
        return Err(HostProblem::Unsupported);
    }
    let text = std::str::from_utf8(&controls[0].inline_data).map_err(|_| HostProblem::Malformed)?;
    match crate::db2_tso::parse(text)? {
        crate::db2_tso::Action::Run { program, .. } if program == controller.selector.program() => {
            Ok(())
        }
        _ => Err(HostProblem::IdempotencyConflict),
    }
}
fn plan_digest(plan: &JobPlan) -> Result<[u8; 32], HostProblem> {
    Ok(
        Sha256::digest(serde_json::to_vec(plan).map_err(|_| HostProblem::InfrastructureFailure)?)
            .into(),
    )
}
fn decode(row: &ProviderStateRecord) -> Result<Binding, HostProblem> {
    row.validate_write(MAX_PROVIDER_COMPOSITION_BYTES / 8)
        .map_err(store_error)?;
    if row.namespace != LOCAL_CONTROLLER_SUBMISSION_NAMESPACE
        || row.version != 1
        || !row.key.starts_with(LOCAL_CONTROLLER_KEY_PREFIX)
    {
        return Err(HostProblem::Malformed);
    }
    let binding: Binding =
        serde_json::from_slice(&row.payload).map_err(|_| HostProblem::InfrastructureFailure)?;
    if binding.schema_version != SCHEMA
        || binding.generation == 0
        || binding.generation > i64::MAX as u64
        || serde_json::to_vec(&binding).map_err(|_| HostProblem::InfrastructureFailure)?
            != row.payload
    {
        return Err(HostProblem::Malformed);
    }
    Ok(binding)
}
fn matches_binding(
    binding: &Binding,
    controller: &ResolvedBatchController,
    owner: &str,
    plan: &JobPlan,
) -> Result<(), HostProblem> {
    if binding.application != controller.application
        || binding.generation != controller.generation
        || binding.identity != controller.identity
        || binding.controller != controller.name
        || binding.program_identity != controller.program.identity
        || binding.program != controller.selector.program()
        || binding.owner != owner
        || binding.plan_sha256 != plan_digest(plan)?
    {
        return Err(HostProblem::IdempotencyConflict);
    }
    Ok(())
}
