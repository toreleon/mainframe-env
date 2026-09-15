//! Bounded dataset/catalog authority and host-provider adapters.

#![forbid(unsafe_code)]

mod browse;
mod codec;
mod dataset_locks;
mod dependency;
mod retention;
mod service;

pub use retention::{
    CICS_NESTED_EFFECT_ORIGIN_BINDING, CICS_NESTED_EFFECT_ORIGIN_SCHEMA,
    CICS_OUTER_EFFECT_ORIGIN_BINDING, CICS_OUTER_EFFECT_ORIGIN_SCHEMA, DATASET_REPLAY_NAMESPACE,
    DatasetReplayCodecVersion, DatasetReplayDependencyState, DatasetReplayOwnerKind,
    DatasetReplayResultState, DatasetReplayRetentionState, DatasetReplayRowDescriptor,
    DatasetReplayValidationError,
};
pub use service::{
    DatasetLimits, DatasetReplayClock, DatasetSeedObject, DatasetService, SeedInstallReceipt,
    dataset_providers, describe_dataset_replay_row, describe_dataset_replay_row_with_limits,
    reconcile_dataset_replay_row, validate_dataset_replay_effect,
};
