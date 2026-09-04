use crate::ams::{
    AmsCommand, AmsRegister, AmsStatement, compare, numeric_operand, operand, pair_operand,
};
use crate::controller::{
    BatchControllerRegistry, BatchControllerRegistryState, MAX_CONTROLLER_STATE_BYTES,
    ResolvedBatchController,
};
use crate::program::{
    ProgramRegistration, RegisteredProgramHandler, TsoProgramExecution,
    resolve_program_registration, tso_program_execution,
};
use crate::{
    BatchControllerGeneration, BatchControllerInstallReceipt, BatchControllerPlan,
    BatchControllerSelector, DdAllocationPlan, DdDispositionPlan, DdSourceKind,
    DdStatusDisposition, DdTerminalDisposition, JES_DURABLE_JOB_CONTRACT, JES_OUTPUT_CONTRACT,
    JES_SPOOL_CONTRACT, JclBundle, JclLimits, JesJobKind, JesJobRoute, JesOutputGroup,
    JesOutputState, JesSchedulerConfiguration, JesSpoolDescriptor, JesSpoolState,
    JesSubmissionOrigin, JesTopology, JobPlan, JobSelectionCandidate, JobState, ProgramInput,
    StepExecution, StepPlan, StepState, StepTermination, decode_program_output, parse_jcl,
    plan_dd_allocations, select_job,
};
use mainframe_env_execution_api::{
    BoundedPayload, IdempotencyKey, Invocation, InvocationLimits, PrincipalId,
};
use mainframe_env_host_api::{
    AccessIntent, CicsConditionPolicy, CicsDisposition, CicsOperation, CicsRequest,
    DatasetAttributes, DatasetDefinition, DatasetLifecycleState, DatasetLockMode,
    DatasetLockTarget, DatasetName, DatasetOrganization, DatasetRequest, DatasetResult,
    Db2Operation, Db2Request, EffectRequest, HostProblem, HostRequest, HostResult, ImsOperation,
    ImsQualifier, ImsRequest, JobName, MemberName, Mutation, ProgramName, ProgramRequest,
    RecordFormat, ResourceName, ScopedHostService, SecurityDecision, SecurityRequest, SpoolRequest,
    SpoolResult,
};
use mainframe_env_store_api::{ProviderStateRecord, ProviderStateStore, StoreError};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

const CONTROLLER_STATE_NAMESPACE: &str = "batch-controller-state";
const CONTROLLER_STATE_KEY: &str = "registry";
const SCHEDULER_STATE_NAMESPACE: &str = "jes-scheduler";
const TOPOLOGY_STATE_NAMESPACE: &str = "jes-topology";
const CONFIGURATION_STATE_KEY: &str = "configuration";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BatchLimits {
    pub max_jobs: usize,
    pub max_queued: usize,
    pub max_active: usize,
    pub max_spool_files: usize,
    pub max_spool_records: usize,
    pub max_spool_bytes: usize,
    pub spool_retention_ticks: u64,
    pub max_nje_nodes: usize,
    pub max_mas_members: usize,
    pub max_events: usize,
    pub max_attempts: u32,
}

impl Default for BatchLimits {
    fn default() -> Self {
        Self {
            max_jobs: 16384,
            max_queued: 4096,
            max_active: 1,
            max_spool_files: 128,
            max_spool_records: 262_144,
            max_spool_bytes: 256 * 1024 * 1024,
            spool_retention_ticks: 86_400,
            max_nje_nodes: 256,
            max_mas_members: 4_096,
            max_events: 65536,
            max_attempts: 3,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JobSnapshot {
    pub id: String,
    pub name: String,
    pub owner: String,
    pub class: char,
    pub priority: u8,
    pub state: JobState,
    pub return_code: Option<i32>,
    pub abend_code: Option<String>,
    pub active_step: Option<String>,
    pub initiator: Option<String>,
    pub steps: Vec<StepExecution>,
    pub attempt: u32,
    pub version: u64,
    pub kind: JesJobKind,
    pub origin: JesSubmissionOrigin,
    pub route: JesJobRoute,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct Job {
    #[serde(default = "legacy_jes_job_contract")]
    schema_version: String,
    id: String,
    name: String,
    owner: String,
    #[serde(default)]
    kind: JesJobKind,
    #[serde(default)]
    origin: JesSubmissionOrigin,
    #[serde(default)]
    route: JesJobRoute,
    class: char,
    priority: u8,
    state: JobState,
    return_code: Option<i32>,
    #[serde(default)]
    abend_code: Option<String>,
    active_step: Option<String>,
    #[serde(default)]
    initiator: Option<String>,
    #[serde(default)]
    steps: Vec<StepExecution>,
    attempt: u32,
    version: u64,
    submit_key: String,
    plan: JobPlan,
    #[serde(default)]
    program_registrations: BTreeMap<String, ProgramRegistration>,
    #[serde(default)]
    dataset_resolutions: BTreeMap<String, String>,
    #[serde(default)]
    temporary_datasets: Vec<String>,
    #[serde(default)]
    spool_sequence: u64,
    #[serde(default)]
    spool_files: BTreeMap<String, JesSpoolDescriptor>,
    #[serde(default)]
    output_groups: BTreeMap<String, JesOutputGroup>,
    #[serde(default)]
    spool: BTreeMap<String, Vec<Vec<u8>>>,
    events: Vec<String>,
}

#[derive(Clone, Debug)]
struct DdRuntimeAllocation {
    ordinal: usize,
    dataset: DatasetName,
    member: Option<MemberName>,
    disposition: DdDispositionPlan,
    lock_id: Option<String>,
    lock_owner: PrincipalId,
}

struct State {
    jobs: BTreeMap<String, Job>,
    replay: BTreeMap<String, String>,
    next_id: u64,
}

struct DurableControllers {
    store_version: u64,
    registry: BatchControllerRegistry,
}

struct DurableScheduler {
    store_version: u64,
    configuration: JesSchedulerConfiguration,
}

struct DurableTopology {
    store_version: u64,
    configuration: JesTopology,
}

pub struct BatchService {
    host: Arc<ScopedHostService>,
    store: Arc<dyn ProviderStateStore>,
    jcl_limits: JclLimits,
    limits: BatchLimits,
    scheduler: Mutex<DurableScheduler>,
    topology: Mutex<DurableTopology>,
    controllers: Mutex<DurableControllers>,
    state: Mutex<State>,
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

    pub fn open_with_scheduler(
        host: Arc<ScopedHostService>,
        store: Arc<dyn ProviderStateStore>,
        jcl_limits: JclLimits,
        limits: BatchLimits,
        scheduler: JesSchedulerConfiguration,
    ) -> Result<Arc<Self>, HostProblem> {
        scheduler.validate()?;
        if limits.max_nje_nodes == 0 || limits.max_mas_members == 0 {
            return Err(HostProblem::Malformed);
        }
        let mut jobs = BTreeMap::new();
        let mut replay = BTreeMap::new();
        for row in store
            .list_provider_state("jes-job", limits.max_jobs)
            .map_err(store_error)?
        {
            let mut job: Job = serde_json::from_slice(&row.payload)
                .map_err(|_| HostProblem::InfrastructureFailure)?;
            if job.version != row.version {
                return Err(HostProblem::InfrastructureFailure);
            }
            if !matches!(
                job.schema_version.as_str(),
                "mainframe-env.jes-durable-job@1" | JES_DURABLE_JOB_CONTRACT
            ) {
                return Err(HostProblem::InfrastructureFailure);
            }
            let previous = job.version;
            let mut changed = false;
            if job.schema_version != JES_DURABLE_JOB_CONTRACT {
                job.schema_version = JES_DURABLE_JOB_CONTRACT.into();
                if job.steps.is_empty() {
                    job.steps = initial_step_executions(&job.plan);
                }
                job.events.push("migrated:jes-durable-job@1-to-2".into());
                changed = true;
            }
            if job.program_registrations.is_empty() {
                job.program_registrations = resolve_plan_programs(&job.plan)?;
                job.events.push("migrated:program-registrations".into());
                changed = true;
            } else {
                validate_plan_programs(&job.plan, &job.program_registrations)?;
            }
            if matches!(job.state, JobState::Selected | JobState::Running) {
                job.version += 1;
                job.state = if job.attempt >= limits.max_attempts {
                    JobState::Failed
                } else {
                    JobState::Queued
                };
                job.active_step = None;
                job.initiator = None;
                for step in &mut job.steps {
                    if !step.state.terminal() {
                        step.state = StepState::Pending;
                        step.termination = None;
                    }
                }
                job.events.push("warm-start-recovered".into());
                changed = true;
            } else if job.state == JobState::Output {
                job.version += 1;
                job.state = JobState::Completed;
                job.initiator = None;
                job.events.push("warm-start-output-completed".into());
                changed = true;
            } else if changed {
                job.version += 1;
            }
            if changed {
                store
                    .put_provider_state(job_record(&job)?, Some(previous))
                    .map_err(store_error)?;
            }
            replay.insert(job.submit_key.clone(), job.id.clone());
            jobs.insert(job.id.clone(), job);
        }
        let next_id = store
            .get_provider_state("jes-meta", "next-id")
            .map_err(store_error)?
            .map(|row| {
                if row.payload.len() != 8 {
                    return Err(HostProblem::InfrastructureFailure);
                }
                Ok(u64::from_be_bytes(
                    row.payload
                        .try_into()
                        .map_err(|_| HostProblem::InfrastructureFailure)?,
                ))
            })
            .transpose()?
            .unwrap_or(1);
        let (controller_store_version, controller_registry) = match store
            .get_provider_state(CONTROLLER_STATE_NAMESPACE, CONTROLLER_STATE_KEY)
            .map_err(store_error)?
        {
            Some(record) => {
                if record.payload.len() > MAX_CONTROLLER_STATE_BYTES {
                    return Err(HostProblem::ResourceExhausted);
                }
                let state: BatchControllerRegistryState =
                    serde_json::from_slice(&record.payload)
                        .map_err(|_| HostProblem::InfrastructureFailure)?;
                (
                    record.version,
                    BatchControllerRegistry::from_state(state)
                        .map_err(|_| HostProblem::InfrastructureFailure)?,
                )
            }
            None => (0, BatchControllerRegistry::default()),
        };
        let (scheduler_store_version, scheduler) = match store
            .get_provider_state(SCHEDULER_STATE_NAMESPACE, CONFIGURATION_STATE_KEY)
            .map_err(store_error)?
        {
            Some(record) => {
                let configuration: JesSchedulerConfiguration =
                    serde_json::from_slice(&record.payload)
                        .map_err(|_| HostProblem::InfrastructureFailure)?;
                configuration.validate()?;
                (record.version, configuration)
            }
            None => (0, scheduler),
        };
        let (topology_store_version, topology) = match store
            .get_provider_state(TOPOLOGY_STATE_NAMESPACE, CONFIGURATION_STATE_KEY)
            .map_err(store_error)?
        {
            Some(record) => {
                let configuration: JesTopology = serde_json::from_slice(&record.payload)
                    .map_err(|_| HostProblem::InfrastructureFailure)?;
                configuration.validate(limits.max_nje_nodes, limits.max_mas_members)?;
                (record.version, configuration)
            }
            None => {
                let configuration = JesTopology::single_node(limits.max_active);
                configuration.validate(limits.max_nje_nodes, limits.max_mas_members)?;
                (0, configuration)
            }
        };
        for job in jobs.values().filter(|job| !job.state.terminal()) {
            topology.validate_route(&job.route)?;
        }
        Ok(Arc::new(Self {
            host,
            store,
            jcl_limits,
            limits,
            scheduler: Mutex::new(DurableScheduler {
                store_version: scheduler_store_version,
                configuration: scheduler,
            }),
            topology: Mutex::new(DurableTopology {
                store_version: topology_store_version,
                configuration: topology,
            }),
            controllers: Mutex::new(DurableControllers {
                store_version: controller_store_version,
                registry: controller_registry,
            }),
            state: Mutex::new(State {
                jobs,
                replay,
                next_id,
            }),
        }))
    }

    pub fn install_controllers(
        &self,
        generation: BatchControllerGeneration,
    ) -> Result<BatchControllerInstallReceipt, HostProblem> {
        let mut durable = self
            .controllers
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        durable.registry.preflight_install(&generation)?;
        let mut replacement = durable.registry.clone();
        let receipt = replacement.install(generation)?;
        self.persist_controllers(&mut durable, replacement)?;
        Ok(receipt)
    }

    pub fn rollback_controllers(
        &self,
        application: &str,
        generation: u64,
    ) -> Result<BatchControllerInstallReceipt, HostProblem> {
        let mut durable = self
            .controllers
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        let mut replacement = durable.registry.clone();
        let receipt = replacement.select(application, generation)?;
        self.persist_controllers(&mut durable, replacement)?;
        Ok(receipt)
    }

    pub fn submit(
        &self,
        invocation: &Invocation,
        bundle: &JclBundle,
        key: &IdempotencyKey,
        hold: bool,
    ) -> Result<JobSnapshot, HostProblem> {
        self.submit_with_origin(
            invocation,
            bundle,
            key,
            hold,
            JesJobKind::Batch,
            JesSubmissionOrigin::External,
        )
    }

    pub fn start_task(
        &self,
        invocation: &Invocation,
        task_name: &str,
        bundle: &JclBundle,
        key: &IdempotencyKey,
    ) -> Result<JobSnapshot, HostProblem> {
        validate_jes_name(task_name)?;
        self.submit_with_origin(
            invocation,
            bundle,
            key,
            false,
            JesJobKind::StartedTask,
            JesSubmissionOrigin::StartedTask {
                task_name: task_name.to_ascii_uppercase(),
            },
        )
    }

    fn submit_internal_reader(
        &self,
        invocation: &Invocation,
        parent_job_id: &str,
        step_name: &str,
        bundle: &JclBundle,
        key: &IdempotencyKey,
    ) -> Result<JobSnapshot, HostProblem> {
        self.submit_with_origin(
            invocation,
            bundle,
            key,
            false,
            JesJobKind::Batch,
            JesSubmissionOrigin::InternalReader {
                parent_job_id: parent_job_id.into(),
                step_name: step_name.into(),
            },
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn submit_with_origin(
        &self,
        invocation: &Invocation,
        bundle: &JclBundle,
        key: &IdempotencyKey,
        hold: bool,
        kind: JesJobKind,
        origin: JesSubmissionOrigin,
    ) -> Result<JobSnapshot, HostProblem> {
        let plan = parse_jcl(bundle, self.jcl_limits)?;
        let (security_class, resource) = match &origin {
            JesSubmissionOrigin::External => ("JESJOBS", format!("JOB.{}", plan.name)),
            JesSubmissionOrigin::InternalReader { parent_job_id, .. } => {
                ("JESJOBS", format!("JOB.{parent_job_id}.INTRDR"))
            }
            JesSubmissionOrigin::StartedTask { task_name } => {
                ("STARTED", format!("{task_name}.{}", plan.name))
            }
        };
        self.authorize(
            invocation,
            security_class,
            &resource,
            AccessIntent::Execute,
            1,
        )?;
        let class = self
            .scheduler
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)?
            .configuration
            .classes
            .get(&plan.class)
            .cloned()
            .ok_or(HostProblem::UnsupportedCapability {
                capability: "jes.class".into(),
                detail: format!("job class {} is not configured", plan.class),
            })?;
        if plan.priority < class.priority_floor || plan.priority > class.priority_ceiling {
            return Err(HostProblem::Condition {
                name: "PRIORITY_OUT_OF_CLASS_RANGE".into(),
                response: 400,
                response2: 0,
            });
        }
        let route = self.default_route()?;
        let topology = self.topology()?;
        let inbound_limit = topology
            .nodes
            .get(&route.execution_node)
            .ok_or(HostProblem::InfrastructureFailure)?
            .max_inbound_jobs;
        let mut state = self.lock()?;
        if let Some(id) = state.replay.get(key.as_str()) {
            let job = state.jobs.get(id).ok_or(HostProblem::UnknownOutcome)?;
            return if job.plan == plan
                && job.owner == invocation.principal.id().as_str()
                && job.kind == kind
                && job.origin == origin
            {
                Ok(snapshot(job))
            } else {
                Err(HostProblem::IdempotencyConflict)
            };
        }
        if state.jobs.len() >= self.limits.max_jobs
            || state
                .jobs
                .values()
                .filter(|job| job.state == JobState::Queued)
                .count()
                >= self.limits.max_queued
            || state
                .jobs
                .values()
                .filter(|job| {
                    !job.state.terminal() && job.route.execution_node == route.execution_node
                })
                .count()
                >= inbound_limit
        {
            return Err(HostProblem::ResourceExhausted);
        }
        let id = format!("JOB{:05}", state.next_id);
        let previous_meta = state.next_id;
        state.next_id = state
            .next_id
            .checked_add(1)
            .ok_or(HostProblem::ResourceExhausted)?;
        self.persist_meta(state.next_id, previous_meta)?;
        let jcl_records = plan
            .source
            .lines()
            .map(|line| line.as_bytes().to_vec())
            .collect();
        let program_registrations = resolve_plan_programs(&plan)?;
        let origin_event = match &origin {
            JesSubmissionOrigin::External => "origin:external".into(),
            JesSubmissionOrigin::InternalReader {
                parent_job_id,
                step_name,
            } => format!("origin:internal-reader:{parent_job_id}:{step_name}"),
            JesSubmissionOrigin::StartedTask { task_name } => {
                format!("origin:started-task:{task_name}")
            }
        };
        let mut job = Job {
            schema_version: JES_DURABLE_JOB_CONTRACT.into(),
            id: id.clone(),
            name: plan.name.clone(),
            owner: invocation.principal.id().as_str().into(),
            kind,
            origin,
            route,
            class: plan.class,
            priority: plan.priority,
            state: if hold || class.held_by_default {
                JobState::Held
            } else {
                JobState::Queued
            },
            return_code: None,
            abend_code: None,
            active_step: None,
            initiator: None,
            steps: initial_step_executions(&plan),
            attempt: 0,
            version: 1,
            submit_key: key.as_str().into(),
            plan,
            program_registrations,
            dataset_resolutions: BTreeMap::new(),
            temporary_datasets: Vec::new(),
            spool_sequence: 0,
            spool_files: BTreeMap::new(),
            output_groups: BTreeMap::new(),
            spool: BTreeMap::new(),
            events: vec![
                "submitted".into(),
                "admitted".into(),
                origin_event,
                if hold || class.held_by_default {
                    "held"
                } else {
                    "queued"
                }
                .into(),
            ],
        };
        if let Err(problem) = (|| {
            self.append_spool_records(invocation, &mut job, None, None, "JESJCL", jcl_records)?;
            self.append_spool_records(
                invocation,
                &mut job,
                None,
                None,
                "JESMSGLG",
                vec![format!("{id} SUBMITTED").into_bytes()],
            )?;
            self.append_spool_records(
                invocation,
                &mut job,
                None,
                None,
                "JOBLOG",
                vec![format!("{id} ADMITTED").into_bytes()],
            )
        })() {
            let _ = self.purge_spool_authority(invocation, &mut job);
            return Err(problem);
        }
        if let Err(problem) = self.persist_job(&job, None) {
            return match self.purge_spool_authority(invocation, &mut job) {
                Ok(SpoolResult::Mutated { .. }) => Err(problem),
                Ok(SpoolResult::PurgePending { .. }) | Err(_) => Err(HostProblem::UnknownOutcome),
                Ok(_) => Err(HostProblem::ProviderFailure),
            };
        }
        let result = snapshot(&job);
        state.replay.insert(key.as_str().into(), id.clone());
        state.jobs.insert(id, job);
        Ok(result)
    }

    pub fn hold(&self, invocation: &Invocation, id: &str) -> Result<JobSnapshot, HostProblem> {
        self.transition(invocation, id, JobState::Queued, JobState::Held, "held")
    }

    pub fn release(&self, invocation: &Invocation, id: &str) -> Result<JobSnapshot, HostProblem> {
        self.transition(invocation, id, JobState::Held, JobState::Queued, "released")
    }

    pub fn cancel(&self, invocation: &Invocation, id: &str) -> Result<JobSnapshot, HostProblem> {
        self.ensure_spool_migrated(invocation, id)?;
        let mut state = self.lock()?;
        let current = state.jobs.get(id).cloned().ok_or(HostProblem::NotFound)?;
        self.authorize(
            invocation,
            "JESJOBS",
            &format!("JOB.{}", current.name),
            AccessIntent::Alter,
            1,
        )?;
        if !matches!(
            current.state,
            JobState::Queued | JobState::Held | JobState::Running
        ) {
            return Err(HostProblem::Condition {
                name: "INVALID_STATE".into(),
                response: 409,
                response2: 0,
            });
        }
        let mut next = current.clone();
        next.version += 1;
        next.state = JobState::Cancelled;
        next.active_step = None;
        next.events.push("cancelled".into());
        self.append_spool_records(
            invocation,
            &mut next,
            None,
            None,
            "JESMSGLG",
            vec![b"CANCELLED".to_vec()],
        )?;
        self.cancel_output(invocation, &mut next)?;
        self.persist_job(&next, Some(current.version))?;
        let result = snapshot(&next);
        state.jobs.insert(id.into(), next);
        Ok(result)
    }

    pub fn stop_task(
        &self,
        invocation: &Invocation,
        task_name: &str,
        id: &str,
    ) -> Result<JobSnapshot, HostProblem> {
        validate_jes_name(task_name)?;
        let job = self
            .lock()?
            .jobs
            .get(id)
            .cloned()
            .ok_or(HostProblem::NotFound)?;
        if job.kind != JesJobKind::StartedTask
            || !matches!(
                &job.origin,
                JesSubmissionOrigin::StartedTask { task_name: current }
                    if current.eq_ignore_ascii_case(task_name)
            )
        {
            return Err(HostProblem::NotFound);
        }
        self.authorize(
            invocation,
            "STARTED",
            &format!("{}.{}", task_name.to_ascii_uppercase(), job.name),
            AccessIntent::Control,
            1,
        )?;
        self.cancel(invocation, id)
    }

    pub fn change_class(
        &self,
        invocation: &Invocation,
        id: &str,
        class: char,
    ) -> Result<JobSnapshot, HostProblem> {
        let definition = self
            .scheduler
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)?
            .configuration
            .classes
            .get(&class)
            .cloned()
            .ok_or(HostProblem::UnsupportedCapability {
                capability: "jes.class".into(),
                detail: format!("job class {class} is not configured"),
            })?;
        self.mutate_queued_job(invocation, id, "class-changed", |job| {
            if job.priority < definition.priority_floor
                || job.priority > definition.priority_ceiling
            {
                return Err(HostProblem::Condition {
                    name: "PRIORITY_OUT_OF_CLASS_RANGE".into(),
                    response: 400,
                    response2: 0,
                });
            }
            job.class = class;
            Ok(())
        })
    }

    pub fn change_priority(
        &self,
        invocation: &Invocation,
        id: &str,
        priority: u8,
    ) -> Result<JobSnapshot, HostProblem> {
        let scheduler = self
            .scheduler
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)?
            .configuration
            .clone();
        self.mutate_queued_job(invocation, id, "priority-changed", |job| {
            let definition = scheduler
                .classes
                .get(&job.class)
                .ok_or(HostProblem::InfrastructureFailure)?;
            if priority < definition.priority_floor || priority > definition.priority_ceiling {
                return Err(HostProblem::Condition {
                    name: "PRIORITY_OUT_OF_CLASS_RANGE".into(),
                    response: 400,
                    response2: 0,
                });
            }
            job.priority = priority;
            Ok(())
        })
    }

    pub fn topology(&self) -> Result<JesTopology, HostProblem> {
        Ok(self
            .topology
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)?
            .configuration
            .clone())
    }

    pub fn install_topology(
        &self,
        invocation: &Invocation,
        configuration: JesTopology,
    ) -> Result<(), HostProblem> {
        configuration.validate(self.limits.max_nje_nodes, self.limits.max_mas_members)?;
        self.authorize(
            invocation,
            "OPERCMDS",
            "JES2.TOPOLOGY",
            AccessIntent::Alter,
            1,
        )?;
        {
            let state = self.lock()?;
            let mut inbound = BTreeMap::<String, usize>::new();
            for job in state.jobs.values().filter(|job| !job.state.terminal()) {
                configuration.validate_route(&job.route)?;
                let count = inbound.entry(job.route.execution_node.clone()).or_default();
                *count = count.checked_add(1).ok_or(HostProblem::ResourceExhausted)?;
            }
            for (node, count) in inbound {
                if count
                    > configuration
                        .nodes
                        .get(&node)
                        .ok_or(HostProblem::InfrastructureFailure)?
                        .max_inbound_jobs
                {
                    return Err(HostProblem::ResourceExhausted);
                }
            }
        }
        let mut durable = self
            .topology
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        let version = durable
            .store_version
            .checked_add(1)
            .ok_or(HostProblem::ResourceExhausted)?;
        self.store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: TOPOLOGY_STATE_NAMESPACE.into(),
                    key: CONFIGURATION_STATE_KEY.into(),
                    version,
                    payload: serde_json::to_vec(&configuration)
                        .map_err(|_| HostProblem::InfrastructureFailure)?,
                },
                (durable.store_version > 0).then_some(durable.store_version),
            )
            .map_err(store_error)?;
        durable.store_version = version;
        durable.configuration = configuration;
        Ok(())
    }

    pub fn route_job(
        &self,
        invocation: &Invocation,
        id: &str,
        execution_node: &str,
        output_node: &str,
    ) -> Result<JobSnapshot, HostProblem> {
        let execution_node = execution_node.to_ascii_uppercase();
        let output_node = output_node.to_ascii_uppercase();
        let topology = self.topology()?;
        let inbound_limit = topology
            .nodes
            .get(&execution_node)
            .ok_or(HostProblem::NotFound)?
            .max_inbound_jobs;
        if self
            .lock()?
            .jobs
            .values()
            .filter(|job| {
                job.id != id && !job.state.terminal() && job.route.execution_node == execution_node
            })
            .count()
            >= inbound_limit
        {
            return Err(HostProblem::ResourceExhausted);
        }
        self.mutate_queued_job(invocation, id, "job-routed", |job| {
            let previous_output_node = job.route.output_node.clone();
            let route = JesJobRoute {
                origin_node: job.route.origin_node.clone(),
                execution_node: execution_node.clone(),
                output_node: output_node.clone(),
                owner_member: None,
            };
            topology.validate_route(&route)?;
            job.route = route;
            for descriptor in job.spool_files.values_mut() {
                if descriptor.destination == previous_output_node {
                    descriptor.destination.clone_from(&output_node);
                }
            }
            for group in job.output_groups.values_mut() {
                if group.destination == previous_output_node {
                    group.destination.clone_from(&output_node);
                }
            }
            Ok(())
        })
    }

    pub fn start_initiator(
        &self,
        invocation: &Invocation,
        initiator: &str,
    ) -> Result<(), HostProblem> {
        self.set_initiator(invocation, initiator, true)
    }

    pub fn stop_initiator(
        &self,
        invocation: &Invocation,
        initiator: &str,
    ) -> Result<(), HostProblem> {
        self.set_initiator(invocation, initiator, false)
    }

    pub fn run_next(
        &self,
        invocation: &Invocation,
        cancelled: bool,
    ) -> Result<Option<JobSnapshot>, HostProblem> {
        self.run_next_on(invocation, "INIT0001", cancelled)
    }

    pub fn run_next_on(
        &self,
        invocation: &Invocation,
        initiator: &str,
        cancelled: bool,
    ) -> Result<Option<JobSnapshot>, HostProblem> {
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
            if let Some(job) = self.run_next_on_member(invocation, &member, initiator, cancelled)? {
                return Ok(Some(job));
            }
        }
        Ok(None)
    }

    pub fn run_next_on_member(
        &self,
        invocation: &Invocation,
        member: &str,
        initiator: &str,
        cancelled: bool,
    ) -> Result<Option<JobSnapshot>, HostProblem> {
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
                .filter(|job| job.owner == invocation.principal.id().as_str())
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
        self.ensure_spool_migrated(invocation, &id)?;
        if cancelled {
            return self.cancel(invocation, &id).map(Some);
        }
        let mut job = {
            let mut state = self.lock()?;
            let current = state.jobs.get(&id).cloned().ok_or(HostProblem::NotFound)?;
            if current.owner != invocation.principal.id().as_str() {
                return Err(HostProblem::Unauthorized);
            }
            self.authorize(
                invocation,
                "JESJOBS",
                &format!("JOB.{}", current.name),
                AccessIntent::Execute,
                2,
            )?;
            let mut selected = current.clone();
            selected.version += 1;
            selected.state = JobState::Selected;
            selected.initiator = Some(initiator.into());
            selected.route.owner_member = Some(member.clone());
            selected.events.push(format!("selected:{initiator}"));
            self.persist_job(&selected, Some(current.version))?;
            state.jobs.insert(id.clone(), selected.clone());
            let mut running = current.clone();
            running.version = selected.version + 1;
            running.attempt += 1;
            running.state = JobState::Running;
            running.initiator = Some(initiator.into());
            running.route.owner_member = Some(member);
            running.events.push("running".into());
            self.persist_job(&running, Some(selected.version))?;
            state.jobs.insert(id.clone(), running.clone());
            running
        };
        let outcome = self.execute(invocation, &mut job);
        let cleanup = self.cleanup_job_temporary_datasets(invocation, &mut job);
        let outcome = match (outcome, cleanup) {
            (Ok(return_code), Ok(())) => Ok(return_code),
            (Err(problem), Ok(())) | (_, Err(problem)) => Err(problem),
        };
        let mut state = self.lock()?;
        let current = state.jobs.get(&id).cloned().ok_or(HostProblem::NotFound)?;
        job.version = current.version + 1;
        let terminal_step = job.active_step.clone();
        job.active_step = None;
        match outcome {
            Ok(return_code) => {
                job.return_code = Some(return_code);
                job.state = JobState::Output;
                job.events.push("output".into());
                self.append_spool_records(
                    invocation,
                    &mut job,
                    None,
                    None,
                    "JESMSGLG",
                    vec![format!("ENDED RC={return_code:04}").into_bytes()],
                )?;
                self.complete_output(invocation, &mut job)?;
                self.persist_job(&job, Some(current.version))?;
                state.jobs.insert(id.clone(), job.clone());
                let output_version = job.version;
                job.version += 1;
                job.state = JobState::Completed;
                job.initiator = None;
                job.events.push("completed".into());
                self.persist_job(&job, Some(output_version))?;
                let result = snapshot(&job);
                state.jobs.insert(id, job);
                return Ok(Some(result));
            }
            Err(problem) => {
                if let HostProblem::Condition { name, .. } = &problem
                    && let Some(code) = name.strip_prefix("ABEND:")
                {
                    job.abend_code = Some(code.to_string());
                }
                job.state = JobState::Failed;
                job.initiator = None;
                job.events.push(format!("failed:{problem:?}"));
                if let Some(step) = terminal_step {
                    self.append_spool_records(
                        invocation,
                        &mut job,
                        Some(&step),
                        None,
                        "JOBLOG",
                        vec![format!("{step} FAILED {problem:?}").into_bytes()],
                    )?;
                }
                self.append_spool_records(
                    invocation,
                    &mut job,
                    None,
                    None,
                    "JESMSGLG",
                    vec![format!("FAILED {problem:?}").into_bytes()],
                )?;
                self.complete_output(invocation, &mut job)?;
            }
        }
        self.persist_job(&job, Some(current.version))?;
        let result = snapshot(&job);
        state.jobs.insert(id, job);
        Ok(Some(result))
    }

    pub fn drain_queued(&self, invocation: &Invocation) -> Result<Vec<JobSnapshot>, HostProblem> {
        let mut completed = Vec::new();
        for _ in 0..self.limits.max_queued {
            let Some(job) = self.run_next(invocation, false)? else {
                return Ok(completed);
            };
            completed.push(job);
        }
        if self.lock()?.jobs.values().any(|job| {
            job.state == JobState::Queued && job.owner == invocation.principal.id().as_str()
        }) {
            Err(HostProblem::ResourceExhausted)
        } else {
            Ok(completed)
        }
    }

    fn execute(&self, invocation: &Invocation, job: &mut Job) -> Result<i32, HostProblem> {
        let mut max_rc = 0;
        let mut abended = false;
        let mut first_abend = None::<String>;
        let steps = job.plan.steps.clone();
        let restart = job.plan.restart_step.clone();
        let mut restart_reached = restart.is_none();
        let mut effect_sequence = 0u64;
        for step in &steps {
            if let Some(previous) = job.steps.iter().find(|current| current.name == step.name)
                && previous.state.terminal()
            {
                if let Some(StepTermination::ReturnCode { code }) = &previous.termination {
                    max_rc = max_rc.max(*code);
                }
                continue;
            }
            if let Some(cancellation) = &invocation.cancellation {
                self.mark_step(
                    job,
                    step,
                    StepState::Cancelled,
                    Some(StepTermination::Cancelled {
                        reason: cancellation.reason.clone(),
                    }),
                )?;
                return Err(HostProblem::Cancelled);
            }
            if !restart_reached {
                restart_reached = restart.as_deref() == Some(step.name.as_str());
                if !restart_reached {
                    self.append_spool_records(
                        invocation,
                        job,
                        Some(&step.name),
                        None,
                        "JOBLOG",
                        vec![format!("{} BYPASSED RESTART", step.name).into_bytes()],
                    )?;
                    self.mark_step(job, step, StepState::BypassedRestart, None)?;
                    continue;
                }
            }
            if !step.condition.should_run(max_rc, abended) {
                self.append_spool_records(
                    invocation,
                    job,
                    Some(&step.name),
                    None,
                    "JOBLOG",
                    vec![format!("{} SKIPPED COND", step.name).into_bytes()],
                )?;
                self.mark_step(job, step, StepState::SkippedCondition, None)?;
                continue;
            }
            job.active_step = Some(step.name.clone());
            self.mark_step(job, step, StepState::Allocating, None)?;
            let allocation_plans = plan_dd_allocations(&step.dds)?;
            let mut allocations = Vec::new();
            let step_result = (|| -> Result<crate::ProgramOutput, HostProblem> {
                allocations = self.allocate_dds(
                    invocation,
                    job,
                    step,
                    &allocation_plans,
                    &mut effect_sequence,
                )?;
                let mut dds = step.dds.clone();
                self.hydrate_dds(
                    invocation,
                    job,
                    &allocations,
                    &mut dds,
                    &mut effect_sequence,
                )?;
                self.mark_step(job, step, StepState::Running, None)?;
                for dd in &mut dds {
                    if !is_program_library_dd(dd)
                        && let Some(raw_name) = dd.dataset.clone()
                    {
                        dd.dataset = Some(resolved_dataset(
                            job,
                            dd,
                            &raw_name,
                            &job.dataset_resolutions,
                        ));
                    }
                }
                let input = ProgramInput {
                    parameter: step.parameter.clone(),
                    dds,
                };
                let registration = job
                    .program_registrations
                    .get(&step.name)
                    .filter(|registration| registration.program == step.program)
                    .cloned()
                    .ok_or(HostProblem::InfrastructureFailure)?;
                let dataset_resolutions = job.dataset_resolutions.clone();
                let idcams_return_code = if registration.handler
                    == RegisteredProgramHandler::Utility(crate::UtilityHandler::Idcams)
                {
                    Some(self.execute_idcams(
                        invocation,
                        job,
                        step,
                        &dataset_resolutions,
                        &input,
                        &mut effect_sequence,
                    )?)
                } else {
                    None
                };
                let mut output = match registration.handler {
                    RegisteredProgramHandler::Sdsf => {
                        self.execute_sdsf(invocation, job, step, &input, &mut effect_sequence)?
                    }
                    RegisteredProgramHandler::Db2Tso => {
                        self.execute_db2_tso(invocation, job, step, &input, &mut effect_sequence)?
                    }
                    RegisteredProgramHandler::ImsController => self.execute_ims_controller(
                        invocation,
                        job,
                        step,
                        &input,
                        &mut effect_sequence,
                    )?,
                    RegisteredProgramHandler::ProgramService
                    | RegisteredProgramHandler::Utility(_) => self.execute_program_controller(
                        invocation,
                        job,
                        step,
                        &input,
                        &mut effect_sequence,
                        &registration.program,
                    )?,
                    RegisteredProgramHandler::Unsupported => return Err(HostProblem::Unsupported),
                };
                if let Some(return_code) = idcams_return_code {
                    output.return_code = return_code;
                }
                self.write_dd_outputs(
                    invocation,
                    job,
                    step,
                    &dataset_resolutions,
                    &allocations,
                    &output.dd_outputs,
                    &mut effect_sequence,
                )?;
                Ok(output)
            })();
            let output = match step_result {
                Ok(output) => output,
                Err(problem) => {
                    let disposition = self.dispose_dds(
                        invocation,
                        job,
                        step,
                        &allocations,
                        &mut effect_sequence,
                        true,
                    );
                    let terminal_problem = disposition.err().unwrap_or(problem);
                    if let Some(code) = abend_code(&terminal_problem) {
                        first_abend.get_or_insert_with(|| code.clone());
                        job.abend_code = Some(code.clone());
                        self.mark_step(
                            job,
                            step,
                            StepState::Abended,
                            Some(StepTermination::Abend { code, system: true }),
                        )?;
                        abended = true;
                        continue;
                    }
                    let state = if terminal_problem == HostProblem::Cancelled {
                        StepState::Cancelled
                    } else {
                        StepState::Failed
                    };
                    self.mark_step(
                        job,
                        step,
                        state,
                        Some(if state == StepState::Cancelled {
                            StepTermination::Cancelled {
                                reason: "execution cancellation observed".into(),
                            }
                        } else {
                            StepTermination::Failed {
                                category: problem_category(&terminal_problem).into(),
                            }
                        }),
                    )?;
                    return Err(terminal_problem);
                }
            };
            if !(0..=4_095).contains(&output.return_code) {
                self.mark_step(
                    job,
                    step,
                    StepState::Failed,
                    Some(StepTermination::Failed {
                        category: "invalid-return-code".into(),
                    }),
                )?;
                return Err(HostProblem::ProviderFailure);
            }
            max_rc = max_rc.max(output.return_code);
            self.append_spool_records(
                invocation,
                job,
                Some(&step.name),
                step.dds
                    .iter()
                    .find(|dd| dd.name.eq_ignore_ascii_case("SYSPRINT")),
                "SYSPRINT",
                output.records,
            )?;
            self.append_spool_records(
                invocation,
                job,
                Some(&step.name),
                None,
                "JOBLOG",
                vec![
                    format!(
                        "{} {} RC={:04}",
                        step.name, step.program, output.return_code
                    )
                    .into_bytes(),
                ],
            )?;
            self.mark_step(job, step, StepState::Disposing, None)?;
            if let Err(problem) = self.dispose_dds(
                invocation,
                job,
                step,
                &allocations,
                &mut effect_sequence,
                false,
            ) {
                self.mark_step(
                    job,
                    step,
                    StepState::Failed,
                    Some(StepTermination::Failed {
                        category: problem_category(&problem).into(),
                    }),
                )?;
                return Err(problem);
            }
            self.mark_step(
                job,
                step,
                StepState::Completed,
                Some(StepTermination::ReturnCode {
                    code: output.return_code,
                }),
            )?;
        }
        if let Some(code) = first_abend {
            Err(HostProblem::Condition {
                name: format!("ABEND:{code}"),
                response: 500,
                response2: 0,
            })
        } else {
            Ok(max_rc)
        }
    }

    fn mark_step(
        &self,
        job: &mut Job,
        step: &StepPlan,
        next: StepState,
        termination: Option<StepTermination>,
    ) -> Result<(), HostProblem> {
        let execution = job
            .steps
            .iter_mut()
            .find(|current| current.name == step.name)
            .ok_or(HostProblem::InfrastructureFailure)?;
        if !execution.state.can_transition_to(next)
            || next.terminal() != termination.is_some()
                && !matches!(
                    next,
                    StepState::BypassedRestart | StepState::SkippedCondition
                )
        {
            return Err(HostProblem::InfrastructureFailure);
        }
        if next == StepState::Allocating {
            execution.attempt = execution
                .attempt
                .checked_add(1)
                .ok_or(HostProblem::ResourceExhausted)?;
        }
        execution.state = next;
        execution.termination = termination;
        if job.events.len() >= self.limits.max_events {
            return Err(HostProblem::ResourceExhausted);
        }
        job.events.push(format!("step:{}:{next:?}", step.name));
        let expected = job.version;
        job.version = job
            .version
            .checked_add(1)
            .ok_or(HostProblem::ResourceExhausted)?;
        let mut state = self.lock()?;
        let current = state.jobs.get(&job.id).ok_or(HostProblem::NotFound)?;
        if current.version != expected || current.state != JobState::Running {
            return Err(HostProblem::IdempotencyConflict);
        }
        self.persist_job(job, Some(expected))?;
        state.jobs.insert(job.id.clone(), job.clone());
        Ok(())
    }

    fn cleanup_job_temporary_datasets(
        &self,
        invocation: &Invocation,
        job: &mut Job,
    ) -> Result<(), HostProblem> {
        let mut sequence = 0u64;
        for dataset in job.temporary_datasets.clone() {
            self.authorize(
                invocation,
                "DATASET",
                &dataset,
                AccessIntent::Update,
                next_effect_sequence(invocation, &mut sequence)?,
            )?;
            let sequence_value = next_effect_sequence(invocation, &mut sequence)?;
            let key = IdempotencyKey::new(
                format!("jes:{}:job-cleanup:{sequence_value}", job.id),
                InvocationLimits::default(),
            )
            .map_err(|_| HostProblem::ResourceExhausted)?;
            let result = self.host.invoke(
                invocation,
                invocation.deadline_tick.saturating_sub(1),
                false,
                EffectRequest {
                    run_unit: invocation.run_unit_id.clone(),
                    sequence: sequence_value,
                    deadline_tick: invocation.deadline_tick,
                    idempotency_key: Some(key.clone()),
                    request: HostRequest::Dataset(DatasetRequest::Delete {
                        dataset: DatasetName::new(&dataset, 128)
                            .map_err(|_| HostProblem::InfrastructureFailure)?,
                        member: None,
                        expected_version: None,
                        purge: true,
                        current_date: None,
                        mutation: Mutation {
                            sequence: sequence_value,
                            idempotency_key: key,
                            transaction: Some(job.id.clone()),
                        },
                    }),
                },
            );
            match result.effect.outcome {
                Ok(HostResult::Dataset(DatasetResult::Mutated { .. }))
                | Err(HostProblem::NotFound) => {
                    job.temporary_datasets.retain(|current| current != &dataset);
                }
                Ok(_) => return Err(HostProblem::ProviderFailure),
                Err(problem) => return Err(problem),
            }
        }
        Ok(())
    }

    fn append_spool_records(
        &self,
        invocation: &Invocation,
        job: &mut Job,
        step_name: Option<&str>,
        dd: Option<&crate::DdPlan>,
        file: &str,
        records: Vec<Vec<u8>>,
    ) -> Result<(), HostProblem> {
        if records.is_empty() {
            return Ok(());
        }
        let internal_name = spool_internal_name(step_name, file);
        let resource = spool_resource(job, &internal_name);
        let authorize_sequence = next_spool_sequence(invocation, job)?;
        self.authorize(
            invocation,
            "JESJOBS",
            &resource,
            AccessIntent::Update,
            authorize_sequence,
        )?;
        let sequence = next_spool_sequence(invocation, job)?;
        let key = IdempotencyKey::new(
            format!("jes:{}:spool:{sequence}", job.id),
            InvocationLimits::default(),
        )
        .map_err(|_| HostProblem::ResourceExhausted)?;
        let record_count = records.len();
        let byte_count = records
            .iter()
            .try_fold(0usize, |total, record| total.checked_add(record.len()))
            .ok_or(HostProblem::ResourceExhausted)?;
        let result = self.host.invoke(
            invocation,
            invocation.deadline_tick.saturating_sub(1),
            false,
            EffectRequest {
                run_unit: invocation.run_unit_id.clone(),
                sequence,
                deadline_tick: invocation.deadline_tick,
                idempotency_key: Some(key.clone()),
                request: HostRequest::Spool(SpoolRequest::Append {
                    job: JobName::new(&job.id, 128).map_err(|_| HostProblem::Malformed)?,
                    file: internal_name.clone(),
                    records,
                    mutation: Mutation {
                        sequence,
                        idempotency_key: key,
                        transaction: Some(job.id.clone()),
                    },
                }),
            },
        );
        let HostResult::Spool(SpoolResult::Mutated { version, .. }) = result.effect.outcome? else {
            return Err(HostProblem::ProviderFailure);
        };
        let route = spool_route(job, dd);
        let output_id = output_group_id(job, &route);
        let descriptor = job
            .spool_files
            .entry(internal_name.clone())
            .or_insert_with(|| JesSpoolDescriptor {
                schema_version: JES_SPOOL_CONTRACT.into(),
                id: spool_descriptor_id(&job.id, &internal_name),
                job_id: job.id.clone(),
                step_name: step_name.map(str::to_string),
                dd_name: file.to_ascii_uppercase(),
                class: route.class,
                destination: route.destination.clone(),
                writer: route.writer.clone(),
                forms: route.forms.clone(),
                record_count: 0,
                byte_count: 0,
                created_tick: 0,
                retain_until_tick: self.limits.spool_retention_ticks,
                state: if route.hold {
                    JesSpoolState::Held
                } else {
                    JesSpoolState::Open
                },
            });
        descriptor.record_count = descriptor
            .record_count
            .checked_add(record_count)
            .ok_or(HostProblem::ResourceExhausted)?;
        descriptor.byte_count = descriptor
            .byte_count
            .checked_add(byte_count)
            .ok_or(HostProblem::ResourceExhausted)?;
        let retain_until_tick = descriptor.retain_until_tick;
        let internal_name_for_group = internal_name.clone();
        let _ = descriptor;
        let group = job
            .output_groups
            .entry(output_id.clone())
            .or_insert_with(|| JesOutputGroup {
                schema_version: JES_OUTPUT_CONTRACT.into(),
                id: output_id,
                job_id: job.id.clone(),
                class: route.class,
                destination: route.destination,
                writer: route.writer,
                forms: route.forms,
                copies: route.copies,
                retain_until_tick,
                state: if route.hold {
                    JesOutputState::Held
                } else {
                    JesOutputState::AwaitingSelection
                },
                spool_files: Vec::new(),
            });
        if !group
            .spool_files
            .iter()
            .any(|file| file == &internal_name_for_group)
        {
            if group.spool_files.len() >= self.limits.max_spool_files {
                return Err(HostProblem::ResourceExhausted);
            }
            group.spool_files.push(internal_name_for_group);
        }
        let _ = version;
        Ok(())
    }

    fn complete_output(&self, invocation: &Invocation, job: &mut Job) -> Result<(), HostProblem> {
        self.seal_output(invocation, job, false)
    }

    fn cancel_output(&self, invocation: &Invocation, job: &mut Job) -> Result<(), HostProblem> {
        self.seal_output(invocation, job, true)
    }

    fn seal_output(
        &self,
        invocation: &Invocation,
        job: &mut Job,
        cancelled: bool,
    ) -> Result<(), HostProblem> {
        let files = job.spool_files.keys().cloned().collect::<Vec<_>>();
        for file in files {
            let resource = spool_resource(job, &file);
            let authorize_sequence = next_spool_sequence(invocation, job)?;
            self.authorize(
                invocation,
                "JESJOBS",
                &resource,
                AccessIntent::Update,
                authorize_sequence,
            )?;
            let sequence = next_spool_sequence(invocation, job)?;
            let key = IdempotencyKey::new(
                format!("jes:{}:spool:{sequence}", job.id),
                InvocationLimits::default(),
            )
            .map_err(|_| HostProblem::ResourceExhausted)?;
            let result = self.host.invoke(
                invocation,
                invocation.deadline_tick.saturating_sub(1),
                false,
                EffectRequest {
                    run_unit: invocation.run_unit_id.clone(),
                    sequence,
                    deadline_tick: invocation.deadline_tick,
                    idempotency_key: Some(key.clone()),
                    request: HostRequest::Spool(SpoolRequest::Seal {
                        job: JobName::new(&job.id, 128).map_err(|_| HostProblem::Malformed)?,
                        file: file.clone(),
                        mutation: Mutation {
                            sequence,
                            idempotency_key: key,
                            transaction: Some(job.id.clone()),
                        },
                    }),
                },
            );
            let HostResult::Spool(SpoolResult::Mutated { .. }) = result.effect.outcome? else {
                return Err(HostProblem::ProviderFailure);
            };
            let descriptor = job
                .spool_files
                .get_mut(&file)
                .ok_or(HostProblem::InfrastructureFailure)?;
            descriptor.state = if cancelled {
                JesSpoolState::Cancelled
            } else if descriptor.state == JesSpoolState::Held {
                JesSpoolState::Held
            } else {
                JesSpoolState::Closed
            };
        }
        for group in job.output_groups.values_mut() {
            group.state = if cancelled {
                JesOutputState::Cancelled
            } else if group.state == JesOutputState::Held {
                JesOutputState::Held
            } else {
                JesOutputState::AwaitingSelection
            };
        }
        Ok(())
    }

    fn purge_spool_authority(
        &self,
        invocation: &Invocation,
        job: &mut Job,
    ) -> Result<SpoolResult, HostProblem> {
        let resource = format!("JOB.{}", job.name);
        let authorize_sequence = next_spool_sequence(invocation, job)?;
        self.authorize(
            invocation,
            "JESJOBS",
            &resource,
            AccessIntent::Alter,
            authorize_sequence,
        )?;
        let sequence = next_spool_sequence(invocation, job)?;
        let key = IdempotencyKey::new(
            format!("jes:{}:spool:{sequence}", job.id),
            InvocationLimits::default(),
        )
        .map_err(|_| HostProblem::ResourceExhausted)?;
        let outcome = self.host.invoke(
            invocation,
            invocation.deadline_tick.saturating_sub(1),
            false,
            EffectRequest {
                run_unit: invocation.run_unit_id.clone(),
                sequence,
                deadline_tick: invocation.deadline_tick,
                idempotency_key: Some(key.clone()),
                request: HostRequest::Spool(SpoolRequest::Purge {
                    job: JobName::new(&job.id, 128).map_err(|_| HostProblem::Malformed)?,
                    mutation: Mutation {
                        sequence,
                        idempotency_key: key,
                        transaction: Some(job.id.clone()),
                    },
                }),
            },
        );
        match outcome.effect.outcome? {
            HostResult::Spool(result @ SpoolResult::Mutated { .. })
            | HostResult::Spool(result @ SpoolResult::PurgePending { .. }) => Ok(result),
            _ => Err(HostProblem::ProviderFailure),
        }
    }

    fn execute_sdsf(
        &self,
        invocation: &Invocation,
        job: &Job,
        step: &StepPlan,
        input: &ProgramInput,
        effect_sequence: &mut u64,
    ) -> Result<crate::ProgramOutput, HostProblem> {
        let controls = parse_sdsf_file_controls(&input_dd_text(input, "ISFIN")?)?;
        let arguments = controls
            .iter()
            .map(|control| {
                BoundedPayload::new(
                    "mainframe-env.cics.file-status@1",
                    control.status.as_bytes().to_vec(),
                    InvocationLimits::default(),
                )
                .map(|payload| (control.file.clone(), payload))
                .map_err(|_| HostProblem::ResourceExhausted)
            })
            .collect::<Result<BTreeMap<_, _>, _>>()?;
        if arguments.len() != controls.len() {
            return Err(HostProblem::Malformed);
        }
        let sequence = next_effect_sequence(invocation, effect_sequence)?;
        let key = effect_key(job, step, sequence)?;
        let result = self.host.invoke(
            invocation,
            invocation.deadline_tick.saturating_sub(1),
            false,
            EffectRequest {
                run_unit: invocation.run_unit_id.clone(),
                sequence,
                deadline_tick: invocation.deadline_tick,
                idempotency_key: Some(key.clone()),
                request: HostRequest::Cics(CicsRequest {
                    operation: CicsOperation::SetFileStatus,
                    arguments,
                    condition_policy: CicsConditionPolicy::Default,
                    mutation: Some(Mutation {
                        sequence,
                        idempotency_key: key,
                        transaction: Some(
                            invocation
                                .bindings
                                .get("cics.transaction")
                                .and_then(|value| std::str::from_utf8(value.bytes()).ok())
                                .unwrap_or("DEFAULT")
                                .to_ascii_uppercase(),
                        ),
                    }),
                }),
            },
        );
        match result.effect.outcome? {
            HostResult::Cics(response)
                if response.disposition == CicsDisposition::Complete
                    && response.condition == "NORMAL" => {}
            HostResult::Cics(_) => return Err(HostProblem::ProviderFailure),
            _ => return Err(HostProblem::ProviderFailure),
        }
        let command_output = controls
            .iter()
            .map(|control| format!("{} {}", control.file, control.status).into_bytes())
            .collect::<Vec<_>>();
        Ok(crate::ProgramOutput {
            return_code: 0,
            records: vec![format!("SDSF CICS FILE CONTROL {}", controls.len()).into_bytes()],
            dd_outputs: BTreeMap::from([
                ("CMDOUT".into(), command_output),
                (
                    "ISFOUT".into(),
                    vec![format!("{} COMMANDS COMPLETED", controls.len()).into_bytes()],
                ),
            ]),
        })
    }

    fn execute_db2_tso(
        &self,
        invocation: &Invocation,
        job: &Job,
        step: &StepPlan,
        input: &ProgramInput,
        effect_sequence: &mut u64,
    ) -> Result<crate::ProgramOutput, HostProblem> {
        let control = input_dd_text(input, "SYSTSIN")?;
        if control.to_ascii_uppercase().contains("FREE PLAN")
            || control.to_ascii_uppercase().contains("FREE PACKAGE")
        {
            let result = self.db2_call(
                invocation,
                job,
                step,
                effect_sequence,
                Db2Operation::FreePlans,
                control,
                0,
            )?;
            return Ok(crate::ProgramOutput {
                return_code: i32::from(result.sqlcode != 0) * 8,
                records: vec![format!("IKJEFT01 SQLCODE={}", result.sqlcode).into_bytes()],
                dd_outputs: BTreeMap::new(),
            });
        }
        let program = tso_run_program(&control)?;
        let selector = BatchControllerSelector::tso(&program)?;
        if let Some(controller) = self.resolve_controller(&selector)? {
            return match controller.plan {
                BatchControllerPlan::ProgramCall => {
                    let program = self.verify_controller_program(&controller.program)?;
                    self.execute_program_controller(
                        invocation,
                        job,
                        step,
                        input,
                        effect_sequence,
                        &program,
                    )
                }
                _ => Err(HostProblem::ProviderFailure),
            };
        }
        let statement = input_dd_text(input, "SYSIN")?;
        let operation = match tso_program_execution(&program).ok_or(HostProblem::Unsupported)? {
            TsoProgramExecution::ExecuteScript => Db2Operation::ExecuteScript,
            TsoProgramExecution::Extract => Db2Operation::Extract,
        };
        let result = self.db2_call(
            invocation,
            job,
            step,
            effect_sequence,
            operation,
            statement,
            4_096,
        )?;
        let dd_outputs = if operation == Db2Operation::Extract {
            let ccsid = input
                .dds
                .iter()
                .find(|dd| dd.name.eq_ignore_ascii_case("SYSREC00"))
                .and_then(|dd| dd.ccsid);
            BTreeMap::from([(
                "SYSREC00".into(),
                result
                    .rows
                    .iter()
                    .map(|row| {
                        let record = row
                            .columns
                            .first()
                            .cloned()
                            .ok_or(HostProblem::ProviderFailure)?;
                        match ccsid {
                            None | Some(1208) => Ok(record),
                            Some(37) => mainframe_env_encoding::CodePage::Cp037
                                .encode(
                                    std::str::from_utf8(&record)
                                        .map_err(|_| HostProblem::ProviderFailure)?,
                                    record.len().saturating_mul(4).max(1),
                                )
                                .map_err(|_| HostProblem::ProviderFailure),
                            Some(_) => Err(HostProblem::Unsupported),
                        }
                    })
                    .collect::<Result<Vec<_>, _>>()?,
            )])
        } else {
            BTreeMap::new()
        };
        Ok(crate::ProgramOutput {
            return_code: i32::from(result.sqlcode != 0) * 8,
            records: vec![
                format!(
                    "IKJEFT01 {program} SQLCODE={} ROWS={}",
                    result.sqlcode,
                    result.rows.len()
                )
                .into_bytes(),
            ],
            dd_outputs,
        })
    }

    fn resolve_controller(
        &self,
        selector: &BatchControllerSelector,
    ) -> Result<Option<ResolvedBatchController>, HostProblem> {
        self.controllers
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)
            .map(|durable| durable.registry.resolve(selector))
    }

    fn verify_controller_program(
        &self,
        program: &crate::BatchControllerProgram,
    ) -> Result<String, HostProblem> {
        let name = program
            .path
            .rsplit('/')
            .next()
            .ok_or(HostProblem::InfrastructureFailure)?
            .to_ascii_uppercase();
        let record = self
            .store
            .get_provider_state("batch-program", &name)
            .map_err(store_error)?
            .ok_or(HostProblem::NotFound)?;
        if record.payload != program.identity.as_bytes() {
            return Err(HostProblem::IdempotencyConflict);
        }
        Ok(name)
    }

    fn execute_program_controller(
        &self,
        invocation: &Invocation,
        job: &Job,
        step: &StepPlan,
        input: &ProgramInput,
        effect_sequence: &mut u64,
        program: &str,
    ) -> Result<crate::ProgramOutput, HostProblem> {
        let payload = BoundedPayload::new(
            "mainframe-env.program.input@1",
            serde_json::to_vec(input).map_err(|_| HostProblem::ProviderFailure)?,
            InvocationLimits::default(),
        )
        .map_err(|_| HostProblem::ResourceExhausted)?;
        let sequence = next_effect_sequence(invocation, effect_sequence)?;
        let key = effect_key(job, step, sequence)?;
        let result = self.host.invoke(
            invocation,
            invocation.deadline_tick.saturating_sub(1),
            false,
            EffectRequest {
                run_unit: invocation.run_unit_id.clone(),
                sequence,
                deadline_tick: invocation.deadline_tick,
                idempotency_key: Some(key),
                request: HostRequest::Program(ProgramRequest::Call {
                    program: ProgramName::new(program, 128).map_err(|_| HostProblem::Malformed)?,
                    payload,
                    service: None,
                }),
            },
        );
        match result.effect.outcome? {
            HostResult::Program(payload) => decode_program_output(&payload),
            _ => Err(HostProblem::ProviderFailure),
        }
    }

    fn execute_ims_controller(
        &self,
        invocation: &Invocation,
        job: &Job,
        step: &StepPlan,
        input: &ProgramInput,
        effect_sequence: &mut u64,
    ) -> Result<crate::ProgramOutput, HostProblem> {
        let selector = ims_controller_selector(input.parameter.as_deref().unwrap_or_default())?;
        let controller = self
            .resolve_controller(&selector)?
            .ok_or(HostProblem::Unsupported)?;
        let program = controller
            .program
            .path
            .rsplit('/')
            .next()
            .ok_or(HostProblem::InfrastructureFailure)?
            .to_string();
        let mode = controller.selector.mode().unwrap_or_default().to_string();
        match controller.plan {
            BatchControllerPlan::ProgramCall => Err(HostProblem::ProviderFailure),
            BatchControllerPlan::ImsLoad {
                database,
                root_dd,
                child_dd,
                root_record_bytes,
                child_record_bytes,
                parent_key_bytes,
            } => {
                let roots = input_dd_records(input, &root_dd)?;
                let children = input_dd_records(input, &child_dd)?;
                let mut hierarchy = roots
                    .into_iter()
                    .map(|data| {
                        if data.len() != root_record_bytes || data.len() < parent_key_bytes {
                            return Err(HostProblem::Malformed);
                        }
                        Ok((
                            data[..parent_key_bytes].to_vec(),
                            (data, Vec::<Vec<u8>>::new()),
                        ))
                    })
                    .collect::<Result<BTreeMap<_, _>, _>>()?;
                for record in children {
                    if record.len() != child_record_bytes || record.len() < parent_key_bytes {
                        return Err(HostProblem::Malformed);
                    }
                    hierarchy
                        .get_mut(&record[..parent_key_bytes])
                        .ok_or(HostProblem::Malformed)?
                        .1
                        .push(record[parent_key_bytes..].to_vec());
                }
                let image = serde_json::json!({
                    "database": database,
                    "roots": hierarchy.into_values().map(|(data, children)| {
                        serde_json::json!({"data": data, "children": children})
                    }).collect::<Vec<_>>()
                });
                let result = self.ims_call(
                    invocation,
                    job,
                    step,
                    effect_sequence,
                    ImsOperation::Load,
                    Some(database),
                    serde_json::to_vec(&image).map_err(|_| HostProblem::ProviderFailure)?,
                    1,
                )?;
                Ok(crate::ProgramOutput {
                    return_code: i32::from(result.status != "  ") * 8,
                    records: vec![
                        format!("DFSRRC00 LOAD SEGMENTS={}", result.affected_segments).into_bytes(),
                    ],
                    dd_outputs: BTreeMap::new(),
                })
            }
            BatchControllerPlan::ImsUnload {
                database,
                root_segment,
                child_segment,
                root_output_dd,
                child_output_dd,
                combined_output_dd,
            } => {
                let result = self.ims_call(
                    invocation,
                    job,
                    step,
                    effect_sequence,
                    ImsOperation::Unload,
                    Some(database),
                    Vec::new(),
                    4_096,
                )?;
                let mut roots = Vec::new();
                let mut children = Vec::new();
                for segment in &result.segments {
                    if segment.name == root_segment {
                        roots.push(segment.data.clone());
                    } else if segment.name == child_segment {
                        let mut record = segment
                            .parent_key
                            .clone()
                            .ok_or(HostProblem::ProviderFailure)?;
                        record.extend_from_slice(&segment.data);
                        children.push(record);
                    } else {
                        return Err(HostProblem::ProviderFailure);
                    }
                }
                let dd_outputs = if let Some(combined) = combined_output_dd {
                    BTreeMap::from([(combined, roots.into_iter().chain(children).collect())])
                } else {
                    BTreeMap::from([
                        (root_output_dd.ok_or(HostProblem::ProviderFailure)?, roots),
                        (
                            child_output_dd.ok_or(HostProblem::ProviderFailure)?,
                            children,
                        ),
                    ])
                };
                Ok(crate::ProgramOutput {
                    return_code: i32::from(result.status != "  ") * 8,
                    records: vec![
                        format!("DFSRRC00 UNLOAD SEGMENTS={}", result.segments.len()).into_bytes(),
                    ],
                    dd_outputs,
                })
            }
            BatchControllerPlan::ImsPurge {
                psb,
                root_segment,
                child_segment,
                control_dd,
                required_expiry_days,
                checkpoint_prefix,
                summary_field,
            } => {
                let control = input_dd_records(input, &control_dd)?;
                let range = control
                    .first()
                    .ok_or(HostProblem::Malformed)
                    .and_then(|record| {
                        std::str::from_utf8(record).map_err(|_| HostProblem::Malformed)
                    })?;
                let fields = range.split(',').map(str::trim).collect::<Vec<_>>();
                let expiry_days = fields.first().ok_or(HostProblem::Malformed)?;
                if fields.len() != 4
                    || expiry_days.len() != 2
                    || fields[1].len() != 5
                    || fields[2].len() != 5
                    || !fields[..3]
                        .iter()
                        .all(|field| field.bytes().all(|byte| byte.is_ascii_digit()))
                    || !matches!(fields[3], "Y" | "N")
                {
                    return Err(HostProblem::Malformed);
                }
                if *expiry_days != required_expiry_days {
                    return Err(HostProblem::Unsupported);
                }
                self.ims_dli_call(
                    invocation,
                    job,
                    step,
                    effect_sequence,
                    ImsOperation::Schedule,
                    Some(psb),
                    Vec::new(),
                    Vec::new(),
                    Vec::new(),
                    None,
                    1,
                )?;
                let mut deleted_roots = 0u64;
                let mut deleted_children = 0u64;
                loop {
                    let root = self.ims_dli_call(
                        invocation,
                        job,
                        step,
                        effect_sequence,
                        ImsOperation::GetNext,
                        None,
                        vec![root_segment.clone()],
                        Vec::new(),
                        Vec::new(),
                        None,
                        1,
                    )?;
                    if root.status == "GB" {
                        break;
                    }
                    if !root.status.trim().is_empty() {
                        return Err(HostProblem::ProviderFailure);
                    }
                    loop {
                        let child = self.ims_dli_call(
                            invocation,
                            job,
                            step,
                            effect_sequence,
                            ImsOperation::GetNextParent,
                            None,
                            vec![child_segment.clone()],
                            Vec::new(),
                            Vec::new(),
                            None,
                            1,
                        )?;
                        if child.status == "GE" {
                            break;
                        }
                        if !child.status.trim().is_empty() {
                            return Err(HostProblem::ProviderFailure);
                        }
                        let deleted = self.ims_dli_call(
                            invocation,
                            job,
                            step,
                            effect_sequence,
                            ImsOperation::Delete,
                            None,
                            vec![child_segment.clone()],
                            Vec::new(),
                            Vec::new(),
                            None,
                            1,
                        )?;
                        deleted_children =
                            deleted_children.saturating_add(deleted.affected_segments);
                    }
                    let deleted = self.ims_dli_call(
                        invocation,
                        job,
                        step,
                        effect_sequence,
                        ImsOperation::Delete,
                        None,
                        vec![root_segment.clone()],
                        Vec::new(),
                        Vec::new(),
                        None,
                        1,
                    )?;
                    deleted_roots = deleted_roots.saturating_add(deleted.affected_segments);
                }
                let checkpoint = format!(
                    "{checkpoint_prefix}{:0>3}",
                    job.id.trim_start_matches("JOB")
                );
                self.ims_dli_call(
                    invocation,
                    job,
                    step,
                    effect_sequence,
                    ImsOperation::Checkpoint,
                    None,
                    Vec::new(),
                    Vec::new(),
                    Vec::new(),
                    Some(checkpoint.clone()),
                    1,
                )?;
                Ok(crate::ProgramOutput {
                    return_code: 0,
                    records: vec![format!(
                        "DFSRRC00 {mode} PROGRAM={program} EXPIRY-DAYS={expiry_days} ROOTS={deleted_roots} CHILDREN={deleted_children} CHECKPOINT={checkpoint} {summary_field}={deleted_roots}"
                    )
                    .into_bytes()],
                    dd_outputs: BTreeMap::new(),
                })
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn ims_call(
        &self,
        invocation: &Invocation,
        job: &Job,
        step: &StepPlan,
        effect_sequence: &mut u64,
        operation: ImsOperation,
        psb: Option<String>,
        data: Vec<u8>,
        max_segments: u32,
    ) -> Result<mainframe_env_host_api::ImsResult, HostProblem> {
        self.ims_dli_call(
            invocation,
            job,
            step,
            effect_sequence,
            operation,
            psb,
            Vec::new(),
            data,
            Vec::new(),
            None,
            max_segments,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn ims_dli_call(
        &self,
        invocation: &Invocation,
        job: &Job,
        step: &StepPlan,
        effect_sequence: &mut u64,
        operation: ImsOperation,
        psb: Option<String>,
        segments: Vec<String>,
        data: Vec<u8>,
        qualifiers: Vec<ImsQualifier>,
        checkpoint_id: Option<String>,
        max_segments: u32,
    ) -> Result<mainframe_env_host_api::ImsResult, HostProblem> {
        let sequence = next_effect_sequence(invocation, effect_sequence)?;
        let mutation = if operation.is_mutating() {
            let key = effect_key(job, step, sequence)?;
            Some(Mutation {
                sequence,
                idempotency_key: key,
                transaction: Some(job.id.clone()),
            })
        } else {
            None
        };
        let result = self.host.invoke(
            invocation,
            invocation.deadline_tick.saturating_sub(1),
            false,
            EffectRequest {
                run_unit: invocation.run_unit_id.clone(),
                sequence,
                deadline_tick: invocation.deadline_tick,
                idempotency_key: mutation
                    .as_ref()
                    .map(|mutation| mutation.idempotency_key.clone()),
                request: HostRequest::Ims(ImsRequest {
                    operation,
                    psb,
                    pcb: 1,
                    segments,
                    data,
                    qualifiers,
                    checkpoint_id,
                    max_segments,
                    mutation,
                }),
            },
        );
        match result.effect.outcome? {
            HostResult::Ims(result) => Ok(result),
            _ => Err(HostProblem::ProviderFailure),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn db2_call(
        &self,
        invocation: &Invocation,
        job: &Job,
        step: &StepPlan,
        effect_sequence: &mut u64,
        operation: Db2Operation,
        statement: String,
        max_rows: u32,
    ) -> Result<mainframe_env_host_api::Db2Result, HostProblem> {
        let sequence = next_effect_sequence(invocation, effect_sequence)?;
        let mutation = if operation.is_mutating() {
            let key = effect_key(job, step, sequence)?;
            Some(Mutation {
                sequence,
                idempotency_key: key,
                transaction: Some(job.id.clone()),
            })
        } else {
            None
        };
        let result = self.host.invoke(
            invocation,
            invocation.deadline_tick.saturating_sub(1),
            false,
            EffectRequest {
                run_unit: invocation.run_unit_id.clone(),
                sequence,
                deadline_tick: invocation.deadline_tick,
                idempotency_key: mutation
                    .as_ref()
                    .map(|mutation| mutation.idempotency_key.clone()),
                request: HostRequest::Db2(Db2Request {
                    operation,
                    statement,
                    cursor: None,
                    inputs: BTreeMap::new(),
                    outputs: Vec::new(),
                    max_rows,
                    mutation,
                }),
            },
        );
        match result.effect.outcome? {
            HostResult::Db2(result) => Ok(result),
            _ => Err(HostProblem::ProviderFailure),
        }
    }

    fn execute_idcams(
        &self,
        invocation: &Invocation,
        job: &mut Job,
        step: &StepPlan,
        dataset_resolutions: &BTreeMap<String, String>,
        input: &ProgramInput,
        effect_sequence: &mut u64,
    ) -> Result<i32, HostProblem> {
        let control = input_dd_text(input, "SYSIN")?;
        crate::ams::validate_idcams_control(control.as_bytes())?;
        let statements = crate::ams::parse_idcams_control(control.as_bytes())?;
        let mut max_cc = 0u8;
        let mut last_cc = 0u8;
        for statement in &statements {
            self.execute_ams_statement(
                invocation,
                job,
                step,
                dataset_resolutions,
                input,
                effect_sequence,
                statement,
                &mut max_cc,
                &mut last_cc,
            )?;
        }
        Ok(i32::from(max_cc))
    }

    #[allow(clippy::too_many_arguments)]
    fn execute_ams_statement(
        &self,
        invocation: &Invocation,
        job: &mut Job,
        step: &StepPlan,
        dataset_resolutions: &BTreeMap<String, String>,
        input: &ProgramInput,
        effect_sequence: &mut u64,
        statement: &AmsStatement,
        max_cc: &mut u8,
        last_cc: &mut u8,
    ) -> Result<(), HostProblem> {
        match statement {
            AmsStatement::Set { register, value } => match register {
                AmsRegister::MaxCc => *max_cc = *value,
                AmsRegister::LastCc => *last_cc = *value,
            },
            AmsStatement::If {
                register,
                comparison,
                value,
                action,
            } => {
                let current = match register {
                    AmsRegister::MaxCc => *max_cc,
                    AmsRegister::LastCc => *last_cc,
                };
                if compare(current, *comparison, *value) {
                    self.execute_ams_statement(
                        invocation,
                        job,
                        step,
                        dataset_resolutions,
                        input,
                        effect_sequence,
                        action,
                        max_cc,
                        last_cc,
                    )?;
                }
            }
            AmsStatement::Command(command) => {
                *last_cc = match self.execute_ams_command(
                    invocation,
                    job,
                    step,
                    dataset_resolutions,
                    input,
                    effect_sequence,
                    command,
                ) {
                    Ok(()) => 0,
                    Err(problem) => {
                        let code = ams_condition_code(&problem);
                        self.append_spool_records(
                            invocation,
                            job,
                            Some(&step.name),
                            step.dds
                                .iter()
                                .find(|dd| dd.name.eq_ignore_ascii_case("SYSPRINT")),
                            "SYSPRINT",
                            vec![
                                format!("IDCAMS {} CC={code:02} {problem:?}", command.label())
                                    .into_bytes(),
                            ],
                        )?;
                        code
                    }
                };
                *max_cc = (*max_cc).max(*last_cc);
            }
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn execute_ams_command(
        &self,
        invocation: &Invocation,
        job: &mut Job,
        step: &StepPlan,
        dataset_resolutions: &BTreeMap<String, String>,
        input: &ProgramInput,
        effect_sequence: &mut u64,
        command: &AmsCommand,
    ) -> Result<(), HostProblem> {
        self.authorize_ams_command(invocation, command, effect_sequence)?;
        if let Some(capability) = command.capability() {
            return Err(HostProblem::UnsupportedCapability {
                capability: capability.into(),
                detail: format!(
                    "{} requires provider capability {capability}",
                    command.label()
                ),
            });
        }
        if let Some((capability, operand)) = unimplemented_ams_operand(command) {
            return Err(HostProblem::UnsupportedCapability {
                capability: capability.into(),
                detail: format!("{operand} is not implemented by the AMS adapter"),
            });
        }
        match command.id() {
            "define-alias"
            | "define-alternateindex"
            | "define-cluster"
            | "define-generationdatagroup"
            | "define-nonvsam"
            | "define-path"
            | "define-usercatalog" => {
                self.define_idcams(invocation, job, step, command.source(), effect_sequence)
            }
            "repro" => self.repro_idcams(
                invocation,
                job,
                step,
                dataset_resolutions,
                input,
                effect_sequence,
                command.source(),
            ),
            "delete" => {
                self.delete_idcams(invocation, job, step, effect_sequence, command.source())
            }
            "listcat" => {
                self.listcat_idcams(invocation, job, step, effect_sequence, command.source())
            }
            _ => self.execute_ams_typed_command(
                invocation,
                job,
                step,
                dataset_resolutions,
                input,
                effect_sequence,
                command,
            ),
        }
    }

    fn authorize_ams_command(
        &self,
        invocation: &Invocation,
        command: &AmsCommand,
        effect_sequence: &mut u64,
    ) -> Result<(), HostProblem> {
        let source = command.source();
        let mut resources = Vec::<(String, AccessIntent)>::new();
        let mut push = |value: Option<String>, intent| {
            if let Some(value) = value {
                resources.push((value, intent));
            }
        };
        match command.id() {
            "listcat" => push(
                operand(source, &["ENTRIES", "ENTRY", "LEVEL"]).or_else(|| Some("**".into())),
                AccessIntent::Read,
            ),
            "dcollect" => push(Some("**".into()), AccessIntent::Read),
            "define-alias" => {
                push(operand(source, &["NAME"]), AccessIntent::Update);
                push(operand(source, &["RELATE"]), AccessIntent::Read);
            }
            "define-alternateindex" | "bldindex" => {
                push(
                    operand(source, &["RELATE", "INDATASET"]),
                    AccessIntent::Read,
                );
                push(
                    operand(source, &["NAME", "OUTDATASET"]),
                    AccessIntent::Update,
                );
            }
            "define-path" => {
                push(operand(source, &["PATHENTRY"]), AccessIntent::Read);
                push(operand(source, &["NAME"]), AccessIntent::Update);
            }
            "repro" => {
                push(operand(source, &["INDATASET"]), AccessIntent::Read);
                push(operand(source, &["OUTDATASET"]), AccessIntent::Update);
            }
            "export" | "export-disconnect" => push(
                operand(source, &["ENTRIES", "INDATASET"]),
                if command.id() == "export-disconnect" {
                    AccessIntent::Update
                } else {
                    AccessIntent::Control
                },
            ),
            "diagnose" | "examine" | "listdata" => push(
                operand(source, &["INDATASET", "DATASET", "ENTRIES"])
                    .or_else(|| crate::ams::bare_target(source, command.label())),
                AccessIntent::Read,
            ),
            "shcds" => push(operand(source, &["DATASET"]), AccessIntent::Read),
            "print" => push(operand(source, &["INDATASET"]), AccessIntent::Read),
            "verify" | "recover" | "alter" | "delete" => push(
                operand(source, &["INDATASET", "DATASET"])
                    .or_else(|| crate::ams::bare_target(source, command.label())),
                AccessIntent::Update,
            ),
            "allocate"
            | "define-cluster"
            | "define-generationdatagroup"
            | "define-nonvsam"
            | "define-usercatalog"
            | "define-pagespace" => {
                push(operand(source, &["DATASET", "NAME"]), AccessIntent::Update)
            }
            _ => {}
        }
        resources.sort_by(|left, right| left.0.cmp(&right.0));
        resources.dedup();
        for (resource, intent) in resources {
            self.authorize(
                invocation,
                "DATASET",
                &resource,
                intent,
                next_effect_sequence(invocation, effect_sequence)?,
            )?;
        }
        Ok(())
    }

    fn ams_dataset_read(
        &self,
        invocation: &Invocation,
        effect_sequence: &mut u64,
        request: DatasetRequest,
    ) -> Result<DatasetResult, HostProblem> {
        let sequence = next_effect_sequence(invocation, effect_sequence)?;
        match self
            .host
            .invoke(
                invocation,
                invocation.deadline_tick.saturating_sub(1),
                false,
                EffectRequest {
                    run_unit: invocation.run_unit_id.clone(),
                    sequence,
                    deadline_tick: invocation.deadline_tick,
                    idempotency_key: None,
                    request: HostRequest::Dataset(request),
                },
            )
            .effect
            .outcome?
        {
            HostResult::Dataset(result) => Ok(result),
            _ => Err(HostProblem::ProviderFailure),
        }
    }

    fn ams_dataset_mutation(
        &self,
        invocation: &Invocation,
        job: &Job,
        step: &StepPlan,
        effect_sequence: &mut u64,
        build: impl FnOnce(Mutation) -> DatasetRequest,
    ) -> Result<DatasetResult, HostProblem> {
        let sequence = next_effect_sequence(invocation, effect_sequence)?;
        let key = effect_key(job, step, sequence)?;
        let request = build(Mutation {
            sequence,
            idempotency_key: key.clone(),
            transaction: Some(job.id.clone()),
        });
        match self
            .host
            .invoke(
                invocation,
                invocation.deadline_tick.saturating_sub(1),
                false,
                EffectRequest {
                    run_unit: invocation.run_unit_id.clone(),
                    sequence,
                    deadline_tick: invocation.deadline_tick,
                    idempotency_key: Some(key),
                    request: HostRequest::Dataset(request),
                },
            )
            .effect
            .outcome?
        {
            HostResult::Dataset(result) => Ok(result),
            _ => Err(HostProblem::ProviderFailure),
        }
    }

    fn delete_idcams(
        &self,
        invocation: &Invocation,
        job: &Job,
        step: &StepPlan,
        effect_sequence: &mut u64,
        statement: &str,
    ) -> Result<(), HostProblem> {
        let name = crate::ams::bare_target(statement, "DELETE").ok_or(HostProblem::Malformed)?;
        let dataset = dataset_name(&name)?;
        let purge = statement.contains(" PURGE");
        let current_date = operand(statement, &["CURRENTDATE"])
            .map(|value| value.parse().map_err(|_| HostProblem::Malformed))
            .transpose()?;
        self.ams_dataset_mutation(invocation, job, step, effect_sequence, |mutation| {
            DatasetRequest::Delete {
                dataset,
                member: None,
                expected_version: None,
                purge,
                current_date,
                mutation,
            }
        })?;
        Ok(())
    }

    fn listcat_idcams(
        &self,
        invocation: &Invocation,
        job: &mut Job,
        step: &StepPlan,
        effect_sequence: &mut u64,
        statement: &str,
    ) -> Result<(), HostProblem> {
        let pattern =
            operand(statement, &["ENTRIES", "ENTRY", "LEVEL"]).unwrap_or_else(|| "**".into());
        let DatasetResult::CatalogEntries { entries, more } = self.ams_dataset_read(
            invocation,
            effect_sequence,
            DatasetRequest::ListCatalog {
                pattern,
                start: None,
                max_items: 4_096,
            },
        )?
        else {
            return Err(HostProblem::ProviderFailure);
        };
        let mut records = entries
            .into_iter()
            .map(|entry| {
                format!(
                    "{} {:?} VERSION={} RELATE={}",
                    entry.name.as_str(),
                    entry.kind,
                    entry.version,
                    entry.related.as_ref().map_or("-", DatasetName::as_str)
                )
                .into_bytes()
            })
            .collect::<Vec<_>>();
        if more {
            records.push(b"IDCAMS LISTCAT MORE".to_vec());
        }
        self.append_spool_records(
            invocation,
            job,
            Some(&step.name),
            step.dds
                .iter()
                .find(|dd| dd.name.eq_ignore_ascii_case("SYSPRINT")),
            "SYSPRINT",
            records,
        )?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn repro_idcams(
        &self,
        invocation: &Invocation,
        job: &mut Job,
        step: &StepPlan,
        dataset_resolutions: &BTreeMap<String, String>,
        input: &ProgramInput,
        effect_sequence: &mut u64,
        statement: &str,
    ) -> Result<(), HostProblem> {
        let records = if let Some(input_name) = operand(statement, &["INFILE", "IFILE"]) {
            input_dd_records(input, &input_name)?
        } else {
            let dataset =
                dataset_name(&operand(statement, &["INDATASET"]).ok_or(HostProblem::Malformed)?)?;
            let DatasetResult::Records { records, .. } = self.ams_dataset_read(
                invocation,
                effect_sequence,
                DatasetRequest::Read {
                    dataset,
                    member: None,
                    key: None,
                    max_records: 4_096,
                    control: Default::default(),
                },
            )?
            else {
                return Err(HostProblem::ProviderFailure);
            };
            records
        };
        let skip = operand(statement, &["SKIP"])
            .map(|value| value.parse::<usize>().map_err(|_| HostProblem::Malformed))
            .transpose()?
            .unwrap_or(0);
        let count = operand(statement, &["COUNT"])
            .map(|value| value.parse::<usize>().map_err(|_| HostProblem::Malformed))
            .transpose()?
            .unwrap_or(4_096);
        if skip > 4_096 || count > 4_096 {
            return Err(HostProblem::ResourceExhausted);
        }
        let records = records
            .into_iter()
            .skip(skip)
            .take(count)
            .collect::<Vec<_>>();
        if let Some(output_name) = operand(statement, &["OUTFILE", "OFILE"]) {
            self.write_dd_outputs(
                invocation,
                job,
                step,
                dataset_resolutions,
                &[],
                &BTreeMap::from([(output_name, records)]),
                effect_sequence,
            )?;
        } else {
            let dataset =
                dataset_name(&operand(statement, &["OUTDATASET"]).ok_or(HostProblem::Malformed)?)?;
            self.ams_dataset_mutation(invocation, job, step, effect_sequence, |mutation| {
                DatasetRequest::Write {
                    dataset,
                    member: None,
                    records,
                    expected_version: None,
                    mutation,
                }
            })?;
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn execute_ams_typed_command(
        &self,
        invocation: &Invocation,
        job: &mut Job,
        step: &StepPlan,
        dataset_resolutions: &BTreeMap<String, String>,
        input: &ProgramInput,
        effect_sequence: &mut u64,
        command: &AmsCommand,
    ) -> Result<(), HostProblem> {
        let statement = command.source();
        match command.id() {
            "allocate" => {
                let name =
                    operand(statement, &["DATASET", "NAME"]).ok_or(HostProblem::Malformed)?;
                let dataset = dataset_name(&name)?;
                let (_, maximum) = pair_operand(statement, "RECORDSIZE").unwrap_or((80, 80));
                let mut definition =
                    mainframe_env_host_api::DatasetDefinition::compatibility(DatasetAttributes {
                        organization: DatasetOrganization::Sequential,
                        record_format: RecordFormat::Fixed,
                        logical_record_length: maximum,
                        key_offset: None,
                        key_length: None,
                        ccsid: Some(37),
                    });
                apply_ams_definition_operands(statement, &mut definition)?;
                self.ams_dataset_mutation(invocation, job, step, effect_sequence, |mutation| {
                    DatasetRequest::Define {
                        dataset,
                        definition: Box::new(definition),
                        mutation,
                    }
                })?;
            }
            "alter" => self.alter_idcams(invocation, job, step, effect_sequence, statement)?,
            "bldindex" => {
                let base = dataset_name(
                    &operand(statement, &["INDATASET"]).ok_or(HostProblem::Malformed)?,
                )?;
                let index = dataset_name(
                    &operand(statement, &["OUTDATASET"]).ok_or(HostProblem::Malformed)?,
                )?;
                self.ams_dataset_mutation(invocation, job, step, effect_sequence, |mutation| {
                    DatasetRequest::BuildAlternateIndex {
                        base,
                        index,
                        mutation,
                    }
                })?;
            }
            "dcollect" => {
                let DatasetResult::Listed { names, .. } = self.ams_dataset_read(
                    invocation,
                    effect_sequence,
                    DatasetRequest::List {
                        pattern: "**".into(),
                        start: None,
                        max_items: 256,
                    },
                )?
                else {
                    return Err(HostProblem::ProviderFailure);
                };
                let dd = operand(statement, &["OUTFILE", "OFILE"]).ok_or(HostProblem::Malformed)?;
                let mut collected = Vec::with_capacity(names.len());
                for name in names {
                    let record = match self.ams_dataset_read(
                        invocation,
                        effect_sequence,
                        DatasetRequest::Describe {
                            dataset: name.clone(),
                        },
                    ) {
                        Ok(DatasetResult::Description(description)) => {
                            let extents = description
                                .extents
                                .iter()
                                .map(|extent| {
                                    format!(
                                        "{}:{}:{}:{}:{}",
                                        extent.ordinal,
                                        extent.start,
                                        extent.volume_start,
                                        extent.length,
                                        extent.volume_id
                                    )
                                })
                                .collect::<Vec<_>>()
                                .join(",");
                            format!(
                                "{}|ORG={:?}|VERSION={}|USED={}|ALLOCATED={}|VOLUMES={}|EXTENTS={extents}|LIFECYCLE={:?}|BACKUP={}",
                                name.as_str(),
                                description.definition.attributes.organization,
                                description.version,
                                description.used_bytes,
                                description.allocated_bytes,
                                description.definition.volumes.volume_ids.join(","),
                                description.definition.lifecycle.state,
                                description.definition.lifecycle.backup_generation,
                            )
                            .into_bytes()
                        }
                        Err(HostProblem::NotFound) | Err(HostProblem::Unsupported) => {
                            format!("{}|TYPE=CATALOG|VERSION=0|USED=0", name.as_str()).into_bytes()
                        }
                        Ok(_) => return Err(HostProblem::ProviderFailure),
                        Err(problem) => return Err(problem),
                    };
                    collected.push(record);
                }
                let DatasetResult::Volumes { volumes, more } = self.ams_dataset_read(
                    invocation,
                    effect_sequence,
                    DatasetRequest::ListVolumes {
                        start: None,
                        max_items: 4_096,
                    },
                )?
                else {
                    return Err(HostProblem::ProviderFailure);
                };
                for volume in volumes {
                    collected.push(
                        format!(
                            "VOLUME|{}|ALLOCATED={}|USED={}|EXTENTS={}",
                            volume.volume_id,
                            volume.allocated_bytes,
                            volume.used_bytes,
                            volume.extents.len(),
                        )
                        .into_bytes(),
                    );
                }
                if more {
                    collected.push(b"VOLUME|MORE".to_vec());
                }
                self.write_dd_outputs(
                    invocation,
                    job,
                    step,
                    dataset_resolutions,
                    &[],
                    &BTreeMap::from([(dd, collected)]),
                    effect_sequence,
                )?;
            }
            "diagnose" | "examine" => {
                let target = ams_target(command)?;
                if command.id() == "examine" {
                    let DatasetResult::Attributes { attributes, .. } = self.ams_dataset_read(
                        invocation,
                        effect_sequence,
                        DatasetRequest::Attributes {
                            dataset: target.clone(),
                        },
                    )?
                    else {
                        return Err(HostProblem::ProviderFailure);
                    };
                    if attributes.organization != DatasetOrganization::KeySequenced {
                        return Err(HostProblem::Unsupported);
                    }
                }
                let result = self.ams_dataset_read(
                    invocation,
                    effect_sequence,
                    DatasetRequest::Diagnose { dataset: target },
                )?;
                self.append_spool_records(
                    invocation,
                    job,
                    Some(&step.name),
                    step.dds
                        .iter()
                        .find(|dd| dd.name.eq_ignore_ascii_case("SYSPRINT")),
                    "SYSPRINT",
                    vec![format!("{} {result:?}", command.label()).into_bytes()],
                )?;
            }
            "export" | "export-disconnect" => self.export_idcams(
                invocation,
                job,
                step,
                dataset_resolutions,
                effect_sequence,
                statement,
                command.id() == "export-disconnect",
            )?,
            "import" | "import-connect" => {
                self.import_idcams(invocation, job, step, input, effect_sequence, statement)?
            }
            "listdata" => {
                let result = self.ams_dataset_read(
                    invocation,
                    effect_sequence,
                    DatasetRequest::Describe {
                        dataset: ams_target(command)?,
                    },
                )?;
                self.append_spool_records(
                    invocation,
                    job,
                    Some(&step.name),
                    step.dds
                        .iter()
                        .find(|dd| dd.name.eq_ignore_ascii_case("SYSPRINT")),
                    "SYSPRINT",
                    vec![format!("LISTDATA {result:?}").into_bytes()],
                )?;
            }
            "print" => {
                let records = if let Some(input_name) = operand(statement, &["INFILE", "IFILE"]) {
                    input_dd_records(input, &input_name)?
                } else {
                    let dataset = dataset_name(
                        &operand(statement, &["INDATASET"]).ok_or(HostProblem::Malformed)?,
                    )?;
                    let DatasetResult::Records { records, .. } = self.ams_dataset_read(
                        invocation,
                        effect_sequence,
                        DatasetRequest::Read {
                            dataset,
                            member: None,
                            key: None,
                            max_records: 4_096,
                            control: Default::default(),
                        },
                    )?
                    else {
                        return Err(HostProblem::ProviderFailure);
                    };
                    records
                };
                let skip = operand(statement, &["SKIP"])
                    .map(|value| value.parse::<usize>().map_err(|_| HostProblem::Malformed))
                    .transpose()?
                    .unwrap_or(0);
                let count = operand(statement, &["COUNT"])
                    .map(|value| value.parse::<usize>().map_err(|_| HostProblem::Malformed))
                    .transpose()?
                    .unwrap_or(4_096);
                if skip > 4_096 || count > 4_096 {
                    return Err(HostProblem::ResourceExhausted);
                }
                let records = records
                    .into_iter()
                    .skip(skip)
                    .take(count)
                    .map(|record| {
                        if statement.contains(" HEX") {
                            hex_bytes(&record).into_bytes()
                        } else {
                            record
                        }
                    })
                    .collect();
                self.append_spool_records(
                    invocation,
                    job,
                    Some(&step.name),
                    step.dds
                        .iter()
                        .find(|dd| dd.name.eq_ignore_ascii_case("SYSPRINT")),
                    "SYSPRINT",
                    records,
                )?;
            }
            "recover" => {
                self.import_idcams(invocation, job, step, input, effect_sequence, statement)?
            }
            "shcds" => {
                let result = if let Some(dataset) = operand(statement, &["DATASET"]) {
                    let now_tick = effect_sequence
                        .checked_add(1)
                        .ok_or(HostProblem::ResourceExhausted)?;
                    self.ams_dataset_read(
                        invocation,
                        effect_sequence,
                        DatasetRequest::ListLocks {
                            dataset: dataset_name(&dataset)?,
                            now_tick,
                            max_items: 4_096,
                        },
                    )?
                } else {
                    let transaction =
                        operand(statement, &["TRANSACTION"]).ok_or(HostProblem::Malformed)?;
                    if statement.contains(" COMMIT") || statement.contains(" ROLLBACK") {
                        self.ams_dataset_mutation(
                            invocation,
                            job,
                            step,
                            effect_sequence,
                            |mutation| DatasetRequest::ReconcileTvs {
                                transaction,
                                owner: invocation.principal.id().clone(),
                                committed: statement.contains(" COMMIT"),
                                mutation,
                            },
                        )?
                    } else {
                        self.ams_dataset_read(
                            invocation,
                            effect_sequence,
                            DatasetRequest::TvsStatus {
                                transaction,
                                owner: invocation.principal.id().clone(),
                            },
                        )?
                    }
                };
                self.append_spool_records(
                    invocation,
                    job,
                    Some(&step.name),
                    step.dds
                        .iter()
                        .find(|dd| dd.name.eq_ignore_ascii_case("SYSPRINT")),
                    "SYSPRINT",
                    vec![format!("SHCDS {result:?}").into_bytes()],
                )?;
            }
            "verify" => {
                self.verify_idcams(invocation, job, step, effect_sequence, ams_target(command)?)?
            }
            _ => return Err(HostProblem::Unsupported),
        }
        Ok(())
    }

    fn alter_idcams(
        &self,
        invocation: &Invocation,
        job: &Job,
        step: &StepPlan,
        effect_sequence: &mut u64,
        statement: &str,
    ) -> Result<(), HostProblem> {
        let from = dataset_name(
            &crate::ams::bare_target(statement, "ALTER").ok_or(HostProblem::Malformed)?,
        )?;
        if let Some(new_name) = operand(statement, &["NEWNAME"]) {
            let to = dataset_name(&new_name)?;
            let normalized = statement
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .replace("NEWNAME (", "NEWNAME(");
            if normalized != format!("ALTER {} NEWNAME({})", from.as_str(), to.as_str()) {
                return Err(HostProblem::UnsupportedCapability {
                    capability: "ams-combined-alter".into(),
                    detail: "rename ALTER cannot be combined with definition operands".into(),
                });
            }
            self.ams_dataset_mutation(invocation, job, step, effect_sequence, |mutation| {
                DatasetRequest::Rename { from, to, mutation }
            })?;
            return Ok(());
        }
        let next = if statement.contains(" RECOVERYREQUIRED") {
            Some((
                "RECOVERYREQUIRED",
                mainframe_env_host_api::DatasetLifecycleState::RecoveryRequired,
            ))
        } else if statement.contains(" MIGRATE") || statement.contains(" MIGRATED") {
            Some((
                if statement.contains(" MIGRATED") {
                    "MIGRATED"
                } else {
                    "MIGRATE"
                },
                mainframe_env_host_api::DatasetLifecycleState::Migrated,
            ))
        } else if statement.contains(" RECALL") {
            Some((
                if statement.contains(" RECALLPENDING") {
                    "RECALLPENDING"
                } else {
                    "RECALL"
                },
                mainframe_env_host_api::DatasetLifecycleState::RecallPending,
            ))
        } else if statement.contains(" CLOSED") {
            Some((
                "CLOSED",
                mainframe_env_host_api::DatasetLifecycleState::Closed,
            ))
        } else if statement.contains(" OPEN") {
            Some(("OPEN", mainframe_env_host_api::DatasetLifecycleState::Open))
        } else {
            None
        };
        if let Some((keyword, next)) = next {
            let normalized = statement.split_whitespace().collect::<Vec<_>>().join(" ");
            if normalized != format!("ALTER {} {keyword}", from.as_str()) {
                return Err(HostProblem::UnsupportedCapability {
                    capability: "ams-combined-alter".into(),
                    detail: "lifecycle ALTER cannot be combined with definition operands".into(),
                });
            }
            self.ams_dataset_mutation(invocation, job, step, effect_sequence, |mutation| {
                DatasetRequest::SetLifecycle {
                    dataset: from,
                    state: next,
                    expected_version: None,
                    mutation,
                }
            })?;
        } else {
            let DatasetResult::Description(description) = self.ams_dataset_read(
                invocation,
                effect_sequence,
                DatasetRequest::Describe {
                    dataset: from.clone(),
                },
            )?
            else {
                return Err(HostProblem::ProviderFailure);
            };
            let mut definition = description.definition;
            apply_ams_definition_operands(statement, &mut definition)?;
            self.ams_dataset_mutation(invocation, job, step, effect_sequence, |mutation| {
                DatasetRequest::Alter {
                    dataset: from,
                    definition: Box::new(definition),
                    expected_version: Some(description.version),
                    mutation,
                }
            })?;
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn export_idcams(
        &self,
        invocation: &Invocation,
        job: &mut Job,
        step: &StepPlan,
        dataset_resolutions: &BTreeMap<String, String>,
        effect_sequence: &mut u64,
        statement: &str,
        disconnect: bool,
    ) -> Result<(), HostProblem> {
        let target = dataset_name(
            &operand(statement, &["ENTRIES", "INDATASET"]).ok_or(HostProblem::Malformed)?,
        )?;
        let dd = operand(statement, &["OUTFILE", "OFILE"]).ok_or(HostProblem::Malformed)?;
        let mut backup_version = None;
        let records = if disconnect {
            let core = format!("MEAMSCAT1|{}", target.as_str());
            let header = format!("{core}|{}", ams_snapshot_digest(&core, &[])).into_bytes();
            vec![header]
        } else {
            let DatasetResult::Snapshot { snapshot, version } = self.ams_dataset_read(
                invocation,
                effect_sequence,
                DatasetRequest::Snapshot {
                    dataset: target.clone(),
                    max_records: 4_096,
                    max_members: 4_096,
                },
            )?
            else {
                return Err(HostProblem::ProviderFailure);
            };
            backup_version = Some(version);
            encode_ams_snapshot(&target, &snapshot)?
        };
        self.write_dd_outputs(
            invocation,
            job,
            step,
            dataset_resolutions,
            &[],
            &BTreeMap::from([(dd, records)]),
            effect_sequence,
        )?;
        if disconnect {
            self.ams_dataset_mutation(invocation, job, step, effect_sequence, |mutation| {
                DatasetRequest::SetCatalogConnection {
                    catalog: target,
                    connected: false,
                    expected_version: None,
                    mutation,
                }
            })?;
        } else if let Some(version) = backup_version {
            self.ams_dataset_mutation(invocation, job, step, effect_sequence, |mutation| {
                DatasetRequest::RecordBackup {
                    dataset: target,
                    expected_version: Some(version),
                    mutation,
                }
            })?;
        }
        Ok(())
    }

    fn import_idcams(
        &self,
        invocation: &Invocation,
        job: &Job,
        step: &StepPlan,
        input: &ProgramInput,
        effect_sequence: &mut u64,
        statement: &str,
    ) -> Result<(), HostProblem> {
        let dd = operand(statement, &["INFILE", "IFILE"]).ok_or(HostProblem::Malformed)?;
        let mut records = input_dd_records(input, &dd)?;
        if records.is_empty() {
            return Err(HostProblem::Malformed);
        }
        let header = String::from_utf8(records.remove(0)).map_err(|_| HostProblem::Malformed)?;
        if let Some(rest) = header.strip_prefix("MEAMSCAT1|") {
            let (catalog, digest) = rest.rsplit_once('|').ok_or(HostProblem::Malformed)?;
            let core = format!("MEAMSCAT1|{catalog}");
            if digest != ams_snapshot_digest(&core, &[]) {
                return Err(HostProblem::IdempotencyConflict);
            }
            let catalog = dataset_name(catalog)?;
            self.authorize(
                invocation,
                "DATASET",
                catalog.as_str(),
                AccessIntent::Update,
                next_effect_sequence(invocation, effect_sequence)?,
            )?;
            self.ams_dataset_mutation(invocation, job, step, effect_sequence, |mutation| {
                DatasetRequest::SetCatalogConnection {
                    catalog,
                    connected: true,
                    expected_version: None,
                    mutation,
                }
            })?;
            return Ok(());
        }
        let (snapshot_name, mut snapshot) = decode_ams_snapshot(&header, &mut records)?;
        let target = operand(statement, &["OUTDATASET", "INDATASET", "DATASET"])
            .map_or(Ok(snapshot_name), |name| dataset_name(&name))?;
        self.authorize(
            invocation,
            "DATASET",
            target.as_str(),
            AccessIntent::Update,
            next_effect_sequence(invocation, effect_sequence)?,
        )?;
        if statement.starts_with("RECOVER") {
            snapshot.definition.lifecycle.state =
                mainframe_env_host_api::DatasetLifecycleState::Closed;
        }
        let current = self.ams_dataset_read(
            invocation,
            effect_sequence,
            DatasetRequest::Attributes {
                dataset: target.clone(),
            },
        );
        let expected_version = match current {
            Ok(DatasetResult::Attributes { version, .. }) => Some(version),
            Err(HostProblem::NotFound) => None,
            Ok(_) => return Err(HostProblem::ProviderFailure),
            Err(problem) => return Err(problem),
        };
        self.ams_dataset_mutation(invocation, job, step, effect_sequence, |mutation| {
            DatasetRequest::Restore {
                dataset: target,
                snapshot: Box::new(snapshot),
                expected_version,
                mutation,
            }
        })?;
        Ok(())
    }

    fn verify_idcams(
        &self,
        invocation: &Invocation,
        job: &Job,
        step: &StepPlan,
        effect_sequence: &mut u64,
        dataset: DatasetName,
    ) -> Result<(), HostProblem> {
        let DatasetResult::Description(description) = self.ams_dataset_read(
            invocation,
            effect_sequence,
            DatasetRequest::Describe {
                dataset: dataset.clone(),
            },
        )?
        else {
            return Err(HostProblem::ProviderFailure);
        };
        if description.definition.lifecycle.state
            == mainframe_env_host_api::DatasetLifecycleState::RecoveryRequired
        {
            self.ams_dataset_mutation(invocation, job, step, effect_sequence, |mutation| {
                DatasetRequest::SetLifecycle {
                    dataset,
                    state: mainframe_env_host_api::DatasetLifecycleState::Closed,
                    expected_version: Some(description.version),
                    mutation,
                }
            })?;
        }
        Ok(())
    }

    fn define_idcams(
        &self,
        invocation: &Invocation,
        job: &Job,
        step: &StepPlan,
        statement: &str,
        effect_sequence: &mut u64,
    ) -> Result<(), HostProblem> {
        let sequence = next_effect_sequence(invocation, effect_sequence)?;
        let key = effect_key(job, step, sequence)?;
        let request = if statement.starts_with("DEFINE ALIAS") {
            DatasetRequest::DefineAlias {
                alias: dataset_name(&operand(statement, &["NAME"]).ok_or(HostProblem::Malformed)?)?,
                target: dataset_name(
                    &operand(statement, &["RELATE"]).ok_or(HostProblem::Malformed)?,
                )?,
                mutation: Mutation {
                    sequence,
                    idempotency_key: key.clone(),
                    transaction: Some(job.id.clone()),
                },
            }
        } else if statement.starts_with("DEFINE USERCATALOG") {
            DatasetRequest::DefineCatalog {
                catalog: dataset_name(
                    &operand(statement, &["NAME"]).ok_or(HostProblem::Malformed)?,
                )?,
                kind: mainframe_env_host_api::CatalogKind::User,
                mutation: Mutation {
                    sequence,
                    idempotency_key: key.clone(),
                    transaction: Some(job.id.clone()),
                },
            }
        } else if statement.starts_with("DEFINE NONVSAM") {
            let (minimum, maximum) = pair_operand(statement, "RECORDSIZE").unwrap_or((80, 80));
            let mut definition =
                mainframe_env_host_api::DatasetDefinition::compatibility(DatasetAttributes {
                    organization: DatasetOrganization::Sequential,
                    record_format: if minimum == maximum {
                        RecordFormat::Fixed
                    } else {
                        RecordFormat::Variable
                    },
                    logical_record_length: maximum,
                    key_offset: None,
                    key_length: None,
                    ccsid: Some(37),
                });
            apply_ams_definition_operands(statement, &mut definition)?;
            DatasetRequest::Define {
                dataset: dataset_name(
                    &operand(statement, &["NAME"]).ok_or(HostProblem::Malformed)?,
                )?,
                definition: Box::new(definition),
                mutation: Mutation {
                    sequence,
                    idempotency_key: key.clone(),
                    transaction: Some(job.id.clone()),
                },
            }
        } else if statement.contains(" PATH ") || statement.starts_with("DEFINE PATH") {
            DatasetRequest::DefinePath {
                path: DatasetName::new(
                    operand(statement, &["NAME"]).ok_or(HostProblem::Malformed)?,
                    128,
                )
                .map_err(|_| HostProblem::Malformed)?,
                index: DatasetName::new(
                    operand(statement, &["PATHENTRY"]).ok_or(HostProblem::Malformed)?,
                    128,
                )
                .map_err(|_| HostProblem::Malformed)?,
                mutation: Mutation {
                    sequence,
                    idempotency_key: key.clone(),
                    transaction: Some(job.id.clone()),
                },
            }
        } else if statement.contains("GENERATIONDATAGROUP") {
            DatasetRequest::DefineGenerationGroup {
                base: DatasetName::new(
                    operand(statement, &["NAME"]).ok_or(HostProblem::Malformed)?,
                    128,
                )
                .map_err(|_| HostProblem::Malformed)?,
                limit: numeric_operand(statement, "LIMIT").unwrap_or(255),
                scratch: !statement.contains("NOSCRATCH"),
                empty: statement.contains(" EMPTY") && !statement.contains("NOEMPTY"),
                mutation: Mutation {
                    sequence,
                    idempotency_key: key.clone(),
                    transaction: Some(job.id.clone()),
                },
            }
        } else if statement.contains("ALTERNATEINDEX") {
            let (key_length, key_offset) =
                pair_operand(statement, "KEYS").ok_or(HostProblem::Malformed)?;
            DatasetRequest::DefineAlternateIndex {
                base: DatasetName::new(
                    operand(statement, &["RELATE"]).ok_or(HostProblem::Malformed)?,
                    128,
                )
                .map_err(|_| HostProblem::Malformed)?,
                index: DatasetName::new(
                    operand(statement, &["NAME"]).ok_or(HostProblem::Malformed)?,
                    128,
                )
                .map_err(|_| HostProblem::Malformed)?,
                key_offset,
                key_length,
                allow_duplicates: statement.contains("NONUNIQUEKEY"),
                upgrade: !statement.contains("NOUPGRADE"),
                mutation: Mutation {
                    sequence,
                    idempotency_key: key.clone(),
                    transaction: Some(job.id.clone()),
                },
            }
        } else if statement.starts_with("DEFINE CLUSTER") {
            let (minimum, maximum) = pair_operand(statement, "RECORDSIZE").unwrap_or((80, 80));
            let keys = pair_operand(statement, "KEYS");
            let organization = if statement.contains(" LINEAR") {
                DatasetOrganization::Linear
            } else if statement.contains("NUMBERED") && minimum != maximum {
                DatasetOrganization::VariableRelative
            } else if statement.contains("NUMBERED") {
                DatasetOrganization::Relative
            } else if statement.contains("NONINDEXED") {
                DatasetOrganization::EntrySequenced
            } else {
                DatasetOrganization::KeySequenced
            };
            let spanned = statement.contains(" SPANNED");
            let record_format = if organization == DatasetOrganization::Linear {
                RecordFormat::Undefined
            } else if spanned && statement.contains(" BLOCKED") {
                RecordFormat::VariableBlockedSpanned
            } else if spanned {
                RecordFormat::VariableSpanned
            } else if minimum == maximum {
                RecordFormat::Fixed
            } else {
                RecordFormat::Variable
            };
            let mut definition =
                mainframe_env_host_api::DatasetDefinition::compatibility(DatasetAttributes {
                    organization,
                    record_format,
                    logical_record_length: maximum,
                    key_offset: keys.map(|(_, offset)| offset),
                    key_length: keys.map(|(length, _)| length),
                    ccsid: Some(37),
                });
            definition.vsam.control_interval_size =
                operand(statement, &["CONTROLINTERVALSIZE", "CISZ"])
                    .map(|value| value.parse().map_err(|_| HostProblem::Malformed))
                    .transpose()?;
            definition.vsam.control_area_size = operand(statement, &["CONTROLAREASIZE"])
                .map(|value| value.parse().map_err(|_| HostProblem::Malformed))
                .transpose()?;
            if let Some((cross_region, cross_system)) = pair_operand(statement, "SHAREOPTIONS") {
                definition.vsam.share_options = mainframe_env_host_api::DatasetShareOptions {
                    cross_region: u8::try_from(cross_region).map_err(|_| HostProblem::Malformed)?,
                    cross_system: u8::try_from(cross_system).map_err(|_| HostProblem::Malformed)?,
                };
            }
            definition.vsam.access_mode = if statement.contains(" TVS") {
                mainframe_env_host_api::VsamAccessMode::Tvs
            } else if statement.contains(" RLS") {
                mainframe_env_host_api::VsamAccessMode::Rls
            } else {
                mainframe_env_host_api::VsamAccessMode::NonRls
            };
            definition.vsam.spanned = spanned;
            definition.vsam.reuse = statement.contains(" REUSE");
            definition.vsam.speed = statement.contains(" SPEED");
            definition.vsam.write_check = statement.contains(" WRITECHECK");
            definition.vsam.erase_on_delete = statement.contains(" ERASE");
            apply_ams_definition_operands(statement, &mut definition)?;
            DatasetRequest::Define {
                dataset: dataset_name(
                    &operand(statement, &["NAME"]).ok_or(HostProblem::Malformed)?,
                )?,
                definition: Box::new(definition),
                mutation: Mutation {
                    sequence,
                    idempotency_key: key.clone(),
                    transaction: Some(job.id.clone()),
                },
            }
        } else {
            return Err(HostProblem::Unsupported);
        };
        let result = self.host.invoke(
            invocation,
            invocation.deadline_tick.saturating_sub(1),
            false,
            EffectRequest {
                run_unit: invocation.run_unit_id.clone(),
                sequence,
                deadline_tick: invocation.deadline_tick,
                idempotency_key: Some(key),
                request: HostRequest::Dataset(request),
            },
        );
        if !matches!(result.effect.outcome?, HostResult::Dataset(_)) {
            return Err(HostProblem::ProviderFailure);
        }
        Ok(())
    }

    fn allocate_dds(
        &self,
        invocation: &Invocation,
        job: &mut Job,
        step: &StepPlan,
        plans: &[DdAllocationPlan],
        effect_sequence: &mut u64,
    ) -> Result<Vec<DdRuntimeAllocation>, HostProblem> {
        if plans.len() != step.dds.len() {
            return Err(HostProblem::InfrastructureFailure);
        }
        let lock_owner = invocation.principal.id().clone();
        let mut allocations = Vec::new();
        let mut lock_modes = BTreeMap::<String, DatasetLockMode>::new();
        let allocation_result = (|| -> Result<(), HostProblem> {
            for (dd, plan) in step.dds.iter().zip(plans) {
                if is_program_library_dd(dd) || plan.source != DdSourceKind::Dataset {
                    continue;
                }
                let Some(raw_name) = &dd.dataset else {
                    return Err(HostProblem::InfrastructureFailure);
                };
                let disposition = plan.disposition.ok_or(HostProblem::InfrastructureFailure)?;
                let name = resolved_dataset(job, dd, raw_name, &job.dataset_resolutions);
                self.authorize(
                    invocation,
                    "DATASET",
                    &name,
                    if disposition.status == DdStatusDisposition::Shared {
                        AccessIntent::Read
                    } else {
                        AccessIntent::Update
                    },
                    next_effect_sequence(invocation, effect_sequence)?,
                )?;
                if let Some(relative) = dd.generation {
                    let resolution_key = dataset_resolution_key(dd, raw_name);
                    if !job.dataset_resolutions.contains_key(&resolution_key) {
                        let sequence = next_effect_sequence(invocation, effect_sequence)?;
                        let key = effect_key(job, step, sequence)?;
                        let base_name = DatasetName::new(raw_name.to_ascii_uppercase(), 128)
                            .map_err(|_| HostProblem::Malformed)?;
                        let (request, idempotency_key) =
                            if relative == 1 && disposition.status == DdStatusDisposition::New {
                                (
                                    DatasetRequest::CreateGeneration {
                                        base: base_name,
                                        attributes: dataset_attributes_for_dd(dd)?,
                                        records: Vec::new(),
                                        mutation: Mutation {
                                            sequence,
                                            idempotency_key: key.clone(),
                                            transaction: Some(job.id.clone()),
                                        },
                                    },
                                    Some(key),
                                )
                            } else {
                                (
                                    DatasetRequest::ResolveGeneration {
                                        base: base_name,
                                        relative,
                                    },
                                    None,
                                )
                            };
                        let result = self.host.invoke(
                            invocation,
                            invocation.deadline_tick.saturating_sub(1),
                            false,
                            EffectRequest {
                                run_unit: invocation.run_unit_id.clone(),
                                sequence,
                                deadline_tick: invocation.deadline_tick,
                                idempotency_key,
                                request: HostRequest::Dataset(request),
                            },
                        );
                        let HostResult::Dataset(DatasetResult::Generation { dataset, .. }) =
                            result.effect.outcome?
                        else {
                            return Err(HostProblem::ProviderFailure);
                        };
                        job.dataset_resolutions
                            .insert(resolution_key, dataset.as_str().to_string());
                    }
                }
                let resolved = DatasetName::new(
                    resolved_dataset(job, dd, raw_name, &job.dataset_resolutions),
                    128,
                )
                .map_err(|_| HostProblem::Malformed)?;
                let mut create = disposition.status == DdStatusDisposition::New
                    && dd.generation.is_none()
                    && dd.member.is_none();
                if disposition.status == DdStatusDisposition::Modify && dd.member.is_none() {
                    create = match self.dataset_attributes(invocation, &resolved, effect_sequence) {
                        Ok(_) => false,
                        Err(HostProblem::NotFound) => true,
                        Err(problem) => return Err(problem),
                    };
                }
                if create {
                    let sequence = next_effect_sequence(invocation, effect_sequence)?;
                    let key = effect_key(job, step, sequence)?;
                    let mut definition =
                        DatasetDefinition::compatibility(dataset_attributes_for_dd(dd)?);
                    definition.lifecycle.state = DatasetLifecycleState::Allocated;
                    let result = self.host.invoke(
                        invocation,
                        invocation.deadline_tick.saturating_sub(1),
                        false,
                        EffectRequest {
                            run_unit: invocation.run_unit_id.clone(),
                            sequence,
                            deadline_tick: invocation.deadline_tick,
                            idempotency_key: Some(key.clone()),
                            request: HostRequest::Dataset(DatasetRequest::Define {
                                dataset: resolved.clone(),
                                definition: Box::new(definition),
                                mutation: Mutation {
                                    sequence,
                                    idempotency_key: key,
                                    transaction: Some(job.id.clone()),
                                },
                            }),
                        },
                    );
                    match result.effect.outcome? {
                        HostResult::Dataset(_) => {}
                        _ => return Err(HostProblem::ProviderFailure),
                    }
                }
                if dd.temporary
                    && !job
                        .temporary_datasets
                        .iter()
                        .any(|dataset| dataset == resolved.as_str())
                {
                    job.temporary_datasets.push(resolved.as_str().to_string());
                }
                let requested_mode = if disposition.status == DdStatusDisposition::Shared {
                    DatasetLockMode::Shared
                } else {
                    DatasetLockMode::Exclusive
                };
                lock_modes
                    .entry(resolved.as_str().to_string())
                    .and_modify(|mode| {
                        if requested_mode == DatasetLockMode::Exclusive {
                            *mode = DatasetLockMode::Exclusive;
                        }
                    })
                    .or_insert(requested_mode);
                allocations.push(DdRuntimeAllocation {
                    ordinal: plan.ordinal,
                    dataset: resolved,
                    member: dd
                        .member
                        .as_ref()
                        .map(|member| {
                            MemberName::new(member, 8).map_err(|_| HostProblem::Malformed)
                        })
                        .transpose()?,
                    disposition,
                    lock_id: None,
                    lock_owner: lock_owner.clone(),
                });
            }
            for (dataset, mode) in lock_modes {
                let sequence = next_effect_sequence(invocation, effect_sequence)?;
                let key = effect_key(job, step, sequence)?;
                let result = self.host.invoke(
                    invocation,
                    invocation.deadline_tick.saturating_sub(1),
                    false,
                    EffectRequest {
                        run_unit: invocation.run_unit_id.clone(),
                        sequence,
                        deadline_tick: invocation.deadline_tick,
                        idempotency_key: Some(key.clone()),
                        request: HostRequest::Dataset(DatasetRequest::AcquireLock {
                            dataset: DatasetName::new(&dataset, 128)
                                .map_err(|_| HostProblem::Malformed)?,
                            target: DatasetLockTarget::Dataset,
                            owner: lock_owner.clone(),
                            mode,
                            now_tick: sequence,
                            lease_ticks: invocation.deadline_tick.saturating_sub(sequence).max(1),
                            transaction: Some(job.id.clone()),
                            mutation: Mutation {
                                sequence,
                                idempotency_key: key,
                                transaction: Some(job.id.clone()),
                            },
                        }),
                    },
                );
                let HostResult::Dataset(DatasetResult::Locks { locks }) = result.effect.outcome?
                else {
                    return Err(HostProblem::ProviderFailure);
                };
                let lock = locks.first().ok_or(HostProblem::ProviderFailure)?;
                for allocation in &mut allocations {
                    if allocation.dataset.as_str() == dataset {
                        allocation.lock_id = Some(lock.lock_id.clone());
                    }
                }
            }
            Ok(())
        })();
        if let Err(problem) = allocation_result {
            let cleanup =
                self.dispose_dds(invocation, job, step, &allocations, effect_sequence, true);
            return Err(cleanup.err().unwrap_or(problem));
        }
        Ok(allocations)
    }

    fn dataset_attributes(
        &self,
        invocation: &Invocation,
        dataset: &DatasetName,
        effect_sequence: &mut u64,
    ) -> Result<DatasetAttributes, HostProblem> {
        let sequence = next_effect_sequence(invocation, effect_sequence)?;
        let result = self.host.invoke(
            invocation,
            invocation.deadline_tick.saturating_sub(1),
            false,
            EffectRequest {
                run_unit: invocation.run_unit_id.clone(),
                sequence,
                deadline_tick: invocation.deadline_tick,
                idempotency_key: None,
                request: HostRequest::Dataset(DatasetRequest::Attributes {
                    dataset: dataset.clone(),
                }),
            },
        );
        let HostResult::Dataset(DatasetResult::Attributes { attributes, .. }) =
            result.effect.outcome?
        else {
            return Err(HostProblem::ProviderFailure);
        };
        Ok(attributes)
    }

    fn read_dataset_records(
        &self,
        invocation: &Invocation,
        allocation: &DdRuntimeAllocation,
        effect_sequence: &mut u64,
    ) -> Result<Vec<Vec<u8>>, HostProblem> {
        let sequence = next_effect_sequence(invocation, effect_sequence)?;
        let result = self.host.invoke(
            invocation,
            invocation.deadline_tick.saturating_sub(1),
            false,
            EffectRequest {
                run_unit: invocation.run_unit_id.clone(),
                sequence,
                deadline_tick: invocation.deadline_tick,
                idempotency_key: None,
                request: HostRequest::Dataset(DatasetRequest::Read {
                    dataset: allocation.dataset.clone(),
                    member: allocation.member.clone(),
                    key: None,
                    max_records: 4_096,
                    control: Default::default(),
                }),
            },
        );
        let HostResult::Dataset(DatasetResult::Records { records, .. }) = result.effect.outcome?
        else {
            return Err(HostProblem::ProviderFailure);
        };
        Ok(records)
    }

    fn hydrate_dds(
        &self,
        invocation: &Invocation,
        job: &Job,
        allocations: &[DdRuntimeAllocation],
        dds: &mut [crate::DdPlan],
        effect_sequence: &mut u64,
    ) -> Result<(), HostProblem> {
        let _ = job;
        let mut start = 0usize;
        while start < dds.len() {
            let name = dds[start].name.clone();
            let mut end = start + 1;
            while end < dds.len()
                && dds[end].concatenation
                && dds[end].name.eq_ignore_ascii_case(&name)
            {
                end += 1;
            }
            if is_program_library_dd(&dds[start]) {
                start = end;
                continue;
            }
            let mut payload = Vec::new();
            let group_allocations = allocations
                .iter()
                .filter(|allocation| (start..end).contains(&allocation.ordinal))
                .collect::<Vec<_>>();
            let can_read_as_concatenation = group_allocations.len() > 1
                && group_allocations.iter().all(|allocation| {
                    !matches!(allocation.disposition.status, DdStatusDisposition::New)
                        && allocation.member == group_allocations[0].member
                })
                && group_allocations.len() == end - start;
            let mut ccsid = None;
            for allocation in &group_allocations {
                let attributes =
                    self.dataset_attributes(invocation, &allocation.dataset, effect_sequence)?;
                if ccsid.is_some() && ccsid != attributes.ccsid {
                    return Err(HostProblem::Unsupported);
                }
                ccsid = attributes.ccsid;
            }
            if can_read_as_concatenation {
                let sequence = next_effect_sequence(invocation, effect_sequence)?;
                let result = self.host.invoke(
                    invocation,
                    invocation.deadline_tick.saturating_sub(1),
                    false,
                    EffectRequest {
                        run_unit: invocation.run_unit_id.clone(),
                        sequence,
                        deadline_tick: invocation.deadline_tick,
                        idempotency_key: None,
                        request: HostRequest::Dataset(DatasetRequest::ReadConcatenation {
                            datasets: group_allocations
                                .iter()
                                .map(|allocation| allocation.dataset.clone())
                                .collect(),
                            member: group_allocations[0].member.clone(),
                            max_records: 4_096,
                        }),
                    },
                );
                let HostResult::Dataset(DatasetResult::Records { records, .. }) =
                    result.effect.outcome?
                else {
                    return Err(HostProblem::ProviderFailure);
                };
                append_inline_records(&mut payload, records);
            } else {
                for (ordinal, dd) in dds.iter().enumerate().take(end).skip(start) {
                    if let Some(allocation) = allocations
                        .iter()
                        .find(|allocation| allocation.ordinal == ordinal)
                    {
                        if allocation.disposition.status != DdStatusDisposition::New {
                            let records =
                                self.read_dataset_records(invocation, allocation, effect_sequence)?;
                            append_inline_records(&mut payload, records);
                        }
                    } else {
                        payload.extend_from_slice(&dd.inline_data);
                    }
                }
            }
            dds[start].inline_data = payload;
            dds[start].ccsid = ccsid.or(dds[start].ccsid);
            for dd in &mut dds[start + 1..end] {
                dd.inline_data.clear();
                dd.ccsid = ccsid.or(dd.ccsid);
            }
            start = end;
        }
        Ok(())
    }

    fn write_dd_outputs(
        &self,
        invocation: &Invocation,
        job: &mut Job,
        step: &StepPlan,
        dataset_resolutions: &BTreeMap<String, String>,
        allocations: &[DdRuntimeAllocation],
        outputs: &BTreeMap<String, Vec<Vec<u8>>>,
        effect_sequence: &mut u64,
    ) -> Result<(), HostProblem> {
        for (name, records) in outputs {
            let (ordinal, dd) = step
                .dds
                .iter()
                .enumerate()
                .find(|(_, dd)| dd.name.eq_ignore_ascii_case(name))
                .ok_or(HostProblem::NotFound)?;
            if dd
                .sysout
                .as_ref()
                .is_some_and(|sysout| sysout.to_ascii_uppercase().contains("INTRDR"))
            {
                let mut source = String::new();
                for record in records {
                    source
                        .push_str(std::str::from_utf8(record).map_err(|_| HostProblem::Malformed)?);
                    source.push('\n');
                }
                self.submit_internal_reader(
                    invocation,
                    &job.id,
                    &step.name,
                    &JclBundle {
                        primary: source,
                        ..Default::default()
                    },
                    &IdempotencyKey::new(
                        format!("jes:{}:{}:internal-reader", job.id, step.name),
                        InvocationLimits::default(),
                    )
                    .map_err(|_| HostProblem::ResourceExhausted)?,
                )?;
                self.append_spool_records(
                    invocation,
                    job,
                    Some(&step.name),
                    Some(dd),
                    &dd.name,
                    vec![b"INTERNAL READER SUBMITTED".to_vec()],
                )?;
                continue;
            }
            if dd.sysout.is_some() {
                self.append_spool_records(
                    invocation,
                    job,
                    Some(&step.name),
                    Some(dd),
                    &dd.name,
                    records.clone(),
                )?;
                continue;
            }
            let fallback;
            let allocation = if let Some(allocation) = allocations
                .iter()
                .find(|allocation| allocation.ordinal == ordinal)
            {
                allocation
            } else if let Some(raw_name) = &dd.dataset {
                let plans = plan_dd_allocations(&step.dds)?;
                fallback = DdRuntimeAllocation {
                    ordinal,
                    dataset: DatasetName::new(
                        resolved_dataset(job, dd, raw_name, dataset_resolutions),
                        128,
                    )
                    .map_err(|_| HostProblem::Malformed)?,
                    member: dd
                        .member
                        .as_ref()
                        .map(|member| {
                            MemberName::new(member, 8).map_err(|_| HostProblem::Malformed)
                        })
                        .transpose()?,
                    disposition: plans[ordinal]
                        .disposition
                        .ok_or(HostProblem::InfrastructureFailure)?,
                    lock_id: None,
                    lock_owner: dd_lock_owner(job)?,
                };
                &fallback
            } else {
                continue;
            };
            let attribute_sequence = next_effect_sequence(invocation, effect_sequence)?;
            let attributes = self.host.invoke(
                invocation,
                invocation.deadline_tick.saturating_sub(1),
                false,
                EffectRequest {
                    run_unit: invocation.run_unit_id.clone(),
                    sequence: attribute_sequence,
                    deadline_tick: invocation.deadline_tick,
                    idempotency_key: None,
                    request: HostRequest::Dataset(DatasetRequest::Attributes {
                        dataset: allocation.dataset.clone(),
                    }),
                },
            );
            let HostResult::Dataset(DatasetResult::Attributes { attributes, .. }) =
                attributes.effect.outcome?
            else {
                return Err(HostProblem::ProviderFailure);
            };
            let records = normalize_records(records, &attributes)?;
            if attributes.organization == DatasetOrganization::Relative {
                if allocation.disposition.status == DdStatusDisposition::Modify {
                    return Err(HostProblem::Unsupported);
                }
                for (position, record) in records.into_iter().enumerate() {
                    let sequence = next_effect_sequence(invocation, effect_sequence)?;
                    let key = effect_key(job, step, sequence)?;
                    let result = self.host.invoke(
                        invocation,
                        invocation.deadline_tick.saturating_sub(1),
                        false,
                        EffectRequest {
                            run_unit: invocation.run_unit_id.clone(),
                            sequence,
                            deadline_tick: invocation.deadline_tick,
                            idempotency_key: Some(key.clone()),
                            request: HostRequest::Dataset(DatasetRequest::WriteRelative {
                                dataset: allocation.dataset.clone(),
                                record_number: u64::try_from(position + 1)
                                    .map_err(|_| HostProblem::ResourceExhausted)?,
                                record,
                                expected_version: None,
                                mutation: Mutation {
                                    sequence,
                                    idempotency_key: key,
                                    transaction: allocation.lock_id.clone(),
                                },
                            }),
                        },
                    );
                    if !matches!(
                        result.effect.outcome?,
                        HostResult::Dataset(DatasetResult::Mutated { .. })
                    ) {
                        return Err(HostProblem::ProviderFailure);
                    }
                }
                continue;
            }
            let sequence = next_effect_sequence(invocation, effect_sequence)?;
            let key = effect_key(job, step, sequence)?;
            let request = if allocation.disposition.status == DdStatusDisposition::Modify {
                DatasetRequest::Append {
                    dataset: allocation.dataset.clone(),
                    member: allocation.member.clone(),
                    records,
                    expected_version: None,
                    mutation: Mutation {
                        sequence,
                        idempotency_key: key.clone(),
                        transaction: allocation.lock_id.clone(),
                    },
                }
            } else {
                DatasetRequest::Write {
                    dataset: allocation.dataset.clone(),
                    member: allocation.member.clone(),
                    records,
                    expected_version: None,
                    mutation: Mutation {
                        sequence,
                        idempotency_key: key.clone(),
                        transaction: allocation.lock_id.clone(),
                    },
                }
            };
            let result = self.host.invoke(
                invocation,
                invocation.deadline_tick.saturating_sub(1),
                false,
                EffectRequest {
                    run_unit: invocation.run_unit_id.clone(),
                    sequence,
                    deadline_tick: invocation.deadline_tick,
                    idempotency_key: Some(key.clone()),
                    request: HostRequest::Dataset(request),
                },
            );
            if !matches!(
                result.effect.outcome?,
                HostResult::Dataset(DatasetResult::Mutated { .. })
            ) {
                return Err(HostProblem::ProviderFailure);
            }
        }
        Ok(())
    }

    fn dispose_dds(
        &self,
        invocation: &Invocation,
        job: &mut Job,
        step: &StepPlan,
        allocations: &[DdRuntimeAllocation],
        effect_sequence: &mut u64,
        abnormal: bool,
    ) -> Result<(), HostProblem> {
        let mut first_problem = None;
        let mut disposed = BTreeMap::<String, ()>::new();
        for allocation in allocations {
            let terminal = if abnormal {
                allocation.disposition.abnormal
            } else {
                allocation.disposition.normal
            };
            if matches!(
                terminal,
                DdTerminalDisposition::Pass | DdTerminalDisposition::Keep
            ) {
                continue;
            }
            let disposition_key = format!(
                "{}({})",
                allocation.dataset.as_str(),
                allocation.member.as_ref().map_or("", MemberName::as_str)
            );
            if disposed.insert(disposition_key, ()).is_some() {
                continue;
            }
            let sequence = next_effect_sequence(invocation, effect_sequence)?;
            let key = effect_key(job, step, sequence)?;
            let mutation = Mutation {
                sequence,
                idempotency_key: key.clone(),
                transaction: allocation.lock_id.clone(),
            };
            let request = match terminal {
                DdTerminalDisposition::Delete => DatasetRequest::Delete {
                    dataset: allocation.dataset.clone(),
                    member: allocation.member.clone(),
                    expected_version: None,
                    purge: true,
                    current_date: None,
                    mutation,
                },
                DdTerminalDisposition::Catalog => DatasetRequest::SetLifecycle {
                    dataset: allocation.dataset.clone(),
                    state: DatasetLifecycleState::Cataloged,
                    expected_version: None,
                    mutation,
                },
                DdTerminalDisposition::Uncatalog => DatasetRequest::SetLifecycle {
                    dataset: allocation.dataset.clone(),
                    state: DatasetLifecycleState::Allocated,
                    expected_version: None,
                    mutation,
                },
                DdTerminalDisposition::Pass | DdTerminalDisposition::Keep => unreachable!(),
            };
            let outcome = self.host.invoke(
                invocation,
                invocation.deadline_tick.saturating_sub(1),
                false,
                EffectRequest {
                    run_unit: invocation.run_unit_id.clone(),
                    sequence,
                    deadline_tick: invocation.deadline_tick,
                    idempotency_key: Some(key),
                    request: HostRequest::Dataset(request),
                },
            );
            if let Err(problem) = outcome.effect.outcome {
                first_problem.get_or_insert(problem);
            } else if terminal == DdTerminalDisposition::Delete {
                job.temporary_datasets
                    .retain(|dataset| dataset != allocation.dataset.as_str());
            }
        }
        let mut released = BTreeMap::<String, ()>::new();
        for allocation in allocations {
            let Some(lock_id) = &allocation.lock_id else {
                continue;
            };
            if released.insert(lock_id.clone(), ()).is_some() {
                continue;
            }
            let sequence = next_effect_sequence(invocation, effect_sequence)?;
            let key = effect_key(job, step, sequence)?;
            let result = self.host.invoke(
                invocation,
                invocation.deadline_tick.saturating_sub(1),
                false,
                EffectRequest {
                    run_unit: invocation.run_unit_id.clone(),
                    sequence,
                    deadline_tick: invocation.deadline_tick,
                    idempotency_key: Some(key.clone()),
                    request: HostRequest::Dataset(DatasetRequest::ReleaseLock {
                        dataset: allocation.dataset.clone(),
                        lock_id: lock_id.clone(),
                        owner: allocation.lock_owner.clone(),
                        mutation: Mutation {
                            sequence,
                            idempotency_key: key,
                            transaction: Some(job.id.clone()),
                        },
                    }),
                },
            );
            if let Err(problem) = result.effect.outcome {
                first_problem.get_or_insert(problem);
            }
        }
        if let Some(problem) = first_problem {
            return Err(problem);
        }
        Ok(())
    }

    pub fn get(&self, id: &str) -> Result<JobSnapshot, HostProblem> {
        self.lock()?
            .jobs
            .get(id)
            .map(snapshot)
            .ok_or(HostProblem::NotFound)
    }

    pub fn list(
        &self,
        owner: Option<&PrincipalId>,
        start: Option<&str>,
        max: usize,
    ) -> Result<(Vec<JobSnapshot>, bool), HostProblem> {
        if max == 0 || max > self.limits.max_jobs {
            return Err(HostProblem::ResourceExhausted);
        }
        let state = self.lock()?;
        let mut jobs = Vec::new();
        let mut more = false;
        for job in state.jobs.values().filter(|job| {
            owner.is_none_or(|owner| owner.as_str() == job.owner)
                && start.is_none_or(|start| job.id.as_str() > start)
        }) {
            if jobs.len() == max {
                more = true;
                break;
            }
            jobs.push(snapshot(job));
        }
        Ok((jobs, more))
    }

    pub fn spool(
        &self,
        invocation: &Invocation,
        id: &str,
        file: &str,
        start: usize,
        max: usize,
    ) -> Result<(Vec<Vec<u8>>, bool), HostProblem> {
        if max == 0 || max > self.limits.max_spool_records {
            return Err(HostProblem::ResourceExhausted);
        }
        self.ensure_spool_migrated(invocation, id)?;
        let job = self
            .lock()?
            .jobs
            .get(id)
            .cloned()
            .ok_or(HostProblem::NotFound)?;
        let requested = file.to_ascii_uppercase();
        let files = if job.spool_files.contains_key(&requested) {
            vec![requested]
        } else {
            job.spool_files
                .iter()
                .filter(|(_, descriptor)| descriptor.dd_name == requested)
                .map(|(name, _)| name.clone())
                .collect::<Vec<_>>()
        };
        if files.is_empty() {
            return Err(HostProblem::NotFound);
        }
        self.read_spool_files(invocation, &job, &files, start, max)
    }

    pub fn spool_files(
        &self,
        invocation: &Invocation,
        id: &str,
    ) -> Result<Vec<(usize, String, usize, usize)>, HostProblem> {
        self.ensure_spool_migrated(invocation, id)?;
        let job = self
            .lock()?
            .jobs
            .get(id)
            .cloned()
            .ok_or(HostProblem::NotFound)?;
        self.authorize(
            invocation,
            "JESJOBS",
            &format!("JOB.{}.*", job.name),
            AccessIntent::Read,
            1,
        )?;
        let result = self.host.invoke(
            invocation,
            invocation.deadline_tick.saturating_sub(1),
            false,
            EffectRequest {
                run_unit: invocation.run_unit_id.clone(),
                sequence: 2,
                deadline_tick: invocation.deadline_tick,
                idempotency_key: None,
                request: HostRequest::Spool(SpoolRequest::List {
                    job: JobName::new(&job.id, 128).map_err(|_| HostProblem::Malformed)?,
                }),
            },
        );
        let HostResult::Spool(SpoolResult::Files { files }) = result.effect.outcome? else {
            return Err(HostProblem::ProviderFailure);
        };
        files
            .into_iter()
            .enumerate()
            .map(|(index, file)| {
                if !job.spool_files.contains_key(&file.file) {
                    return Err(HostProblem::InfrastructureFailure);
                }
                Ok((
                    index,
                    file.file,
                    usize::try_from(file.record_count)
                        .map_err(|_| HostProblem::ResourceExhausted)?,
                    usize::try_from(file.byte_count).map_err(|_| HostProblem::ResourceExhausted)?,
                ))
            })
            .collect()
    }

    pub fn spool_by_index(
        &self,
        invocation: &Invocation,
        id: &str,
        file: usize,
        start: usize,
        max: usize,
    ) -> Result<(Vec<Vec<u8>>, bool), HostProblem> {
        if max == 0 || max > self.limits.max_spool_records {
            return Err(HostProblem::ResourceExhausted);
        }
        self.ensure_spool_migrated(invocation, id)?;
        let job = self
            .lock()?
            .jobs
            .get(id)
            .cloned()
            .ok_or(HostProblem::NotFound)?;
        let name = job
            .spool_files
            .keys()
            .nth(file)
            .cloned()
            .ok_or(HostProblem::NotFound)?;
        self.read_spool_files(invocation, &job, &[name], start, max)
    }

    pub fn output_groups(
        &self,
        invocation: &Invocation,
        id: &str,
    ) -> Result<Vec<JesOutputGroup>, HostProblem> {
        self.ensure_spool_migrated(invocation, id)?;
        let job = self
            .lock()?
            .jobs
            .get(id)
            .cloned()
            .ok_or(HostProblem::NotFound)?;
        self.authorize(
            invocation,
            "JESJOBS",
            &format!("JOB.{}.*", job.name),
            AccessIntent::Read,
            1,
        )?;
        Ok(job.output_groups.into_values().collect())
    }

    pub fn hold_output(
        &self,
        invocation: &Invocation,
        id: &str,
        output_id: &str,
    ) -> Result<JesOutputGroup, HostProblem> {
        self.ensure_spool_migrated(invocation, id)?;
        self.transition_output(
            invocation,
            id,
            output_id,
            &[JesOutputState::AwaitingSelection, JesOutputState::Released],
            JesOutputState::Held,
            JesSpoolState::Held,
            "output-held",
        )
    }

    pub fn release_output(
        &self,
        invocation: &Invocation,
        id: &str,
        output_id: &str,
    ) -> Result<JesOutputGroup, HostProblem> {
        self.ensure_spool_migrated(invocation, id)?;
        self.transition_output(
            invocation,
            id,
            output_id,
            &[JesOutputState::Held],
            JesOutputState::Released,
            JesSpoolState::Released,
            "output-released",
        )
    }

    pub fn select_output(
        &self,
        invocation: &Invocation,
        id: &str,
        output_id: &str,
    ) -> Result<JesOutputGroup, HostProblem> {
        self.ensure_spool_migrated(invocation, id)?;
        self.transition_output(
            invocation,
            id,
            output_id,
            &[JesOutputState::AwaitingSelection, JesOutputState::Released],
            JesOutputState::Selected,
            JesSpoolState::Selected,
            "output-selected",
        )
    }

    pub fn complete_selected_output(
        &self,
        invocation: &Invocation,
        id: &str,
        output_id: &str,
    ) -> Result<JesOutputGroup, HostProblem> {
        self.ensure_spool_migrated(invocation, id)?;
        self.transition_output(
            invocation,
            id,
            output_id,
            &[JesOutputState::Selected, JesOutputState::Printing],
            JesOutputState::Complete,
            JesSpoolState::Complete,
            "output-complete",
        )
    }

    pub fn route_output(
        &self,
        invocation: &Invocation,
        id: &str,
        output_id: &str,
        destination: &str,
        writer: Option<&str>,
        forms: Option<&str>,
    ) -> Result<JesOutputGroup, HostProblem> {
        validate_output_route(destination, writer, forms)?;
        self.ensure_spool_migrated(invocation, id)?;
        let mut state = self.lock()?;
        let current = state.jobs.get(id).cloned().ok_or(HostProblem::NotFound)?;
        let current_group = current
            .output_groups
            .get(output_id)
            .ok_or(HostProblem::NotFound)?;
        if !matches!(
            current_group.state,
            JesOutputState::AwaitingSelection | JesOutputState::Held | JesOutputState::Released
        ) {
            return Err(invalid_state());
        }
        self.authorize(
            invocation,
            "JESJOBS",
            &format!("JOB.{}.{}", current.name, output_id),
            AccessIntent::Control,
            1,
        )?;
        let mut next = current.clone();
        next.version = next.version.saturating_add(1);
        let group = next
            .output_groups
            .get_mut(output_id)
            .ok_or(HostProblem::InfrastructureFailure)?;
        group.destination = destination.to_ascii_uppercase();
        group.writer = writer.map(str::to_ascii_uppercase);
        group.forms = forms.map(str::to_ascii_uppercase);
        for file in &group.spool_files {
            let descriptor = next
                .spool_files
                .get_mut(file)
                .ok_or(HostProblem::InfrastructureFailure)?;
            descriptor.destination.clone_from(&group.destination);
            descriptor.writer.clone_from(&group.writer);
            descriptor.forms.clone_from(&group.forms);
        }
        next.events.push(format!("output-routed:{output_id}"));
        self.persist_job(&next, Some(current.version))?;
        let result = next
            .output_groups
            .get(output_id)
            .cloned()
            .ok_or(HostProblem::InfrastructureFailure)?;
        state.jobs.insert(id.into(), next);
        Ok(result)
    }

    pub fn purge_expired(
        &self,
        invocation: &Invocation,
        now_tick: u64,
        max: usize,
    ) -> Result<Vec<String>, HostProblem> {
        if max == 0 || max > self.limits.max_jobs {
            return Err(HostProblem::ResourceExhausted);
        }
        let ids = self
            .lock()?
            .jobs
            .values()
            .filter(|job| {
                job.state.terminal()
                    && !job.output_groups.is_empty()
                    && job
                        .output_groups
                        .values()
                        .all(|group| group.retain_until_tick <= now_tick)
            })
            .take(max)
            .map(|job| job.id.clone())
            .collect::<Vec<_>>();
        let mut purged = Vec::with_capacity(ids.len());
        for id in ids {
            self.purge(invocation, &id)?;
            purged.push(id);
        }
        Ok(purged)
    }

    pub fn purge(&self, invocation: &Invocation, id: &str) -> Result<(), HostProblem> {
        self.ensure_spool_migrated(invocation, id)?;
        let mut state = self.lock()?;
        let mut job = state.jobs.get(id).cloned().ok_or(HostProblem::NotFound)?;
        if !matches!(
            job.state,
            JobState::Completed | JobState::Failed | JobState::Cancelled
        ) {
            return Err(HostProblem::Condition {
                name: "INVALID_STATE".into(),
                response: 409,
                response2: 0,
            });
        }
        match self.purge_spool_authority(invocation, &mut job)? {
            SpoolResult::Mutated { .. } => {}
            SpoolResult::PurgePending { .. } => return Err(HostProblem::UnknownOutcome),
            _ => return Err(HostProblem::ProviderFailure),
        }
        self.store
            .delete_provider_state("jes-job", id, job.version)
            .map_err(store_error)?;
        let submit_key = job.submit_key.clone();
        state.jobs.remove(id);
        state.replay.remove(&submit_key);
        Ok(())
    }

    fn ensure_spool_migrated(&self, invocation: &Invocation, id: &str) -> Result<(), HostProblem> {
        let mut state = self.lock()?;
        let current = state.jobs.get(id).cloned().ok_or(HostProblem::NotFound)?;
        if current.spool.is_empty() {
            return Ok(());
        }
        let mut next = current.clone();
        for (file, records) in current.spool {
            if !records.is_empty() {
                self.append_spool_records(invocation, &mut next, None, None, &file, records)?;
            }
        }
        next.spool.clear();
        next.version = next.version.saturating_add(1);
        next.events
            .push("migrated:embedded-spool-to-artifacts".into());
        self.persist_job(&next, Some(current.version))?;
        state.jobs.insert(id.into(), next);
        Ok(())
    }

    fn read_spool_files(
        &self,
        invocation: &Invocation,
        job: &Job,
        files: &[String],
        start: usize,
        max: usize,
    ) -> Result<(Vec<Vec<u8>>, bool), HostProblem> {
        let total_records = files.iter().try_fold(0usize, |total, file| {
            let descriptor = job
                .spool_files
                .get(file)
                .ok_or(HostProblem::InfrastructureFailure)?;
            total
                .checked_add(descriptor.record_count)
                .ok_or(HostProblem::ResourceExhausted)
        })?;
        let mut skip = start;
        let mut records = Vec::with_capacity(max.min(total_records.saturating_sub(start)));
        for (index, file) in files.iter().enumerate() {
            let descriptor = job
                .spool_files
                .get(file)
                .ok_or(HostProblem::InfrastructureFailure)?;
            if skip >= descriptor.record_count {
                skip -= descriptor.record_count;
                continue;
            }
            let remaining = max.saturating_sub(records.len());
            if remaining == 0 {
                break;
            }
            let sequence = u64::try_from(index)
                .ok()
                .and_then(|index| index.checked_mul(2))
                .and_then(|sequence| sequence.checked_add(1))
                .ok_or(HostProblem::ResourceExhausted)?;
            self.authorize(
                invocation,
                "JESJOBS",
                &spool_resource(job, file),
                AccessIntent::Read,
                sequence,
            )?;
            let result = self.host.invoke(
                invocation,
                invocation.deadline_tick.saturating_sub(1),
                false,
                EffectRequest {
                    run_unit: invocation.run_unit_id.clone(),
                    sequence: sequence.saturating_add(1),
                    deadline_tick: invocation.deadline_tick,
                    idempotency_key: None,
                    request: HostRequest::Spool(SpoolRequest::Read {
                        job: JobName::new(&job.id, 128).map_err(|_| HostProblem::Malformed)?,
                        file: file.clone(),
                        start: u64::try_from(skip).map_err(|_| HostProblem::ResourceExhausted)?,
                        max_records: u32::try_from(remaining)
                            .map_err(|_| HostProblem::ResourceExhausted)?,
                    }),
                },
            );
            let HostResult::Spool(SpoolResult::Records {
                records: selected, ..
            }) = result.effect.outcome?
            else {
                return Err(HostProblem::ProviderFailure);
            };
            records.extend(selected);
            skip = 0;
        }
        let consumed = start.saturating_add(records.len());
        Ok((records, consumed < total_records))
    }

    #[allow(clippy::too_many_arguments)]
    fn transition_output(
        &self,
        invocation: &Invocation,
        id: &str,
        output_id: &str,
        from: &[JesOutputState],
        to: JesOutputState,
        spool_state: JesSpoolState,
        event: &str,
    ) -> Result<JesOutputGroup, HostProblem> {
        let mut state = self.lock()?;
        let current = state.jobs.get(id).cloned().ok_or(HostProblem::NotFound)?;
        let current_group = current
            .output_groups
            .get(output_id)
            .ok_or(HostProblem::NotFound)?;
        if !from.contains(&current_group.state) || !current_group.state.can_transition_to(to) {
            return Err(invalid_state());
        }
        self.authorize(
            invocation,
            "JESJOBS",
            &format!("JOB.{}.{}", current.name, output_id),
            AccessIntent::Control,
            1,
        )?;
        let mut next = current.clone();
        next.version = next.version.saturating_add(1);
        let files = {
            let group = next
                .output_groups
                .get_mut(output_id)
                .ok_or(HostProblem::InfrastructureFailure)?;
            group.state = to;
            group.spool_files.clone()
        };
        for file in files {
            let descriptor = next
                .spool_files
                .get_mut(&file)
                .ok_or(HostProblem::InfrastructureFailure)?;
            if !descriptor.state.can_transition_to(spool_state) {
                return Err(invalid_state());
            }
            descriptor.state = spool_state;
        }
        next.events.push(format!("{event}:{output_id}"));
        self.persist_job(&next, Some(current.version))?;
        let result = next
            .output_groups
            .get(output_id)
            .cloned()
            .ok_or(HostProblem::InfrastructureFailure)?;
        state.jobs.insert(id.into(), next);
        Ok(result)
    }

    fn transition(
        &self,
        invocation: &Invocation,
        id: &str,
        from: JobState,
        to: JobState,
        event: &str,
    ) -> Result<JobSnapshot, HostProblem> {
        let mut state = self.lock()?;
        let current = state.jobs.get(id).cloned().ok_or(HostProblem::NotFound)?;
        if current.state != from {
            return Err(invalid_state());
        }
        self.authorize(
            invocation,
            "JESJOBS",
            &format!("JOB.{}", current.name),
            AccessIntent::Control,
            1,
        )?;
        let mut next = current.clone();
        next.version += 1;
        next.state = to;
        next.events.push(event.into());
        self.persist_job(&next, Some(current.version))?;
        let result = snapshot(&next);
        state.jobs.insert(id.into(), next);
        Ok(result)
    }

    fn mutate_queued_job(
        &self,
        invocation: &Invocation,
        id: &str,
        event: &str,
        mutation: impl FnOnce(&mut Job) -> Result<(), HostProblem>,
    ) -> Result<JobSnapshot, HostProblem> {
        let mut state = self.lock()?;
        let current = state.jobs.get(id).cloned().ok_or(HostProblem::NotFound)?;
        if !matches!(current.state, JobState::Queued | JobState::Held) {
            return Err(invalid_state());
        }
        self.authorize(
            invocation,
            "JESJOBS",
            &format!("JOB.{}", current.name),
            AccessIntent::Control,
            1,
        )?;
        let mut next = current.clone();
        mutation(&mut next)?;
        next.version = next
            .version
            .checked_add(1)
            .ok_or(HostProblem::ResourceExhausted)?;
        next.events.push(event.into());
        self.persist_job(&next, Some(current.version))?;
        let result = snapshot(&next);
        state.jobs.insert(id.into(), next);
        Ok(result)
    }

    fn set_initiator(
        &self,
        invocation: &Invocation,
        initiator: &str,
        enabled: bool,
    ) -> Result<(), HostProblem> {
        validate_jes_name(initiator)?;
        let initiator = initiator.to_ascii_uppercase();
        self.authorize(
            invocation,
            "OPERCMDS",
            &format!("JES2.INITIATOR.{initiator}"),
            AccessIntent::Control,
            1,
        )?;
        let mut durable = self
            .scheduler
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        let current = durable
            .configuration
            .initiators
            .get(&initiator)
            .ok_or(HostProblem::NotFound)?;
        if current.enabled == enabled {
            return Ok(());
        }
        let mut configuration = durable.configuration.clone();
        configuration
            .initiators
            .get_mut(&initiator)
            .ok_or(HostProblem::InfrastructureFailure)?
            .enabled = enabled;
        configuration.validate()?;
        let version = durable
            .store_version
            .checked_add(1)
            .ok_or(HostProblem::ResourceExhausted)?;
        self.store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: SCHEDULER_STATE_NAMESPACE.into(),
                    key: CONFIGURATION_STATE_KEY.into(),
                    version,
                    payload: serde_json::to_vec(&configuration)
                        .map_err(|_| HostProblem::InfrastructureFailure)?,
                },
                (durable.store_version > 0).then_some(durable.store_version),
            )
            .map_err(store_error)?;
        durable.store_version = version;
        durable.configuration = configuration;
        Ok(())
    }

    fn default_route(&self) -> Result<JesJobRoute, HostProblem> {
        let topology = self.topology()?;
        let route = JesJobRoute {
            origin_node: topology.local_node.clone(),
            execution_node: topology.local_node.clone(),
            output_node: topology.local_node.clone(),
            owner_member: None,
        };
        topology.validate_route(&route)?;
        Ok(route)
    }

    fn authorize(
        &self,
        invocation: &Invocation,
        class: &str,
        resource: &str,
        intent: AccessIntent,
        sequence: u64,
    ) -> Result<(), HostProblem> {
        let result = self.host.invoke(
            invocation,
            invocation.deadline_tick.saturating_sub(1),
            false,
            EffectRequest {
                run_unit: invocation.run_unit_id.clone(),
                sequence,
                deadline_tick: invocation.deadline_tick,
                idempotency_key: None,
                request: HostRequest::Security(SecurityRequest::Authorize {
                    principal: PrincipalId::new(
                        invocation.principal.id().as_str(),
                        InvocationLimits::default(),
                    )
                    .map_err(|_| HostProblem::InfrastructureFailure)?,
                    class: class.into(),
                    resource: ResourceName::new(resource, 246)
                        .map_err(|_| HostProblem::Malformed)?,
                    intent,
                }),
            },
        );
        match result.effect.outcome? {
            HostResult::Security(SecurityDecision::Allow) => Ok(()),
            HostResult::Security(_) => Err(HostProblem::Unauthorized),
            _ => Err(HostProblem::ProviderFailure),
        }
    }

    fn persist_meta(&self, next: u64, previous: u64) -> Result<(), HostProblem> {
        self.store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: "jes-meta".into(),
                    key: "next-id".into(),
                    version: previous,
                    payload: next.to_be_bytes().to_vec(),
                },
                (previous > 1).then_some(previous - 1),
            )
            .map_err(store_error)
    }

    fn persist_job(&self, job: &Job, expected: Option<u64>) -> Result<(), HostProblem> {
        self.store
            .put_provider_state(job_record(job)?, expected)
            .map_err(store_error)
    }

    fn persist_controllers(
        &self,
        durable: &mut DurableControllers,
        registry: BatchControllerRegistry,
    ) -> Result<(), HostProblem> {
        let payload = registry.state_payload()?;
        let version = durable
            .store_version
            .checked_add(1)
            .ok_or(HostProblem::ResourceExhausted)?;
        self.store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: CONTROLLER_STATE_NAMESPACE.into(),
                    key: CONTROLLER_STATE_KEY.into(),
                    version,
                    payload,
                },
                (durable.store_version != 0).then_some(durable.store_version),
            )
            .map_err(store_error)?;
        durable.store_version = version;
        durable.registry = registry;
        Ok(())
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, State>, HostProblem> {
        self.state
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)
    }
}

fn dataset_attributes_for_dd(dd: &crate::DdPlan) -> Result<DatasetAttributes, HostProblem> {
    let organization = match dd.organization.as_deref().unwrap_or("PS") {
        "PS" => DatasetOrganization::Sequential,
        "PO" => DatasetOrganization::Partitioned,
        _ => return Err(HostProblem::Unsupported),
    };
    let record_format = match dd.record_format.as_deref().unwrap_or("V") {
        "F" => RecordFormat::Fixed,
        "FB" => RecordFormat::FixedBlocked,
        "V" => RecordFormat::Variable,
        "VB" => RecordFormat::VariableBlocked,
        "U" => RecordFormat::Undefined,
        _ => return Err(HostProblem::Unsupported),
    };
    let logical_record_length = dd.logical_record_length.unwrap_or({
        if matches!(
            record_format,
            RecordFormat::Fixed | RecordFormat::FixedBlocked
        ) {
            80
        } else {
            32_760
        }
    });
    Ok(DatasetAttributes {
        organization,
        record_format,
        logical_record_length,
        key_offset: None,
        key_length: None,
        ccsid: Some(37),
    })
}

fn dd_lock_owner(job: &Job) -> Result<PrincipalId, HostProblem> {
    PrincipalId::new(&job.owner, InvocationLimits::default())
        .map_err(|_| HostProblem::ResourceExhausted)
}

fn append_inline_records(target: &mut Vec<u8>, records: Vec<Vec<u8>>) {
    for record in records {
        target.extend_from_slice(&record);
        target.push(b'\n');
    }
}

fn is_program_library_dd(dd: &crate::DdPlan) -> bool {
    dd.name.eq_ignore_ascii_case("STEPLIB")
        || dd.name.eq_ignore_ascii_case("JOBLIB")
        || dd.name.eq_ignore_ascii_case("DBRMLIB")
        || dd.name.eq_ignore_ascii_case("DFSRESLB")
        || dd.name.eq_ignore_ascii_case("IMS")
        || dd.name.eq_ignore_ascii_case("DFSVSAMP")
        || dd.name.eq_ignore_ascii_case("PROCLIB")
        || dd.name.eq_ignore_ascii_case("DFSSEL")
        || dd.name.to_ascii_uppercase().starts_with("DDPAUT")
}

fn normalize_records(
    records: &[Vec<u8>],
    attributes: &DatasetAttributes,
) -> Result<Vec<Vec<u8>>, HostProblem> {
    records
        .iter()
        .cloned()
        .map(|mut record| {
            if record.len() > attributes.logical_record_length as usize {
                return Err(HostProblem::Condition {
                    name: "LENGERR".into(),
                    response: 22,
                    response2: 0,
                });
            }
            if matches!(
                attributes.record_format,
                RecordFormat::Fixed | RecordFormat::FixedBlocked
            ) {
                record.resize(attributes.logical_record_length as usize, b' ');
            }
            Ok(record)
        })
        .collect()
}

fn resolved_dataset(
    job: &Job,
    dd: &crate::DdPlan,
    raw: &str,
    resolutions: &BTreeMap<String, String>,
) -> String {
    if raw.starts_with("&&") {
        format!(
            "{}.TMP.{}.{}",
            job.owner,
            job.id,
            raw.trim_start_matches('&').to_ascii_uppercase()
        )
    } else if dd.generation.is_some() {
        resolutions
            .get(&dataset_resolution_key(dd, raw))
            .cloned()
            .unwrap_or_else(|| raw.to_ascii_uppercase())
    } else {
        raw.to_ascii_uppercase()
    }
}

fn input_dd_records(input: &ProgramInput, name: &str) -> Result<Vec<Vec<u8>>, HostProblem> {
    let dds = input
        .dds
        .iter()
        .filter(|dd| dd.name.eq_ignore_ascii_case(name))
        .collect::<Vec<_>>();
    if dds.is_empty() {
        return Err(HostProblem::NotFound);
    }
    Ok(dds
        .into_iter()
        .flat_map(|dd| {
            dd.inline_data
                .split(|byte| *byte == b'\n')
                .filter(|record| !record.is_empty())
                .map(<[u8]>::to_vec)
        })
        .collect())
}

fn input_dd_text(input: &ProgramInput, name: &str) -> Result<String, HostProblem> {
    String::from_utf8(
        input_dd_records(input, name)?
            .into_iter()
            .flat_map(|mut record| {
                record.push(b'\n');
                record
            })
            .collect(),
    )
    .map(|text| text.to_ascii_uppercase())
    .map_err(|_| HostProblem::Malformed)
}

fn tso_run_program(control: &str) -> Result<String, HostProblem> {
    let upper = control.to_ascii_uppercase();
    let start = upper.find("RUN PROGRAM(").ok_or(HostProblem::Unsupported)? + "RUN PROGRAM(".len();
    let end = upper[start..]
        .find(')')
        .map(|offset| start + offset)
        .ok_or(HostProblem::Malformed)?;
    let program = upper[start..end].trim();
    if program.is_empty()
        || program.len() > 128
        || !program.bytes().all(|byte| byte.is_ascii_alphanumeric())
    {
        Err(HostProblem::Malformed)
    } else {
        Ok(program.into())
    }
}

fn ims_controller_selector(parameter: &str) -> Result<BatchControllerSelector, HostProblem> {
    let normalized = parameter
        .trim()
        .trim_matches(|character| matches!(character, '\'' | '"' | '(' | ')'));
    let fields = normalized.split(',').map(str::trim).collect::<Vec<_>>();
    let mode = fields.first().copied().ok_or(HostProblem::Malformed)?;
    let program = fields.get(1).copied().ok_or(HostProblem::Malformed)?;
    let qualifier = fields.get(2).copied().filter(|value| !value.is_empty());
    BatchControllerSelector::ims(mode, program, qualifier)
}

struct SdsfFileControl {
    file: String,
    status: &'static str,
}

fn parse_sdsf_file_controls(control: &str) -> Result<Vec<SdsfFileControl>, HostProblem> {
    let mut controls = Vec::new();
    let mut region = None;
    for line in control.lines().filter(|line| !line.trim().is_empty()) {
        let command = line.trim();
        let (target, cemt) = command
            .strip_prefix("/F ")
            .and_then(|command| command.split_once(",'"))
            .ok_or(HostProblem::Malformed)?;
        if target.is_empty()
            || target.len() > 8
            || !target.bytes().all(|byte| byte.is_ascii_alphanumeric())
            || region
                .as_ref()
                .is_some_and(|existing: &String| existing != target)
        {
            return Err(HostProblem::Malformed);
        }
        region.get_or_insert_with(|| target.to_string());
        let cemt = cemt.strip_suffix('\'').ok_or(HostProblem::Malformed)?;
        let rest = cemt
            .strip_prefix("CEMT SET FIL(")
            .ok_or(HostProblem::Unsupported)?;
        let (file, status) = rest.split_once(')').ok_or(HostProblem::Malformed)?;
        let file = file.trim();
        if file.is_empty()
            || file.len() > 16
            || !file
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        {
            return Err(HostProblem::Malformed);
        }
        let status = match status.trim() {
            "CLO" | "CLOSED" => "CLOSED",
            "OPE" | "OPEN" => "OPEN",
            _ => return Err(HostProblem::Unsupported),
        };
        controls.push(SdsfFileControl {
            file: file.into(),
            status,
        });
    }
    if controls.is_empty() {
        Err(HostProblem::Malformed)
    } else {
        Ok(controls)
    }
}

fn ams_condition_code(problem: &HostProblem) -> u8 {
    match problem {
        HostProblem::NotFound => 8,
        HostProblem::Condition { response, .. } if *response <= 4 => 4,
        HostProblem::Condition { response, .. } if *response <= 8 => 8,
        HostProblem::Condition { response, .. } if *response <= 12 => 12,
        HostProblem::Unsupported
        | HostProblem::UnsupportedCapability { .. }
        | HostProblem::Malformed
        | HostProblem::Unauthorized
        | HostProblem::IdempotencyConflict => 12,
        _ => 16,
    }
}

fn abend_code(problem: &HostProblem) -> Option<String> {
    match problem {
        HostProblem::Condition { name, .. } => name.strip_prefix("ABEND:").and_then(|code| {
            (!code.is_empty()
                && code.len() <= 16
                && code
                    .bytes()
                    .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit()))
            .then(|| code.to_string())
        }),
        _ => None,
    }
}

fn problem_category(problem: &HostProblem) -> &'static str {
    match problem {
        HostProblem::Malformed => "malformed",
        HostProblem::Unsupported | HostProblem::UnsupportedCapability { .. } => "unsupported",
        HostProblem::NotFound => "not-found",
        HostProblem::Condition { .. } => "condition",
        HostProblem::Unauthorized => "unauthorized",
        HostProblem::Cancelled => "cancelled",
        HostProblem::TimedOut => "timed-out",
        HostProblem::ResourceExhausted => "resource-exhausted",
        HostProblem::ProviderFailure => "provider-failure",
        HostProblem::InfrastructureFailure => "infrastructure-failure",
        HostProblem::MissingIdempotency => "missing-idempotency",
        HostProblem::IdempotencyConflict => "idempotency-conflict",
        HostProblem::UnknownOutcome => "unknown-outcome",
    }
}

fn unimplemented_ams_operand(command: &AmsCommand) -> Option<(&'static str, &'static str)> {
    let terms = ams_top_level_terms(command);
    if terms
        .iter()
        .any(|term| matches!(term.as_str(), "DATA" | "INDEX" | "FREESPACE"))
    {
        return Some(("vsam-components", "DATA/INDEX component override"));
    }
    terms
        .iter()
        .find(|term| !ams_operand_allowed(command.id(), term))
        .map(|_| ("ams-operand", "unknown or inapplicable AMS operand"))
}

fn ams_top_level_terms(command: &AmsCommand) -> Vec<String> {
    let mut rest = command
        .source()
        .strip_prefix(command.label())
        .unwrap_or_default()
        .trim();
    if rest.starts_with('(') && rest.ends_with(')') {
        rest = &rest[1..rest.len().saturating_sub(1)];
    }
    let bytes = rest.as_bytes();
    let mut at = 0usize;
    let mut terms = Vec::<(String, bool)>::new();
    while at < bytes.len() {
        while bytes
            .get(at)
            .is_some_and(|byte| byte.is_ascii_whitespace() || matches!(byte, b',' | b'(' | b')'))
        {
            at += 1;
        }
        let start = at;
        while bytes
            .get(at)
            .is_some_and(|byte| !byte.is_ascii_whitespace() && !matches!(byte, b',' | b'(' | b')'))
        {
            at += 1;
        }
        if start == at {
            break;
        }
        let token = rest[start..at]
            .trim_matches(['\'', '"'])
            .to_ascii_uppercase();
        while bytes.get(at).is_some_and(u8::is_ascii_whitespace) {
            at += 1;
        }
        let parenthesized = bytes.get(at) == Some(&b'(');
        terms.push((token, parenthesized));
        if parenthesized {
            let mut depth = 0u32;
            let mut quote = None;
            while let Some(byte) = bytes.get(at).copied() {
                if matches!(byte, b'\'' | b'"') {
                    if quote == Some(byte) {
                        quote = None;
                    } else if quote.is_none() {
                        quote = Some(byte);
                    }
                } else if quote.is_none() {
                    if byte == b'(' {
                        depth = depth.saturating_add(1);
                    } else if byte == b')' {
                        depth = depth.saturating_sub(1);
                        if depth == 0 {
                            at += 1;
                            break;
                        }
                    }
                }
                at += 1;
            }
        }
    }
    if matches!(
        command.id(),
        "alter" | "delete" | "diagnose" | "examine" | "listdata" | "verify"
    ) && terms
        .first()
        .is_some_and(|(_, parenthesized)| !parenthesized)
    {
        terms.remove(0);
    }
    terms.into_iter().map(|(term, _)| term).collect()
}

fn ams_operand_allowed(command: &str, operand: &str) -> bool {
    const DEFINITION: &[&str] = &[
        "NAME",
        "DATASET",
        "DSORG",
        "RECORDSIZE",
        "LRECL",
        "RECFM",
        "BLKSIZE",
        "BUFNO",
        "BUFSIZE",
        "CCSID",
        "KEYLEN",
        "KEYOFF",
        "KEYS",
        "TRACKS",
        "CYLINDERS",
        "BLOCKS",
        "KILOBYTES",
        "MEGABYTES",
        "RECORDS",
        "SPACE",
        "PRIMARY",
        "SECONDARY",
        "DIRECTORY",
        "RLSE",
        "NORLSE",
        "CONTIG",
        "NOCONTIG",
        "ROUND",
        "NOROUND",
        "VOLUMES",
        "VOLUME",
        "UNITCOUNT",
        "UNIT",
        "TAPE",
        "DATACLAS",
        "MGMTCLAS",
        "STORCLAS",
        "ACSROUTINE",
        "GUARANTEEDSPACE",
        "NOGUARANTEEDSPACE",
        "EXTENDEDADDRESSABLE",
        "NOEXTENDEDADDRESSABLE",
        "EXTENDED",
        "NOEXTENDED",
        "COMPRESS",
        "NOCOMPRESS",
        "KEYLABEL",
        "STRIPECOUNT",
        "CATALOG",
        "OWNER",
        "ENTRYTYPE",
        "CREATEDATE",
        "TO",
        "EXPIRATION",
        "FOR",
        "RETPD",
        "CONTROLINTERVALSIZE",
        "CISZ",
        "CONTROLAREASIZE",
        "SHAREOPTIONS",
        "BUFFERING",
        "NONRLS",
        "RLS",
        "TVS",
        "REUSE",
        "NOREUSE",
        "SPEED",
        "RECOVERY",
        "WRITECHECK",
        "NOWRITECHECK",
        "ERASE",
        "NOERASE",
        "SPANNED",
        "NOSPANNED",
        "BLOCKED",
        "LINE",
        "INDEXED",
        "NONINDEXED",
        "NUMBERED",
        "LINEAR",
        "UNIQUEKEY",
        "DATA",
        "INDEX",
        "FREESPACE",
    ];
    if matches!(command, "allocate" | "define-cluster" | "define-nonvsam") {
        return DEFINITION.contains(&operand);
    }
    if command == "alter" {
        return DEFINITION.contains(&operand)
            || matches!(
                operand,
                "NEWNAME"
                    | "OPEN"
                    | "CLOSED"
                    | "RECOVERYREQUIRED"
                    | "MIGRATE"
                    | "MIGRATED"
                    | "RECALL"
                    | "RECALLPENDING"
            );
    }
    let allowed: &[&str] = match command {
        "bldindex" => &["INDATASET", "OUTDATASET"],
        "dcollect" => &["OUTFILE", "OFILE"],
        "define-alias" => &["NAME", "RELATE"],
        "define-alternateindex" => &[
            "NAME",
            "RELATE",
            "KEYS",
            "UNIQUEKEY",
            "NONUNIQUEKEY",
            "UPGRADE",
            "NOUPGRADE",
            "DATA",
            "INDEX",
            "FREESPACE",
        ],
        "define-generationdatagroup" => {
            &["NAME", "LIMIT", "SCRATCH", "NOSCRATCH", "EMPTY", "NOEMPTY"]
        }
        "define-path" => &["NAME", "PATHENTRY"],
        "define-usercatalog" => &["NAME"],
        "delete" => &["PURGE", "CURRENTDATE"],
        "diagnose" | "examine" | "listdata" | "verify" => &["INDATASET", "DATASET", "ENTRIES"],
        "export" | "export-disconnect" => &["ENTRIES", "INDATASET", "OUTFILE", "OFILE"],
        "import" | "import-connect" => &["INFILE", "IFILE", "OUTDATASET", "INDATASET", "DATASET"],
        "listcat" => &["ENTRIES", "ENTRY", "LEVEL", "ALL"],
        "print" => &[
            "INDATASET",
            "INFILE",
            "IFILE",
            "HEX",
            "CHAR",
            "SKIP",
            "COUNT",
        ],
        "repro" => &[
            "INDATASET",
            "INFILE",
            "IFILE",
            "OUTDATASET",
            "OUTFILE",
            "OFILE",
            "SKIP",
            "COUNT",
        ],
        "recover" => &["INDATASET", "DATASET", "INFILE", "IFILE"],
        "shcds" => &["DATASET", "TRANSACTION", "COMMIT", "ROLLBACK"],
        _ => &[],
    };
    allowed.contains(&operand)
}

fn ams_values(statement: &str, name: &str) -> Option<Vec<String>> {
    let value = operand(statement, &[name])?;
    Some(
        value
            .split(|character: char| character == ',' || character.is_whitespace())
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .collect(),
    )
}

fn ams_word(statement: &str, word: &str) -> bool {
    statement
        .split(|character: char| !character.is_ascii_alphanumeric())
        .any(|candidate| candidate == word)
}

fn ams_u64_values(statement: &str, name: &str) -> Result<Option<Vec<u64>>, HostProblem> {
    ams_values(statement, name)
        .map(|values| {
            values
                .into_iter()
                .map(|value| value.parse().map_err(|_| HostProblem::Malformed))
                .collect()
        })
        .transpose()
}

fn apply_ams_definition_operands(
    statement: &str,
    definition: &mut mainframe_env_host_api::DatasetDefinition,
) -> Result<(), HostProblem> {
    for name in [
        "LRECL",
        "BLKSIZE",
        "BUFNO",
        "BUFSIZE",
        "CCSID",
        "KEYLEN",
        "KEYOFF",
        "DIRECTORY",
        "UNITCOUNT",
        "CONTROLINTERVALSIZE",
        "CISZ",
        "CONTROLAREASIZE",
        "STRIPECOUNT",
        "CREATEDATE",
        "TO",
        "EXPIRATION",
        "FOR",
        "RETPD",
    ] {
        if operand(statement, &[name]).is_some() && numeric_operand(statement, name).is_none() {
            return Err(HostProblem::Malformed);
        }
    }
    for name in ["RECORDSIZE", "KEYS", "SHAREOPTIONS"] {
        if operand(statement, &[name]).is_some() && pair_operand(statement, name).is_none() {
            return Err(HostProblem::Malformed);
        }
    }
    let indexed = ams_word(statement, "INDEXED");
    let nonindexed = ams_word(statement, "NONINDEXED");
    let numbered = ams_word(statement, "NUMBERED");
    let linear = ams_word(statement, "LINEAR");
    if usize::from(operand(statement, &["DSORG"]).is_some())
        + usize::from(indexed)
        + usize::from(nonindexed)
        + usize::from(numbered)
        + usize::from(linear)
        > 1
    {
        return Err(HostProblem::Malformed);
    }
    if let Some(dsorg) = operand(statement, &["DSORG"]) {
        definition.attributes.organization = match dsorg.as_str() {
            "PS" => DatasetOrganization::Sequential,
            "PO" => DatasetOrganization::Partitioned,
            "PO-E" | "POE" => DatasetOrganization::PartitionedExtended,
            _ => return Err(HostProblem::Malformed),
        };
        definition.allocation.directory_blocks = if matches!(
            definition.attributes.organization,
            DatasetOrganization::Partitioned | DatasetOrganization::PartitionedExtended
        ) {
            definition.allocation.directory_blocks.max(1)
        } else {
            0
        };
    }
    if let Some((minimum, maximum)) = pair_operand(statement, "RECORDSIZE") {
        definition.attributes.logical_record_length = maximum;
        definition.attributes.record_format = if minimum == maximum {
            RecordFormat::Fixed
        } else {
            RecordFormat::Variable
        };
    }
    if let Some(value) = numeric_operand(statement, "LRECL") {
        definition.attributes.logical_record_length = value;
    }
    if let Some(recfm) = operand(statement, &["RECFM"]) {
        definition.attributes.record_format = match recfm.as_str() {
            "F" => RecordFormat::Fixed,
            "FB" => RecordFormat::FixedBlocked,
            "FBS" => RecordFormat::FixedBlockedStandard,
            "V" => RecordFormat::Variable,
            "VB" => RecordFormat::VariableBlocked,
            "VS" => RecordFormat::VariableSpanned,
            "VBS" => RecordFormat::VariableBlockedSpanned,
            "U" => RecordFormat::Undefined,
            "LINE" => RecordFormat::Line,
            _ => return Err(HostProblem::Malformed),
        };
        definition.vsam.spanned = matches!(
            definition.attributes.record_format,
            RecordFormat::VariableSpanned | RecordFormat::VariableBlockedSpanned
        );
    }
    if statement.contains(" NOSPANNED") {
        definition.vsam.spanned = false;
        definition.attributes.record_format = match definition.attributes.record_format {
            RecordFormat::VariableSpanned => RecordFormat::Variable,
            RecordFormat::VariableBlockedSpanned => RecordFormat::VariableBlocked,
            value => value,
        };
    } else if statement.contains(" SPANNED") {
        definition.vsam.spanned = true;
        definition.attributes.record_format = match definition.attributes.record_format {
            RecordFormat::Variable => RecordFormat::VariableSpanned,
            RecordFormat::VariableBlocked => RecordFormat::VariableBlockedSpanned,
            value => value,
        };
    }
    if ams_word(statement, "BLOCKED") {
        definition.attributes.record_format = match definition.attributes.record_format {
            RecordFormat::Fixed => RecordFormat::FixedBlocked,
            RecordFormat::Variable => RecordFormat::VariableBlocked,
            RecordFormat::VariableSpanned => RecordFormat::VariableBlockedSpanned,
            value => value,
        };
    }
    if ams_word(statement, "LINE") {
        definition.attributes.record_format = RecordFormat::Line;
    }
    if linear {
        definition.attributes.organization = DatasetOrganization::Linear;
        definition.attributes.record_format = RecordFormat::Undefined;
        definition.attributes.key_offset = None;
        definition.attributes.key_length = None;
        definition.vsam.spanned = false;
    } else if numbered {
        definition.attributes.organization = if matches!(
            definition.attributes.record_format,
            RecordFormat::Variable
                | RecordFormat::VariableBlocked
                | RecordFormat::VariableSpanned
                | RecordFormat::VariableBlockedSpanned
        ) {
            DatasetOrganization::VariableRelative
        } else {
            DatasetOrganization::Relative
        };
        definition.attributes.key_offset = None;
        definition.attributes.key_length = None;
    } else if nonindexed {
        definition.attributes.organization = DatasetOrganization::EntrySequenced;
        definition.attributes.key_offset = None;
        definition.attributes.key_length = None;
    } else if indexed {
        definition.attributes.organization = DatasetOrganization::KeySequenced;
    }
    if let Some((length, offset)) = pair_operand(statement, "KEYS") {
        definition.attributes.key_length = Some(length);
        definition.attributes.key_offset = Some(offset);
    }
    if let Some(value) = numeric_operand(statement, "KEYLEN") {
        definition.attributes.key_length = Some(value);
    }
    if let Some(value) = numeric_operand(statement, "KEYOFF") {
        definition.attributes.key_offset = Some(value);
    }
    if let Some(value) = numeric_operand(statement, "BLKSIZE") {
        definition.dcb.block_size = value;
    }
    if let Some(value) = numeric_operand(statement, "BUFNO") {
        definition.dcb.buffer_count = u16::try_from(value).map_err(|_| HostProblem::Malformed)?;
    }
    definition.dcb.buffer_size =
        numeric_operand(statement, "BUFSIZE").or(definition.dcb.buffer_size);
    if let Some(value) = numeric_operand(statement, "CCSID") {
        definition.attributes.ccsid =
            Some(u16::try_from(value).map_err(|_| HostProblem::Malformed)?);
    }
    definition.vsam.control_interval_size = operand(statement, &["CONTROLINTERVALSIZE", "CISZ"])
        .map(|value| value.parse().map_err(|_| HostProblem::Malformed))
        .transpose()?
        .or(definition.vsam.control_interval_size);
    definition.vsam.control_area_size = operand(statement, &["CONTROLAREASIZE"])
        .map(|value| value.parse().map_err(|_| HostProblem::Malformed))
        .transpose()?
        .or(definition.vsam.control_area_size);
    if let Some((cross_region, cross_system)) = pair_operand(statement, "SHAREOPTIONS") {
        definition.vsam.share_options = mainframe_env_host_api::DatasetShareOptions {
            cross_region: u8::try_from(cross_region).map_err(|_| HostProblem::Malformed)?,
            cross_system: u8::try_from(cross_system).map_err(|_| HostProblem::Malformed)?,
        };
    }
    if let Some(buffering) = operand(statement, &["BUFFERING"]) {
        definition.vsam.buffering = match buffering.as_str() {
            "SYSTEM" => mainframe_env_host_api::BufferingMode::System,
            "NSR" => mainframe_env_host_api::BufferingMode::NonsharedResources,
            "LSR" => mainframe_env_host_api::BufferingMode::LocalSharedResources,
            "GSR" => mainframe_env_host_api::BufferingMode::GlobalSharedResources,
            _ => return Err(HostProblem::Malformed),
        };
    }
    if statement.contains(" NONRLS") {
        definition.vsam.access_mode = mainframe_env_host_api::VsamAccessMode::NonRls;
    } else if statement.contains(" TVS") {
        definition.vsam.access_mode = mainframe_env_host_api::VsamAccessMode::Tvs;
    } else if statement.contains(" RLS") {
        definition.vsam.access_mode = mainframe_env_host_api::VsamAccessMode::Rls;
    }
    if statement.contains(" NOREUSE") {
        definition.vsam.reuse = false;
    } else if statement.contains(" REUSE") {
        definition.vsam.reuse = true;
    }
    if statement.contains(" RECOVERY") {
        definition.vsam.speed = false;
    } else if statement.contains(" SPEED") {
        definition.vsam.speed = true;
    }
    if statement.contains(" NOWRITECHECK") {
        definition.vsam.write_check = false;
    } else if statement.contains(" WRITECHECK") {
        definition.vsam.write_check = true;
    }
    if statement.contains(" NOERASE") {
        definition.vsam.erase_on_delete = false;
    } else if statement.contains(" ERASE") {
        definition.vsam.erase_on_delete = true;
    }

    let nested_space = operand(statement, &["SPACE"]).is_some();
    let mut applied_space = false;
    for (operand_name, unit) in [
        ("TRACKS", mainframe_env_host_api::SpaceUnit::Tracks),
        ("CYLINDERS", mainframe_env_host_api::SpaceUnit::Cylinders),
        ("BLOCKS", mainframe_env_host_api::SpaceUnit::Blocks),
        ("KILOBYTES", mainframe_env_host_api::SpaceUnit::Kilobytes),
        ("MEGABYTES", mainframe_env_host_api::SpaceUnit::Megabytes),
        ("RECORDS", mainframe_env_host_api::SpaceUnit::Records),
    ] {
        if let Some(values) = ams_u64_values(statement, operand_name)? {
            if values.is_empty() || values.len() > 2 {
                return Err(HostProblem::Malformed);
            }
            definition.allocation.unit = unit;
            definition.allocation.primary = values[0];
            definition.allocation.secondary = values.get(1).copied().unwrap_or(0);
            applied_space = true;
        }
    }
    if let Some(value) =
        ams_u64_values(statement, "PRIMARY")?.and_then(|values| values.first().copied())
    {
        definition.allocation.primary = value;
        applied_space = true;
    }
    if let Some(value) =
        ams_u64_values(statement, "SECONDARY")?.and_then(|values| values.first().copied())
    {
        definition.allocation.secondary = value;
        applied_space = true;
    }
    if nested_space && !applied_space {
        return Err(HostProblem::Malformed);
    }
    if let Some(value) = numeric_operand(statement, "DIRECTORY") {
        definition.allocation.directory_blocks = value;
    }
    if statement.contains(" NORLSE") {
        definition.allocation.release_unused = false;
    } else if statement.contains(" RLSE") {
        definition.allocation.release_unused = true;
    }
    if statement.contains(" NOCONTIG") {
        definition.allocation.contiguous = false;
    } else if statement.contains(" CONTIG") {
        definition.allocation.contiguous = true;
    }
    if statement.contains(" NOROUND") {
        definition.allocation.round_to_cylinder = false;
    } else if statement.contains(" ROUND") {
        definition.allocation.round_to_cylinder = true;
    }

    if let Some(volumes) =
        ams_values(statement, "VOLUMES").or_else(|| ams_values(statement, "VOLUME"))
    {
        if volumes.is_empty() {
            return Err(HostProblem::Malformed);
        }
        definition.volumes.volume_ids = volumes;
    }
    if statement.contains(" TAPE") {
        definition.volumes.kind = mainframe_env_host_api::VolumeKind::Tape;
        definition.volumes.device_type =
            operand(statement, &["TAPE"]).or_else(|| Some("TAPE".into()));
    }
    if let Some(value) = numeric_operand(statement, "UNITCOUNT") {
        definition.volumes.unit_count = u16::try_from(value).map_err(|_| HostProblem::Malformed)?;
    } else if let Some(unit) = ams_values(statement, "UNIT") {
        if unit.is_empty() || unit.len() > 2 {
            return Err(HostProblem::Malformed);
        }
        if unit.len() == 1
            && unit[0]
                .parse::<u16>()
                .is_ok_and(|count| (1..=255).contains(&count))
        {
            definition.volumes.unit_count = unit[0].parse().map_err(|_| HostProblem::Malformed)?;
        } else {
            definition.volumes.device_type = Some(unit[0].clone());
            definition.volumes.kind = if definition.volumes.kind
                == mainframe_env_host_api::VolumeKind::Tape
                || unit[0].contains("TAPE")
            {
                mainframe_env_host_api::VolumeKind::Tape
            } else {
                mainframe_env_host_api::VolumeKind::PhysicalDisk
            };
            if let Some(count) = unit.get(1) {
                definition.volumes.unit_count =
                    count.parse().map_err(|_| HostProblem::Malformed)?;
            }
        }
    }

    definition.sms.data_class =
        operand(statement, &["DATACLAS"]).or(definition.sms.data_class.take());
    definition.sms.management_class =
        operand(statement, &["MGMTCLAS"]).or(definition.sms.management_class.take());
    definition.sms.storage_class =
        operand(statement, &["STORCLAS"]).or(definition.sms.storage_class.take());
    definition.sms.acs_routine =
        operand(statement, &["ACSROUTINE"]).or(definition.sms.acs_routine.take());
    if statement.contains(" NOGUARANTEEDSPACE") {
        definition.sms.guaranteed_space = false;
    } else if statement.contains(" GUARANTEEDSPACE") {
        definition.sms.guaranteed_space = true;
    }
    if statement.contains(" NOEXTENDEDADDRESSABLE") {
        definition.sms.extended_addressable = false;
    } else if statement.contains(" EXTENDEDADDRESSABLE") {
        definition.sms.extended_addressable = true;
    }
    if statement.contains(" NOEXTENDED") {
        definition.sms.extended_format = false;
    } else if statement.contains(" EXTENDED") {
        definition.sms.extended_format = true;
    }
    if statement.contains(" NOCOMPRESS") {
        definition.security.compression = mainframe_env_host_api::CompressionMode::None;
    } else if let Some(compression) = operand(statement, &["COMPRESS"]) {
        definition.security.compression = match compression.as_str() {
            "GENERIC" => mainframe_env_host_api::CompressionMode::Generic,
            "TAILORED" => mainframe_env_host_api::CompressionMode::Tailored,
            _ => return Err(HostProblem::Malformed),
        };
    } else if statement.contains(" COMPRESS") {
        definition.security.compression = mainframe_env_host_api::CompressionMode::Generic;
    }
    definition.security.encryption_key_label =
        operand(statement, &["KEYLABEL"]).or(definition.security.encryption_key_label.take());
    if let Some(value) = numeric_operand(statement, "STRIPECOUNT") {
        definition.vsam.stripe_count = u16::try_from(value).map_err(|_| HostProblem::Malformed)?;
    }

    if let Some(catalog) = operand(statement, &["CATALOG"]) {
        definition.catalog.catalog = Some(dataset_name(&catalog)?);
    }
    definition.catalog.owner = operand(statement, &["OWNER"]).or(definition.catalog.owner.take());
    if let Some(entry_type) = operand(statement, &["ENTRYTYPE"]) {
        definition.catalog.entry_kind = match entry_type.as_str() {
            "DATASET" | "NONVSAM" | "CLUSTER" => mainframe_env_host_api::CatalogEntryKind::Dataset,
            "ALTERNATEINDEX" => mainframe_env_host_api::CatalogEntryKind::AlternateIndex,
            "PATH" => mainframe_env_host_api::CatalogEntryKind::Path,
            "ALIAS" => mainframe_env_host_api::CatalogEntryKind::Alias,
            "GENERATIONDATAGROUP" => mainframe_env_host_api::CatalogEntryKind::GenerationDataGroup,
            "USERCATALOG" => mainframe_env_host_api::CatalogEntryKind::UserCatalog,
            "MASTERCATALOG" => mainframe_env_host_api::CatalogEntryKind::MasterCatalog,
            "LIBRARY" => mainframe_env_host_api::CatalogEntryKind::Library,
            "VOLUME" => mainframe_env_host_api::CatalogEntryKind::Volume,
            "PAGESPACE" => mainframe_env_host_api::CatalogEntryKind::PageSpace,
            _ => return Err(HostProblem::Malformed),
        };
    }
    definition.catalog.creation_date =
        numeric_operand(statement, "CREATEDATE").or(definition.catalog.creation_date);
    let expiration =
        numeric_operand(statement, "TO").or_else(|| numeric_operand(statement, "EXPIRATION"));
    let retention =
        numeric_operand(statement, "FOR").or_else(|| numeric_operand(statement, "RETPD"));
    if expiration.is_some() && retention.is_some() {
        return Err(HostProblem::Malformed);
    }
    if let Some(expiration) = expiration {
        definition.catalog.expiration_date = Some(expiration);
        definition.catalog.retention_days = None;
    } else if let Some(retention) = retention {
        definition.catalog.retention_days =
            Some(u16::try_from(retention).map_err(|_| HostProblem::Malformed)?);
        definition.catalog.expiration_date = None;
    }
    Ok(())
}

fn dataset_name(value: &str) -> Result<DatasetName, HostProblem> {
    DatasetName::new(value.to_ascii_uppercase(), 128).map_err(|_| HostProblem::Malformed)
}

fn ams_target(command: &AmsCommand) -> Result<DatasetName, HostProblem> {
    let value = operand(command.source(), &["INDATASET", "DATASET", "ENTRIES"])
        .or_else(|| crate::ams::bare_target(command.source(), command.label()))
        .ok_or(HostProblem::Malformed)?;
    dataset_name(&value)
}

fn encode_ams_snapshot(
    dataset: &DatasetName,
    snapshot: &mainframe_env_host_api::DatasetSnapshot,
) -> Result<Vec<Vec<u8>>, HostProblem> {
    let manifest = serde_json::to_vec(snapshot).map_err(|_| HostProblem::InfrastructureFailure)?;
    let chunks = manifest.chunks(192).map(<[u8]>::to_vec).collect::<Vec<_>>();
    if chunks.is_empty() || chunks.len() > 4_096 {
        return Err(HostProblem::ResourceExhausted);
    }
    let core = format!("MEAMS2|{}|{}", dataset.as_str(), chunks.len());
    let header = format!(
        "{core}|{}",
        ams_definition_snapshot_digest(&core, &manifest, &[])
    )
    .into_bytes();
    let mut encoded = Vec::with_capacity(
        1usize
            .checked_add(chunks.len())
            .ok_or(HostProblem::ResourceExhausted)?,
    );
    encoded.push(header);
    encoded.extend(chunks);
    Ok(encoded)
}

fn decode_ams_snapshot(
    header: &str,
    records: &mut Vec<Vec<u8>>,
) -> Result<(DatasetName, mainframe_env_host_api::DatasetSnapshot), HostProblem> {
    let fields = header.split('|').collect::<Vec<_>>();
    if fields.len() != 4 || fields[0] != "MEAMS2" {
        return Err(HostProblem::Malformed);
    }
    let chunk_count = fields[2]
        .parse::<usize>()
        .map_err(|_| HostProblem::Malformed)?;
    if chunk_count == 0 || chunk_count > 4_096 || records.len() < chunk_count {
        return Err(HostProblem::Malformed);
    }
    let manifest_chunks = records.drain(..chunk_count).collect::<Vec<_>>();
    let manifest_length = manifest_chunks.iter().try_fold(0usize, |total, chunk| {
        total
            .checked_add(chunk.len())
            .ok_or(HostProblem::ResourceExhausted)
    })?;
    let mut manifest = Vec::with_capacity(manifest_length);
    for chunk in manifest_chunks {
        manifest.extend_from_slice(&chunk);
    }
    if !records.is_empty() {
        return Err(HostProblem::Malformed);
    }
    let core = fields[..3].join("|");
    if fields[3] != ams_definition_snapshot_digest(&core, &manifest, &[]) {
        return Err(HostProblem::IdempotencyConflict);
    }
    let snapshot = serde_json::from_slice(&manifest).map_err(|_| HostProblem::Malformed)?;
    Ok((dataset_name(fields[1])?, snapshot))
}

fn ams_snapshot_digest(core: &str, records: &[Vec<u8>]) -> String {
    let mut digest = Sha256::new();
    digest.update(b"mainframe-env.ams-snapshot@1");
    digest.update((core.len() as u64).to_be_bytes());
    digest.update(core.as_bytes());
    digest.update((records.len() as u64).to_be_bytes());
    for record in records {
        digest.update((record.len() as u64).to_be_bytes());
        digest.update(record);
    }
    format!("sha256:{:x}", digest.finalize())
}

fn ams_definition_snapshot_digest(core: &str, manifest: &[u8], records: &[Vec<u8>]) -> String {
    let mut digest = Sha256::new();
    digest.update(b"mainframe-env.ams-definition-snapshot@2");
    digest.update((core.len() as u64).to_be_bytes());
    digest.update(core.as_bytes());
    digest.update((manifest.len() as u64).to_be_bytes());
    digest.update(manifest);
    digest.update((records.len() as u64).to_be_bytes());
    for record in records {
        digest.update((record.len() as u64).to_be_bytes());
        digest.update(record);
    }
    format!("sha256:{:x}", digest.finalize())
}

fn hex_bytes(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02X}")).collect()
}

fn dataset_resolution_key(dd: &crate::DdPlan, raw: &str) -> String {
    format!(
        "{}({:+})",
        raw.to_ascii_uppercase(),
        dd.generation.unwrap_or_default()
    )
}

fn next_effect_sequence(invocation: &Invocation, sequence: &mut u64) -> Result<u64, HostProblem> {
    *sequence = sequence
        .checked_add(1)
        .ok_or(HostProblem::ResourceExhausted)?;
    if *sequence > invocation.limits.max_effects {
        return Err(HostProblem::ResourceExhausted);
    }
    Ok(*sequence)
}

fn effect_key(job: &Job, step: &StepPlan, sequence: u64) -> Result<IdempotencyKey, HostProblem> {
    IdempotencyKey::new(
        format!("jes:{}:{}:{sequence}", job.id, step.name),
        InvocationLimits::default(),
    )
    .map_err(|_| HostProblem::ResourceExhausted)
}

fn next_spool_sequence(invocation: &Invocation, job: &mut Job) -> Result<u64, HostProblem> {
    job.spool_sequence = job
        .spool_sequence
        .checked_add(1)
        .ok_or(HostProblem::ResourceExhausted)?;
    if job.spool_sequence > invocation.limits.max_effects {
        return Err(HostProblem::ResourceExhausted);
    }
    Ok(job.spool_sequence)
}

fn spool_internal_name(step_name: Option<&str>, file: &str) -> String {
    let candidate = step_name.map_or_else(
        || file.to_ascii_uppercase(),
        |step| {
            format!(
                "{}:{}",
                step.to_ascii_uppercase(),
                file.to_ascii_uppercase()
            )
        },
    );
    if candidate.len() <= 64 {
        candidate
    } else {
        let mut digest = Sha256::new();
        digest.update(b"mainframe-env.jes-spool-file@1");
        digest.update(candidate.as_bytes());
        let encoded = format!("{:x}", digest.finalize());
        format!("F-{}", &encoded[..60])
    }
}

#[derive(Clone)]
struct SpoolRoute {
    class: char,
    destination: String,
    writer: Option<String>,
    forms: Option<String>,
    copies: u16,
    hold: bool,
}

fn spool_route(job: &Job, dd: Option<&crate::DdPlan>) -> SpoolRoute {
    let dd_parameter = |keyword: &str| {
        dd.and_then(|dd| {
            dd.parameters
                .iter()
                .find(|parameter| parameter.identity().keyword() == keyword)
                .map(|parameter| parameter.normalized_value())
        })
    };
    let output = dd_parameter("OUTPUT")
        .map(|name| name.trim_start_matches("*.").to_ascii_uppercase())
        .and_then(|name| {
            job.plan
                .outputs
                .iter()
                .find(|output| output.name.eq_ignore_ascii_case(&name))
        });
    let parameter = |keyword: &str| {
        dd_parameter(keyword).or_else(|| {
            output.and_then(|output| {
                output
                    .parameters
                    .iter()
                    .find(|parameter| parameter.identity().keyword() == keyword)
                    .map(|parameter| parameter.normalized_value())
            })
        })
    };
    let class = dd
        .and_then(|dd| dd.sysout.as_deref())
        .and_then(|value| {
            value
                .trim_matches(['(', ')'])
                .split(',')
                .next()
                .filter(|value| *value != "*")
                .and_then(|value| value.chars().next())
        })
        .or_else(|| parameter("CLASS").and_then(|value| value.chars().next()))
        .unwrap_or(job.class);
    SpoolRoute {
        class,
        destination: parameter("DEST")
            .unwrap_or(job.route.output_node.as_str())
            .to_ascii_uppercase(),
        writer: parameter("WRITER").map(str::to_ascii_uppercase),
        forms: parameter("FORMS").map(str::to_ascii_uppercase),
        copies: parameter("COPIES")
            .and_then(|value| value.trim_matches(['(', ')']).split(',').next())
            .and_then(|value| value.parse().ok())
            .filter(|copies| (1..=255).contains(copies))
            .unwrap_or(1),
        hold: parameter("HOLD").is_some_and(|value| value.eq_ignore_ascii_case("YES"))
            || parameter("OUTDISP").is_some_and(|value| value.eq_ignore_ascii_case("HOLD")),
    }
}

fn spool_descriptor_id(job: &str, file: &str) -> String {
    let mut digest = Sha256::new();
    digest.update(b"mainframe-env.jes-spool-id@1");
    digest.update(job.as_bytes());
    digest.update(file.as_bytes());
    let encoded = format!("{:x}", digest.finalize());
    format!("SP-{}", &encoded[..60])
}

fn spool_resource(job: &Job, file: &str) -> String {
    format!("JOB.{}.{}", job.name, spool_descriptor_id(&job.id, file))
}

fn output_group_id(job: &Job, route: &SpoolRoute) -> String {
    let mut digest = Sha256::new();
    digest.update(b"mainframe-env.jes-output-id@1");
    digest.update(job.id.as_bytes());
    digest.update([route.class as u8]);
    digest.update(route.destination.as_bytes());
    digest.update(route.writer.as_deref().unwrap_or("").as_bytes());
    digest.update(route.forms.as_deref().unwrap_or("").as_bytes());
    let encoded = format!("{:x}", digest.finalize());
    format!("OUT-{}", &encoded[..60])
}

fn validate_output_route(
    destination: &str,
    writer: Option<&str>,
    forms: Option<&str>,
) -> Result<(), HostProblem> {
    for value in [Some(destination), writer, forms].into_iter().flatten() {
        if value.is_empty()
            || value.len() > 128
            || value.chars().any(|character| {
                character.is_control() || character.is_whitespace() || !character.is_ascii()
            })
        {
            return Err(HostProblem::Malformed);
        }
    }
    Ok(())
}

fn invalid_state() -> HostProblem {
    HostProblem::Condition {
        name: "INVALID_STATE".into(),
        response: 409,
        response2: 0,
    }
}

fn validate_jes_name(value: &str) -> Result<(), HostProblem> {
    if value.is_empty()
        || value.len() > 16
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'@' | b'#' | b'$'))
    {
        Err(HostProblem::Malformed)
    } else {
        Ok(())
    }
}

fn snapshot(job: &Job) -> JobSnapshot {
    JobSnapshot {
        id: job.id.clone(),
        name: job.name.clone(),
        owner: job.owner.clone(),
        class: job.class,
        priority: job.priority,
        state: job.state,
        return_code: job.return_code,
        abend_code: job.abend_code.clone(),
        active_step: job.active_step.clone(),
        initiator: job.initiator.clone(),
        steps: job.steps.clone(),
        attempt: job.attempt,
        version: job.version,
        kind: job.kind,
        origin: job.origin.clone(),
        route: job.route.clone(),
    }
}

fn legacy_jes_job_contract() -> String {
    "mainframe-env.jes-durable-job@1".into()
}

fn resolve_plan_programs(
    plan: &JobPlan,
) -> Result<BTreeMap<String, ProgramRegistration>, HostProblem> {
    let mut registrations = BTreeMap::new();
    for step in &plan.steps {
        if registrations
            .insert(
                step.name.clone(),
                resolve_program_registration(&step.program)?,
            )
            .is_some()
        {
            return Err(HostProblem::Malformed);
        }
    }
    Ok(registrations)
}

fn validate_plan_programs(
    plan: &JobPlan,
    registrations: &BTreeMap<String, ProgramRegistration>,
) -> Result<(), HostProblem> {
    if registrations.len() != plan.steps.len() {
        return Err(HostProblem::InfrastructureFailure);
    }
    for step in &plan.steps {
        let registration = registrations
            .get(&step.name)
            .ok_or(HostProblem::InfrastructureFailure)?;
        registration
            .validate()
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        if registration.program != step.program
            || registration
                != &resolve_program_registration(&step.program)
                    .map_err(|_| HostProblem::InfrastructureFailure)?
        {
            return Err(HostProblem::InfrastructureFailure);
        }
    }
    Ok(())
}

fn initial_step_executions(plan: &JobPlan) -> Vec<StepExecution> {
    plan.steps
        .iter()
        .map(|step| StepExecution {
            name: step.name.clone(),
            state: StepState::Pending,
            attempt: 0,
            termination: None,
        })
        .collect()
}

fn job_record(job: &Job) -> Result<ProviderStateRecord, HostProblem> {
    Ok(ProviderStateRecord {
        namespace: "jes-job".into(),
        key: job.id.clone(),
        version: job.version,
        payload: serde_json::to_vec(job).map_err(|_| HostProblem::ProviderFailure)?,
    })
}

fn store_error(error: StoreError) -> HostProblem {
    match error {
        StoreError::Conflict => HostProblem::IdempotencyConflict,
        StoreError::CapacityExceeded | StoreError::PayloadTooLarge => {
            HostProblem::ResourceExhausted
        }
        _ => HostProblem::InfrastructureFailure,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Program, ProgramOutput, ProgramRouter};
    use mainframe_env_dataset::{DatasetLimits, DatasetService, dataset_providers};
    use mainframe_env_execution_api::{
        ArtifactRef, CapabilityId, ExecutionId, Principal, RequestId, ResourceLimits, RunUnitId,
        Selector, ServiceClass, TraceId,
    };
    use mainframe_env_host_api::{
        CapabilityDescriptor, CicsDisposition, CicsResponse, DatasetResult, EffectResult,
        HostLimits, HostProvider, RegistrySnapshot,
    };
    use mainframe_env_spool::{ProviderArtifactStore, SpoolLimits, SpoolService, spool_providers};
    use mainframe_env_store::{MemoryStore, SqliteStateStore};
    use mainframe_env_store_api::{ProviderStateMutation, ProviderStateWrite};
    use std::collections::BTreeSet;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    struct SecurityProvider {
        descriptor: CapabilityDescriptor,
        deny_spool: bool,
        deny_control: bool,
        deny_internal_reader: bool,
    }

    struct FailDatasetCommitOnceStore {
        inner: MemoryStore,
        fail_next_mutation: AtomicBool,
    }

    impl FailDatasetCommitOnceStore {
        fn new() -> Self {
            Self {
                inner: MemoryStore::new(Default::default()),
                fail_next_mutation: AtomicBool::new(false),
            }
        }

        fn arm(&self) {
            self.fail_next_mutation.store(true, Ordering::SeqCst);
        }
    }

    impl ProviderStateStore for FailDatasetCommitOnceStore {
        fn get_provider_state(
            &self,
            namespace: &str,
            key: &str,
        ) -> Result<Option<ProviderStateRecord>, StoreError> {
            self.inner.get_provider_state(namespace, key)
        }

        fn list_provider_state(
            &self,
            namespace: &str,
            max: usize,
        ) -> Result<Vec<ProviderStateRecord>, StoreError> {
            self.inner.list_provider_state(namespace, max)
        }

        fn put_provider_state(
            &self,
            record: ProviderStateRecord,
            expected_version: Option<u64>,
        ) -> Result<(), StoreError> {
            self.inner.put_provider_state(record, expected_version)
        }

        fn delete_provider_state(
            &self,
            namespace: &str,
            key: &str,
            expected_version: u64,
        ) -> Result<(), StoreError> {
            self.inner
                .delete_provider_state(namespace, key, expected_version)
        }

        fn move_provider_state(
            &self,
            record: ProviderStateRecord,
            old_key: &str,
            expected_version: u64,
        ) -> Result<(), StoreError> {
            self.inner
                .move_provider_state(record, old_key, expected_version)
        }

        fn put_provider_states_atomic(
            &self,
            writes: Vec<ProviderStateWrite>,
        ) -> Result<(), StoreError> {
            self.inner.put_provider_states_atomic(writes)
        }

        fn mutate_provider_states_atomic(
            &self,
            mutations: Vec<ProviderStateMutation>,
        ) -> Result<(), StoreError> {
            if self.fail_next_mutation.swap(false, Ordering::SeqCst) {
                Err(StoreError::Infrastructure(
                    "injected-dataset-commit-failure".into(),
                ))
            } else {
                self.inner.mutate_provider_states_atomic(mutations)
            }
        }
    }

    struct DatasetProvider {
        descriptor: CapabilityDescriptor,
        records: Arc<Mutex<BTreeMap<String, Vec<Vec<u8>>>>>,
        generations: Arc<Mutex<BTreeMap<String, Vec<String>>>>,
    }

    struct CicsFileControlProvider {
        descriptor: CapabilityDescriptor,
        controls: Arc<Mutex<Vec<BTreeMap<String, BoundedPayload>>>>,
    }

    impl HostProvider for CicsFileControlProvider {
        fn descriptor(&self) -> &CapabilityDescriptor {
            &self.descriptor
        }

        fn invoke(&self, _: &Invocation, effect: EffectRequest) -> EffectResult {
            let outcome = match effect.request {
                HostRequest::Cics(request) => self
                    .controls
                    .lock()
                    .map_err(|_| HostProblem::InfrastructureFailure)
                    .map(|mut controls| {
                        controls.push(request.arguments);
                        HostResult::Cics(CicsResponse {
                            disposition: CicsDisposition::Complete,
                            condition: "NORMAL".into(),
                            response: 0,
                            response2: 0,
                            applid: "ME01".into(),
                            sysid: "S001".into(),
                            transaction: "DEFAULT".into(),
                            aid: 0,
                            target: None,
                            next_transaction: None,
                            payload: BoundedPayload::new(
                                "mainframe-env.cics.payload@1",
                                Vec::new(),
                                InvocationLimits::default(),
                            )
                            .unwrap(),
                            outputs: BTreeMap::new(),
                            unit_of_work: None,
                        })
                    }),
                _ => Err(HostProblem::Malformed),
            };
            EffectResult {
                sequence: effect.sequence,
                outcome,
            }
        }
    }

    impl HostProvider for DatasetProvider {
        fn descriptor(&self) -> &CapabilityDescriptor {
            &self.descriptor
        }

        fn invoke(&self, _: &Invocation, effect: EffectRequest) -> EffectResult {
            let outcome = match effect.request {
                HostRequest::Dataset(DatasetRequest::Attributes { dataset }) => self
                    .records
                    .lock()
                    .map_err(|_| HostProblem::InfrastructureFailure)
                    .and_then(|records| {
                        records
                            .contains_key(dataset.as_str())
                            .then_some(HostResult::Dataset(DatasetResult::Attributes {
                                attributes: DatasetAttributes {
                                    organization: DatasetOrganization::Sequential,
                                    record_format: RecordFormat::Variable,
                                    logical_record_length: 32_760,
                                    key_offset: None,
                                    key_length: None,
                                    ccsid: Some(37),
                                },
                                version: 1,
                            }))
                            .ok_or(HostProblem::NotFound)
                    }),
                HostRequest::Dataset(DatasetRequest::Read { dataset, .. }) => self
                    .records
                    .lock()
                    .map_err(|_| HostProblem::InfrastructureFailure)
                    .and_then(|records| {
                        records
                            .get(dataset.as_str())
                            .cloned()
                            .map(|records| {
                                HostResult::Dataset(DatasetResult::Records {
                                    identities: vec![Vec::new(); records.len()],
                                    records,
                                    version: 1,
                                })
                            })
                            .ok_or(HostProblem::NotFound)
                    }),
                HostRequest::Dataset(DatasetRequest::ReadConcatenation {
                    datasets, ..
                }) => self
                    .records
                    .lock()
                    .map_err(|_| HostProblem::InfrastructureFailure)
                    .and_then(|state| {
                        let mut combined = Vec::new();
                        for dataset in datasets {
                            combined.extend(
                                state
                                    .get(dataset.as_str())
                                    .cloned()
                                    .ok_or(HostProblem::NotFound)?,
                            );
                        }
                        Ok(HostResult::Dataset(DatasetResult::Records {
                            identities: vec![Vec::new(); combined.len()],
                            records: combined,
                            version: 1,
                        }))
                    }),
                HostRequest::Dataset(DatasetRequest::Write {
                    dataset, records, ..
                }) => self
                    .records
                    .lock()
                    .map_err(|_| HostProblem::InfrastructureFailure)
                    .map(|mut state| {
                        state.insert(dataset.as_str().into(), records);
                        HostResult::Dataset(DatasetResult::Mutated { version: 2 })
                    }),
                HostRequest::Dataset(DatasetRequest::Append {
                    dataset, records, ..
                }) => self
                    .records
                    .lock()
                    .map_err(|_| HostProblem::InfrastructureFailure)
                    .and_then(|mut state| {
                        state
                            .get_mut(dataset.as_str())
                            .ok_or(HostProblem::NotFound)?
                            .extend(records);
                        Ok(HostResult::Dataset(DatasetResult::Mutated {
                            version: 2,
                        }))
                    }),
                HostRequest::Dataset(DatasetRequest::AcquireLock {
                    dataset,
                    target,
                    owner,
                    mode,
                    now_tick,
                    lease_ticks,
                    ..
                }) => Ok(HostResult::Dataset(DatasetResult::Locks {
                    locks: vec![mainframe_env_host_api::DatasetLockReceipt {
                        lock_id: format!("lock-{}-{}", dataset.as_str(), effect.sequence),
                        dataset,
                        target,
                        owner,
                        mode,
                        expires_at: now_tick.saturating_add(lease_ticks),
                        transaction: None,
                        version: 1,
                    }],
                })),
                HostRequest::Dataset(DatasetRequest::ReleaseLock { .. })
                | HostRequest::Dataset(DatasetRequest::SetLifecycle { .. }) => Ok(
                    HostResult::Dataset(DatasetResult::Mutated { version: 2 }),
                ),
                HostRequest::Dataset(
                    DatasetRequest::Create { dataset, .. }
                    | DatasetRequest::Define { dataset, .. },
                ) => self
                    .records
                    .lock()
                    .map_err(|_| HostProblem::InfrastructureFailure)
                    .and_then(|mut state| {
                        if state.contains_key(dataset.as_str()) {
                            return Err(HostProblem::IdempotencyConflict);
                        }
                        state.insert(dataset.as_str().into(), Vec::new());
                        Ok(HostResult::Dataset(DatasetResult::Created { version: 1 }))
                    }),
                HostRequest::Dataset(DatasetRequest::Delete { dataset, .. }) => self
                    .records
                    .lock()
                    .map_err(|_| HostProblem::InfrastructureFailure)
                    .and_then(|mut state| {
                        state
                            .remove(dataset.as_str())
                            .map(|_| HostResult::Dataset(DatasetResult::Mutated { version: 2 }))
                            .ok_or(HostProblem::NotFound)
                    }),
                HostRequest::Dataset(DatasetRequest::DefineGenerationGroup { base, .. }) => self
                    .generations
                    .lock()
                    .map_err(|_| HostProblem::InfrastructureFailure)
                    .map(|mut groups| {
                        groups.entry(base.as_str().into()).or_default();
                        HostResult::Dataset(DatasetResult::Mutated { version: 1 })
                    }),
                HostRequest::Dataset(DatasetRequest::CreateGeneration {
                    base, records, ..
                }) => (|| {
                        let mut groups = self
                            .generations
                            .lock()
                            .map_err(|_| HostProblem::InfrastructureFailure)?;
                        let group = groups.get_mut(base.as_str()).ok_or(HostProblem::NotFound)?;
                        let absolute = u32::try_from(group.len() + 1)
                            .map_err(|_| HostProblem::ResourceExhausted)?;
                        let name = format!("{}.G{absolute:04}V00", base.as_str());
                        group.push(name.clone());
                        self.records
                            .lock()
                            .map_err(|_| HostProblem::InfrastructureFailure)?
                            .insert(name.clone(), records);
                        Ok(HostResult::Dataset(DatasetResult::Generation {
                            dataset: DatasetName::new(name, 128)
                                .map_err(|_| HostProblem::InfrastructureFailure)?,
                            absolute_generation: absolute,
                            version: 1,
                        }))
                    })(),
                HostRequest::Dataset(DatasetRequest::ResolveGeneration { base, relative }) => (|| {
                        let groups = self
                            .generations
                            .lock()
                            .map_err(|_| HostProblem::InfrastructureFailure)?;
                        let group = groups.get(base.as_str()).ok_or(HostProblem::NotFound)?;
                        let index = i32::try_from(group.len())
                            .map_err(|_| HostProblem::ResourceExhausted)?
                            - 1
                            + relative;
                        let name = group
                            .get(usize::try_from(index).map_err(|_| HostProblem::NotFound)?)
                            .ok_or(HostProblem::NotFound)?;
                        Ok(HostResult::Dataset(DatasetResult::Generation {
                            dataset: DatasetName::new(name, 128)
                                .map_err(|_| HostProblem::InfrastructureFailure)?,
                            absolute_generation: u32::try_from(index + 1)
                                .map_err(|_| HostProblem::InfrastructureFailure)?,
                            version: 1,
                        }))
                    })(),
                HostRequest::Dataset(DatasetRequest::DefineAlternateIndex {
                    base,
                    index,
                    key_offset,
                    key_length,
                    allow_duplicates,
                    ..
                }) => self
                    .records
                    .lock()
                    .map_err(|_| HostProblem::InfrastructureFailure)
                    .map(|mut state| {
                        state.insert(
                            index.as_str().into(),
                            vec![format!(
                                "BASE={} OFFSET={key_offset} LENGTH={key_length} DUP={allow_duplicates}",
                                base.as_str()
                            )
                            .into_bytes()],
                        );
                        HostResult::Dataset(DatasetResult::Mutated { version: 1 })
                    }),
                HostRequest::Dataset(DatasetRequest::BuildAlternateIndex { index, .. }) => self
                    .records
                    .lock()
                    .map_err(|_| HostProblem::InfrastructureFailure)
                    .and_then(|state| {
                        state
                            .contains_key(index.as_str())
                            .then_some(HostResult::Dataset(DatasetResult::Mutated { version: 2 }))
                            .ok_or(HostProblem::NotFound)
                    }),
                HostRequest::Dataset(DatasetRequest::DefinePath { path, index, .. }) => self
                    .records
                    .lock()
                    .map_err(|_| HostProblem::InfrastructureFailure)
                    .and_then(|mut state| {
                        let records = state
                            .get(index.as_str())
                            .cloned()
                            .ok_or(HostProblem::NotFound)?;
                        state.insert(path.as_str().into(), records);
                        Ok(HostResult::Dataset(DatasetResult::Mutated { version: 1 }))
                    }),
                HostRequest::Dataset(DatasetRequest::List { pattern, .. }) => self
                    .records
                    .lock()
                    .map_err(|_| HostProblem::InfrastructureFailure)
                    .map(|state| {
                        HostResult::Dataset(DatasetResult::Listed {
                            names: state
                                .keys()
                                .filter(|name| pattern == "**" || name.contains(&pattern))
                                .filter_map(|name| DatasetName::new(name, 128).ok())
                                .collect(),
                            more: false,
                        })
                    }),
                _ => Err(HostProblem::Unsupported),
            };
            EffectResult {
                sequence: effect.sequence,
                outcome,
            }
        }
    }

    impl HostProvider for SecurityProvider {
        fn descriptor(&self) -> &CapabilityDescriptor {
            &self.descriptor
        }

        fn invoke(&self, _: &Invocation, effect: EffectRequest) -> EffectResult {
            let outcome = match effect.request {
                HostRequest::Security(SecurityRequest::Authorize { resource, .. })
                    if self.deny_spool && resource.as_str().contains(".SP-") =>
                {
                    Ok(HostResult::Security(SecurityDecision::Deny))
                }
                HostRequest::Security(SecurityRequest::Authorize { intent, .. })
                    if self.deny_control
                        && matches!(intent, AccessIntent::Control | AccessIntent::Alter) =>
                {
                    Ok(HostResult::Security(SecurityDecision::Deny))
                }
                HostRequest::Security(SecurityRequest::Authorize { resource, .. })
                    if self.deny_internal_reader && resource.as_str().ends_with(".INTRDR") =>
                {
                    Ok(HostResult::Security(SecurityDecision::Deny))
                }
                HostRequest::Security(_) => Ok(HostResult::Security(SecurityDecision::Allow)),
                _ => Err(HostProblem::Malformed),
            };
            EffectResult {
                sequence: effect.sequence,
                outcome,
            }
        }
    }

    fn host_with(
        program: Arc<dyn HostProvider>,
        extra: Vec<Arc<dyn HostProvider>>,
    ) -> Arc<ScopedHostService> {
        host_with_security(program, extra, false, false, false)
    }

    fn host_with_security(
        program: Arc<dyn HostProvider>,
        mut extra: Vec<Arc<dyn HostProvider>>,
        deny_spool: bool,
        deny_control: bool,
        deny_internal_reader: bool,
    ) -> Arc<ScopedHostService> {
        let limits = InvocationLimits::default();
        let security: Arc<dyn HostProvider> = Arc::new(SecurityProvider {
            descriptor: CapabilityDescriptor {
                capability: mainframe_env_execution_api::CapabilityId::new(
                    "host.security.authorize",
                    limits,
                )
                .unwrap(),
                provider_id: "test-security".into(),
                generation: "1".into(),
                request_schema: "security@1".into(),
                result_schema: "decision@1".into(),
                max_request_bytes: 65536,
                max_result_bytes: 65536,
                ready: true,
            },
            deny_spool,
            deny_control,
            deny_internal_reader,
        });
        let mut providers = vec![security, program];
        providers.append(&mut extra);
        Arc::new(ScopedHostService::new(
            Arc::new(RegistrySnapshot::new(1, providers, limits).unwrap()),
            HostLimits::default(),
        ))
    }

    fn invocation() -> Invocation {
        let limits = InvocationLimits::default();
        let grants = [
            "host.security.authorize",
            "host.program.invoke",
            "host.dataset.read",
            "host.dataset.write",
            "host.spool.read",
            "host.spool.write",
            "host.cics.execute",
        ]
        .into_iter()
        .map(|name| CapabilityId::new(name, limits).unwrap())
        .collect::<BTreeSet<_>>();
        Invocation::new(
            RequestId::new("request", limits).unwrap(),
            ExecutionId::new("execution", limits).unwrap(),
            RunUnitId::new("run", limits).unwrap(),
            None,
            Selector::new("jes:submit", limits).unwrap(),
            ArtifactRef::new("jcl", limits).unwrap(),
            Principal::new(PrincipalId::new("IBMUSER", limits).unwrap(), grants, limits).unwrap(),
            ServiceClass::Batch,
            0,
            100,
            TraceId::new("trace", limits).unwrap(),
            IdempotencyKey::new("invocation", limits).unwrap(),
            1,
            ResourceLimits::default(),
            BTreeMap::new(),
            limits,
        )
        .unwrap()
    }

    fn bundle(program: &str) -> JclBundle {
        JclBundle {
            primary: format!("//TESTJOB JOB CLASS=A,PRTY=7\n//STEP1 EXEC PGM={program}\n"),
            ..Default::default()
        }
    }

    fn service(
        store: Arc<dyn ProviderStateStore>,
        program: Arc<dyn HostProvider>,
    ) -> Arc<BatchService> {
        let providers = spool_test_providers(store.clone());
        BatchService::open(
            host_with(program, providers),
            store,
            Default::default(),
            Default::default(),
        )
        .unwrap()
    }

    fn spool_test_providers(store: Arc<dyn ProviderStateStore>) -> Vec<Arc<dyn HostProvider>> {
        let artifacts = ProviderArtifactStore::new(store.clone(), 4 * 1024 * 1024).unwrap();
        let spool = SpoolService::open(store, artifacts, SpoolLimits::default()).unwrap();
        spool_providers(spool, InvocationLimits::default())
    }

    fn builtins() -> Arc<dyn HostProvider> {
        ProgramRouter::with_builtins(InvocationLimits::default())
    }

    fn controller_generation(generation: u64, program: &str) -> BatchControllerGeneration {
        BatchControllerGeneration {
            schema_version: crate::BATCH_CONTROLLER_REGISTRY_CONTRACT.into(),
            application: "RESTART-FIXTURE".into(),
            generation,
            identity: format!("sha256:{generation:064x}"),
            controllers: vec![crate::BatchControllerDefinition {
                name: format!("CONTROLLER-{generation}"),
                selector: BatchControllerSelector::TsoRun {
                    program: program.into(),
                },
                program: crate::BatchControllerProgram {
                    path: format!("program/{program}"),
                    identity: format!("sha256:{:064x}", generation + 100),
                },
                plan: BatchControllerPlan::ProgramCall,
            }],
        }
    }

    fn service_with_datasets(
        records: Arc<Mutex<BTreeMap<String, Vec<Vec<u8>>>>>,
    ) -> Arc<BatchService> {
        let limits = InvocationLimits::default();
        let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
        let generations = Arc::new(Mutex::new(BTreeMap::new()));
        let provider = |capability: &str| {
            Arc::new(DatasetProvider {
                descriptor: CapabilityDescriptor {
                    capability: CapabilityId::new(capability, limits).unwrap(),
                    provider_id: format!("test-{capability}"),
                    generation: "1".into(),
                    request_schema: "dataset@1".into(),
                    result_schema: "dataset-result@1".into(),
                    max_request_bytes: 65536,
                    max_result_bytes: 65536,
                    ready: true,
                },
                records: records.clone(),
                generations: generations.clone(),
            }) as Arc<dyn HostProvider>
        };
        let mut providers = vec![
            provider("host.dataset.read"),
            provider("host.dataset.write"),
        ];
        providers.extend(spool_test_providers(store.clone()));
        BatchService::open(
            host_with(builtins(), providers),
            store,
            Default::default(),
            Default::default(),
        )
        .unwrap()
    }

    fn service_with_real_datasets() -> (Arc<BatchService>, Arc<DatasetService>) {
        let store = Arc::new(MemoryStore::new(Default::default()));
        let dataset_store: Arc<dyn ProviderStateStore> = store.clone();
        let dataset = DatasetService::open(dataset_store, DatasetLimits::default()).unwrap();
        let mut providers = dataset_providers(dataset.clone(), InvocationLimits::default());
        let batch_store: Arc<dyn ProviderStateStore> = store;
        providers.extend(spool_test_providers(batch_store.clone()));
        let batch = BatchService::open(
            host_with(builtins(), providers),
            batch_store,
            Default::default(),
            Default::default(),
        )
        .unwrap();
        (batch, dataset)
    }

    fn dataset_test_mutation(sequence: u64) -> Mutation {
        Mutation {
            sequence,
            idempotency_key: IdempotencyKey::new(
                format!("ams-real-{sequence}"),
                InvocationLimits::default(),
            )
            .unwrap(),
            transaction: None,
        }
    }

    fn seed_real_dataset(
        dataset: &DatasetService,
        name: &str,
        records: Vec<Vec<u8>>,
        sequence: u64,
    ) {
        let name = DatasetName::new(name, 128).unwrap();
        dataset
            .invoke(DatasetRequest::Create {
                dataset: name.clone(),
                attributes: DatasetAttributes {
                    organization: DatasetOrganization::Sequential,
                    record_format: RecordFormat::Variable,
                    logical_record_length: 256,
                    key_offset: None,
                    key_length: None,
                    ccsid: Some(37),
                },
                mutation: dataset_test_mutation(sequence),
            })
            .unwrap();
        if !records.is_empty() {
            dataset
                .invoke(DatasetRequest::Write {
                    dataset: name,
                    member: None,
                    records,
                    expected_version: Some(1),
                    mutation: dataset_test_mutation(sequence + 1),
                })
                .unwrap();
        }
    }

    #[test]
    fn submit_replay_hold_release_run_spool_and_purge() {
        let service = service(Arc::new(MemoryStore::new(Default::default())), builtins());
        let invocation = invocation();
        let key = IdempotencyKey::new("submit-1", InvocationLimits::default()).unwrap();
        let submitted = service
            .submit(&invocation, &bundle("IEFBR14"), &key, false)
            .unwrap();
        assert_eq!(
            service
                .submit(&invocation, &bundle("IEFBR14"), &key, false)
                .unwrap()
                .id,
            submitted.id
        );
        assert_eq!(
            service.hold(&invocation, &submitted.id).unwrap().state,
            JobState::Held
        );
        assert_eq!(
            service.release(&invocation, &submitted.id).unwrap().state,
            JobState::Queued
        );
        let completed = service.run_next(&invocation, false).unwrap().unwrap();
        assert_eq!(completed.state, JobState::Completed);
        assert_eq!(completed.return_code, Some(0));
        assert_eq!(completed.initiator, None);
        assert_eq!(completed.steps.len(), 1);
        assert_eq!(completed.steps[0].state, StepState::Completed);
        assert_eq!(completed.steps[0].attempt, 1);
        assert_eq!(
            completed.steps[0].termination,
            Some(StepTermination::ReturnCode { code: 0 })
        );
        let (records, more) = service
            .spool(&invocation, &submitted.id, "SYSPRINT", 0, 10)
            .unwrap();
        assert_eq!(records, vec![b"IEFBR14".to_vec()]);
        assert!(!more);
        service.purge(&invocation, &submitted.id).unwrap();
        assert_eq!(service.get(&submitted.id), Err(HostProblem::NotFound));
    }

    #[test]
    fn spool_descriptors_preserve_step_ownership_and_output_routing_transitions() {
        let service = service(Arc::new(MemoryStore::new(Default::default())), builtins());
        let invocation = invocation();
        let bundle = JclBundle {
            primary: "//OUTJOB JOB CLASS=A\n//OUT1 OUTPUT CLASS=B,DEST=REMOTE,OUTDISP=HOLD\n//STEP1 EXEC PGM=IEFBR14\n//SYSPRINT DD SYSOUT=*,OUTPUT=*.OUT1\n"
                .into(),
            ..Default::default()
        };
        let conversion = crate::convert_jcl(&bundle, Default::default()).unwrap();
        assert!(conversion.is_valid(), "{:?}", conversion.diagnostics());
        let submitted = service
            .submit(
                &invocation,
                &bundle,
                &IdempotencyKey::new("output-routing", InvocationLimits::default()).unwrap(),
                false,
            )
            .unwrap();
        assert_eq!(
            service.run_next(&invocation, false).unwrap().unwrap().state,
            JobState::Completed
        );
        let routed = {
            let state = service.lock().unwrap();
            let job = state.jobs.get(&submitted.id).unwrap();
            let descriptor = job
                .spool_files
                .values()
                .find(|descriptor| descriptor.dd_name == "SYSPRINT")
                .unwrap();
            assert_eq!(descriptor.step_name.as_deref(), Some("STEP1"));
            assert_eq!(descriptor.state, JesSpoolState::Held);
            job.output_groups
                .values()
                .find(|group| group.class == 'B')
                .cloned()
                .unwrap()
        };
        assert_eq!(routed.destination, "REMOTE");
        assert_eq!(routed.writer, None);
        assert_eq!(routed.forms, None);
        assert_eq!(routed.copies, 1);
        assert_eq!(routed.state, JesOutputState::Held);

        let rerouted = service
            .route_output(
                &invocation,
                &submitted.id,
                &routed.id,
                "NODE2",
                Some("WTR2"),
                Some("BLUE"),
            )
            .unwrap();
        assert_eq!(rerouted.destination, "NODE2");
        assert_eq!(
            service
                .release_output(&invocation, &submitted.id, &routed.id)
                .unwrap()
                .state,
            JesOutputState::Released
        );
        assert_eq!(
            service
                .select_output(&invocation, &submitted.id, &routed.id)
                .unwrap()
                .state,
            JesOutputState::Selected
        );
        assert_eq!(
            service
                .complete_selected_output(&invocation, &submitted.id, &routed.id)
                .unwrap()
                .state,
            JesOutputState::Complete
        );
    }

    #[test]
    fn spool_access_is_denied_before_artifact_mutation() {
        let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
        let providers = spool_test_providers(store.clone());
        let service = BatchService::open(
            host_with_security(builtins(), providers, true, false, false),
            store.clone(),
            Default::default(),
            Default::default(),
        )
        .unwrap();
        assert_eq!(
            service.submit(
                &invocation(),
                &bundle("IEFBR14"),
                &IdempotencyKey::new("denied-spool", InvocationLimits::default()).unwrap(),
                false,
            ),
            Err(HostProblem::Unauthorized)
        );
        assert!(
            store
                .list_provider_state("spool-artifact", 16)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn artifact_backed_spool_survives_service_restart_and_retention_purge() {
        let store = Arc::new(MemoryStore::new(Default::default()));
        let invocation = invocation();
        let first = service(store.clone(), builtins());
        let submitted = first
            .submit(
                &invocation,
                &bundle("IEFBR14"),
                &IdempotencyKey::new("spool-restart", InvocationLimits::default()).unwrap(),
                false,
            )
            .unwrap();
        first.run_next(&invocation, false).unwrap().unwrap();
        drop(first);

        let restarted = service(store.clone(), builtins());
        assert_eq!(
            restarted
                .spool(&invocation, &submitted.id, "SYSPRINT", 0, 8)
                .unwrap()
                .0,
            vec![b"IEFBR14".to_vec()]
        );
        assert_eq!(
            restarted
                .purge_expired(&invocation, BatchLimits::default().spool_retention_ticks, 8)
                .unwrap(),
            vec![submitted.id.clone()]
        );
        assert_eq!(restarted.get(&submitted.id), Err(HostProblem::NotFound));
        assert!(
            store
                .list_provider_state("spool-artifact", 16)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn started_task_identity_and_authorized_stop_are_durable() {
        let store = Arc::new(MemoryStore::new(Default::default()));
        let invocation = invocation();
        let first = service(store.clone(), builtins());
        let started = first
            .start_task(
                &invocation,
                "PAYROLL",
                &JclBundle {
                    primary: "//STCJOB JOB CLASS=A\n//RUN EXEC PGM=IEFBR14\n".into(),
                    ..Default::default()
                },
                &IdempotencyKey::new("start-payroll", InvocationLimits::default()).unwrap(),
            )
            .unwrap();
        assert_eq!(started.kind, JesJobKind::StartedTask);
        assert_eq!(
            started.origin,
            JesSubmissionOrigin::StartedTask {
                task_name: "PAYROLL".into()
            }
        );
        assert_eq!(
            first
                .stop_task(&invocation, "PAYROLL", &started.id)
                .unwrap()
                .state,
            JobState::Cancelled
        );
        drop(first);
        let restarted = service(store, builtins());
        let recovered = restarted.get(&started.id).unwrap();
        assert_eq!(recovered.kind, JesJobKind::StartedTask);
        assert!(matches!(
            recovered.origin,
            JesSubmissionOrigin::StartedTask { ref task_name } if task_name == "PAYROLL"
        ));
    }

    #[test]
    fn nje_route_and_mas_member_ownership_drive_selection_and_survive_restart() {
        let store = Arc::new(MemoryStore::new(Default::default()));
        let invocation = invocation();
        let first = service(store.clone(), builtins());
        let topology = JesTopology {
            schema_version: crate::JES_TOPOLOGY_CONTRACT.into(),
            local_node: "LOCAL".into(),
            nodes: BTreeMap::from([
                (
                    "LOCAL".into(),
                    crate::JesNodeDefinition {
                        name: "LOCAL".into(),
                        connected: true,
                        enabled: true,
                        max_inbound_jobs: 8,
                    },
                ),
                (
                    "REMOTE".into(),
                    crate::JesNodeDefinition {
                        name: "REMOTE".into(),
                        connected: true,
                        enabled: true,
                        max_inbound_jobs: 8,
                    },
                ),
            ]),
            members: BTreeMap::from([
                (
                    "MEMBER1".into(),
                    crate::JesMasMemberDefinition {
                        name: "MEMBER1".into(),
                        node: "LOCAL".into(),
                        enabled: true,
                        max_active: 1,
                    },
                ),
                (
                    "MEMBER2".into(),
                    crate::JesMasMemberDefinition {
                        name: "MEMBER2".into(),
                        node: "REMOTE".into(),
                        enabled: true,
                        max_active: 1,
                    },
                ),
            ]),
        };
        first
            .install_topology(&invocation, topology.clone())
            .unwrap();
        let submitted = first
            .submit(
                &invocation,
                &bundle("IEFBR14"),
                &IdempotencyKey::new("remote-job", InvocationLimits::default()).unwrap(),
                false,
            )
            .unwrap();
        let routed = first
            .route_job(&invocation, &submitted.id, "REMOTE", "REMOTE")
            .unwrap();
        assert_eq!(routed.route.execution_node, "REMOTE");
        assert_eq!(
            first
                .run_next_on_member(&invocation, "MEMBER1", "INIT0001", false)
                .unwrap(),
            None
        );
        let completed = first
            .run_next_on_member(&invocation, "MEMBER2", "INIT0001", false)
            .unwrap()
            .unwrap();
        assert_eq!(completed.state, JobState::Completed);
        assert_eq!(completed.route.owner_member.as_deref(), Some("MEMBER2"));
        drop(first);

        let restarted = service(store, builtins());
        assert_eq!(restarted.topology().unwrap(), topology);
        assert_eq!(
            restarted
                .get(&submitted.id)
                .unwrap()
                .route
                .owner_member
                .as_deref(),
            Some("MEMBER2")
        );
    }

    #[test]
    fn initiator_and_queued_job_controls_are_authorized_and_persisted() {
        let store = Arc::new(MemoryStore::new(Default::default()));
        let invocation = invocation();
        let first = service(store.clone(), builtins());
        let submitted = first
            .submit(
                &invocation,
                &bundle("IEFBR14"),
                &IdempotencyKey::new("controlled-job", InvocationLimits::default()).unwrap(),
                false,
            )
            .unwrap();
        assert_eq!(
            first
                .change_class(&invocation, &submitted.id, 'B')
                .unwrap()
                .class,
            'B'
        );
        assert_eq!(
            first
                .change_priority(&invocation, &submitted.id, 42)
                .unwrap()
                .priority,
            42
        );
        first.stop_initiator(&invocation, "INIT0001").unwrap();
        assert_eq!(first.run_next(&invocation, false).unwrap(), None);
        drop(first);

        let restarted = service(store, builtins());
        assert_eq!(restarted.run_next(&invocation, false).unwrap(), None);
        restarted.start_initiator(&invocation, "INIT0001").unwrap();
        assert_eq!(
            restarted
                .run_next(&invocation, false)
                .unwrap()
                .unwrap()
                .state,
            JobState::Completed
        );
    }

    #[test]
    fn denied_operator_control_publishes_no_job_transition() {
        let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
        let providers = spool_test_providers(store.clone());
        let service = BatchService::open(
            host_with_security(builtins(), providers, false, true, false),
            store,
            Default::default(),
            Default::default(),
        )
        .unwrap();
        let invocation = invocation();
        let submitted = service
            .submit(
                &invocation,
                &bundle("IEFBR14"),
                &IdempotencyKey::new("deny-control", InvocationLimits::default()).unwrap(),
                false,
            )
            .unwrap();
        assert_eq!(
            service.hold(&invocation, &submitted.id),
            Err(HostProblem::Unauthorized)
        );
        assert_eq!(service.get(&submitted.id).unwrap().state, JobState::Queued);
    }

    #[test]
    fn controller_generations_survive_restart_and_rollback_atomically() {
        let store = Arc::new(MemoryStore::new(Default::default()));
        let first = service(store.clone(), builtins());
        first
            .install_controllers(controller_generation(1, "FIRST"))
            .unwrap();
        first
            .install_controllers(controller_generation(2, "SECOND"))
            .unwrap();
        drop(first);

        let restarted = service(store.clone(), builtins());
        assert!(
            restarted
                .resolve_controller(&BatchControllerSelector::tso("SECOND").unwrap())
                .unwrap()
                .is_some()
        );
        assert!(
            restarted
                .resolve_controller(&BatchControllerSelector::tso("FIRST").unwrap())
                .unwrap()
                .is_none()
        );
        restarted
            .rollback_controllers("RESTART-FIXTURE", 1)
            .unwrap();
        drop(restarted);

        let rolled_back = service(store, builtins());
        let resolved = rolled_back
            .resolve_controller(&BatchControllerSelector::tso("FIRST").unwrap())
            .unwrap()
            .unwrap();
        assert_eq!(resolved.program.path, "program/FIRST");
        assert!(
            rolled_back
                .resolve_controller(&BatchControllerSelector::tso("SECOND").unwrap())
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn empty_controller_generation_survives_restart_and_can_roll_back() {
        let store = Arc::new(MemoryStore::new(Default::default()));
        let first = service(store.clone(), builtins());
        first
            .install_controllers(controller_generation(1, "FIRST"))
            .unwrap();
        let mut empty = controller_generation(2, "UNUSED");
        empty.controllers.clear();
        assert_eq!(first.install_controllers(empty).unwrap().controllers, 0);
        drop(first);

        let restarted = service(store.clone(), builtins());
        assert!(
            restarted
                .resolve_controller(&BatchControllerSelector::tso("FIRST").unwrap())
                .unwrap()
                .is_none()
        );
        restarted
            .rollback_controllers("RESTART-FIXTURE", 1)
            .unwrap();
        drop(restarted);

        let rolled_back = service(store, builtins());
        assert!(
            rolled_back
                .resolve_controller(&BatchControllerSelector::tso("FIRST").unwrap())
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn controller_program_substitution_is_rejected_against_durable_mapping() {
        let store = Arc::new(MemoryStore::new(Default::default()));
        let service = service(store.clone(), builtins());
        let generation = controller_generation(1, "SIGNEDPGM");
        let signed = generation.controllers[0].program.clone();
        service.install_controllers(generation).unwrap();
        store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: "batch-program".into(),
                    key: "SIGNEDPGM".into(),
                    version: 1,
                    payload: format!("sha256:{:064x}", 999).into_bytes(),
                },
                None,
            )
            .unwrap();
        assert_eq!(
            service.verify_controller_program(&signed),
            Err(HostProblem::IdempotencyConflict)
        );
        store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: "batch-program".into(),
                    key: "SIGNEDPGM".into(),
                    version: 2,
                    payload: signed.identity.as_bytes().to_vec(),
                },
                Some(1),
            )
            .unwrap();
        assert_eq!(
            service.verify_controller_program(&signed).unwrap(),
            "SIGNEDPGM"
        );
    }

    #[test]
    fn retained_controller_generation_limit_fails_before_clone_and_survives_restart() {
        let store = Arc::new(MemoryStore::new(Default::default()));
        let initial = service(store.clone(), builtins());
        for generation in 1..=64 {
            initial
                .install_controllers(controller_generation(
                    generation,
                    &format!("PROGRAM{generation}"),
                ))
                .unwrap();
        }
        assert_eq!(
            initial.install_controllers(controller_generation(65, "PROGRAM65")),
            Err(HostProblem::ResourceExhausted)
        );
        drop(initial);

        let restarted = service(store, builtins());
        assert!(
            restarted
                .resolve_controller(&BatchControllerSelector::tso("PROGRAM64").unwrap())
                .unwrap()
                .is_some()
        );
        assert!(
            restarted
                .resolve_controller(&BatchControllerSelector::tso("PROGRAM65").unwrap())
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn missing_program_fails_job_and_cancel_is_terminal() {
        let service = service(Arc::new(MemoryStore::new(Default::default())), builtins());
        let invocation = invocation();
        let missing = service
            .submit(
                &invocation,
                &bundle("NOTREAL"),
                &IdempotencyKey::new("missing", InvocationLimits::default()).unwrap(),
                false,
            )
            .unwrap();
        assert_eq!(
            service.run_next(&invocation, false).unwrap().unwrap().state,
            JobState::Failed
        );
        let queued = service
            .submit(
                &invocation,
                &bundle("IEFBR14"),
                &IdempotencyKey::new("cancel", InvocationLimits::default()).unwrap(),
                false,
            )
            .unwrap();
        assert_eq!(
            service.cancel(&invocation, &queued.id).unwrap().state,
            JobState::Cancelled
        );
        assert_eq!(missing.state, JobState::Queued);
    }

    #[test]
    fn reached_external_utilities_fail_with_explicit_dispositions() {
        let service = service(Arc::new(MemoryStore::new(Default::default())), builtins());
        let invocation = invocation();
        for (index, program) in ["FTP", "IKJEFT1B"].into_iter().enumerate() {
            let submitted = service
                .submit(
                    &invocation,
                    &bundle(program),
                    &IdempotencyKey::new(
                        format!("explicit-utility-{index}"),
                        InvocationLimits::default(),
                    )
                    .unwrap(),
                    false,
                )
                .unwrap();
            assert_eq!(
                service.run_next(&invocation, false).unwrap().unwrap().state,
                JobState::Failed
            );
            assert!(
                service
                    .spool(&invocation, &submitted.id, "JESMSGLG", 0, 20)
                    .unwrap()
                    .0
                    .iter()
                    .any(|record| String::from_utf8_lossy(record).contains("Unsupported"))
            );
        }
    }

    #[test]
    fn sdsf_applies_carddemo_cics_file_controls_as_one_typed_effect() {
        let controls = Arc::new(Mutex::new(Vec::new()));
        let limits = InvocationLimits::default();
        let cics: Arc<dyn HostProvider> = Arc::new(CicsFileControlProvider {
            descriptor: CapabilityDescriptor {
                capability: CapabilityId::new("host.cics.execute", limits).unwrap(),
                provider_id: "test-cics-file-control".into(),
                generation: "1".into(),
                request_schema: "cics@1".into(),
                result_schema: "cics-result@1".into(),
                max_request_bytes: 65536,
                max_result_bytes: 65536,
                ready: true,
            },
            controls: controls.clone(),
        });
        let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
        let mut providers = vec![cics];
        providers.extend(spool_test_providers(store.clone()));
        let service = BatchService::open(
            host_with(builtins(), providers),
            store,
            Default::default(),
            Default::default(),
        )
        .unwrap();
        let invocation = invocation();
        let submitted = service
            .submit(
                &invocation,
                &JclBundle {
                    primary: "//CLOSEFIL JOB CLASS=A\n//CLCIFIL EXEC PGM=SDSF\n//ISFOUT DD SYSOUT=*\n//CMDOUT DD SYSOUT=*\n//ISFIN DD *\n /F CICSAWSA,'CEMT SET FIL(TRANSACT ) CLO'\n /F CICSAWSA,'CEMT SET FIL(CCXREF ) CLO'\n /F CICSAWSA,'CEMT SET FIL(ACCTDAT ) CLO'\n /F CICSAWSA,'CEMT SET FIL(CXACAIX ) CLO'\n /F CICSAWSA,'CEMT SET FIL(USRSEC ) CLO'\n/*\n".into(),
                    ..Default::default()
                },
                &IdempotencyKey::new("sdsf-cics-close", limits).unwrap(),
                false,
            )
            .unwrap();
        let completed = service.run_next(&invocation, false).unwrap().unwrap();
        assert_eq!(completed.state, JobState::Completed);
        assert_eq!(completed.return_code, Some(0));
        let controls = controls.lock().unwrap();
        assert_eq!(controls.len(), 1);
        assert_eq!(controls[0].len(), 5);
        assert!(controls[0].values().all(|value| value.bytes() == b"CLOSED"));
        assert!(
            service
                .spool(&invocation, &submitted.id, "CMDOUT", 0, 16)
                .unwrap()
                .0
                .iter()
                .any(|record| record == b"TRANSACT CLOSED")
        );
    }

    #[test]
    fn iebgener_runs_through_jes_and_preserves_inline_record_bytes() {
        let service = service(Arc::new(MemoryStore::new(Default::default())), builtins());
        let invocation = invocation();
        let bundle = JclBundle {
            primary: "//GENJOB JOB CLASS=A\n//GENER EXEC PGM=IEBGENER\n//SYSUT1 DD *\nFIRST\nSECOND\n/*\n//SYSUT2 DD SYSOUT=*\n".into(),
            ..Default::default()
        };
        let submitted = service
            .submit(
                &invocation,
                &bundle,
                &IdempotencyKey::new("generate", InvocationLimits::default()).unwrap(),
                false,
            )
            .unwrap();
        let completed = service.run_next(&invocation, false).unwrap().unwrap();
        assert_eq!(completed.state, JobState::Completed);
        assert_eq!(
            service
                .spool(&invocation, &submitted.id, "SYSPRINT", 0, 10)
                .unwrap()
                .0,
            vec![b"SYSUT2 RECORDS=2".to_vec()]
        );
        assert_eq!(
            service
                .spool(&invocation, &submitted.id, "SYSUT2", 0, 10)
                .unwrap()
                .0,
            vec![b"FIRST".to_vec(), b"SECOND".to_vec()]
        );
    }

    #[test]
    fn iebgener_reads_and_writes_dataset_backed_dds() {
        let records = Arc::new(Mutex::new(BTreeMap::from([
            (
                "IBMUSER.INPUT".into(),
                vec![b"FIRST".to_vec(), b"SECOND".to_vec()],
            ),
            ("IBMUSER.OUTPUT".into(), vec![b"OLD".to_vec()]),
        ])));
        let limits = InvocationLimits::default();
        let service = service_with_datasets(records.clone());
        let invocation = invocation();
        service
            .submit(
                &invocation,
                &JclBundle {
                    primary: "//COPYJOB JOB CLASS=A\n//COPY EXEC PGM=IEBGENER\n//SYSUT1 DD DSN=IBMUSER.INPUT,DISP=SHR\n//SYSUT2 DD DSN=IBMUSER.OUTPUT,DISP=OLD\n".into(),
                    ..Default::default()
                },
                &IdempotencyKey::new("dataset-copy", limits).unwrap(),
                false,
            )
            .unwrap();
        let completed = service.run_next(&invocation, false).unwrap().unwrap();
        assert_eq!(completed.state, JobState::Completed);
        assert_eq!(
            records.lock().unwrap()["IBMUSER.OUTPUT"],
            vec![b"FIRST".to_vec(), b"SECOND".to_vec()]
        );
    }

    #[test]
    fn registered_copy_generate_edit_and_update_utilities_mutate_exact_datasets() {
        let records = Arc::new(Mutex::new(BTreeMap::from([
            (
                "IBMUSER.COPYIN".into(),
                vec![b"ONE".to_vec(), b"TWO".to_vec()],
            ),
            ("IBMUSER.COPYOUT".into(), Vec::new()),
            ("IBMUSER.GENOUT".into(), Vec::new()),
            (
                "IBMUSER.JCLIN".into(),
                vec![
                    b"//JOB1 JOB".to_vec(),
                    b"//S1 EXEC PGM=A".to_vec(),
                    b"//JOB2 JOB".to_vec(),
                    b"//S2 EXEC PGM=B".to_vec(),
                ],
            ),
            ("IBMUSER.JCLOUT".into(), Vec::new()),
            ("IBMUSER.UPD".into(), vec![b"OLD".to_vec()]),
        ])));
        let service = service_with_datasets(records.clone());
        let invocation = invocation();
        service
            .submit(
                &invocation,
                &JclBundle {
                    primary: "//UTILJOB JOB CLASS=A\n//COPY EXEC PGM=IEBCOPY\n//INPUT DD DSN=IBMUSER.COPYIN,DISP=SHR\n//OUTPUT DD DSN=IBMUSER.COPYOUT,DISP=OLD\n//SYSIN DD *\n COPY INDD=INPUT,OUTDD=OUTPUT\n/*\n//GEN EXEC PGM=IEBDG\n//DATA DD DSN=IBMUSER.GENOUT,DISP=OLD\n//SYSIN DD *\n DSD OUTPUT=(DATA)\n FD NAME=FIELD,LENGTH=4\n CREATE QUANTITY=2\n END\n/*\n//EDIT EXEC PGM=IEBEDIT\n//SYSUT1 DD DSN=IBMUSER.JCLIN,DISP=SHR\n//SYSUT2 DD DSN=IBMUSER.JCLOUT,DISP=OLD\n//SYSIN DD *\n EDIT START=JOB2\n/*\n//UPDATE EXEC PGM=IEBUPDTE\n//SYSUT2 DD DSN=IBMUSER.UPD(MEMBER),DISP=OLD\n//SYSIN DD *\n./ REPL NAME=MEMBER\nNEW ONE\nNEW TWO\n./ ENDUP\n/*\n"
                        .into(),
                    ..Default::default()
                },
                &IdempotencyKey::new("real-utility-families", InvocationLimits::default())
                    .unwrap(),
                false,
            )
            .unwrap();
        assert_eq!(
            service.run_next(&invocation, false).unwrap().unwrap().state,
            JobState::Completed
        );
        let records = records.lock().unwrap();
        assert_eq!(
            records["IBMUSER.COPYOUT"],
            [b"ONE".to_vec(), b"TWO".to_vec()]
        );
        assert_eq!(
            records["IBMUSER.GENOUT"],
            [b"0001".to_vec(), b"0002".to_vec()]
        );
        assert_eq!(
            records["IBMUSER.JCLOUT"],
            [b"//JOB2 JOB".to_vec(), b"//S2 EXEC PGM=B".to_vec()]
        );
        assert_eq!(
            records["IBMUSER.UPD"],
            [b"NEW ONE".to_vec(), b"NEW TWO".to_vec()]
        );
    }

    #[test]
    fn dd_concatenation_preserves_declared_dataset_order() {
        let records = Arc::new(Mutex::new(BTreeMap::from([
            ("IBMUSER.INPUT1".into(), vec![b"FIRST".to_vec()]),
            ("IBMUSER.INPUT2".into(), vec![b"SECOND".to_vec()]),
            ("IBMUSER.OUTPUT".into(), Vec::new()),
        ])));
        let service = service_with_datasets(records.clone());
        let invocation = invocation();
        service
            .submit(
                &invocation,
                &JclBundle {
                    primary: "//CONCAT JOB CLASS=A\n//COPY EXEC PGM=IEBGENER\n//SYSUT1 DD DSN=IBMUSER.INPUT1,DISP=SHR\n// DD DSN=IBMUSER.INPUT2,DISP=SHR\n//SYSUT2 DD DSN=IBMUSER.OUTPUT,DISP=OLD\n"
                        .into(),
                    ..Default::default()
                },
                &IdempotencyKey::new("dataset-concatenation", InvocationLimits::default())
                    .unwrap(),
                false,
            )
            .unwrap();
        assert_eq!(
            service.run_next(&invocation, false).unwrap().unwrap().state,
            JobState::Completed
        );
        assert_eq!(
            records.lock().unwrap()["IBMUSER.OUTPUT"],
            vec![b"FIRST".to_vec(), b"SECOND".to_vec()]
        );
    }

    #[test]
    fn modify_disposition_appends_instead_of_replacing() {
        let records = Arc::new(Mutex::new(BTreeMap::from([(
            "IBMUSER.OUTPUT".into(),
            vec![b"OLD".to_vec()],
        )])));
        let service = service_with_datasets(records.clone());
        let invocation = invocation();
        service
            .submit(
                &invocation,
                &JclBundle {
                    primary: "//MODOUT JOB CLASS=A\n//COPY EXEC PGM=IEBGENER\n//SYSUT1 DD *\nNEW\n/*\n//SYSUT2 DD DSN=IBMUSER.OUTPUT,DISP=MOD\n"
                        .into(),
                    ..Default::default()
                },
                &IdempotencyKey::new("dataset-mod-append", InvocationLimits::default()).unwrap(),
                false,
            )
            .unwrap();
        assert_eq!(
            service.run_next(&invocation, false).unwrap().unwrap().state,
            JobState::Completed
        );
        assert_eq!(
            records.lock().unwrap()["IBMUSER.OUTPUT"],
            vec![b"OLD".to_vec(), b"NEW".to_vec()]
        );
    }

    #[test]
    fn dummy_dd_is_an_empty_typed_source() {
        let service = service(Arc::new(MemoryStore::new(Default::default())), builtins());
        let invocation = invocation();
        let submitted = service
            .submit(
                &invocation,
                &JclBundle {
                    primary: "//DUMMY JOB CLASS=A\n//COPY EXEC PGM=IEBGENER\n//SYSUT1 DD DUMMY\n//SYSUT2 DD SYSOUT=*\n"
                        .into(),
                    ..Default::default()
                },
                &IdempotencyKey::new("dummy-dd", InvocationLimits::default()).unwrap(),
                false,
            )
            .unwrap();
        assert_eq!(
            service.run_next(&invocation, false).unwrap().unwrap().state,
            JobState::Completed
        );
        assert_eq!(
            service.spool(&invocation, &submitted.id, "SYSUT2", 0, 8),
            Err(HostProblem::NotFound)
        );
    }

    #[test]
    fn iebgener_internal_reader_submits_the_copied_jcl() {
        let records = Arc::new(Mutex::new(BTreeMap::from([(
            "IBMUSER.JCL".into(),
            vec![
                b"//CHILD JOB CLASS=A".to_vec(),
                b"//RUN EXEC PGM=IEFBR14".to_vec(),
            ],
        )])));
        let service = service_with_datasets(records);
        let invocation = invocation();
        let parent = service
            .submit(
                &invocation,
                &JclBundle {
                    primary: "//PARENT JOB CLASS=A\n//SUBMIT EXEC PGM=IEBGENER\n//SYSUT1 DD DSN=IBMUSER.JCL(CHILD),DISP=SHR\n//SYSUT2 DD SYSOUT=(A,INTRDR)\n".into(),
                    ..Default::default()
                },
                &IdempotencyKey::new("internal-reader", InvocationLimits::default()).unwrap(),
                false,
            )
            .unwrap();
        assert_eq!(
            service.run_next(&invocation, false).unwrap().unwrap().state,
            JobState::Completed
        );
        let (jobs, more) = service
            .list(Some(invocation.principal.id()), None, 10)
            .unwrap();
        assert!(!more);
        assert_eq!(jobs.len(), 2);
        let child = jobs.iter().find(|job| job.name == "CHILD").unwrap();
        assert_eq!(child.state, JobState::Queued);
        assert_eq!(
            child.origin,
            JesSubmissionOrigin::InternalReader {
                parent_job_id: parent.id,
                step_name: "SUBMIT".into(),
            }
        );
        let drained = service.drain_queued(&invocation).unwrap();
        assert_eq!(drained.len(), 1);
        assert_eq!(drained[0].name, "CHILD");
        assert_eq!(drained[0].state, JobState::Completed);
    }

    #[test]
    fn denied_internal_reader_submission_creates_no_child_job() {
        let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
        let providers = spool_test_providers(store.clone());
        let service = BatchService::open(
            host_with_security(builtins(), providers, false, false, true),
            store,
            Default::default(),
            Default::default(),
        )
        .unwrap();
        let invocation = invocation();
        service
            .submit(
                &invocation,
                &JclBundle {
                    primary: "//PARENT JOB CLASS=A\n//SUBMIT EXEC PGM=IEBGENER\n//SYSUT1 DD DATA,DLM=@@\n//CHILD JOB CLASS=A\n//RUN EXEC PGM=IEFBR14\n@@\n//SYSUT2 DD SYSOUT=(A,INTRDR)\n"
                        .into(),
                    ..Default::default()
                },
                &IdempotencyKey::new("denied-internal-reader", InvocationLimits::default())
                    .unwrap(),
                false,
            )
            .unwrap();
        assert_eq!(
            service.run_next(&invocation, false).unwrap().unwrap().state,
            JobState::Failed
        );
        assert_eq!(
            service
                .list(Some(invocation.principal.id()), None, 8)
                .unwrap()
                .0
                .len(),
            1
        );
    }

    #[test]
    fn temporary_dataset_pass_and_normal_delete_follow_disp_positions() {
        let records = Arc::new(Mutex::new(BTreeMap::new()));
        let service = service_with_datasets(records.clone());
        let invocation = invocation();
        service
            .submit(
                &invocation,
                &JclBundle {
                    primary: "//TEMPJOB JOB CLASS=A\n//MAKE EXEC PGM=IEFBR14\n//WORK DD DSN=&&WORK,DISP=(NEW,PASS,DELETE)\n//USE EXEC PGM=IEFBR14\n//INPUT DD DSN=&&WORK,DISP=(OLD,DELETE,DELETE)\n".into(),
                    ..Default::default()
                },
                &IdempotencyKey::new("temporary-lifecycle", InvocationLimits::default()).unwrap(),
                false,
            )
            .unwrap();
        assert_eq!(
            service.run_next(&invocation, false).unwrap().unwrap().state,
            JobState::Completed
        );
        assert!(records.lock().unwrap().is_empty());
    }

    #[test]
    fn passed_temporary_dataset_is_removed_at_job_terminal_cleanup() {
        let records = Arc::new(Mutex::new(BTreeMap::new()));
        let service = service_with_datasets(records.clone());
        let invocation = invocation();
        service
            .submit(
                &invocation,
                &JclBundle {
                    primary: "//TCLEAN JOB CLASS=A\n//MAKE EXEC PGM=IEFBR14\n//WORK DD DSN=&&WORK,DISP=(NEW,PASS,DELETE)\n"
                        .into(),
                    ..Default::default()
                },
                &IdempotencyKey::new("temporary-terminal-cleanup", InvocationLimits::default())
                    .unwrap(),
                false,
            )
            .unwrap();
        assert_eq!(
            service.run_next(&invocation, false).unwrap().unwrap().state,
            JobState::Completed
        );
        assert!(records.lock().unwrap().is_empty());
    }

    #[test]
    fn catalog_and_uncatalog_dispositions_use_dataset_lifecycle_authority() {
        let (service, dataset) = service_with_real_datasets();
        let invocation = invocation();
        service
            .submit(
                &invocation,
                &JclBundle {
                    primary: "//CATJOB JOB CLASS=A\n//MAKE EXEC PGM=IEFBR14\n//DATA DD DSN=USER.DISP,DISP=(NEW,CATLG,DELETE)\n"
                        .into(),
                    ..Default::default()
                },
                &IdempotencyKey::new("catalog-disposition", InvocationLimits::default()).unwrap(),
                false,
            )
            .unwrap();
        assert_eq!(
            service.run_next(&invocation, false).unwrap().unwrap().state,
            JobState::Completed
        );
        assert!(matches!(
            dataset.invoke(DatasetRequest::Describe {
                dataset: DatasetName::new("USER.DISP", 128).unwrap(),
            }),
            Ok(DatasetResult::Description(ref description))
                if description.definition.lifecycle.state == DatasetLifecycleState::Cataloged
        ));
        service
            .submit(
                &invocation,
                &JclBundle {
                    primary: "//UNCATJOB JOB CLASS=A\n//USE EXEC PGM=IEFBR14\n//DATA DD DSN=USER.DISP,DISP=(OLD,UNCATLG,KEEP)\n"
                        .into(),
                    ..Default::default()
                },
                &IdempotencyKey::new("uncatalog-disposition", InvocationLimits::default())
                    .unwrap(),
                false,
            )
            .unwrap();
        assert_eq!(
            service.run_next(&invocation, false).unwrap().unwrap().state,
            JobState::Completed
        );
        assert!(matches!(
            dataset.invoke(DatasetRequest::Describe {
                dataset: DatasetName::new("USER.DISP", 128).unwrap(),
            }),
            Ok(DatasetResult::Description(ref description))
                if description.definition.lifecycle.state == DatasetLifecycleState::Allocated
        ));
        assert!(matches!(
            dataset.invoke(DatasetRequest::ListCatalog {
                pattern: "USER.DISP".into(),
                start: None,
                max_items: 8,
            }),
            Ok(DatasetResult::CatalogEntries { entries, more: false }) if entries.is_empty()
        ));
    }

    #[test]
    fn dd_lock_conflict_fails_the_job_without_running_the_program() {
        let (service, dataset) = service_with_real_datasets();
        seed_real_dataset(&dataset, "USER.LOCKED", Vec::new(), 800);
        let owner = PrincipalId::new("IBMUSER", InvocationLimits::default()).unwrap();
        let DatasetResult::Locks { locks } = dataset
            .invoke(DatasetRequest::AcquireLock {
                dataset: DatasetName::new("USER.LOCKED", 128).unwrap(),
                target: DatasetLockTarget::Dataset,
                owner: owner.clone(),
                mode: DatasetLockMode::Exclusive,
                now_tick: 1,
                lease_ticks: 99,
                transaction: Some("OTHERJOB".into()),
                mutation: dataset_test_mutation(801),
            })
            .unwrap()
        else {
            panic!("expected allocation lock");
        };
        let invocation = invocation();
        service
            .submit(
                &invocation,
                &JclBundle {
                    primary: "//LOCKJOB JOB CLASS=A\n//STEP EXEC PGM=IEFBR14\n//DATA DD DSN=USER.LOCKED,DISP=SHR\n"
                        .into(),
                    ..Default::default()
                },
                &IdempotencyKey::new("dd-lock-conflict", InvocationLimits::default()).unwrap(),
                false,
            )
            .unwrap();
        assert_eq!(
            service.run_next(&invocation, false).unwrap().unwrap().state,
            JobState::Failed
        );
        dataset
            .invoke(DatasetRequest::ReleaseLock {
                dataset: DatasetName::new("USER.LOCKED", 128).unwrap(),
                lock_id: locks[0].lock_id.clone(),
                owner,
                mutation: dataset_test_mutation(802),
            })
            .unwrap();
    }

    #[test]
    fn partial_dd_allocation_failure_rolls_back_created_datasets() {
        let records = Arc::new(Mutex::new(BTreeMap::new()));
        let service = service_with_datasets(records.clone());
        let invocation = invocation();
        service
            .submit(
                &invocation,
                &JclBundle {
                    primary: "//PARTIAL JOB CLASS=A\n//STEP EXEC PGM=IEFBR14\n//ONE DD DSN=USER.PARTIAL,DISP=NEW\n//TWO DD DSN=USER.PARTIAL,DISP=NEW\n"
                        .into(),
                    ..Default::default()
                },
                &IdempotencyKey::new("partial-dd-allocation", InvocationLimits::default()).unwrap(),
                false,
            )
            .unwrap();
        assert_eq!(
            service.run_next(&invocation, false).unwrap().unwrap().state,
            JobState::Failed
        );
        assert!(records.lock().unwrap().is_empty());
    }

    #[test]
    fn abnormal_step_rolls_back_new_delete_allocation() {
        let records = Arc::new(Mutex::new(BTreeMap::new()));
        let service = service_with_datasets(records.clone());
        let invocation = invocation();
        service
            .submit(
                &invocation,
                &JclBundle {
                    primary: "//ROLLJOB JOB CLASS=A\n//FAIL EXEC PGM=NOTREAL\n//WORK DD DSN=IBMUSER.ROLLBACK,DISP=(NEW,KEEP,DELETE)\n".into(),
                    ..Default::default()
                },
                &IdempotencyKey::new("abnormal-rollback", InvocationLimits::default()).unwrap(),
                false,
            )
            .unwrap();
        assert_eq!(
            service.run_next(&invocation, false).unwrap().unwrap().state,
            JobState::Failed
        );
        assert!(!records.lock().unwrap().contains_key("IBMUSER.ROLLBACK"));
    }

    #[test]
    fn modify_disposition_allocates_a_missing_dataset_before_delete() {
        let records = Arc::new(Mutex::new(BTreeMap::new()));
        let service = service_with_datasets(records.clone());
        let invocation = invocation();
        service
            .submit(
                &invocation,
                &JclBundle {
                    primary: "//MODJOB JOB CLASS=A\n//PREDEL EXEC PGM=IEFBR14\n//DD1 DD DSN=IBMUSER.MISSING,DISP=(MOD,DELETE,DELETE)\n".into(),
                    ..Default::default()
                },
                &IdempotencyKey::new("mod-create-delete", InvocationLimits::default()).unwrap(),
                false,
            )
            .unwrap();
        let completed = service.run_next(&invocation, false).unwrap().unwrap();
        assert_eq!(completed.state, JobState::Completed);
        assert_eq!(completed.return_code, Some(0));
        assert!(!records.lock().unwrap().contains_key("IBMUSER.MISSING"));
    }

    #[test]
    fn idcams_delete_define_and_repro_mutate_exact_dataset_state() {
        let records = Arc::new(Mutex::new(BTreeMap::from([
            (
                "IBMUSER.INPUT".into(),
                vec![b"FIRST".to_vec(), b"SECOND".to_vec()],
            ),
            ("IBMUSER.TARGET".into(), vec![b"STALE".to_vec()]),
        ])));
        let service = service_with_datasets(records.clone());
        let invocation = invocation();
        service
            .submit(
                &invocation,
                &JclBundle {
                    primary: "//AMSJOB JOB CLASS=A\n//AMS EXEC PGM=IDCAMS\n//INPUT DD DSN=IBMUSER.INPUT,DISP=SHR\n//OUTPUT DD DSN=IBMUSER.TARGET,DISP=OLD\n//SYSIN DD *\n DELETE IBMUSER.TARGET\n IF MAXCC LE 08 THEN SET MAXCC = 0\n DEFINE CLUSTER (NAME(IBMUSER.TARGET) NONINDEXED RECORDSIZE(6 6))\n REPRO INFILE(INPUT) OUTFILE(OUTPUT) SKIP(1) COUNT(1)\n/*\n".into(),
                    ..Default::default()
                },
                &IdempotencyKey::new("idcams-mutations", InvocationLimits::default()).unwrap(),
                false,
            )
            .unwrap();
        assert_eq!(
            service.run_next(&invocation, false).unwrap().unwrap().state,
            JobState::Completed
        );
        assert_eq!(
            records.lock().unwrap()["IBMUSER.TARGET"],
            vec![b"SECOND".to_vec()]
        );
    }

    #[test]
    fn generated_idcams_handlers_route_metadata_catalog_and_modal_commands() {
        let (service, dataset) = service_with_real_datasets();
        seed_real_dataset(&dataset, "USER.COLLECT", Vec::new(), 500);
        let invocation = invocation();
        let submitted = service
            .submit(
                &invocation,
                &JclBundle {
                    primary: "//AMSJOB JOB CLASS=A\n//AMS EXEC PGM=IDCAMS\n//OUT DD DSN=USER.COLLECT,DISP=OLD\n//SYSIN DD *\n ALLOCATE DATASET(USER.PS) RECORDSIZE(80 80) SPACE(TRACKS(2 1)) CONTIG ROUND BUFNO(3) BUFSIZE(128) DATACLAS(STD) MGMTCLAS(ACT) STORCLAS(ABS) EXTENDED EXTENDEDADDRESSABLE VOLUMES(VOLA VOLB) UNIT(2) OWNER(IBMUSER) CREATEDATE(2026001) RETPD(30)\n ALTER USER.PS OPEN\n ALTER USER.PS BUFNO(4)\n DEFINE CLUSTER (NAME(USER.KSDS) INDEXED KEYS(2 0) RECORDSIZE(4 4))\n DEFINE NONVSAM (NAME(USER.NV) RECORDSIZE(16 32))\n DEFINE USERCATALOG (NAME(USER.CAT))\n DEFINE ALIAS (NAME(USER.CATALIAS) RELATE(USER.CAT))\n DIAGNOSE USER.PS\n EXAMINE USER.KSDS\n LISTDATA USER.PS\n PRINT INDATASET(USER.PS)\n VERIFY USER.PS\n LISTCAT\n DCOLLECT OUTFILE(OUT)\n/*\n".into(),
                    ..Default::default()
                },
                &IdempotencyKey::new("ams-generated-handlers", InvocationLimits::default())
                    .unwrap(),
                false,
            )
            .unwrap();
        assert_eq!(
            service.run_next(&invocation, false).unwrap().unwrap().state,
            JobState::Completed
        );
        assert!(matches!(
            dataset.invoke(DatasetRequest::Describe {
                dataset: DatasetName::new("USER.PS", 128).unwrap(),
            }),
            Ok(DatasetResult::Description(ref description))
                if description.definition.lifecycle.state
                    == mainframe_env_host_api::DatasetLifecycleState::Open
                    && description.allocated_bytes == 849_960
                    && description.extents.len() == 1
                    && description.extents[0].volume_id == "VOLA"
                    && description.buffer_bytes == 512
                    && description.max_rba == u64::MAX
                    && description.definition.catalog.owner.as_deref() == Some("IBMUSER")
                    && description.definition.catalog.creation_date == Some(2_026_001)
                    && description.definition.catalog.retention_days == Some(30)
                    && description.definition.sms.data_class.as_deref() == Some("STD")
                    && description.definition.sms.management_class.as_deref() == Some("ACT")
                    && description.definition.sms.storage_class.as_deref() == Some("ABS")
                    && description.definition.volumes.unit_count == 2
        ));
        assert!(matches!(
            dataset.invoke(DatasetRequest::List {
                pattern: "USER.CATALIAS".into(),
                start: None,
                max_items: 8,
            }),
            Ok(DatasetResult::Listed { names, more: false })
                if names.iter().any(|name| name.as_str() == "USER.CATALIAS")
        ));
        assert!(matches!(
            dataset.invoke(DatasetRequest::Read {
                dataset: DatasetName::new("USER.COLLECT", 128).unwrap(),
                member: None,
                key: None,
                max_records: 64,
                control: Default::default(),
            }),
            Ok(DatasetResult::Records { records, .. })
                if records.iter().any(|record| {
                    let record = String::from_utf8_lossy(record);
                    record.starts_with("USER.PS|")
                        && record.contains("VOLUMES=VOLA,VOLB")
                        && record.contains("EXTENTS=0:0:0:849960:VOLA")
                        && record.contains("LIFECYCLE=Open")
                }) && records.iter().any(|record| record.starts_with(b"VOLUME|VOLA|"))
        ));
        assert!(
            service
                .spool(&invocation, &submitted.id, "SYSPRINT", 0, 64)
                .unwrap()
                .0
                .iter()
                .any(|record| String::from_utf8_lossy(record).contains("LISTDATA"))
        );
        let syprint = service
            .spool(&invocation, &submitted.id, "SYSPRINT", 0, 64)
            .unwrap()
            .0;
        assert!(syprint.iter().any(|record| {
            let record = String::from_utf8_lossy(record);
            record.contains("USER.CAT") && record.contains("UserCatalog")
        }));
        assert!(syprint.iter().any(|record| {
            let record = String::from_utf8_lossy(record);
            record.contains("USER.CATALIAS") && record.contains("Alias")
        }));
    }

    #[test]
    fn idcams_shcds_reconciles_unknown_tvs_commit_and_survives_restart() {
        let store = Arc::new(FailDatasetCommitOnceStore::new());
        let provider_store: Arc<dyn ProviderStateStore> = store.clone();
        let dataset = DatasetService::open(provider_store, DatasetLimits::default()).unwrap();
        let name = DatasetName::new("USER.TVSCMD", 128).unwrap();
        let mut definition =
            mainframe_env_host_api::DatasetDefinition::compatibility(DatasetAttributes {
                organization: DatasetOrganization::KeySequenced,
                record_format: RecordFormat::Fixed,
                logical_record_length: 4,
                key_offset: Some(0),
                key_length: Some(2),
                ccsid: Some(37),
            });
        definition.vsam.access_mode = mainframe_env_host_api::VsamAccessMode::Tvs;
        dataset
            .invoke(DatasetRequest::Define {
                dataset: name.clone(),
                definition: Box::new(definition),
                mutation: dataset_test_mutation(550),
            })
            .unwrap();
        let owner = PrincipalId::new("IBMUSER", InvocationLimits::default()).unwrap();
        dataset
            .invoke(DatasetRequest::BeginTvs {
                transaction: "TX-AMS".into(),
                owner: owner.clone(),
                mutation: dataset_test_mutation(551),
            })
            .unwrap();
        dataset
            .invoke(DatasetRequest::StageTvs {
                transaction: "TX-AMS".into(),
                owner: owner.clone(),
                operation: mainframe_env_host_api::TvsRecordOperation::Insert {
                    dataset: name.clone(),
                    record: b"AA11".to_vec(),
                },
                mutation: dataset_test_mutation(552),
            })
            .unwrap();
        store.arm();
        assert_eq!(
            dataset.invoke(DatasetRequest::CompleteTvs {
                transaction: "TX-AMS".into(),
                owner: owner.clone(),
                commit: true,
                mutation: dataset_test_mutation(553),
            }),
            Err(HostProblem::UnknownOutcome)
        );
        assert!(matches!(
            dataset.invoke(DatasetRequest::TvsStatus {
                transaction: "TX-AMS".into(),
                owner: owner.clone(),
            }),
            Ok(DatasetResult::Tvs(
                mainframe_env_host_api::TvsUnitOfWorkReceipt {
                    state: mainframe_env_host_api::TvsUnitOfWorkState::Unknown,
                    ..
                }
            ))
        ));

        let mut providers = dataset_providers(dataset.clone(), InvocationLimits::default());
        let batch_store: Arc<dyn ProviderStateStore> = store.clone();
        providers.extend(spool_test_providers(batch_store.clone()));
        let batch = BatchService::open(
            host_with(builtins(), providers),
            batch_store,
            Default::default(),
            Default::default(),
        )
        .unwrap();
        let invocation = invocation();
        let job = batch
            .submit(
                &invocation,
                &JclBundle {
                    primary: "//SHCJOB JOB CLASS=A\n//AMS EXEC PGM=IDCAMS\n//SYSIN DD *\n SHCDS TRANSACTION(TX-AMS) COMMIT\n/*\n".into(),
                    ..Default::default()
                },
                &IdempotencyKey::new("ams-shcds-reconcile", InvocationLimits::default()).unwrap(),
                false,
            )
            .unwrap();
        assert_eq!(
            batch
                .run_next(&invocation, false)
                .unwrap()
                .unwrap()
                .return_code,
            Some(0)
        );
        assert!(
            batch
                .spool(&invocation, &job.id, "SYSPRINT", 0, 8)
                .unwrap()
                .0
                .iter()
                .any(|record| String::from_utf8_lossy(record).contains("Committed"))
        );
        drop(batch);
        drop(dataset);
        let reopened_store: Arc<dyn ProviderStateStore> = store;
        let reopened = DatasetService::open(reopened_store, DatasetLimits::default()).unwrap();
        assert!(matches!(
            reopened.invoke(DatasetRequest::TvsStatus {
                transaction: "TX-AMS".into(),
                owner,
            }),
            Ok(DatasetResult::Tvs(
                mainframe_env_host_api::TvsUnitOfWorkReceipt {
                    state: mainframe_env_host_api::TvsUnitOfWorkState::Committed,
                    ..
                }
            ))
        ));
        assert!(matches!(
            reopened.invoke(DatasetRequest::Read {
                dataset: name,
                member: None,
                key: Some(b"AA".to_vec()),
                max_records: 1,
                control: Default::default(),
            }),
            Ok(DatasetResult::Records { records, version: 2, .. })
                if records == [b"AA11".to_vec()]
        ));
    }

    #[test]
    fn idcams_verify_reconciles_recovery_required_state() {
        let (service, dataset) = service_with_real_datasets();
        seed_real_dataset(&dataset, "USER.VERIFY", vec![b"DATA".to_vec()], 560);
        let name = DatasetName::new("USER.VERIFY", 128).unwrap();
        dataset
            .invoke(DatasetRequest::SetLifecycle {
                dataset: name.clone(),
                state: mainframe_env_host_api::DatasetLifecycleState::Open,
                expected_version: Some(2),
                mutation: dataset_test_mutation(562),
            })
            .unwrap();
        dataset
            .invoke(DatasetRequest::SetLifecycle {
                dataset: name.clone(),
                state: mainframe_env_host_api::DatasetLifecycleState::RecoveryRequired,
                expected_version: Some(3),
                mutation: dataset_test_mutation(563),
            })
            .unwrap();
        let invocation = invocation();
        service
            .submit(
                &invocation,
                &JclBundle {
                    primary: "//VERJOB JOB CLASS=A\n//AMS EXEC PGM=IDCAMS\n//SYSIN DD *\n VERIFY USER.VERIFY\n/*\n".into(),
                    ..Default::default()
                },
                &IdempotencyKey::new("ams-verify-recovery", InvocationLimits::default()).unwrap(),
                false,
            )
            .unwrap();
        assert_eq!(
            service
                .run_next(&invocation, false)
                .unwrap()
                .unwrap()
                .return_code,
            Some(0)
        );
        assert!(matches!(
            dataset.invoke(DatasetRequest::Describe { dataset: name }),
            Ok(DatasetResult::Description(description))
                if description.version == 5
                    && description.definition.lifecycle.state
                        == mainframe_env_host_api::DatasetLifecycleState::Closed
        ));
    }

    #[test]
    fn ams_expiration_forms_are_unambiguous_and_replace_each_other() {
        let mut definition =
            mainframe_env_host_api::DatasetDefinition::compatibility(DatasetAttributes {
                organization: DatasetOrganization::Sequential,
                record_format: RecordFormat::Fixed,
                logical_record_length: 80,
                key_offset: None,
                key_length: None,
                ccsid: Some(37),
            });
        apply_ams_definition_operands("ALTER USER.A FOR(30)", &mut definition).unwrap();
        assert_eq!(definition.catalog.retention_days, Some(30));
        assert_eq!(definition.catalog.expiration_date, None);
        apply_ams_definition_operands("ALTER USER.A TO(2026100)", &mut definition).unwrap();
        assert_eq!(definition.catalog.retention_days, None);
        assert_eq!(definition.catalog.expiration_date, Some(2_026_100));
        assert_eq!(
            apply_ams_definition_operands("ALTER USER.A TO(2026200) RETPD(10)", &mut definition,),
            Err(HostProblem::Malformed)
        );
    }

    #[test]
    fn ams_dcb_access_and_capability_operands_populate_typed_definition() {
        let mut definition =
            mainframe_env_host_api::DatasetDefinition::compatibility(DatasetAttributes {
                organization: DatasetOrganization::KeySequenced,
                record_format: RecordFormat::Variable,
                logical_record_length: 80,
                key_offset: Some(0),
                key_length: Some(2),
                ccsid: Some(37),
            });
        apply_ams_definition_operands(
            "ALTER USER.A RECFM(VBS) LRECL(512) BLKSIZE(1024) KEYLEN(4) KEYOFF(8) CCSID(1047) \
             BUFNO(7) BUFSIZE(2048) BUFFERING(LSR) CONTROLINTERVALSIZE(4096) \
             CONTROLAREASIZE(65536) SHAREOPTIONS(2 3) RLS REUSE SPEED WRITECHECK ERASE \
             ACSROUTINE(STANDARD) COMPRESS(TAILORED) KEYLABEL(KEY.ONE) STRIPECOUNT(2) \
             UNIT(3390 2) ENTRYTYPE(DATASET)",
            &mut definition,
        )
        .unwrap();
        assert_eq!(
            definition.attributes.record_format,
            RecordFormat::VariableBlockedSpanned
        );
        assert_eq!(definition.attributes.logical_record_length, 512);
        assert_eq!(definition.attributes.key_length, Some(4));
        assert_eq!(definition.attributes.key_offset, Some(8));
        assert_eq!(definition.attributes.ccsid, Some(1047));
        assert_eq!(definition.dcb.block_size, 1024);
        assert_eq!(definition.dcb.buffer_count, 7);
        assert_eq!(definition.dcb.buffer_size, Some(2048));
        assert_eq!(
            definition.vsam.buffering,
            mainframe_env_host_api::BufferingMode::LocalSharedResources
        );
        assert_eq!(definition.vsam.control_interval_size, Some(4096));
        assert_eq!(definition.vsam.control_area_size, Some(65_536));
        assert_eq!(definition.vsam.share_options.cross_region, 2);
        assert_eq!(
            definition.vsam.access_mode,
            mainframe_env_host_api::VsamAccessMode::Rls
        );
        assert!(
            definition.vsam.spanned
                && definition.vsam.reuse
                && definition.vsam.speed
                && definition.vsam.write_check
                && definition.vsam.erase_on_delete
        );
        assert_eq!(definition.sms.acs_routine.as_deref(), Some("STANDARD"));
        assert_eq!(
            definition.security.compression,
            mainframe_env_host_api::CompressionMode::Tailored
        );
        assert_eq!(
            definition.security.encryption_key_label.as_deref(),
            Some("KEY.ONE")
        );
        assert_eq!(definition.vsam.stripe_count, 2);
        assert_eq!(
            definition.volumes.kind,
            mainframe_env_host_api::VolumeKind::PhysicalDisk
        );
        assert_eq!(definition.volumes.device_type.as_deref(), Some("3390"));
        assert_eq!(definition.volumes.unit_count, 2);

        let mut partitioned =
            mainframe_env_host_api::DatasetDefinition::compatibility(DatasetAttributes {
                organization: DatasetOrganization::Sequential,
                record_format: RecordFormat::Fixed,
                logical_record_length: 80,
                key_offset: None,
                key_length: None,
                ccsid: Some(37),
            });
        apply_ams_definition_operands(
            "ALLOCATE DATASET(USER.PDS) DSORG(PO) RECFM(FBS) LRECL(80) DIRECTORY(3)",
            &mut partitioned,
        )
        .unwrap();
        assert_eq!(
            partitioned.attributes.organization,
            DatasetOrganization::Partitioned
        );
        assert_eq!(
            partitioned.attributes.record_format,
            RecordFormat::FixedBlockedStandard
        );
        assert_eq!(partitioned.allocation.directory_blocks, 3);
    }

    #[test]
    fn idcams_print_reads_dd_input_and_preserves_hex_bytes() {
        let (service, dataset) = service_with_real_datasets();
        seed_real_dataset(
            &dataset,
            "USER.PRINTIN",
            vec![vec![0x00, 0x41, 0xff], vec![0x10, 0x20]],
            510,
        );
        let invocation = invocation();
        let job = service
            .submit(
                &invocation,
                &JclBundle {
                    primary: "//PRTJOB JOB CLASS=A\n//AMS EXEC PGM=IDCAMS\n//IN DD DSN=USER.PRINTIN,DISP=SHR\n//SYSIN DD *\n PRINT INFILE(IN) HEX SKIP(1) COUNT(1)\n/*\n".into(),
                    ..Default::default()
                },
                &IdempotencyKey::new("ams-print-infile", InvocationLimits::default()).unwrap(),
                false,
            )
            .unwrap();
        assert_eq!(
            service
                .run_next(&invocation, false)
                .unwrap()
                .unwrap()
                .return_code,
            Some(0)
        );
        assert!(
            service
                .spool(&invocation, &job.id, "SYSPRINT", 0, 8)
                .unwrap()
                .0
                .iter()
                .any(|record| record == b"1020")
        );
    }

    #[test]
    fn idcams_export_import_and_recover_preserve_snapshot_records() {
        let (service, dataset) = service_with_real_datasets();
        seed_real_dataset(
            &dataset,
            "USER.SOURCE",
            vec![b"FIRST".to_vec(), b"SECOND".to_vec()],
            520,
        );
        seed_real_dataset(&dataset, "USER.SNAPSHOT", Vec::new(), 522);
        let DatasetResult::Description(source_description) = dataset
            .invoke(DatasetRequest::Describe {
                dataset: DatasetName::new("USER.SOURCE", 128).unwrap(),
            })
            .unwrap()
        else {
            panic!("expected source description");
        };
        let source_definition = source_description.definition;
        let invocation = invocation();
        service
            .submit(
                &invocation,
                &JclBundle {
                    primary: "//EXPJOB JOB CLASS=A\n//AMS EXEC PGM=IDCAMS\n//OUT DD DSN=USER.SNAPSHOT,DISP=OLD\n//SYSIN DD *\n EXPORT ENTRIES(USER.SOURCE) OUTFILE(OUT)\n/*\n".into(),
                    ..Default::default()
                },
                &IdempotencyKey::new("ams-export", InvocationLimits::default()).unwrap(),
                false,
            )
            .unwrap();
        assert_eq!(
            service.run_next(&invocation, false).unwrap().unwrap().state,
            JobState::Completed
        );
        assert!(matches!(
            dataset.invoke(DatasetRequest::Describe {
                dataset: DatasetName::new("USER.SOURCE", 128).unwrap(),
            }),
            Ok(DatasetResult::Description(ref description))
                if description.version == 3
                    && description.definition.lifecycle.backup_generation == 1
        ));
        service
            .submit(
                &invocation,
                &JclBundle {
                    primary: "//IMPJOB JOB CLASS=A\n//AMS EXEC PGM=IDCAMS\n//IN DD DSN=USER.SNAPSHOT,DISP=SHR\n//SYSIN DD *\n IMPORT INFILE(IN) OUTDATASET(USER.RESTORED)\n/*\n".into(),
                    ..Default::default()
                },
                &IdempotencyKey::new("ams-import", InvocationLimits::default()).unwrap(),
                false,
            )
            .unwrap();
        assert_eq!(
            service.run_next(&invocation, false).unwrap().unwrap().state,
            JobState::Completed
        );
        assert!(matches!(
            dataset.invoke(DatasetRequest::Read {
                dataset: DatasetName::new("USER.RESTORED", 128).unwrap(),
                member: None,
                key: None,
                max_records: 8,
                control: Default::default(),
            }),
            Ok(DatasetResult::Records { records, .. })
                if records == [b"FIRST".to_vec(), b"SECOND".to_vec()]
        ));
        assert!(matches!(
            dataset.invoke(DatasetRequest::Describe {
                dataset: DatasetName::new("USER.RESTORED", 128).unwrap(),
            }),
            Ok(DatasetResult::Description(ref description))
                if description.definition == source_definition
        ));
        service
            .submit(
                &invocation,
                &JclBundle {
                    primary: "//RECJOB JOB CLASS=A\n//AMS EXEC PGM=IDCAMS\n//IN DD DSN=USER.SNAPSHOT,DISP=SHR\n//SYSIN DD *\n RECOVER INDATASET(USER.RESTORED) INFILE(IN)\n/*\n".into(),
                    ..Default::default()
                },
                &IdempotencyKey::new("ams-recover", InvocationLimits::default()).unwrap(),
                false,
            )
            .unwrap();
        assert_eq!(
            service.run_next(&invocation, false).unwrap().unwrap().state,
            JobState::Completed
        );
        let snapshot = DatasetName::new("USER.SNAPSHOT", 128).unwrap();
        let DatasetResult::Records { mut records, .. } = dataset
            .invoke(DatasetRequest::Read {
                dataset: snapshot.clone(),
                member: None,
                key: None,
                max_records: 8,
                control: Default::default(),
            })
            .unwrap()
        else {
            panic!("expected exported snapshot records");
        };
        let last = records[0].len() - 1;
        records[0][last] = if records[0][last] == b'0' { b'1' } else { b'0' };
        dataset
            .invoke(DatasetRequest::Write {
                dataset: snapshot,
                member: None,
                records,
                expected_version: Some(2),
                mutation: dataset_test_mutation(530),
            })
            .unwrap();
        service
            .submit(
                &invocation,
                &JclBundle {
                    primary: "//BADJOB JOB CLASS=A\n//AMS EXEC PGM=IDCAMS\n//IN DD DSN=USER.SNAPSHOT,DISP=SHR\n//SYSIN DD *\n IMPORT INFILE(IN) OUTDATASET(USER.BADREST)\n/*\n".into(),
                    ..Default::default()
                },
                &IdempotencyKey::new("ams-corrupt-import", InvocationLimits::default()).unwrap(),
                false,
            )
            .unwrap();
        assert_eq!(
            service
                .run_next(&invocation, false)
                .unwrap()
                .unwrap()
                .return_code,
            Some(12)
        );
        assert_eq!(
            dataset.invoke(DatasetRequest::Attributes {
                dataset: DatasetName::new("USER.BADREST", 128).unwrap(),
            }),
            Err(HostProblem::NotFound)
        );
    }

    #[test]
    fn capability_gated_ams_handlers_return_explicit_condition_codes() {
        let (service, _) = service_with_real_datasets();
        let invocation = invocation();
        let submitted = service
            .submit(
                &invocation,
                &JclBundle {
                    primary: "//CAPJOB JOB CLASS=A\n//AMS EXEC PGM=IDCAMS\n//SYSIN DD *\n ALTER LIBRARYENTRY NAME(LIB)\n ALTER VOLUMEENTRY NAME(VOL001)\n CREATE LIBRARYENTRY NAME(LIB)\n CREATE VOLUMEENTRY NAME(VOL001)\n DEFINE PAGESPACE (NAME(PAGE.ONE))\n SETCACHE NAME(VOL001)\n ALLOCATE DATASET(USER.PHYS) UNIT(3390)\n ALLOCATE DATASET(USER.TAPE) TAPE UNIT(3490)\n ALLOCATE DATASET(USER.ACS) ACSROUTINE(STANDARD)\n ALLOCATE DATASET(USER.COMP) COMPRESS(GENERIC)\n ALLOCATE DATASET(USER.ENC) KEYLABEL(KEY.ONE)\n DEFINE CLUSTER (NAME(USER.STRIPE) INDEXED KEYS(2 0) RECORDSIZE(4 4) STRIPECOUNT(2) EXTENDED)\n ALLOCATE DATASET(USER.UNKNOWN) FROBULATE(1)\n DEFINE CLUSTER (NAME(USER.REUSE) INDEXED KEYS(2 0) RECORDSIZE(4 4) REUSE)\n DEFINE CLUSTER (NAME(USER.COMPNT) INDEXED KEYS(2 0) RECORDSIZE(4 4) DATA(RECORDSIZE(4 4)))\n ALTER USER.A MIGRATE\n ALTER USER.A RECALL\n ALTER USER.A OPEN BUFNO(8)\n/*\n".into(),
                    ..Default::default()
                },
                &IdempotencyKey::new("ams-capabilities", InvocationLimits::default()).unwrap(),
                false,
            )
            .unwrap();
        let completed = service.run_next(&invocation, false).unwrap().unwrap();
        assert_eq!(completed.state, JobState::Completed);
        assert_eq!(completed.return_code, Some(12));
        let syprint = service
            .spool(&invocation, &submitted.id, "SYSPRINT", 0, 64)
            .unwrap()
            .0;
        for label in [
            "ALTER LIBRARYENTRY",
            "ALTER VOLUMEENTRY",
            "CREATE LIBRARYENTRY",
            "CREATE VOLUMEENTRY",
            "DEFINE PAGESPACE",
            "SETCACHE",
        ] {
            assert!(syprint.iter().any(|record| {
                let record = String::from_utf8_lossy(record);
                record.contains(label) && record.contains("UnsupportedCapability")
            }));
        }
        for capability in [
            "physical-volumes",
            "tape",
            "sms-acs",
            "compression",
            "encryption",
            "striping",
            "vsam-data-options",
            "vsam-components",
            "migration-recall",
            "ams-combined-alter",
            "ams-operand",
        ] {
            assert!(
                syprint.iter().any(|record| {
                    let record = String::from_utf8_lossy(record);
                    record.contains("UnsupportedCapability") && record.contains(capability)
                }),
                "missing {capability} in {syprint:?}"
            );
        }
    }

    #[test]
    fn idcams_export_disconnect_and_import_connect_transition_catalog_state() {
        let (service, dataset) = service_with_real_datasets();
        dataset
            .invoke(DatasetRequest::DefineCatalog {
                catalog: DatasetName::new("USER.EXPCAT", 128).unwrap(),
                kind: mainframe_env_host_api::CatalogKind::User,
                mutation: dataset_test_mutation(540),
            })
            .unwrap();
        seed_real_dataset(&dataset, "USER.CATIMAGE", Vec::new(), 541);
        let invocation = invocation();
        service
            .submit(
                &invocation,
                &JclBundle {
                    primary: "//DISCJOB JOB CLASS=A\n//AMS EXEC PGM=IDCAMS\n//OUT DD DSN=USER.CATIMAGE,DISP=OLD\n//SYSIN DD *\n EXPORT DISCONNECT ENTRIES(USER.EXPCAT) OUTFILE(OUT)\n/*\n".into(),
                    ..Default::default()
                },
                &IdempotencyKey::new("ams-disconnect", InvocationLimits::default()).unwrap(),
                false,
            )
            .unwrap();
        assert_eq!(
            service
                .run_next(&invocation, false)
                .unwrap()
                .unwrap()
                .return_code,
            Some(0)
        );
        assert!(matches!(
            dataset.invoke(DatasetRequest::Read {
                dataset: DatasetName::new("USER.CATIMAGE", 128).unwrap(),
                member: None,
                key: None,
                max_records: 4,
                control: Default::default(),
            }),
            Ok(DatasetResult::Records { records, .. })
                if records.len() == 1
                    && records[0].starts_with(b"MEAMSCAT1|USER.EXPCAT|sha256:")
        ));
        service
            .submit(
                &invocation,
                &JclBundle {
                    primary: "//CONNJOB JOB CLASS=A\n//AMS EXEC PGM=IDCAMS\n//IN DD DSN=USER.CATIMAGE,DISP=SHR\n//SYSIN DD *\n IMPORT CONNECT INFILE(IN)\n/*\n".into(),
                    ..Default::default()
                },
                &IdempotencyKey::new("ams-connect", InvocationLimits::default()).unwrap(),
                false,
            )
            .unwrap();
        assert_eq!(
            service
                .run_next(&invocation, false)
                .unwrap()
                .unwrap()
                .return_code,
            Some(0)
        );
        assert_eq!(
            dataset.invoke(DatasetRequest::SetCatalogConnection {
                catalog: DatasetName::new("USER.EXPCAT", 128).unwrap(),
                connected: false,
                expected_version: Some(3),
                mutation: dataset_test_mutation(543),
            }),
            Ok(DatasetResult::Mutated { version: 4 })
        );
    }

    #[test]
    fn idcams_export_disconnect_output_failure_leaves_catalog_connected() {
        let (service, dataset) = service_with_real_datasets();
        let catalog = DatasetName::new("USER.SAFECAT", 128).unwrap();
        dataset
            .invoke(DatasetRequest::DefineCatalog {
                catalog: catalog.clone(),
                kind: mainframe_env_host_api::CatalogKind::User,
                mutation: dataset_test_mutation(544),
            })
            .unwrap();
        dataset
            .invoke(DatasetRequest::Create {
                dataset: DatasetName::new("USER.TINYOUT", 128).unwrap(),
                attributes: DatasetAttributes {
                    organization: DatasetOrganization::Sequential,
                    record_format: RecordFormat::Fixed,
                    logical_record_length: 4,
                    key_offset: None,
                    key_length: None,
                    ccsid: Some(37),
                },
                mutation: dataset_test_mutation(545),
            })
            .unwrap();
        let invocation = invocation();
        service
            .submit(
                &invocation,
                &JclBundle {
                    primary: "//BADDISC JOB CLASS=A\n//AMS EXEC PGM=IDCAMS\n//OUT DD DSN=USER.TINYOUT,DISP=OLD\n//SYSIN DD *\n EXPORT DISCONNECT ENTRIES(USER.SAFECAT) OUTFILE(OUT)\n/*\n"
                        .into(),
                    ..Default::default()
                },
                &IdempotencyKey::new("ams-disconnect-output-failure", InvocationLimits::default())
                    .unwrap(),
                false,
            )
            .unwrap();
        assert_eq!(
            service
                .run_next(&invocation, false)
                .unwrap()
                .unwrap()
                .return_code,
            Some(16)
        );
        assert!(matches!(
            dataset.invoke(DatasetRequest::ResolveCatalog { name: catalog }),
            Ok(DatasetResult::Catalog(resolution)) if resolution.catalog.is_some()
        ));
    }

    #[test]
    fn idcams_unknown_statement_fails_before_any_dataset_mutation() {
        let records = Arc::new(Mutex::new(BTreeMap::from([(
            "IBMUSER.TARGET".into(),
            vec![b"UNCHANGED".to_vec()],
        )])));
        let service = service_with_datasets(records.clone());
        let invocation = invocation();
        service
            .submit(
                &invocation,
                &JclBundle {
                    primary: "//AMSJOB JOB CLASS=A\n//AMS EXEC PGM=IDCAMS\n//SYSIN DD *\n DELETE IBMUSER.TARGET\n UNKNOWN CONTROL\n/*\n".into(),
                    ..Default::default()
                },
                &IdempotencyKey::new("idcams-unknown", InvocationLimits::default()).unwrap(),
                false,
            )
            .unwrap();
        assert_eq!(
            service.run_next(&invocation, false).unwrap().unwrap().state,
            JobState::Failed
        );
        assert_eq!(
            records.lock().unwrap()["IBMUSER.TARGET"],
            vec![b"UNCHANGED".to_vec()]
        );
    }

    #[test]
    fn idcams_gdg_definition_and_positive_generation_output_are_exact() {
        let records = Arc::new(Mutex::new(BTreeMap::new()));
        let service = service_with_datasets(records.clone());
        let invocation = invocation();
        service
            .submit(
                &invocation,
                &JclBundle {
                    primary: "//GDGJOB JOB CLASS=A\n//DEFINE EXEC PGM=IDCAMS\n//SYSIN DD *\n DEFINE GENERATIONDATAGROUP (NAME(IBMUSER.HISTORY) LIMIT(3) SCRATCH NOEMPTY)\n/*\n//COPY EXEC PGM=IEBGENER\n//SYSUT1 DD *\nGENERATION\n/*\n//SYSUT2 DD DSN=IBMUSER.HISTORY(+1),DISP=(NEW,CATLG,DELETE)\n".into(),
                    ..Default::default()
                },
                &IdempotencyKey::new("gdg-output", InvocationLimits::default()).unwrap(),
                false,
            )
            .unwrap();
        assert_eq!(
            service.run_next(&invocation, false).unwrap().unwrap().state,
            JobState::Completed
        );
        assert_eq!(
            records.lock().unwrap()["IBMUSER.HISTORY.G0001V00"],
            vec![b"GENERATION".to_vec()]
        );
    }

    #[test]
    fn idcams_aix_path_and_build_route_to_typed_index_state() {
        let records = Arc::new(Mutex::new(BTreeMap::from([(
            "IBMUSER.BASE".into(),
            vec![b"BASE".to_vec()],
        )])));
        let service = service_with_datasets(records.clone());
        let invocation = invocation();
        service
            .submit(
                &invocation,
                &JclBundle {
                    primary: "//AIXJOB JOB CLASS=A\n//AIX EXEC PGM=IDCAMS\n//SYSIN DD *\n DEFINE ALTERNATEINDEX (NAME(IBMUSER.BASE.AIX) RELATE(IBMUSER.BASE) KEYS(4 2) NONUNIQUEKEY UPGRADE)\n DEFINE PATH (NAME(IBMUSER.BASE.PATH) PATHENTRY(IBMUSER.BASE.AIX))\n BLDINDEX INDATASET(IBMUSER.BASE) OUTDATASET(IBMUSER.BASE.AIX)\n/*\n".into(),
                    ..Default::default()
                },
                &IdempotencyKey::new("aix-route", InvocationLimits::default()).unwrap(),
                false,
            )
            .unwrap();
        assert_eq!(
            service.run_next(&invocation, false).unwrap().unwrap().state,
            JobState::Completed
        );
        assert_eq!(
            records.lock().unwrap()["IBMUSER.BASE.AIX"],
            vec![b"BASE=IBMUSER.BASE OFFSET=2 LENGTH=4 DUP=true".to_vec()]
        );
    }

    struct CobolProgram {
        calls: Arc<AtomicUsize>,
    }

    struct AbendProgram;

    impl Program for AbendProgram {
        fn execute(&self, _: &Invocation, _: &ProgramInput) -> Result<ProgramOutput, HostProblem> {
            Err(HostProblem::Condition {
                name: "ABEND:S0C7".into(),
                response: 500,
                response2: 7,
            })
        }
    }

    impl Program for CobolProgram {
        fn execute(&self, _: &Invocation, _: &ProgramInput) -> Result<ProgramOutput, HostProblem> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(ProgramOutput {
                return_code: 4,
                records: vec![b"COBOL OUTPUT".to_vec()],
                dd_outputs: BTreeMap::new(),
            })
        }
    }

    #[test]
    fn cobol_selector_dispatches_only_through_program_service() {
        let calls = Arc::new(AtomicUsize::new(0));
        let router: Arc<dyn HostProvider> = ProgramRouter::new(
            BTreeMap::from([(
                "COBOL".into(),
                Arc::new(CobolProgram {
                    calls: calls.clone(),
                }) as Arc<dyn Program>,
            )]),
            InvocationLimits::default(),
        )
        .unwrap();
        let service = service(Arc::new(MemoryStore::new(Default::default())), router);
        let invocation = invocation();
        service
            .submit(
                &invocation,
                &bundle("COBOL"),
                &IdempotencyKey::new("cobol", InvocationLimits::default()).unwrap(),
                false,
            )
            .unwrap();
        let completed = service.run_next(&invocation, false).unwrap().unwrap();
        assert_eq!(completed.return_code, Some(4));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn abend_propagates_after_only_cleanup_and_skips_normal_steps() {
        let cleanup_calls = Arc::new(AtomicUsize::new(0));
        let router: Arc<dyn HostProvider> = ProgramRouter::new(
            BTreeMap::from([
                ("FAILPGM".into(), Arc::new(AbendProgram) as Arc<dyn Program>),
                (
                    "CLEANUP".into(),
                    Arc::new(CobolProgram {
                        calls: cleanup_calls.clone(),
                    }) as Arc<dyn Program>,
                ),
            ]),
            InvocationLimits::default(),
        )
        .unwrap();
        let service = service(Arc::new(MemoryStore::new(Default::default())), router);
        let invocation = invocation();
        service
            .submit(
                &invocation,
                &JclBundle {
                    primary: "//ABENDJOB JOB CLASS=A\n//FAIL EXEC PGM=FAILPGM\n//NORMAL EXEC PGM=CLEANUP\n//RECOVER EXEC PGM=CLEANUP,COND=ONLY\n".into(),
                    ..Default::default()
                },
                &IdempotencyKey::new("abend-cleanup", InvocationLimits::default()).unwrap(),
                false,
            )
            .unwrap();
        let failed = service.run_next(&invocation, false).unwrap().unwrap();
        assert_eq!(failed.state, JobState::Failed);
        assert_eq!(failed.abend_code.as_deref(), Some("S0C7"));
        assert_eq!(cleanup_calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            failed
                .steps
                .iter()
                .map(|step| step.state)
                .collect::<Vec<_>>(),
            vec![
                StepState::Abended,
                StepState::SkippedCondition,
                StepState::Completed
            ]
        );
    }

    #[test]
    fn configured_initiator_selects_only_eligible_classes() {
        let mut scheduler = JesSchedulerConfiguration::single_node(1);
        scheduler.initiators.get_mut("INIT0001").unwrap().classes = BTreeSet::from(['B']);
        let store = Arc::new(MemoryStore::new(Default::default()));
        let providers = spool_test_providers(store.clone());
        let service = BatchService::open_with_scheduler(
            host_with(builtins(), providers),
            store,
            Default::default(),
            Default::default(),
            scheduler,
        )
        .unwrap();
        let invocation = invocation();
        let class_a = service
            .submit(
                &invocation,
                &bundle("IEFBR14"),
                &IdempotencyKey::new("class-a", InvocationLimits::default()).unwrap(),
                false,
            )
            .unwrap();
        let class_b = service
            .submit(
                &invocation,
                &JclBundle {
                    primary: "//BJOB JOB CLASS=B,PRTY=1\n//STEP1 EXEC PGM=IEFBR14\n".into(),
                    ..Default::default()
                },
                &IdempotencyKey::new("class-b", InvocationLimits::default()).unwrap(),
                false,
            )
            .unwrap();
        assert_eq!(
            service
                .run_next_on(&invocation, "INIT0001", false)
                .unwrap()
                .unwrap()
                .id,
            class_b.id
        );
        assert_eq!(service.get(&class_a.id).unwrap().state, JobState::Queued);
    }

    #[test]
    fn legacy_durable_job_migrates_and_recovers_without_losing_plan() {
        let store = Arc::new(MemoryStore::new(Default::default()));
        let invocation = invocation();
        let first = service(store.clone(), builtins());
        let id = first
            .submit(
                &invocation,
                &bundle("IEFBR14"),
                &IdempotencyKey::new("legacy-job", InvocationLimits::default()).unwrap(),
                false,
            )
            .unwrap()
            .id;
        let row = store.get_provider_state("jes-job", &id).unwrap().unwrap();
        let mut value: serde_json::Value = serde_json::from_slice(&row.payload).unwrap();
        let object = value.as_object_mut().unwrap();
        object.remove("schema_version");
        object.remove("initiator");
        object.remove("steps");
        object.remove("kind");
        object.remove("origin");
        object.remove("route");
        object.remove("program_registrations");
        object.remove("spool_sequence");
        object.remove("spool_files");
        object.remove("output_groups");
        object.insert(
            "spool".into(),
            serde_json::json!({"LEGACY": [[79, 76, 68]]}),
        );
        object.insert("state".into(), serde_json::Value::String("Running".into()));
        object.insert("attempt".into(), serde_json::json!(1));
        object.insert("version".into(), serde_json::json!(2));
        store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: "jes-job".into(),
                    key: id.clone(),
                    version: 2,
                    payload: serde_json::to_vec(&value).unwrap(),
                },
                Some(row.version),
            )
            .unwrap();
        for namespace in ["jes-spool", "spool-artifact"] {
            for record in store.list_provider_state(namespace, 128).unwrap() {
                store
                    .delete_provider_state(namespace, &record.key, record.version)
                    .unwrap();
            }
        }
        drop(first);

        let recovered = service(store.clone(), builtins());
        let snapshot = recovered.get(&id).unwrap();
        assert_eq!(snapshot.state, JobState::Queued);
        assert_eq!(snapshot.steps.len(), 1);
        assert_eq!(snapshot.steps[0].state, StepState::Pending);
        assert_eq!(snapshot.kind, JesJobKind::Batch);
        assert_eq!(snapshot.origin, JesSubmissionOrigin::External);
        assert_eq!(snapshot.route, JesJobRoute::default());
        assert_eq!(
            recovered.spool(&invocation, &id, "LEGACY", 0, 8).unwrap().0,
            vec![b"OLD".to_vec()]
        );
        let migrated = store.get_provider_state("jes-job", &id).unwrap().unwrap();
        let migrated: serde_json::Value = serde_json::from_slice(&migrated.payload).unwrap();
        assert_eq!(migrated["schema_version"], JES_DURABLE_JOB_CONTRACT);
        assert_eq!(migrated["state"], "queued");
        assert_eq!(migrated["spool"], serde_json::json!({}));
        assert!(migrated["spool_files"].get("LEGACY").is_some());
        assert!(migrated["program_registrations"].get("STEP1").is_some());
        let schema: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../conformance/0.8/schemas/jes-durable-job.schema.json"
        ))
        .unwrap();
        jsonschema::draft202012::options()
            .offline()
            .build(&schema)
            .unwrap()
            .validate(&migrated)
            .unwrap();
    }

    #[test]
    fn durable_program_registration_substitution_fails_closed_before_execution() {
        let store = Arc::new(MemoryStore::new(Default::default()));
        let invocation = invocation();
        let first = service(store.clone(), builtins());
        let job = first
            .submit(
                &invocation,
                &bundle("IEFBR14"),
                &IdempotencyKey::new("registration-substitution", InvocationLimits::default())
                    .unwrap(),
                false,
            )
            .unwrap();
        let row = store
            .get_provider_state("jes-job", &job.id)
            .unwrap()
            .unwrap();
        let mut value: serde_json::Value = serde_json::from_slice(&row.payload).unwrap();
        value["program_registrations"]["STEP1"]["handler"] =
            serde_json::json!({"kind": "unsupported"});
        value["version"] = serde_json::json!(row.version + 1);
        store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: "jes-job".into(),
                    key: job.id,
                    version: row.version + 1,
                    payload: serde_json::to_vec(&value).unwrap(),
                },
                Some(row.version),
            )
            .unwrap();
        drop(first);

        let provider_store: Arc<dyn ProviderStateStore> = store;
        let providers = spool_test_providers(provider_store.clone());
        assert!(matches!(
            BatchService::open(
                host_with(builtins(), providers),
                provider_store,
                Default::default(),
                Default::default(),
            ),
            Err(HostProblem::InfrastructureFailure)
        ));
    }

    #[test]
    fn sqlite_warm_start_recovers_running_job_to_queue() {
        let directory = std::env::temp_dir().join(format!(
            "mainframe-env-batch-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("batch.db");
        let url = format!("sqlite://{}?mode=rwc", path.display());
        let invocation = invocation();
        let id;
        {
            let store: Arc<dyn ProviderStateStore> =
                Arc::new(SqliteStateStore::open(&url, 8 * 1024 * 1024, 65536).unwrap());
            let service = service(store.clone(), builtins());
            id = service
                .submit(
                    &invocation,
                    &bundle("IEFBR14"),
                    &IdempotencyKey::new("restart", InvocationLimits::default()).unwrap(),
                    false,
                )
                .unwrap()
                .id;
            let row = store.get_provider_state("jes-job", &id).unwrap().unwrap();
            let mut job: Job = serde_json::from_slice(&row.payload).unwrap();
            job.version += 1;
            job.attempt = 1;
            job.state = JobState::Running;
            store
                .put_provider_state(job_record(&job).unwrap(), Some(row.version))
                .unwrap();
        }
        {
            let store: Arc<dyn ProviderStateStore> =
                Arc::new(SqliteStateStore::open(&url, 8 * 1024 * 1024, 65536).unwrap());
            let service = service(store, builtins());
            assert_eq!(service.get(&id).unwrap().state, JobState::Queued);
            assert_eq!(
                service.run_next(&invocation, false).unwrap().unwrap().state,
                JobState::Completed
            );
        }
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_dir(directory);
    }
}
