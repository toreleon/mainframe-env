use crate::{
    ArtifactRecord, CheckpointRecord, EffectRecord, ExecutionRecord, ExecutionState,
    GenerationRecord, ProviderStateRecord, SessionRecord, StoreError, WorkRecord,
};
use mainframe_env_execution_api::{ArtifactRef, ExecutionId, IdempotencyKey, LifecycleEvent};

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
    fn enqueue(&self, work: WorkRecord) -> Result<(), StoreError>;
    fn claim(
        &self,
        worker: &str,
        now_tick: u64,
        lease_ticks: u64,
    ) -> Result<Option<WorkRecord>, StoreError>;
    fn complete(&self, work_id: &str, lease_id: &str) -> Result<(), StoreError>;
}

pub trait CheckpointStore: Send + Sync {
    fn put_checkpoint(&self, record: CheckpointRecord) -> Result<(), StoreError>;
    fn get_checkpoint(&self, id: &ExecutionId) -> Result<Option<CheckpointRecord>, StoreError>;
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
}

pub trait GenerationStore: Send + Sync {
    fn publish_generation(&self, record: GenerationRecord) -> Result<(), StoreError>;
    fn generation(&self, provider: &str) -> Result<Option<GenerationRecord>, StoreError>;
}

pub trait IdempotencyStore: Send + Sync {
    fn record_intent(&self, record: EffectRecord) -> Result<(), StoreError>;
    fn record_result(&self, key: &IdempotencyKey, record: EffectRecord) -> Result<(), StoreError>;
    fn effect(&self, key: &IdempotencyKey) -> Result<Option<EffectRecord>, StoreError>;
}

pub trait ProviderStateStore: Send + Sync {
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
}
