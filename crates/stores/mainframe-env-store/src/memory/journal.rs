//! Undo log for the per-effect hot path (#185).
//!
//! `admit_execution`, `commit_execution_step` and
//! `mutate_provider_states_atomic` used to stage every write on a full
//! `State` clone and only write it back on success. That clone is O(store
//! size), and it runs on every nested `CALL` journal write (#55), which made
//! `CREASTMT` quadratic in the number of calls.
//!
//! Those three methods now mutate the locked `State` in place. Before each
//! mutation they call one of this module's `touch_*` methods, which records
//! the prior value (or absence) of exactly the entry about to change. On any
//! `Err`, [`Journal::rollback`] replays those entries in reverse order,
//! restoring `State` to what it held before the call. Every `touch_*` call
//! is O(1) or O(size of the one entry touched), never O(store size).
//!
//! `JournalStore for MemoryStore` (`admit_execution`, `commit_execution_step`)
//! lives here too, moved from `memory.rs` so that file stays within its
//! reviewed module-size ceiling; `mutate_provider_states_atomic` stays in
//! `memory.rs` since it is one method of that file's much larger
//! `ProviderStateStore` impl.
use super::*;

enum Entry {
    Execution(ExecutionId, Option<ExecutionRecord>),
    Effect(IdempotencyKey, Option<EffectRecord>),
    Checkpoint(ExecutionId, Option<CheckpointRecord>),
    EventsLen(ExecutionId, Option<usize>),
    Outbox(String, Option<OutboxRecord>),
    ProviderState((String, String), Option<ProviderStateRecord>),
    NewAuditKey(String),
    BlobBytes(usize),
    ProviderEpoch(u64),
    NextAuditOrdinal(u64),
    LogicalTick(u64),
}

/// Records the prior value of every `State` entry a hot-path method is
/// about to mutate, so [`Journal::rollback`] can restore exactly those
/// entries, in reverse order, on any `Err`.
#[derive(Default)]
pub(super) struct Journal {
    entries: Vec<Entry>,
}

fn set_or_remove<K: Ord, V>(map: &mut std::collections::BTreeMap<K, V>, key: K, prior: Option<V>) {
    match prior {
        Some(value) => {
            map.insert(key, value);
        }
        None => {
            map.remove(&key);
        }
    }
}

impl Journal {
    pub(super) fn touch_logical_tick(&mut self, state: &State) {
        self.entries.push(Entry::LogicalTick(state.logical_tick));
    }
    pub(super) fn touch_execution(&mut self, state: &State, id: &ExecutionId) {
        self.entries.push(Entry::Execution(
            id.clone(),
            state.executions.get(id).cloned(),
        ));
    }

    pub(super) fn touch_effect(&mut self, state: &State, key: &IdempotencyKey) {
        self.entries
            .push(Entry::Effect(key.clone(), state.effects.get(key).cloned()));
    }

    pub(super) fn touch_checkpoint(&mut self, state: &State, id: &ExecutionId) {
        self.entries.push(Entry::Checkpoint(
            id.clone(),
            state.checkpoints.get(id).cloned(),
        ));
    }

    pub(super) fn touch_events(&mut self, state: &State, id: &ExecutionId) {
        self.entries.push(Entry::EventsLen(
            id.clone(),
            state.events.get(id).map(Vec::len),
        ));
    }

    pub(super) fn touch_outbox(&mut self, state: &State, notification_id: &str) {
        self.entries.push(Entry::Outbox(
            notification_id.to_string(),
            state.outbox.get(notification_id).cloned(),
        ));
    }

    pub(super) fn touch_provider_state(&mut self, state: &State, key: &(String, String)) {
        self.entries.push(Entry::ProviderState(
            key.clone(),
            state.provider_state.get(key).cloned(),
        ));
    }

    pub(super) fn touch_blob_bytes(&mut self, state: &State) {
        self.entries.push(Entry::BlobBytes(state.blob_bytes));
    }

    pub(super) fn touch_provider_epoch(&mut self, state: &State) {
        self.entries
            .push(Entry::ProviderEpoch(state.provider_epoch));
    }

    pub(super) fn touch_next_audit_ordinal(&mut self, state: &State) {
        self.entries
            .push(Entry::NextAuditOrdinal(state.next_audit_ordinal));
    }

    /// Records a just-inserted audit key for removal on rollback. Audit
    /// keys embed a strictly increasing ordinal, so the key an
    /// `append_audit_locked` call is about to insert (if it reaches that
    /// point) can be predicted before calling it: this must be recorded
    /// before the call, not after, so a failure *inside* that call (for
    /// example the trailing epoch bump overflowing) still rolls back the
    /// audit insert it already made.
    pub(super) fn record_new_audit_key(&mut self, key: String) {
        self.entries.push(Entry::NewAuditKey(key));
    }

    /// Restores every touched entry to its pre-call value, in reverse
    /// order, so that entries touched more than once (for example the same
    /// provider-state key written twice in one batch) unwind correctly.
    pub(super) fn rollback(self, state: &mut State) {
        for entry in self.entries.into_iter().rev() {
            match entry {
                Entry::Execution(id, prior) => set_or_remove(&mut state.executions, id, prior),
                Entry::Effect(key, prior) => set_or_remove(&mut state.effects, key, prior),
                Entry::Checkpoint(id, prior) => set_or_remove(&mut state.checkpoints, id, prior),
                Entry::EventsLen(id, prior_len) => match prior_len {
                    Some(len) => {
                        if let Some(events) = state.events.get_mut(&id) {
                            events.truncate(len);
                        }
                    }
                    None => {
                        state.events.remove(&id);
                    }
                },
                Entry::Outbox(id, prior) => set_or_remove(&mut state.outbox, id, prior),
                Entry::ProviderState(key, prior) => {
                    set_or_remove(&mut state.provider_state, key, prior);
                }
                Entry::NewAuditKey(key) => {
                    state.audits.remove(&key);
                }
                Entry::BlobBytes(prior) => state.blob_bytes = prior,
                Entry::ProviderEpoch(prior) => state.provider_epoch = prior,
                Entry::NextAuditOrdinal(prior) => state.next_audit_ordinal = prior,
                Entry::LogicalTick(prior) => state.logical_tick = prior,
            }
        }
    }
}

/// Runs `body` against the locked `State`. On `Err`, rolls back every entry
/// `body` touched through its [`Journal`] before returning the error; on
/// `Ok`, `state` is left exactly as `body` mutated it and the journal is
/// simply dropped.
pub(super) fn journaled<T>(
    state: &mut State,
    body: impl FnOnce(&mut State, &mut Journal) -> Result<T, StoreError>,
) -> Result<T, StoreError> {
    let mut journal = Journal::default();
    match body(state, &mut journal) {
        Ok(value) => Ok(value),
        Err(err) => {
            journal.rollback(state);
            Err(err)
        }
    }
}

impl JournalStore for MemoryStore {
    fn mutate_root_provider_states(
        &self,
        request: mainframe_env_store_api::RootProviderPublication,
    ) -> Result<(), StoreError> {
        self.root_mutate_provider(request)
    }
    fn fence_root_driver(
        &self,
        claim: &mainframe_env_store_api::RootDriverClaim,
        execution: &ExecutionRecord,
        tick: u64,
    ) -> Result<ProviderStateRecord, StoreError> {
        self.root_fence(claim, execution, tick)
    }
    fn register_root_provider_row(
        &self,
        admission: mainframe_env_store_api::RootProviderRowAdmission,
    ) -> Result<(), StoreError> {
        self.root_register_row(admission)
    }
    fn admit_root_driver(
        &self,
        admission: mainframe_env_store_api::RootDriverAdmission,
    ) -> Result<mainframe_env_store_api::RootDriverClaim, StoreError> {
        self.root_admit(admission)
    }
    fn admit_root_child(
        &self,
        admission: mainframe_env_store_api::RootChildAdmission,
    ) -> Result<(), StoreError> {
        self.root_admit_child(admission)
    }
    fn close_root_driver(
        &self,
        claim: &mainframe_env_store_api::RootDriverClaim,
        execution: &ExecutionRecord,
        tick: u64,
    ) -> Result<mainframe_env_store_api::RootClosureSnapshot, StoreError> {
        self.root_close(claim, execution, tick)
    }
    fn commit_root_terminal_step(
        &self,
        request: mainframe_env_store_api::RootTerminalPublication,
    ) -> Result<mainframe_env_store_api::RootTerminalCommit, StoreError> {
        self.root_commit(request)
    }
    fn admit_execution(
        &self,
        execution: ExecutionRecord,
        event: LifecycleEvent,
        notification: OutboxRecord,
    ) -> Result<(), StoreError> {
        validation::admission(&execution, &event, &notification)?;
        Self::validate_encoded_size(encode_execution(&execution)?, self.limits)?;
        let mut state = self.lock()?;
        let limits = self.limits;
        journaled(&mut state, |state, journal| {
            super::root_terminal::guard_unenrolled(state, &execution)?;
            if state.executions.contains_key(&execution.execution_id) {
                return Err(StoreError::AlreadyExists);
            }
            if state.executions.len() >= limits.max_executions {
                return Err(StoreError::CapacityExceeded);
            }
            journal.touch_execution(state, &execution.execution_id);
            state
                .executions
                .insert(execution.execution_id.clone(), execution);
            journal.touch_events(state, &event.execution_id);
            journal.touch_provider_epoch(state);
            Self::append_event_locked(state, event, limits)?;
            journal.touch_outbox(state, &notification.notification_id);
            journal.touch_blob_bytes(state);
            journal.touch_provider_epoch(state);
            Self::append_outbox_locked(state, notification, limits)?;
            journal.touch_provider_epoch(state);
            Self::bump_retention_epoch(state)?;
            Ok(())
        })
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
        super::root_terminal::guard_actor(&state, execution_id, next_state)?;
        super::root_terminal::guard_event(&state, &event)?;
        if checkpoint.is_some() {
            super::root_terminal::guard_work(&state, execution_id)?;
        }
        let limits = self.limits;
        journaled(&mut state, |state, journal| {
            let current = state
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
            Self::validate_encoded_size(encode_execution(&updated)?, limits)?;
            journal.touch_execution(state, execution_id);
            state
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
                Self::validate_encoded_size(encode_effect(&effect)?, limits)?;
                validation::effect_execution(&current, &effect)?;
                match effect.state {
                    EffectState::Intent => {
                        validation::new_intent(&effect)?;
                        validation::effect_event(&event, &effect)?;
                        if state.effects.contains_key(&effect.key) {
                            return Err(StoreError::Conflict);
                        }
                        if state.effects.len() >= limits.max_effects {
                            return Err(StoreError::CapacityExceeded);
                        }
                        journal.touch_effect(state, &effect.key);
                        state.effects.insert(effect.key.clone(), effect);
                    }
                    EffectState::Completed | EffectState::Failed | EffectState::UnknownOutcome => {
                        let intent = state.effects.get(&effect.key).ok_or(StoreError::NotFound)?;
                        validation::result(&effect.key, intent, &effect)?;
                        validation::effect_event(&event, &effect)?;
                        journal.touch_effect(state, &effect.key);
                        state.effects.insert(effect.key.clone(), effect);
                    }
                }
            }
            if let Some(audit) = audit {
                let audit_execution_id = audit.execution_id.clone();
                journal.touch_next_audit_ordinal(state);
                journal.touch_provider_epoch(state);
                if let Some(ordinal) = state.next_audit_ordinal.checked_add(1) {
                    journal.record_new_audit_key(crate::durable::audit_storage_key(
                        &audit_execution_id,
                        &format!("memory:{ordinal:020}"),
                    ));
                }
                Self::append_audit_locked(state, audit, limits)?;
            }
            if let Some(checkpoint) = checkpoint {
                validation::checkpoint(&checkpoint)?;
                Self::validate_encoded_size(encode_checkpoint(&checkpoint)?, limits)?;
                validation::checkpoint_execution(&current, &checkpoint)?;
                let old = state
                    .checkpoints
                    .get(execution_id)
                    .map_or(0, |record| record.payload.len());
                if old == 0 && state.checkpoints.len() >= limits.max_checkpoints {
                    return Err(StoreError::CapacityExceeded);
                }
                journal.touch_blob_bytes(state);
                Self::reserve_blob(state, old, checkpoint.payload.len(), limits)?;
                journal.touch_checkpoint(state, execution_id);
                state.checkpoints.insert(execution_id.clone(), checkpoint);
            }
            journal.touch_events(state, &event.execution_id);
            journal.touch_provider_epoch(state);
            Self::append_event_locked(state, event, limits)?;
            journal.touch_outbox(state, &notification.notification_id);
            journal.touch_blob_bytes(state);
            journal.touch_provider_epoch(state);
            Self::append_outbox_locked(state, notification, limits)?;
            journal.touch_provider_epoch(state);
            Self::bump_retention_epoch(state)?;
            Ok(updated)
        })
    }
}
