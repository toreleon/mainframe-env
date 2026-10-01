//! Bounded durable IMS HIDAM hierarchy, PCB, DLI, and checkpoint authority.

#![forbid(unsafe_code)]

pub mod database;
mod metadata;
mod metadata_publication;
pub mod recovery;
mod retention;
mod service;
mod tm;

pub use metadata_publication::{ImsMetadataGeneration, ImsMetadataPublicationReceipt};

pub use mainframe_env_host_api::{
    IMS_METADATA_SCHEMA_V1, ImsDatabaseMetadata, ImsDatabaseOrganization, ImsDatabasePcbMetadata,
    ImsDbLevel, ImsFieldMetadata, ImsLogicalRelationshipMetadata, ImsMetadataCatalog,
    ImsMetadataIdentity, ImsMetadataLimits, ImsMetadataProblem, ImsPcbMetadata, ImsPsbMetadata,
    ImsSecondaryIndexMetadata, ImsSegmentMetadata, ImsSensitiveSegmentMetadata,
    ImsTerminalPcbMetadata, validate_ims_metadata,
};

pub use retention::{
    CICS_NESTED_EFFECT_ORIGIN_BINDING, CICS_NESTED_EFFECT_ORIGIN_SCHEMA,
    CICS_OUTER_EFFECT_ORIGIN_BINDING, CICS_OUTER_EFFECT_ORIGIN_SCHEMA, ImsReplayDependency,
    ImsReplayOwnerKind, ImsReplayRetentionDescriptor, ImsReplayRetentionError,
    ImsReplayRetentionState, describe_ims_replay_row,
};

pub use service::{
    ImsApplicationDefinition, ImsDatabaseDefinition, ImsInstallReceipt, ImsLimits, ImsLoadImage,
    ImsLoadRoot, ImsPcbDefinition, ImsPsbDefinition, ImsReplayClock, ImsSegmentDefinition,
    ImsService, ims_providers,
};

pub use tm::{
    TmAlternatePcbDefinition, TmCall, TmCallResult, TmCancelReceipt, TmConversationAction,
    TmConversationView, TmDefinitionSet, TmDestination, TmEnqueueReceipt, TmExecutionContext,
    TmInputMessage, TmInstallReceipt, TmLimits, TmMessageState, TmOutboundMessage,
    TmPackageBinding, TmPcb, TmPcbStatus, TmPcbView, TmScheduleReceipt, TmService,
    TmTransactionDefinition,
};
