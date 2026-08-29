use mainframe_env_execution_api::{ArtifactRef, ExecutionId, IdempotencyKey, LifecycleEvent};
use mainframe_env_store_api::{
    ArtifactRecord, ArtifactStore, CheckpointRecord, CheckpointStore, EffectRecord, EffectState,
    EventStore, ExecutionRecord, ExecutionState, ExecutionStore, GenerationRecord, GenerationStore,
    IdempotencyStore, ProviderStateRecord, ProviderStateStore, SessionRecord, SessionStore,
    StoreError, WorkRecord, WorkState, WorkStore,
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
            max_provider_state: 262_144,
            max_blob_bytes: 64 * 1024 * 1024,
            max_total_blob_bytes: 512 * 1024 * 1024,
        }
    }
}

#[derive(Default)]
struct State {
    executions: BTreeMap<ExecutionId, ExecutionRecord>,
    events: BTreeMap<ExecutionId, Vec<LifecycleEvent>>,
    work: BTreeMap<String, WorkRecord>,
    checkpoints: BTreeMap<ExecutionId, CheckpointRecord>,
    sessions: BTreeMap<String, SessionRecord>,
    artifacts: BTreeMap<ArtifactRef, ArtifactRecord>,
    generations: BTreeMap<String, GenerationRecord>,
    effects: BTreeMap<IdempotencyKey, EffectRecord>,
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
}

impl ExecutionStore for MemoryStore {
    fn create_execution(&self, record: ExecutionRecord) -> Result<(), StoreError> {
        if record.attempt == 0 || record.version == 0 || record.state != ExecutionState::Admitted {
            return Err(StoreError::InvalidTransition);
        }
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
        if !event.validate() {
            return Err(StoreError::InvalidSequence);
        }
        let mut state = self.lock()?;
        let total: usize = state.events.values().map(Vec::len).sum();
        if total >= self.limits.max_events {
            return Err(StoreError::CapacityExceeded);
        }
        let events = state.events.entry(event.execution_id.clone()).or_default();
        if events.len() >= self.limits.max_events_per_execution {
            return Err(StoreError::CapacityExceeded);
        }
        let expected = events.last().map_or(1, |last| last.sequence + 1);
        if event.sequence != expected {
            return Err(StoreError::InvalidSequence);
        }
        events.push(event);
        Ok(())
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
    fn enqueue(&self, work: WorkRecord) -> Result<(), StoreError> {
        if work.work_id.is_empty()
            || work.attempt != 0
            || work.state != WorkState::Queued
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
        now_tick: u64,
        lease_ticks: u64,
    ) -> Result<Option<WorkRecord>, StoreError> {
        if worker.is_empty() || lease_ticks == 0 {
            return Err(StoreError::LeaseConflict);
        }
        let mut state = self.lock()?;
        for work in state.work.values_mut() {
            if work.state == WorkState::Claimed
                && work
                    .lease_expiry_tick
                    .is_some_and(|expiry| expiry <= now_tick)
            {
                work.state = WorkState::Queued;
                work.lease_id = None;
                work.lease_expiry_tick = None;
            }
        }
        let Some(work) = state
            .work
            .values_mut()
            .find(|work| work.state == WorkState::Queued && work.available_tick <= now_tick)
        else {
            return Ok(None);
        };
        work.attempt = work.attempt.checked_add(1).ok_or(StoreError::Conflict)?;
        work.state = WorkState::Claimed;
        work.lease_id = Some(format!("{worker}:{}", work.attempt));
        work.lease_expiry_tick = Some(
            now_tick
                .checked_add(lease_ticks)
                .ok_or(StoreError::Conflict)?,
        );
        Ok(Some(work.clone()))
    }

    fn complete(&self, work_id: &str, lease_id: &str) -> Result<(), StoreError> {
        let mut state = self.lock()?;
        let work = state.work.get_mut(work_id).ok_or(StoreError::NotFound)?;
        if work.state != WorkState::Claimed || work.lease_id.as_deref() != Some(lease_id) {
            return Err(StoreError::LeaseConflict);
        }
        work.state = WorkState::Completed;
        work.lease_expiry_tick = None;
        Ok(())
    }
}

impl CheckpointStore for MemoryStore {
    fn put_checkpoint(&self, record: CheckpointRecord) -> Result<(), StoreError> {
        if record.schema_version == 0 || record.payload.is_empty() {
            return Err(StoreError::IncompatibleVersion);
        }
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
        if record.sequence == 0
            || record.state != EffectState::Intent
            || record.result_digest.is_some()
        {
            return Err(StoreError::InvalidTransition);
        }
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
        if &record.key != key
            || !matches!(
                record.state,
                EffectState::Completed | EffectState::Failed | EffectState::UnknownOutcome
            )
        {
            return Err(StoreError::InvalidTransition);
        }
        let mut state = self.lock()?;
        let intent = state.effects.get(key).ok_or(StoreError::NotFound)?;
        if intent.execution_id != record.execution_id
            || intent.run_unit_id != record.run_unit_id
            || intent.sequence != record.sequence
            || intent.request_digest != record.request_digest
            || intent.state != EffectState::Intent
        {
            return Err(StoreError::Conflict);
        }
        state.effects.insert(key.clone(), record);
        Ok(())
    }

    fn effect(&self, key: &IdempotencyKey) -> Result<Option<EffectRecord>, StoreError> {
        Ok(self.lock()?.effects.get(key).cloned())
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
        if record.namespace.is_empty() || record.key.is_empty() || record.version == 0 {
            return Err(StoreError::IncompatibleVersion);
        }
        let mut state = self.lock()?;
        let key = (record.namespace.clone(), record.key.clone());
        let current = state.provider_state.get(&key);
        match (current, expected_version) {
            (None, None) if record.version == 1 => {}
            (Some(current), Some(expected))
                if current.version == expected && record.version == expected + 1 => {}
            _ => return Err(StoreError::Conflict),
        }
        let old = current.map_or(0, |item| item.payload.len());
        if old == 0 && state.provider_state.len() >= self.limits.max_provider_state {
            return Err(StoreError::CapacityExceeded);
        }
        Self::reserve_blob(&mut state, old, record.payload.len(), self.limits)?;
        state.provider_state.insert(key, record);
        Ok(())
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
        if record.namespace.is_empty()
            || record.key.is_empty()
            || record.key == old_key
            || record.version
                != expected_version
                    .checked_add(1)
                    .ok_or(StoreError::Conflict)?
        {
            return Err(StoreError::Conflict);
        }
        let mut state = self.lock()?;
        let old_map_key = (record.namespace.clone(), old_key.to_string());
        let new_map_key = (record.namespace.clone(), record.key.clone());
        let old = state
            .provider_state
            .get(&old_map_key)
            .ok_or(StoreError::NotFound)?;
        if old.version != expected_version || state.provider_state.contains_key(&new_map_key) {
            return Err(StoreError::Conflict);
        }
        let old_bytes = old.payload.len();
        Self::reserve_blob(&mut state, old_bytes, record.payload.len(), self.limits)?;
        state.provider_state.remove(&old_map_key);
        state.provider_state.insert(new_map_key, record);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mainframe_env_execution_api::{
        InvocationLimits, LifecycleEventKind, PrincipalId, RunUnitId, Selector,
    };

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
                state: WorkState::Queued,
                attempt: 0,
                available_tick: 1,
                lease_id: None,
                lease_expiry_tick: None,
                payload: vec![1],
            })
            .unwrap();
        let first = store.claim("worker", 1, 1).unwrap().unwrap();
        let second = store.claim("worker", 2, 1).unwrap().unwrap();
        assert_eq!((first.attempt, second.attempt), (1, 2));
    }

    #[test]
    fn artifact_is_immutable() {
        let store = MemoryStore::new(StoreLimits::default());
        let ids = ids();
        let first = ArtifactRecord {
            artifact: ids.artifact.clone(),
            media_type: "application/test".into(),
            payload_digest: [1; 32],
            payload: vec![1],
        };
        store.put_artifact(first.clone()).unwrap();
        assert!(store.put_artifact(first).is_ok());
        let conflicting = ArtifactRecord {
            artifact: ids.artifact,
            media_type: "application/test".into(),
            payload_digest: [2; 32],
            payload: vec![2],
        };
        assert_eq!(store.put_artifact(conflicting), Err(StoreError::Conflict));
    }

    #[test]
    fn idempotency_conflict_and_unknown_outcome_are_explicit() {
        let store = MemoryStore::new(StoreLimits::default());
        let ids = ids();
        let intent = EffectRecord {
            execution_id: ids.execution,
            run_unit_id: ids.run,
            sequence: 1,
            key: ids.idem.clone(),
            request_digest: [1; 32],
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
            ..intent
        };
        store.record_result(&ids.idem, unknown.clone()).unwrap();
        assert_eq!(store.effect(&ids.idem).unwrap(), Some(unknown));
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
