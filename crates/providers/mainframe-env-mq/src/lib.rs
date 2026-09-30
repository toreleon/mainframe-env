//! Bounded durable MQ request/reply, correlation, trigger, and syncpoint authority.

#![forbid(unsafe_code)]

mod abi;
mod host_context;
mod message;
mod object;
mod retention;
mod service;

pub use abi::mq_abi_library;

pub use object::{
    MQ_OBJECT_CATALOG_SCHEMA, MQ_OBJECT_NAME_BYTES, MqAliasTarget, MqChannelRoute, MqCloseMode,
    MqCloseOutcome, MqDynamicQueueKind, MqDynamicQueuePattern, MqDynamicQueueState,
    MqLifecycleOwner, MqLocalQueueUsage, MqModelInstance, MqObjectCapability, MqObjectCatalog,
    MqObjectDefinition, MqObjectError, MqObjectIdentity, MqObjectKind, MqObjectLimits,
    MqObjectLookup, MqObjectName, MqQueueManagerDefinition, MqResolution, MqResolvedTarget,
    MqSubscriptionDestination,
};

pub use retention::{
    CICS_NESTED_EFFECT_ORIGIN_BINDING, CICS_NESTED_EFFECT_ORIGIN_SCHEMA,
    CICS_OUTER_EFFECT_ORIGIN_BINDING, CICS_OUTER_EFFECT_ORIGIN_SCHEMA, MqReplayDependency,
    MqReplayOwnerKind, MqReplayRetentionDescriptor, MqReplayRetentionError, MqReplayRetentionState,
    describe_mq_replay_row,
};

pub use service::{
    MqInstallReceipt, MqLimits, MqQueueDefinition, MqReplayClock, MqService, mq_providers,
};
