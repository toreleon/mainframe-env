//! Framework-free execution identities, state-machine, outcome, and event contracts.

#![forbid(unsafe_code)]

mod audit;
mod context;
mod identity;
mod machine;

pub use audit::{
    AUDIT_RECORD_CONTRACT, AuditDecision, AuditRecord, AuditResourceDigest,
    AuditResourceDigestFormat,
};
pub use context::{
    BoundedPayload, Cancellation, CancellationProbe, Invocation, InvocationLimits,
    InvocationProblem, Principal, ResourceLimits, ServiceClass,
};
pub use identity::{
    ArtifactRef, CancellationId, CapabilityId, ExecutionId, IdempotencyKey, IdentityProblem,
    PrincipalId, RequestId, RunUnitId, Selector, TraceId,
};
pub use machine::{
    Abend, ChildInvocation, Completion, Condition, ExecutionOutcome, Frame, FrameId,
    LifecycleEvent, LifecycleEventKind, Machine, MachineDrive, MachineResume, Quantum, Suspension,
    Transfer,
};

pub const EXECUTION_CONTRACT: &str = "mainframe-env.execution@1";
