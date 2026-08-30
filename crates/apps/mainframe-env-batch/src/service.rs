use crate::{
    Disposition, JclBundle, JclLimits, JobPlan, ProgramInput, StepPlan, UtilityDisposition,
    decode_program_output, parse_jcl, utility_disposition,
};
use mainframe_env_execution_api::{
    BoundedPayload, IdempotencyKey, Invocation, InvocationLimits, PrincipalId,
};
use mainframe_env_host_api::{
    AccessIntent, DatasetAttributes, DatasetName, DatasetOrganization, DatasetRequest,
    DatasetResult, EffectRequest, HostProblem, HostRequest, HostResult, MemberName, Mutation,
    ProgramName, ProgramRequest, RecordFormat, ResourceName, ScopedHostService, SecurityDecision,
    SecurityRequest,
};
use mainframe_env_store_api::{ProviderStateRecord, ProviderStateStore, StoreError};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

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

pub struct BatchService {
    host: Arc<ScopedHostService>,
    store: Arc<dyn ProviderStateStore>,
    jcl_limits: JclLimits,
    limits: BatchLimits,
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
        Ok(Arc::new(Self {
            host,
            store,
            jcl_limits,
            limits,
            state: Mutex::new(State {
                jobs,
                replay,
                next_id,
            }),
        }))
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
                .filter(|job| job.state == JobState::Queued)
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
                    dd.dataset = Some(resolved_dataset(job, dd, &raw_name, &dataset_resolutions));
                }
            }
            let input = ProgramInput {
                parameter: step.parameter.clone(),
                dds,
            };
            if step.program.eq_ignore_ascii_case("IDCAMS") {
                self.execute_idcams(
                    invocation,
                    job,
                    step,
                    &dataset_resolutions,
                    &input,
                    &mut effect_sequence,
                )?;
            }
            let bytes = serde_json::to_vec(&input).map_err(|_| HostProblem::ProviderFailure)?;
            let payload = BoundedPayload::new(
                "mainframe-env.program.input@1",
                bytes,
                InvocationLimits::default(),
            )
            .map_err(|_| HostProblem::ResourceExhausted)?;
            if utility_disposition(&step.program)
                .is_some_and(|disposition| disposition != UtilityDisposition::Implemented)
            {
                return Err(HostProblem::Unsupported);
            }
            let program =
                ProgramName::new(&step.program, 128).map_err(|_| HostProblem::Malformed)?;
            let sequence = next_effect_sequence(invocation, &mut effect_sequence)?;
            let result = self.host.invoke(
                invocation,
                invocation.deadline_tick.saturating_sub(1),
                false,
                EffectRequest {
                    run_unit: invocation.run_unit_id.clone(),
                    sequence,
                    deadline_tick: invocation.deadline_tick,
                    idempotency_key: Some(effect_key(job, step, sequence)?),
                    request: HostRequest::Program(ProgramRequest::Call { program, payload }),
                },
            );
            let output = match result.effect.outcome? {
                HostResult::Program(payload) => decode_program_output(&payload)?,
                _ => return Err(HostProblem::ProviderFailure),
            };
            self.write_dd_outputs(
                invocation,
                job,
                step,
                &dataset_resolutions,
                &output.dd_outputs,
                &mut effect_sequence,
            )?;
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

    fn execute_idcams(
        &self,
        invocation: &Invocation,
        job: &mut Job,
        step: &StepPlan,
        dataset_resolutions: &BTreeMap<String, String>,
        input: &ProgramInput,
        effect_sequence: &mut u64,
    ) -> Result<(), HostProblem> {
        let control = input_dd_text(input, "SYSIN")?;
        let statements = idcams_statements(&control)?;
        for statement in &statements {
            let operation = statement
                .split_whitespace()
                .next()
                .ok_or(HostProblem::Malformed)?;
            if !matches!(
                operation,
                "DELETE" | "DEFINE" | "REPRO" | "LISTCAT" | "BLDINDEX" | "IF" | "SET"
            ) {
                return Err(HostProblem::Unsupported);
            }
        }
        for statement in statements {
            let operation = statement
                .split_whitespace()
                .next()
                .ok_or(HostProblem::Malformed)?;
            match operation {
                "IF" | "SET" | "BLDINDEX" => {}
                "REPRO" => {
                    let input_name = parenthesized_operand(&statement, &["INFILE", "IFILE"])
                        .ok_or(HostProblem::Malformed)?;
                    let output_name = parenthesized_operand(&statement, &["OUTFILE", "OFILE"])
                        .ok_or(HostProblem::Malformed)?;
                    let records = input_dd_records(input, &input_name)?;
                    self.write_dd_outputs(
                        invocation,
                        job,
                        step,
                        dataset_resolutions,
                        &BTreeMap::from([(output_name, records)]),
                        effect_sequence,
                    )?;
                }
                "DELETE" => {
                    let name = statement
                        .split_whitespace()
                        .nth(1)
                        .map(|value| value.trim_matches(['(', ')']).to_ascii_uppercase())
                        .ok_or(HostProblem::Malformed)?;
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
                            request: HostRequest::Dataset(DatasetRequest::Delete {
                                dataset: DatasetName::new(name, 128)
                                    .map_err(|_| HostProblem::Malformed)?,
                                member: None,
                                expected_version: None,
                                mutation: Mutation {
                                    sequence,
                                    idempotency_key: key,
                                    transaction: Some(job.id.clone()),
                                },
                            }),
                        },
                    );
                    match result.effect.outcome {
                        Ok(HostResult::Dataset(_)) | Err(HostProblem::NotFound) => {}
                        Ok(_) => return Err(HostProblem::ProviderFailure),
                        Err(problem) => return Err(problem),
                    }
                }
                "DEFINE" => {
                    self.define_idcams(invocation, job, step, &statement, effect_sequence)?
                }
                "LISTCAT" => {
                    let pattern = parenthesized_operand(&statement, &["ENTRIES", "ENTRY"])
                        .unwrap_or_else(|| "**".into());
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
                            request: HostRequest::Dataset(DatasetRequest::List {
                                pattern,
                                start: None,
                                max_items: 4_096,
                            }),
                        },
                    );
                    if !matches!(
                        result.effect.outcome?,
                        HostResult::Dataset(DatasetResult::Listed { .. })
                    ) {
                        return Err(HostProblem::ProviderFailure);
                    }
                }
                _ => return Err(HostProblem::Unsupported),
            }
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
        if statement.contains(" PATH ") || statement.starts_with("DEFINE PATH") {
            return Ok(());
        }
        let sequence = next_effect_sequence(invocation, effect_sequence)?;
        let key = effect_key(job, step, sequence)?;
        let request = if statement.contains("GENERATIONDATAGROUP") {
            DatasetRequest::DefineGenerationGroup {
                base: DatasetName::new(
                    parenthesized_operand(statement, &["NAME"]).ok_or(HostProblem::Malformed)?,
                    128,
                )
                .map_err(|_| HostProblem::Malformed)?,
                limit: numeric_parenthesized_operand(statement, "LIMIT").unwrap_or(255),
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
                pair_parenthesized_operand(statement, "KEYS").ok_or(HostProblem::Malformed)?;
            DatasetRequest::DefineAlternateIndex {
                base: DatasetName::new(
                    parenthesized_operand(statement, &["RELATE"]).ok_or(HostProblem::Malformed)?,
                    128,
                )
                .map_err(|_| HostProblem::Malformed)?,
                index: DatasetName::new(
                    parenthesized_operand(statement, &["NAME"]).ok_or(HostProblem::Malformed)?,
                    128,
                )
                .map_err(|_| HostProblem::Malformed)?,
                key_offset,
                key_length,
                allow_duplicates: statement.contains("NONUNIQUEKEY"),
                mutation: Mutation {
                    sequence,
                    idempotency_key: key.clone(),
                    transaction: Some(job.id.clone()),
                },
            }
        } else if statement.contains("CLUSTER") {
            let (minimum, maximum) =
                pair_parenthesized_operand(statement, "RECORDSIZE").unwrap_or((80, 80));
            let keys = pair_parenthesized_operand(statement, "KEYS");
            let organization = if statement.contains("NUMBERED") {
                DatasetOrganization::Relative
            } else if statement.contains("NONINDEXED") {
                DatasetOrganization::EntrySequenced
            } else if statement.contains("INDEXED") {
                DatasetOrganization::KeySequenced
            } else {
                DatasetOrganization::Sequential
            };
            DatasetRequest::Create {
                dataset: DatasetName::new(
                    parenthesized_operand(statement, &["NAME"]).ok_or(HostProblem::Malformed)?,
                    128,
                )
                .map_err(|_| HostProblem::Malformed)?,
                attributes: DatasetAttributes {
                    organization,
                    record_format: if minimum == maximum {
                        RecordFormat::Fixed
                    } else {
                        RecordFormat::Variable
                    },
                    logical_record_length: maximum,
                    key_offset: keys.map(|(_, offset)| offset),
                    key_length: keys.map(|(length, _)| length),
                    ccsid: Some(37),
                },
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
            let Some(raw_name) = &dd.dataset else {
                continue;
            };
            if dd.disposition.contains(&Disposition::New) {
                continue;
            }
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
                        key: None,
                        max_records: 4_096,
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
    dd.name.eq_ignore_ascii_case("STEPLIB") || dd.name.eq_ignore_ascii_case("JOBLIB")
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

fn idcams_statements(control: &str) -> Result<Vec<String>, HostProblem> {
    let mut statements = Vec::new();
    let mut current = String::new();
    for line in control.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('*') {
            continue;
        }
        let raw_operation = line.split_whitespace().next().unwrap_or_default();
        let operation = raw_operation.split('(').next().unwrap_or_default();
        let starts_statement = matches!(
            operation,
            "DELETE" | "DEFINE" | "REPRO" | "LISTCAT" | "BLDINDEX" | "IF" | "SET"
        );
        let continuation = raw_operation.starts_with('(')
            || matches!(
                operation,
                "CLUSTER"
                    | "ALTERNATEINDEX"
                    | "DATA"
                    | "INDEX"
                    | "NAME"
                    | "RELATE"
                    | "KEYS"
                    | "RECORDSIZE"
                    | "SHAREOPTIONS"
                    | "ERASE"
                    | "INDEXED"
                    | "NONINDEXED"
                    | "NUMBERED"
                    | "VOLUMES"
                    | "CYLINDERS"
                    | "TRACKS"
                    | "KILOBYTES"
                    | "MEGABYTES"
                    | "FREESPACE"
                    | "CISZ"
                    | "REUSE"
                    | "UNIQUEKEY"
                    | "NONUNIQUEKEY"
                    | "UPGRADE"
                    | "NOUPGRADE"
                    | "PATHENTRY"
                    | "INDATASET"
                    | "OUTDATASET"
                    | "LIMIT"
                    | "SCRATCH"
                    | "NOSCRATCH"
                    | "EMPTY"
                    | "NOEMPTY"
                    | "PURGE"
                    | ")"
            );
        if (starts_statement || (!continuation && !current.is_empty())) && !current.is_empty() {
            statements.push(current.trim().to_string());
            current.clear();
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(line.trim_end_matches('-').trim_end());
        if current.len() > 1024 * 1024 {
            return Err(HostProblem::ResourceExhausted);
        }
    }
    if !current.is_empty() {
        statements.push(current.trim().to_string());
    }
    if statements.is_empty() {
        return Err(HostProblem::Malformed);
    }
    Ok(statements)
}

pub fn validate_idcams_control(control: &[u8]) -> Result<usize, HostProblem> {
    let control = std::str::from_utf8(control)
        .map_err(|_| HostProblem::Malformed)?
        .to_ascii_uppercase();
    let statements = idcams_statements(&control)?;
    if statements.iter().any(|statement| {
        !matches!(
            statement.split_whitespace().next().unwrap_or_default(),
            "DELETE" | "DEFINE" | "REPRO" | "LISTCAT" | "BLDINDEX" | "IF" | "SET"
        )
    }) {
        return Err(HostProblem::Unsupported);
    }
    Ok(statements.len())
}

fn parenthesized_operand(statement: &str, names: &[&str]) -> Option<String> {
    names.iter().find_map(|name| {
        let compact = format!("{name}(");
        let spaced = format!("{name} (");
        let start = statement
            .find(&compact)
            .map(|at| at + compact.len())
            .or_else(|| statement.find(&spaced).map(|at| at + spaced.len()))?;
        let end = statement[start..].find(')')? + start;
        let value = statement[start..end].trim().trim_matches(['\'', '"']);
        (!value.is_empty()).then(|| value.to_string())
    })
}

fn numeric_parenthesized_operand(statement: &str, name: &str) -> Option<u32> {
    parenthesized_operand(statement, &[name])?.parse().ok()
}

fn pair_parenthesized_operand(statement: &str, name: &str) -> Option<(u32, u32)> {
    let value = parenthesized_operand(statement, &[name])?;
    let values = value
        .split(|ch: char| ch == ',' || ch.is_whitespace())
        .filter(|value| !value.is_empty())
        .map(str::parse)
        .collect::<Result<Vec<u32>, _>>()
        .ok()?;
    (values.len() == 2).then(|| (values[0], values[1]))
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
    use mainframe_env_execution_api::{
        ArtifactRef, CapabilityId, ExecutionId, Principal, RequestId, ResourceLimits, RunUnitId,
        Selector, ServiceClass, TraceId,
    };
    use mainframe_env_host_api::{
        CapabilityDescriptor, DatasetResult, EffectResult, HostLimits, HostProvider,
        RegistrySnapshot,
    };
    use mainframe_env_store::{MemoryStore, SqliteStateStore};
    use std::collections::BTreeSet;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct SecurityProvider {
        descriptor: CapabilityDescriptor,
    }

    struct DatasetProvider {
        descriptor: CapabilityDescriptor,
        records: Arc<Mutex<BTreeMap<String, Vec<Vec<u8>>>>>,
        generations: Arc<Mutex<BTreeMap<String, Vec<String>>>>,
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
                HostRequest::Dataset(DatasetRequest::Create { dataset, .. }) => self
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
        for (index, program) in ["SDSF", "FTP", "IKJEFT1B"].into_iter().enumerate() {
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
                    primary: "//AMSJOB JOB CLASS=A\n//AMS EXEC PGM=IDCAMS\n//INPUT DD DSN=IBMUSER.INPUT,DISP=SHR\n//OUTPUT DD DSN=IBMUSER.TARGET,DISP=OLD\n//SYSIN DD *\n DELETE IBMUSER.TARGET\n IF MAXCC LE 08 THEN SET MAXCC = 0\n DEFINE CLUSTER (NAME(IBMUSER.TARGET) NONINDEXED RECORDSIZE(6 6))\n REPRO INFILE(INPUT) OUTFILE(OUTPUT)\n/*\n".into(),
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
            vec![b"FIRST".to_vec(), b"SECOND".to_vec()]
        );
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
