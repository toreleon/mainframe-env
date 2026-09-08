use crate::{
    ArtifactRecord, ArtifactStoreHealth, CheckpointRecord, EffectRecord, ExecutionRecord,
    ExecutionState, GenerationRecord, OutboxRecord, ProviderStateArchiveDeletion,
    ProviderStateArchiveReplacement, ProviderStateMutation, ProviderStateRecord,
    ProviderStateWrite, RetentionAgeReconciliation, RetentionArchive, RetentionForecast,
    RetentionLegacyRow, RetentionPolicy, RetentionReceipt, RetentionReconciliationReceipt,
    RetentionRequest, RetentionTarget, SessionRecord, StoreError, WorkRecord,
};
use mainframe_env_execution_api::{
    ArtifactRef, AuditRecord, ExecutionId, IdempotencyKey, LifecycleEvent,
};

/// Mandatory persistence boundary for typed security-relevant host decisions.
pub trait AuditSink: Send + Sync {
    fn record_audit(&self, record: AuditRecord) -> Result<(), StoreError>;
    fn audit_records(
        &self,
        execution_id: &ExecutionId,
        start_effect_sequence: u64,
        max: usize,
    ) -> Result<Vec<AuditRecord>, StoreError>;
}

pub trait ExecutionStore: Send + Sync {
    fn create_execution(&self, record: ExecutionRecord) -> Result<(), StoreError>;
    fn get_execution(&self, id: &ExecutionId) -> Result<Option<ExecutionRecord>, StoreError>;
    fn transition_execution(
        &self,
        id: &ExecutionId,
        expected_version: u64,
        next: ExecutionState,
        now_tick: u64,
    ) -> Result<ExecutionRecord, StoreError>;
}

pub trait EventStore: Send + Sync {
    fn append_event(&self, event: LifecycleEvent) -> Result<(), StoreError>;
    fn events(
        &self,
        id: &ExecutionId,
        start_sequence: u64,
        max: usize,
    ) -> Result<Vec<LifecycleEvent>, StoreError>;
}

pub trait WorkStore: Send + Sync {
    /// Observe durable cancellation without claiming, leasing, or mutating work.
    fn get_work(&self, work_id: &str) -> Result<Option<WorkRecord>, StoreError>;
    fn enqueue(&self, work: WorkRecord) -> Result<(), StoreError>;
    /// Claim the highest-priority, oldest available work, optionally restricted
    /// to one required generation. `None` is the compatibility form for a
    /// caller that owns the entire durable work namespace.
    fn claim(
        &self,
        worker: &str,
        required_generation: Option<&str>,
        now_tick: u64,
        lease_ticks: u64,
    ) -> Result<Option<WorkRecord>, StoreError>;
    fn heartbeat(
        &self,
        work_id: &str,
        lease_id: &str,
        lease_epoch: u64,
        now_tick: u64,
        lease_ticks: u64,
    ) -> Result<WorkRecord, StoreError>;
    fn release(
        &self,
        work_id: &str,
        lease_id: &str,
        lease_epoch: u64,
        now_tick: u64,
        available_tick: u64,
    ) -> Result<WorkRecord, StoreError>;
    fn request_cancellation(&self, work_id: &str) -> Result<WorkRecord, StoreError>;
    fn dead_letter(
        &self,
        work_id: &str,
        lease_id: &str,
        lease_epoch: u64,
        now_tick: u64,
    ) -> Result<WorkRecord, StoreError>;
    fn complete(
        &self,
        work_id: &str,
        lease_id: &str,
        lease_epoch: u64,
        now_tick: u64,
    ) -> Result<(), StoreError>;
}

pub trait OutboxStore: Send + Sync {
    fn append_notification(&self, record: OutboxRecord) -> Result<(), StoreError>;
    fn pending_notifications(&self, max: usize) -> Result<Vec<OutboxRecord>, StoreError>;
    fn mark_notification_delivered(
        &self,
        notification_id: &str,
        expected_version: u64,
        delivered_tick: u64,
    ) -> Result<OutboxRecord, StoreError>;
}

/// Forecasting and atomic archive-before-prune operations for bounded durable state.
pub trait RetentionStore: Send + Sync {
    /// Prove writable authority and read bounded capacity counters without payload-codec scans.
    ///
    /// Durable backends exercise provider-state create, update, and delete authority inside a
    /// rolled-back transaction. Calling this method must not consume provider-state quota or
    /// advance the provider mutation epoch.
    fn retention_capacity_health(
        &self,
        _policy: RetentionPolicy,
    ) -> Result<crate::RetentionCapacityHealth, StoreError> {
        Err(StoreError::InvalidTransition)
    }
    /// Measure one target's eligibility, headroom, and saturation trajectory.
    fn retention_forecast(
        &self,
        target: RetentionTarget,
        policy: RetentionPolicy,
        now_tick: u64,
        observed_growth_per_tick: u64,
    ) -> Result<RetentionForecast, StoreError>;
    /// Forecast a core family using an exhaustive provider-owned dependency snapshot.
    fn retention_forecast_with_dependencies(
        &self,
        _target: RetentionTarget,
        _policy: RetentionPolicy,
        _now_tick: u64,
        _observed_growth_per_tick: u64,
        _dependencies: &crate::CoreRetentionDependencySnapshot,
    ) -> Result<RetentionForecast, StoreError> {
        Err(StoreError::InvalidTransition)
    }
    /// Forecast a provider-owned target after its full codec validated the counts.
    #[allow(clippy::too_many_arguments)]
    fn provider_validated_retention_forecast(
        &self,
        _target: RetentionTarget,
        _policy: RetentionPolicy,
        _now_tick: u64,
        _observed_growth_per_tick: u64,
        _active_records: usize,
        _eligible_records: usize,
        _source_capacity: usize,
    ) -> Result<RetentionForecast, StoreError> {
        Err(StoreError::InvalidTransition)
    }
    /// Copy eligible source rows into one verified archive and delete them atomically.
    fn archive_and_prune(
        &self,
        policy: RetentionPolicy,
        request: RetentionRequest,
    ) -> Result<RetentionReceipt, StoreError>;
    /// Archive a core family while atomically fencing provider-owned dependencies.
    fn archive_and_prune_with_dependencies(
        &self,
        _policy: RetentionPolicy,
        _request: RetentionRequest,
        _dependencies: &crate::CoreRetentionDependencySnapshot,
    ) -> Result<RetentionReceipt, StoreError> {
        Err(StoreError::InvalidTransition)
    }
    /// Read whole verified archives containing at most `max` source rows in total.
    fn retention_archives(
        &self,
        target: RetentionTarget,
        max: usize,
    ) -> Result<Vec<RetentionArchive>, StoreError>;
    /// Permanently delete whole archive batches without authorizing a bound override.
    ///
    /// An indivisible historical archive larger than `max` is retained and reported as an
    /// invalid transition. Operator surfaces should use
    /// [`Self::prune_retention_archives_authorized`] to obtain its exact identity.
    fn prune_retention_archives(
        &self,
        policy: RetentionPolicy,
        now_tick: u64,
        max: usize,
    ) -> Result<usize, StoreError> {
        match self.prune_retention_archives_authorized(
            policy,
            crate::RetentionArchivePruneRequest {
                now_tick,
                max_records: max,
                authorized_oversized_archive_id: None,
            },
        )? {
            crate::RetentionArchivePruneOutcome::Pruned(receipt) => Ok(receipt.pruned_source_rows),
            crate::RetentionArchivePruneOutcome::AuthorizationRequired { .. } => {
                Err(StoreError::InvalidTransition)
            }
        }
    }
    /// Permanently delete expired whole archives with an optional exact oversized authorization.
    fn prune_retention_archives_authorized(
        &self,
        _policy: RetentionPolicy,
        _request: crate::RetentionArchivePruneRequest,
    ) -> Result<crate::RetentionArchivePruneOutcome, StoreError> {
        Err(StoreError::InvalidTransition)
    }
    /// Add conservative owner/age metadata to one inspected legacy row under CAS.
    fn reconcile_retention_age(
        &self,
        request: RetentionAgeReconciliation,
        now_tick: u64,
    ) -> Result<RetentionReconciliationReceipt, StoreError>;
    /// List protected legacy rows with the CAS token needed for explicit reconciliation.
    fn retention_legacy_rows(
        &self,
        target: RetentionTarget,
        max: usize,
    ) -> Result<Vec<RetentionLegacyRow>, StoreError>;
}

pub trait JournalStore: Send + Sync {
    fn admit_execution(
        &self,
        execution: ExecutionRecord,
        event: LifecycleEvent,
        notification: OutboxRecord,
    ) -> Result<(), StoreError>;
    #[allow(clippy::too_many_arguments)]
    fn commit_execution_step(
        &self,
        execution_id: &ExecutionId,
        expected_version: u64,
        next_state: Option<ExecutionState>,
        event: LifecycleEvent,
        effect: Option<EffectRecord>,
        audit: Option<AuditRecord>,
        checkpoint: Option<CheckpointRecord>,
        notification: OutboxRecord,
    ) -> Result<ExecutionRecord, StoreError>;
}

pub trait CheckpointStore: Send + Sync {
    fn put_checkpoint(&self, record: CheckpointRecord) -> Result<(), StoreError>;
    fn get_checkpoint(&self, id: &ExecutionId) -> Result<Option<CheckpointRecord>, StoreError>;
    fn delete_checkpoint(&self, id: &ExecutionId) -> Result<(), StoreError>;
}

pub trait SessionStore: Send + Sync {
    fn put_session(
        &self,
        record: SessionRecord,
        expected_version: Option<u64>,
    ) -> Result<(), StoreError>;
    fn get_session(&self, id: &str) -> Result<Option<SessionRecord>, StoreError>;
}

pub trait ArtifactStore: Send + Sync {
    /// Prove readable and writable access and report every enforced capacity dimension.
    ///
    /// Implementations that cannot make those guarantees fail closed until
    /// they provide a backend-specific probe.
    fn health(&self) -> Result<ArtifactStoreHealth, StoreError> {
        Err(StoreError::IncompatibleVersion)
    }
    fn put_artifact(&self, record: ArtifactRecord) -> Result<(), StoreError>;
    fn get_artifact(&self, id: &ArtifactRef) -> Result<Option<ArtifactRecord>, StoreError>;
    fn delete_artifact(&self, id: &ArtifactRef) -> Result<(), StoreError>;
}

pub trait GenerationStore: Send + Sync {
    fn publish_generation(&self, record: GenerationRecord) -> Result<(), StoreError>;
    fn generation(&self, provider: &str) -> Result<Option<GenerationRecord>, StoreError>;
}

pub trait IdempotencyStore: Send + Sync {
    fn record_intent(&self, record: EffectRecord) -> Result<(), StoreError>;
    fn record_result(&self, key: &IdempotencyKey, record: EffectRecord) -> Result<(), StoreError>;
    fn effect(&self, key: &IdempotencyKey) -> Result<Option<EffectRecord>, StoreError>;
    fn unknown_effects(&self, max: usize) -> Result<Vec<EffectRecord>, StoreError>;
    /// Enumerate all intent or unknown-outcome effects that still require recovery.
    fn unresolved_effects(&self, _max: usize) -> Result<Vec<EffectRecord>, StoreError> {
        Err(StoreError::InvalidTransition)
    }
    /// Enumerate intents old enough for recovery whose recovery lease is absent or expired.
    fn stale_intents(
        &self,
        now_tick: u64,
        minimum_age_ticks: u64,
        max: usize,
    ) -> Result<Vec<EffectRecord>, StoreError>;
    /// Claim one stale intent under a monotonically increasing recovery epoch.
    fn claim_stale_intent(
        &self,
        key: &IdempotencyKey,
        expected_intent_epoch: u64,
        recovery_owner: &str,
        now_tick: u64,
        minimum_age_ticks: u64,
        lease_ticks: u64,
    ) -> Result<EffectRecord, StoreError>;
    /// Resolve a claimed intent without redispatching the original mutation.
    #[allow(clippy::too_many_arguments)]
    fn reconcile_stale_intent(
        &self,
        key: &IdempotencyKey,
        recovery_owner: &str,
        recovery_epoch: u64,
        now_tick: u64,
        final_state: crate::EffectState,
        format: crate::EffectDigestFormat,
        result_digest: [u8; 32],
    ) -> Result<EffectRecord, StoreError>;
    fn reconcile_unknown(
        &self,
        key: &IdempotencyKey,
        final_state: crate::EffectState,
        result_digest: [u8; 32],
    ) -> Result<EffectRecord, StoreError>;
    /// Reconcile only within the explicitly observed encoding domain.
    /// Stored identities and domains are immutable across result transitions.
    fn reconcile_unknown_versioned(
        &self,
        key: &IdempotencyKey,
        final_state: crate::EffectState,
        format: crate::EffectDigestFormat,
        result_digest: [u8; 32],
    ) -> Result<EffectRecord, StoreError> {
        let current = self.effect(key)?.ok_or(StoreError::NotFound)?;
        if current.digest_format != format {
            return Err(StoreError::Conflict);
        }
        self.reconcile_unknown(key, final_state, result_digest)
    }
}

pub trait ProviderStateStore: AuditSink + Send + Sync {
    /// Return a positive durable tick no lower than `observed_floor` or any
    /// previously observed floor.
    ///
    /// Equal observations may return the same tick: logical time measures elapsed
    /// duration and must not advance merely because request volume increases. Clock
    /// advancement must not consume provider-state row capacity or change the
    /// provider mutation epoch used by retention scans.
    fn advance_logical_clock(&self, _observed_floor: u64) -> Result<u64, StoreError> {
        Err(StoreError::InvalidTransition)
    }
    fn get_provider_state(
        &self,
        namespace: &str,
        key: &str,
    ) -> Result<Option<ProviderStateRecord>, StoreError>;
    fn list_provider_state(
        &self,
        namespace: &str,
        max: usize,
    ) -> Result<Vec<ProviderStateRecord>, StoreError>;
    /// List a bounded, key-ordered snapshot across provider namespaces sharing a prefix.
    fn list_provider_state_prefix(
        &self,
        _namespace_prefix: &str,
        _max: usize,
    ) -> Result<Vec<ProviderStateRecord>, StoreError> {
        Err(StoreError::InvalidTransition)
    }
    fn put_provider_state(
        &self,
        record: ProviderStateRecord,
        expected_version: Option<u64>,
    ) -> Result<(), StoreError>;
    fn delete_provider_state(
        &self,
        namespace: &str,
        key: &str,
        expected_version: u64,
    ) -> Result<(), StoreError>;
    fn move_provider_state(
        &self,
        record: ProviderStateRecord,
        old_key: &str,
        expected_version: u64,
    ) -> Result<(), StoreError>;
    fn put_provider_states_atomic(&self, writes: Vec<ProviderStateWrite>)
    -> Result<(), StoreError>;
    fn mutate_provider_states_atomic(
        &self,
        mutations: Vec<ProviderStateMutation>,
    ) -> Result<(), StoreError>;
    /// Atomically archive extracted subrecords and CAS-replace their aggregate provider row.
    fn archive_provider_state_replacement(
        &self,
        _request: ProviderStateArchiveReplacement,
    ) -> Result<RetentionArchive, StoreError> {
        Err(StoreError::InvalidTransition)
    }
    /// Measure archived row and payload-byte usage for one provider-owned family.
    fn provider_retention_archive_usage(
        &self,
        _target: RetentionTarget,
    ) -> Result<(usize, u64), StoreError> {
        Err(StoreError::InvalidTransition)
    }
    /// Return target usage and actual shared archive/observation capacities.
    fn provider_retention_authority_usage(
        &self,
        _target: RetentionTarget,
    ) -> Result<crate::RetentionAuthorityUsage, StoreError> {
        Err(StoreError::InvalidTransition)
    }
    /// Read the provider-state mutation epoch used to fence provider-built retention plans.
    fn provider_state_retention_epoch(&self) -> Result<u64, StoreError> {
        Err(StoreError::InvalidTransition)
    }
    /// List bounded dedicated age observations for provider full-codec validation.
    fn provider_retention_observations(
        &self,
        _target: RetentionTarget,
        _max: usize,
    ) -> Result<Vec<crate::RetentionObservation>, StoreError> {
        Err(StoreError::InvalidTransition)
    }
    /// Read one bounded, stable-key page of provider retention observations.
    fn provider_retention_observation_page(
        &self,
        _target: RetentionTarget,
        _after: Option<&crate::ProviderStateIdentity>,
        _max: usize,
    ) -> Result<Vec<(u64, crate::RetentionObservation)>, StoreError> {
        Err(StoreError::InvalidTransition)
    }
    /// CAS-delete one stale provider observation after full-codec source comparison.
    fn delete_provider_retention_observation(
        &self,
        _request: crate::ProviderRetentionObservationDeletion,
    ) -> Result<(), StoreError> {
        Err(StoreError::InvalidTransition)
    }
    /// Attest one provider-validated logical row while CAS-fencing its exact aggregate source.
    fn record_provider_retention_observation(
        &self,
        _source: ProviderStateRecord,
        _expected_epoch: u64,
        _observation: crate::RetentionObservation,
    ) -> Result<RetentionReconciliationReceipt, StoreError> {
        Err(StoreError::InvalidTransition)
    }
    /// Atomically archive and delete provider-validated rows after rechecking generic dependencies.
    fn archive_provider_state_deletion(
        &self,
        _request: ProviderStateArchiveDeletion,
    ) -> Result<RetentionArchive, StoreError> {
        Err(StoreError::InvalidTransition)
    }
}

pub trait PlatformStore:
    ExecutionStore
    + EventStore
    + WorkStore
    + CheckpointStore
    + SessionStore
    + ArtifactStore
    + GenerationStore
    + IdempotencyStore
    + OutboxStore
    + JournalStore
    + AuditSink
    + ProviderStateStore
    + RetentionStore
    + Send
    + Sync
{
}

impl<T> PlatformStore for T where
    T: ExecutionStore
        + EventStore
        + WorkStore
        + CheckpointStore
        + SessionStore
        + ArtifactStore
        + GenerationStore
        + IdempotencyStore
        + OutboxStore
        + JournalStore
        + AuditSink
        + ProviderStateStore
        + RetentionStore
        + Send
        + Sync
{
}
