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
mod step;
pub(super) use step::commit_step_locked;

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
    fn commit_checked_replay_refusal(
        &self,
        request: mainframe_env_store_api::CheckedReplayRefusalStep,
    ) -> Result<ExecutionRecord, StoreError> {
        self.settle_checked_replay_refusal(request)
    }
    fn mutate_root_preparation_states(
        &self,
        request: mainframe_env_store_api::RootPreparationPublication,
    ) -> Result<(), StoreError> {
        self.root_mutate_preparation(request)
    }
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
        commit_step_locked(
            &mut state,
            self.limits,
            execution_id,
            expected_version,
            next_state,
            event,
            effect,
            audit,
            checkpoint,
            notification,
        )
    }
}
