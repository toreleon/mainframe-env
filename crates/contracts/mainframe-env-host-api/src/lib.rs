//! Typed host-service and CICS effects with deterministic capability selection.

#![forbid(unsafe_code)]

mod dataset;
mod names;
mod registry;
mod request;
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
pub use names::{
    DatasetName, HostNameProblem, JobName, MemberName, ProgramName, ResourceName, SessionId,
};
pub use registry::{
    CapabilityDescriptor, HostProvider, RegistryProblem, RegistryPublisher, RegistrySnapshot,
    SUBSYSTEM_HANDLER_REGISTRY_CONTRACT, SubsystemHandler, SubsystemHandlerDescriptor,
    SubsystemHandlerPublisher, SubsystemHandlerRegistry,
};
pub use request::{
    AccessIntent, AuditEvent, CicsConditionPolicy, CicsDisposition, CicsOperation, CicsRequest,
    CicsResponse, CicsUnitOfWorkOutcome, ClockRequest, DatasetAttributes, DatasetOrganization,
    DatasetRequest, DatasetResult, Db2HostVariable, Db2Operation, Db2Request, Db2Result, Db2Row,
    EffectRequest, EffectResult, HostLimits, HostProblem, HostRequest, HostResult, ImsOperation,
    ImsQualifier, ImsRequest, ImsResult, ImsSegment, MqOperation, MqRequest, MqResult, Mutation,
    ProgramRequest, RecordFormat, SecretRef, SecurityDecision, SecurityRequest, SpoolRequest,
    StateRequest, TerminalField, TerminalRequest,
};

pub const HOST_CONTRACT: &str = "mainframe-env.host@1";
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
