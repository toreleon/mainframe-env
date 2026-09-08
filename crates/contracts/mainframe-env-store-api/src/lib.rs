//! Framework-free durable-state contracts for execution and providers.

#![forbid(unsafe_code)]

mod model;
mod traits;

pub use model::{
    ArtifactRecord, CheckpointRecord, EffectDigestFormat, EffectIntentMetadata, EffectRecord,
    EffectRecoveryLease, EffectState, ExecutionRecord, ExecutionState, GenerationRecord,
    MAX_EFFECT_RECOVERY_OWNER_BYTES, OutboxRecord, ProviderStateMutation, ProviderStateRecord,
    ProviderStateWrite, SessionRecord, StoreError, WorkRecord, WorkState,
};
pub use traits::{
    ArtifactStore, AuditSink, CheckpointStore, EventStore, ExecutionStore, GenerationStore,
    IdempotencyStore, JournalStore, OutboxStore, PlatformStore, ProviderStateStore, SessionStore,
    WorkStore,
};

pub const STORE_CONTRACT: &str = "mainframe-env.store@1";
pub const PROVIDER_STATE_STORE_CONTRACT: &str = "mainframe-env.provider-state-store@2";
