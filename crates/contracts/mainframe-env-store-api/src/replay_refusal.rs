//! Current observations for one coordinator-owned Completed replay refusal.
use crate::*;
use mainframe_env_execution_api::{AuditRecord, LifecycleEvent};

/// Atomic refusal settlement, never an effect rewrite or dispatch permit.
///
/// Backends compare the full original Completed occurrence and current Running
/// execution plus all dependencies inside the same physical transaction that
/// appends the actual scoped refusal audit and cursor event/outbox. No provider
/// mutation, checkpoint or terminal transition is representable here. The
/// existing conservative root@1 row/namespace overlap rule applies.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CheckedReplayRefusalStep {
    /// Exact canonical Completed original, including result digest and metadata.
    pub effect: EffectRecord,
    /// Full observed current Running execution, not a permission-bearing DTO.
    pub execution: ExecutionRecord,
    /// Actual unique current read observations; these are never written.
    pub dependencies: Vec<TerminalRowDependency>,
    /// Actual non-Success decision created by the scoped host for this attempt.
    pub audit: AuditRecord,
    /// Genuine cursor's next existing EffectResult lifecycle event.
    pub event: LifecycleEvent,
    /// Existing lifecycle notification, matching the event exactly.
    pub notification: OutboxRecord,
}

impl CheckedReplayRefusalStep {
    /// Bound borrowed structural observations before a capture clones core fields.
    /// Reserve the same ten operations as refusal settlement before the outcome
    /// is known, so an accepted capture can fit either success or refusal.
    /// No current physical equality or authority is established by this check.
    pub fn validate_observation_bounds(
        effect: &EffectRecord,
        execution: &ExecutionRecord,
        dependencies: &[TerminalRowDependency],
        max_blob: usize,
    ) -> Result<usize, StoreError> {
        let mut bytes = crate::checked_read::core_bytes(effect, execution, max_blob)?;
        crate::checked_read::dependencies(dependencies, 10, max_blob, &mut bytes)?;
        Ok(bytes)
    }
    /// Preflight before clone, codec allocation or physical payload fetch.
    /// Ten reserved operations conservatively cover execution/event/outbox/audit,
    /// their SQLite epoch-trigger updates and clock/retention accounting. This is
    /// not a larger transaction allowance: dependencies plus that reservation
    /// must fit 4096, and all captured/current/encoded bytes fit 64MiB/max_blob.
    pub fn validate_bounds(&self, max_blob: usize) -> Result<usize, StoreError> {
        let mut bytes = crate::checked_read::core_bytes(&self.effect, &self.execution, max_blob)?;
        crate::checked_read::dependencies(&self.dependencies, 10, max_blob, &mut bytes)?;
        for size in [
            self.audit.execution_id.as_str().len(),
            self.audit.run_unit_id.as_str().len(),
            self.audit.principal.as_str().len(),
            self.audit.invocation_key.as_str().len(),
            self.audit.capability.as_str().len(),
            self.event.execution_id.as_str().len(),
            self.event.run_unit_id.as_str().len(),
            self.notification.notification_id.len(),
            self.notification.execution_id.as_str().len(),
            self.notification.topic.len(),
            self.notification.payload.len(),
        ] {
            if size > max_blob {
                return Err(StoreError::PayloadTooLarge);
            }
            crate::checked_read::add(&mut bytes, size)?;
        }
        Ok(bytes)
    }
}
