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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Abend {
    pub code: String,
    pub reason: Option<String>,
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
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LifecycleEventKind {
    Admitted,
    Queued,
    Claimed,
    Started,
    EffectIntent { sequence: u64 },
    EffectResult { sequence: u64 },
    Suspended,
    Resumed,
    CancellationRequested,
    Cancelled,
    TimedOut,
    Completed { return_code: i32 },
    Condition,
    Abend,
    Failed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LifecycleEvent {
    pub execution_id: ExecutionId,
    pub run_unit_id: RunUnitId,
    pub sequence: u64,
    pub attempt: u32,
    pub tick: u64,
    pub kind: LifecycleEventKind,
}

impl LifecycleEvent {
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
