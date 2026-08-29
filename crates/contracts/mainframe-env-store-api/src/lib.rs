//! Framework-free durable-state contracts for execution and providers.

#![forbid(unsafe_code)]

mod model;
mod traits;

pub use model::{
    ArtifactRecord, CheckpointRecord, EffectRecord, EffectState, ExecutionRecord, ExecutionState,
    GenerationRecord, OutboxRecord, ProviderStateRecord, ProviderStateWrite, SessionRecord,
    StoreError, WorkRecord, WorkState,
};
pub use traits::{
    ArtifactStore, CheckpointStore, EventStore, ExecutionStore, GenerationStore, IdempotencyStore,
    JournalStore, OutboxStore, PlatformStore, ProviderStateStore, SessionStore, WorkStore,
};

pub const STORE_CONTRACT: &str = "mainframe-env.store@1";
