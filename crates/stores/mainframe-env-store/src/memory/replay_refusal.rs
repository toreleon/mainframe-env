//! No physical gap between checked observations and the existing journal kernel.
use super::*;
use mainframe_env_store_api::CheckedReplayRefusalStep;
#[cfg(test)]
#[path = "replay_refusal/tests.rs"]
mod tests;
impl MemoryStore {
    pub(in crate::memory) fn settle_checked_replay_refusal(
        &self,
        r: CheckedReplayRefusalStep,
    ) -> Result<ExecutionRecord, StoreError> {
        let mut budget = crate::replay_refusal::validate(&r, self.limits.max_blob_bytes)?;
        let mut state = self.lock()?;
        super::check(
            &state,
            &r.effect,
            &r.execution,
            r.event.tick,
            &r.dependencies,
            self.limits.max_blob_bytes,
            &mut budget,
        )?;
        // Charge/validate encoded current outputs before the touched-entry kernel.
        let _ = crate::replay_refusal::plan(&r, self.limits.max_blob_bytes, &mut budget)?;
        crate::memory::journal::commit_step_locked(
            &mut state,
            self.limits,
            &r.execution.execution_id,
            r.execution.version,
            None,
            r.event,
            None,
            Some(r.audit),
            None,
            r.notification,
        )
    }
}
