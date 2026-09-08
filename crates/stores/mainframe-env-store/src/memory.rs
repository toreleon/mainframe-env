use crate::validation;
use mainframe_env_execution_api::{ArtifactRef, ExecutionId, IdempotencyKey, LifecycleEvent};
use mainframe_env_store_api::{
    ArtifactRecord, ArtifactStore, CheckpointRecord, CheckpointStore, EffectRecord,
    EffectRecoveryLease, EffectState, EventStore, ExecutionRecord, ExecutionState, ExecutionStore,
    GenerationRecord, GenerationStore, IdempotencyStore, JournalStore, OutboxRecord, OutboxStore,
    ProviderStateMutation, ProviderStateRecord, ProviderStateStore, ProviderStateWrite,
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
    pub max_outbox: usize,
    pub max_provider_state: usize,
    pub max_blob_bytes: usize,
    pub max_total_blob_bytes: usize,
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
            max_outbox: 262_144,
            max_provider_state: 262_144,
            max_blob_bytes: 64 * 1024 * 1024,
            max_total_blob_bytes: 512 * 1024 * 1024,
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
    outbox: BTreeMap<String, OutboxRecord>,
    provider_state: BTreeMap<(String, String), ProviderStateRecord>,
    blob_bytes: usize,
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

    fn put_provider_state_locked(
        state: &mut State,
        record: ProviderStateRecord,
        expected_version: Option<u64>,
        limits: StoreLimits,
    ) -> Result<(), StoreError> {
        if record.namespace.is_empty() || record.key.is_empty() || record.version == 0 {
            return Err(StoreError::IncompatibleVersion);
        }
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
        Self::reserve_blob(state, old, record.payload.len(), limits)?;
        state.provider_state.insert(key, record);
        Ok(())
    }

    fn append_event_locked(
        state: &mut State,
        event: LifecycleEvent,
        limits: StoreLimits,
    ) -> Result<(), StoreError> {
        validation::event(&event)?;
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
        Ok(())
    }

    fn append_outbox_locked(
        state: &mut State,
        record: OutboxRecord,
        limits: StoreLimits,
    ) -> Result<(), StoreError> {
        validation::new_outbox(&record)?;
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
        Ok(())
    }
}

impl ExecutionStore for MemoryStore {
    fn create_execution(&self, record: ExecutionRecord) -> Result<(), StoreError> {
        validation::new_execution(&record)?;
        let mut state = self.lock()?;
        if state.executions.contains_key(&record.execution_id) {
            return Err(StoreError::AlreadyExists);
        }
        if state.executions.len() >= self.limits.max_executions {
            return Err(StoreError::CapacityExceeded);
        }
        state.executions.insert(record.execution_id.clone(), record);
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
        record.version = record.version.checked_add(1).ok_or(StoreError::Conflict)?;
        Ok(record.clone())
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
            || work.payload.len() > self.limits.max_blob_bytes
        {
            return Err(StoreError::InvalidTransition);
        }
        let mut state = self.lock()?;
        if state.work.contains_key(&work.work_id) {
            return Err(StoreError::AlreadyExists);
        }
        if state.work.len() >= self.limits.max_work_items {
            return Err(StoreError::CapacityExceeded);
        }
        Self::reserve_blob(&mut state, 0, work.payload.len(), self.limits)?;
        state.work.insert(work.work_id.clone(), work);
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
                clear_lease(work);
            } else if work.state == WorkState::Queued && work.deadline_tick <= now_tick {
                work.state = WorkState::DeadLetter;
                clear_lease(work);
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
            return Ok(None);
        };
        work.attempt = work.attempt.checked_add(1).ok_or(StoreError::Conflict)?;
        work.lease_epoch = work
            .lease_epoch
            .checked_add(1)
            .ok_or(StoreError::Conflict)?;
        work.state = WorkState::Claimed;
        work.worker_id = Some(worker.into());
        work.lease_id = Some(format!("{worker}:{}", work.lease_epoch));
        work.lease_expiry_tick = Some(
            now_tick
                .checked_add(lease_ticks)
                .ok_or(StoreError::Conflict)?
                .min(work.deadline_tick),
        );
        work.heartbeat_tick = Some(now_tick);
        Ok(Some(work.clone()))
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
        Ok(work.clone())
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
        work.available_tick = available_tick;
        clear_lease(work);
        Ok(work.clone())
    }

    fn request_cancellation(&self, work_id: &str) -> Result<WorkRecord, StoreError> {
        let mut state = self.lock()?;
        let work = state.work.get_mut(work_id).ok_or(StoreError::NotFound)?;
        work.cancellation_requested = true;
        if work.state == WorkState::Queued {
            work.state = WorkState::Cancelled;
        }
        Ok(work.clone())
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
        clear_lease(work);
        Ok(work.clone())
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
        clear_lease(work);
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
        Ok(())
    }

    fn record_result(&self, key: &IdempotencyKey, record: EffectRecord) -> Result<(), StoreError> {
        validation::terminal(key, &record)?;
        let mut state = self.lock()?;
        let intent = state.effects.get(key).ok_or(StoreError::NotFound)?;
        validation::result(key, intent, &record)?;
        state.effects.insert(key.clone(), record);
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
        Ok(record.clone())
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
        let record = state.effects.get_mut(key).ok_or(StoreError::NotFound)?;
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
        validation::effect(record)?;
        Ok(record.clone())
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
        Ok(record.clone())
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
    ) -> Result<OutboxRecord, StoreError> {
        let mut state = self.lock()?;
        let record = state
            .outbox
            .get_mut(notification_id)
            .ok_or(StoreError::NotFound)?;
        if record.version != expected_version || record.delivered {
            return Err(StoreError::Conflict);
        }
        record.delivered = true;
        record.attempt = record.attempt.checked_add(1).ok_or(StoreError::Conflict)?;
        record.version = record.version.checked_add(1).ok_or(StoreError::Conflict)?;
        Ok(record.clone())
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
        *state = staged;
        Ok(())
    }

    fn commit_execution_step(
        &self,
        execution_id: &ExecutionId,
        expected_version: u64,
        next_state: Option<ExecutionState>,
        event: LifecycleEvent,
        effect: Option<EffectRecord>,
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
        let mut updated = current.clone();
        if let Some(next) = next_state {
            updated.state = next;
        }
        updated.version = updated.version.checked_add(1).ok_or(StoreError::Conflict)?;
        staged
            .executions
            .insert(execution_id.clone(), updated.clone());
        if let Some(effect) = effect {
            validation::effect(&effect)?;
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
        if let Some(checkpoint) = checkpoint {
            validation::checkpoint(&checkpoint)?;
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
        *state = staged;
        Ok(updated)
    }
}

impl ProviderStateStore for MemoryStore {
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
        if max == 0 || max > self.limits.max_provider_state {
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
        let bytes = current.payload.len();
        state.provider_state.remove(&map_key);
        state.blob_bytes = state.blob_bytes.saturating_sub(bytes);
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
        let old_bytes = old.payload.len();
        Self::reserve_blob(&mut state, old_bytes, record.payload.len(), self.limits)?;
        state.provider_state.remove(&old_map_key);
        state.provider_state.insert(new_map_key, record);
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
        for mutation in mutations {
            match mutation {
                ProviderStateMutation::Put(write) => Self::put_provider_state_locked(
                    &mut staged,
                    write.record,
                    write.expected_version,
                    self.limits,
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
                    Self::reserve_blob(&mut staged, old_bytes, record.payload.len(), self.limits)?;
                    staged.provider_state.remove(&old_map_key);
                    staged.provider_state.insert(new_map_key, record);
                }
            }
        }
        *state = staged;
        Ok(())
    }
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
        }
    }

    #[test]
    fn execution_transition_is_optimistic_and_monotonic() {
        let store = MemoryStore::new(StoreLimits::default());
        let ids = ids();
        store.create_execution(execution(&ids)).unwrap();
        let queued = store
            .transition_execution(&ids.execution, 1, ExecutionState::Queued)
            .unwrap();
        assert_eq!(queued.version, 2);
        assert_eq!(
            store.transition_execution(&ids.execution, 1, ExecutionState::Running),
            Err(StoreError::Conflict)
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
                created_tick: 1,
                recovery_after_tick: 1,
                epoch: 1,
                recovery_lease: None,
            },
            state: EffectState::Intent,
            result_digest: None,
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
