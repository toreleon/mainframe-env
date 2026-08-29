//! Framework-free execution identities, state-machine, outcome, and event contracts.

#![forbid(unsafe_code)]

mod context;
mod identity;
mod machine;

pub use context::{
    BoundedPayload, Cancellation, Invocation, InvocationLimits, InvocationProblem, Principal,
    ResourceLimits, ServiceClass,
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
