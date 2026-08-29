use crate::{
    Disposition, JclBundle, JclLimits, JobPlan, ProgramInput, StepPlan, decode_program_output,
    parse_jcl,
};
use mainframe_env_execution_api::{
    BoundedPayload, IdempotencyKey, Invocation, InvocationLimits, PrincipalId,
};
use mainframe_env_host_api::{
    AccessIntent, DatasetAttributes, DatasetName, DatasetOrganization, DatasetRequest,
    EffectRequest, HostProblem, HostRequest, HostResult, Mutation, ProgramName, ProgramRequest,
    RecordFormat, ResourceName, ScopedHostService, SecurityDecision, SecurityRequest,
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
        for (index, step) in steps.iter().enumerate() {
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
            self.allocate_dds(invocation, job, step, index as u64 * 100 + 10)?;
            let input = ProgramInput {
                parameter: step.parameter.clone(),
                dds: step.dds.clone(),
            };
            let bytes = serde_json::to_vec(&input).map_err(|_| HostProblem::ProviderFailure)?;
            let payload = BoundedPayload::new(
                "mainframe-env.program.input@1",
                bytes,
                InvocationLimits::default(),
            )
            .map_err(|_| HostProblem::ResourceExhausted)?;
            let program =
                ProgramName::new(&step.program, 128).map_err(|_| HostProblem::Malformed)?;
            let sequence = index as u64 + 1000;
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
            self.dispose_dds(invocation, job, step, index as u64 * 100 + 50, abended)?;
            if abended {
                break;
            }
        }
        Ok(max_rc)
    }

    fn allocate_dds(
        &self,
        invocation: &Invocation,
        job: &Job,
        step: &StepPlan,
        base: u64,
    ) -> Result<(), HostProblem> {
        for (offset, dd) in step.dds.iter().enumerate() {
            let Some(raw_name) = &dd.dataset else {
                continue;
            };
            let name = resolved_dataset(job, dd, raw_name);
            self.authorize(
                invocation,
                "DATASET",
                &name,
                AccessIntent::Update,
                base + offset as u64 * 2,
            )?;
            if dd.disposition.contains(&Disposition::New) {
                let sequence = base + offset as u64 * 2 + 1;
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
                            attributes: DatasetAttributes {
                                organization: DatasetOrganization::Sequential,
                                record_format: RecordFormat::Variable,
                                logical_record_length: 32760,
                                key_offset: None,
                                key_length: None,
                                ccsid: Some(37),
                            },
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

    fn dispose_dds(
        &self,
        invocation: &Invocation,
        job: &Job,
        step: &StepPlan,
        base: u64,
        abnormal: bool,
    ) -> Result<(), HostProblem> {
        for (offset, dd) in step.dds.iter().enumerate() {
            let Some(raw_name) = &dd.dataset else {
                continue;
            };
            let delete = dd.disposition.contains(&Disposition::Delete)
                || (abnormal && dd.temporary)
                || (dd.temporary && !dd.disposition.contains(&Disposition::Pass));
            if !delete {
                continue;
            }
            let sequence = base + offset as u64 + 1;
            let key = effect_key(job, step, sequence)?;
            let request = DatasetRequest::Delete {
                dataset: DatasetName::new(resolved_dataset(job, dd, raw_name), 128)
                    .map_err(|_| HostProblem::Malformed)?,
                member: None,
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

fn resolved_dataset(job: &Job, dd: &crate::DdPlan, raw: &str) -> String {
    if raw.starts_with("&&") {
        format!("TMP.{}.{}", job.id, dd.name)
    } else {
        raw.to_ascii_uppercase()
    }
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
        CapabilityDescriptor, EffectResult, HostLimits, HostProvider, RegistrySnapshot,
    };
    use mainframe_env_store::{MemoryStore, SqliteStateStore};
    use std::collections::BTreeSet;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct SecurityProvider {
        descriptor: CapabilityDescriptor,
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
        Arc::new(ScopedHostService::new(
            Arc::new(RegistrySnapshot::new(1, vec![security, program], limits).unwrap()),
            HostLimits::default(),
        ))
    }

    fn invocation() -> Invocation {
        let limits = InvocationLimits::default();
        let grants = ["host.security.authorize", "host.program.invoke"]
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
        assert_eq!(
            service.run_next(&invocation, false).unwrap().unwrap().state,
            JobState::Completed
        );
        assert_eq!(
            service.spool(&submitted.id, "SYSPRINT", 0, 10).unwrap().0,
            vec![b"FIRST".to_vec(), b"SECOND".to_vec()]
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
