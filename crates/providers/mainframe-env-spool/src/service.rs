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

use crate::retention::{JobState, Replay, SPOOL_STATE_CONTRACT, STATE_NAMESPACE, decode_job_state};

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

struct DurableJob {
    version: u64,
    state: JobState,
}

pub struct SpoolService {
    store: Arc<dyn ProviderStateStore>,
    artifacts: Arc<dyn ArtifactStore>,
    limits: SpoolLimits,
    jobs: Mutex<BTreeMap<String, DurableJob>>,
    retention_clock: Option<Arc<dyn SpoolRetentionClock>>,
}

/// Trusted durable logical-time source sampled after physical purge.
pub trait SpoolRetentionClock: Send + Sync {
    /// Observe the current nonzero durable logical tick.
    fn now_tick(&self) -> Result<u64, HostProblem>;
}

impl SpoolService {
    pub fn open(
        store: Arc<dyn ProviderStateStore>,
        artifacts: Arc<dyn ArtifactStore>,
        limits: SpoolLimits,
    ) -> Result<Arc<Self>, HostProblem> {
        Self::open_inner(store, artifacts, limits, None)
    }

    /// Open with the durable clock used to age physically purged jobs.
    pub fn open_with_retention_clock(
        store: Arc<dyn ProviderStateStore>,
        artifacts: Arc<dyn ArtifactStore>,
        limits: SpoolLimits,
        retention_clock: Arc<dyn SpoolRetentionClock>,
    ) -> Result<Arc<Self>, HostProblem> {
        Self::open_inner(store, artifacts, limits, Some(retention_clock))
    }

    fn open_inner(
        store: Arc<dyn ProviderStateStore>,
        artifacts: Arc<dyn ArtifactStore>,
        limits: SpoolLimits,
        retention_clock: Option<Arc<dyn SpoolRetentionClock>>,
    ) -> Result<Arc<Self>, HostProblem> {
        validate_limits(limits)?;
        let jobs = load_jobs(&*store, &*artifacts, limits)?;
        Ok(Arc::new(Self {
            store,
            artifacts,
            limits,
            jobs: Mutex::new(jobs),
            retention_clock,
        }))
    }

    pub fn invoke(&self, request: SpoolRequest) -> Result<SpoolResult, HostProblem> {
        self.invoke_observed(request, None, None)
    }

    /// Invoke a spool request with a trusted, nonzero logical observation tick.
    ///
    /// The tick is recorded only when physical purge completes. Callers which
    /// cannot supply the product's durable clock should use [`Self::invoke`];
    /// those terminal rows remain protected from age-based retention.
    #[cfg(test)]
    fn invoke_at(
        &self,
        request: SpoolRequest,
        observed_tick: u64,
    ) -> Result<SpoolResult, HostProblem> {
        if observed_tick == 0 {
            return Err(HostProblem::Malformed);
        }
        self.invoke_observed(request, Some(observed_tick), Some(observed_tick))
    }

    fn invoke_with_boundary(
        &self,
        request: SpoolRequest,
        purge_boundary_tick: u64,
    ) -> Result<SpoolResult, HostProblem> {
        if purge_boundary_tick == 0 {
            return Err(HostProblem::Malformed);
        }
        self.invoke_observed(request, Some(purge_boundary_tick), None)
    }

    fn invoke_observed(
        &self,
        request: SpoolRequest,
        purge_boundary_tick: Option<u64>,
        test_observed_tick: Option<u64>,
    ) -> Result<SpoolResult, HostProblem> {
        HostRequest::Spool(request.clone()).validate(mainframe_env_host_api::HostLimits {
            max_record_bytes: self.limits.max_record_bytes,
            max_records: self.limits.max_records_per_file,
            ..Default::default()
        })?;
        // Provider rows can be pruned by another process. Never consult the
        // in-memory index until it reflects the current durable authority.
        self.refresh_after_external_retention()?;
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
            SpoolRequest::Purge { job, mutation } => self.purge(
                job.as_str(),
                mutation,
                purge_boundary_tick,
                test_observed_tick,
            ),
        }
    }

    /// Reload the durable job index after externally coordinated retention.
    ///
    /// The replacement is all-or-nothing: corrupt or incomplete durable rows
    /// leave the live cache untouched and fail closed.
    pub fn refresh_after_external_retention(&self) -> Result<(), HostProblem> {
        let mut jobs = self.lock()?;
        let next = load_jobs(&*self.store, &*self.artifacts, self.limits)?;
        *jobs = next;
        Ok(())
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
        next.schema_version = SPOOL_STATE_CONTRACT.into();
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
        // Chunks are content-addressed and may already be referenced by a
        // committed append. Leave an unreferenced chunk for purge/GC rather
        // than risk deleting live spool data when this publication fails.
        persist_job(
            &*self.store,
            job,
            version,
            current.map(|job| job.version),
            &next,
        )?;
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
        next.schema_version = SPOOL_STATE_CONTRACT.into();
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
        purge_boundary_tick: Option<u64>,
        test_observed_tick: Option<u64>,
    ) -> Result<SpoolResult, HostProblem> {
        let request_digest = format!("purge:{job}");
        let mut jobs = self.lock()?;
        let durable = jobs.get(job).ok_or(HostProblem::NotFound)?;
        if let Some(replay) = durable.state.replay.get(mutation.idempotency_key.as_str()) {
            return replay_result(replay, &request_digest);
        }
        let mut next = durable.state.clone();
        next.schema_version = SPOOL_STATE_CONTRACT.into();
        next.purge_pending = true;
        if let Some(tick) = purge_boundary_tick {
            next.purge_boundary_tick =
                Some(next.purge_boundary_tick.map_or(tick, |old| old.max(tick)));
        }
        next.purged_tick = None;
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
        let pending_state = next.clone();
        next.files.clear();
        next.purge_pending = false;
        next.purged = true;
        let observed_tick = match test_observed_tick {
            Some(tick) => Some(tick),
            None => match &self.retention_clock {
                Some(clock) => match clock.now_tick() {
                    Ok(tick) if tick != 0 => Some(tick),
                    _ => {
                        jobs.insert(
                            job.into(),
                            DurableJob {
                                version: intent_version,
                                state: pending_state,
                            },
                        );
                        return Err(HostProblem::UnknownOutcome);
                    }
                },
                None => None,
            },
        };
        if let Some(tick) = observed_tick {
            let tick = next
                .purge_boundary_tick
                .map_or(tick, |boundary| boundary.max(tick));
            next.purge_boundary_tick = Some(tick);
            next.purged_tick = Some(tick);
        }
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
        persist_job(&*self.store, job, version, Some(intent_version), &next)
            .map_err(|_| HostProblem::UnknownOutcome)?;
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

    fn invoke(&self, invocation: &Invocation, effect: EffectRequest) -> EffectResult {
        // Product invocations derive both deadlines from the durable logical
        // clock. The later boundary is conservative for age-based deletion.
        let observed_tick = invocation.deadline_tick.max(effect.deadline_tick);
        let outcome = match effect.request {
            HostRequest::Spool(request) => self
                .service
                .invoke_with_boundary(request, observed_tick)
                .map(HostResult::Spool),
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

fn load_jobs(
    store: &dyn ProviderStateStore,
    artifacts: &dyn ArtifactStore,
    limits: SpoolLimits,
) -> Result<BTreeMap<String, DurableJob>, HostProblem> {
    let mut jobs = BTreeMap::new();
    for record in store
        .list_provider_state(STATE_NAMESPACE, limits.max_jobs)
        .map_err(store_problem)?
    {
        let (state, _) =
            decode_job_state(&record, limits).map_err(|_| HostProblem::InfrastructureFailure)?;
        validate_artifacts(artifacts, &record.key, &state, limits)?;
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
    Ok(jobs)
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
    use mainframe_env_store::{MemoryStore, SqliteStateStore};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

    static NEXT_TEST_ROOT: AtomicU64 = AtomicU64::new(1);

    struct TestSpoolClock {
        tick: AtomicU64,
        fail_next: AtomicBool,
    }

    impl SpoolRetentionClock for TestSpoolClock {
        fn now_tick(&self) -> Result<u64, HostProblem> {
            if self.fail_next.swap(false, Ordering::SeqCst) {
                Err(HostProblem::InfrastructureFailure)
            } else {
                Ok(self.tick.load(Ordering::SeqCst))
            }
        }
    }

    struct RacingSpoolClock {
        store: Arc<MemoryStore>,
        job: String,
        race_next: AtomicBool,
        tick: u64,
    }

    impl SpoolRetentionClock for RacingSpoolClock {
        fn now_tick(&self) -> Result<u64, HostProblem> {
            if self.race_next.swap(false, Ordering::SeqCst) {
                let mut row = self
                    .store
                    .get_provider_state(STATE_NAMESPACE, &self.job)
                    .map_err(store_problem)?
                    .ok_or(HostProblem::InfrastructureFailure)?;
                let expected = row.version;
                row.version = row
                    .version
                    .checked_add(1)
                    .ok_or(HostProblem::ResourceExhausted)?;
                self.store
                    .put_provider_state(row, Some(expected))
                    .map_err(store_problem)?;
            }
            Ok(self.tick)
        }
    }

    struct TestRoot(PathBuf);

    impl TestRoot {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "mainframe-spool-retention-{}-{}",
                std::process::id(),
                NEXT_TEST_ROOT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn sqlite_url(&self) -> String {
            format!("sqlite://{}?mode=rwc", self.0.join("state.db").display())
        }
    }

    impl Drop for TestRoot {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

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

    struct RaceStateOnPutArtifactStore {
        inner: Arc<ProviderArtifactStore>,
        state: Arc<MemoryStore>,
        job: String,
        race: AtomicBool,
    }

    impl ArtifactStore for RaceStateOnPutArtifactStore {
        fn put_artifact(&self, record: ArtifactRecord) -> Result<(), StoreError> {
            if self.race.swap(false, Ordering::SeqCst) {
                let mut durable = self
                    .state
                    .get_provider_state(STATE_NAMESPACE, &self.job)?
                    .ok_or(StoreError::NotFound)?;
                let expected = durable.version;
                durable.version = durable.version.checked_add(1).ok_or(StoreError::Conflict)?;
                self.state.put_provider_state(durable, Some(expected))?;
            }
            self.inner.put_artifact(record)
        }

        fn get_artifact(&self, id: &ArtifactRef) -> Result<Option<ArtifactRecord>, StoreError> {
            self.inner.get_artifact(id)
        }

        fn delete_artifact(&self, id: &ArtifactRef) -> Result<(), StoreError> {
            self.inner.delete_artifact(id)
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
    fn failed_append_persist_never_deletes_an_existing_content_addressed_chunk() {
        let store = Arc::new(MemoryStore::new(Default::default()));
        let job = JobName::new("JOB00004", 128).unwrap();
        let provider_store: Arc<dyn ProviderStateStore> = store.clone();
        let artifacts =
            ProviderArtifactStore::new(provider_store.clone(), 4 * 1024 * 1024).unwrap();
        let racing = Arc::new(RaceStateOnPutArtifactStore {
            inner: artifacts,
            state: store.clone(),
            job: job.as_str().into(),
            race: AtomicBool::new(false),
        });
        let spool =
            SpoolService::open(provider_store, racing.clone(), SpoolLimits::default()).unwrap();
        spool
            .invoke(SpoolRequest::Append {
                job: job.clone(),
                file: "SYSOUT".into(),
                records: vec![b"ONE".to_vec()],
                mutation: mutation(1, "append-live"),
            })
            .unwrap();

        // Race after the invocation's durable refresh, so the next state CAS
        // still loses without making the content-addressed chunk disposable.
        // The second append deliberately addresses the same committed chunk.
        racing.race.store(true, Ordering::SeqCst);
        assert_eq!(
            spool.invoke(SpoolRequest::Append {
                job: job.clone(),
                file: "SYSOUT".into(),
                records: vec![b"ONE".to_vec()],
                mutation: mutation(1, "append-raced"),
            }),
            Err(HostProblem::IdempotencyConflict)
        );
        assert!(matches!(
            spool.invoke(SpoolRequest::Read {
                job,
                file: "SYSOUT".into(),
                start: 0,
                max_records: 8,
            }),
            Ok(SpoolResult::Records { records, .. }) if records == [b"ONE".to_vec()]
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

    #[test]
    fn two_live_services_observe_external_retention_and_reuse_capacity() {
        let store = Arc::new(MemoryStore::new(Default::default()));
        let provider_store: Arc<dyn ProviderStateStore> = store.clone();
        let artifacts = ProviderArtifactStore::new(provider_store.clone(), 1024 * 1024).unwrap();
        let limits = SpoolLimits {
            max_jobs: 1,
            ..Default::default()
        };
        let first = SpoolService::open(provider_store.clone(), artifacts.clone(), limits).unwrap();
        let second = SpoolService::open(provider_store.clone(), artifacts.clone(), limits).unwrap();
        let job = JobName::new("JOB00005", 128).unwrap();
        let append = SpoolRequest::Append {
            job: job.clone(),
            file: "SYSOUT".into(),
            records: vec![b"ONE".to_vec()],
            mutation: mutation(1, "append-before-retention"),
        };
        assert!(matches!(
            first.invoke(append.clone()),
            Ok(SpoolResult::Mutated {
                replayed: false,
                ..
            })
        ));
        assert!(matches!(
            second.invoke(append.clone()),
            Ok(SpoolResult::Mutated { replayed: true, .. })
        ));
        first
            .invoke_at(
                SpoolRequest::Purge {
                    job: job.clone(),
                    mutation: mutation(2, "purge-before-retention"),
                },
                100,
            )
            .unwrap();
        let terminal = store
            .get_provider_state(STATE_NAMESPACE, job.as_str())
            .unwrap()
            .unwrap();
        assert_eq!(
            crate::describe_spool_retention_row(&terminal, limits)
                .unwrap()
                .terminal_tick,
            Some(100)
        );
        store
            .delete_provider_state(STATE_NAMESPACE, job.as_str(), terminal.version)
            .unwrap();

        // The second process was open before deletion. Its ordinary next call
        // must not serve the expired replay from its old in-memory cache.
        assert!(matches!(
            second.invoke(append),
            Ok(SpoolResult::Mutated {
                replayed: false,
                ..
            })
        ));
        second
            .invoke_at(
                SpoolRequest::Purge {
                    job: job.clone(),
                    mutation: mutation(3, "purge-recreated"),
                },
                200,
            )
            .unwrap();
        let recreated = store
            .get_provider_state(STATE_NAMESPACE, job.as_str())
            .unwrap()
            .unwrap();
        store
            .delete_provider_state(STATE_NAMESPACE, job.as_str(), recreated.version)
            .unwrap();

        drop(first);
        drop(second);
        let restarted = SpoolService::open(provider_store, artifacts, limits).unwrap();
        let next = JobName::new("JOB00006", 128).unwrap();
        assert!(matches!(
            restarted.invoke(SpoolRequest::Append {
                job: next,
                file: "SYSOUT".into(),
                records: vec![b"TWO".to_vec()],
                mutation: mutation(1, "capacity-reused"),
            }),
            Ok(SpoolResult::Mutated {
                replayed: false,
                ..
            })
        ));
    }

    #[test]
    fn late_purge_recovery_advances_the_observation_boundary() {
        let store = Arc::new(MemoryStore::new(Default::default()));
        let provider_store: Arc<dyn ProviderStateStore> = store.clone();
        let artifacts = ProviderArtifactStore::new(provider_store.clone(), 1024 * 1024).unwrap();
        let failing = Arc::new(FailDeleteOnceArtifactStore {
            inner: artifacts,
            fail: AtomicBool::new(true),
        });
        let spool = SpoolService::open(provider_store, failing, SpoolLimits::default()).unwrap();
        let job = JobName::new("JOB00007", 128).unwrap();
        spool
            .invoke(SpoolRequest::Append {
                job: job.clone(),
                file: "SYSOUT".into(),
                records: vec![b"ONE".to_vec()],
                mutation: mutation(1, "append-before-late-purge"),
            })
            .unwrap();
        assert!(matches!(
            spool.invoke_at(
                SpoolRequest::Purge {
                    job: job.clone(),
                    mutation: mutation(2, "late-purge"),
                },
                100,
            ),
            Ok(SpoolResult::PurgePending { .. })
        ));
        spool
            .invoke_at(
                SpoolRequest::Purge {
                    job: job.clone(),
                    mutation: mutation(2, "late-purge"),
                },
                250,
            )
            .unwrap();
        let row = store
            .get_provider_state(STATE_NAMESPACE, job.as_str())
            .unwrap()
            .unwrap();
        let descriptor = crate::describe_spool_retention_row(&row, SpoolLimits::default()).unwrap();
        assert_eq!(descriptor.purge_boundary_tick, Some(250));
        assert_eq!(descriptor.terminal_tick, Some(250));
    }

    #[test]
    fn sqlite_restart_preserves_terminal_age_and_reuses_pruned_job_capacity() {
        let root = TestRoot::new();
        let url = root.sqlite_url();
        let limits = SpoolLimits {
            max_jobs: 1,
            ..Default::default()
        };
        let store = Arc::new(SqliteStateStore::open(&url, 4 * 1024 * 1024, 128).unwrap());
        let provider_store: Arc<dyn ProviderStateStore> = store.clone();
        let artifacts = ProviderArtifactStore::new(provider_store.clone(), 1024 * 1024).unwrap();
        let spool = SpoolService::open(provider_store, artifacts, limits).unwrap();
        let first = JobName::new("JOB00008", 128).unwrap();
        spool
            .invoke(SpoolRequest::Append {
                job: first.clone(),
                file: "SYSOUT".into(),
                records: vec![b"ONE".to_vec()],
                mutation: mutation(1, "sqlite-append"),
            })
            .unwrap();
        spool
            .invoke_at(
                SpoolRequest::Purge {
                    job: first.clone(),
                    mutation: mutation(2, "sqlite-purge"),
                },
                700,
            )
            .unwrap();
        drop(spool);
        drop(store);

        let reopened = Arc::new(SqliteStateStore::open(&url, 4 * 1024 * 1024, 128).unwrap());
        let provider_store: Arc<dyn ProviderStateStore> = reopened.clone();
        let artifacts = ProviderArtifactStore::new(provider_store.clone(), 1024 * 1024).unwrap();
        let spool = SpoolService::open(provider_store, artifacts, limits).unwrap();
        let terminal = reopened
            .get_provider_state(STATE_NAMESPACE, first.as_str())
            .unwrap()
            .unwrap();
        assert_eq!(
            crate::describe_spool_retention_row(&terminal, limits)
                .unwrap()
                .terminal_tick,
            Some(700)
        );
        assert_eq!(
            spool.invoke(SpoolRequest::Append {
                job: JobName::new("JOB00009", 128).unwrap(),
                file: "SYSOUT".into(),
                records: vec![b"BLOCKED".to_vec()],
                mutation: mutation(1, "before-prune"),
            }),
            Err(HostProblem::ResourceExhausted)
        );
        reopened
            .delete_provider_state(STATE_NAMESPACE, first.as_str(), terminal.version)
            .unwrap();
        assert!(matches!(
            spool.invoke(SpoolRequest::Append {
                job: JobName::new("JOB00009", 128).unwrap(),
                file: "SYSOUT".into(),
                records: vec![b"REUSED".to_vec()],
                mutation: mutation(1, "after-prune"),
            }),
            Ok(SpoolResult::Mutated {
                replayed: false,
                ..
            })
        ));
    }

    #[test]
    fn clock_failure_after_physical_purge_leaves_pending_and_retry_resolves_once() {
        let store = Arc::new(MemoryStore::new(Default::default()));
        let provider_store: Arc<dyn ProviderStateStore> = store.clone();
        let artifacts = ProviderArtifactStore::new(provider_store.clone(), 1024 * 1024).unwrap();
        let clock = Arc::new(TestSpoolClock {
            tick: AtomicU64::new(250),
            fail_next: AtomicBool::new(true),
        });
        let spool = SpoolService::open_with_retention_clock(
            provider_store,
            artifacts,
            SpoolLimits::default(),
            clock.clone(),
        )
        .unwrap();
        let job = JobName::new("JOBCLOCK", 128).unwrap();
        spool
            .invoke(SpoolRequest::Append {
                job: job.clone(),
                file: "SYSOUT".into(),
                records: vec![b"CLOCK".to_vec()],
                mutation: mutation(1, "clock-append"),
            })
            .unwrap();
        let purge = SpoolRequest::Purge {
            job: job.clone(),
            mutation: mutation(2, "clock-purge"),
        };
        assert_eq!(
            spool.invoke(purge.clone()),
            Err(HostProblem::UnknownOutcome)
        );
        let pending = store
            .get_provider_state(STATE_NAMESPACE, job.as_str())
            .unwrap()
            .unwrap();
        let pending =
            crate::describe_spool_retention_row(&pending, SpoolLimits::default()).unwrap();
        assert_eq!(pending.retention, crate::SpoolRetentionState::PurgeRecovery);
        spool.invoke(purge.clone()).unwrap();
        let terminal = store
            .get_provider_state(STATE_NAMESPACE, job.as_str())
            .unwrap()
            .unwrap();
        assert_eq!(
            crate::describe_spool_retention_row(&terminal, SpoolLimits::default())
                .unwrap()
                .terminal_tick,
            Some(250)
        );
        clock.tick.store(900, Ordering::SeqCst);
        spool.invoke(purge).unwrap();
        assert_eq!(
            store
                .get_provider_state(STATE_NAMESPACE, job.as_str())
                .unwrap()
                .unwrap(),
            terminal
        );
    }

    #[test]
    fn terminal_state_cas_failure_is_unknown_and_pending_retry_recovers() {
        let store = Arc::new(MemoryStore::new(Default::default()));
        let provider_store: Arc<dyn ProviderStateStore> = store.clone();
        let artifacts = ProviderArtifactStore::new(provider_store.clone(), 1024 * 1024).unwrap();
        let job = JobName::new("JOBCAS", 128).unwrap();
        let clock = Arc::new(RacingSpoolClock {
            store: store.clone(),
            job: job.as_str().into(),
            race_next: AtomicBool::new(true),
            tick: 300,
        });
        let spool = SpoolService::open_with_retention_clock(
            provider_store,
            artifacts,
            SpoolLimits::default(),
            clock,
        )
        .unwrap();
        spool
            .invoke(SpoolRequest::Append {
                job: job.clone(),
                file: "SYSOUT".into(),
                records: vec![b"CAS".to_vec()],
                mutation: mutation(1, "cas-append"),
            })
            .unwrap();
        let purge = SpoolRequest::Purge {
            job: job.clone(),
            mutation: mutation(2, "cas-purge"),
        };
        assert_eq!(
            spool.invoke(purge.clone()),
            Err(HostProblem::UnknownOutcome)
        );
        assert_eq!(
            crate::describe_spool_retention_row(
                &store
                    .get_provider_state(STATE_NAMESPACE, job.as_str())
                    .unwrap()
                    .unwrap(),
                SpoolLimits::default(),
            )
            .unwrap()
            .retention,
            crate::SpoolRetentionState::PurgeRecovery
        );
        spool.invoke(purge).unwrap();
        assert_eq!(
            crate::describe_spool_retention_row(
                &store
                    .get_provider_state(STATE_NAMESPACE, job.as_str())
                    .unwrap()
                    .unwrap(),
                SpoolLimits::default(),
            )
            .unwrap()
            .terminal_tick,
            Some(300)
        );
    }
}
