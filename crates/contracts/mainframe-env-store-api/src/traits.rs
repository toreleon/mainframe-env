use crate::{
    ArtifactRecord, CheckpointRecord, EffectRecord, ExecutionRecord, ExecutionState,
    GenerationRecord, OutboxRecord, ProviderStateMutation, ProviderStateRecord, ProviderStateWrite,
    SessionRecord, StoreError, WorkRecord,
};
use mainframe_env_execution_api::{
    ArtifactRef, AuditRecord, ExecutionId, IdempotencyKey, LifecycleEvent,
};

/// Mandatory persistence boundary for typed security-relevant host decisions.
pub trait AuditSink: Send + Sync {
    fn record_audit(&self, record: AuditRecord) -> Result<(), StoreError>;
    fn audit_records(
        &self,
        execution_id: &ExecutionId,
        start_effect_sequence: u64,
        max: usize,
    ) -> Result<Vec<AuditRecord>, StoreError>;
}

pub trait ExecutionStore: Send + Sync {
    fn create_execution(&self, record: ExecutionRecord) -> Result<(), StoreError>;
    fn get_execution(&self, id: &ExecutionId) -> Result<Option<ExecutionRecord>, StoreError>;
    fn transition_execution(
        &self,
        id: &ExecutionId,
        expected_version: u64,
        next: ExecutionState,
    ) -> Result<ExecutionRecord, StoreError>;
}

pub trait EventStore: Send + Sync {
    fn append_event(&self, event: LifecycleEvent) -> Result<(), StoreError>;
    fn events(
        &self,
        id: &ExecutionId,
        start_sequence: u64,
        max: usize,
    ) -> Result<Vec<LifecycleEvent>, StoreError>;
}

pub trait WorkStore: Send + Sync {
    /// Observe durable cancellation without claiming, leasing, or mutating work.
    fn get_work(&self, work_id: &str) -> Result<Option<WorkRecord>, StoreError>;
    fn enqueue(&self, work: WorkRecord) -> Result<(), StoreError>;
    /// Claim the highest-priority, oldest available work, optionally restricted
    /// to one required generation. `None` is the compatibility form for a
    /// caller that owns the entire durable work namespace.
    fn claim(
        &self,
        worker: &str,
        required_generation: Option<&str>,
        now_tick: u64,
        lease_ticks: u64,
    ) -> Result<Option<WorkRecord>, StoreError>;
    fn heartbeat(
        &self,
        work_id: &str,
        lease_id: &str,
        lease_epoch: u64,
        now_tick: u64,
        lease_ticks: u64,
    ) -> Result<WorkRecord, StoreError>;
    fn release(
        &self,
        work_id: &str,
        lease_id: &str,
        lease_epoch: u64,
        now_tick: u64,
        available_tick: u64,
    ) -> Result<WorkRecord, StoreError>;
    fn request_cancellation(&self, work_id: &str) -> Result<WorkRecord, StoreError>;
    fn dead_letter(
        &self,
        work_id: &str,
        lease_id: &str,
        lease_epoch: u64,
        now_tick: u64,
    ) -> Result<WorkRecord, StoreError>;
    fn complete(
        &self,
        work_id: &str,
        lease_id: &str,
        lease_epoch: u64,
        now_tick: u64,
    ) -> Result<(), StoreError>;
}

pub trait OutboxStore: Send + Sync {
    fn append_notification(&self, record: OutboxRecord) -> Result<(), StoreError>;
    fn pending_notifications(&self, max: usize) -> Result<Vec<OutboxRecord>, StoreError>;
    fn mark_notification_delivered(
        &self,
        notification_id: &str,
        expected_version: u64,
    ) -> Result<OutboxRecord, StoreError>;
}

pub trait JournalStore: Send + Sync {
    fn admit_execution(
        &self,
        execution: ExecutionRecord,
        event: LifecycleEvent,
        notification: OutboxRecord,
    ) -> Result<(), StoreError>;
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
    ) -> Result<ExecutionRecord, StoreError>;
}

pub trait CheckpointStore: Send + Sync {
    fn put_checkpoint(&self, record: CheckpointRecord) -> Result<(), StoreError>;
    fn get_checkpoint(&self, id: &ExecutionId) -> Result<Option<CheckpointRecord>, StoreError>;
    fn delete_checkpoint(&self, id: &ExecutionId) -> Result<(), StoreError>;
}

pub trait SessionStore: Send + Sync {
    fn put_session(
        &self,
        record: SessionRecord,
        expected_version: Option<u64>,
    ) -> Result<(), StoreError>;
    fn get_session(&self, id: &str) -> Result<Option<SessionRecord>, StoreError>;
}

pub trait ArtifactStore: Send + Sync {
    fn put_artifact(&self, record: ArtifactRecord) -> Result<(), StoreError>;
    fn get_artifact(&self, id: &ArtifactRef) -> Result<Option<ArtifactRecord>, StoreError>;
    fn delete_artifact(&self, id: &ArtifactRef) -> Result<(), StoreError>;
}

pub trait GenerationStore: Send + Sync {
    fn publish_generation(&self, record: GenerationRecord) -> Result<(), StoreError>;
    fn generation(&self, provider: &str) -> Result<Option<GenerationRecord>, StoreError>;
}

pub trait IdempotencyStore: Send + Sync {
    fn record_intent(&self, record: EffectRecord) -> Result<(), StoreError>;
    fn record_result(&self, key: &IdempotencyKey, record: EffectRecord) -> Result<(), StoreError>;
    fn effect(&self, key: &IdempotencyKey) -> Result<Option<EffectRecord>, StoreError>;
    fn unknown_effects(&self, max: usize) -> Result<Vec<EffectRecord>, StoreError>;
    /// Enumerate intents old enough for recovery whose recovery lease is absent or expired.
    fn stale_intents(
        &self,
        now_tick: u64,
        minimum_age_ticks: u64,
        max: usize,
    ) -> Result<Vec<EffectRecord>, StoreError>;
    /// Claim one stale intent under a monotonically increasing recovery epoch.
    fn claim_stale_intent(
        &self,
        key: &IdempotencyKey,
        expected_intent_epoch: u64,
        recovery_owner: &str,
        now_tick: u64,
        minimum_age_ticks: u64,
        lease_ticks: u64,
    ) -> Result<EffectRecord, StoreError>;
    /// Resolve a claimed intent without redispatching the original mutation.
    #[allow(clippy::too_many_arguments)]
    fn reconcile_stale_intent(
        &self,
        key: &IdempotencyKey,
        recovery_owner: &str,
        recovery_epoch: u64,
        now_tick: u64,
        final_state: crate::EffectState,
        format: crate::EffectDigestFormat,
        result_digest: [u8; 32],
    ) -> Result<EffectRecord, StoreError>;
    fn reconcile_unknown(
        &self,
        key: &IdempotencyKey,
        final_state: crate::EffectState,
        result_digest: [u8; 32],
    ) -> Result<EffectRecord, StoreError>;
    /// Reconcile only within the explicitly observed encoding domain.
    /// Stored identities and domains are immutable across result transitions.
    fn reconcile_unknown_versioned(
        &self,
        key: &IdempotencyKey,
        final_state: crate::EffectState,
        format: crate::EffectDigestFormat,
        result_digest: [u8; 32],
    ) -> Result<EffectRecord, StoreError> {
        let current = self.effect(key)?.ok_or(StoreError::NotFound)?;
        if current.digest_format != format {
            return Err(StoreError::Conflict);
        }
        self.reconcile_unknown(key, final_state, result_digest)
    }
}

pub trait ProviderStateStore: AuditSink + Send + Sync {
    fn get_provider_state(
        &self,
        namespace: &str,
        key: &str,
    ) -> Result<Option<ProviderStateRecord>, StoreError>;
    fn list_provider_state(
        &self,
        namespace: &str,
        max: usize,
    ) -> Result<Vec<ProviderStateRecord>, StoreError>;
    fn put_provider_state(
        &self,
        record: ProviderStateRecord,
        expected_version: Option<u64>,
    ) -> Result<(), StoreError>;
    fn delete_provider_state(
        &self,
        namespace: &str,
        key: &str,
        expected_version: u64,
    ) -> Result<(), StoreError>;
    fn move_provider_state(
        &self,
        record: ProviderStateRecord,
        old_key: &str,
        expected_version: u64,
    ) -> Result<(), StoreError>;
    fn put_provider_states_atomic(&self, writes: Vec<ProviderStateWrite>)
    -> Result<(), StoreError>;
    fn mutate_provider_states_atomic(
        &self,
        mutations: Vec<ProviderStateMutation>,
    ) -> Result<(), StoreError>;
}

pub trait PlatformStore:
    ExecutionStore
    + EventStore
    + WorkStore
    + CheckpointStore
    + SessionStore
    + ArtifactStore
    + GenerationStore
    + IdempotencyStore
    + OutboxStore
    + JournalStore
    + AuditSink
    + ProviderStateStore
    + Send
    + Sync
{
}

impl<T> PlatformStore for T where
    T: ExecutionStore
        + EventStore
        + WorkStore
        + CheckpointStore
        + SessionStore
        + ArtifactStore
        + GenerationStore
        + IdempotencyStore
        + OutboxStore
        + JournalStore
        + AuditSink
        + ProviderStateStore
        + Send
        + Sync
{
}
