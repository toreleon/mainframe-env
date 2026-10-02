//! Typed host-service and CICS effects with deterministic capability selection.

#![forbid(unsafe_code)]

mod canonical;
pub use canonical::{
    AUDIT_RESOURCE_DIGEST_DOMAIN, EFFECT_CANONICAL_SCHEMA, MAX_CANONICAL_EFFECT_BYTES,
    PROVIDER_REPLAY_DIGEST_FORMAT, canonical_audit_resource_digest, canonical_db2_request_digest,
    canonical_ims_request_digest, canonical_mq_request_digest, canonical_request_digest,
    canonical_request_size, canonical_result_digest, canonical_result_size,
};

mod cics_catalog;
mod clock;
mod dataset;
mod enterprise;
mod ims;
mod ims_applicability;
mod ims_metadata;
mod ims_pcb;
mod ims_status;
mod ims_system;
mod ims_tm;
mod mq_catalog;
mod mq_context;
mod mq_contract;
mod mq_handles;
mod mq_message_contract;
pub mod mq_mqi;
pub mod mq_object_route;
pub mod mq_status;
mod mq_validation;
mod names;
mod registry;
mod request;
mod runtime_service;
mod semantic;
mod service;
mod surface;

pub use cics_catalog::{
    CICS_APPLICATION_COMMAND_COUNT, CICS_APPLICATION_COMMAND_IDENTITY_SET_SHA256,
    CicsApplicationCommandIdentityDescriptor, cics_application_command_identities,
    cics_application_command_identity,
};
pub use clock::ClockRequest;
pub use dataset::{
    AllocationSpace, BufferingMode, CatalogEntryKind, CatalogKind, CatalogListEntry,
    CatalogMetadata, CatalogResolution, CompressionMode, DATASET_DEFINITION_CONTRACT,
    DATASET_PROVIDER_CAPABILITY_CONTRACT, DATASET_REQUEST_CONTRACT, DATASET_RESULT_CONTRACT,
    DATASET_STATE_SCHEMA_VERSION, DataSecurity, DatasetDefinition, DatasetDescription,
    DatasetDiagnostic, DatasetExtent, DatasetLifecycleState, DatasetLockMode, DatasetLockReceipt,
    DatasetLockTarget, DatasetMemberGenerationSnapshot, DatasetMemberSnapshot,
    DatasetProviderCapabilities, DatasetRelativeRecordSnapshot, DatasetShareOptions,
    DatasetSnapshot, DatasetVolumeDescription, DatasetVolumeExtent, DcbOptions, LifecycleMetadata,
    SmsClasses, SpaceUnit, TvsRecordOperation, TvsUnitOfWorkReceipt, TvsUnitOfWorkState,
    VolumeKind, VolumeSelection, VsamAccessMode, VsamAttributes,
};
pub use enterprise::{EnterpriseAuthorizer, EnterpriseResource, EnterpriseResourceClass};
pub use ims::{
    IMS_SSA_BOOLEAN_CONNECTORS, IMS_SSA_COMMAND_CODES, IMS_SSA_FIELD_NAME_BYTES,
    IMS_SSA_RELATIONAL_OPERATOR_BYTES, IMS_SSA_RELATIONAL_OPERATORS, IMS_SSA_RULES_SHA256,
    IMS_SSA_SEGMENT_NAME_BYTES, IMS_SSA_TOPIC_MANIFEST_SHA256, ImsSsa, ImsSsaBoolean,
    ImsSsaBooleanDescriptor, ImsSsaCommand, ImsSsaCommandCodeDescriptor, ImsSsaField,
    ImsSsaFieldResolver, ImsSsaLimits, ImsSsaPredicate, ImsSsaProblem, ImsSsaRelation,
    ImsSsaRelationDescriptor, ims_ssa_boolean, ims_ssa_command_code, ims_ssa_relation,
    parse_ims_ssa,
};
pub use ims_applicability::{
    IMS_CALL_APPLICABILITY, IMS_CALL_APPLICABILITY_RULES_SHA256, ImsApplicabilityProblem,
    ImsCallApplicabilityDescriptor, ImsCallSite, ImsCallSyntax, ImsCallVariant,
    ImsProcessingOptionClass, ImsSsaForm, validate_ims_call_site,
};
pub use ims_metadata::{
    IMS_METADATA_SCHEMA_V1, ImsDatabaseMetadata, ImsDatabaseOrganization, ImsDatabasePcbMetadata,
    ImsDbLevel, ImsFieldMetadata, ImsLogicalRelationshipMetadata, ImsMetadataCatalog,
    ImsMetadataIdentity, ImsMetadataLimits, ImsMetadataProblem, ImsPcbMetadata, ImsPsbMetadata,
    ImsSecondaryIndexMetadata, ImsSegmentMetadata, ImsSensitiveSegmentMetadata,
    ImsTerminalPcbMetadata, validate_ims_metadata,
};
pub use ims_pcb::{
    IMS_PCB_MASK_COUNT, IMS_PCB_MASKS, IMS_PCB_STATUS_RULES_SHA256,
    IMS_PCB_STATUS_TOPIC_MANIFEST_SHA256, ImsExecutionContext, ImsPcbField, ImsPcbFieldDescriptor,
    ImsPcbFieldWidth, ImsPcbKind, ImsPcbLayout, ImsPcbLimits, ImsPcbMaskDescriptor, ImsPcbProblem,
    ImsPcbSemanticValue, ims_pcb_layout, ims_pcb_mask, ims_pcb_masks,
};
pub use ims_status::{
    IMS_STATUS_CONTEXT_MEMBERSHIPS, IMS_STATUS_CONTEXTS, IMS_STATUS_DISTINCT_CODE_COUNT,
    ImsStatusCategory, ImsStatusCode, ImsStatusContext, ImsStatusContextDescriptor,
    ImsStatusDescriptor, ImsStatusProblem, ims_status, ims_status_context, ims_status_contexts,
    resolve_ims_status,
};
pub use ims_system::{
    ImsAcceptRow, ImsBufferPoolDefinition, ImsBufferPoolKind, ImsBufferStatistics,
    ImsDedbAreaDefinition, ImsPcbAvailability, ImsPositionArea, ImsPositionKeyword, ImsPositionSsa,
    ImsQClass, ImsStatisticsFamily, ImsStatisticsFormat, ImsStatisticsFunction, ImsStatusGroup,
    ImsSystemCall, ImsSystemDirectory, ImsSystemRequest, ImsSystemResult,
    ImsSystemRuntimeDefinition,
};
pub use ims_tm::{
    TmAlternatePcbDefinition, TmDefinitionSet, TmDestination, TmExecutionContext, TmLimits,
    TmTransactionDefinition,
};
pub use mq_catalog::{
    MQ_MQI_CALL_COUNT, MQ_MQI_CALL_IDENTITY_SET_SHA256, MQ_MQI_SOURCE_ROW_COUNT,
    MqMqiCallIdentityDescriptor, mq_mqi_call_identities, mq_mqi_call_identity,
    mq_mqi_call_identity_by_label,
};
pub use mq_context::{
    MQCC_FAILED, MQRC_ENVIRONMENT_ERROR, MqContextDisposition, MqHostEnvironment, MqSyncpointCall,
    MqSyncpointOwner, mq_syncpoint_context_disposition,
};
pub use mq_contract::{
    MQ_MQI_CONTRACT_CATALOG_SHA256, MQ_MQI_CONTRACT_COUNT, MQ_MQI_CONTRACT_SET_SHA256,
    MQ_MQI_PARAMETER_COUNT, MQ_MQI_PENDING_SIGNATURE_COUNT, MQ_MQI_VERIFIED_SIGNATURE_COUNT,
    MqMqiContractDescriptor, MqMqiHandleAction, MqMqiHandleRole, MqMqiParameterDescriptor,
    MqMqiParameterDirection, MqMqiParameterRole, MqMqiSignatureStatus, MqMqiSourceSpellingAnomaly,
    MqMqiSourceStatus, mq_mqi_contract, mq_mqi_contract_by_label, mq_mqi_contracts,
};
pub use mq_handles::{
    MQ_MAX_HANDLE_SLOTS, MQHC_DEF_HCONN, MQHC_UNASSOCIATED_HCONN, MqConnectionId, MqHandle,
    MqHandleKind, MqHandleObservation, MqHandleOwner, MqHandleProblem, MqHandleRegistry,
    MqHandleSharing, MqHconn, MqHmsg, MqHobj, MqHsub,
};
pub use mq_message_contract::{
    MQ_MESSAGE_CONTRACT, MQ_MESSAGE_PENDING, MQ_MESSAGE_SOURCES, MqDeliveryOutcome,
    MqDistributionItemResult, MqDistributionResult, MqExpiry, MqGetContract, MqGetDisposition,
    MqGetMode, MqMessage, MqMessageDescriptor, MqMessageIdentifiers, MqMessageLimits,
    MqMessageMatch, MqMessageOrdering, MqMessagePending, MqMessageProblem, MqMessageProperty,
    MqMessageSource, MqPersistence, MqPriority, MqPropertyQuery, MqPropertyType, MqTruncation,
    MqTruncationDisposition, MqWait,
};
pub use mq_validation::{
    MqArgument, MqArgumentValue, MqExecutionDisposition, MqPendingDisposition, MqStructureVersion,
    MqValidationError, MqValidationProblem, MqValidationReport, MqValidationSource,
    validate_mqi_call,
};
pub use names::{
    ClassName, DatasetName, HostNameProblem, JobName, MemberName, MethodName, ProgramName,
    ResourceName, RuntimeServiceName, SessionId,
};
pub use registry::{
    CapabilityDescriptor, HostProvider, RegistryProblem, RegistryPublisher, RegistrySnapshot,
    SUBSYSTEM_HANDLER_REGISTRY_CONTRACT, SubsystemHandler, SubsystemHandlerDescriptor,
    SubsystemHandlerPublisher, SubsystemHandlerRegistry,
};
pub use request::{
    AccessIntent, AuditEvent, CicsConditionPolicy, CicsDisposition, CicsOperation, CicsRequest,
    CicsResponse, CicsUnitOfWorkOutcome, DatasetAttributes, DatasetCloseControl,
    DatasetOrganization, DatasetReadControl, DatasetReadLockMode, DatasetReelUnit, DatasetRequest,
    DatasetResult, Db2HostVariable, Db2Operation, Db2Request, Db2Result, Db2Row, EffectRequest,
    EffectResult, HostLimits, HostProblem, HostRequest, HostResult, ImsOperation, ImsQualifier,
    ImsRequest, ImsResult, ImsSegment, KeyRelation, MqMqiEffectOccurrence, MqMqiHostRequest,
    MqMqiHostResult, MqOperation, MqRequest, MqResult, Mutation, ProgramLinkSelection,
    ProgramRequest, RecordFormat, RuntimeServiceKind, RuntimeServiceSelector,
    SPOOL_REQUEST_CONTRACT, SPOOL_RESULT_CONTRACT, SecretRef, SecurityDecision, SecurityRequest,
    SpoolFileSummary, SpoolRequest, SpoolResult, StateRequest, TerminalField, TerminalRequest,
};
pub use runtime_service::{
    RUNTIME_SERVICE_REGISTRY_CONTRACT, RuntimeServiceDescriptor, RuntimeServiceRegistry,
};

/// Stable identifier for the typed host-effect contract.
pub const HOST_CONTRACT: &str = "mainframe-env.host@1";
/// Stable identifier for the CICS request and response contract.
pub const CICS_CONTRACT: &str = "mainframe-env.cics@1";

pub use semantic::{
    GENERATED_IDENTITY_CATALOG_SHA256, GENERATED_IDENTITY_CONTRACT, GENERATED_IDENTITY_SET_SHA256,
    SemanticIdentityDescriptor, SemanticIdentityProblem, SemanticNamespace, SemanticOperationId,
    official_semantic_identities, official_semantic_identity,
};
pub use service::{AuditedEffectResult, ScopedHostService};
pub use surface::{
    DATASET_SURFACE_INVENTORY_SHA256, DatasetSurfaceDescriptor, dataset_surface_descriptor,
    dataset_surface_descriptors,
};
