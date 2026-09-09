//! Framework-free durable-state contracts for execution and providers.

#![forbid(unsafe_code)]

mod model;
mod traits;

pub use model::{
    ArchivedRetentionRow, ArtifactRecord, CheckpointRecord, CoreRetentionDependencySnapshot,
    EffectDigestFormat, EffectIntentMetadata, EffectRecord, EffectRecoveryLease, EffectState,
    ExecutionRecord, ExecutionState, GenerationRecord, MAX_CORE_RETENTION_DEPENDENCIES,
    MAX_EFFECT_RECOVERY_OWNER_BYTES, MAX_PROVIDER_KEY_BYTES, MAX_PROVIDER_NAMESPACE_BYTES,
    MAX_PROVIDER_STATE_SCAN, MAX_RETENTION_BATCH, OutboxRecord, ProviderRetentionDependency,
    ProviderRetentionObservationDeletion, ProviderRetentionObservationSource, ProviderRetentionRow,
    ProviderStateArchiveDeletion, ProviderStateArchiveReplacement, ProviderStateIdentity,
    ProviderStateMutation, ProviderStateRecord, ProviderStateWrite, RetentionAgeReconciliation,
    RetentionArchive, RetentionArchivePruneOutcome, RetentionArchivePruneReceipt,
    RetentionArchivePruneRequest, RetentionAuthorityUsage, RetentionCapacityHealth,
    RetentionForecast, RetentionLegacyRow, RetentionObservation, RetentionObservationProof,
    RetentionPolicy, RetentionReceipt, RetentionReconciliationReceipt, RetentionRequest,
    RetentionTarget, RetentionTargetCapacity, RetentionWatermarks, SaturationLevel, SessionRecord,
    StoreError, WorkRecord, WorkState,
};
pub use traits::{
    ArtifactStore, AuditSink, CheckpointStore, EventStore, ExecutionStore, GenerationStore,
    IdempotencyStore, JournalStore, OutboxStore, PlatformStore, ProviderStateStore, RetentionStore,
    SessionStore, WorkStore,
};

pub const STORE_CONTRACT: &str = "mainframe-env.store@2";
pub const PROVIDER_STATE_STORE_CONTRACT: &str = "mainframe-env.provider-state-store@2";
/// Versioned bounded retention and archive-before-prune contract.
pub const RETENTION_CONTRACT: &str = "mainframe-env.retention@1";
