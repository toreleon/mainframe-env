use crate::durable::{
    AUDIT_NAMESPACE, decode_audit, encode_audit, encode_checkpoint, encode_effect, encode_event,
    encode_execution, encode_outbox, encode_work,
};
use crate::retention::{
    ForecastCounts, build_archive, cics_replay_metadata, forecast, replay_metadata, replay_schema,
    target_namespace, target_watermark, target_window_open, validate_request,
};
use crate::validation;
use mainframe_env_execution_api::{
    ArtifactRef, AuditRecord, ExecutionId, IdempotencyKey, LifecycleEvent,
};
use mainframe_env_store_api::{
    ArchivedRetentionRow, ArtifactRecord, ArtifactStore, ArtifactStoreHealth, AuditSink,
    CheckpointRecord, CheckpointStore, CoreRetentionDependencySnapshot, EffectRecord,
    EffectRecoveryLease, EffectState, EventStore, ExecutionRecord, ExecutionState, ExecutionStore,
    GenerationRecord, GenerationStore, IdempotencyStore, JournalStore, OutboxRecord, OutboxStore,
    ProviderRetentionDependency, ProviderRetentionObservationDeletion,
    ProviderRetentionObservationSource, ProviderRetentionRow, ProviderStateArchiveDeletion,
    ProviderStateArchiveReplacement, ProviderStateMutation, ProviderStateRecord,
    ProviderStateStore, ProviderStateWrite, RetentionAgeReconciliation, RetentionArchive,
    RetentionArchivePruneOutcome, RetentionArchivePruneReceipt, RetentionArchivePruneRequest,
    RetentionForecast, RetentionLegacyRow, RetentionObservation, RetentionPolicy, RetentionReceipt,
    RetentionReconciliationReceipt, RetentionRequest, RetentionStore, RetentionTarget,
    SessionRecord, SessionStore, StoreError, WorkRecord, WorkState, WorkStore,
};
use std::collections::BTreeMap;
use std::sync::{Mutex, MutexGuard};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StoreLimits {
    pub max_executions: usize,
    pub max_events: usize,
    pub max_events_per_execution: usize,
    pub max_work_items: usize,
    pub max_checkpoints: usize,
    pub max_sessions: usize,
    pub max_artifacts: usize,
    pub max_generations: usize,
    pub max_effects: usize,
    pub max_audits: usize,
    pub max_outbox: usize,
    pub max_provider_state: usize,
    pub max_blob_bytes: usize,
    pub max_total_blob_bytes: usize,
    /// Maximum source rows retained across all archive batches.
    pub max_retention_archive_rows: usize,
    /// Maximum source-payload bytes retained across all archive batches.
    pub max_retention_archive_bytes: u64,
}

impl Default for StoreLimits {
    fn default() -> Self {
        Self {
            max_executions: 4096,
            max_events: 262_144,
            max_events_per_execution: 65_536,
            max_work_items: 16_384,
            max_checkpoints: 4096,
            max_sessions: 4096,
            max_artifacts: 4096,
            max_generations: 256,
            max_effects: 262_144,
            max_audits: 262_144,
            max_outbox: 262_144,
            max_provider_state: 262_144,
            max_blob_bytes: 64 * 1024 * 1024,
            max_total_blob_bytes: 512 * 1024 * 1024,
            max_retention_archive_rows: 262_144,
            max_retention_archive_bytes: 512 * 1024 * 1024,
        }
    }
}

#[derive(Clone, Default)]
struct State {
    executions: BTreeMap<ExecutionId, ExecutionRecord>,
    events: BTreeMap<ExecutionId, Vec<LifecycleEvent>>,
    work: BTreeMap<String, WorkRecord>,
    checkpoints: BTreeMap<ExecutionId, CheckpointRecord>,
    sessions: BTreeMap<String, SessionRecord>,
    artifacts: BTreeMap<ArtifactRef, ArtifactRecord>,
    generations: BTreeMap<String, GenerationRecord>,
    effects: BTreeMap<IdempotencyKey, EffectRecord>,
    audits: BTreeMap<String, AuditRecord>,
    next_audit_ordinal: u64,
    outbox: BTreeMap<String, OutboxRecord>,
    provider_state: BTreeMap<(String, String), ProviderStateRecord>,
    blob_bytes: usize,
    archives: BTreeMap<String, RetentionArchive>,
    archive_bytes: u64,
    observations: BTreeMap<(RetentionTarget, String, String), (u64, RetentionObservation)>,
    observation_bytes: u64,
    retention_cas_versions: BTreeMap<(RetentionTarget, String), u64>,
    provider_epoch: u64,
    logical_tick: u64,
}

pub struct MemoryStore {
    limits: StoreLimits,
    state: Mutex<State>,
}

impl MemoryStore {
    #[must_use]
    pub fn new(limits: StoreLimits) -> Self {
        Self {
            limits,
            state: Mutex::new(State::default()),
        }
    }

    fn lock(&self) -> Result<MutexGuard<'_, State>, StoreError> {
        self.state.lock().map_err(|_| StoreError::Poisoned)
    }

    fn reserve_blob(
        state: &mut State,
        old: usize,
        new: usize,
        limits: StoreLimits,
    ) -> Result<(), StoreError> {
        if new > limits.max_blob_bytes {
            return Err(StoreError::PayloadTooLarge);
        }
        let total = state
            .blob_bytes
            .checked_sub(old)
            .and_then(|value| value.checked_add(new))
            .ok_or(StoreError::CapacityExceeded)?;
        if total > limits.max_total_blob_bytes {
            return Err(StoreError::CapacityExceeded);
        }
        state.blob_bytes = total;
        Ok(())
    }

    fn bump_retention_epoch(state: &mut State) -> Result<(), StoreError> {
        state.provider_epoch = state
            .provider_epoch
            .checked_add(1)
            .ok_or(StoreError::CapacityExceeded)?;
        Ok(())
    }

    fn validate_encoded_size(bytes: Vec<u8>, limits: StoreLimits) -> Result<(), StoreError> {
        if bytes.len() > limits.max_blob_bytes {
            Err(StoreError::PayloadTooLarge)
        } else {
            Ok(())
        }
    }

    fn put_provider_state_locked(
        state: &mut State,
        record: ProviderStateRecord,
        expected_version: Option<u64>,
        limits: StoreLimits,
    ) -> Result<(), StoreError> {
        record.validate_write(limits.max_blob_bytes)?;
        let key = (record.namespace.clone(), record.key.clone());
        let current = state.provider_state.get(&key);
        match (current, expected_version) {
            (None, None) if record.version == 1 => {}
            (Some(current), Some(expected))
                if current.version == expected && record.version == expected + 1 => {}
            _ => return Err(StoreError::Conflict),
        }
        let old = current.map_or(0, |item| item.payload.len());
        if old == 0 && state.provider_state.len() >= limits.max_provider_state {
            return Err(StoreError::CapacityExceeded);
        }
        let next_epoch = state
            .provider_epoch
            .checked_add(1)
            .ok_or(StoreError::CapacityExceeded)?;
        Self::reserve_blob(state, old, record.payload.len(), limits)?;
        state.provider_state.insert(key, record);
        state.provider_epoch = next_epoch;
        Ok(())
    }

    fn append_event_locked(
        state: &mut State,
        event: LifecycleEvent,
        limits: StoreLimits,
    ) -> Result<(), StoreError> {
        validation::event(&event)?;
        Self::validate_encoded_size(encode_event(&event)?, limits)?;
        let total: usize = state.events.values().map(Vec::len).sum();
        if total >= limits.max_events {
            return Err(StoreError::CapacityExceeded);
        }
        let events = state.events.entry(event.execution_id.clone()).or_default();
        if events.len() >= limits.max_events_per_execution {
            return Err(StoreError::CapacityExceeded);
        }
        let expected = events.last().map_or(1, |last| last.sequence + 1);
        if event.sequence != expected {
            return Err(StoreError::InvalidSequence);
        }
        events.push(event);
        Self::bump_retention_epoch(state)?;
        Ok(())
    }

    fn append_outbox_locked(
        state: &mut State,
        record: OutboxRecord,
        limits: StoreLimits,
    ) -> Result<(), StoreError> {
        validation::new_outbox(&record)?;
        Self::validate_encoded_size(encode_outbox(&record)?, limits)?;
        if let Some(existing) = state.outbox.get(&record.notification_id) {
            return if existing == &record {
                Ok(())
            } else {
                Err(StoreError::Conflict)
            };
        }
        if state.outbox.len() >= limits.max_outbox {
            return Err(StoreError::CapacityExceeded);
        }
        Self::reserve_blob(state, 0, record.payload.len(), limits)?;
        state.outbox.insert(record.notification_id.clone(), record);
        Self::bump_retention_epoch(state)?;
        Ok(())
    }

    fn append_audit_locked(
        state: &mut State,
        record: AuditRecord,
        limits: StoreLimits,
    ) -> Result<(), StoreError> {
        validation::audit(&record)?;
        Self::validate_encoded_size(encode_audit(&record)?, limits)?;
        if state.audits.len() >= limits.max_audits {
            return Err(StoreError::CapacityExceeded);
        }
        state.next_audit_ordinal = state
            .next_audit_ordinal
            .checked_add(1)
            .ok_or(StoreError::CapacityExceeded)?;
        let key = crate::durable::audit_storage_key(
            &record.execution_id,
            &format!("memory:{:020}", state.next_audit_ordinal),
        );
        state.audits.insert(key, record);
        Self::bump_retention_epoch(state)?;
        Ok(())
    }
}

impl AuditSink for MemoryStore {
    fn record_audit(&self, record: AuditRecord) -> Result<(), StoreError> {
        let mut state = self.lock()?;
        Self::append_audit_locked(&mut state, record, self.limits)
    }

    fn audit_records(
        &self,
        execution_id: &ExecutionId,
        start_effect_sequence: u64,
        max: usize,
    ) -> Result<Vec<AuditRecord>, StoreError> {
        if max == 0 || max > self.limits.max_audits {
            return Err(StoreError::CapacityExceeded);
        }
        let mut records = self
            .lock()?
            .audits
            .values()
            .filter(|record| {
                &record.execution_id == execution_id
                    && record.effect_sequence >= start_effect_sequence
            })
            .cloned()
            .collect::<Vec<_>>();
        records.sort_by(|left, right| {
            (left.attempt, left.effect_sequence, &left.invocation_key).cmp(&(
                right.attempt,
                right.effect_sequence,
                &right.invocation_key,
            ))
        });
        records.truncate(max);
        Ok(records)
    }
}

impl ExecutionStore for MemoryStore {
    fn create_execution(&self, record: ExecutionRecord) -> Result<(), StoreError> {
        validation::new_execution(&record)?;
        Self::validate_encoded_size(encode_execution(&record)?, self.limits)?;
        let mut state = self.lock()?;
        if state.executions.contains_key(&record.execution_id) {
            return Err(StoreError::AlreadyExists);
        }
        if state.executions.len() >= self.limits.max_executions {
            return Err(StoreError::CapacityExceeded);
        }
        state.executions.insert(record.execution_id.clone(), record);
        Self::bump_retention_epoch(&mut state)?;
        Ok(())
    }

    fn get_execution(&self, id: &ExecutionId) -> Result<Option<ExecutionRecord>, StoreError> {
        Ok(self.lock()?.executions.get(id).cloned())
    }

    fn transition_execution(
        &self,
        id: &ExecutionId,
        expected_version: u64,
        next: ExecutionState,
        now_tick: u64,
    ) -> Result<ExecutionRecord, StoreError> {
        let mut state = self.lock()?;
        let record = state.executions.get_mut(id).ok_or(StoreError::NotFound)?;
        if record.version != expected_version {
            return Err(StoreError::Conflict);
        }
        if !record.state.can_transition_to(next) {
            return Err(StoreError::InvalidTransition);
        }
        record.state = next;
        record.terminal_tick = next
            .terminal()
            .then_some(now_tick)
            .filter(|tick| *tick != 0);
        record.version = record.version.checked_add(1).ok_or(StoreError::Conflict)?;
        Self::validate_encoded_size(encode_execution(record)?, self.limits)?;
        let updated = record.clone();
        Self::bump_retention_epoch(&mut state)?;
        Ok(updated)
    }
}

impl EventStore for MemoryStore {
    fn append_event(&self, event: LifecycleEvent) -> Result<(), StoreError> {
        let mut state = self.lock()?;
        Self::append_event_locked(&mut state, event, self.limits)
    }

    fn events(
        &self,
        id: &ExecutionId,
        start_sequence: u64,
        max: usize,
    ) -> Result<Vec<LifecycleEvent>, StoreError> {
        if max == 0 || max > self.limits.max_events_per_execution {
            return Err(StoreError::CapacityExceeded);
        }
        Ok(self
            .lock()?
            .events
            .get(id)
            .into_iter()
            .flatten()
            .filter(|event| event.sequence >= start_sequence)
            .take(max)
            .cloned()
            .collect())
    }
}

impl WorkStore for MemoryStore {
    fn get_work(&self, work_id: &str) -> Result<Option<WorkRecord>, StoreError> {
        if work_id.is_empty() {
            return Err(StoreError::InvalidTransition);
        }
        Ok(self.lock()?.work.get(work_id).cloned())
    }

    fn enqueue(&self, work: WorkRecord) -> Result<(), StoreError> {
        if work.work_id.is_empty()
            || work.attempt != 0
            || work.lease_epoch != 0
            || work.max_attempts == 0
            || work.deadline_tick == 0
            || work.required_generation.is_empty()
            || work.state != WorkState::Queued
            || work.worker_id.is_some()
            || work.lease_id.is_some()
            || work.lease_expiry_tick.is_some()
            || work.heartbeat_tick.is_some()
            || work.terminal_tick.is_some()
            || work.payload.len() > self.limits.max_blob_bytes
        {
            return Err(StoreError::InvalidTransition);
        }
        Self::validate_encoded_size(encode_work(&work)?, self.limits)?;
        let mut state = self.lock()?;
        if state.work.contains_key(&work.work_id) {
            return Err(StoreError::AlreadyExists);
        }
        if state.work.len() >= self.limits.max_work_items {
            return Err(StoreError::CapacityExceeded);
        }
        Self::reserve_blob(&mut state, 0, work.payload.len(), self.limits)?;
        state.work.insert(work.work_id.clone(), work);
        Self::bump_retention_epoch(&mut state)?;
        Ok(())
    }

    fn claim(
        &self,
        worker: &str,
        required_generation: Option<&str>,
        now_tick: u64,
        lease_ticks: u64,
    ) -> Result<Option<WorkRecord>, StoreError> {
        if worker.is_empty()
            || lease_ticks == 0
            || required_generation.is_some_and(|generation| {
                generation.is_empty()
                    || generation.len() > 128
                    || generation.chars().any(char::is_control)
            })
        {
            return Err(StoreError::LeaseConflict);
        }
        let mut state = self.lock()?;
        let mut mutated = false;
        for work in state.work.values_mut() {
            if required_generation.is_some_and(|generation| work.required_generation != generation)
            {
                continue;
            }
            if work.state == WorkState::Claimed
                && (work
                    .lease_expiry_tick
                    .is_none_or(|expiry| expiry <= now_tick)
                    || work.deadline_tick <= now_tick)
            {
                work.state = if work.cancellation_requested {
                    WorkState::Cancelled
                } else if work.attempt >= work.max_attempts || work.deadline_tick <= now_tick {
                    WorkState::DeadLetter
                } else {
                    WorkState::Queued
                };
                work.terminal_tick = work
                    .state
                    .terminal()
                    .then_some(now_tick)
                    .filter(|tick| *tick != 0);
                clear_lease(work);
                mutated = true;
            } else if work.state == WorkState::Queued && work.deadline_tick <= now_tick {
                work.state = WorkState::DeadLetter;
                work.terminal_tick = (now_tick != 0).then_some(now_tick);
                clear_lease(work);
                mutated = true;
            }
        }
        let candidate = state
            .work
            .values()
            .filter(|work| {
                required_generation.is_none_or(|generation| work.required_generation == generation)
                    && work.state == WorkState::Queued
                    && work.available_tick <= now_tick
                    && work.deadline_tick > now_tick
            })
            .min_by(|left, right| {
                right
                    .priority
                    .cmp(&left.priority)
                    .then_with(|| left.available_tick.cmp(&right.available_tick))
                    .then_with(|| left.work_id.cmp(&right.work_id))
            })
            .map(|work| work.work_id.clone());
        let Some(work) = candidate.and_then(|work_id| state.work.get_mut(&work_id)) else {
            if mutated {
                Self::bump_retention_epoch(&mut state)?;
            }
            return Ok(None);
        };
        work.attempt = work.attempt.checked_add(1).ok_or(StoreError::Conflict)?;
        work.lease_epoch = work
            .lease_epoch
            .checked_add(1)
            .ok_or(StoreError::Conflict)?;
        work.state = WorkState::Claimed;
        work.terminal_tick = None;
        work.worker_id = Some(worker.into());
        work.lease_id = Some(format!("{worker}:{}", work.lease_epoch));
        work.lease_expiry_tick = Some(
            now_tick
                .checked_add(lease_ticks)
                .ok_or(StoreError::Conflict)?
                .min(work.deadline_tick),
        );
        work.heartbeat_tick = Some(now_tick);
        Self::validate_encoded_size(encode_work(work)?, self.limits)?;
        let claimed = work.clone();
        Self::bump_retention_epoch(&mut state)?;
        Ok(Some(claimed))
    }

    fn heartbeat(
        &self,
        work_id: &str,
        lease_id: &str,
        lease_epoch: u64,
        now_tick: u64,
        lease_ticks: u64,
    ) -> Result<WorkRecord, StoreError> {
        if lease_ticks == 0 {
            return Err(StoreError::LeaseConflict);
        }
        let mut state = self.lock()?;
        let work = state.work.get_mut(work_id).ok_or(StoreError::NotFound)?;
        valid_lease(work, lease_id, lease_epoch, now_tick)?;
        work.heartbeat_tick = Some(now_tick);
        work.lease_expiry_tick = Some(
            now_tick
                .checked_add(lease_ticks)
                .ok_or(StoreError::LeaseConflict)?
                .min(work.deadline_tick),
        );
        Self::validate_encoded_size(encode_work(work)?, self.limits)?;
        let updated = work.clone();
        Self::bump_retention_epoch(&mut state)?;
        Ok(updated)
    }

    fn release(
        &self,
        work_id: &str,
        lease_id: &str,
        lease_epoch: u64,
        now_tick: u64,
        available_tick: u64,
    ) -> Result<WorkRecord, StoreError> {
        let mut state = self.lock()?;
        let work = state.work.get_mut(work_id).ok_or(StoreError::NotFound)?;
        valid_lease(work, lease_id, lease_epoch, now_tick)?;
        work.state = if work.cancellation_requested {
            WorkState::Cancelled
        } else if work.attempt >= work.max_attempts || available_tick >= work.deadline_tick {
            WorkState::DeadLetter
        } else {
            WorkState::Queued
        };
        work.terminal_tick = work
            .state
            .terminal()
            .then_some(now_tick)
            .filter(|tick| *tick != 0);
        work.available_tick = available_tick;
        clear_lease(work);
        Self::validate_encoded_size(encode_work(work)?, self.limits)?;
        let updated = work.clone();
        Self::bump_retention_epoch(&mut state)?;
        Ok(updated)
    }

    fn request_cancellation(&self, work_id: &str) -> Result<WorkRecord, StoreError> {
        let mut state = self.lock()?;
        let work = state.work.get_mut(work_id).ok_or(StoreError::NotFound)?;
        work.cancellation_requested = true;
        if work.state == WorkState::Queued {
            work.state = WorkState::Cancelled;
            // This compatibility API has no observation tick, so cancellation remains
            // protected until an operator attaches a CAS-fenced age.
            work.terminal_tick = None;
        }
        Self::validate_encoded_size(encode_work(work)?, self.limits)?;
        let updated = work.clone();
        Self::bump_retention_epoch(&mut state)?;
        Ok(updated)
    }

    fn dead_letter(
        &self,
        work_id: &str,
        lease_id: &str,
        lease_epoch: u64,
        now_tick: u64,
    ) -> Result<WorkRecord, StoreError> {
        let mut state = self.lock()?;
        let work = state.work.get_mut(work_id).ok_or(StoreError::NotFound)?;
        valid_lease(work, lease_id, lease_epoch, now_tick)?;
        work.state = WorkState::DeadLetter;
        work.terminal_tick = (now_tick != 0).then_some(now_tick);
        clear_lease(work);
        Self::validate_encoded_size(encode_work(work)?, self.limits)?;
        let updated = work.clone();
        Self::bump_retention_epoch(&mut state)?;
        Ok(updated)
    }

    fn complete(
        &self,
        work_id: &str,
        lease_id: &str,
        lease_epoch: u64,
        now_tick: u64,
    ) -> Result<(), StoreError> {
        let mut state = self.lock()?;
        let work = state.work.get_mut(work_id).ok_or(StoreError::NotFound)?;
        valid_lease(work, lease_id, lease_epoch, now_tick)?;
        work.state = WorkState::Completed;
        work.terminal_tick = (now_tick != 0).then_some(now_tick);
        clear_lease(work);
        Self::validate_encoded_size(encode_work(work)?, self.limits)?;
        Self::bump_retention_epoch(&mut state)?;
        Ok(())
    }
}

fn valid_lease(
    work: &WorkRecord,
    lease_id: &str,
    lease_epoch: u64,
    now_tick: u64,
) -> Result<(), StoreError> {
    if work.state == WorkState::Claimed
        && lease_epoch != 0
        && work.lease_epoch == lease_epoch
        && work.lease_id.as_deref() == Some(lease_id)
        && work
            .lease_expiry_tick
            .is_some_and(|expiry| expiry > now_tick)
        && work.deadline_tick > now_tick
        && work.heartbeat_tick.is_some_and(|tick| tick <= now_tick)
    {
        Ok(())
    } else {
        Err(StoreError::LeaseConflict)
    }
}

fn clear_lease(work: &mut WorkRecord) {
    work.worker_id = None;
    work.lease_id = None;
    work.lease_expiry_tick = None;
    work.heartbeat_tick = None;
}

impl CheckpointStore for MemoryStore {
    fn put_checkpoint(&self, record: CheckpointRecord) -> Result<(), StoreError> {
        validation::checkpoint(&record)?;
        Self::validate_encoded_size(encode_checkpoint(&record)?, self.limits)?;
        let mut state = self.lock()?;
        let old = state
            .checkpoints
            .get(&record.execution_id)
            .map_or(0, |item| item.payload.len());
        if old == 0 && state.checkpoints.len() >= self.limits.max_checkpoints {
            return Err(StoreError::CapacityExceeded);
        }
        Self::reserve_blob(&mut state, old, record.payload.len(), self.limits)?;
        state
            .checkpoints
            .insert(record.execution_id.clone(), record);
        Self::bump_retention_epoch(&mut state)?;
        Ok(())
    }
    fn get_checkpoint(&self, id: &ExecutionId) -> Result<Option<CheckpointRecord>, StoreError> {
        Ok(self.lock()?.checkpoints.get(id).cloned())
    }

    fn delete_checkpoint(&self, id: &ExecutionId) -> Result<(), StoreError> {
        let mut state = self.lock()?;
        let checkpoint = state.checkpoints.remove(id).ok_or(StoreError::NotFound)?;
        state.blob_bytes = state
            .blob_bytes
            .checked_sub(checkpoint.payload.len())
            .ok_or(StoreError::IncompatibleVersion)?;
        Self::bump_retention_epoch(&mut state)?;
        Ok(())
    }
}

impl SessionStore for MemoryStore {
    fn put_session(
        &self,
        record: SessionRecord,
        expected_version: Option<u64>,
    ) -> Result<(), StoreError> {
        if record.schema_version == 0 || record.version == 0 {
            return Err(StoreError::IncompatibleVersion);
        }
        let mut state = self.lock()?;
        let current = state.sessions.get(&record.session_id);
        match (current, expected_version) {
            (None, None) if record.version == 1 => {}
            (Some(current), Some(expected))
                if current.version == expected && record.version == expected + 1 => {}
            _ => return Err(StoreError::Conflict),
        }
        let old = current.map_or(0, |item| item.payload.len());
        if old == 0 && state.sessions.len() >= self.limits.max_sessions {
            return Err(StoreError::CapacityExceeded);
        }
        Self::reserve_blob(&mut state, old, record.payload.len(), self.limits)?;
        state.sessions.insert(record.session_id.clone(), record);
        Ok(())
    }
    fn get_session(&self, id: &str) -> Result<Option<SessionRecord>, StoreError> {
        Ok(self.lock()?.sessions.get(id).cloned())
    }
}

impl ArtifactStore for MemoryStore {
    fn health(&self) -> Result<ArtifactStoreHealth, StoreError> {
        let state = self.lock()?;
        Ok(ArtifactStoreHealth {
            readable: true,
            writable: true,
            used_objects: Some(state.artifacts.len()),
            max_objects: Some(self.limits.max_artifacts),
            used_bytes: Some(state.blob_bytes),
            max_bytes: Some(self.limits.max_total_blob_bytes),
        })
    }

    fn put_artifact(&self, record: ArtifactRecord) -> Result<(), StoreError> {
        validation::artifact(&record)?;
        let mut state = self.lock()?;
        if let Some(existing) = state.artifacts.get(&record.artifact) {
            return if existing == &record {
                Ok(())
            } else {
                Err(StoreError::Conflict)
            };
        }
        if state.artifacts.len() >= self.limits.max_artifacts {
            return Err(StoreError::CapacityExceeded);
        }
        Self::reserve_blob(&mut state, 0, record.payload.len(), self.limits)?;
        state.artifacts.insert(record.artifact.clone(), record);
        Ok(())
    }
    fn get_artifact(&self, id: &ArtifactRef) -> Result<Option<ArtifactRecord>, StoreError> {
        Ok(self.lock()?.artifacts.get(id).cloned())
    }

    fn delete_artifact(&self, id: &ArtifactRef) -> Result<(), StoreError> {
        let mut state = self.lock()?;
        let record = state.artifacts.remove(id).ok_or(StoreError::NotFound)?;
        state.blob_bytes = state
            .blob_bytes
            .checked_sub(record.payload.len())
            .ok_or(StoreError::IncompatibleVersion)?;
        Ok(())
    }
}

impl GenerationStore for MemoryStore {
    fn publish_generation(&self, record: GenerationRecord) -> Result<(), StoreError> {
        if record.provider.is_empty() || record.generation.is_empty() || record.version == 0 {
            return Err(StoreError::IncompatibleVersion);
        }
        let mut state = self.lock()?;
        if let Some(existing) = state.generations.get(&record.provider) {
            if record.version <= existing.version {
                return Err(StoreError::Conflict);
            }
        } else if state.generations.len() >= self.limits.max_generations {
            return Err(StoreError::CapacityExceeded);
        }
        state.generations.insert(record.provider.clone(), record);
        Ok(())
    }
    fn generation(&self, provider: &str) -> Result<Option<GenerationRecord>, StoreError> {
        Ok(self.lock()?.generations.get(provider).cloned())
    }
}

impl IdempotencyStore for MemoryStore {
    fn record_intent(&self, record: EffectRecord) -> Result<(), StoreError> {
        validation::new_intent(&record)?;
        Self::validate_encoded_size(encode_effect(&record)?, self.limits)?;
        let mut state = self.lock()?;
        if let Some(existing) = state.effects.get(&record.key) {
            return if existing == &record {
                Ok(())
            } else {
                Err(StoreError::Conflict)
            };
        }
        if state.effects.len() >= self.limits.max_effects {
            return Err(StoreError::CapacityExceeded);
        }
        state.effects.insert(record.key.clone(), record);
        Self::bump_retention_epoch(&mut state)?;
        Ok(())
    }

    fn record_result(&self, key: &IdempotencyKey, record: EffectRecord) -> Result<(), StoreError> {
        validation::terminal(key, &record)?;
        Self::validate_encoded_size(encode_effect(&record)?, self.limits)?;
        let mut state = self.lock()?;
        let intent = state.effects.get(key).ok_or(StoreError::NotFound)?;
        validation::result(key, intent, &record)?;
        state.effects.insert(key.clone(), record);
        Self::bump_retention_epoch(&mut state)?;
        Ok(())
    }

    fn effect(&self, key: &IdempotencyKey) -> Result<Option<EffectRecord>, StoreError> {
        Ok(self.lock()?.effects.get(key).cloned())
    }

    fn unknown_effects(&self, max: usize) -> Result<Vec<EffectRecord>, StoreError> {
        if max == 0 || max > self.limits.max_effects {
            return Err(StoreError::CapacityExceeded);
        }
        Ok(self
            .lock()?
            .effects
            .values()
            .filter(|record| record.state == EffectState::UnknownOutcome)
            .take(max)
            .cloned()
            .collect())
    }

    fn unresolved_effects(&self, max: usize) -> Result<Vec<EffectRecord>, StoreError> {
        if max == 0 || max > mainframe_env_store_api::MAX_PROVIDER_STATE_SCAN {
            return Err(StoreError::CapacityExceeded);
        }
        Ok(self
            .lock()?
            .effects
            .values()
            .filter(|record| {
                matches!(
                    record.state,
                    EffectState::Intent | EffectState::UnknownOutcome
                )
            })
            .take(max)
            .cloned()
            .collect())
    }

    fn stale_intents(
        &self,
        now_tick: u64,
        minimum_age_ticks: u64,
        max: usize,
    ) -> Result<Vec<EffectRecord>, StoreError> {
        if max == 0 || max > self.limits.max_effects {
            return Err(StoreError::CapacityExceeded);
        }
        if minimum_age_ticks == 0 {
            return Err(StoreError::InvalidTransition);
        }
        self.lock()?
            .effects
            .values()
            .filter_map(|record| {
                match validation::stale_intent(record, now_tick, minimum_age_ticks) {
                    Ok(true) => Some(Ok(record.clone())),
                    Ok(false) => None,
                    Err(error) => Some(Err(error)),
                }
            })
            .take(max)
            .collect()
    }

    fn claim_stale_intent(
        &self,
        key: &IdempotencyKey,
        expected_intent_epoch: u64,
        recovery_owner: &str,
        now_tick: u64,
        minimum_age_ticks: u64,
        lease_ticks: u64,
    ) -> Result<EffectRecord, StoreError> {
        let mut state = self.lock()?;
        let record = state.effects.get_mut(key).ok_or(StoreError::NotFound)?;
        validation::stale_claim(
            record,
            expected_intent_epoch,
            recovery_owner,
            now_tick,
            minimum_age_ticks,
            lease_ticks,
        )?;
        let (attempt, epoch) =
            record
                .intent
                .recovery_lease
                .as_ref()
                .map_or(Ok((1, 1)), |lease| {
                    Ok((
                        lease
                            .attempt
                            .checked_add(1)
                            .ok_or(StoreError::LeaseConflict)?,
                        lease
                            .epoch
                            .checked_add(1)
                            .ok_or(StoreError::LeaseConflict)?,
                    ))
                })?;
        record.intent.recovery_lease = Some(EffectRecoveryLease {
            owner: recovery_owner.into(),
            attempt,
            epoch,
            expires_tick: now_tick
                .checked_add(lease_ticks)
                .ok_or(StoreError::LeaseConflict)?,
        });
        Self::validate_encoded_size(encode_effect(record)?, self.limits)?;
        let updated = record.clone();
        Self::bump_retention_epoch(&mut state)?;
        Ok(updated)
    }

    fn reconcile_stale_intent(
        &self,
        key: &IdempotencyKey,
        recovery_owner: &str,
        recovery_epoch: u64,
        now_tick: u64,
        final_state: EffectState,
        format: mainframe_env_store_api::EffectDigestFormat,
        result_digest: [u8; 32],
    ) -> Result<EffectRecord, StoreError> {
        let mut state = self.lock()?;
        let mut staged = state.clone();
        let record = staged.effects.get_mut(key).ok_or(StoreError::NotFound)?;
        validation::stale_reconciliation(
            key,
            record,
            recovery_owner,
            recovery_epoch,
            now_tick,
            final_state,
            format,
        )?;
        record.state = final_state;
        record.result_digest = Some(result_digest);
        record.resolved_tick = Some(now_tick);
        validation::effect(record)?;
        Self::validate_encoded_size(encode_effect(record)?, self.limits)?;
        let updated = record.clone();
        if updated.intent.audit_resource.is_some() {
            let execution = staged
                .executions
                .get(&updated.execution_id)
                .ok_or(StoreError::NotFound)?;
            if let Some(audit) = validation::recovered_audit(execution, &updated, now_tick)? {
                Self::append_audit_locked(&mut staged, audit, self.limits)?;
            }
        }
        Self::bump_retention_epoch(&mut staged)?;
        *state = staged;
        Ok(updated)
    }

    fn reconcile_unknown(
        &self,
        key: &IdempotencyKey,
        final_state: EffectState,
        result_digest: [u8; 32],
    ) -> Result<EffectRecord, StoreError> {
        if !matches!(final_state, EffectState::Completed | EffectState::Failed) {
            return Err(StoreError::InvalidTransition);
        }
        let mut state = self.lock()?;
        let record = state.effects.get_mut(key).ok_or(StoreError::NotFound)?;
        if record.state != EffectState::UnknownOutcome {
            return Err(StoreError::InvalidTransition);
        }
        record.state = final_state;
        record.result_digest = Some(result_digest);
        Self::validate_encoded_size(encode_effect(record)?, self.limits)?;
        let updated = record.clone();
        Self::bump_retention_epoch(&mut state)?;
        Ok(updated)
    }
}

impl OutboxStore for MemoryStore {
    fn append_notification(&self, record: OutboxRecord) -> Result<(), StoreError> {
        let mut state = self.lock()?;
        Self::append_outbox_locked(&mut state, record, self.limits)
    }

    fn pending_notifications(&self, max: usize) -> Result<Vec<OutboxRecord>, StoreError> {
        if max == 0 || max > self.limits.max_outbox {
            return Err(StoreError::CapacityExceeded);
        }
        Ok(self
            .lock()?
            .outbox
            .values()
            .filter(|record| !record.delivered)
            .take(max)
            .cloned()
            .collect())
    }

    fn mark_notification_delivered(
        &self,
        notification_id: &str,
        expected_version: u64,
        delivered_tick: u64,
    ) -> Result<OutboxRecord, StoreError> {
        let mut state = self.lock()?;
        let record = state
            .outbox
            .get_mut(notification_id)
            .ok_or(StoreError::NotFound)?;
        if delivered_tick == 0 || record.version != expected_version || record.delivered {
            return Err(StoreError::Conflict);
        }
        record.delivered = true;
        record.delivered_tick = Some(delivered_tick);
        record.attempt = record.attempt.checked_add(1).ok_or(StoreError::Conflict)?;
        record.version = record.version.checked_add(1).ok_or(StoreError::Conflict)?;
        Self::validate_encoded_size(encode_outbox(record)?, self.limits)?;
        let updated = record.clone();
        Self::bump_retention_epoch(&mut state)?;
        Ok(updated)
    }
}

impl JournalStore for MemoryStore {
    fn admit_execution(
        &self,
        execution: ExecutionRecord,
        event: LifecycleEvent,
        notification: OutboxRecord,
    ) -> Result<(), StoreError> {
        validation::admission(&execution, &event, &notification)?;
        Self::validate_encoded_size(encode_execution(&execution)?, self.limits)?;
        let mut state = self.lock()?;
        let mut staged = state.clone();
        if staged.executions.contains_key(&execution.execution_id) {
            return Err(StoreError::AlreadyExists);
        }
        if staged.executions.len() >= self.limits.max_executions {
            return Err(StoreError::CapacityExceeded);
        }
        staged
            .executions
            .insert(execution.execution_id.clone(), execution);
        Self::append_event_locked(&mut staged, event, self.limits)?;
        Self::append_outbox_locked(&mut staged, notification, self.limits)?;
        Self::bump_retention_epoch(&mut staged)?;
        *state = staged;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn commit_execution_step(
        &self,
        execution_id: &ExecutionId,
        expected_version: u64,
        next_state: Option<ExecutionState>,
        event: LifecycleEvent,
        effect: Option<EffectRecord>,
        audit: Option<AuditRecord>,
        checkpoint: Option<CheckpointRecord>,
        notification: OutboxRecord,
    ) -> Result<ExecutionRecord, StoreError> {
        let mut state = self.lock()?;
        let mut staged = state.clone();
        let current = staged
            .executions
            .get(execution_id)
            .cloned()
            .ok_or(StoreError::NotFound)?;
        validation::execution_step(execution_id, &current, &event, &notification)?;
        if current.version != expected_version {
            return Err(StoreError::Conflict);
        }
        if next_state.is_some_and(|next| !current.state.can_transition_to(next)) {
            return Err(StoreError::InvalidTransition);
        }
        validation::audit_event(&current, &event, effect.as_ref(), audit.as_ref())?;
        let mut updated = current.clone();
        if let Some(next) = next_state {
            updated.state = next;
            updated.terminal_tick = next
                .terminal()
                .then_some(event.tick)
                .filter(|tick| *tick != 0);
        }
        updated.version = updated.version.checked_add(1).ok_or(StoreError::Conflict)?;
        Self::validate_encoded_size(encode_execution(&updated)?, self.limits)?;
        staged
            .executions
            .insert(execution_id.clone(), updated.clone());
        if let Some(mut effect) = effect {
            if matches!(effect.state, EffectState::Completed | EffectState::Failed)
                && effect.digest_format
                    == mainframe_env_store_api::EffectDigestFormat::CanonicalHostV1
                && effect.resolved_tick.is_none()
                && event.tick != 0
            {
                effect.resolved_tick = Some(event.tick);
            }
            validation::effect(&effect)?;
            Self::validate_encoded_size(encode_effect(&effect)?, self.limits)?;
            validation::effect_execution(&current, &effect)?;
            match effect.state {
                EffectState::Intent => {
                    validation::new_intent(&effect)?;
                    validation::effect_event(&event, &effect)?;
                    if staged.effects.contains_key(&effect.key) {
                        return Err(StoreError::Conflict);
                    }
                    if staged.effects.len() >= self.limits.max_effects {
                        return Err(StoreError::CapacityExceeded);
                    }
                    staged.effects.insert(effect.key.clone(), effect);
                }
                EffectState::Completed | EffectState::Failed | EffectState::UnknownOutcome => {
                    let intent = staged
                        .effects
                        .get(&effect.key)
                        .ok_or(StoreError::NotFound)?;
                    validation::result(&effect.key, intent, &effect)?;
                    validation::effect_event(&event, &effect)?;
                    staged.effects.insert(effect.key.clone(), effect);
                }
            }
        }
        if let Some(audit) = audit {
            Self::append_audit_locked(&mut staged, audit, self.limits)?;
        }
        if let Some(checkpoint) = checkpoint {
            validation::checkpoint(&checkpoint)?;
            Self::validate_encoded_size(encode_checkpoint(&checkpoint)?, self.limits)?;
            validation::checkpoint_execution(&current, &checkpoint)?;
            let old = staged
                .checkpoints
                .get(execution_id)
                .map_or(0, |record| record.payload.len());
            if old == 0 && staged.checkpoints.len() >= self.limits.max_checkpoints {
                return Err(StoreError::CapacityExceeded);
            }
            Self::reserve_blob(&mut staged, old, checkpoint.payload.len(), self.limits)?;
            staged.checkpoints.insert(execution_id.clone(), checkpoint);
        }
        Self::append_event_locked(&mut staged, event, self.limits)?;
        Self::append_outbox_locked(&mut staged, notification, self.limits)?;
        Self::bump_retention_epoch(&mut staged)?;
        *state = staged;
        Ok(updated)
    }
}

impl ProviderStateStore for MemoryStore {
    fn advance_logical_clock(&self, observed_floor: u64) -> Result<u64, StoreError> {
        if observed_floor > i64::MAX as u64 {
            return Err(StoreError::CapacityExceeded);
        }
        let mut state = self.lock()?;
        if let Some(legacy) = state
            .provider_state
            .get(&("jes-worker-meta".into(), "logical-clock".into()))
            .cloned()
        {
            let tick = u64::from_be_bytes(
                legacy
                    .payload
                    .as_slice()
                    .try_into()
                    .map_err(|_| StoreError::IncompatibleVersion)?,
            );
            if tick == 0 || legacy.version == 0 {
                return Err(StoreError::IncompatibleVersion);
            }
            state.logical_tick = state.logical_tick.max(tick);
            state
                .provider_state
                .remove(&("jes-worker-meta".into(), "logical-clock".into()));
            state.blob_bytes = state.blob_bytes.saturating_sub(legacy.payload.len());
            state.provider_epoch = state
                .provider_epoch
                .checked_add(1)
                .ok_or(StoreError::CapacityExceeded)?;
        }
        let tick = state.logical_tick.max(observed_floor.max(1));
        state.logical_tick = tick;
        Ok(tick)
    }

    fn get_provider_state(
        &self,
        namespace: &str,
        key: &str,
    ) -> Result<Option<ProviderStateRecord>, StoreError> {
        Ok(self
            .lock()?
            .provider_state
            .get(&(namespace.to_string(), key.to_string()))
            .cloned())
    }

    fn list_provider_state(
        &self,
        namespace: &str,
        max: usize,
    ) -> Result<Vec<ProviderStateRecord>, StoreError> {
        if max == 0 || max > mainframe_env_store_api::MAX_PROVIDER_STATE_SCAN {
            return Err(StoreError::CapacityExceeded);
        }
        Ok(self
            .lock()?
            .provider_state
            .iter()
            .filter(|((candidate, _), _)| candidate == namespace)
            .take(max)
            .map(|(_, record)| record.clone())
            .collect())
    }

    fn list_provider_state_prefix(
        &self,
        namespace_prefix: &str,
        max: usize,
    ) -> Result<Vec<ProviderStateRecord>, StoreError> {
        if namespace_prefix.is_empty()
            || namespace_prefix.len() > mainframe_env_store_api::MAX_PROVIDER_NAMESPACE_BYTES
            || max == 0
            || max > mainframe_env_store_api::MAX_PROVIDER_STATE_SCAN
        {
            return Err(StoreError::CapacityExceeded);
        }
        Ok(self
            .lock()?
            .provider_state
            .iter()
            .filter(|((namespace, _), _)| namespace.starts_with(namespace_prefix))
            .take(max)
            .map(|(_, record)| record.clone())
            .collect())
    }

    fn put_provider_state(
        &self,
        record: ProviderStateRecord,
        expected_version: Option<u64>,
    ) -> Result<(), StoreError> {
        let mut state = self.lock()?;
        Self::put_provider_state_locked(&mut state, record, expected_version, self.limits)
    }

    fn delete_provider_state(
        &self,
        namespace: &str,
        key: &str,
        expected_version: u64,
    ) -> Result<(), StoreError> {
        let mut state = self.lock()?;
        let map_key = (namespace.to_string(), key.to_string());
        let current = state
            .provider_state
            .get(&map_key)
            .ok_or(StoreError::NotFound)?;
        if current.version != expected_version {
            return Err(StoreError::Conflict);
        }
        let next_epoch = state
            .provider_epoch
            .checked_add(1)
            .ok_or(StoreError::CapacityExceeded)?;
        let bytes = current.payload.len();
        state.provider_state.remove(&map_key);
        state.blob_bytes = state.blob_bytes.saturating_sub(bytes);
        state.provider_epoch = next_epoch;
        Ok(())
    }

    fn move_provider_state(
        &self,
        record: ProviderStateRecord,
        old_key: &str,
        expected_version: u64,
    ) -> Result<(), StoreError> {
        record.validate_move(old_key, expected_version, self.limits.max_blob_bytes)?;
        let mut state = self.lock()?;
        let old_map_key = (record.namespace.clone(), old_key.to_string());
        let new_map_key = (record.namespace.clone(), record.key.clone());
        let old = state
            .provider_state
            .get(&old_map_key)
            .ok_or(StoreError::Conflict)?;
        if old.version != expected_version || state.provider_state.contains_key(&new_map_key) {
            return Err(StoreError::Conflict);
        }
        let next_epoch = state
            .provider_epoch
            .checked_add(1)
            .ok_or(StoreError::CapacityExceeded)?;
        let old_bytes = old.payload.len();
        Self::reserve_blob(&mut state, old_bytes, record.payload.len(), self.limits)?;
        state.provider_state.remove(&old_map_key);
        state.provider_state.insert(new_map_key, record);
        state.provider_epoch = next_epoch;
        Ok(())
    }

    fn put_provider_states_atomic(
        &self,
        writes: Vec<ProviderStateWrite>,
    ) -> Result<(), StoreError> {
        self.mutate_provider_states_atomic(
            writes.into_iter().map(ProviderStateMutation::Put).collect(),
        )
    }

    fn mutate_provider_states_atomic(
        &self,
        mutations: Vec<ProviderStateMutation>,
    ) -> Result<(), StoreError> {
        if mutations.is_empty() {
            return Err(StoreError::InvalidTransition);
        }
        let mut state = self.lock()?;
        let mut staged = state.clone();
        let staging_limits = StoreLimits {
            max_provider_state: usize::MAX,
            max_total_blob_bytes: usize::MAX,
            ..self.limits
        };
        for mutation in mutations {
            match mutation {
                ProviderStateMutation::Put(write) => Self::put_provider_state_locked(
                    &mut staged,
                    write.record,
                    write.expected_version,
                    staging_limits,
                )?,
                ProviderStateMutation::Delete {
                    namespace,
                    key,
                    expected_version,
                } => {
                    if namespace.is_empty() || key.is_empty() || expected_version == 0 {
                        return Err(StoreError::Conflict);
                    }
                    let map_key = (namespace, key);
                    let current = staged
                        .provider_state
                        .get(&map_key)
                        .ok_or(StoreError::NotFound)?;
                    if current.version != expected_version {
                        return Err(StoreError::Conflict);
                    }
                    let bytes = current.payload.len();
                    staged.provider_state.remove(&map_key);
                    staged.blob_bytes = staged.blob_bytes.saturating_sub(bytes);
                    staged.provider_epoch = staged
                        .provider_epoch
                        .checked_add(1)
                        .ok_or(StoreError::CapacityExceeded)?;
                }
                ProviderStateMutation::Move {
                    record,
                    old_key,
                    expected_version,
                } => {
                    record.validate_move(&old_key, expected_version, self.limits.max_blob_bytes)?;
                    let old_map_key = (record.namespace.clone(), old_key);
                    let new_map_key = (record.namespace.clone(), record.key.clone());
                    let old = staged
                        .provider_state
                        .get(&old_map_key)
                        .ok_or(StoreError::Conflict)?;
                    if old.version != expected_version
                        || staged.provider_state.contains_key(&new_map_key)
                    {
                        return Err(StoreError::Conflict);
                    }
                    let old_bytes = old.payload.len();
                    Self::reserve_blob(
                        &mut staged,
                        old_bytes,
                        record.payload.len(),
                        staging_limits,
                    )?;
                    staged.provider_state.remove(&old_map_key);
                    staged.provider_state.insert(new_map_key, record);
                    staged.provider_epoch = staged
                        .provider_epoch
                        .checked_add(1)
                        .ok_or(StoreError::CapacityExceeded)?;
                }
            }
        }
        if staged.provider_state.len() > self.limits.max_provider_state
            || staged.blob_bytes > self.limits.max_total_blob_bytes
        {
            return Err(StoreError::CapacityExceeded);
        }
        *state = staged;
        Ok(())
    }

    fn archive_provider_state_replacement(
        &self,
        request: ProviderStateArchiveReplacement,
    ) -> Result<RetentionArchive, StoreError> {
        crate::retention::validate_provider_replacement(&request, self.limits.max_blob_bytes)?;
        let mut state = self.lock()?;
        if state.provider_epoch != request.expected_epoch {
            return Err(StoreError::Conflict);
        }
        let current = state
            .provider_state
            .get(&(request.source.namespace.clone(), request.source.key.clone()))
            .ok_or(StoreError::Conflict)?;
        if current != &request.source {
            return Err(StoreError::Conflict);
        }
        for candidate in &request.rows {
            if let Some(proof) = &candidate.observation
                && state.observations.get(&(
                    request.target,
                    proof.observation.namespace.clone(),
                    proof.observation.key.clone(),
                )) != Some(&(proof.version, proof.observation.clone()))
            {
                return Err(StoreError::Conflict);
            }
        }
        let archive = build_archive(
            request.target,
            request.archived_tick,
            request.watermark_tick,
            request
                .rows
                .into_iter()
                .map(|candidate| ArchivedRetentionRow {
                    namespace: candidate.row.namespace,
                    key: candidate.row.key,
                    version: candidate.row.version,
                    payload: candidate.row.payload,
                    retention_tick: candidate.retention_tick,
                    owner_execution: candidate.owner_execution,
                })
                .collect(),
        )?;
        let mut staged = state.clone();
        Self::put_provider_state_locked(
            &mut staged,
            request.replacement.record,
            request.replacement.expected_version,
            self.limits,
        )?;
        for row in &archive.rows {
            remove_memory_observation(&mut staged, request.target, row)?;
        }
        insert_memory_archive(&mut staged, archive.clone(), self.limits)?;
        *state = staged;
        Ok(archive)
    }

    fn provider_retention_archive_usage(
        &self,
        target: RetentionTarget,
    ) -> Result<(usize, u64), StoreError> {
        let state = self.lock()?;
        let mut rows = 0usize;
        let mut bytes = 0u64;
        for archive in state
            .archives
            .values()
            .filter(|archive| archive.target == target)
        {
            rows = rows
                .checked_add(archive.rows.len())
                .ok_or(StoreError::CapacityExceeded)?;
            bytes = bytes
                .checked_add(crate::retention::archive_storage_bytes(&archive.rows)?)
                .ok_or(StoreError::CapacityExceeded)?;
        }
        Ok((rows, bytes))
    }

    fn provider_retention_authority_usage(
        &self,
        target: RetentionTarget,
    ) -> Result<mainframe_env_store_api::RetentionAuthorityUsage, StoreError> {
        let state = self.lock()?;
        let mut archive_rows = 0usize;
        let mut archive_bytes = 0u64;
        for archive in state
            .archives
            .values()
            .filter(|archive| archive.target == target)
        {
            archive_rows = archive_rows
                .checked_add(archive.rows.len())
                .ok_or(StoreError::CapacityExceeded)?;
            archive_bytes = archive_bytes
                .checked_add(crate::retention::archive_storage_bytes(&archive.rows)?)
                .ok_or(StoreError::CapacityExceeded)?;
        }
        let mut observation_rows = 0usize;
        let mut observation_bytes = 0u64;
        for ((candidate, _, _), (_, observation)) in &state.observations {
            if *candidate == target {
                observation_rows = observation_rows
                    .checked_add(1)
                    .ok_or(StoreError::CapacityExceeded)?;
                observation_bytes = observation_bytes
                    .checked_add(crate::retention::observation::storage_bytes(observation)?)
                    .ok_or(StoreError::CapacityExceeded)?;
            }
        }
        Ok(mainframe_env_store_api::RetentionAuthorityUsage {
            archive_rows,
            shared_archive_rows: state
                .archives
                .values()
                .map(|archive| archive.rows.len())
                .sum(),
            archive_row_capacity: self.limits.max_retention_archive_rows,
            archive_bytes,
            shared_archive_bytes: state.archive_bytes,
            archive_byte_capacity: self.limits.max_retention_archive_bytes,
            observation_rows,
            shared_observation_rows: state.observations.len(),
            observation_row_capacity: self.limits.max_retention_archive_rows,
            observation_bytes,
            shared_observation_bytes: state.observation_bytes,
            observation_byte_capacity: self.limits.max_retention_archive_bytes,
        })
    }

    fn provider_state_retention_epoch(&self) -> Result<u64, StoreError> {
        Ok(self.lock()?.provider_epoch)
    }

    fn archive_provider_state_deletion(
        &self,
        request: ProviderStateArchiveDeletion,
    ) -> Result<RetentionArchive, StoreError> {
        crate::retention::validate_provider_deletion(&request, self.limits.max_blob_bytes)?;
        let mut state = self.lock()?;
        if state.provider_epoch != request.expected_epoch {
            return Err(StoreError::Conflict);
        }
        for candidate in &request.rows {
            let current = state
                .provider_state
                .get(&(candidate.row.namespace.clone(), candidate.row.key.clone()))
                .ok_or(StoreError::Conflict)?;
            if current != &candidate.row {
                return Err(StoreError::Conflict);
            }
            if let Some(proof) = &candidate.observation
                && state.observations.get(&(
                    request.target,
                    proof.observation.namespace.clone(),
                    proof.observation.key.clone(),
                )) != Some(&(proof.version, proof.observation.clone()))
            {
                return Err(StoreError::Conflict);
            }
            validate_memory_provider_dependency(&state, candidate)?;
        }
        let archive = build_archive(
            request.target,
            request.archived_tick,
            request.watermark_tick,
            request
                .rows
                .into_iter()
                .map(|candidate| ArchivedRetentionRow {
                    namespace: candidate.row.namespace,
                    key: candidate.row.key,
                    version: candidate.row.version,
                    payload: candidate.row.payload,
                    retention_tick: candidate.retention_tick,
                    owner_execution: candidate.owner_execution,
                })
                .collect(),
        )?;
        let mut staged = state.clone();
        for row in &archive.rows {
            remove_memory_row(&mut staged, row)?;
            remove_memory_observation(&mut staged, request.target, row)?;
        }
        staged.provider_epoch = staged
            .provider_epoch
            .checked_add(
                u64::try_from(archive.rows.len()).map_err(|_| StoreError::CapacityExceeded)?,
            )
            .ok_or(StoreError::CapacityExceeded)?;
        insert_memory_archive(&mut staged, archive.clone(), self.limits)?;
        *state = staged;
        Ok(archive)
    }

    fn provider_retention_observations(
        &self,
        target: RetentionTarget,
        max: usize,
    ) -> Result<Vec<RetentionObservation>, StoreError> {
        if max == 0 || max > mainframe_env_store_api::MAX_RETENTION_BATCH {
            return Err(StoreError::CapacityExceeded);
        }
        Ok(self
            .lock()?
            .observations
            .iter()
            .filter(|((candidate, _, _), _)| *candidate == target)
            .map(|(_, (_, observation))| observation.clone())
            .take(max)
            .collect())
    }

    fn provider_retention_observation_page(
        &self,
        target: RetentionTarget,
        after: Option<&mainframe_env_store_api::ProviderStateIdentity>,
        max: usize,
    ) -> Result<Vec<(u64, RetentionObservation)>, StoreError> {
        if max == 0 || max > mainframe_env_store_api::MAX_RETENTION_BATCH {
            return Err(StoreError::CapacityExceeded);
        }
        let state = self.lock()?;
        Ok(state
            .observations
            .iter()
            .filter(|((candidate, namespace, key), _)| {
                *candidate == target
                    && after.is_none_or(|after| {
                        (namespace.as_str(), key.as_str())
                            > (after.namespace.as_str(), after.key.as_str())
                    })
            })
            .map(|(_, entry)| entry.clone())
            .take(max)
            .collect())
    }

    fn delete_provider_retention_observation(
        &self,
        request: ProviderRetentionObservationDeletion,
    ) -> Result<(), StoreError> {
        let observation = request.observation;
        crate::retention::observation::validate(&observation)?;
        if request.expected_observation_version == 0 {
            return Err(StoreError::Conflict);
        }
        let mut state = self.lock()?;
        if state.provider_epoch != request.expected_epoch {
            return Err(StoreError::Conflict);
        }
        match request.source {
            ProviderRetentionObservationSource::Present(source) => {
                if state
                    .provider_state
                    .get(&(source.namespace.clone(), source.key.clone()))
                    != Some(&source)
                {
                    return Err(StoreError::Conflict);
                }
            }
            ProviderRetentionObservationSource::Absent(identity) => {
                if state
                    .provider_state
                    .contains_key(&(identity.namespace, identity.key))
                {
                    return Err(StoreError::Conflict);
                }
            }
        }
        let key = (
            observation.target,
            observation.namespace.clone(),
            observation.key.clone(),
        );
        if state.observations.get(&key)
            != Some(&(request.expected_observation_version, observation.clone()))
        {
            return Err(StoreError::Conflict);
        }
        state.observations.remove(&key);
        state.observation_bytes = state
            .observation_bytes
            .saturating_sub(crate::retention::observation::storage_bytes(&observation)?);
        Self::bump_retention_epoch(&mut state)?;
        Ok(())
    }

    fn record_provider_retention_observation(
        &self,
        source: ProviderStateRecord,
        expected_epoch: u64,
        observation: RetentionObservation,
    ) -> Result<RetentionReconciliationReceipt, StoreError> {
        crate::retention::observation::validate(&observation)?;
        let mut state = self.lock()?;
        if state.provider_epoch != expected_epoch
            || state
                .provider_state
                .get(&(source.namespace.clone(), source.key.clone()))
                != Some(&source)
        {
            return Err(StoreError::Conflict);
        }
        let map_key = (
            observation.target,
            observation.namespace.clone(),
            observation.key.clone(),
        );
        let old = state.observations.get(&map_key).cloned();
        if let Some((version, current)) = &old
            && current.source_version == observation.source_version
            && current.source_digest == observation.source_digest
            && current.owner_execution == observation.owner_execution
        {
            return Ok(RetentionReconciliationReceipt {
                target: current.target,
                namespace: current.namespace.clone(),
                key: current.key.clone(),
                source_version: current.source_version,
                observation_version: *version,
                reconciled_tick: current.observed_tick,
            });
        }
        let observation_version = old.as_ref().map_or(Ok(1), |(version, _)| {
            version.checked_add(1).ok_or(StoreError::CapacityExceeded)
        })?;
        if old.is_none() && state.observations.len() >= self.limits.max_retention_archive_rows {
            return Err(StoreError::CapacityExceeded);
        }
        let old_bytes = old
            .as_ref()
            .map(|(_, item)| crate::retention::observation::storage_bytes(item))
            .transpose()?
            .unwrap_or(0);
        let new_bytes = crate::retention::observation::storage_bytes(&observation)?;
        let total = state
            .observation_bytes
            .checked_sub(old_bytes)
            .and_then(|bytes| bytes.checked_add(new_bytes))
            .ok_or(StoreError::CapacityExceeded)?;
        if total > self.limits.max_retention_archive_bytes {
            return Err(StoreError::CapacityExceeded);
        }
        state.observation_bytes = total;
        state
            .observations
            .insert(map_key, (observation_version, observation.clone()));
        Self::bump_retention_epoch(&mut state)?;
        Ok(RetentionReconciliationReceipt {
            target: observation.target,
            namespace: observation.namespace,
            key: observation.key,
            source_version: observation.source_version,
            observation_version,
            reconciled_tick: observation.observed_tick,
        })
    }
}

impl RetentionStore for MemoryStore {
    fn retention_capacity_health(
        &self,
        policy: RetentionPolicy,
    ) -> Result<mainframe_env_store_api::RetentionCapacityHealth, StoreError> {
        let state = self.lock()?;
        let event_rows = state.events.values().map(Vec::len).sum();
        let provider_rows = state.provider_state.len();
        let source_usage = RetentionTarget::ALL
            .into_iter()
            .map(|target| {
                let (used, capacity) = match target {
                    RetentionTarget::TerminalExecutions => {
                        (state.executions.len(), self.limits.max_executions)
                    }
                    RetentionTarget::TerminalWork => (state.work.len(), self.limits.max_work_items),
                    RetentionTarget::LifecycleEvents => (event_rows, self.limits.max_events),
                    RetentionTarget::DeliveredOutbox => {
                        (state.outbox.len(), self.limits.max_outbox)
                    }
                    RetentionTarget::ResolvedEffects => {
                        (state.effects.len(), self.limits.max_effects)
                    }
                    RetentionTarget::Audit => (state.audits.len(), self.limits.max_audits),
                    RetentionTarget::Db2Replay
                    | RetentionTarget::ImsReplay
                    | RetentionTarget::MqReplay
                    | RetentionTarget::CicsReplay
                    | RetentionTarget::DatasetReplay
                    | RetentionTarget::CicsUnitOfWork
                    | RetentionTarget::RacfEvidence
                    | RetentionTarget::CobolLifecycle
                    | RetentionTarget::SpoolJobs
                    | RetentionTarget::ConsoleLog => {
                        (provider_rows, self.limits.max_provider_state)
                    }
                };
                (target, used, capacity)
            })
            .collect();
        let archive_rows = state
            .archives
            .values()
            .map(|archive| archive.rows.len())
            .sum();
        crate::retention::capacity_health(
            policy,
            source_usage,
            archive_rows,
            self.limits.max_retention_archive_rows,
            state.archive_bytes,
            self.limits.max_retention_archive_bytes,
            state.observations.len(),
            self.limits.max_retention_archive_rows,
            state.observation_bytes,
            self.limits.max_retention_archive_bytes,
        )
    }

    fn retention_forecast(
        &self,
        target: RetentionTarget,
        policy: RetentionPolicy,
        now_tick: u64,
        observed_growth_per_tick: u64,
    ) -> Result<RetentionForecast, StoreError> {
        if crate::retention::dependency_sensitive_core_target(target)
            || crate::retention::provider_owned_target(target)
        {
            return Err(StoreError::InvalidTransition);
        }
        let watermarks = policy.watermarks(now_tick)?;
        let state = self.lock()?;
        let (active, eligible) = memory_candidates(
            &state,
            target,
            target_watermark(target, watermarks),
            target_window_open(target, policy, now_tick),
            None,
        )?;
        let (capacity, total_used) = match target {
            RetentionTarget::TerminalExecutions => {
                (self.limits.max_executions, state.executions.len())
            }
            RetentionTarget::TerminalWork => (self.limits.max_work_items, state.work.len()),
            RetentionTarget::LifecycleEvents => (
                self.limits.max_events,
                state.events.values().map(Vec::len).sum(),
            ),
            RetentionTarget::DeliveredOutbox => (self.limits.max_outbox, state.outbox.len()),
            RetentionTarget::ResolvedEffects => (self.limits.max_effects, state.effects.len()),
            RetentionTarget::Db2Replay
            | RetentionTarget::ImsReplay
            | RetentionTarget::MqReplay
            | RetentionTarget::CicsReplay
            | RetentionTarget::DatasetReplay
            | RetentionTarget::CicsUnitOfWork => {
                (self.limits.max_provider_state, state.provider_state.len())
            }
            RetentionTarget::Audit => (self.limits.max_audits, state.audits.len()),
            RetentionTarget::RacfEvidence
            | RetentionTarget::CobolLifecycle
            | RetentionTarget::SpoolJobs
            | RetentionTarget::ConsoleLog => return Err(StoreError::InvalidTransition),
        };
        let archive_records = state
            .archives
            .values()
            .map(|archive| archive.rows.len())
            .sum();
        forecast(
            target,
            policy,
            now_tick,
            observed_growth_per_tick,
            ForecastCounts {
                active,
                eligible: eligible.len(),
                capacity,
                total_used,
                archive_records,
                archive_capacity: self.limits.max_retention_archive_rows,
                archive_total_used: archive_records,
                archive_bytes: state.archive_bytes,
                archive_byte_capacity: self.limits.max_retention_archive_bytes,
                observation_records: state.observations.len(),
                observation_capacity: self.limits.max_retention_archive_rows,
                observation_bytes: state.observation_bytes,
                observation_byte_capacity: self.limits.max_retention_archive_bytes,
                max_source_storage_bytes: crate::retention::worst_case_archived_row_storage_bytes(
                    self.limits.max_blob_bytes,
                ),
            },
        )
    }

    fn retention_forecast_with_dependencies(
        &self,
        target: RetentionTarget,
        policy: RetentionPolicy,
        now_tick: u64,
        observed_growth_per_tick: u64,
        dependencies: &CoreRetentionDependencySnapshot,
    ) -> Result<RetentionForecast, StoreError> {
        if !crate::retention::dependency_sensitive_core_target(target) {
            return Err(StoreError::InvalidTransition);
        }
        let watermarks = policy.watermarks(now_tick)?;
        let state = self.lock()?;
        let (active, eligible) = memory_candidates(
            &state,
            target,
            target_watermark(target, watermarks),
            target_window_open(target, policy, now_tick),
            Some(dependencies),
        )?;
        let (capacity, total_used) = match target {
            RetentionTarget::TerminalExecutions => {
                (self.limits.max_executions, state.executions.len())
            }
            RetentionTarget::TerminalWork => (self.limits.max_work_items, state.work.len()),
            RetentionTarget::LifecycleEvents => (
                self.limits.max_events,
                state.events.values().map(Vec::len).sum(),
            ),
            RetentionTarget::ResolvedEffects => (self.limits.max_effects, state.effects.len()),
            _ => unreachable!(),
        };
        let archive_records = state
            .archives
            .values()
            .map(|archive| archive.rows.len())
            .sum();
        forecast(
            target,
            policy,
            now_tick,
            observed_growth_per_tick,
            ForecastCounts {
                active,
                eligible: eligible.len(),
                capacity,
                total_used,
                archive_records,
                archive_capacity: self.limits.max_retention_archive_rows,
                archive_total_used: archive_records,
                archive_bytes: state.archive_bytes,
                archive_byte_capacity: self.limits.max_retention_archive_bytes,
                observation_records: state.observations.len(),
                observation_capacity: self.limits.max_retention_archive_rows,
                observation_bytes: state.observation_bytes,
                observation_byte_capacity: self.limits.max_retention_archive_bytes,
                max_source_storage_bytes: crate::retention::worst_case_archived_row_storage_bytes(
                    self.limits.max_blob_bytes,
                ),
            },
        )
    }

    fn provider_validated_retention_forecast(
        &self,
        target: RetentionTarget,
        policy: RetentionPolicy,
        now_tick: u64,
        observed_growth_per_tick: u64,
        active_records: usize,
        eligible_records: usize,
        source_capacity: usize,
    ) -> Result<RetentionForecast, StoreError> {
        let state = self.lock()?;
        if source_capacity == 0 || active_records > source_capacity {
            return Err(StoreError::IncompatibleVersion);
        }
        let archive_records = state.archives.values().map(|item| item.rows.len()).sum();
        let shared_headroom = self
            .limits
            .max_provider_state
            .saturating_sub(state.provider_state.len());
        let local_headroom = source_capacity.saturating_sub(active_records);
        let effective_capacity = active_records
            .checked_add(shared_headroom.min(local_headroom))
            .ok_or(StoreError::CapacityExceeded)?;
        let mut result = forecast(
            target,
            policy,
            now_tick,
            observed_growth_per_tick,
            ForecastCounts {
                active: active_records,
                eligible: eligible_records,
                capacity: effective_capacity,
                total_used: active_records,
                archive_records,
                archive_capacity: self.limits.max_retention_archive_rows,
                archive_total_used: archive_records,
                archive_bytes: state.archive_bytes,
                archive_byte_capacity: self.limits.max_retention_archive_bytes,
                observation_records: state.observations.len(),
                observation_capacity: self.limits.max_retention_archive_rows,
                observation_bytes: state.observation_bytes,
                observation_byte_capacity: self.limits.max_retention_archive_bytes,
                max_source_storage_bytes: crate::retention::worst_case_archived_row_storage_bytes(
                    self.limits.max_blob_bytes,
                ),
            },
        )?;
        result.saturation = result.saturation.max(crate::retention::saturation(
            self.limits.max_provider_state,
            state.provider_state.len(),
            policy.low_watermark_percent,
            policy.high_watermark_percent,
        ));
        Ok(result)
    }

    fn archive_and_prune(
        &self,
        policy: RetentionPolicy,
        request: RetentionRequest,
    ) -> Result<RetentionReceipt, StoreError> {
        if crate::retention::dependency_sensitive_core_target(request.target)
            || crate::retention::provider_owned_target(request.target)
        {
            return Err(StoreError::InvalidTransition);
        }
        let watermarks = validate_request(policy, request)?;
        let watermark = target_watermark(request.target, watermarks);
        let mut state = self.lock()?;
        let (examined, eligible) = memory_candidates(
            &state,
            request.target,
            watermark,
            target_window_open(request.target, policy, request.now_tick),
            None,
        )?;
        let eligible_count = eligible.len();
        let rows = eligible
            .into_iter()
            .take(request.max_records)
            .map(|(_, row)| row)
            .collect::<Vec<_>>();
        if rows.is_empty() {
            return Ok(RetentionReceipt {
                target: request.target,
                watermark_tick: watermark,
                examined,
                archived: 0,
                pruned: 0,
                protected: examined.saturating_sub(eligible_count),
                observations_created: 0,
                observations_reused: 0,
                stale_observations_removed: 0,
                archive_id: None,
            });
        }
        let mut batch = rows.len();
        let (archive, staged) = loop {
            let archive = build_archive(
                request.target,
                request.now_tick,
                watermark,
                rows.iter().take(batch).cloned().collect(),
            )?;
            let mut staged = state.clone();
            for row in &archive.rows {
                remove_memory_row(&mut staged, row)?;
                remove_memory_observation(&mut staged, request.target, row)?;
            }
            match insert_memory_archive(&mut staged, archive.clone(), self.limits) {
                Ok(()) => {
                    Self::bump_retention_epoch(&mut staged)?;
                    break (archive, staged);
                }
                Err(StoreError::CapacityExceeded | StoreError::PayloadTooLarge) if batch > 1 => {
                    batch = (batch / 2).max(1);
                }
                Err(problem) => return Err(problem),
            }
        };
        *state = staged;
        Ok(RetentionReceipt {
            target: request.target,
            watermark_tick: watermark,
            examined,
            archived: archive.rows.len(),
            pruned: archive.rows.len(),
            protected: examined.saturating_sub(eligible_count),
            observations_created: 0,
            observations_reused: 0,
            stale_observations_removed: 0,
            archive_id: Some(archive.archive_id),
        })
    }

    fn archive_and_prune_with_dependencies(
        &self,
        policy: RetentionPolicy,
        request: RetentionRequest,
        dependencies: &CoreRetentionDependencySnapshot,
    ) -> Result<RetentionReceipt, StoreError> {
        let watermarks = validate_request(policy, request)?;
        if !crate::retention::dependency_sensitive_core_target(request.target) {
            return Err(StoreError::InvalidTransition);
        }
        let watermark = target_watermark(request.target, watermarks);
        let mut state = self.lock()?;
        let (examined, eligible) = memory_candidates(
            &state,
            request.target,
            watermark,
            target_window_open(request.target, policy, request.now_tick),
            Some(dependencies),
        )?;
        let eligible_count = eligible.len();
        let rows = eligible
            .into_iter()
            .take(request.max_records)
            .map(|(_, row)| row)
            .collect::<Vec<_>>();
        if rows.is_empty() {
            return Ok(RetentionReceipt {
                target: request.target,
                watermark_tick: watermark,
                examined,
                archived: 0,
                pruned: 0,
                protected: examined.saturating_sub(eligible_count),
                observations_created: 0,
                observations_reused: 0,
                stale_observations_removed: 0,
                archive_id: None,
            });
        }
        let mut batch = rows.len();
        let (archive, staged) = loop {
            let archive = build_archive(
                request.target,
                request.now_tick,
                watermark,
                rows.iter().take(batch).cloned().collect(),
            )?;
            let mut staged = state.clone();
            for row in &archive.rows {
                remove_memory_row(&mut staged, row)?;
                remove_memory_observation(&mut staged, request.target, row)?;
            }
            match insert_memory_archive(&mut staged, archive.clone(), self.limits) {
                Ok(()) => {
                    Self::bump_retention_epoch(&mut staged)?;
                    break (archive, staged);
                }
                Err(StoreError::CapacityExceeded | StoreError::PayloadTooLarge) if batch > 1 => {
                    batch = (batch / 2).max(1);
                }
                Err(problem) => return Err(problem),
            }
        };
        *state = staged;
        Ok(RetentionReceipt {
            target: request.target,
            watermark_tick: watermark,
            examined,
            archived: archive.rows.len(),
            pruned: archive.rows.len(),
            protected: examined.saturating_sub(eligible_count),
            observations_created: 0,
            observations_reused: 0,
            stale_observations_removed: 0,
            archive_id: Some(archive.archive_id),
        })
    }

    fn retention_archives(
        &self,
        target: RetentionTarget,
        max: usize,
    ) -> Result<Vec<RetentionArchive>, StoreError> {
        if max == 0 || max > mainframe_env_store_api::MAX_RETENTION_BATCH {
            return Err(StoreError::CapacityExceeded);
        }
        let state = self.lock()?;
        let mut archives = state
            .archives
            .values()
            .filter(|archive| archive.target == target)
            .collect::<Vec<_>>();
        archives.sort_by(|left, right| {
            left.archived_tick
                .cmp(&right.archived_tick)
                .then_with(|| left.archive_id.cmp(&right.archive_id))
        });
        let mut rows = 0usize;
        let mut result = Vec::new();
        for archive in archives {
            let next = rows
                .checked_add(archive.rows.len())
                .ok_or(StoreError::CapacityExceeded)?;
            if next > max && !result.is_empty() {
                break;
            }
            rows = next;
            result.push(archive.clone());
        }
        Ok(result)
    }

    fn prune_retention_archives_authorized(
        &self,
        policy: RetentionPolicy,
        request: RetentionArchivePruneRequest,
    ) -> Result<RetentionArchivePruneOutcome, StoreError> {
        let watermark = policy.watermarks(request.now_tick)?.archive_tick;
        if request.now_tick == 0
            || request.max_records == 0
            || request.max_records > policy.max_batch
            || request.max_records > mainframe_env_store_api::MAX_RETENTION_BATCH
        {
            return Err(StoreError::InvalidTransition);
        }
        if request.now_tick < policy.archive_ticks {
            return if request.authorized_oversized_archive_id.is_some() {
                Err(StoreError::Conflict)
            } else {
                Ok(RetentionArchivePruneOutcome::Pruned(
                    RetentionArchivePruneReceipt {
                        pruned_source_rows: 0,
                        archive_ids: Vec::new(),
                        oversized_authorization_used: false,
                    },
                ))
            };
        }
        let mut state = self.lock()?;
        let mut staged = state.clone();
        let mut selected_rows = 0usize;
        let mut keys = Vec::new();
        let mut eligible = staged
            .archives
            .iter()
            .filter(|(_, archive)| archive.archived_tick <= watermark)
            .map(|(key, archive)| {
                (
                    archive.archived_tick,
                    archive.archive_id.clone(),
                    key.clone(),
                    archive.rows.len(),
                )
            })
            .collect::<Vec<_>>();
        eligible.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
        for (_, _, key, row_count) in eligible {
            let next = selected_rows
                .checked_add(row_count)
                .ok_or(StoreError::CapacityExceeded)?;
            if next > request.max_records && !keys.is_empty() {
                break;
            }
            selected_rows = next;
            keys.push(key);
        }
        if keys.is_empty() {
            return if request.authorized_oversized_archive_id.is_some() {
                Err(StoreError::Conflict)
            } else {
                Ok(RetentionArchivePruneOutcome::Pruned(
                    RetentionArchivePruneReceipt {
                        pruned_source_rows: 0,
                        archive_ids: Vec::new(),
                        oversized_authorization_used: false,
                    },
                ))
            };
        }
        let first_key = keys.first().ok_or(StoreError::Conflict)?;
        let first = staged.archives.get(first_key).ok_or(StoreError::Conflict)?;
        let oversized = first.rows.len() > request.max_records;
        match (
            oversized,
            request.authorized_oversized_archive_id.as_deref(),
        ) {
            (true, None) => {
                return Ok(RetentionArchivePruneOutcome::AuthorizationRequired {
                    archive_id: first.archive_id.clone(),
                    source_rows: first.rows.len(),
                    requested_max_records: request.max_records,
                });
            }
            (true, Some(authorized)) if authorized == first.archive_id.as_str() => {}
            (false, None) => {}
            (true, Some(_)) | (false, Some(_)) => return Err(StoreError::Conflict),
        }
        let archive_ids = keys
            .iter()
            .map(|key| {
                staged
                    .archives
                    .get(key)
                    .map(|archive| archive.archive_id.clone())
                    .ok_or(StoreError::Conflict)
            })
            .collect::<Result<Vec<_>, _>>()?;
        for key in &keys {
            let archive = staged.archives.remove(key).ok_or(StoreError::Conflict)?;
            let bytes = crate::retention::archive_storage_bytes(&archive.rows)?;
            staged.archive_bytes = staged.archive_bytes.saturating_sub(bytes);
        }
        *state = staged;
        Ok(RetentionArchivePruneOutcome::Pruned(
            RetentionArchivePruneReceipt {
                pruned_source_rows: selected_rows,
                archive_ids,
                oversized_authorization_used: oversized,
            },
        ))
    }

    fn reconcile_retention_age(
        &self,
        request: RetentionAgeReconciliation,
        now_tick: u64,
    ) -> Result<RetentionReconciliationReceipt, StoreError> {
        if crate::retention::provider_owned_target(request.target) {
            return Err(StoreError::InvalidTransition);
        }
        if now_tick == 0
            || request.namespace.is_empty()
            || request.key.is_empty()
            || request.expected_version == 0
        {
            return Err(StoreError::InvalidTransition);
        }
        let mut state = self.lock()?;
        let (source, owner_execution) = match request.target {
            RetentionTarget::TerminalExecutions => {
                if request.owner_execution.is_some() || request.namespace != "durable-execution" {
                    return Err(StoreError::InvalidTransition);
                }
                let key = state
                    .executions
                    .keys()
                    .find(|key| key.as_str() == request.key)
                    .cloned()
                    .ok_or(StoreError::NotFound)?;
                let execution = state.executions.get(&key).ok_or(StoreError::NotFound)?;
                if execution.version != request.expected_version
                    || !execution.state.terminal()
                    || execution.terminal_tick.is_some()
                {
                    return Err(StoreError::Conflict);
                }
                (
                    ProviderStateRecord {
                        namespace: request.namespace.clone(),
                        key: request.key.clone(),
                        version: execution.version,
                        payload: encode_execution(execution)?,
                    },
                    Some(execution.execution_id.clone()),
                )
            }
            RetentionTarget::TerminalWork => {
                if request.owner_execution.is_some() || request.namespace != "durable-work" {
                    return Err(StoreError::InvalidTransition);
                }
                let work = state.work.get(&request.key).ok_or(StoreError::NotFound)?;
                if work.lease_epoch.max(1) != request.expected_version
                    || !work.state.terminal()
                    || work.terminal_tick.is_some()
                {
                    return Err(StoreError::Conflict);
                }
                (
                    ProviderStateRecord {
                        namespace: request.namespace.clone(),
                        key: request.key.clone(),
                        version: work.lease_epoch.max(1),
                        payload: encode_work(work)?,
                    },
                    Some(work.execution_id.clone()),
                )
            }
            RetentionTarget::DeliveredOutbox => {
                if request.owner_execution.is_some() || request.namespace != "durable-outbox" {
                    return Err(StoreError::InvalidTransition);
                }
                let outbox = state.outbox.get(&request.key).ok_or(StoreError::NotFound)?;
                if outbox.version != request.expected_version
                    || !outbox.delivered
                    || outbox.delivered_tick.is_some()
                {
                    return Err(StoreError::Conflict);
                }
                (
                    ProviderStateRecord {
                        namespace: request.namespace.clone(),
                        key: request.key.clone(),
                        version: outbox.version,
                        payload: encode_outbox(outbox)?,
                    },
                    Some(outbox.execution_id.clone()),
                )
            }
            RetentionTarget::Db2Replay
            | RetentionTarget::ImsReplay
            | RetentionTarget::MqReplay
            | RetentionTarget::CicsReplay => {
                let owner = request
                    .owner_execution
                    .as_ref()
                    .ok_or(StoreError::InvalidTransition)?;
                if !memory_execution_prunable(&state, owner) {
                    return Err(StoreError::InvalidTransition);
                }
                let namespace = target_namespace(request.target)
                    .ok_or(StoreError::InvalidTransition)?
                    .to_string();
                if request.namespace != namespace {
                    return Err(StoreError::InvalidTransition);
                }
                let key = (namespace, request.key.clone());
                let row = state
                    .provider_state
                    .get(&key)
                    .cloned()
                    .ok_or(StoreError::NotFound)?;
                if row.version != request.expected_version {
                    return Err(StoreError::Conflict);
                }
                validate_memory_replay_reconciliation(&state, &row.key, owner)?;
                let already_aged = if request.target == RetentionTarget::CicsReplay {
                    cics_replay_metadata(&row.payload)?.is_some()
                } else {
                    replay_metadata(
                        &row.payload,
                        &row.key,
                        replay_schema(request.target).ok_or(StoreError::InvalidTransition)?,
                    )?
                    .is_some()
                };
                if already_aged {
                    return Err(StoreError::Conflict);
                }
                (row, Some(owner.clone()))
            }
            RetentionTarget::ResolvedEffects => {
                if request.owner_execution.is_some() || request.namespace != "durable-effect" {
                    return Err(StoreError::InvalidTransition);
                }
                let key = state
                    .effects
                    .keys()
                    .find(|key| key.as_str() == request.key)
                    .cloned()
                    .ok_or(StoreError::NotFound)?;
                let effect = state.effects.get(&key).ok_or(StoreError::NotFound)?;
                if effect.intent.epoch.max(1) != request.expected_version
                    || !matches!(effect.state, EffectState::Completed | EffectState::Failed)
                    || effect.resolved_tick.is_some()
                {
                    return Err(StoreError::Conflict);
                }
                (
                    ProviderStateRecord {
                        namespace: request.namespace.clone(),
                        key: request.key.clone(),
                        version: effect.intent.epoch.max(1),
                        payload: encode_effect(effect)?,
                    },
                    Some(effect.execution_id.clone()),
                )
            }
            RetentionTarget::LifecycleEvents => {
                if request.owner_execution.is_some()
                    || !request.namespace.starts_with("durable-event:")
                {
                    return Err(StoreError::InvalidTransition);
                }
                let execution_id = request
                    .namespace
                    .strip_prefix("durable-event:")
                    .ok_or(StoreError::InvalidTransition)?;
                let (owner, event) = state
                    .events
                    .iter()
                    .find(|(owner, _)| owner.as_str() == execution_id)
                    .and_then(|(owner, events)| {
                        request.key.parse::<u64>().ok().and_then(|sequence| {
                            events
                                .iter()
                                .find(|event| event.sequence == sequence)
                                .map(|event| (owner, event))
                        })
                    })
                    .ok_or(StoreError::NotFound)?;
                if event.tick != 0 || request.expected_version != 1 {
                    return Err(StoreError::Conflict);
                }
                (
                    ProviderStateRecord {
                        namespace: request.namespace.clone(),
                        key: request.key.clone(),
                        version: 1,
                        payload: encode_event(event)?,
                    },
                    Some(owner.clone()),
                )
            }
            RetentionTarget::Audit => {
                if request.owner_execution.is_some() || request.namespace != AUDIT_NAMESPACE {
                    return Err(StoreError::InvalidTransition);
                }
                let audit = state.audits.get(&request.key).ok_or(StoreError::NotFound)?;
                if audit.observed_tick != 0 || request.expected_version != 1 {
                    return Err(StoreError::Conflict);
                }
                (
                    ProviderStateRecord {
                        namespace: request.namespace.clone(),
                        key: request.key.clone(),
                        version: 1,
                        payload: encode_audit(audit)?,
                    },
                    Some(audit.execution_id.clone()),
                )
            }
            RetentionTarget::RacfEvidence
            | RetentionTarget::DatasetReplay
            | RetentionTarget::CicsUnitOfWork
            | RetentionTarget::CobolLifecycle
            | RetentionTarget::SpoolJobs
            | RetentionTarget::ConsoleLog => {
                return Err(StoreError::InvalidTransition);
            }
        };
        if source.version != request.expected_version {
            return Err(StoreError::Conflict);
        }
        let observation = RetentionObservation {
            target: request.target,
            namespace: source.namespace.clone(),
            key: source.key.clone(),
            source_version: source.version,
            source_digest: crate::retention::source_digest(&source.payload),
            observed_tick: now_tick,
            owner_execution,
        };
        crate::retention::observation::validate(&observation)?;
        let map_key = (
            observation.target,
            observation.namespace.clone(),
            observation.key.clone(),
        );
        let old = state.observations.get(&map_key).cloned();
        if let Some((version, current)) = &old
            && current.source_version == observation.source_version
            && current.source_digest == observation.source_digest
            && current.owner_execution == observation.owner_execution
        {
            return Ok(RetentionReconciliationReceipt {
                target: request.target,
                namespace: request.namespace,
                key: request.key,
                source_version: request.expected_version,
                observation_version: *version,
                reconciled_tick: current.observed_tick,
            });
        }
        let observation_version = old.as_ref().map_or(Ok(1), |(version, _)| {
            version.checked_add(1).ok_or(StoreError::CapacityExceeded)
        })?;
        if old.is_none() && state.observations.len() >= self.limits.max_retention_archive_rows {
            return Err(StoreError::CapacityExceeded);
        }
        let old_bytes = old
            .as_ref()
            .map(|(_, item)| crate::retention::observation::storage_bytes(item))
            .transpose()?
            .unwrap_or(0);
        let new_bytes = crate::retention::observation::storage_bytes(&observation)?;
        let total = state
            .observation_bytes
            .checked_sub(old_bytes)
            .and_then(|bytes| bytes.checked_add(new_bytes))
            .ok_or(StoreError::CapacityExceeded)?;
        if total > self.limits.max_retention_archive_bytes {
            return Err(StoreError::CapacityExceeded);
        }
        state.observation_bytes = total;
        state
            .observations
            .insert(map_key, (observation_version, observation));
        Self::bump_retention_epoch(&mut state)?;
        Ok(RetentionReconciliationReceipt {
            target: request.target,
            namespace: request.namespace,
            key: request.key,
            source_version: request.expected_version,
            observation_version,
            reconciled_tick: now_tick,
        })
    }

    fn retention_legacy_rows(
        &self,
        target: RetentionTarget,
        max: usize,
    ) -> Result<Vec<RetentionLegacyRow>, StoreError> {
        if crate::retention::provider_owned_target(target) {
            return Err(StoreError::InvalidTransition);
        }
        if max == 0 || max > mainframe_env_store_api::MAX_RETENTION_BATCH {
            return Err(StoreError::CapacityExceeded);
        }
        let state = self.lock()?;
        let mut rows = Vec::new();
        match target {
            RetentionTarget::TerminalExecutions => {
                for record in state.executions.values() {
                    let payload = encode_execution(record)?;
                    let version = memory_retention_version(
                        &state,
                        target,
                        record.execution_id.as_str(),
                        record.version,
                    );
                    if record.state.terminal()
                        && record.terminal_tick.is_none()
                        && memory_observed_age(
                            &state,
                            target,
                            "durable-execution",
                            record.execution_id.as_str(),
                            version,
                            &payload,
                        )
                        .is_none()
                    {
                        rows.push(RetentionLegacyRow {
                            target,
                            namespace: "durable-execution".into(),
                            key: record.execution_id.as_str().into(),
                            source_version: version,
                        });
                    }
                }
            }
            RetentionTarget::TerminalWork => {
                for record in state.work.values() {
                    let payload = encode_work(record)?;
                    let version = memory_retention_version(
                        &state,
                        target,
                        &record.work_id,
                        record.lease_epoch.max(1),
                    );
                    if record.state.terminal()
                        && record.terminal_tick.is_none()
                        && memory_observed_age(
                            &state,
                            target,
                            "durable-work",
                            &record.work_id,
                            version,
                            &payload,
                        )
                        .is_none()
                    {
                        rows.push(RetentionLegacyRow {
                            target,
                            namespace: "durable-work".into(),
                            key: record.work_id.clone(),
                            source_version: version,
                        });
                    }
                }
            }
            RetentionTarget::DeliveredOutbox => {
                for record in state.outbox.values() {
                    let payload = encode_outbox(record)?;
                    if record.delivered
                        && record.delivered_tick.is_none()
                        && memory_observed_age(
                            &state,
                            target,
                            "durable-outbox",
                            &record.notification_id,
                            record.version,
                            &payload,
                        )
                        .is_none()
                    {
                        rows.push(RetentionLegacyRow {
                            target,
                            namespace: "durable-outbox".into(),
                            key: record.notification_id.clone(),
                            source_version: record.version,
                        });
                    }
                }
            }
            RetentionTarget::ResolvedEffects => {
                for record in state.effects.values() {
                    let payload = encode_effect(record)?;
                    let version = memory_retention_version(
                        &state,
                        target,
                        record.key.as_str(),
                        record.intent.epoch.max(1),
                    );
                    if matches!(record.state, EffectState::Completed | EffectState::Failed)
                        && record.resolved_tick.is_none()
                        && memory_observed_age(
                            &state,
                            target,
                            "durable-effect",
                            record.key.as_str(),
                            version,
                            &payload,
                        )
                        .is_none()
                    {
                        rows.push(RetentionLegacyRow {
                            target,
                            namespace: "durable-effect".into(),
                            key: record.key.as_str().into(),
                            source_version: version,
                        });
                    }
                }
            }
            RetentionTarget::Db2Replay
            | RetentionTarget::ImsReplay
            | RetentionTarget::MqReplay
            | RetentionTarget::CicsReplay => {
                let namespace = target_namespace(target).ok_or(StoreError::InvalidTransition)?;
                for ((candidate_namespace, _), record) in &state.provider_state {
                    if candidate_namespace != namespace {
                        continue;
                    }
                    let missing = if target == RetentionTarget::CicsReplay {
                        cics_replay_metadata(&record.payload)?.is_none()
                    } else {
                        replay_metadata(
                            &record.payload,
                            &record.key,
                            replay_schema(target).ok_or(StoreError::InvalidTransition)?,
                        )?
                        .is_none()
                    };
                    if missing
                        && memory_observed_age(
                            &state,
                            target,
                            &record.namespace,
                            &record.key,
                            record.version,
                            &record.payload,
                        )
                        .is_none()
                    {
                        rows.push(RetentionLegacyRow {
                            target,
                            namespace: record.namespace.clone(),
                            key: record.key.clone(),
                            source_version: record.version,
                        });
                    }
                }
            }
            RetentionTarget::LifecycleEvents => {
                for (execution_id, events) in &state.events {
                    let namespace = format!("durable-event:{execution_id}");
                    for event in events {
                        let key = format!("{:020}", event.sequence);
                        let payload = encode_event(event)?;
                        if event.tick == 0
                            && memory_observed_age(&state, target, &namespace, &key, 1, &payload)
                                .is_none()
                        {
                            rows.push(RetentionLegacyRow {
                                target,
                                namespace: namespace.clone(),
                                key,
                                source_version: 1,
                            });
                        }
                    }
                }
            }
            RetentionTarget::Audit => {
                for (key, audit) in &state.audits {
                    let payload = encode_audit(audit)?;
                    if audit.observed_tick == 0
                        && memory_observed_age(&state, target, AUDIT_NAMESPACE, key, 1, &payload)
                            .is_none()
                    {
                        rows.push(RetentionLegacyRow {
                            target,
                            namespace: AUDIT_NAMESPACE.into(),
                            key: key.clone(),
                            source_version: 1,
                        });
                    }
                }
            }
            RetentionTarget::RacfEvidence
            | RetentionTarget::DatasetReplay
            | RetentionTarget::CicsUnitOfWork
            | RetentionTarget::CobolLifecycle
            | RetentionTarget::SpoolJobs
            | RetentionTarget::ConsoleLog => {
                return Err(StoreError::InvalidTransition);
            }
        }
        rows.sort_by(|left, right| left.key.cmp(&right.key));
        rows.truncate(max);
        Ok(rows)
    }
}

fn insert_memory_archive(
    state: &mut State,
    archive: RetentionArchive,
    limits: StoreLimits,
) -> Result<(), StoreError> {
    let current_rows = state
        .archives
        .values()
        .map(|archive| archive.rows.len())
        .sum::<usize>();
    let payload_bytes = crate::retention::archive_storage_bytes(&archive.rows)?;
    if current_rows
        .checked_add(archive.rows.len())
        .is_none_or(|rows| rows > limits.max_retention_archive_rows)
        || state
            .archive_bytes
            .checked_add(payload_bytes)
            .is_none_or(|bytes| bytes > limits.max_retention_archive_bytes)
        || state.archives.contains_key(&archive.archive_id)
    {
        return Err(StoreError::CapacityExceeded);
    }
    state.archive_bytes += payload_bytes;
    state.archives.insert(archive.archive_id.clone(), archive);
    Ok(())
}

fn remove_memory_observation(
    state: &mut State,
    target: RetentionTarget,
    row: &ArchivedRetentionRow,
) -> Result<(), StoreError> {
    let key = (target, row.namespace.clone(), row.key.clone());
    if let Some((_, observation)) = state.observations.remove(&key) {
        let bytes = crate::retention::observation::storage_bytes(&observation)?;
        state.observation_bytes = state.observation_bytes.saturating_sub(bytes);
    }
    Ok(())
}

type MemoryCoreDependencies = (BTreeMap<ExecutionId, ()>, BTreeMap<String, ()>, bool);

fn memory_core_dependencies(
    state: &State,
    supplied: Option<&CoreRetentionDependencySnapshot>,
) -> Result<MemoryCoreDependencies, StoreError> {
    let Some(supplied) = supplied else {
        return Ok((BTreeMap::new(), BTreeMap::new(), false));
    };
    if supplied.expected_epoch != state.provider_epoch
        || supplied.blocked_executions.len()
            > mainframe_env_store_api::MAX_CORE_RETENTION_DEPENDENCIES
        || supplied.blocked_effect_keys.len()
            > mainframe_env_store_api::MAX_CORE_RETENTION_DEPENDENCIES
    {
        return Err(StoreError::Conflict);
    }
    let executions = supplied
        .blocked_executions
        .iter()
        .cloned()
        .map(|id| (id, ()))
        .collect::<BTreeMap<_, _>>();
    let effects = supplied
        .blocked_effect_keys
        .iter()
        .map(|key| (key.as_str().to_owned(), ()))
        .collect::<BTreeMap<_, _>>();
    if executions.len() != supplied.blocked_executions.len()
        || effects.len() != supplied.blocked_effect_keys.len()
    {
        return Err(StoreError::IncompatibleVersion);
    }
    Ok((executions, effects, supplied.unowned))
}

fn memory_candidates(
    state: &State,
    target: RetentionTarget,
    watermark: u64,
    window_open: bool,
    dependencies: Option<&CoreRetentionDependencySnapshot>,
) -> Result<(usize, Vec<(u64, ArchivedRetentionRow)>), StoreError> {
    let (provider_executions, provider_effects, unowned_provider) =
        memory_core_dependencies(state, dependencies)?;
    let mut rows = Vec::new();
    let active = match target {
        RetentionTarget::TerminalExecutions => {
            for record in state.executions.values() {
                let payload = encode_execution(record)?;
                let version = memory_retention_version(
                    state,
                    RetentionTarget::TerminalExecutions,
                    record.execution_id.as_str(),
                    record.version,
                );
                let Some(terminal_tick) =
                    record.terminal_tick.filter(|tick| *tick != 0).or_else(|| {
                        memory_observed_age(
                            state,
                            RetentionTarget::TerminalExecutions,
                            "durable-execution",
                            record.execution_id.as_str(),
                            version,
                            &payload,
                        )
                        .map(|item| item.observed_tick)
                    })
                else {
                    continue;
                };
                let blocked = state.checkpoints.contains_key(&record.execution_id)
                    || state
                        .events
                        .get(&record.execution_id)
                        .is_some_and(|events| !events.is_empty())
                    || state
                        .outbox
                        .values()
                        .any(|item| item.execution_id == record.execution_id)
                    || state
                        .effects
                        .values()
                        .any(|item| item.execution_id == record.execution_id)
                    || state
                        .work
                        .values()
                        .any(|item| item.execution_id == record.execution_id)
                    || state
                        .audits
                        .values()
                        .any(|item| item.execution_id == record.execution_id)
                    || provider_executions.contains_key(&record.execution_id)
                    || unowned_provider;
                if record.state.terminal() && window_open && terminal_tick <= watermark && !blocked
                {
                    rows.push((
                        terminal_tick,
                        ArchivedRetentionRow {
                            namespace: "durable-execution".into(),
                            key: record.execution_id.as_str().into(),
                            version,
                            payload,
                            retention_tick: terminal_tick,
                            owner_execution: Some(record.execution_id.clone()),
                        },
                    ));
                }
            }
            state.executions.len()
        }
        RetentionTarget::TerminalWork => {
            for record in state.work.values() {
                let payload = encode_work(record)?;
                let version = memory_retention_version(
                    state,
                    RetentionTarget::TerminalWork,
                    &record.work_id,
                    record.lease_epoch.max(1),
                );
                let Some(terminal_tick) =
                    record.terminal_tick.filter(|tick| *tick != 0).or_else(|| {
                        memory_observed_age(
                            state,
                            RetentionTarget::TerminalWork,
                            "durable-work",
                            &record.work_id,
                            version,
                            &payload,
                        )
                        .map(|item| item.observed_tick)
                    })
                else {
                    continue;
                };
                let blocked = state.checkpoints.contains_key(&record.execution_id)
                    || state.effects.values().any(|effect| {
                        effect.execution_id == record.execution_id
                            && matches!(
                                effect.state,
                                EffectState::Intent | EffectState::UnknownOutcome
                            )
                    })
                    || state.work.values().any(|work| {
                        work.execution_id == record.execution_id && !work.state.terminal()
                    })
                    || provider_executions.contains_key(&record.execution_id)
                    || unowned_provider;
                if record.state.terminal()
                    && window_open
                    && terminal_tick <= watermark
                    && memory_execution_prunable(state, &record.execution_id)
                    && !blocked
                {
                    rows.push((
                        terminal_tick,
                        ArchivedRetentionRow {
                            namespace: "durable-work".into(),
                            key: record.work_id.clone(),
                            version,
                            payload,
                            retention_tick: terminal_tick,
                            owner_execution: Some(record.execution_id.clone()),
                        },
                    ));
                }
            }
            state.work.len()
        }
        RetentionTarget::LifecycleEvents => {
            for (execution_id, events) in &state.events {
                let prunable = state
                    .executions
                    .get(execution_id)
                    .is_some_and(|execution| execution.state.terminal())
                    && !state.checkpoints.contains_key(execution_id)
                    && !state
                        .outbox
                        .values()
                        .any(|item| item.execution_id == *execution_id)
                    && !state
                        .effects
                        .values()
                        .any(|item| item.execution_id == *execution_id)
                    && !state
                        .work
                        .values()
                        .any(|item| item.execution_id == *execution_id)
                    && !state
                        .audits
                        .values()
                        .any(|item| item.execution_id == *execution_id)
                    && !provider_executions.contains_key(execution_id)
                    && !unowned_provider;
                if prunable && window_open {
                    rows.extend(
                        events
                            .iter()
                            .filter_map(|event| {
                                let namespace = format!("durable-event:{execution_id}");
                                let key = format!("{:020}", event.sequence);
                                let payload = encode_event(event).ok()?;
                                let retention_tick =
                                    (event.tick != 0).then_some(event.tick).or_else(|| {
                                        memory_observed_age(
                                            state,
                                            RetentionTarget::LifecycleEvents,
                                            &namespace,
                                            &key,
                                            1,
                                            &payload,
                                        )
                                        .map(|item| item.observed_tick)
                                    })?;
                                (retention_tick <= watermark).then(|| {
                                    Ok((
                                        retention_tick,
                                        ArchivedRetentionRow {
                                            namespace,
                                            key,
                                            version: 1,
                                            payload,
                                            retention_tick,
                                            owner_execution: Some(execution_id.clone()),
                                        },
                                    ))
                                })
                            })
                            .collect::<Result<Vec<_>, StoreError>>()?,
                    );
                }
            }
            state.events.values().map(Vec::len).sum()
        }
        RetentionTarget::DeliveredOutbox => {
            for record in state.outbox.values() {
                let payload = encode_outbox(record)?;
                let retention_tick =
                    record.delivered_tick.filter(|tick| *tick != 0).or_else(|| {
                        memory_observed_age(
                            state,
                            RetentionTarget::DeliveredOutbox,
                            "durable-outbox",
                            &record.notification_id,
                            record.version,
                            &payload,
                        )
                        .map(|item| item.observed_tick)
                    });
                if record.delivered
                    && retention_tick.is_some_and(|tick| window_open && tick <= watermark)
                    && memory_execution_prunable(state, &record.execution_id)
                    && let Some(delivered_tick) = retention_tick
                {
                    rows.push((
                        delivered_tick,
                        ArchivedRetentionRow {
                            namespace: "durable-outbox".into(),
                            key: record.notification_id.clone(),
                            version: record.version,
                            payload,
                            retention_tick: delivered_tick,
                            owner_execution: Some(record.execution_id.clone()),
                        },
                    ));
                }
            }
            state.outbox.len()
        }
        RetentionTarget::ResolvedEffects => {
            for record in state.effects.values() {
                let payload = encode_effect(record)?;
                let version = memory_retention_version(
                    state,
                    RetentionTarget::ResolvedEffects,
                    record.key.as_str(),
                    record.intent.epoch.max(1),
                );
                let retention_tick = record.resolved_tick.filter(|tick| *tick != 0).or_else(|| {
                    memory_observed_age(
                        state,
                        RetentionTarget::ResolvedEffects,
                        "durable-effect",
                        record.key.as_str(),
                        version,
                        &payload,
                    )
                    .map(|item| item.observed_tick)
                });
                if matches!(record.state, EffectState::Completed | EffectState::Failed)
                    && window_open
                    && retention_tick.is_some_and(|tick| tick <= watermark)
                    && memory_execution_prunable(state, &record.execution_id)
                    && !provider_effects.contains_key(record.key.as_str())
                    && !unowned_provider
                    && let Some(resolved_tick) = retention_tick
                {
                    rows.push((
                        resolved_tick,
                        ArchivedRetentionRow {
                            namespace: "durable-effect".into(),
                            key: record.key.as_str().into(),
                            version,
                            payload,
                            retention_tick: resolved_tick,
                            owner_execution: Some(record.execution_id.clone()),
                        },
                    ));
                }
            }
            state.effects.len()
        }
        RetentionTarget::Db2Replay | RetentionTarget::ImsReplay | RetentionTarget::MqReplay => {
            let namespace = target_namespace(target).ok_or(StoreError::InvalidTransition)?;
            for ((candidate_namespace, _), record) in &state.provider_state {
                if candidate_namespace == namespace {
                    let age = replay_metadata(
                        &record.payload,
                        &record.key,
                        replay_schema(target).ok_or(StoreError::InvalidTransition)?,
                    )?
                    .map(|metadata| (metadata.deadline_tick, metadata.owner_execution))
                    .or_else(|| {
                        memory_observed_age(
                            state,
                            target,
                            &record.namespace,
                            &record.key,
                            record.version,
                            &record.payload,
                        )
                        .and_then(|item| {
                            item.owner_execution
                                .clone()
                                .map(|owner| (item.observed_tick, owner))
                        })
                    });
                    if let Some((retention_tick, owner)) = age
                        && window_open
                        && retention_tick <= watermark
                        && memory_execution_prunable(state, &owner)
                        && memory_effect_recovery_is_clear(state, &record.key)
                    {
                        rows.push((
                            retention_tick,
                            ArchivedRetentionRow {
                                namespace: record.namespace.clone(),
                                key: record.key.clone(),
                                version: record.version,
                                payload: record.payload.clone(),
                                retention_tick,
                                owner_execution: Some(owner),
                            },
                        ));
                    }
                }
            }
            state
                .provider_state
                .keys()
                .filter(|(candidate, _)| candidate == namespace)
                .count()
        }
        RetentionTarget::CicsReplay => {
            for ((namespace, _), record) in &state.provider_state {
                if namespace != "cics-effect-replay-v1" {
                    continue;
                }
                let age = cics_replay_metadata(&record.payload)?
                    .map(|metadata| (metadata.deadline_tick, metadata.owner_execution))
                    .or_else(|| {
                        memory_observed_age(
                            state,
                            target,
                            &record.namespace,
                            &record.key,
                            record.version,
                            &record.payload,
                        )
                        .and_then(|item| {
                            item.owner_execution
                                .clone()
                                .map(|owner| (item.observed_tick, owner))
                        })
                    });
                if let Some((retention_tick, owner)) = age
                    && window_open
                    && retention_tick <= watermark
                    && memory_execution_prunable(state, &owner)
                    && memory_effect_recovery_is_clear(state, &record.key)
                {
                    rows.push((
                        retention_tick,
                        ArchivedRetentionRow {
                            namespace: record.namespace.clone(),
                            key: record.key.clone(),
                            version: record.version,
                            payload: record.payload.clone(),
                            retention_tick,
                            owner_execution: Some(owner),
                        },
                    ));
                }
            }
            state
                .provider_state
                .keys()
                .filter(|(namespace, _)| namespace == "cics-effect-replay-v1")
                .count()
        }
        RetentionTarget::Audit => {
            let unresolved = state
                .effects
                .values()
                .filter(|effect| {
                    matches!(
                        effect.state,
                        EffectState::Intent | EffectState::UnknownOutcome
                    )
                })
                .map(|effect| effect.execution_id.clone())
                .collect::<std::collections::BTreeSet<_>>();
            for (key, record) in &state.audits {
                let payload = encode_audit(record)?;
                let retention_tick = (record.observed_tick != 0)
                    .then_some(record.observed_tick)
                    .or_else(|| {
                        memory_observed_age(
                            state,
                            RetentionTarget::Audit,
                            AUDIT_NAMESPACE,
                            key,
                            1,
                            &payload,
                        )
                        .map(|item| item.observed_tick)
                    });
                let owner_ready = state
                    .executions
                    .get(&record.execution_id)
                    .is_none_or(|execution| execution.state.terminal());
                if window_open
                    && retention_tick.is_some_and(|tick| tick <= watermark)
                    && owner_ready
                    && !state.checkpoints.contains_key(&record.execution_id)
                    && memory_effect_recovery_is_clear(state, record.invocation_key.as_str())
                    && !unresolved.contains(&record.execution_id)
                {
                    rows.push((
                        retention_tick.ok_or(StoreError::Conflict)?,
                        ArchivedRetentionRow {
                            namespace: AUDIT_NAMESPACE.into(),
                            key: key.clone(),
                            version: 1,
                            payload,
                            retention_tick: retention_tick.ok_or(StoreError::Conflict)?,
                            owner_execution: Some(record.execution_id.clone()),
                        },
                    ));
                }
            }
            state.audits.len()
        }
        RetentionTarget::RacfEvidence
        | RetentionTarget::DatasetReplay
        | RetentionTarget::CicsUnitOfWork
        | RetentionTarget::CobolLifecycle
        | RetentionTarget::SpoolJobs
        | RetentionTarget::ConsoleLog => return Err(StoreError::InvalidTransition),
    };
    rows.sort_by(|left, right| {
        left.0
            .cmp(&right.0)
            .then_with(|| left.1.namespace.cmp(&right.1.namespace))
            .then_with(|| left.1.key.cmp(&right.1.key))
    });
    Ok((active, rows))
}

fn memory_observed_age<'a>(
    state: &'a State,
    target: RetentionTarget,
    namespace: &str,
    key: &str,
    version: u64,
    payload: &[u8],
) -> Option<&'a RetentionObservation> {
    state
        .observations
        .get(&(target, namespace.to_string(), key.to_string()))
        .map(|(_, observation)| observation)
        .filter(|observation| {
            crate::retention::observation_matches_source(observation, version, payload)
        })
}

fn validate_memory_provider_dependency(
    state: &State,
    candidate: &ProviderRetentionRow,
) -> Result<(), StoreError> {
    match &candidate.dependency {
        ProviderRetentionDependency::CoreEffect {
            key,
            request_digest,
            result_digest,
        } => {
            let owner = candidate
                .owner_execution
                .as_ref()
                .ok_or(StoreError::IncompatibleVersion)?;
            let run = candidate
                .owner_run_unit
                .as_ref()
                .ok_or(StoreError::IncompatibleVersion)?;
            if !memory_execution_prunable(state, owner)
                || state.effects.values().any(|effect| {
                    effect.execution_id == *owner
                        && effect.run_unit_id == *run
                        && matches!(
                            effect.state,
                            EffectState::Intent | EffectState::UnknownOutcome
                        )
                })
            {
                return Err(StoreError::Conflict);
            }
            let effect = state.effects.get(key).ok_or(StoreError::Conflict)?;
            if effect.execution_id != *owner
                || effect.run_unit_id != *run
                || effect.state != EffectState::Completed
                || effect.digest_format
                    != mainframe_env_store_api::EffectDigestFormat::CanonicalHostV1
                || effect.request_digest != *request_digest
                || effect.result_digest != Some(*result_digest)
                || effect
                    .resolved_tick
                    .is_none_or(|tick| tick == 0 || candidate.retention_tick < tick)
            {
                return Err(StoreError::Conflict);
            }
        }
        ProviderRetentionDependency::CicsNested { provenance, absent } => {
            let owner = candidate
                .owner_execution
                .as_ref()
                .ok_or(StoreError::IncompatibleVersion)?;
            let run = candidate
                .owner_run_unit
                .as_ref()
                .ok_or(StoreError::IncompatibleVersion)?;
            if !memory_execution_prunable(state, owner)
                || state.effects.values().any(|effect| {
                    effect.execution_id == *owner
                        && effect.run_unit_id == *run
                        && matches!(
                            effect.state,
                            EffectState::Intent | EffectState::UnknownOutcome
                        )
                })
            {
                return Err(StoreError::Conflict);
            }
            if state
                .provider_state
                .get(&(provenance.namespace.clone(), provenance.key.clone()))
                != Some(provenance)
                || absent.iter().any(|identity| {
                    if identity.namespace == "durable-effect" {
                        state.effects.keys().any(|key| key.as_str() == identity.key)
                    } else {
                        state
                            .provider_state
                            .contains_key(&(identity.namespace.clone(), identity.key.clone()))
                    }
                })
            {
                return Err(StoreError::Conflict);
            }
            let mut terminal_origin = false;
            for effect in state.effects.values().filter(|effect| {
                effect.execution_id == *owner
                    && effect.run_unit_id == *run
                    && effect.key.as_str() == provenance.key
            }) {
                if matches!(
                    effect.state,
                    EffectState::Intent | EffectState::UnknownOutcome
                ) {
                    return Err(StoreError::Conflict);
                }
                terminal_origin |= effect.state == EffectState::Completed
                    && effect.digest_format
                        == mainframe_env_store_api::EffectDigestFormat::CanonicalHostV1
                    && effect.intent.capability.as_ref().map(|item| item.as_str())
                        == Some("host.cics.execute")
                    && effect
                        .resolved_tick
                        .is_some_and(|tick| tick != 0 && candidate.retention_tick >= tick);
            }
            if !terminal_origin
                || state
                    .provider_state
                    .contains_key(&("cics-uow-undo".into(), run.as_str().into()))
            {
                return Err(StoreError::Conflict);
            }
        }
        ProviderRetentionDependency::ProviderGraph {
            required_rows,
            required_executions,
        } => {
            for required in required_rows {
                if state
                    .provider_state
                    .get(&(required.namespace.clone(), required.key.clone()))
                    != Some(required)
                {
                    return Err(StoreError::Conflict);
                }
            }
            for required in required_executions {
                let execution = state.executions.get(required).ok_or(StoreError::Conflict)?;
                if !execution.state.terminal()
                    || state.checkpoints.contains_key(required)
                    || state.effects.values().any(|effect| {
                        effect.execution_id == *required
                            && matches!(
                                effect.state,
                                EffectState::Intent | EffectState::UnknownOutcome
                            )
                    })
                {
                    return Err(StoreError::Conflict);
                }
            }
            if let Some(owner) = &candidate.owner_execution {
                let run = candidate
                    .owner_run_unit
                    .as_ref()
                    .ok_or(StoreError::IncompatibleVersion)?;
                let execution = state.executions.get(owner).ok_or(StoreError::Conflict)?;
                if !execution.state.terminal()
                    || execution.run_unit_id != *run
                    || state.checkpoints.contains_key(owner)
                    || state.effects.values().any(|effect| {
                        effect.execution_id == *owner
                            && effect.run_unit_id == *run
                            && matches!(
                                effect.state,
                                EffectState::Intent | EffectState::UnknownOutcome
                            )
                    })
                {
                    return Err(StoreError::Conflict);
                }
            }
        }
        ProviderRetentionDependency::DirectProduct => {
            if let Some(owner) = &candidate.owner_execution
                && let Some(execution) = state.executions.get(owner)
            {
                let run = candidate
                    .owner_run_unit
                    .as_ref()
                    .ok_or(StoreError::IncompatibleVersion)?;
                if !execution.state.terminal()
                    || execution.run_unit_id != *run
                    || state.checkpoints.contains_key(owner)
                    || state.effects.values().any(|effect| {
                        effect.execution_id == *owner
                            && effect.run_unit_id == *run
                            && matches!(
                                effect.state,
                                EffectState::Intent | EffectState::UnknownOutcome
                            )
                    })
                {
                    return Err(StoreError::Conflict);
                }
            }
        }
        ProviderRetentionDependency::None => {
            if candidate.owner_execution.is_some() || candidate.owner_run_unit.is_some() {
                return Err(StoreError::IncompatibleVersion);
            }
        }
    }
    Ok(())
}

fn memory_execution_prunable(state: &State, execution_id: &ExecutionId) -> bool {
    state
        .executions
        .get(execution_id)
        .is_some_and(|execution| execution.state.terminal())
        && !state.checkpoints.contains_key(execution_id)
}

fn memory_retention_version(
    state: &State,
    target: RetentionTarget,
    key: &str,
    default: u64,
) -> u64 {
    state
        .retention_cas_versions
        .get(&(target, key.to_string()))
        .copied()
        .unwrap_or(default)
}

fn memory_effect_recovery_is_clear(state: &State, idempotency_key: &str) -> bool {
    state
        .effects
        .iter()
        .find(|(key, _)| key.as_str() == idempotency_key)
        .is_none_or(|(_, effect)| {
            matches!(effect.state, EffectState::Completed | EffectState::Failed)
        })
}

fn validate_memory_replay_reconciliation(
    state: &State,
    idempotency_key: &str,
    owner: &ExecutionId,
) -> Result<(), StoreError> {
    let Some((_, effect)) = state
        .effects
        .iter()
        .find(|(key, _)| key.as_str() == idempotency_key)
    else {
        return Ok(());
    };
    if effect.execution_id != *owner {
        return Err(StoreError::Conflict);
    }
    if matches!(effect.state, EffectState::Completed | EffectState::Failed) {
        Ok(())
    } else {
        Err(StoreError::InvalidTransition)
    }
}

fn remove_memory_row(state: &mut State, row: &ArchivedRetentionRow) -> Result<(), StoreError> {
    match row.namespace.as_str() {
        "durable-execution" => {
            let key = state
                .executions
                .keys()
                .find(|key| key.as_str() == row.key)
                .cloned()
                .ok_or(StoreError::Conflict)?;
            let expected = memory_retention_version(
                state,
                RetentionTarget::TerminalExecutions,
                &row.key,
                state
                    .executions
                    .get(&key)
                    .ok_or(StoreError::Conflict)?
                    .version,
            );
            if expected != row.version {
                return Err(StoreError::Conflict);
            }
            state.executions.remove(&key).ok_or(StoreError::Conflict)?;
            state
                .retention_cas_versions
                .remove(&(RetentionTarget::TerminalExecutions, row.key.clone()));
        }
        "durable-work" => {
            let work = state.work.get(&row.key).ok_or(StoreError::Conflict)?;
            let expected = memory_retention_version(
                state,
                RetentionTarget::TerminalWork,
                &row.key,
                work.lease_epoch.max(1),
            );
            if expected != row.version {
                return Err(StoreError::Conflict);
            }
            let removed = state.work.remove(&row.key).ok_or(StoreError::Conflict)?;
            state
                .retention_cas_versions
                .remove(&(RetentionTarget::TerminalWork, row.key.clone()));
            state.blob_bytes = state.blob_bytes.saturating_sub(removed.payload.len());
        }
        "durable-outbox" => {
            let removed = state.outbox.remove(&row.key).ok_or(StoreError::Conflict)?;
            if removed.version != row.version {
                return Err(StoreError::Conflict);
            }
            state.blob_bytes = state.blob_bytes.saturating_sub(removed.payload.len());
        }
        "durable-effect" => {
            let key = state
                .effects
                .keys()
                .find(|key| key.as_str() == row.key)
                .cloned()
                .ok_or(StoreError::Conflict)?;
            let effect = state.effects.get(&key).ok_or(StoreError::Conflict)?;
            let expected = memory_retention_version(
                state,
                RetentionTarget::ResolvedEffects,
                &row.key,
                effect.intent.epoch.max(1),
            );
            if expected != row.version {
                return Err(StoreError::Conflict);
            }
            state.effects.remove(&key);
            state
                .retention_cas_versions
                .remove(&(RetentionTarget::ResolvedEffects, row.key.clone()));
        }
        AUDIT_NAMESPACE => {
            let audit = decode_audit(&row.payload)?;
            let current = state.audits.get(&row.key).ok_or(StoreError::Conflict)?;
            if current != &audit {
                return Err(StoreError::Conflict);
            }
            state.audits.remove(&row.key);
        }
        namespace if namespace.starts_with("durable-event:") => {
            let execution_id = namespace.trim_start_matches("durable-event:");
            let events = state
                .events
                .iter_mut()
                .find(|(id, _)| id.as_str() == execution_id)
                .map(|(_, events)| events)
                .ok_or(StoreError::Conflict)?;
            let sequence = row.key.parse::<u64>().map_err(|_| StoreError::Conflict)?;
            let before = events.len();
            events.retain(|event| event.sequence != sequence);
            if events.len() == before {
                return Err(StoreError::Conflict);
            }
        }
        _ => {
            let key = (row.namespace.clone(), row.key.clone());
            let removed = state
                .provider_state
                .remove(&key)
                .ok_or(StoreError::Conflict)?;
            if removed.version != row.version {
                return Err(StoreError::Conflict);
            }
            state.blob_bytes = state.blob_bytes.saturating_sub(removed.payload.len());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use mainframe_env_execution_api::{
        InvocationLimits, LifecycleEventKind, PrincipalId, RunUnitId, Selector,
    };
    use sha2::{Digest, Sha256};

    struct Ids {
        execution: ExecutionId,
        run: RunUnitId,
        artifact: ArtifactRef,
        principal: PrincipalId,
        selector: Selector,
        idem: IdempotencyKey,
    }
    fn ids() -> Ids {
        let limits = InvocationLimits::default();
        Ids {
            execution: ExecutionId::new("exec-1", limits).unwrap(),
            run: RunUnitId::new("run-1", limits).unwrap(),
            artifact: ArtifactRef::new("sha256:abc", limits).unwrap(),
            principal: PrincipalId::new("IBMUSER", limits).unwrap(),
            selector: Selector::new("program:HELLO", limits).unwrap(),
            idem: IdempotencyKey::new("idem-1", limits).unwrap(),
        }
    }
    fn execution(ids: &Ids) -> ExecutionRecord {
        ExecutionRecord {
            execution_id: ids.execution.clone(),
            run_unit_id: ids.run.clone(),
            selector: ids.selector.clone(),
            artifact: ids.artifact.clone(),
            principal: ids.principal.clone(),
            state: ExecutionState::Admitted,
            attempt: 1,
            version: 1,
            owner_lease: None,
            lease_expiry_tick: None,
            terminal_tick: None,
        }
    }

    #[test]
    fn execution_transition_is_optimistic_and_monotonic() {
        let store = MemoryStore::new(StoreLimits::default());
        let ids = ids();
        store.create_execution(execution(&ids)).unwrap();
        let queued = store
            .transition_execution(&ids.execution, 1, ExecutionState::Queued, 1)
            .unwrap();
        assert_eq!(queued.version, 2);
        assert_eq!(
            store.transition_execution(&ids.execution, 1, ExecutionState::Running, 1),
            Err(StoreError::Conflict)
        );
    }

    #[test]
    fn terminal_age_reconciliation_preserves_journal_sequence_version() {
        let store = MemoryStore::new(StoreLimits::default());
        let ids = ids();
        let notification = |sequence| OutboxRecord {
            notification_id: format!("retention-journal-{sequence}"),
            execution_id: ids.execution.clone(),
            sequence,
            topic: "execution.lifecycle".into(),
            payload: vec![u8::try_from(sequence).unwrap()],
            attempt: 0,
            delivered: false,
            delivered_tick: None,
            version: 1,
        };
        store
            .admit_execution(
                execution(&ids),
                LifecycleEvent {
                    execution_id: ids.execution.clone(),
                    run_unit_id: ids.run.clone(),
                    sequence: 1,
                    attempt: 1,
                    tick: 1,
                    kind: LifecycleEventKind::Admitted,
                },
                notification(1),
            )
            .unwrap();
        let mut version = 1;
        for (sequence, state, kind) in [
            (2, ExecutionState::Queued, LifecycleEventKind::Queued),
            (3, ExecutionState::Running, LifecycleEventKind::Started),
            (
                4,
                ExecutionState::Completing,
                LifecycleEventKind::Completing,
            ),
            (
                5,
                ExecutionState::Completed,
                LifecycleEventKind::Completed { return_code: 0 },
            ),
        ] {
            version = store
                .commit_execution_step(
                    &ids.execution,
                    version,
                    Some(state),
                    LifecycleEvent {
                        execution_id: ids.execution.clone(),
                        run_unit_id: ids.run.clone(),
                        sequence,
                        attempt: 1,
                        tick: sequence,
                        kind,
                    },
                    None,
                    None,
                    None,
                    notification(sequence),
                )
                .unwrap()
                .version;
        }
        {
            let mut state = store.lock().unwrap();
            state
                .executions
                .get_mut(&ids.execution)
                .unwrap()
                .terminal_tick = None;
        }
        let candidate = store
            .retention_legacy_rows(RetentionTarget::TerminalExecutions, 1)
            .unwrap()
            .pop()
            .unwrap();
        let receipt = store
            .reconcile_retention_age(
                RetentionAgeReconciliation {
                    target: RetentionTarget::TerminalExecutions,
                    namespace: "durable-execution".into(),
                    key: ids.execution.as_str().into(),
                    expected_version: candidate.source_version,
                    owner_execution: None,
                },
                100,
            )
            .unwrap();
        let execution = store.get_execution(&ids.execution).unwrap().unwrap();
        let events = store.events(&ids.execution, 1, 8).unwrap();
        assert_eq!(execution.version, 5);
        assert_eq!(events.last().unwrap().sequence, execution.version);
        assert_eq!(execution.terminal_tick, None);
        assert_eq!((receipt.source_version, receipt.reconciled_tick), (5, 100));
        assert_eq!(
            store
                .provider_retention_observations(RetentionTarget::TerminalExecutions, 1)
                .unwrap()[0]
                .observed_tick,
            100
        );
    }

    #[test]
    fn event_order_and_capacity_are_enforced() {
        let store = MemoryStore::new(StoreLimits {
            max_events_per_execution: 1,
            ..StoreLimits::default()
        });
        let ids = ids();
        let event = LifecycleEvent {
            execution_id: ids.execution.clone(),
            run_unit_id: ids.run.clone(),
            sequence: 1,
            attempt: 1,
            tick: 1,
            kind: LifecycleEventKind::Admitted,
        };
        store.append_event(event.clone()).unwrap();
        assert_eq!(
            store.append_event(LifecycleEvent {
                sequence: 2,
                ..event
            }),
            Err(StoreError::CapacityExceeded)
        );
    }

    #[test]
    fn lease_expiry_redelivers_with_higher_attempt() {
        let store = MemoryStore::new(StoreLimits::default());
        let ids = ids();
        store
            .enqueue(WorkRecord {
                work_id: "work-1".into(),
                execution_id: ids.execution,
                required_selector: ids.selector,
                required_generation: "test@1".into(),
                artifact: ids.artifact,
                state: WorkState::Queued,
                priority: 0,
                attempt: 0,
                max_attempts: 3,
                available_tick: 1,
                deadline_tick: 100,
                cancellation_requested: false,
                worker_id: None,
                lease_id: None,
                lease_epoch: 0,
                lease_expiry_tick: None,
                heartbeat_tick: None,
                terminal_tick: None,
                checkpoint_id: None,
                effect_sequence: 0,
                payload: vec![1],
            })
            .unwrap();
        let first = store.claim("worker", None, 1, 1).unwrap().unwrap();
        let second = store.claim("worker", None, 2, 1).unwrap().unwrap();
        assert_eq!((first.attempt, second.attempt), (1, 2));
        assert_eq!((first.lease_epoch, second.lease_epoch), (1, 2));
        assert_eq!(
            store.complete(
                "work-1",
                first.lease_id.as_deref().unwrap(),
                first.lease_epoch,
                2,
            ),
            Err(StoreError::LeaseConflict)
        );
        store
            .complete(
                "work-1",
                second.lease_id.as_deref().unwrap(),
                second.lease_epoch,
                2,
            )
            .unwrap();
    }

    #[test]
    fn heartbeat_and_attempt_policy_control_terminal_work_state() {
        let store = MemoryStore::new(StoreLimits::default());
        let ids = ids();
        store
            .enqueue(WorkRecord {
                work_id: "bounded-work".into(),
                execution_id: ids.execution,
                required_selector: ids.selector,
                required_generation: "test@1".into(),
                artifact: ids.artifact,
                state: WorkState::Queued,
                priority: 0,
                attempt: 0,
                max_attempts: 1,
                available_tick: 1,
                deadline_tick: 100,
                cancellation_requested: false,
                worker_id: None,
                lease_id: None,
                lease_epoch: 0,
                lease_expiry_tick: None,
                heartbeat_tick: None,
                terminal_tick: None,
                checkpoint_id: None,
                effect_sequence: 0,
                payload: vec![1],
            })
            .unwrap();
        let claimed = store.claim("worker", None, 1, 2).unwrap().unwrap();
        let lease = claimed.lease_id.as_deref().unwrap();
        let heartbeat = store
            .heartbeat("bounded-work", lease, claimed.lease_epoch, 2, 10)
            .unwrap();
        assert_eq!(heartbeat.heartbeat_tick, Some(2));
        assert!(store.claim("other", None, 3, 1).unwrap().is_none());
        assert_eq!(
            store
                .release("bounded-work", lease, claimed.lease_epoch, 3, 4)
                .unwrap()
                .state,
            WorkState::DeadLetter
        );
    }

    #[test]
    fn journal_admission_rolls_back_when_outbox_is_saturated() {
        let store = MemoryStore::new(StoreLimits {
            max_outbox: 0,
            ..StoreLimits::default()
        });
        let ids = ids();
        let event = LifecycleEvent {
            execution_id: ids.execution.clone(),
            run_unit_id: ids.run.clone(),
            sequence: 1,
            attempt: 1,
            tick: 1,
            kind: LifecycleEventKind::Admitted,
        };
        assert_eq!(
            store.admit_execution(
                execution(&ids),
                event,
                OutboxRecord {
                    notification_id: "event-1".into(),
                    execution_id: ids.execution.clone(),
                    sequence: 1,
                    topic: "execution.lifecycle".into(),
                    payload: vec![1],
                    attempt: 0,
                    delivered: false,
                    delivered_tick: None,
                    version: 1,
                }
            ),
            Err(StoreError::CapacityExceeded)
        );
        assert_eq!(store.get_execution(&ids.execution).unwrap(), None);
        assert!(store.events(&ids.execution, 1, 8).unwrap().is_empty());
    }

    #[test]
    fn artifact_is_immutable() {
        let store = MemoryStore::new(StoreLimits::default());
        let payload = vec![1];
        let hash = Sha256::digest(&payload);
        let identity = format!("sha256:{hash:x}");
        let digest: [u8; 32] = hash.into();
        let artifact = ArtifactRef::new(identity, InvocationLimits::default()).unwrap();
        let first = ArtifactRecord {
            artifact: artifact.clone(),
            media_type: "application/test".into(),
            payload_digest: digest,
            payload,
        };
        store.put_artifact(first.clone()).unwrap();
        assert!(store.put_artifact(first).is_ok());
        let conflicting = ArtifactRecord {
            artifact,
            media_type: "application/other".into(),
            payload_digest: digest,
            payload: vec![1],
        };
        assert_eq!(store.put_artifact(conflicting), Err(StoreError::Conflict));
    }

    #[test]
    fn artifact_health_fails_at_object_and_byte_saturation() {
        fn one_byte_artifact() -> ArtifactRecord {
            let payload = vec![1];
            let digest: [u8; 32] = Sha256::digest(&payload).into();
            ArtifactRecord {
                artifact: ArtifactRef::new(
                    format!("sha256:{:x}", Sha256::digest(&payload)),
                    InvocationLimits::default(),
                )
                .unwrap(),
                media_type: "application/test".into(),
                payload_digest: digest,
                payload,
            }
        }

        let object_limited = MemoryStore::new(StoreLimits {
            max_artifacts: 1,
            ..StoreLimits::default()
        });
        assert_eq!(object_limited.health().unwrap().object_headroom(), Some(1));
        object_limited.put_artifact(one_byte_artifact()).unwrap();
        let full = object_limited.health().unwrap();
        assert_eq!(full.object_headroom(), Some(0));
        assert!(!full.ready());

        let byte_limited = MemoryStore::new(StoreLimits {
            max_artifacts: 2,
            max_total_blob_bytes: 1,
            ..StoreLimits::default()
        });
        byte_limited.put_artifact(one_byte_artifact()).unwrap();
        let full = byte_limited.health().unwrap();
        assert_eq!(full.byte_headroom(), Some(0));
        assert!(!full.ready());
    }

    #[test]
    fn mixed_provider_state_batch_is_atomic_on_success_and_conflict() {
        let store = MemoryStore::new(StoreLimits::default());
        for key in ["a", "b"] {
            store
                .put_provider_state(
                    ProviderStateRecord {
                        namespace: "dataset".into(),
                        key: key.into(),
                        version: 1,
                        payload: key.as_bytes().to_vec(),
                    },
                    None,
                )
                .unwrap();
        }
        store
            .mutate_provider_states_atomic(vec![
                ProviderStateMutation::Delete {
                    namespace: "dataset".into(),
                    key: "a".into(),
                    expected_version: 1,
                },
                ProviderStateMutation::Put(ProviderStateWrite {
                    record: ProviderStateRecord {
                        namespace: "dataset".into(),
                        key: "b".into(),
                        version: 2,
                        payload: b"B2".to_vec(),
                    },
                    expected_version: Some(1),
                }),
                ProviderStateMutation::Put(ProviderStateWrite {
                    record: ProviderStateRecord {
                        namespace: "dataset".into(),
                        key: "c".into(),
                        version: 1,
                        payload: b"C1".to_vec(),
                    },
                    expected_version: None,
                }),
            ])
            .unwrap();
        assert_eq!(store.get_provider_state("dataset", "a").unwrap(), None);
        assert_eq!(
            store
                .get_provider_state("dataset", "b")
                .unwrap()
                .unwrap()
                .payload,
            b"B2"
        );
        assert!(matches!(
            store.mutate_provider_states_atomic(vec![
                ProviderStateMutation::Put(ProviderStateWrite {
                    record: ProviderStateRecord {
                        namespace: "dataset".into(),
                        key: "b".into(),
                        version: 3,
                        payload: b"B3".to_vec(),
                    },
                    expected_version: Some(2),
                }),
                ProviderStateMutation::Delete {
                    namespace: "dataset".into(),
                    key: "missing".into(),
                    expected_version: 1,
                },
            ]),
            Err(StoreError::NotFound)
        ));
        assert_eq!(
            store
                .get_provider_state("dataset", "b")
                .unwrap()
                .unwrap()
                .version,
            2
        );
    }

    #[test]
    fn idempotency_conflict_and_unknown_outcome_are_explicit() {
        let store = MemoryStore::new(StoreLimits::default());
        let ids = ids();
        let intent = EffectRecord {
            execution_id: ids.execution.clone(),
            run_unit_id: ids.run,
            sequence: 1,
            key: ids.idem.clone(),
            digest_format: mainframe_env_store_api::EffectDigestFormat::LegacyDebug,
            request_digest: [1; 32],
            intent: mainframe_env_store_api::EffectIntentMetadata {
                owner: ids.execution.clone(),
                attempt: 1,
                capability: Some(
                    mainframe_env_execution_api::CapabilityId::new(
                        "host.state.write",
                        InvocationLimits::default(),
                    )
                    .unwrap(),
                ),
                audit_resource: None,
                audit_invocation_key: None,
                created_tick: 1,
                recovery_after_tick: 1,
                epoch: 1,
                recovery_lease: None,
            },
            state: EffectState::Intent,
            result_digest: None,
            resolved_tick: None,
        };
        store.record_intent(intent.clone()).unwrap();
        let conflict = EffectRecord {
            request_digest: [2; 32],
            ..intent.clone()
        };
        assert_eq!(store.record_intent(conflict), Err(StoreError::Conflict));
        let unknown = EffectRecord {
            state: EffectState::UnknownOutcome,
            result_digest: Some([2; 32]),
            ..intent
        };
        store.record_result(&ids.idem, unknown.clone()).unwrap();
        assert_eq!(store.effect(&ids.idem).unwrap(), Some(unknown));
        assert_eq!(store.unknown_effects(8).unwrap().len(), 1);
        let reconciled = store
            .reconcile_unknown(&ids.idem, EffectState::Completed, [9; 32])
            .unwrap();
        assert_eq!(reconciled.state, EffectState::Completed);
        assert!(store.unknown_effects(8).unwrap().is_empty());
    }

    #[test]
    fn saturation_fails_without_mutation() {
        let store = MemoryStore::new(StoreLimits {
            max_executions: 0,
            ..StoreLimits::default()
        });
        let ids = ids();
        assert_eq!(
            store.create_execution(execution(&ids)),
            Err(StoreError::CapacityExceeded)
        );
        assert_eq!(store.get_execution(&ids.execution).unwrap(), None);
    }
}

#[cfg(test)]
mod hardening_bench;
