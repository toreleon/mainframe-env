//! Framework-free execution identities, state-machine, outcome, and event contracts.
//!
//! # Lifecycle events
//!
//! Lifecycle records carry stable execution and run-unit identities plus an
//! explicit sequence, attempt, and logical tick. Construct them only after the
//! identifiers have passed the shared bounds:
//!
//! ```
//! use mainframe_env_execution_api::{
//!     ExecutionId, InvocationLimits, LifecycleEvent, LifecycleEventKind, RunUnitId,
//! };
//!
//! let limits = InvocationLimits::default();
//! let event = LifecycleEvent {
//!     execution_id: ExecutionId::new("execution-42", limits).unwrap(),
//!     run_unit_id: RunUnitId::new("run-unit-1", limits).unwrap(),
//!     sequence: 1,
//!     attempt: 1,
//!     tick: 10,
//!     kind: LifecycleEventKind::Admitted,
//! };
//!
//! assert!(event.validate());
//! ```

#![forbid(unsafe_code)]

mod audit;
mod context;
mod identity;
mod machine;
mod participant;

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
    Abend, AbendDumpDisposition, ChildInvocation, Completion, Condition, ExecutionOutcome, Frame,
    FrameId, LifecycleEvent, LifecycleEventKind, Machine, MachineDrive, MachineResume, Quantum,
    Suspension, Transfer,
};
pub use participant::{
    ExplicitSyncpoint, ParticipantCapabilities, ParticipantContextCapability,
    ParticipantContractProblem, ParticipantDeadlineCancellation, ParticipantEffectStep,
    ParticipantFencing, ParticipantLockStep, ParticipantMode, ParticipantOutcome,
    ParticipantRejection, ParticipantSchemas, ParticipantSecurityAudit, ParticipantStatus,
    PrepareCapability, SyncpointOwner, TRANSACTION_PARTICIPANT_CONTRACT,
    TRANSACTION_PARTICIPANT_VERSION, TransactionParticipantContract,
    TransactionParticipantDescriptor, read_transaction_participant_contract,
    transaction_participant_contract_v1,
};

/// Stable identifier for this execution contract generation.
pub const EXECUTION_CONTRACT: &str = "mainframe-env.execution@1";
