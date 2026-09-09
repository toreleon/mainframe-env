//! Framework-free durable-state contracts for execution and providers.
//!
//! # Transition semantics
//!
//! Store implementations apply the same typed lifecycle before performing a
//! compare-and-swap transition. Terminal states cannot be reopened:
//!
//! ```
//! use mainframe_env_store_api::ExecutionState;
//!
//! let admitted = ExecutionState::Admitted;
//! assert!(admitted.can_transition_to(ExecutionState::Queued));
//! assert!(!admitted.can_transition_to(ExecutionState::Completed));
//! assert!(ExecutionState::Completed.terminal());
//! assert!(!ExecutionState::Running.terminal());
//! ```
//!
//! # Retention semantics
//!
//! Retention policy derives inclusive age boundaries from one durable logical
//! clock observation. Callers then select one member of the closed target
//! inventory for a bounded forecast or archive-before-prune operation:
//!
//! ```
//! use mainframe_env_store_api::{RetentionPolicy, RetentionTarget};
//!
//! let policy = RetentionPolicy {
//!     lifecycle_ticks: 20,
//!     idempotency_ticks: 30,
//!     audit_ticks: 40,
//!     archive_ticks: 50,
//!     low_watermark_percent: 70,
//!     high_watermark_percent: 90,
//!     max_batch: 128,
//! };
//! let watermarks = policy.watermarks(100)?;
//!
//! assert_eq!(watermarks.lifecycle_tick, 80);
//! assert_eq!(watermarks.archive_tick, 50);
//! assert_eq!(RetentionTarget::ALL[0].as_str(), "db2-replay");
//! # Ok::<(), mainframe_env_store_api::StoreError>(())
//! ```

#![forbid(unsafe_code)]

mod model;
mod traits;

pub use model::{
    ArchivedRetentionRow, ArtifactRecord, ArtifactStoreHealth, CheckpointRecord,
    CoreRetentionDependencySnapshot, EffectDigestFormat, EffectIntentMetadata, EffectRecord,
    EffectRecoveryLease, EffectState, ExecutableArtifactMetadata, ExecutionRecord, ExecutionState,
    GenerationRecord, MAX_CORE_RETENTION_DEPENDENCIES, MAX_EFFECT_RECOVERY_OWNER_BYTES,
    MAX_PROVIDER_KEY_BYTES, MAX_PROVIDER_NAMESPACE_BYTES, MAX_PROVIDER_STATE_SCAN,
    MAX_RETENTION_BATCH, OutboxRecord, ProviderRetentionDependency,
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

/// Stable identifier for the platform durability and bounded-retention contract.
pub const STORE_CONTRACT: &str = "mainframe-env.store@2";
/// Stable identifier for versioned provider-object persistence.
pub const PROVIDER_STATE_STORE_CONTRACT: &str = "mainframe-env.provider-state-store@2";
/// Versioned bounded retention and archive-before-prune contract.
pub const RETENTION_CONTRACT: &str = "mainframe-env.retention@1";
