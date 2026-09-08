use mainframe_env_execution_api::{
    ArtifactRef, CapabilityId, ExecutionId, IdempotencyKey, LifecycleEvent, PrincipalId, RunUnitId,
    Selector,
};
use std::collections::BTreeMap;
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
    Cancelled,
    DeadLetter,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkRecord {
    pub work_id: String,
    pub execution_id: ExecutionId,
    pub required_selector: Selector,
    pub required_generation: String,
    pub artifact: ArtifactRef,
    pub state: WorkState,
    pub priority: u8,
    pub attempt: u32,
    pub max_attempts: u32,
    pub available_tick: u64,
    pub deadline_tick: u64,
    pub cancellation_requested: bool,
    pub worker_id: Option<String>,
    pub lease_id: Option<String>,
    pub lease_epoch: u64,
    pub lease_expiry_tick: Option<u64>,
    pub heartbeat_tick: Option<u64>,
    pub checkpoint_id: Option<String>,
    pub effect_sequence: u64,
    pub payload: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CheckpointRecord {
    pub execution_id: ExecutionId,
    pub run_unit_id: RunUnitId,
    pub session_id: Option<String>,
    pub schema_version: u32,
    pub machine_schema_version: u32,
    pub artifact: ArtifactRef,
    pub provider_generation: String,
    pub required_host_interfaces: BTreeMap<String, String>,
    pub effect_sequence: u64,
    pub transaction: Option<String>,
    pub principal: PrincipalId,
    pub security_classification: String,
    pub encryption_key_reference: Option<String>,
    pub payload_size: u64,
    pub payload_digest: [u8; 32],
    pub payload: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OutboxRecord {
    pub notification_id: String,
    pub execution_id: ExecutionId,
    pub sequence: u64,
    pub topic: String,
    pub payload: Vec<u8>,
    pub attempt: u32,
    pub delivered: bool,
    pub version: u64,
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

pub const MAX_EFFECT_RECOVERY_OWNER_BYTES: usize = 128;

/// Durable ownership fence for one stale-intent reconciliation attempt.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EffectRecoveryLease {
    pub owner: String,
    pub attempt: u32,
    pub epoch: u64,
    pub expires_tick: u64,
}

/// Metadata that makes an in-flight effect intent attributable and ageable.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EffectIntentMetadata {
    pub owner: ExecutionId,
    pub attempt: u32,
    pub capability: Option<CapabilityId>,
    pub created_tick: u64,
    pub recovery_after_tick: u64,
    pub epoch: u64,
    pub recovery_lease: Option<EffectRecoveryLease>,
}

/// Identity of the persisted digest algorithm/domain. Never compare across formats.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EffectDigestFormat {
    /// Pre-hardening diagnostic encoding. Read/reconcile only; do not recompute.
    LegacyDebug,
    /// Explicit host canonical bytes, domain/version 1, SHA-256.
    CanonicalHostV1,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EffectRecord {
    pub execution_id: ExecutionId,
    pub run_unit_id: RunUnitId,
    pub sequence: u64,
    pub key: IdempotencyKey,
    pub digest_format: EffectDigestFormat,
    pub request_digest: [u8; 32],
    pub intent: EffectIntentMetadata,
    pub state: EffectState,
    pub result_digest: Option<[u8; 32]>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderStateRecord {
    pub namespace: String,
    pub key: String,
    pub version: u64,
    pub payload: Vec<u8>,
}

impl ProviderStateRecord {
    /// Validate the backend-independent compare-and-swap move contract.
    ///
    /// Identifiers must be nonempty, source and destination must differ, and
    /// the new positive version must be the successor of `expected_version`.
    /// Versions share the signed 64-bit range used by durable SQL adapters.
    /// Shape/version failures take precedence over the payload bound. Missing
    /// or stale source state and an occupied destination are CAS conflicts;
    /// implementations must reject them without modifying either record.
    pub fn validate_move(
        &self,
        old_key: &str,
        expected_version: u64,
        max_payload_bytes: usize,
    ) -> Result<(), StoreError> {
        if self.namespace.is_empty()
            || self.key.is_empty()
            || old_key.is_empty()
            || self.key == old_key
            || expected_version == 0
            || self.version > i64::MAX as u64
            || expected_version.checked_add(1) != Some(self.version)
        {
            return Err(StoreError::Conflict);
        }
        if self.payload.len() > max_payload_bytes {
            return Err(StoreError::PayloadTooLarge);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderStateWrite {
    pub record: ProviderStateRecord,
    pub expected_version: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProviderStateMutation {
    Put(ProviderStateWrite),
    Delete {
        namespace: String,
        key: String,
        expected_version: u64,
    },
    Move {
        record: ProviderStateRecord,
        old_key: String,
        expected_version: u64,
    },
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
