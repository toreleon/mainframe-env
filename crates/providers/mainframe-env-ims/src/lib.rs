//! Bounded durable IMS HIDAM hierarchy, PCB, DLI, and checkpoint authority.

#![forbid(unsafe_code)]

pub mod database;
mod metadata;
mod retention;
mod service;
mod tm;

pub use metadata::{
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
    TmAlternatePcbDefinition, TmCall, TmConversationAction, TmDefinitionSet, TmDestination,
    TmExecutionContext, TmInputMessage, TmLimits, TmPcb, TmPcbStatus, TmTransactionDefinition,
};
