//! Typed host-service and CICS effects with deterministic capability selection.

#![forbid(unsafe_code)]

mod names;
mod registry;
mod request;
mod service;

pub use names::{
    DatasetName, HostNameProblem, JobName, MemberName, ProgramName, ResourceName, SessionId,
};
pub use registry::{
    CapabilityDescriptor, HostProvider, RegistryProblem, RegistryPublisher, RegistrySnapshot,
};
pub use request::{
    AccessIntent, AuditEvent, CicsConditionPolicy, CicsDisposition, CicsOperation, CicsRequest,
    CicsResponse, CicsUnitOfWorkOutcome, ClockRequest, DatasetAttributes, DatasetOrganization,
    DatasetRequest, DatasetResult, Db2HostVariable, Db2Operation, Db2Request, Db2Result, Db2Row,
    EffectRequest, EffectResult, HostLimits, HostProblem, HostRequest, HostResult, Mutation,
    ProgramRequest, RecordFormat, SecretRef, SecurityDecision, SecurityRequest, SpoolRequest,
    StateRequest, TerminalField, TerminalRequest,
};

pub const HOST_CONTRACT: &str = "mainframe-env.host@1";
pub const CICS_CONTRACT: &str = "mainframe-env.cics@1";

pub use service::{AuditedEffectResult, ScopedHostService};
