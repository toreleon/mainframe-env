//! Typed host-service and CICS effects with deterministic capability selection.

#![forbid(unsafe_code)]

mod names;
mod registry;
mod request;
mod runtime_service;
mod semantic;
mod service;

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
    DatasetLockMode, DatasetOrganization, DatasetReadControl, DatasetReelUnit, DatasetRequest,
    DatasetResult, Db2HostVariable, Db2Operation, Db2Request, Db2Result, Db2Row, EffectRequest,
    EffectResult, HostLimits, HostProblem, HostRequest, HostResult, ImsOperation, ImsQualifier,
    ImsRequest, ImsResult, ImsSegment, KeyRelation, MqOperation, MqRequest, MqResult, Mutation,
    ProgramRequest, RecordFormat, RuntimeServiceKind, RuntimeServiceSelector, SecretRef,
    SecurityDecision, SecurityRequest, SpoolRequest, StateRequest, TerminalField, TerminalRequest,
};
pub use runtime_service::{
    RUNTIME_SERVICE_REGISTRY_CONTRACT, RuntimeServiceDescriptor, RuntimeServiceRegistry,
};

pub const HOST_CONTRACT: &str = "mainframe-env.host@1";
pub const CICS_CONTRACT: &str = "mainframe-env.cics@1";

pub use semantic::{
    GENERATED_IDENTITY_CATALOG_SHA256, GENERATED_IDENTITY_CONTRACT, GENERATED_IDENTITY_SET_SHA256,
    SemanticIdentityDescriptor, SemanticIdentityProblem, SemanticNamespace, SemanticOperationId,
    official_semantic_identities, official_semantic_identity,
};
pub use service::{AuditedEffectResult, ScopedHostService};
