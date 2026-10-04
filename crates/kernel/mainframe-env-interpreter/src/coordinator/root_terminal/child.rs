//! Actual child admission under the original winning parent CALL occurrence.
use super::*;
use mainframe_env_store_api::{ProviderStateRecord, RootChildAdmission, RootProviderRowAdmission};

/// Bounded structural same-store observations from the genuine compiled CALL
/// winner. This DTO does not attest host topology/compilation or mint a lease.
/// The owning backend reads the actual current parent and retained original
/// intent again while enrolling the actual child in the same transaction.
pub struct NativeChildEnrollment {
    /// Exact same root claim retained by its genuine configured root driver.
    pub claim: RootDriverClaim,
    /// Exact original retained CALL occurrence and running parent observation.
    pub parent_occurrence: RootProviderRowAdmission,
    /// Exact actual winning CALL reservation; no fabricated EffectRecord.
    pub call: ProviderStateRecord,
    /// Exact compiled child batch catalog observation, physically rechecked.
    pub catalog: ProviderStateRecord,
}
impl NativeChildEnrollment {
    pub(in crate::coordinator) fn admit(
        &self,
        store: &Arc<dyn PlatformStore>,
        original: &Invocation,
        execution: ExecutionRecord,
        event: LifecycleEvent,
        notification: OutboxRecord,
    ) -> Result<(), StoreError> {
        self.claim.admission().validate()?;
        self.call
            .validate_write(mainframe_env_store_api::MAX_ROOT_PAYLOAD_BYTES)?;
        if original.parent_execution_id.as_ref()
            != Some(&self.parent_occurrence.execution.execution_id)
            || original.execution_id == self.parent_occurrence.execution.execution_id
            || original.run_unit_id != self.parent_occurrence.execution.run_unit_id
            || *original.principal.id() != self.parent_occurrence.execution.principal
            || original.attempt != self.parent_occurrence.execution.attempt
            || original.deadline_tick > self.claim.admission().deadline_tick
            || event.tick < self.parent_occurrence.observed_tick
        {
            return Err(StoreError::Conflict);
        }
        let mut parent_occurrence = self.parent_occurrence.clone();
        parent_occurrence.observed_tick = event.tick;
        store.admit_root_child(RootChildAdmission {
            claim: self.claim.clone(),
            parent: self.parent_occurrence.execution.execution_id.clone(),
            parent_occurrence,
            execution,
            event,
            notification,
            call: self.call.clone(),
            catalog: self.catalog.clone(),
        })
    }
}
impl ExecutionCoordinator {
    /// Genuine configured compiled child entry. Host admission and SAME TASK
    /// directory proof remain separate prerequisites. No root relabeling,
    /// implicit task-end, recovery mint or broad mixed-participant acceptance.
    pub fn execute_enrolled_child_with_control<M, F>(
        &self,
        machine: &mut M,
        original: &Invocation,
        enrollment: NativeChildEnrollment,
        observe: F,
    ) -> ExecutionOutcome
    where
        M: Machine<Effect = EffectRequest, EffectResult = EffectResult>,
        F: FnMut() -> Result<ExecutionControl, ExecutionControlError>,
    {
        if original.parent_execution_id.is_none() || self.store.is_none() || self.host.is_none() {
            return failed_outcome(problem(
                FailureCategory::UnknownOutcome,
                "native child setup refused",
            ));
        }
        self.execute_inner(
            machine,
            original,
            observe,
            false,
            None,
            None,
            Some(&enrollment),
        )
    }
}
