use mainframe_env_execution_api::{ArtifactRef, CapabilityId, Invocation, InvocationLimits};
use mainframe_env_host_api::{
    CapabilityDescriptor, EffectRequest, EffectResult, HostProblem, HostProvider, HostRequest,
    HostResult, SpoolFileSummary, SpoolRequest, SpoolResult,
};
use mainframe_env_store_api::{
    ArtifactRecord, ArtifactStore, ProviderStateRecord, ProviderStateStore, StoreError,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard};

pub const SPOOL_STATE_CONTRACT: &str = "mainframe-env.spool-state@1";
const STATE_NAMESPACE: &str = "jes-spool";
const ARTIFACT_NAMESPACE: &str = "spool-artifact";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SpoolLimits {
    pub max_jobs: usize,
    pub max_files_per_job: usize,
    pub max_records_per_file: usize,
    pub max_record_bytes: usize,
    pub max_bytes_per_job: usize,
    pub max_artifacts_per_file: usize,
    pub max_replays_per_job: usize,
}

impl Default for SpoolLimits {
    fn default() -> Self {
        Self {
            max_jobs: 16_384,
            max_files_per_job: 128,
            max_records_per_file: 262_144,
            max_record_bytes: 1024 * 1024,
            max_bytes_per_job: 256 * 1024 * 1024,
            max_artifacts_per_file: 262_144,
            max_replays_per_job: 65_536,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct Chunk {
    contract: String,
    job: String,
    file: String,
    sequence: u64,
    records: Vec<Vec<u8>>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
struct FileState {
    artifacts: Vec<String>,
    record_count: u64,
    byte_count: u64,
    sealed: bool,
    version: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct Replay {
    request_digest: String,
    result: SpoolResult,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct JobState {
    schema_version: String,
    files: BTreeMap<String, FileState>,
    replay: BTreeMap<String, Replay>,
    purge_pending: bool,
    purged: bool,
}

impl Default for JobState {
    fn default() -> Self {
        Self {
            schema_version: SPOOL_STATE_CONTRACT.into(),
            files: BTreeMap::new(),
            replay: BTreeMap::new(),
            purge_pending: false,
            purged: false,
        }
    }
}

struct DurableJob {
    version: u64,
    state: JobState,
}

pub struct SpoolService {
    store: Arc<dyn ProviderStateStore>,
    artifacts: Arc<dyn ArtifactStore>,
    limits: SpoolLimits,
    jobs: Mutex<BTreeMap<String, DurableJob>>,
}

impl SpoolService {
    pub fn open(
        store: Arc<dyn ProviderStateStore>,
        artifacts: Arc<dyn ArtifactStore>,
        limits: SpoolLimits,
    ) -> Result<Arc<Self>, HostProblem> {
        validate_limits(limits)?;
        let mut jobs = BTreeMap::new();
        for record in store
            .list_provider_state(STATE_NAMESPACE, limits.max_jobs)
            .map_err(store_problem)?
        {
            let state: JobState = serde_json::from_slice(&record.payload)
                .map_err(|_| HostProblem::InfrastructureFailure)?;
            validate_state(&state, limits)?;
            validate_artifacts(&*artifacts, &record.key, &state, limits)?;
            if jobs
                .insert(
                    record.key,
                    DurableJob {
                        version: record.version,
                        state,
                    },
                )
                .is_some()
            {
                return Err(HostProblem::InfrastructureFailure);
            }
        }
        Ok(Arc::new(Self {
            store,
            artifacts,
            limits,
            jobs: Mutex::new(jobs),
        }))
    }

    pub fn invoke(&self, request: SpoolRequest) -> Result<SpoolResult, HostProblem> {
        HostRequest::Spool(request.clone()).validate(mainframe_env_host_api::HostLimits {
            max_record_bytes: self.limits.max_record_bytes,
            max_records: self.limits.max_records_per_file,
            ..Default::default()
        })?;
        match request {
            SpoolRequest::Append {
                job,
                file,
                records,
                mutation,
            } => self.append(job.as_str(), &file, records, mutation),
            SpoolRequest::List { job } => self.list(job.as_str()),
            SpoolRequest::Read {
                job,
                file,
                start,
                max_records,
            } => self.read(job.as_str(), &file, start, max_records),
            SpoolRequest::Seal {
                job,
                file,
                mutation,
            } => self.seal(job.as_str(), &file, mutation),
            SpoolRequest::Purge { job, mutation } => self.purge(job.as_str(), mutation),
        }
    }

    fn append(
        &self,
        job: &str,
        file: &str,
        records: Vec<Vec<u8>>,
        mutation: mainframe_env_host_api::Mutation,
    ) -> Result<SpoolResult, HostProblem> {
        validate_identity(job, file)?;
        if records.is_empty()
            || records.len() > self.limits.max_records_per_file
            || records
                .iter()
                .any(|record| record.len() > self.limits.max_record_bytes)
        {
            return Err(HostProblem::ResourceExhausted);
        }
        let request_digest = append_digest(job, file, &records);
        let replay_key = mutation.idempotency_key.as_str();
        let mut jobs = self.lock()?;
        if let Some(replay) = jobs
            .get(job)
            .and_then(|durable| durable.state.replay.get(replay_key))
        {
            return replay_result(replay, &request_digest);
        }
        if !jobs.contains_key(job) && jobs.len() >= self.limits.max_jobs {
            return Err(HostProblem::ResourceExhausted);
        }
        let current = jobs.get(job);
        let mut next = current.map(|job| job.state.clone()).unwrap_or_default();
        if next.purged || next.purge_pending || next.replay.len() >= self.limits.max_replays_per_job
        {
            return Err(HostProblem::Condition {
                name: "SPOOL_CLOSED".into(),
                response: 409,
                response2: 0,
            });
        }
        if !next.files.contains_key(file) && next.files.len() >= self.limits.max_files_per_job {
            return Err(HostProblem::ResourceExhausted);
        }
        let new_records =
            u64::try_from(records.len()).map_err(|_| HostProblem::ResourceExhausted)?;
        let new_bytes = records
            .iter()
            .try_fold(0u64, |total, record| {
                total.checked_add(u64::try_from(record.len()).ok()?)
            })
            .ok_or(HostProblem::ResourceExhausted)?;
        let total_bytes = next
            .files
            .values()
            .try_fold(new_bytes, |total, file| total.checked_add(file.byte_count))
            .ok_or(HostProblem::ResourceExhausted)?;
        if usize::try_from(total_bytes).map_or(true, |bytes| bytes > self.limits.max_bytes_per_job)
        {
            return Err(HostProblem::ResourceExhausted);
        }
        let file_state = next.files.entry(file.into()).or_default();
        if file_state.sealed
            || file_state.artifacts.len() >= self.limits.max_artifacts_per_file
            || file_state
                .record_count
                .checked_add(new_records)
                .is_none_or(|count| {
                    usize::try_from(count)
                        .map_or(true, |count| count > self.limits.max_records_per_file)
                })
        {
            return Err(HostProblem::ResourceExhausted);
        }
        let chunk = Chunk {
            contract: "mainframe-env.spool-chunk@1".into(),
            job: job.into(),
            file: file.into(),
            sequence: mutation.sequence,
            records,
        };
        let payload = serde_json::to_vec(&chunk).map_err(|_| HostProblem::InfrastructureFailure)?;
        let digest: [u8; 32] = Sha256::digest(&payload).into();
        let artifact = ArtifactRef::new(
            format!("sha256:{}", hex(&digest)),
            InvocationLimits::default(),
        )
        .map_err(|_| HostProblem::InfrastructureFailure)?;
        self.artifacts
            .put_artifact(ArtifactRecord {
                artifact: artifact.clone(),
                media_type: "application/vnd.mainframe-env.spool-chunk+json".into(),
                payload_digest: digest,
                payload,
            })
            .map_err(store_problem)?;
        file_state.artifacts.push(artifact.as_str().into());
        file_state.record_count += new_records;
        file_state.byte_count += new_bytes;
        file_state.version = file_state.version.saturating_add(1).max(1);
        let version = current.map_or(1, |current| current.version.saturating_add(1));
        let result = SpoolResult::Mutated {
            version: file_state.version,
            replayed: false,
        };
        next.replay.insert(
            replay_key.into(),
            Replay {
                request_digest,
                result: result.clone(),
            },
        );
        if let Err(problem) = persist_job(
            &*self.store,
            job,
            version,
            current.map(|job| job.version),
            &next,
        ) {
            if self.artifacts.delete_artifact(&artifact).is_err() {
                return Err(HostProblem::UnknownOutcome);
            }
            return Err(problem);
        }
        jobs.insert(
            job.into(),
            DurableJob {
                version,
                state: next,
            },
        );
        Ok(result)
    }

    fn list(&self, job: &str) -> Result<SpoolResult, HostProblem> {
        let jobs = self.lock()?;
        let durable = jobs.get(job).ok_or(HostProblem::NotFound)?;
        if durable.state.purged {
            return Err(HostProblem::NotFound);
        }
        Ok(SpoolResult::Files {
            files: durable
                .state
                .files
                .iter()
                .map(|(file, state)| SpoolFileSummary {
                    file: file.clone(),
                    record_count: state.record_count,
                    byte_count: state.byte_count,
                    sealed: state.sealed,
                    version: state.version,
                })
                .collect(),
        })
    }

    fn read(
        &self,
        job: &str,
        file: &str,
        start: u64,
        max_records: u32,
    ) -> Result<SpoolResult, HostProblem> {
        if max_records == 0 || max_records as usize > self.limits.max_records_per_file {
            return Err(HostProblem::ResourceExhausted);
        }
        let jobs = self.lock()?;
        let state = jobs
            .get(job)
            .filter(|job| !job.state.purged)
            .and_then(|job| job.state.files.get(file))
            .ok_or(HostProblem::NotFound)?;
        let mut position = 0u64;
        let mut selected = Vec::new();
        for reference in &state.artifacts {
            let artifact = ArtifactRef::new(reference, InvocationLimits::default())
                .map_err(|_| HostProblem::InfrastructureFailure)?;
            let record = self
                .artifacts
                .get_artifact(&artifact)
                .map_err(store_problem)?
                .ok_or(HostProblem::UnknownOutcome)?;
            let chunk = decode_chunk(&record, &artifact)?;
            if chunk.contract != "mainframe-env.spool-chunk@1"
                || chunk.job != job
                || chunk.file != file
            {
                return Err(HostProblem::InfrastructureFailure);
            }
            for record in chunk.records {
                if position >= start && selected.len() < max_records as usize {
                    selected.push(record);
                }
                position = position.saturating_add(1);
            }
            if selected.len() == max_records as usize {
                break;
            }
        }
        Ok(SpoolResult::Records {
            records: selected,
            more: start.saturating_add(u64::from(max_records)) < state.record_count,
            version: state.version,
        })
    }

    fn seal(
        &self,
        job: &str,
        file: &str,
        mutation: mainframe_env_host_api::Mutation,
    ) -> Result<SpoolResult, HostProblem> {
        let request_digest = format!("seal:{job}:{file}");
        let mut jobs = self.lock()?;
        let durable = jobs.get(job).ok_or(HostProblem::NotFound)?;
        if let Some(replay) = durable.state.replay.get(mutation.idempotency_key.as_str()) {
            return replay_result(replay, &request_digest);
        }
        let mut next = durable.state.clone();
        if next.replay.len() >= self.limits.max_replays_per_job {
            return Err(HostProblem::ResourceExhausted);
        }
        let file_state = next.files.get_mut(file).ok_or(HostProblem::NotFound)?;
        file_state.sealed = true;
        file_state.version = file_state.version.saturating_add(1);
        let result = SpoolResult::Mutated {
            version: file_state.version,
            replayed: false,
        };
        next.replay.insert(
            mutation.idempotency_key.as_str().into(),
            Replay {
                request_digest,
                result: result.clone(),
            },
        );
        let version = durable.version.saturating_add(1);
        persist_job(&*self.store, job, version, Some(durable.version), &next)?;
        jobs.insert(
            job.into(),
            DurableJob {
                version,
                state: next,
            },
        );
        Ok(result)
    }

    fn purge(
        &self,
        job: &str,
        mutation: mainframe_env_host_api::Mutation,
    ) -> Result<SpoolResult, HostProblem> {
        let request_digest = format!("purge:{job}");
        let mut jobs = self.lock()?;
        let durable = jobs.get(job).ok_or(HostProblem::NotFound)?;
        if let Some(replay) = durable.state.replay.get(mutation.idempotency_key.as_str()) {
            return replay_result(replay, &request_digest);
        }
        let mut next = durable.state.clone();
        next.purge_pending = true;
        let intent_version = durable.version.saturating_add(1);
        persist_job(
            &*self.store,
            job,
            intent_version,
            Some(durable.version),
            &next,
        )?;
        let artifacts = next
            .files
            .values()
            .flat_map(|file| file.artifacts.iter())
            .cloned()
            .collect::<Vec<_>>();
        for (index, reference) in artifacts.iter().enumerate() {
            let artifact = ArtifactRef::new(reference, InvocationLimits::default())
                .map_err(|_| HostProblem::InfrastructureFailure)?;
            if !matches!(
                self.artifacts.delete_artifact(&artifact),
                Ok(()) | Err(StoreError::NotFound)
            ) {
                jobs.insert(
                    job.into(),
                    DurableJob {
                        version: intent_version,
                        state: next,
                    },
                );
                return Ok(SpoolResult::PurgePending {
                    remaining_artifacts: u64::try_from(artifacts.len() - index).unwrap_or(u64::MAX),
                });
            }
        }
        next.files.clear();
        next.purge_pending = false;
        next.purged = true;
        let result = SpoolResult::Mutated {
            version: intent_version.saturating_add(1),
            replayed: false,
        };
        next.replay.insert(
            mutation.idempotency_key.as_str().into(),
            Replay {
                request_digest,
                result: result.clone(),
            },
        );
        let version = intent_version.saturating_add(1);
        persist_job(&*self.store, job, version, Some(intent_version), &next)?;
        jobs.insert(
            job.into(),
            DurableJob {
                version,
                state: next,
            },
        );
        Ok(result)
    }

    fn lock(&self) -> Result<MutexGuard<'_, BTreeMap<String, DurableJob>>, HostProblem> {
        self.jobs
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)
    }
}

pub struct ProviderArtifactStore {
    store: Arc<dyn ProviderStateStore>,
    max_bytes: usize,
}

impl ProviderArtifactStore {
    pub fn new(
        store: Arc<dyn ProviderStateStore>,
        max_bytes: usize,
    ) -> Result<Arc<Self>, HostProblem> {
        if max_bytes == 0 {
            return Err(HostProblem::Malformed);
        }
        Ok(Arc::new(Self { store, max_bytes }))
    }
}

impl ArtifactStore for ProviderArtifactStore {
    fn put_artifact(&self, record: ArtifactRecord) -> Result<(), StoreError> {
        if record.payload.len() > self.max_bytes {
            return Err(StoreError::PayloadTooLarge);
        }
        if let Some(existing) = self.get_artifact(&record.artifact)? {
            return if existing == record {
                Ok(())
            } else {
                Err(StoreError::Conflict)
            };
        }
        self.store.put_provider_state(
            ProviderStateRecord {
                namespace: ARTIFACT_NAMESPACE.into(),
                key: record.artifact.as_str().into(),
                version: 1,
                payload: serde_json::to_vec(&ArtifactEnvelope::from(record))
                    .map_err(|_| StoreError::IncompatibleVersion)?,
            },
            None,
        )
    }

    fn get_artifact(&self, id: &ArtifactRef) -> Result<Option<ArtifactRecord>, StoreError> {
        self.store
            .get_provider_state(ARTIFACT_NAMESPACE, id.as_str())?
            .map(|record| {
                let envelope: ArtifactEnvelope = serde_json::from_slice(&record.payload)
                    .map_err(|_| StoreError::IncompatibleVersion)?;
                envelope.into_record(id)
            })
            .transpose()
    }

    fn delete_artifact(&self, id: &ArtifactRef) -> Result<(), StoreError> {
        let record = self
            .store
            .get_provider_state(ARTIFACT_NAMESPACE, id.as_str())?
            .ok_or(StoreError::NotFound)?;
        self.store
            .delete_provider_state(ARTIFACT_NAMESPACE, id.as_str(), record.version)
    }
}

#[derive(Deserialize, Serialize)]
struct ArtifactEnvelope {
    media_type: String,
    payload_digest: [u8; 32],
    payload: Vec<u8>,
}

impl From<ArtifactRecord> for ArtifactEnvelope {
    fn from(record: ArtifactRecord) -> Self {
        Self {
            media_type: record.media_type,
            payload_digest: record.payload_digest,
            payload: record.payload,
        }
    }
}

impl ArtifactEnvelope {
    fn into_record(self, id: &ArtifactRef) -> Result<ArtifactRecord, StoreError> {
        let digest: [u8; 32] = Sha256::digest(&self.payload).into();
        if digest != self.payload_digest || id.as_str() != format!("sha256:{}", hex(&digest)) {
            return Err(StoreError::IncompatibleVersion);
        }
        Ok(ArtifactRecord {
            artifact: id.clone(),
            media_type: self.media_type,
            payload_digest: self.payload_digest,
            payload: self.payload,
        })
    }
}

pub fn spool_providers(
    service: Arc<SpoolService>,
    limits: InvocationLimits,
) -> Vec<Arc<dyn HostProvider>> {
    ["host.spool.read", "host.spool.write"]
        .into_iter()
        .map(|capability| {
            Arc::new(SpoolProvider {
                descriptor: CapabilityDescriptor {
                    capability: CapabilityId::new(capability, limits)
                        .expect("spool capability identity is valid"),
                    provider_id: format!("mainframe-env-spool-{capability}"),
                    generation: "1".into(),
                    request_schema: mainframe_env_host_api::SPOOL_REQUEST_CONTRACT.into(),
                    result_schema: mainframe_env_host_api::SPOOL_RESULT_CONTRACT.into(),
                    max_request_bytes: 4 * 1024 * 1024,
                    max_result_bytes: 4 * 1024 * 1024,
                    ready: true,
                },
                service: service.clone(),
            }) as Arc<dyn HostProvider>
        })
        .collect()
}

struct SpoolProvider {
    descriptor: CapabilityDescriptor,
    service: Arc<SpoolService>,
}

impl HostProvider for SpoolProvider {
    fn descriptor(&self) -> &CapabilityDescriptor {
        &self.descriptor
    }

    fn invoke(&self, _: &Invocation, effect: EffectRequest) -> EffectResult {
        let outcome = match effect.request {
            HostRequest::Spool(request) => self.service.invoke(request).map(HostResult::Spool),
            _ => Err(HostProblem::Malformed),
        };
        EffectResult {
            sequence: effect.sequence,
            outcome,
        }
    }
}

fn validate_limits(limits: SpoolLimits) -> Result<(), HostProblem> {
    if limits.max_jobs == 0
        || limits.max_files_per_job == 0
        || limits.max_records_per_file == 0
        || limits.max_record_bytes == 0
        || limits.max_bytes_per_job == 0
        || limits.max_artifacts_per_file == 0
        || limits.max_replays_per_job == 0
    {
        Err(HostProblem::Malformed)
    } else {
        Ok(())
    }
}

fn validate_state(state: &JobState, limits: SpoolLimits) -> Result<(), HostProblem> {
    if state.schema_version != SPOOL_STATE_CONTRACT
        || state.files.len() > limits.max_files_per_job
        || state.replay.len() > limits.max_replays_per_job
        || state.purge_pending && state.purged
    {
        return Err(HostProblem::InfrastructureFailure);
    }
    let mut total_bytes = 0u64;
    for (name, file) in &state.files {
        validate_identity("JOB00000", name)?;
        if file.version == 0
            || file.artifacts.len() > limits.max_artifacts_per_file
            || usize::try_from(file.record_count)
                .map_or(true, |count| count > limits.max_records_per_file)
            || usize::try_from(file.byte_count)
                .map_or(true, |bytes| bytes > limits.max_bytes_per_job)
        {
            return Err(HostProblem::InfrastructureFailure);
        }
        total_bytes = total_bytes
            .checked_add(file.byte_count)
            .ok_or(HostProblem::InfrastructureFailure)?;
    }
    if usize::try_from(total_bytes).map_or(true, |bytes| bytes > limits.max_bytes_per_job) {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(())
}

fn validate_artifacts(
    artifacts: &dyn ArtifactStore,
    job: &str,
    state: &JobState,
    limits: SpoolLimits,
) -> Result<(), HostProblem> {
    for (file_name, file) in &state.files {
        let mut record_count = 0u64;
        let mut byte_count = 0u64;
        for reference in &file.artifacts {
            let artifact = ArtifactRef::new(reference, InvocationLimits::default())
                .map_err(|_| HostProblem::InfrastructureFailure)?;
            let Some(record) = artifacts.get_artifact(&artifact).map_err(store_problem)? else {
                if state.purge_pending {
                    continue;
                }
                return Err(HostProblem::UnknownOutcome);
            };
            let chunk = decode_chunk(&record, &artifact)?;
            if chunk.contract != "mainframe-env.spool-chunk@1"
                || chunk.job != job
                || chunk.file != *file_name
                || chunk.sequence == 0
                || chunk.records.len() > limits.max_records_per_file
                || chunk
                    .records
                    .iter()
                    .any(|record| record.len() > limits.max_record_bytes)
            {
                return Err(HostProblem::InfrastructureFailure);
            }
            record_count = record_count
                .checked_add(
                    u64::try_from(chunk.records.len())
                        .map_err(|_| HostProblem::InfrastructureFailure)?,
                )
                .ok_or(HostProblem::InfrastructureFailure)?;
            byte_count = chunk
                .records
                .iter()
                .try_fold(byte_count, |total, record| {
                    total.checked_add(u64::try_from(record.len()).ok()?)
                })
                .ok_or(HostProblem::InfrastructureFailure)?;
        }
        if !state.purge_pending
            && (record_count != file.record_count || byte_count != file.byte_count)
        {
            return Err(HostProblem::InfrastructureFailure);
        }
    }
    Ok(())
}

fn validate_identity(job: &str, file: &str) -> Result<(), HostProblem> {
    if job.is_empty()
        || job.len() > 128
        || file.is_empty()
        || file.len() > 128
        || file.chars().any(char::is_control)
    {
        Err(HostProblem::Malformed)
    } else {
        Ok(())
    }
}

fn decode_chunk(record: &ArtifactRecord, expected: &ArtifactRef) -> Result<Chunk, HostProblem> {
    let digest: [u8; 32] = Sha256::digest(&record.payload).into();
    if record.artifact != *expected
        || record.media_type != "application/vnd.mainframe-env.spool-chunk+json"
        || record.payload_digest != digest
        || expected.as_str() != format!("sha256:{}", hex(&digest))
    {
        return Err(HostProblem::InfrastructureFailure);
    }
    serde_json::from_slice(&record.payload).map_err(|_| HostProblem::InfrastructureFailure)
}

fn persist_job(
    store: &dyn ProviderStateStore,
    job: &str,
    version: u64,
    expected: Option<u64>,
    state: &JobState,
) -> Result<(), HostProblem> {
    store
        .put_provider_state(
            ProviderStateRecord {
                namespace: STATE_NAMESPACE.into(),
                key: job.into(),
                version,
                payload: serde_json::to_vec(state)
                    .map_err(|_| HostProblem::InfrastructureFailure)?,
            },
            expected,
        )
        .map_err(store_problem)
}

fn append_digest(job: &str, file: &str, records: &[Vec<u8>]) -> String {
    let mut digest = Sha256::new();
    digest.update(b"mainframe-env.spool-append@1");
    digest.update((job.len() as u64).to_be_bytes());
    digest.update(job.as_bytes());
    digest.update((file.len() as u64).to_be_bytes());
    digest.update(file.as_bytes());
    for record in records {
        digest.update((record.len() as u64).to_be_bytes());
        digest.update(record);
    }
    format!("sha256:{}", hex(&digest.finalize()))
}

fn replay_result(replay: &Replay, digest: &str) -> Result<SpoolResult, HostProblem> {
    if replay.request_digest != digest {
        return Err(HostProblem::IdempotencyConflict);
    }
    Ok(match &replay.result {
        SpoolResult::Mutated { version, .. } => SpoolResult::Mutated {
            version: *version,
            replayed: true,
        },
        result => result.clone(),
    })
}

fn store_problem(problem: StoreError) -> HostProblem {
    match problem {
        StoreError::Conflict | StoreError::AlreadyExists => HostProblem::IdempotencyConflict,
        StoreError::CapacityExceeded | StoreError::PayloadTooLarge => {
            HostProblem::ResourceExhausted
        }
        StoreError::NotFound => HostProblem::NotFound,
        StoreError::IncompatibleVersion => HostProblem::InfrastructureFailure,
        StoreError::InvalidTransition
        | StoreError::InvalidSequence
        | StoreError::LeaseConflict
        | StoreError::Poisoned
        | StoreError::Infrastructure(_) => HostProblem::InfrastructureFailure,
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use mainframe_env_execution_api::{IdempotencyKey, InvocationLimits};
    use mainframe_env_host_api::{JobName, Mutation};
    use mainframe_env_store::MemoryStore;
    use std::sync::atomic::{AtomicBool, Ordering};

    struct FailDeleteOnceArtifactStore {
        inner: Arc<ProviderArtifactStore>,
        fail: AtomicBool,
    }

    impl ArtifactStore for FailDeleteOnceArtifactStore {
        fn put_artifact(&self, record: ArtifactRecord) -> Result<(), StoreError> {
            self.inner.put_artifact(record)
        }

        fn get_artifact(&self, id: &ArtifactRef) -> Result<Option<ArtifactRecord>, StoreError> {
            self.inner.get_artifact(id)
        }

        fn delete_artifact(&self, id: &ArtifactRef) -> Result<(), StoreError> {
            if self.fail.swap(false, Ordering::SeqCst) {
                Err(StoreError::Infrastructure("injected-delete-failure".into()))
            } else {
                self.inner.delete_artifact(id)
            }
        }
    }

    fn mutation(sequence: u64, key: &str) -> Mutation {
        Mutation {
            sequence,
            idempotency_key: IdempotencyKey::new(key, InvocationLimits::default()).unwrap(),
            transaction: None,
        }
    }

    fn service(store: Arc<MemoryStore>) -> (Arc<SpoolService>, Arc<ProviderArtifactStore>) {
        let provider_store: Arc<dyn ProviderStateStore> = store.clone();
        let artifacts =
            ProviderArtifactStore::new(provider_store.clone(), 4 * 1024 * 1024).unwrap();
        (
            SpoolService::open(provider_store, artifacts.clone(), Default::default()).unwrap(),
            artifacts,
        )
    }

    #[test]
    fn append_read_seal_replay_restart_and_purge_are_exact() {
        let store = Arc::new(MemoryStore::new(Default::default()));
        let (spool, _) = service(store.clone());
        let job = JobName::new("JOB00001", 128).unwrap();
        let append = SpoolRequest::Append {
            job: job.clone(),
            file: "SYSOUT".into(),
            records: vec![b"ONE".to_vec(), b"TWO".to_vec()],
            mutation: mutation(1, "append-1"),
        };
        assert_eq!(
            spool.invoke(append.clone()),
            Ok(SpoolResult::Mutated {
                version: 1,
                replayed: false,
            })
        );
        assert_eq!(
            spool.invoke(append),
            Ok(SpoolResult::Mutated {
                version: 1,
                replayed: true,
            })
        );
        assert!(matches!(
            spool.invoke(SpoolRequest::Read {
                job: job.clone(),
                file: "SYSOUT".into(),
                start: 1,
                max_records: 1,
            }),
            Ok(SpoolResult::Records { records, more: false, version: 1 })
                if records == [b"TWO".to_vec()]
        ));
        assert!(matches!(
            spool.invoke(SpoolRequest::Seal {
                job: job.clone(),
                file: "SYSOUT".into(),
                mutation: mutation(2, "seal-1"),
            }),
            Ok(SpoolResult::Mutated { version: 2, .. })
        ));
        assert!(
            spool
                .invoke(SpoolRequest::Append {
                    job: job.clone(),
                    file: "SYSOUT".into(),
                    records: vec![b"THREE".to_vec()],
                    mutation: mutation(3, "append-2"),
                })
                .is_err()
        );

        drop(spool);
        let (restarted, _) = service(store);
        assert!(matches!(
            restarted.invoke(SpoolRequest::List { job: job.clone() }),
            Ok(SpoolResult::Files { files })
                if files.len() == 1 && files[0].record_count == 2 && files[0].sealed
        ));
        assert!(matches!(
            restarted.invoke(SpoolRequest::Purge {
                job: job.clone(),
                mutation: mutation(4, "purge-1"),
            }),
            Ok(SpoolResult::Mutated {
                replayed: false,
                ..
            })
        ));
        assert_eq!(
            restarted.invoke(SpoolRequest::List { job }),
            Err(HostProblem::NotFound)
        );
    }

    #[test]
    fn replay_conflict_and_bounds_fail_without_durable_drift() {
        let store = Arc::new(MemoryStore::new(Default::default()));
        let (spool, _) = service(store);
        let job = JobName::new("JOB00002", 128).unwrap();
        spool
            .invoke(SpoolRequest::Append {
                job: job.clone(),
                file: "A".into(),
                records: vec![b"ONE".to_vec()],
                mutation: mutation(1, "same-key"),
            })
            .unwrap();
        assert_eq!(
            spool.invoke(SpoolRequest::Append {
                job: job.clone(),
                file: "A".into(),
                records: vec![b"DIFFERENT".to_vec()],
                mutation: mutation(1, "same-key"),
            }),
            Err(HostProblem::IdempotencyConflict)
        );
        assert!(matches!(
            spool.invoke(SpoolRequest::List { job }),
            Ok(SpoolResult::Files { files }) if files[0].record_count == 1
        ));
    }

    #[test]
    fn purge_intent_survives_artifact_delete_failure_and_reconciles_after_restart() {
        let store = Arc::new(MemoryStore::new(Default::default()));
        let provider_store: Arc<dyn ProviderStateStore> = store.clone();
        let artifacts = ProviderArtifactStore::new(provider_store.clone(), 1024 * 1024).unwrap();
        let failing = Arc::new(FailDeleteOnceArtifactStore {
            inner: artifacts.clone(),
            fail: AtomicBool::new(true),
        });
        let spool = SpoolService::open(
            provider_store.clone(),
            failing.clone(),
            SpoolLimits::default(),
        )
        .unwrap();
        let job = JobName::new("JOB00003", 128).unwrap();
        spool
            .invoke(SpoolRequest::Append {
                job: job.clone(),
                file: "SYSOUT".into(),
                records: vec![b"ONE".to_vec()],
                mutation: mutation(1, "append-before-purge"),
            })
            .unwrap();
        assert_eq!(
            spool.invoke(SpoolRequest::Purge {
                job: job.clone(),
                mutation: mutation(2, "purge-after-failure"),
            }),
            Ok(SpoolResult::PurgePending {
                remaining_artifacts: 1,
            })
        );
        let intent = store
            .get_provider_state(STATE_NAMESPACE, job.as_str())
            .unwrap()
            .unwrap();
        let intent: JobState = serde_json::from_slice(&intent.payload).unwrap();
        assert!(intent.purge_pending);
        assert!(!intent.purged);
        drop(spool);

        let restarted =
            SpoolService::open(provider_store, failing, SpoolLimits::default()).unwrap();
        assert!(matches!(
            restarted.invoke(SpoolRequest::Purge {
                job: job.clone(),
                mutation: mutation(2, "purge-after-failure"),
            }),
            Ok(SpoolResult::Mutated { .. })
        ));
        assert_eq!(
            restarted.invoke(SpoolRequest::List { job }),
            Err(HostProblem::NotFound)
        );
        assert!(
            store
                .list_provider_state(ARTIFACT_NAMESPACE, 8)
                .unwrap()
                .is_empty()
        );
    }
}
