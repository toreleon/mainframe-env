//! Exact-source terminalization through the existing journal and core CAS.
use super::*;

impl ExecutionCoordinator {
    /// Close exactly the already-attested suspended source of a durable handoff.
    ///
    /// Requires an existing source with the pinned version, selector, artifact,
    /// run, principal identity and attempt, and a matching final Suspended event.
    /// Unlike generic resumable admission, this never creates a missing execution
    /// or relaxes the suspended source's executable identity. Concurrent advances
    /// are rejected by the existing journal CAS. The caller must separately prove
    /// checkpoint transfer and provider ownership; this grants no target admission.
    ///
    /// Checkpoint deletion follows the atomic Completed/HandoffCompleted commit.
    /// An error during deletion can therefore follow durable terminalization.
    /// Callers must observe the exact core/event proof before deciding disposition;
    /// an error does not authorize redispatch or a repeated terminal transition.
    pub fn complete_suspended_handoff_at_version(
        &self,
        source: &Invocation,
        exact_suspended_version: u64,
        tick: u64,
    ) -> Result<(), StoreError> {
        let store = self.store.as_ref().ok_or_else(|| {
            StoreError::Infrastructure("handoff completion requires an execution store".into())
        })?;
        let execution = store
            .get_execution(&source.execution_id)?
            .ok_or(StoreError::NotFound)?;
        if exact_suspended_version == 0
            || exact_suspended_version >= i64::MAX as u64
            || execution.version != exact_suspended_version
            || execution.execution_id != source.execution_id
            || execution.run_unit_id != source.run_unit_id
            || execution.principal != *source.principal.id()
            || execution.selector != source.selector
            || execution.artifact != source.artifact
            || execution.attempt != source.attempt
        {
            return Err(StoreError::Conflict);
        }
        if execution.state != ExecutionState::Suspended || execution.terminal_tick.is_some() {
            return Err(StoreError::InvalidTransition);
        }
        // Read only the pinned suspension event. A configured small read quota
        // must not prevent terminalization, and no earlier history is needed to
        // manufacture admission. The core CAS still rejects concurrent advances.
        let events = store.events(&source.execution_id, exact_suspended_version, 1)?;
        let last = events.last().ok_or(StoreError::IncompatibleVersion)?;
        if last.execution_id != source.execution_id
            || last.run_unit_id != source.run_unit_id
            || last.attempt != source.attempt
            || last.sequence != exact_suspended_version
            || last.kind != LifecycleEventKind::Suspended
        {
            return Err(StoreError::IncompatibleVersion);
        }
        if tick == 0 || tick < last.tick || tick > i64::MAX as u64 {
            return Err(StoreError::InvalidSequence);
        }
        // Do not reopen a generic cursor: disappearance between a preflight read
        // and open would otherwise admit a new execution. Only the existing CAS
        // may mutate the source observed above.
        let mut journal = JournalCursor {
            store: Arc::clone(store),
            execution_id: source.execution_id.clone(),
            run_unit_id: source.run_unit_id.clone(),
            attempt: source.attempt,
            tick,
            version: exact_suspended_version,
            sequence: last.sequence,
        };
        journal.record(
            Some(ExecutionState::Completed),
            LifecycleEventKind::HandoffCompleted,
            None,
            None,
            None,
        )?;
        match store.delete_checkpoint(&source.execution_id) {
            Ok(()) | Err(StoreError::NotFound) => Ok(()),
            Err(problem) => Err(problem),
        }
    }
}
