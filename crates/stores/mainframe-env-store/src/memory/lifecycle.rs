//! Existing lifecycle/event/outbox methods with the shared native Closing gates.
use super::*;

impl ExecutionStore for MemoryStore {
    fn create_execution(&self, record: ExecutionRecord) -> Result<(), StoreError> {
        validation::new_execution(&record)?;
        Self::validate_encoded_size(encode_execution(&record)?, self.limits)?;
        let mut state = self.lock()?;
        root_terminal::guard_unenrolled(&state, &record)?;
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
        root_terminal::guard_actor(&state, id, Some(next))?;
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
        root_terminal::guard_actor(&state, &event.execution_id, None)?;
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

impl OutboxStore for MemoryStore {
    fn append_notification(&self, record: OutboxRecord) -> Result<(), StoreError> {
        let mut state = self.lock()?;
        root_terminal::guard_actor(&state, &record.execution_id, None)?;
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
        let id = state
            .outbox
            .get(notification_id)
            .ok_or(StoreError::NotFound)?
            .execution_id
            .clone();
        root_terminal::guard_outbox_delivery(&state, &id)?;
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
