use crate::{ArtifactRef, BoundedPayload, ExecutionId, RunUnitId, Selector};
use mainframe_env_diagnostics::ExecutionProblem;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
/// Positive machine-local frame identifier, not a durable execution or handle token.
pub struct FrameId(u32);

impl FrameId {
    /// Accept a positive numeric frame identity; uniqueness is the machine owner's responsibility.
    pub fn new(value: u32) -> Option<Self> {
        (value > 0).then_some(Self(value))
    }
    #[must_use]
    /// Return the original positive value without changing its scope.
    pub const fn get(self) -> u32 {
        self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Application call-frame metadata, separate from durable invocation attribution.
pub struct Frame {
    /// Machine-local identity of this active frame.
    pub id: FrameId,
    /// Positive nesting depth checked against the invocation's frame budget.
    pub depth: u32,
    /// Admitted code selector for this frame.
    pub selector: Selector,
    /// Optional machine-owned continuation target used when this frame returns.
    pub return_target: Option<String>,
}

impl Frame {
    #[must_use]
    /// Check positive depth within the supplied budget; this does not validate frame identity/linkage.
    pub fn within_limit(&self, max_frames: u32) -> bool {
        self.depth > 0 && self.depth <= max_frames
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Positive per-drive work budget, independent of invocation-wide resource limits.
pub struct Quantum {
    /// Maximum deterministic machine steps in this drive.
    pub max_steps: u32,
    /// Maximum allocated bytes in this drive, as measured by the machine implementation.
    pub max_allocated_bytes: u64,
}

impl Quantum {
    /// Reject either zero budget; scheduling and actual accounting remain with the caller/machine.
    pub fn new(max_steps: u32, max_allocated_bytes: u64) -> Option<Self> {
        (max_steps > 0 && max_allocated_bytes > 0).then_some(Self {
            max_steps,
            max_allocated_bytes,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Ordinary application completion, not proof of durable publication or provider success.
pub struct Completion {
    /// Signed application return code in the program's own domain.
    pub return_code: i32,
    /// Bounded schema-labelled application output.
    pub output: BoundedPayload,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Modeled application condition, kept distinct from infrastructure failures.
pub struct Condition {
    /// Condition identity interpreted by the originating language/subsystem.
    pub name: String,
    /// Primary response in that subsystem's response domain.
    pub response: i32,
    /// Secondary detail whose meaning depends on the condition and primary response.
    pub response2: i32,
    /// Whether application control handled this condition rather than propagating it.
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
/// Modeled abnormal termination; recording it does not itself create a transaction dump.
pub struct Abend {
    /// Originating runtime's abnormal-termination code.
    pub code: String,
    /// Optional runtime explanation, distinct from the dump decision.
    pub reason: Option<String>,
    /// Transaction-dump disposition reported by the originating runtime.
    pub dump: AbendDumpDisposition,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Machine suspension observation; restore requires a separately compatible checkpoint.
pub struct Suspension {
    /// Machine-owned suspension classification.
    pub kind: String,
    /// Opaque continuation identity, not permission to resume an unrelated execution.
    pub resume_token: String,
    /// Reported retained state size in bytes for bounded checkpoint accounting.
    pub state_bytes: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Request for an admitted child execution; the coordinator owns child lifecycle and control.
pub struct ChildInvocation {
    /// Child program selector to resolve through the existing admission authority.
    pub selector: Selector,
    /// Exact child code artifact reference, not a host-native program address.
    pub artifact: ArtifactRef,
    /// Bounded child input, with schema-specific interpretation owned by its receiver.
    pub payload: BoundedPayload,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Program-control transfer request, not an independently created execution authority.
pub struct Transfer {
    /// Target program selector to admit before execution.
    pub selector: Selector,
    /// Bounded transferred application input.
    pub payload: BoundedPayload,
    /// Whether the machine requests replacing the current frame rather than retaining it.
    pub replace_frame: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// One deterministic drive observation; the caller owns external dispatch and durable publication.
pub enum MachineDrive<E> {
    /// The quantum ended while the machine can continue without external input.
    Continue,
    /// An original typed effect awaits host dispatch and a matching resume result.
    HostCall(E),
    /// Child admission/execution is required before the parent can continue.
    Invoke(ChildInvocation),
    /// A program-control transfer needs the caller's admission handling.
    Transfer(Transfer),
    /// Execution yielded with an explicit suspension observation.
    Suspended(Suspension),
    /// Application computation completed; durable completion remains coordinator-owned.
    Completed(Completion),
    /// A modeled condition is returned without collapsing it into generic failure.
    Condition(Condition),
    /// A modeled abnormal termination is returned.
    Abend(Abend),
    /// Machine execution failed with a typed execution problem.
    Failed(ExecutionProblem),
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Input to a machine drive; implementations must match it to their current pending state.
pub enum MachineResume<R> {
    /// Begin or continue ordinary execution without an external result.
    Start,
    /// Return the host observation for the original pending effect.
    HostResult(R),
    /// Resume the parent after its admitted child completed.
    ChildCompleted(Completion),
    /// The execution owner observed cancellation; no mutation-absence guarantee is implied.
    Cancelled,
    /// The execution owner observed its deadline; uncertain effects still require reconciliation.
    TimedOut,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Coordinator-facing execution disposition, retaining application/control/failure distinctions.
pub enum ExecutionOutcome {
    /// Ordinary application completion and bounded output.
    Completed(Completion),
    /// Modeled application condition and handling status.
    Condition(Condition),
    /// Explicit suspension requiring compatible retained state to resume.
    Suspended(Suspension),
    /// Child execution request awaiting admission.
    Invoke(ChildInvocation),
    /// Program-control transfer awaiting admission.
    Transfer(Transfer),
    /// Modeled abnormal application termination.
    Abend(Abend),
    /// Cancellation won execution control; dispatched mutations may still need reconciliation.
    Cancelled,
    /// The logical deadline won execution control, independently of effect uncertainty.
    TimedOut,
    /// Admission or validation rejected execution.
    Rejected(ExecutionProblem),
    /// An owned execution budget was exhausted.
    ResourceExhausted(ExecutionProblem),
    /// A selected provider reported failure.
    ProviderFailure(ExecutionProblem),
    /// Execution infrastructure failed, distinct from a modeled application condition.
    InfrastructureFailure(ExecutionProblem),
}

/// Deterministic machine port with owned effects and explicit resume observations.
/// The implementation does not own provider dispatch, journal publication or shared recovery.
pub trait Machine {
    /// Typed original host-effect request emitted by this machine.
    type Effect;
    /// Matching typed host observation consumed on resume.
    type EffectResult;
    /// Advance within a quantum from the supplied resume state.
    /// External work is returned as data; the caller decides dispatch and durable transitions.
    fn drive(
        &mut self,
        resume: MachineResume<Self::EffectResult>,
        quantum: Quantum,
    ) -> MachineDrive<Self::Effect>;

    /// Return compatible bounded resumable state, or `None` when capture is unsupported.
    /// Absence must not be replaced with a guessed checkpoint; the default refuses capture.
    fn checkpoint(&self) -> Option<BoundedPayload> {
        None
    }

    /// Last effect occurrence known to the machine, used to bind checkpoint/replay state.
    /// The default zero reports no sequence tracking; it is not a durable journal lookup.
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
