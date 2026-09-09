use mainframe_env_execution_api::{
    ArtifactRef, AuditResourceDigest, CapabilityId, ExecutionId, IdempotencyKey, LifecycleEvent,
    PrincipalId, RunUnitId, Selector,
};
use std::collections::BTreeMap;
use std::fmt;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Durable state of one execution attempt.
pub enum ExecutionState {
    /// Identity and admission records exist.
    Admitted,
    /// Execution is waiting for an eligible worker.
    Queued,
    /// A worker is actively driving the machine.
    Running,
    /// A restorable checkpoint is durable.
    Suspended,
    /// The terminal machine result is being committed.
    Completing,
    /// The execution completed normally.
    Completed,
    /// The execution failed outside a modeled condition or ABEND.
    Failed,
    /// Cancellation won before terminal completion.
    Cancelled,
    /// The declared deadline elapsed before terminal completion.
    TimedOut,
    /// Retry policy was exhausted or the work was unrecoverable.
    DeadLetter,
}

impl ExecutionState {
    /// Return whether `next` is a legal direct durable transition.
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
                | (
                    S::Suspended,
                    S::Queued | S::Completed | S::Cancelled | S::TimedOut
                )
                | (S::Completing, S::Completed | S::Failed)
        )
    }

    /// Return whether no later execution transition is permitted.
    #[must_use]
    pub const fn terminal(self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Failed | Self::Cancelled | Self::TimedOut | Self::DeadLetter
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Optimistically versioned durable execution metadata.
pub struct ExecutionRecord {
    /// Stable execution identity.
    pub execution_id: ExecutionId,
    /// Current run-unit identity.
    pub run_unit_id: RunUnitId,
    /// Program or runtime selector required by this execution.
    pub selector: Selector,
    /// Immutable executable artifact identity.
    pub artifact: ArtifactRef,
    /// Principal on whose behalf the execution runs.
    pub principal: PrincipalId,
    /// Current lifecycle state.
    pub state: ExecutionState,
    /// Positive execution or delivery attempt.
    pub attempt: u32,
    /// Positive compare-and-swap version.
    pub version: u64,
    /// Current worker lease owner, when leased.
    pub owner_lease: Option<String>,
    /// Logical tick after which the owner lease is invalid.
    pub lease_expiry_tick: Option<u64>,
    /// Logical tick copied from the durable terminal lifecycle transition.
    pub terminal_tick: Option<u64>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Durable queue-item state.
pub enum WorkState {
    /// Eligible for a future claim.
    Queued,
    /// Held under a fenced, expiring worker lease.
    Claimed,
    /// Completed exactly once.
    Completed,
    /// Cancelled before completion.
    Cancelled,
    /// Terminally removed from retry processing.
    DeadLetter,
}

impl WorkState {
    /// Whether no worker may claim or resume the work item.
    #[must_use]
    pub const fn terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Cancelled | Self::DeadLetter)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkRecord {
    pub work_id: String,
    pub execution_id: ExecutionId,
    pub required_selector: Selector,
    pub required_generation: String,
    pub artifact: ArtifactRef,
    pub state: WorkState,
    /// Scheduling priority; larger values are claimed before smaller values.
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
    /// Logical tick at which the work item entered a terminal state.
    pub terminal_tick: Option<u64>,
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
    /// Logical tick at which delivery was durably acknowledged.
    pub delivered_tick: Option<u64>,
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

/// A bounded, backend-reported artifact authority health and capacity snapshot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ArtifactStoreHealth {
    /// Whether the selected authority can read and validate its durable metadata.
    pub readable: bool,
    /// Whether a bounded write can reach the selected authority.
    pub writable: bool,
    /// Number of immutable objects currently charged to a configured object quota.
    pub used_objects: Option<usize>,
    /// Maximum immutable objects accepted by the authority, when it has a count quota.
    pub max_objects: Option<usize>,
    /// Bytes currently charged to the authority's enforced aggregate byte quota.
    pub used_bytes: Option<usize>,
    /// Maximum aggregate bytes accepted by the authority, when such a quota is enforced.
    pub max_bytes: Option<usize>,
}

impl ArtifactStoreHealth {
    /// Return remaining object slots when the backend exposes a count quota.
    #[must_use]
    pub fn object_headroom(self) -> Option<usize> {
        self.max_objects
            .zip(self.used_objects)
            .and_then(|(capacity, used)| capacity.checked_sub(used))
    }

    /// Return remaining bytes when the backend exposes an aggregate byte quota.
    #[must_use]
    pub fn byte_headroom(self) -> Option<usize> {
        self.max_bytes
            .zip(self.used_bytes)
            .and_then(|(capacity, used)| capacity.checked_sub(used))
    }

    /// Require readable and writable storage plus nonzero headroom in every reported quota.
    #[must_use]
    pub fn ready(self) -> bool {
        self.readable
            && self.writable
            && headroom_ready(self.used_objects, self.max_objects)
            && headroom_ready(self.used_bytes, self.max_bytes)
    }
}

fn headroom_ready(used: Option<usize>, capacity: Option<usize>) -> bool {
    match (used, capacity) {
        (None, None) => true,
        (Some(used), Some(capacity)) => used < capacity,
        _ => false,
    }
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

/// Maximum UTF-8 byte length of a stale-effect recovery owner identity.
pub const MAX_EFFECT_RECOVERY_OWNER_BYTES: usize = 128;

/// Durable ownership fence for one stale-intent reconciliation attempt.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EffectRecoveryLease {
    /// Bounded identity of the recovery worker holding the lease.
    pub owner: String,
    /// One-based number of times recovery has claimed this effect.
    pub attempt: u32,
    /// Monotonic fencing epoch for the recovery lease.
    pub epoch: u64,
    /// Logical clock tick at which another worker may reclaim the effect.
    pub expires_tick: u64,
}

/// Metadata that makes an in-flight effect intent attributable and ageable.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EffectIntentMetadata {
    /// Durable execution that dispatched the effect.
    pub owner: ExecutionId,
    /// Positive execution attempt that dispatched the effect.
    pub attempt: u32,
    /// Capability selected for the original host dispatch, when retained.
    pub capability: Option<CapabilityId>,
    /// Resource identity needed to reconstruct an audit after an in-doubt restart.
    pub audit_resource: Option<AuditResourceDigest>,
    /// Invocation identity used to make recovered audit records unique and correlatable.
    pub audit_invocation_key: Option<IdempotencyKey>,
    /// Logical clock tick at which the intent became durable.
    pub created_tick: u64,
    /// Earliest logical clock tick at which recovery may claim the intent.
    pub recovery_after_tick: u64,
    /// Monotonic intent epoch used to fence concurrent recovery.
    pub epoch: u64,
    /// Current recovery ownership fence, if a live worker claimed the intent.
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
    /// Attribution, age, audit, and recovery-fencing metadata for the intent.
    pub intent: EffectIntentMetadata,
    pub state: EffectState,
    pub result_digest: Option<[u8; 32]>,
    /// Non-zero logical tick at which a completed or failed outcome became durable.
    ///
    /// `None` is the protected compatibility form for legacy results whose resolution
    /// time cannot be reconstructed safely.
    pub resolved_tick: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderStateRecord {
    pub namespace: String,
    pub key: String,
    pub version: u64,
    pub payload: Vec<u8>,
}

/// Maximum UTF-8 bytes accepted in a durable provider-state namespace.
pub const MAX_PROVIDER_NAMESPACE_BYTES: usize = 256;
/// Maximum UTF-8 bytes accepted in a durable provider-state key.
pub const MAX_PROVIDER_KEY_BYTES: usize = 1_024;
/// Maximum bounded provider-state scan, including one overflow-probe row.
pub const MAX_PROVIDER_STATE_SCAN: usize = 262_145;

impl ProviderStateRecord {
    /// Validate bounded identity, positive SQL-compatible version, and payload size.
    pub fn validate_write(&self, max_payload_bytes: usize) -> Result<(), StoreError> {
        if self.namespace.is_empty()
            || self.namespace.len() > MAX_PROVIDER_NAMESPACE_BYTES
            || self.key.is_empty()
            || self.key.len() > MAX_PROVIDER_KEY_BYTES
            || self.version == 0
            || self.version > i64::MAX as u64
        {
            return Err(StoreError::IncompatibleVersion);
        }
        if self.payload.len() > max_payload_bytes {
            return Err(StoreError::PayloadTooLarge);
        }
        Ok(())
    }

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
            || self.namespace.len() > MAX_PROVIDER_NAMESPACE_BYTES
            || self.key.is_empty()
            || self.key.len() > MAX_PROVIDER_KEY_BYTES
            || old_key.is_empty()
            || old_key.len() > MAX_PROVIDER_KEY_BYTES
            || self.key == old_key
            || expected_version == 0
            || self.version == 0
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

/// Exact provider-state identity used by retention dependency proofs.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ProviderStateIdentity {
    /// Provider-state namespace.
    pub namespace: String,
    /// Provider-state key.
    pub key: String,
}

/// Absolute maximum number of source rows moved by one retention transaction.
pub const MAX_RETENTION_BATCH: usize = 4_096;
/// Maximum owner/effect edges emitted by an exhaustive provider dependency inventory.
pub const MAX_CORE_RETENTION_DEPENDENCIES: usize = MAX_PROVIDER_STATE_SCAN * 5;

/// Closed family of independently forecast and pruned durable record classes.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum RetentionTarget {
    /// Terminal execution identity rows after every dependency is gone.
    TerminalExecutions,
    /// Terminal work rows after their recovery dependencies are gone.
    TerminalWork,
    /// Ordered lifecycle-event rows.
    LifecycleEvents,
    /// Successfully delivered lifecycle-notification rows.
    DeliveredOutbox,
    /// Completed or failed mutation-effect receipts.
    ResolvedEffects,
    /// Db2 durable provider replay receipts.
    Db2Replay,
    /// IMS durable provider replay receipts.
    ImsReplay,
    /// MQ durable provider replay receipts.
    MqReplay,
    /// CICS outer-effect replay receipts.
    CicsReplay,
    /// Dataset mutation replay receipts.
    DatasetReplay,
    /// Finalized CICS unit-of-work receipts.
    CicsUnitOfWork,
    /// Typed security-decision audit records.
    Audit,
    /// Terminal RACF audit, transaction, and recovery evidence.
    RacfEvidence,
    /// Terminal installed-COBOL replay, protocol, run, instance, and cancellation rows.
    CobolLifecycle,
    /// Physically purged JES spool job rows.
    SpoolJobs,
    /// Bounded durable console response rows.
    ConsoleLog,
}

impl RetentionTarget {
    /// Every supported retention family, exactly once, in dependency-safe operator order.
    pub const ALL: [Self; 16] = [
        Self::Db2Replay,
        Self::ImsReplay,
        Self::MqReplay,
        Self::DatasetReplay,
        Self::CicsUnitOfWork,
        Self::CicsReplay,
        Self::RacfEvidence,
        Self::CobolLifecycle,
        Self::SpoolJobs,
        Self::ConsoleLog,
        Self::ResolvedEffects,
        Self::DeliveredOutbox,
        Self::Audit,
        Self::TerminalWork,
        Self::LifecycleEvents,
        Self::TerminalExecutions,
    ];

    /// Stable kebab-case name used by operator JSON and durable manifests.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::TerminalExecutions => "terminal-executions",
            Self::TerminalWork => "terminal-work",
            Self::LifecycleEvents => "lifecycle-events",
            Self::DeliveredOutbox => "delivered-outbox",
            Self::ResolvedEffects => "resolved-effects",
            Self::Db2Replay => "db2-replay",
            Self::ImsReplay => "ims-replay",
            Self::MqReplay => "mq-replay",
            Self::CicsReplay => "cics-replay",
            Self::DatasetReplay => "dataset-replay",
            Self::CicsUnitOfWork => "cics-unit-of-work",
            Self::Audit => "audit",
            Self::RacfEvidence => "racf-evidence",
            Self::CobolLifecycle => "cobol-lifecycle",
            Self::SpoolJobs => "spool-jobs",
            Self::ConsoleLog => "console-log",
        }
    }
}

/// Validated lifetimes, alert thresholds, and batch bound for retention work.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RetentionPolicy {
    /// Minimum ticks to retain terminal lifecycle, outbox, execution, and work state.
    pub lifecycle_ticks: u64,
    /// Minimum ticks to retain resolved effects and provider replay receipts.
    pub idempotency_ticks: u64,
    /// Minimum ticks to retain typed audit decisions before archival.
    pub audit_ticks: u64,
    /// Minimum ticks to retain an archive batch before permanent deletion.
    pub archive_ticks: u64,
    /// Used-capacity percentage at which early retention warning begins.
    pub low_watermark_percent: u8,
    /// Used-capacity percentage at which urgent retention warning begins.
    pub high_watermark_percent: u8,
    /// Policy-specific source-row bound for one transaction.
    pub max_batch: usize,
}

impl RetentionPolicy {
    /// Reject zero lifetimes, invalid alert ordering, and oversized batches.
    pub fn validate(self) -> Result<Self, StoreError> {
        if self.lifecycle_ticks == 0
            || self.idempotency_ticks == 0
            || self.audit_ticks == 0
            || self.archive_ticks == 0
            || self.low_watermark_percent == 0
            || self.low_watermark_percent >= self.high_watermark_percent
            || self.high_watermark_percent > 100
            || self.max_batch == 0
            || self.max_batch > MAX_RETENTION_BATCH
        {
            Err(StoreError::InvalidTransition)
        } else {
            Ok(self)
        }
    }

    /// Derive inclusive low-watermark ticks for one observation tick.
    pub fn watermarks(self, now_tick: u64) -> Result<RetentionWatermarks, StoreError> {
        self.validate()?;
        Ok(RetentionWatermarks {
            lifecycle_tick: now_tick.saturating_sub(self.lifecycle_ticks),
            idempotency_tick: now_tick.saturating_sub(self.idempotency_ticks),
            audit_tick: now_tick.saturating_sub(self.audit_ticks),
            archive_tick: now_tick.saturating_sub(self.archive_ticks),
        })
    }
}

/// Inclusive age boundaries derived from a retention policy and observation tick.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RetentionWatermarks {
    /// Oldest lifecycle-relative tick still outside the active window.
    pub lifecycle_tick: u64,
    /// Oldest idempotency-relative tick still outside the active window.
    pub idempotency_tick: u64,
    /// Oldest audit tick still outside the active window.
    pub audit_tick: u64,
    /// Oldest archive creation tick still outside the active window.
    pub archive_tick: u64,
}

/// Severity derived from configured capacity watermarks.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum SaturationLevel {
    /// Usage is below the low watermark.
    Healthy,
    /// Usage has reached the early-warning watermark.
    LowWatermark,
    /// Usage has reached the urgent-action watermark.
    HighWatermark,
    /// No source or archive headroom remains.
    Full,
}

/// Bounded snapshot of one target's eligibility and capacity trajectory.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RetentionForecast {
    /// Record family measured by this forecast.
    pub target: RetentionTarget,
    /// Current source rows consuming the target's capacity.
    pub active_records: usize,
    /// Source rows currently safe to archive.
    pub eligible_records: usize,
    /// Source rows protected by age or recovery dependencies.
    pub protected_records: usize,
    /// Capacity backing the source record family.
    pub capacity: usize,
    /// Remaining capacity backing the source record family.
    pub headroom: usize,
    /// Current archive rows sharing the archive authority.
    pub archive_records: usize,
    /// Capacity available to the archive authority.
    pub archive_capacity: usize,
    /// Remaining capacity available for a new archive row.
    pub archive_headroom: usize,
    /// Payload bytes currently retained by verified archive rows.
    pub archive_bytes: u64,
    /// Maximum payload bytes accepted by the archive authority.
    pub archive_byte_capacity: u64,
    /// Remaining payload-byte capacity in the archive authority.
    pub archive_byte_headroom: u64,
    /// Conservative age attestations retained outside live source payloads.
    pub observation_records: usize,
    /// Maximum independently retained age attestations.
    pub observation_capacity: usize,
    /// Remaining age-attestation row capacity.
    pub observation_headroom: usize,
    /// Accounted bytes retained by age attestations.
    pub observation_bytes: u64,
    /// Maximum accounted bytes retained by age attestations.
    pub observation_byte_capacity: u64,
    /// Remaining age-attestation byte capacity.
    pub observation_byte_headroom: u64,
    /// Caller-observed source growth used for the projection.
    pub observed_growth_per_tick: u64,
    /// Projected ticks until either source or archive capacity is exhausted.
    pub ticks_to_capacity: Option<u64>,
    /// Conservative ticks until archive byte capacity is exhausted at worst-case row size.
    pub ticks_to_archive_byte_capacity: Option<u64>,
    /// Conservative ticks until age-attestation byte capacity is exhausted.
    pub ticks_to_observation_byte_capacity: Option<u64>,
    /// Worst severity across source and archive authorities.
    pub saturation: SaturationLevel,
    /// Age boundaries used by the forecast.
    pub watermarks: RetentionWatermarks,
}

/// Capacity and usage of the dedicated archive and observation authorities.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RetentionAuthorityUsage {
    /// Archived source rows for the selected target.
    pub archive_rows: usize,
    /// Shared archive row capacity.
    pub archive_row_capacity: usize,
    /// Accounted archive bytes for the selected target.
    pub archive_bytes: u64,
    /// Archived rows across every target sharing this authority.
    pub shared_archive_rows: usize,
    /// Accounted archive bytes across every target sharing this authority.
    pub shared_archive_bytes: u64,
    /// Shared archive byte capacity.
    pub archive_byte_capacity: u64,
    /// Live age observations for the selected target.
    pub observation_rows: usize,
    /// Shared observation row capacity.
    pub observation_row_capacity: usize,
    /// Accounted observation bytes for the selected target.
    pub observation_bytes: u64,
    /// Age observations across every target sharing this authority.
    pub shared_observation_rows: usize,
    /// Accounted observation bytes across every target sharing this authority.
    pub shared_observation_bytes: u64,
    /// Shared observation byte capacity.
    pub observation_byte_capacity: u64,
}

/// Constant-cost source-capacity status for one retention family.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RetentionTargetCapacity {
    /// Retention family measured by this entry.
    pub target: RetentionTarget,
    /// Rows currently consuming the family's source authority.
    pub used: usize,
    /// Maximum rows accepted by that source authority.
    pub capacity: usize,
    /// Watermark severity for this source authority.
    pub saturation: SaturationLevel,
}

/// Constant-cost retention-capacity health without eligibility or provider-codec scans.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RetentionCapacityHealth {
    /// Exactly one source-capacity entry for every [`RetentionTarget::ALL`] member, in order.
    pub targets: Vec<RetentionTargetCapacity>,
    /// Archived source rows across all families.
    pub archive_rows: usize,
    /// Shared archive row capacity.
    pub archive_row_capacity: usize,
    /// Accounted bytes across all archives.
    pub archive_bytes: u64,
    /// Shared archive byte capacity.
    pub archive_byte_capacity: u64,
    /// Dedicated age observations across all families.
    pub observation_rows: usize,
    /// Shared observation row capacity.
    pub observation_row_capacity: usize,
    /// Accounted bytes across all observations.
    pub observation_bytes: u64,
    /// Shared observation byte capacity.
    pub observation_byte_capacity: u64,
    /// Worst severity across source, archive, and observation authorities.
    pub saturation: SaturationLevel,
}

/// One bounded request to atomically archive and prune a record family.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RetentionRequest {
    /// Record family to process.
    pub target: RetentionTarget,
    /// Non-zero observation tick in the policy's logical clock domain.
    pub now_tick: u64,
    /// Maximum source rows to move in this transaction.
    pub max_records: usize,
}

/// Provider-owned plan that atomically replaces one aggregate row while archiving extracted rows.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderStateArchiveReplacement {
    /// Provider-state mutation epoch observed before and after plan construction.
    pub expected_epoch: u64,
    /// Logical family assigned to the extracted archive rows.
    pub target: RetentionTarget,
    /// Non-zero observation tick for the atomic operation.
    pub archived_tick: u64,
    /// Inclusive provider-specific eligibility boundary.
    pub watermark_tick: u64,
    /// CAS-fenced replacement for the aggregate provider row after extraction.
    pub replacement: ProviderStateWrite,
    /// Exact aggregate source row inspected by the provider codec.
    pub source: ProviderStateRecord,
    /// Exact extracted records with provider-certified age and generic dependencies.
    pub rows: Vec<ProviderRetentionRow>,
}

/// One provider-validated row plus generic recovery dependencies rechecked by the store.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderRetentionRow {
    /// Exact encoded source row accepted by the owning provider's full decoder.
    pub row: ProviderStateRecord,
    /// Owning execution when the row participates in execution replay.
    pub owner_execution: Option<ExecutionId>,
    /// Owning run unit when provider evidence is nested within CICS.
    pub owner_run_unit: Option<RunUnitId>,
    /// Non-zero provider observation/deadline tick used for eligibility.
    pub retention_tick: u64,
    /// Exact sidecar used as the row's age authority, when intrinsic metadata is absent.
    pub observation: Option<RetentionObservationProof>,
    /// Recovery/effect dependency that the store must recheck atomically.
    pub dependency: ProviderRetentionDependency,
}

/// CAS identity and content of one age observation used by a provider plan.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RetentionObservationProof {
    /// Observation version returned by the bounded page API.
    pub version: u64,
    /// Exact observation whose tick and owner were provider-validated.
    pub observation: RetentionObservation,
}

/// Generic dependency proof attached by a provider-owned full-codec validator.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProviderRetentionDependency {
    /// No execution/effect dependency; used only by provider-local terminal evidence.
    None,
    /// A top-level provider effect with exact canonical request and result digests.
    CoreEffect {
        /// Same-key effect identity.
        key: IdempotencyKey,
        /// Exact canonical request digest decoded from the provider row.
        request_digest: [u8; 32],
        /// Exact canonical result digest decoded from the provider row.
        result_digest: [u8; 32],
    },
    /// A CICS-internal provider effect with exact finalized origin and absence fences.
    CicsNested {
        /// Exact finalized CICS UOW/provenance row validated by the provider codec.
        provenance: ProviderStateRecord,
        /// Recovery rows that must remain absent (undo/continuation authorities).
        absent: Vec<ProviderStateIdentity>,
    },
    /// A terminal execution plus exact provider rows that must outlive this row.
    ProviderGraph {
        /// Exact provider-state dependencies rechecked under the retention fence.
        required_rows: Vec<ProviderStateRecord>,
        /// Additional terminal execution owners rechecked with checkpoint/effect protection.
        required_executions: Vec<ExecutionId>,
    },
    /// A direct product route that legitimately has no durable execution row.
    DirectProduct,
}

/// Provider-built, epoch-fenced plan for archiving and deleting independent source rows.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderStateArchiveDeletion {
    /// Provider-state mutation epoch observed before and after full-codec validation.
    pub expected_epoch: u64,
    /// Provider-owned record family.
    pub target: RetentionTarget,
    /// Non-zero observation tick for the atomic operation.
    pub archived_tick: u64,
    /// Inclusive provider-specific eligibility boundary.
    pub watermark_tick: u64,
    /// Bounded exact rows and dependencies selected by the provider.
    pub rows: Vec<ProviderRetentionRow>,
}

/// Provider-owned, epoch-fenced dependency inventory for core lifecycle retention.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoreRetentionDependencySnapshot {
    /// Provider-state epoch captured before and after the exhaustive descriptor scan.
    pub expected_epoch: u64,
    /// Executions which still own live provider replay or recovery evidence.
    pub blocked_executions: Vec<ExecutionId>,
    /// Core effects which must outlive provider replay or UOW evidence.
    pub blocked_effect_keys: Vec<IdempotencyKey>,
    /// True when legacy/corrupt/unattributed evidence prevents safe owner-specific pruning.
    pub unowned: bool,
}

/// Exact live-source assertion used while removing a stale retention observation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProviderRetentionObservationSource {
    /// The provider fully decoded this exact current aggregate or standalone source row.
    Present(ProviderStateRecord),
    /// The standalone logical source was verified absent.
    Absent(ProviderStateIdentity),
}

/// Epoch- and CAS-fenced deletion of one stale or orphaned age observation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderRetentionObservationDeletion {
    /// Provider-state epoch observed while the source was validated.
    pub expected_epoch: u64,
    /// Exact source assertion made by the provider-owned full decoder.
    pub source: ProviderRetentionObservationSource,
    /// Exact stale observation to remove.
    pub observation: RetentionObservation,
    /// Observation version returned by the bounded page API.
    pub expected_observation_version: u64,
}

/// CAS-fenced request to add conservative retention metadata to a legacy row.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RetentionAgeReconciliation {
    /// Legacy record family being reconciled.
    pub target: RetentionTarget,
    /// Exact logical source namespace.
    pub namespace: String,
    /// Exact source-row key.
    pub key: String,
    /// Source version that the operator inspected.
    pub expected_version: u64,
    /// Required owning execution for a legacy provider replay row.
    pub owner_execution: Option<ExecutionId>,
}

/// One protected legacy row and the exact token required for age reconciliation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RetentionLegacyRow {
    /// Legacy record family containing the row.
    pub target: RetentionTarget,
    /// Exact logical source namespace.
    pub namespace: String,
    /// Exact source-row key.
    pub key: String,
    /// Optimistic-concurrency version observed with the legacy encoding.
    pub source_version: u64,
}

/// Conservative age attestation stored without changing the observed live row.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RetentionObservation {
    /// Retention family that owns the observation.
    pub target: RetentionTarget,
    /// Logical source namespace, including provider-owned embedded-row domains.
    pub namespace: String,
    /// Logical source key, unique within `target` and `namespace`.
    pub key: String,
    /// Exact source version observed by the operator or provider validator.
    pub source_version: u64,
    /// SHA-256 of the exact logical source payload that was observed.
    pub source_digest: [u8; 32],
    /// Positive durable tick at which the source was conservatively observed.
    pub observed_tick: u64,
    /// Owning execution when required for replay/recovery validation.
    pub owner_execution: Option<ExecutionId>,
}

/// Receipt proving a legacy row gained a conservative age without being dispatched.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RetentionReconciliationReceipt {
    /// Reconciled record family.
    pub target: RetentionTarget,
    /// Exact logical source namespace that was attested.
    pub namespace: String,
    /// Reconciled source-row key.
    pub key: String,
    /// Exact live source version that was attested without modification.
    pub source_version: u64,
    /// Monotonic version of the dedicated observation row.
    pub observation_version: u64,
    /// Observation tick used as the conservative age boundary.
    pub reconciled_tick: u64,
}

/// Exact original durable row retained inside an archive batch.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArchivedRetentionRow {
    /// Original store namespace.
    pub namespace: String,
    /// Original store key.
    pub key: String,
    /// Original optimistic-concurrency version.
    pub version: u64,
    /// Exact original encoded payload.
    pub payload: Vec<u8>,
    /// Positive intrinsic or conservatively observed eligibility tick.
    pub retention_tick: u64,
    /// Owning execution retained with replay/audit age evidence when applicable.
    pub owner_execution: Option<ExecutionId>,
}

/// Content-verified archive batch written before its source rows are deleted.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RetentionArchive {
    /// Domain-separated SHA-256 identity of metadata and source rows.
    pub archive_id: String,
    /// Record family contained by the batch.
    pub target: RetentionTarget,
    /// Tick at which archive and source deletion committed atomically.
    pub archived_tick: u64,
    /// Source eligibility boundary used by the transaction.
    pub watermark_tick: u64,
    /// Exact archived source rows.
    pub rows: Vec<ArchivedRetentionRow>,
}

/// Exact authorization supplied when one indivisible historical archive exceeds a pass bound.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RetentionArchivePruneRequest {
    /// Non-zero durable observation tick used to derive the archive watermark.
    pub now_tick: u64,
    /// Cumulative source-row bound across whole archive batches.
    pub max_records: usize,
    /// Exact content-addressed archive reviewed by the operator, when an override is required.
    pub authorized_oversized_archive_id: Option<String>,
}

/// Receipt for whole archive batches permanently removed by one bounded operation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RetentionArchivePruneReceipt {
    /// Number of archived source rows removed across every selected whole batch.
    pub pruned_source_rows: usize,
    /// Exact content-addressed archive identities removed in durable order.
    pub archive_ids: Vec<String>,
    /// Whether the operation consumed its exact oversized-archive authorization.
    pub oversized_authorization_used: bool,
}

/// Safe result of selecting expired whole archives for permanent deletion.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RetentionArchivePruneOutcome {
    /// Every selected archive was deleted atomically.
    Pruned(RetentionArchivePruneReceipt),
    /// The oldest eligible archive is indivisible and exceeds the requested source-row bound.
    AuthorizationRequired {
        /// Exact content-addressed identity which must be supplied on retry.
        archive_id: String,
        /// Number of source rows retained by the indivisible archive.
        source_rows: usize,
        /// Bound which the archive exceeds.
        requested_max_records: usize,
    },
}

/// Result of one bounded archive-before-prune transaction.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RetentionReceipt {
    /// Record family examined.
    pub target: RetentionTarget,
    /// Source eligibility boundary used by the transaction.
    pub watermark_tick: u64,
    /// Active source rows examined.
    pub examined: usize,
    /// Source rows copied into the verified archive.
    pub archived: usize,
    /// Source rows deleted in the same transaction.
    pub pruned: usize,
    /// Examined rows retained because they were not eligible.
    pub protected: usize,
    /// Created archive identity, or `None` when no row was eligible.
    pub archive_id: Option<String>,
    /// New conservative age observations created by this operation.
    pub observations_created: usize,
    /// Existing exact age observations reused by this operation.
    pub observations_reused: usize,
    /// Stale or orphaned age observations removed by this operation.
    pub stale_observations_removed: usize,
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
/// Store-independent failure returned at the durable boundary.
pub enum StoreError {
    /// The requested record does not exist.
    NotFound,
    /// A record with the same unique identity already exists.
    AlreadyExists,
    /// An optimistic version, identity, or immutable value conflicts.
    Conflict,
    /// The requested lifecycle transition is not legal.
    InvalidTransition,
    /// An ordered record has a duplicate, zero, or discontinuous sequence.
    InvalidSequence,
    /// A configured row or byte quota would be exceeded.
    CapacityExceeded,
    /// A payload exceeds the configured per-record bound.
    PayloadTooLarge,
    /// A worker lease identity, epoch, or expiry fence does not match.
    LeaseConflict,
    /// A persisted schema cannot be migrated by this implementation.
    IncompatibleVersion,
    /// In-process synchronization was poisoned.
    Poisoned,
    /// The durable backend failed without a more specific safe category.
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

#[cfg(test)]
mod tests {
    use super::RetentionTarget;

    #[test]
    fn retention_target_inventory_is_unique_and_stably_named() {
        let expected = [
            "db2-replay",
            "ims-replay",
            "mq-replay",
            "dataset-replay",
            "cics-unit-of-work",
            "cics-replay",
            "racf-evidence",
            "cobol-lifecycle",
            "spool-jobs",
            "console-log",
            "resolved-effects",
            "delivered-outbox",
            "audit",
            "terminal-work",
            "lifecycle-events",
            "terminal-executions",
        ];
        assert_eq!(RetentionTarget::ALL.map(RetentionTarget::as_str), expected);
        let unique = RetentionTarget::ALL
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(unique.len(), RetentionTarget::ALL.len());
    }
}
