use crate::{ArtifactRef, BoundedPayload, ExecutionId, RunUnitId, Selector};
use mainframe_env_diagnostics::ExecutionProblem;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
/// Nonzero machine frame identity constructed from a `u32`.
pub struct FrameId(u32);

impl FrameId {
    /// Return a frame identity for a positive value, or `None` for zero.
    pub fn new(value: u32) -> Option<Self> {
        (value > 0).then_some(Self(value))
    }
    /// Return the nonzero numeric identity.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Frame identity, depth, and return metadata carried by machine execution.
///
/// Fields are public; depth is checked only when `within_limit` is called.
pub struct Frame {
    /// Nonzero identity of the frame.
    pub id: FrameId,
    /// One-based depth tested against the frame budget.
    pub depth: u32,
    /// Entry selector associated with this frame.
    pub selector: Selector,
    /// Optional opaque destination for returning control.
    pub return_target: Option<String>,
}

impl Frame {
    /// Check that the depth is positive and does not exceed `max_frames`.
    ///
    /// No frame-stack membership or selector validity is checked.
    #[must_use]
    pub fn within_limit(&self, max_frames: u32) -> bool {
        self.depth > 0 && self.depth <= max_frames
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Step and allocation budgets for one call to [`Machine::drive`].
///
/// The constructor rejects zero budgets. Public fields permit direct construction
/// or mutation; enforcement belongs to the machine implementation.
pub struct Quantum {
    /// Maximum steps requested for this drive call.
    pub max_steps: u32,
    /// Maximum allocation bytes requested for this drive call.
    pub max_allocated_bytes: u64,
}

impl Quantum {
    /// Return a quantum only when both step and allocation budgets are positive.
    pub fn new(max_steps: u32, max_allocated_bytes: u64) -> Option<Self> {
        (max_steps > 0 && max_allocated_bytes > 0).then_some(Self {
            max_steps,
            max_allocated_bytes,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Application return code and bounded output produced by a completed machine.
pub struct Completion {
    /// Application return code, without a success interpretation imposed here.
    pub return_code: i32,
    /// Owned output with its schema and construction-time byte bound.
    pub output: BoundedPayload,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Modeled application condition with response values and handling status.
///
/// These fields are observations; this type performs no condition dispatch.
pub struct Condition {
    /// Condition name reported by the machine.
    pub name: String,
    /// Primary response value accompanying the condition.
    pub response: i32,
    /// Secondary response value accompanying the condition.
    pub response2: i32,
    /// Whether the condition is reported as handled.
    pub handled: bool,
}

/// Whether a terminal abend requested a transaction dump.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AbendDumpDisposition {
    /// The originating runtime did not expose a dump decision.
    Unspecified,
    /// The runtime requested a transaction dump.
    Requested,
    /// The runtime explicitly suppressed a transaction dump.
    Suppressed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Modeled abnormal termination with optional reason and reported dump disposition.
///
/// Carrying a dump request does not itself produce a dump.
pub struct Abend {
    /// Abnormal termination code reported by the machine.
    pub code: String,
    /// Optional explanatory text from the originating runtime.
    pub reason: Option<String>,
    /// Transaction-dump disposition reported by the originating runtime.
    pub dump: AbendDumpDisposition,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Suspension metadata describing how a consumer may identify resumable work.
///
/// Tokens are opaque strings here; this type does not persist or restore state.
pub struct Suspension {
    /// Opaque suspension category interpreted by the consumer.
    pub kind: String,
    /// Opaque token identifying continuation or resume information.
    pub resume_token: String,
    /// Reported size of suspended state in bytes.
    pub state_bytes: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Owned selector, artifact, and bounded input for a requested child execution.
pub struct ChildInvocation {
    /// Entry selector requested for the child.
    pub selector: Selector,
    /// Artifact reference requested for the child.
    pub artifact: ArtifactRef,
    /// Owned bounded input to the child.
    pub payload: BoundedPayload,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Owned target and bounded input for a requested control transfer.
pub struct Transfer {
    /// Entry selector receiving transferred control.
    pub selector: Selector,
    /// Owned bounded input for the transfer target.
    pub payload: BoundedPayload,
    /// Whether the request calls for replacing the current frame.
    pub replace_frame: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Result of one bounded machine drive, transferring any emitted value to the caller.
///
/// `E` is the implementation-defined host effect. The caller handles dispatch,
/// child execution, suspension, and terminal results.
pub enum MachineDrive<E> {
    /// The machine remains runnable and can be driven again.
    Continue,
    /// A host effect requiring caller dispatch before supplying a host result.
    HostCall(E),
    /// A child execution requested by the machine.
    Invoke(ChildInvocation),
    /// A request to transfer control to another selector.
    Transfer(Transfer),
    /// The machine reports suspension metadata to the caller.
    Suspended(Suspension),
    /// The machine produced its application return code and output.
    Completed(Completion),
    /// The machine reports a modeled application condition.
    Condition(Condition),
    /// The machine reports modeled abnormal termination.
    Abend(Abend),
    /// The machine reports a diagnostic failure, retaining any uncertainty.
    Failed(ExecutionProblem),
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Input supplied when driving a machine, including results of external work.
///
/// `R` is the implementation-defined host result. Implementations decide which
/// resume inputs are valid for their current state.
pub enum MachineResume<R> {
    /// Begin or continue runnable work without an external result.
    Start,
    /// Deliver the result of a previously requested host effect.
    HostResult(R),
    /// Deliver a completed child execution result.
    ChildCompleted(Completion),
    /// Notify the machine that cancellation was selected by its caller.
    Cancelled,
    /// Notify the machine that its caller selected deadline expiry.
    TimedOut,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Coordinator-facing outcome preserving completion, control, and failure categories.
///
/// Failure details retain their diagnostic information, including any unknown
/// outcome; cancellation or timeout is not proof that dispatched work was undone.
pub enum ExecutionOutcome {
    /// Ordinary application completion with return code and output.
    Completed(Completion),
    /// A modeled application condition reported by execution.
    Condition(Condition),
    /// Execution yielded suspension metadata.
    Suspended(Suspension),
    /// Execution yielded a request for a child invocation.
    Invoke(ChildInvocation),
    /// Execution yielded a request for control transfer.
    Transfer(Transfer),
    /// Execution ended with modeled abnormal termination.
    Abend(Abend),
    /// Execution stopped under cancellation control.
    Cancelled,
    /// Execution stopped under deadline control.
    TimedOut,
    /// Execution was rejected with a diagnostic reason.
    Rejected(ExecutionProblem),
    /// Execution exhausted a resource budget, with diagnostic details.
    ResourceExhausted(ExecutionProblem),
    /// A provider failure with diagnostic details and any outcome uncertainty.
    ProviderFailure(ExecutionProblem),
    /// An infrastructure failure with diagnostic details and any uncertainty.
    InfrastructureFailure(ExecutionProblem),
}

/// Stateful execution boundary advanced by explicit resume input and a quantum.
///
/// Effects and results are owned values crossing the caller/machine boundary.
/// The trait supplies no dispatcher or persistence service. Checkpoint support
/// and effect sequencing are optional through their default implementations.
pub trait Machine {
    /// Owned host-effect request emitted by this machine.
    type Effect;
    /// Owned host result accepted when resuming this machine.
    type EffectResult;
    /// Advance mutable machine state using a resume input and per-call budgets.
    ///
    /// Return the next runnable, external-work, suspension, or terminal boundary.
    /// The implementation validates resume/state compatibility and enforces budgets.
    fn drive(
        &mut self,
        resume: MachineResume<Self::EffectResult>,
        quantum: Quantum,
    ) -> MachineDrive<Self::Effect>;

    /// Return an optional bounded state image for caller-managed persistence.
    ///
    /// The default returns `None`; this method does not write durable state.
    fn checkpoint(&self) -> Option<BoundedPayload> {
        None
    }

    /// Report the machine's current effect sequence to the caller.
    ///
    /// The default reports zero; sequencing semantics belong to the implementation.
    fn effect_sequence(&self) -> u64 {
        0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// A durable execution lifecycle transition.
pub enum LifecycleEventKind {
    /// The execution identity was accepted into durable state.
    Admitted,
    /// The execution is eligible for a worker.
    Queued,
    /// A worker acquired the execution.
    Claimed,
    /// Machine execution began or resumed on a worker.
    Started,
    /// The machine produced its terminal value and is committing it.
    Completing,
    /// A host-effect intent was durably recorded before dispatch.
    EffectIntent {
        /// Monotonic effect sequence within the execution.
        sequence: u64,
    },
    /// A host-effect result was durably recorded after dispatch.
    EffectResult {
        /// Monotonic effect sequence within the execution.
        sequence: u64,
    },
    /// A restorable checkpoint was committed.
    Suspended,
    /// The interpreter checkpoint was durably handed to a product-owned
    /// continuation, so this execution no longer owns resumable work.
    HandoffCompleted,
    /// Execution continued from a committed checkpoint.
    Resumed,
    /// Durable cancellation was requested.
    CancellationRequested,
    /// Execution stopped because cancellation won the control race.
    Cancelled,
    /// Execution stopped at its declared deadline.
    TimedOut,
    /// Execution reached an ordinary terminal completion.
    Completed {
        /// Application return code committed with the terminal transition.
        return_code: i32,
    },
    /// Execution ended with a modeled application condition.
    Condition,
    /// Execution ended with a modeled abnormal termination.
    Abend,
    /// Execution ended with a non-condition failure.
    Failed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// One ordered, attempt-scoped lifecycle observation.
pub struct LifecycleEvent {
    /// Stable identity shared by every event for the execution.
    pub execution_id: ExecutionId,
    /// Runtime instance that emitted the event.
    pub run_unit_id: RunUnitId,
    /// Positive ordering sequence within the execution.
    pub sequence: u64,
    /// Positive delivery or execution attempt.
    pub attempt: u32,
    /// Caller-owned logical time used for deterministic ordering.
    pub tick: u64,
    /// Typed transition represented by this event.
    pub kind: LifecycleEventKind,
}

impl LifecycleEvent {
    /// Return whether the event meets the contract's non-zero ordering rules.
    #[must_use]
    pub fn validate(&self) -> bool {
        self.sequence > 0 && self.attempt > 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{InvocationLimits, PrincipalId};

    #[test]
    fn quantum_is_never_zero() {
        assert!(Quantum::new(0, 1).is_none());
        assert!(Quantum::new(1, 0).is_none());
        assert_eq!(Quantum::new(1, 1).unwrap().max_steps, 1);
    }

    #[test]
    fn opaque_identity_example_is_valid() {
        assert!(PrincipalId::new("IBMUSER", InvocationLimits::default()).is_ok());
    }
}
