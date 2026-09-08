//! Typed host-service and CICS effects with deterministic capability selection.

#![forbid(unsafe_code)]

mod canonical;
pub use canonical::{
    AUDIT_RESOURCE_DIGEST_DOMAIN, EFFECT_CANONICAL_SCHEMA, MAX_CANONICAL_EFFECT_BYTES,
    PROVIDER_REPLAY_DIGEST_FORMAT, canonical_audit_resource_digest, canonical_db2_request_digest,
    canonical_ims_request_digest, canonical_mq_request_digest, canonical_request_digest,
    canonical_request_size, canonical_result_digest, canonical_result_size,
};

mod dataset;
mod enterprise;
mod names;
mod registry;
mod request;
mod runtime_service;
mod semantic;
mod service;
mod surface;

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
    CicsResponse, CicsUnitOfWorkOutcome, ClockRequest, DatasetAttributes, DatasetCloseControl,
    DatasetOrganization, DatasetReadControl, DatasetReadLockMode, DatasetReelUnit, DatasetRequest,
    DatasetResult, Db2HostVariable, Db2Operation, Db2Request, Db2Result, Db2Row, EffectRequest,
    EffectResult, HostLimits, HostProblem, HostRequest, HostResult, ImsOperation, ImsQualifier,
    ImsRequest, ImsResult, ImsSegment, KeyRelation, MqOperation, MqRequest, MqResult, Mutation,
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
