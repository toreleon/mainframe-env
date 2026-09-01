use crate::ams::{
    AmsCommand, AmsRegister, AmsStatement, compare, numeric_operand, operand, pair_operand,
};
use crate::controller::{
    BatchControllerRegistry, BatchControllerRegistryState, MAX_CONTROLLER_STATE_BYTES,
    ResolvedBatchController,
};
use crate::program::{
    ProgramExecution, TsoProgramExecution, program_execution, tso_program_execution,
};
use crate::{
    BatchControllerGeneration, BatchControllerInstallReceipt, BatchControllerPlan,
    BatchControllerSelector, Disposition, JclBundle, JclLimits, JobPlan, ProgramInput, StepPlan,
    decode_program_output, parse_jcl,
};
use mainframe_env_execution_api::{
    BoundedPayload, IdempotencyKey, Invocation, InvocationLimits, PrincipalId,
};
use mainframe_env_host_api::{
    AccessIntent, CicsConditionPolicy, CicsDisposition, CicsOperation, CicsRequest,
    DatasetAttributes, DatasetName, DatasetOrganization, DatasetRequest, DatasetResult,
    Db2Operation, Db2Request, EffectRequest, HostProblem, HostRequest, HostResult, ImsOperation,
    ImsQualifier, ImsRequest, MemberName, Mutation, ProgramName, ProgramRequest, RecordFormat,
    ResourceName, ScopedHostService, SecurityDecision, SecurityRequest,
};
use mainframe_env_store_api::{ProviderStateRecord, ProviderStateStore, StoreError};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

const CONTROLLER_STATE_NAMESPACE: &str = "batch-controller-state";
const CONTROLLER_STATE_KEY: &str = "registry";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BatchLimits {
    pub max_jobs: usize,
    pub max_queued: usize,
    pub max_active: usize,
    pub max_spool_files: usize,
    pub max_spool_records: usize,
    pub max_spool_bytes: usize,
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
            max_events: 65536,
            max_attempts: 3,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum JobState {
    Submitted,
    Held,
    Queued,
    Running,
    Completed,
    Failed,
    Cancelled,
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
    pub attempt: u32,
    pub version: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct Job {
    id: String,
    name: String,
    owner: String,
    class: char,
    priority: u8,
    state: JobState,
    return_code: Option<i32>,
    #[serde(default)]
    abend_code: Option<String>,
    active_step: Option<String>,
    attempt: u32,
    version: u64,
    submit_key: String,
    plan: JobPlan,
    spool: BTreeMap<String, Vec<Vec<u8>>>,
    events: Vec<String>,
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

pub struct BatchService {
    host: Arc<ScopedHostService>,
    store: Arc<dyn ProviderStateStore>,
    jcl_limits: JclLimits,
    limits: BatchLimits,
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
            if job.state == JobState::Running {
                let previous = job.version;
                job.version += 1;
                job.state = if job.attempt >= limits.max_attempts {
                    JobState::Failed
                } else {
                    JobState::Queued
                };
                job.active_step = None;
                job.events.push("warm-start-recovered".into());
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
        Ok(Arc::new(Self {
            host,
            store,
            jcl_limits,
            limits,
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
        let plan = parse_jcl(bundle, self.jcl_limits)?;
        self.authorize(
            invocation,
            "JESJOBS",
            &format!("JOB.{}", plan.name),
            AccessIntent::Execute,
            1,
        )?;
        let mut state = self.lock()?;
        if let Some(id) = state.replay.get(key.as_str()) {
            let job = state.jobs.get(id).ok_or(HostProblem::UnknownOutcome)?;
            return if job.plan == plan && job.owner == invocation.principal.id().as_str() {
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
        let mut spool = BTreeMap::new();
        spool.insert(
            "JESJCL".into(),
            plan.source
                .lines()
                .map(|line| line.as_bytes().to_vec())
                .collect(),
        );
        spool.insert(
            "JESMSGLG".into(),
            vec![format!("{id} SUBMITTED").into_bytes()],
        );
        spool.insert("JOBLOG".into(), vec![format!("{id} ADMITTED").into_bytes()]);
        let job = Job {
            id: id.clone(),
            name: plan.name.clone(),
            owner: invocation.principal.id().as_str().into(),
            class: plan.class,
            priority: plan.priority,
            state: if hold {
                JobState::Held
            } else {
                JobState::Queued
            },
            return_code: None,
            abend_code: None,
            active_step: None,
            attempt: 0,
            version: 1,
            submit_key: key.as_str().into(),
            plan,
            spool,
            events: vec![
                "submitted".into(),
                "admitted".into(),
                if hold { "held" } else { "queued" }.into(),
            ],
        };
        self.persist_job(&job, None)?;
        let result = snapshot(&job);
        state.replay.insert(key.as_str().into(), id.clone());
        state.jobs.insert(id, job);
        Ok(result)
    }

    pub fn hold(&self, id: &str) -> Result<JobSnapshot, HostProblem> {
        self.transition(id, JobState::Queued, JobState::Held, "held")
    }

    pub fn release(&self, id: &str) -> Result<JobSnapshot, HostProblem> {
        self.transition(id, JobState::Held, JobState::Queued, "released")
    }

    pub fn cancel(&self, id: &str) -> Result<JobSnapshot, HostProblem> {
        let mut state = self.lock()?;
        let current = state.jobs.get(id).cloned().ok_or(HostProblem::NotFound)?;
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
        append_spool(&mut next, "JESMSGLG", b"CANCELLED".to_vec(), self.limits)?;
        self.persist_job(&next, Some(current.version))?;
        let result = snapshot(&next);
        state.jobs.insert(id.into(), next);
        Ok(result)
    }

    pub fn run_next(
        &self,
        invocation: &Invocation,
        cancelled: bool,
    ) -> Result<Option<JobSnapshot>, HostProblem> {
        if self.limits.max_active == 0 {
            return Err(HostProblem::ResourceExhausted);
        }
        let id = {
            let state = self.lock()?;
            state
                .jobs
                .values()
                .filter(|job| {
                    job.state == JobState::Queued && job.owner == invocation.principal.id().as_str()
                })
                .max_by_key(|job| (job.priority, std::cmp::Reverse(job.id.clone())))
                .map(|job| job.id.clone())
        };
        let Some(id) = id else {
            return Ok(None);
        };
        if cancelled {
            return self.cancel(&id).map(Some);
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
            let mut running = current.clone();
            running.version += 1;
            running.attempt += 1;
            running.state = JobState::Running;
            running.events.push("running".into());
            self.persist_job(&running, Some(current.version))?;
            state.jobs.insert(id.clone(), running.clone());
            running
        };
        let outcome = self.execute(invocation, &mut job);
        let mut state = self.lock()?;
        let current = state.jobs.get(&id).cloned().ok_or(HostProblem::NotFound)?;
        job.version = current.version + 1;
        let terminal_step = job.active_step.clone();
        job.active_step = None;
        match outcome {
            Ok(return_code) => {
                job.return_code = Some(return_code);
                job.state = JobState::Completed;
                job.events.push("completed".into());
                append_spool(
                    &mut job,
                    "JESMSGLG",
                    format!("ENDED RC={return_code:04}").into_bytes(),
                    self.limits,
                )?;
            }
            Err(problem) => {
                if let HostProblem::Condition { name, .. } = &problem
                    && let Some(code) = name.strip_prefix("ABEND:")
                {
                    job.abend_code = Some(code.to_string());
                }
                job.state = JobState::Failed;
                job.events.push(format!("failed:{problem:?}"));
                if let Some(step) = terminal_step {
                    append_spool(
                        &mut job,
                        "JOBLOG",
                        format!("{step} FAILED {problem:?}").into_bytes(),
                        self.limits,
                    )?;
                }
                append_spool(
                    &mut job,
                    "JESMSGLG",
                    format!("FAILED {problem:?}").into_bytes(),
                    self.limits,
                )?;
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
        let steps = job.plan.steps.clone();
        let restart = job.plan.restart_step.clone();
        let mut restart_reached = restart.is_none();
        let mut effect_sequence = 0u64;
        let mut dataset_resolutions = BTreeMap::new();
        for step in &steps {
            if !restart_reached {
                restart_reached = restart.as_deref() == Some(step.name.as_str());
                if !restart_reached {
                    append_spool(
                        job,
                        "JOBLOG",
                        format!("{} BYPASSED RESTART", step.name).into_bytes(),
                        self.limits,
                    )?;
                    continue;
                }
            }
            if !step.condition.should_run(max_rc, abended) {
                append_spool(
                    job,
                    "JOBLOG",
                    format!("{} SKIPPED COND", step.name).into_bytes(),
                    self.limits,
                )?;
                continue;
            }
            job.active_step = Some(step.name.clone());
            let step_result = (|| -> Result<crate::ProgramOutput, HostProblem> {
                self.allocate_dds(
                    invocation,
                    job,
                    step,
                    &mut dataset_resolutions,
                    &mut effect_sequence,
                )?;
                let mut dds = step.dds.clone();
                self.hydrate_dds(
                    invocation,
                    job,
                    &dataset_resolutions,
                    &mut dds,
                    &mut effect_sequence,
                )?;
                for dd in &mut dds {
                    if !is_program_library_dd(dd)
                        && let Some(raw_name) = dd.dataset.clone()
                    {
                        dd.dataset =
                            Some(resolved_dataset(job, dd, &raw_name, &dataset_resolutions));
                    }
                }
                let input = ProgramInput {
                    parameter: step.parameter.clone(),
                    dds,
                };
                let execution = program_execution(&step.program);
                let idcams_return_code = if execution == ProgramExecution::Idcams {
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
                let mut output = match execution {
                    ProgramExecution::Sdsf => {
                        self.execute_sdsf(invocation, job, step, &input, &mut effect_sequence)?
                    }
                    ProgramExecution::Db2Tso => {
                        self.execute_db2_tso(invocation, job, step, &input, &mut effect_sequence)?
                    }
                    ProgramExecution::ImsController => self.execute_ims_controller(
                        invocation,
                        job,
                        step,
                        &input,
                        &mut effect_sequence,
                    )?,
                    ProgramExecution::ProgramService | ProgramExecution::Idcams => self
                        .execute_program_controller(
                            invocation,
                            job,
                            step,
                            &input,
                            &mut effect_sequence,
                            &step.program,
                        )?,
                    ProgramExecution::Unsupported => return Err(HostProblem::Unsupported),
                };
                if let Some(return_code) = idcams_return_code {
                    output.return_code = return_code;
                }
                self.write_dd_outputs(
                    invocation,
                    job,
                    step,
                    &dataset_resolutions,
                    &output.dd_outputs,
                    &mut effect_sequence,
                )?;
                Ok(output)
            })();
            let output = match step_result {
                Ok(output) => output,
                Err(problem) => {
                    self.dispose_dds(
                        invocation,
                        job,
                        step,
                        &dataset_resolutions,
                        &mut effect_sequence,
                        true,
                    )?;
                    return Err(problem);
                }
            };
            max_rc = max_rc.max(output.return_code);
            for record in output.records {
                append_spool(job, "SYSPRINT", record, self.limits)?;
            }
            append_spool(
                job,
                "JOBLOG",
                format!(
                    "{} {} RC={:04}",
                    step.name, step.program, output.return_code
                )
                .into_bytes(),
                self.limits,
            )?;
            abended = output.return_code < 0;
            self.dispose_dds(
                invocation,
                job,
                step,
                &dataset_resolutions,
                &mut effect_sequence,
                abended,
            )?;
            if abended {
                break;
            }
        }
        Ok(max_rc)
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
                        append_spool(
                            job,
                            "SYSPRINT",
                            format!("IDCAMS {} CC={code:02} {problem:?}", command.label())
                                .into_bytes(),
                            self.limits,
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
            "listcat" => self.listcat_idcams(invocation, job, effect_sequence, command.source()),
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
        for entry in entries {
            append_spool(
                job,
                "SYSPRINT",
                format!(
                    "{} {:?} VERSION={} RELATE={}",
                    entry.name.as_str(),
                    entry.kind,
                    entry.version,
                    entry.related.as_ref().map_or("-", DatasetName::as_str)
                )
                .into_bytes(),
                self.limits,
            )?;
        }
        if more {
            append_spool(
                job,
                "SYSPRINT",
                b"IDCAMS LISTCAT MORE".to_vec(),
                self.limits,
            )?;
        }
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
                append_spool(
                    job,
                    "SYSPRINT",
                    format!("{} {result:?}", command.label()).into_bytes(),
                    self.limits,
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
                append_spool(
                    job,
                    "SYSPRINT",
                    format!("LISTDATA {result:?}").into_bytes(),
                    self.limits,
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
                for record in records.into_iter().skip(skip).take(count) {
                    let record = if statement.contains(" HEX") {
                        hex_bytes(&record).into_bytes()
                    } else {
                        record
                    };
                    append_spool(job, "SYSPRINT", record, self.limits)?;
                }
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
                append_spool(
                    job,
                    "SYSPRINT",
                    format!("SHCDS {result:?}").into_bytes(),
                    self.limits,
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
        job: &Job,
        step: &StepPlan,
        dataset_resolutions: &mut BTreeMap<String, String>,
        effect_sequence: &mut u64,
    ) -> Result<(), HostProblem> {
        for dd in &step.dds {
            if is_program_library_dd(dd) {
                continue;
            }
            let Some(raw_name) = &dd.dataset else {
                continue;
            };
            let name = resolved_dataset(job, dd, raw_name, dataset_resolutions);
            self.authorize(
                invocation,
                "DATASET",
                &name,
                AccessIntent::Update,
                next_effect_sequence(invocation, effect_sequence)?,
            )?;
            if let Some(relative) = dd.generation {
                if dataset_resolutions.contains_key(&dataset_resolution_key(dd, raw_name)) {
                    continue;
                }
                let sequence = next_effect_sequence(invocation, effect_sequence)?;
                let key = effect_key(job, step, sequence)?;
                let base_name = DatasetName::new(raw_name.to_ascii_uppercase(), 128)
                    .map_err(|_| HostProblem::Malformed)?;
                let (request, idempotency_key) =
                    if relative == 1 && dd.disposition.contains(&Disposition::New) {
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
                dataset_resolutions.insert(
                    dataset_resolution_key(dd, raw_name),
                    dataset.as_str().to_string(),
                );
                continue;
            }
            let mut create = dd.disposition.contains(&Disposition::New);
            if dd.disposition.first() == Some(&Disposition::Modify) && dd.member.is_none() {
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
                            dataset: DatasetName::new(name.clone(), 128)
                                .map_err(|_| HostProblem::Malformed)?,
                        }),
                    },
                );
                match result.effect.outcome {
                    Ok(HostResult::Dataset(DatasetResult::Attributes { .. })) => {}
                    Err(HostProblem::NotFound) => create = true,
                    Ok(_) => return Err(HostProblem::ProviderFailure),
                    Err(problem) => return Err(problem),
                }
            }
            if create && dd.member.is_none() {
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
                        request: HostRequest::Dataset(DatasetRequest::Create {
                            dataset: DatasetName::new(name, 128)
                                .map_err(|_| HostProblem::Malformed)?,
                            attributes: dataset_attributes_for_dd(dd)?,
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
        }
        Ok(())
    }

    fn hydrate_dds(
        &self,
        invocation: &Invocation,
        job: &Job,
        dataset_resolutions: &BTreeMap<String, String>,
        dds: &mut [crate::DdPlan],
        effect_sequence: &mut u64,
    ) -> Result<(), HostProblem> {
        for dd in dds {
            if is_program_library_dd(dd) {
                continue;
            }
            let Some(raw_name) = dd.dataset.clone() else {
                continue;
            };
            if dd.disposition.contains(&Disposition::New) {
                dd.ccsid = dataset_attributes_for_dd(dd)?.ccsid;
                continue;
            }
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
                        dataset: DatasetName::new(
                            resolved_dataset(job, dd, &raw_name, dataset_resolutions),
                            128,
                        )
                        .map_err(|_| HostProblem::Malformed)?,
                    }),
                },
            );
            let HostResult::Dataset(DatasetResult::Attributes { attributes, .. }) =
                attributes.effect.outcome?
            else {
                return Err(HostProblem::ProviderFailure);
            };
            dd.ccsid = attributes.ccsid;
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
                        dataset: DatasetName::new(
                            resolved_dataset(job, dd, &raw_name, dataset_resolutions),
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
                        key: None,
                        max_records: 4_096,
                        control: Default::default(),
                    }),
                },
            );
            let HostResult::Dataset(DatasetResult::Records { records, .. }) =
                result.effect.outcome?
            else {
                return Err(HostProblem::ProviderFailure);
            };
            dd.inline_data.clear();
            for record in records {
                dd.inline_data.extend_from_slice(&record);
                dd.inline_data.push(b'\n');
            }
        }
        Ok(())
    }

    fn write_dd_outputs(
        &self,
        invocation: &Invocation,
        job: &mut Job,
        step: &StepPlan,
        dataset_resolutions: &BTreeMap<String, String>,
        outputs: &BTreeMap<String, Vec<Vec<u8>>>,
        effect_sequence: &mut u64,
    ) -> Result<(), HostProblem> {
        for (name, records) in outputs {
            let dd = step
                .dds
                .iter()
                .find(|dd| dd.name.eq_ignore_ascii_case(name))
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
                self.submit(
                    invocation,
                    &JclBundle {
                        primary: source,
                        ..Default::default()
                    },
                    &IdempotencyKey::new(
                        format!("jes:{}:{}:internal-reader", job.id, step.name),
                        InvocationLimits::default(),
                    )
                    .map_err(|_| HostProblem::ResourceExhausted)?,
                    false,
                )?;
                append_spool(
                    job,
                    &dd.name,
                    b"INTERNAL READER SUBMITTED".to_vec(),
                    self.limits,
                )?;
                continue;
            }
            if dd.sysout.is_some() {
                for record in records {
                    append_spool(job, &dd.name, record.clone(), self.limits)?;
                }
                continue;
            }
            let Some(raw_name) = &dd.dataset else {
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
                        dataset: DatasetName::new(
                            resolved_dataset(job, dd, raw_name, dataset_resolutions),
                            128,
                        )
                        .map_err(|_| HostProblem::Malformed)?,
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
                                dataset: DatasetName::new(
                                    resolved_dataset(job, dd, raw_name, dataset_resolutions),
                                    128,
                                )
                                .map_err(|_| HostProblem::Malformed)?,
                                record_number: u64::try_from(position + 1)
                                    .map_err(|_| HostProblem::ResourceExhausted)?,
                                record,
                                expected_version: None,
                                mutation: Mutation {
                                    sequence,
                                    idempotency_key: key,
                                    transaction: Some(job.id.clone()),
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
            let result = self.host.invoke(
                invocation,
                invocation.deadline_tick.saturating_sub(1),
                false,
                EffectRequest {
                    run_unit: invocation.run_unit_id.clone(),
                    sequence,
                    deadline_tick: invocation.deadline_tick,
                    idempotency_key: Some(key.clone()),
                    request: HostRequest::Dataset(DatasetRequest::Write {
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
                        records,
                        expected_version: None,
                        mutation: Mutation {
                            sequence,
                            idempotency_key: key,
                            transaction: Some(job.id.clone()),
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
        Ok(())
    }

    fn dispose_dds(
        &self,
        invocation: &Invocation,
        job: &Job,
        step: &StepPlan,
        dataset_resolutions: &BTreeMap<String, String>,
        effect_sequence: &mut u64,
        abnormal: bool,
    ) -> Result<(), HostProblem> {
        for dd in &step.dds {
            if is_program_library_dd(dd) {
                continue;
            }
            let Some(raw_name) = &dd.dataset else {
                continue;
            };
            let terminal = if abnormal {
                dd.disposition.get(2)
            } else {
                dd.disposition.get(1)
            };
            let delete = terminal == Some(&Disposition::Delete)
                || (dd.temporary
                    && !matches!(
                        terminal,
                        Some(
                            Disposition::Pass
                                | Disposition::Keep
                                | Disposition::Catalog
                                | Disposition::Uncatalog
                        )
                    ));
            if !delete {
                continue;
            }
            let sequence = next_effect_sequence(invocation, effect_sequence)?;
            let key = effect_key(job, step, sequence)?;
            let request = DatasetRequest::Delete {
                dataset: DatasetName::new(
                    resolved_dataset(job, dd, raw_name, dataset_resolutions),
                    128,
                )
                .map_err(|_| HostProblem::Malformed)?,
                member: dd
                    .member
                    .as_ref()
                    .map(|member| MemberName::new(member, 8).map_err(|_| HostProblem::Malformed))
                    .transpose()?,
                expected_version: None,
                purge: true,
                current_date: None,
                mutation: Mutation {
                    sequence,
                    idempotency_key: key.clone(),
                    transaction: Some(job.id.clone()),
                },
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
            outcome.effect.outcome?;
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
        id: &str,
        file: &str,
        start: usize,
        max: usize,
    ) -> Result<(Vec<Vec<u8>>, bool), HostProblem> {
        if max == 0 || max > self.limits.max_spool_records {
            return Err(HostProblem::ResourceExhausted);
        }
        let state = self.lock()?;
        let records = state
            .jobs
            .get(id)
            .ok_or(HostProblem::NotFound)?
            .spool
            .get(file)
            .ok_or(HostProblem::NotFound)?;
        Ok((
            records.iter().skip(start).take(max).cloned().collect(),
            start.saturating_add(max) < records.len(),
        ))
    }

    pub fn spool_files(&self, id: &str) -> Result<Vec<(usize, String, usize, usize)>, HostProblem> {
        let state = self.lock()?;
        let job = state.jobs.get(id).ok_or(HostProblem::NotFound)?;
        Ok(job
            .spool
            .iter()
            .enumerate()
            .map(|(id, (name, records))| {
                (
                    id,
                    name.clone(),
                    records.len(),
                    records.iter().map(Vec::len).sum(),
                )
            })
            .collect())
    }

    pub fn spool_by_index(
        &self,
        id: &str,
        file: usize,
        start: usize,
        max: usize,
    ) -> Result<(Vec<Vec<u8>>, bool), HostProblem> {
        let state = self.lock()?;
        let job = state.jobs.get(id).ok_or(HostProblem::NotFound)?;
        let records = job.spool.values().nth(file).ok_or(HostProblem::NotFound)?;
        if max == 0 || max > self.limits.max_spool_records {
            return Err(HostProblem::ResourceExhausted);
        }
        Ok((
            records.iter().skip(start).take(max).cloned().collect(),
            start.saturating_add(max) < records.len(),
        ))
    }

    pub fn purge(&self, id: &str) -> Result<(), HostProblem> {
        let mut state = self.lock()?;
        let job = state.jobs.get(id).ok_or(HostProblem::NotFound)?;
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
        self.store
            .delete_provider_state("jes-job", id, job.version)
            .map_err(store_error)?;
        let submit_key = job.submit_key.clone();
        state.jobs.remove(id);
        state.replay.remove(&submit_key);
        Ok(())
    }

    fn transition(
        &self,
        id: &str,
        from: JobState,
        to: JobState,
        event: &str,
    ) -> Result<JobSnapshot, HostProblem> {
        let mut state = self.lock()?;
        let current = state.jobs.get(id).cloned().ok_or(HostProblem::NotFound)?;
        if current.state != from {
            return Err(HostProblem::Condition {
                name: "INVALID_STATE".into(),
                response: 409,
                response2: 0,
            });
        }
        let mut next = current.clone();
        next.version += 1;
        next.state = to;
        next.events.push(event.into());
        self.persist_job(&next, Some(current.version))?;
        let result = snapshot(&next);
        state.jobs.insert(id.into(), next);
        Ok(result)
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

fn append_spool(
    job: &mut Job,
    file: &str,
    record: Vec<u8>,
    limits: BatchLimits,
) -> Result<(), HostProblem> {
    if !job.spool.contains_key(file) && job.spool.len() >= limits.max_spool_files {
        return Err(HostProblem::ResourceExhausted);
    }
    let records = job.spool.values().map(Vec::len).sum::<usize>();
    let bytes = job.spool.values().flatten().map(Vec::len).sum::<usize>();
    if records >= limits.max_spool_records
        || bytes
            .checked_add(record.len())
            .is_none_or(|bytes| bytes > limits.max_spool_bytes)
    {
        return Err(HostProblem::ResourceExhausted);
    }
    job.spool.entry(file.into()).or_default().push(record);
    Ok(())
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
        attempt: job.attempt,
        version: job.version,
    }
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
    use mainframe_env_store::{MemoryStore, SqliteStateStore};
    use mainframe_env_store_api::{ProviderStateMutation, ProviderStateWrite};
    use std::collections::BTreeSet;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    struct SecurityProvider {
        descriptor: CapabilityDescriptor,
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
                HostRequest::Security(_) => Ok(HostResult::Security(SecurityDecision::Allow)),
                _ => Err(HostProblem::Malformed),
            };
            EffectResult {
                sequence: effect.sequence,
                outcome,
            }
        }
    }

    fn host(program: Arc<dyn HostProvider>) -> Arc<ScopedHostService> {
        host_with(program, Vec::new())
    }

    fn host_with(
        program: Arc<dyn HostProvider>,
        mut extra: Vec<Arc<dyn HostProvider>>,
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
        BatchService::open(host(program), store, Default::default(), Default::default()).unwrap()
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
        BatchService::open(
            host_with(
                builtins(),
                vec![
                    provider("host.dataset.read"),
                    provider("host.dataset.write"),
                ],
            ),
            Arc::new(MemoryStore::new(Default::default())),
            Default::default(),
            Default::default(),
        )
        .unwrap()
    }

    fn service_with_real_datasets() -> (Arc<BatchService>, Arc<DatasetService>) {
        let store = Arc::new(MemoryStore::new(Default::default()));
        let dataset_store: Arc<dyn ProviderStateStore> = store.clone();
        let dataset = DatasetService::open(dataset_store, DatasetLimits::default()).unwrap();
        let providers = dataset_providers(dataset.clone(), InvocationLimits::default());
        let batch_store: Arc<dyn ProviderStateStore> = store;
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
        assert_eq!(service.hold(&submitted.id).unwrap().state, JobState::Held);
        assert_eq!(
            service.release(&submitted.id).unwrap().state,
            JobState::Queued
        );
        let completed = service.run_next(&invocation, false).unwrap().unwrap();
        assert_eq!(completed.state, JobState::Completed);
        assert_eq!(completed.return_code, Some(0));
        let (records, more) = service.spool(&submitted.id, "SYSPRINT", 0, 10).unwrap();
        assert_eq!(records, vec![b"IEFBR14".to_vec()]);
        assert!(!more);
        service.purge(&submitted.id).unwrap();
        assert_eq!(service.get(&submitted.id), Err(HostProblem::NotFound));
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
            service.cancel(&queued.id).unwrap().state,
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
                    .spool(&submitted.id, "JESMSGLG", 0, 20)
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
        let service = BatchService::open(
            host_with(builtins(), vec![cics]),
            Arc::new(MemoryStore::new(Default::default())),
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
                .spool(&submitted.id, "CMDOUT", 0, 16)
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
            service.spool(&submitted.id, "SYSPRINT", 0, 10).unwrap().0,
            vec![b"SYSUT2 RECORDS=2".to_vec()]
        );
        assert_eq!(
            service.spool(&submitted.id, "SYSUT2", 0, 10).unwrap().0,
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
        service
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
        assert!(
            jobs.iter()
                .any(|job| job.name == "CHILD" && job.state == JobState::Queued)
        );
        let drained = service.drain_queued(&invocation).unwrap();
        assert_eq!(drained.len(), 1);
        assert_eq!(drained[0].name, "CHILD");
        assert_eq!(drained[0].state, JobState::Completed);
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
                .spool(&submitted.id, "SYSPRINT", 0, 64)
                .unwrap()
                .0
                .iter()
                .any(|record| String::from_utf8_lossy(record).contains("LISTDATA"))
        );
        let syprint = service.spool(&submitted.id, "SYSPRINT", 0, 64).unwrap().0;
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

        let providers = dataset_providers(dataset.clone(), InvocationLimits::default());
        let batch_store: Arc<dyn ProviderStateStore> = store.clone();
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
                .spool(&job.id, "SYSPRINT", 0, 8)
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
                .spool(&job.id, "SYSPRINT", 0, 8)
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
        let syprint = service.spool(&submitted.id, "SYSPRINT", 0, 64).unwrap().0;
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
