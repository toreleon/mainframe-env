//! Bounded durable MQ request/reply, correlation, trigger, and syncpoint authority.

#![forbid(unsafe_code)]

mod abi;
mod delivery;
mod host_context;
mod message;
mod message_handle;
mod mqi_admission;
mod mqi_lifecycle;
mod mqi_replay;
mod object;
pub mod object_inquiry;
mod object_service;
mod pubsub;
mod retention;
mod service;
mod service_mqi_intent;
mod trusted_batch_embedding;
pub use service::{
    ProducerBatchContext as MqTrustedBatchContextObservation,
    ProducerGmt as MqTrustedBatchGmtObservation, ProducerSource as MqTrustedBatchProducerSource,
};

pub use delivery::{
    MQ_DELIVERY_SCHEMA, MqDeliveryError, MqDeliveryGet, MqDeliveryKernel, MqDeliveryLimits,
};

pub use abi::mq_abi_library;
pub use message_handle::{
    MqBufferCodec, MqHandleKernel, MqHandleKernelOption, MqHandleKernelProblem,
};

pub use object::{
    MQ_OBJECT_CATALOG_SCHEMA, MQ_OBJECT_NAME_BYTES, MQ_OBJECT_NATIVE_CATALOG_SCHEMA, MqAliasTarget,
    MqChannelRoute, MqCloseMode, MqCloseOutcome, MqDynamicQueueKind, MqDynamicQueuePattern,
    MqDynamicQueueState, MqLifecycleOwner, MqLocalQueueUsage, MqModelInstance, MqNativeAttributes,
    MqNativeCharacters, MqNativeDeliverySequence, MqNativeQueueAttributes, MqObjectCapability,
    MqObjectCatalog, MqObjectDefinition, MqObjectError, MqObjectIdentity, MqObjectKind,
    MqObjectLimits, MqObjectLookup, MqObjectName, MqQueueManagerDefinition, MqResolution,
    MqResolvedTarget, MqSubscriptionDestination,
};

pub use pubsub::{
    MQ_PUBSUB_SNAPSHOT_SCHEMA, MqCallbackControl, MqCallbackState, MqMessageHandleAccess,
    MqPubsubAuthorization, MqPubsubError, MqPubsubEvent, MqPubsubKernel, MqPubsubLimits,
    MqPubsubResource, MqRegistryAccess, MqSubscriptionHandles, MqSubscriptionMode,
};

pub use retention::{
    CICS_NESTED_EFFECT_ORIGIN_BINDING, CICS_NESTED_EFFECT_ORIGIN_SCHEMA,
    CICS_OUTER_EFFECT_ORIGIN_BINDING, CICS_OUTER_EFFECT_ORIGIN_SCHEMA, MqReplayDependency,
    MqReplayOwnerKind, MqReplayRetentionDescriptor, MqReplayRetentionError, MqReplayRetentionState,
    MqSelectedRetentionDependencies, describe_mq_replay_row,
    describe_mq_selected_retention_dependencies,
};

pub use service::{
    MqInstallReceipt, MqLimits, MqQueueDefinition, MqReplayClock, MqService, mq_providers,
};
pub use trusted_batch_embedding::{
    MqBatchLeDllCodesetSource, MqTrustedBatchFrame, MqTrustedBatchPointProfile,
    MqTrustedBatchPointTarget, MqTrustedBatchRelationship, MqTrustedBatchRoot,
    MqTrustedBatchRuntime, MqTrustedBatchStructureProfile,
};
