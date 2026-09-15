use crate::{ArtifactRef, BoundedPayload, ExecutionId, RunUnitId, Selector};
use mainframe_env_diagnostics::ExecutionProblem;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct FrameId(u32);

impl FrameId {
    pub fn new(value: u32) -> Option<Self> {
        (value > 0).then_some(Self(value))
    }
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Frame {
    pub id: FrameId,
    pub depth: u32,
    pub selector: Selector,
    pub return_target: Option<String>,
}

impl Frame {
    #[must_use]
    pub fn within_limit(&self, max_frames: u32) -> bool {
        self.depth > 0 && self.depth <= max_frames
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Quantum {
    pub max_steps: u32,
    pub max_allocated_bytes: u64,
}

impl Quantum {
    pub fn new(max_steps: u32, max_allocated_bytes: u64) -> Option<Self> {
        (max_steps > 0 && max_allocated_bytes > 0).then_some(Self {
            max_steps,
            max_allocated_bytes,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Completion {
    pub return_code: i32,
    pub output: BoundedPayload,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Condition {
    pub name: String,
    pub response: i32,
    pub response2: i32,
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
pub struct Abend {
    pub code: String,
    pub reason: Option<String>,
    /// Transaction-dump disposition reported by the originating runtime.
    pub dump: AbendDumpDisposition,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Suspension {
    pub kind: String,
    pub resume_token: String,
    pub state_bytes: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChildInvocation {
    pub selector: Selector,
    pub artifact: ArtifactRef,
    pub payload: BoundedPayload,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Transfer {
    pub selector: Selector,
    pub payload: BoundedPayload,
    pub replace_frame: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MachineDrive<E> {
    Continue,
    HostCall(E),
    Invoke(ChildInvocation),
    Transfer(Transfer),
    Suspended(Suspension),
    Completed(Completion),
    Condition(Condition),
    Abend(Abend),
    Failed(ExecutionProblem),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MachineResume<R> {
    Start,
    HostResult(R),
    ChildCompleted(Completion),
    Cancelled,
    TimedOut,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExecutionOutcome {
    Completed(Completion),
    Condition(Condition),
    Suspended(Suspension),
    Invoke(ChildInvocation),
    Transfer(Transfer),
    Abend(Abend),
    Cancelled,
    TimedOut,
    Rejected(ExecutionProblem),
    ResourceExhausted(ExecutionProblem),
    ProviderFailure(ExecutionProblem),
    InfrastructureFailure(ExecutionProblem),
}

pub trait Machine {
    type Effect;
    type EffectResult;
    fn drive(
        &mut self,
        resume: MachineResume<Self::EffectResult>,
        quantum: Quantum,
    ) -> MachineDrive<Self::Effect>;

    fn checkpoint(&self) -> Option<BoundedPayload> {
        None
    }

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
