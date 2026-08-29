use mainframe_env_execution_api::{
    ArtifactRef, ExecutionId, IdempotencyKey, LifecycleEvent, PrincipalId, RunUnitId, Selector,
};
use std::fmt;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExecutionState {
    Admitted,
    Queued,
    Running,
    Suspended,
    Completing,
    Completed,
    Failed,
    Cancelled,
    TimedOut,
    DeadLetter,
}

impl ExecutionState {
    #[must_use]
    pub fn can_transition_to(self, next: Self) -> bool {
        use ExecutionState as S;
        matches!(
            (self, next),
            (S::Admitted, S::Queued | S::Cancelled | S::TimedOut)
                | (
                    S::Queued,
                    S::Running | S::Cancelled | S::TimedOut | S::DeadLetter
                )
                | (
                    S::Running,
                    S::Suspended | S::Completing | S::Failed | S::Cancelled | S::TimedOut
                )
                | (S::Suspended, S::Queued | S::Cancelled | S::TimedOut)
                | (S::Completing, S::Completed | S::Failed)
        )
    }

    #[must_use]
    pub const fn terminal(self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Failed | Self::Cancelled | Self::TimedOut | Self::DeadLetter
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExecutionRecord {
    pub execution_id: ExecutionId,
    pub run_unit_id: RunUnitId,
    pub selector: Selector,
    pub artifact: ArtifactRef,
    pub principal: PrincipalId,
    pub state: ExecutionState,
    pub attempt: u32,
    pub version: u64,
    pub owner_lease: Option<String>,
    pub lease_expiry_tick: Option<u64>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkState {
    Queued,
    Claimed,
    Completed,
    DeadLetter,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkRecord {
    pub work_id: String,
    pub execution_id: ExecutionId,
    pub state: WorkState,
    pub attempt: u32,
    pub available_tick: u64,
    pub lease_id: Option<String>,
    pub lease_expiry_tick: Option<u64>,
    pub payload: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CheckpointRecord {
    pub execution_id: ExecutionId,
    pub run_unit_id: RunUnitId,
    pub schema_version: u32,
    pub artifact: ArtifactRef,
    pub effect_sequence: u64,
    pub payload_digest: [u8; 32],
    pub payload: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionRecord {
    pub session_id: String,
    pub principal: PrincipalId,
    pub schema_version: u32,
    pub version: u64,
    pub payload: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArtifactRecord {
    pub artifact: ArtifactRef,
    pub media_type: String,
    pub payload_digest: [u8; 32],
    pub payload: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GenerationRecord {
    pub provider: String,
    pub generation: String,
    pub ready: bool,
    pub draining: bool,
    pub version: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EffectState {
    Intent,
    Completed,
    Failed,
    UnknownOutcome,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EffectRecord {
    pub execution_id: ExecutionId,
    pub run_unit_id: RunUnitId,
    pub sequence: u64,
    pub key: IdempotencyKey,
    pub request_digest: [u8; 32],
    pub state: EffectState,
    pub result_digest: Option<[u8; 32]>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StoreError {
    NotFound,
    AlreadyExists,
    Conflict,
    InvalidTransition,
    InvalidSequence,
    CapacityExceeded,
    PayloadTooLarge,
    LeaseConflict,
    IncompatibleVersion,
    Poisoned,
    Infrastructure(String),
}

impl fmt::Display for StoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "store failed: {self:?}")
    }
}
impl std::error::Error for StoreError {}

// Retain the imported type in this contract module so implementations cannot
// substitute a framework event row at the boundary.
#[allow(dead_code)]
fn owned_event(_: LifecycleEvent) {}
