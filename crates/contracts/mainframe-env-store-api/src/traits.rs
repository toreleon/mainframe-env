use crate::{
    ArtifactRecord, ArtifactStoreHealth, CheckpointRecord, EffectRecord, ExecutionRecord,
    ExecutionState, GenerationRecord, OutboxRecord, ProviderStateArchiveDeletion,
    ProviderStateArchiveDeletionWithCapacity, ProviderStateArchiveReplacement,
    ProviderStateMutation, ProviderStateRecord, ProviderStateWrite, RetentionAgeReconciliation,
    RetentionArchive, RetentionForecast, RetentionLegacyRow, RetentionPolicy, RetentionReceipt,
    RetentionReconciliationReceipt, RetentionRequest, RetentionTarget, SessionRecord, StoreError,
    WorkRecord,
};
use mainframe_env_execution_api::{
    ArtifactRef, AuditRecord, ExecutionId, IdempotencyKey, LifecycleEvent,
};

/// Mandatory persistence boundary for typed security-relevant host decisions.
pub trait AuditSink: Send + Sync {
    /// Read additive effect/terminal audit subjects using the same stored ordering.
    /// Unsupported backends refuse rather than silently omit terminal decisions.
    fn audit_subject_records(
        &self,
        _execution_id: &ExecutionId,
        _max: usize,
    ) -> Result<Vec<mainframe_env_execution_api::AuditSubjectRecord>, StoreError> {
        Err(StoreError::InvalidTransition)
    }
    /// Persist one security-relevant authorization decision.
    fn record_audit(&self, record: AuditRecord) -> Result<(), StoreError>;
    /// Read a bounded ordered range of audit records for one execution.
    fn audit_records(
        &self,
        execution_id: &ExecutionId,
        start_effect_sequence: u64,
        max: usize,
    ) -> Result<Vec<AuditRecord>, StoreError>;
}

/// Optimistic lifecycle persistence for one execution identity.
pub trait ExecutionStore: Send + Sync {
    /// Insert a previously unseen admitted execution.
    fn create_execution(&self, record: ExecutionRecord) -> Result<(), StoreError>;
    /// Read the current execution record without changing it.
    fn get_execution(&self, id: &ExecutionId) -> Result<Option<ExecutionRecord>, StoreError>;
    /// Apply a legal state transition when `expected_version` still matches.
    ///
    /// A terminal transition records the supplied nonzero `now_tick` as the
    /// execution's durable retention age.
    fn transition_execution(
        &self,
        id: &ExecutionId,
        expected_version: u64,
        next: ExecutionState,
        now_tick: u64,
    ) -> Result<ExecutionRecord, StoreError>;
}

/// Ordered durable lifecycle-event storage.
pub trait EventStore: Send + Sync {
    /// Append a valid event without replacing an existing sequence.
    fn append_event(&self, event: LifecycleEvent) -> Result<(), StoreError>;
    /// Read at most `max` events at or after `start_sequence`.
    fn events(
        &self,
        id: &ExecutionId,
        start_sequence: u64,
        max: usize,
    ) -> Result<Vec<LifecycleEvent>, StoreError>;
}

/// Durable queue storage with expiring, fenced worker leases.
pub trait WorkStore: Send + Sync {
    /// Observe durable cancellation without claiming, leasing, or mutating work.
    fn get_work(&self, work_id: &str) -> Result<Option<WorkRecord>, StoreError>;
    /// Enqueue a previously unseen work identity.
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
    /// Extend a live lease held by the exact lease identity and epoch.
    fn heartbeat(
        &self,
        work_id: &str,
        lease_id: &str,
        lease_epoch: u64,
        now_tick: u64,
        lease_ticks: u64,
    ) -> Result<WorkRecord, StoreError>;
    /// Return leased work to the queue at a bounded future tick.
    fn release(
        &self,
        work_id: &str,
        lease_id: &str,
        lease_epoch: u64,
        now_tick: u64,
        available_tick: u64,
    ) -> Result<WorkRecord, StoreError>;
    /// Mark queued or claimed work for cancellation.
    fn request_cancellation(&self, work_id: &str) -> Result<WorkRecord, StoreError>;
    /// Move exhausted or invalid work to its terminal dead-letter state.
    fn dead_letter(
        &self,
        work_id: &str,
        lease_id: &str,
        lease_epoch: u64,
        now_tick: u64,
    ) -> Result<WorkRecord, StoreError>;
    /// Complete work only while the supplied lease fence remains live.
    fn complete(
        &self,
        work_id: &str,
        lease_id: &str,
        lease_epoch: u64,
        now_tick: u64,
    ) -> Result<(), StoreError>;
}

/// Durable delivery outbox storage.
pub trait OutboxStore: Send + Sync {
    /// Append a unique notification.
    fn append_notification(&self, record: OutboxRecord) -> Result<(), StoreError>;
    /// Return at most `max` undelivered notifications in stable order.
    fn pending_notifications(&self, max: usize) -> Result<Vec<OutboxRecord>, StoreError>;
    /// Mark one notification delivered under optimistic version control and
    /// persist the supplied nonzero `delivered_tick` as its retention age.
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

/// Atomic execution, event, effect, checkpoint, and outbox transactions.
pub trait JournalStore: Send + Sync {
    /// Publish only already registered initial-root preparation rows. The actual
    /// root must remain Admitted version1, Open, root-only and effect/work/checkpoint
    /// free inside the same physical transaction. Structural observations are not
    /// host permission. Unsupported adapters refuse without sequential fallback.
    fn mutate_root_preparation_states(
        &self,
        _request: crate::RootPreparationPublication,
    ) -> Result<(), StoreError> {
        Err(StoreError::InvalidTransition)
    }
    /// Mutate only already registered scopes of this exact Open root/actor/original
    /// canonical intent. All observations and both Move endpoints are checked inside
    /// one physical lock/transaction. No audit, intent completion or fallback is minted.
    /// Unsupported adapters refuse without writes; this is not host admission.
    fn mutate_root_provider_states(
        &self,
        _request: crate::RootProviderPublication,
    ) -> Result<(), StoreError> {
        Err(StoreError::InvalidTransition)
    }
    /// Admit one genuine configured root and its core ownership atomically.
    /// Structural data does not attest host admission; other backends refuse.
    fn admit_root_driver(
        &self,
        _admission: crate::RootDriverAdmission,
    ) -> Result<crate::RootDriverClaim, StoreError> {
        Err(StoreError::InvalidTransition)
    }
    /// Atomically enroll/admit the original compiled child under an Open root.
    fn admit_root_child(&self, _admission: crate::RootChildAdmission) -> Result<(), StoreError> {
        Err(StoreError::InvalidTransition)
    }
    /// Register the exact original CALL/lifecycle row under its existing core
    /// intent before mutation. It grants no dispatch or host admission permission.
    fn register_root_provider_row(
        &self,
        _admission: crate::RootProviderRowAdmission,
    ) -> Result<(), StoreError> {
        Err(StoreError::InvalidTransition)
    }
    /// Protect an uncertain original root without selecting a UOW or terminal
    /// decision. No EffectRecord, known completion, retry or recovery is minted.
    /// Default Unsupported behavior is explicit and has no sequential fallback.
    fn fence_root_driver(
        &self,
        _claim: &crate::RootDriverClaim,
        _execution: &ExecutionRecord,
        _observed_tick: u64,
    ) -> Result<ProviderStateRecord, StoreError> {
        Err(StoreError::InvalidTransition)
    }
    /// Gate new enrolled writes and capture one complete bounded Closing graph.
    /// This method decides no queue work and cannot settle an Unknown root.
    fn close_root_driver(
        &self,
        _claim: &crate::RootDriverClaim,
        _execution: &ExecutionRecord,
        _observed_tick: u64,
    ) -> Result<crate::RootClosureSnapshot, StoreError> {
        Err(StoreError::InvalidTransition)
    }
    /// Publish all core terminal/outbox/provider/audit changes under one lock/TX.
    /// There is no sequential fallback and no synthetic effect permission.
    fn commit_root_terminal_step(
        &self,
        _request: crate::RootTerminalPublication,
    ) -> Result<crate::RootTerminalCommit, StoreError> {
        Err(StoreError::InvalidTransition)
    }
    /// Atomically admit an execution with its first event and notification.
    fn admit_execution(
        &self,
        execution: ExecutionRecord,
        event: LifecycleEvent,
        notification: OutboxRecord,
    ) -> Result<(), StoreError>;
    /// Atomically commit one execution step and all associated durable records,
    /// including terminal lifecycle ages carried by the event and records.
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

/// Restorable execution-checkpoint storage.
pub trait CheckpointStore: Send + Sync {
    /// Insert or replace a validated checkpoint for its execution.
    fn put_checkpoint(&self, record: CheckpointRecord) -> Result<(), StoreError>;
    /// Read the current checkpoint for an execution.
    fn get_checkpoint(&self, id: &ExecutionId) -> Result<Option<CheckpointRecord>, StoreError>;
    /// Delete the current checkpoint, if present.
    fn delete_checkpoint(&self, id: &ExecutionId) -> Result<(), StoreError>;
}

/// Durable authenticated-session storage.
pub trait SessionStore: Send + Sync {
    /// Create or compare-and-swap a session record.
    fn put_session(
        &self,
        record: SessionRecord,
        expected_version: Option<u64>,
    ) -> Result<(), StoreError>;
    /// Read a session by its opaque storage identity.
    fn get_session(&self, id: &str) -> Result<Option<SessionRecord>, StoreError>;
}

/// Immutable content-addressed artifact storage.
pub trait ArtifactStore: Send + Sync {
    /// Prove readable and writable access and report every enforced capacity dimension.
    ///
    /// Implementations that cannot make those guarantees fail closed until
    /// they provide a backend-specific probe.
    fn health(&self) -> Result<ArtifactStoreHealth, StoreError> {
        Err(StoreError::IncompatibleVersion)
    }
    /// Publish an artifact or accept an identical prior publication.
    fn put_artifact(&self, record: ArtifactRecord) -> Result<(), StoreError>;
    /// Read and validate an artifact by content identity.
    fn get_artifact(&self, id: &ArtifactRef) -> Result<Option<ArtifactRecord>, StoreError>;
    /// Remove an artifact under an operator-controlled lifecycle.
    fn delete_artifact(&self, id: &ArtifactRef) -> Result<(), StoreError>;
}

/// Active provider-generation metadata storage.
pub trait GenerationStore: Send + Sync {
    /// Publish a provider generation under optimistic version control.
    fn publish_generation(&self, record: GenerationRecord) -> Result<(), StoreError>;
    /// Read the currently published generation for a provider.
    fn generation(&self, provider: &str) -> Result<Option<GenerationRecord>, StoreError>;
}

/// Durable idempotency intents and terminal effect receipts.
pub trait IdempotencyStore: Send + Sync {
    /// Record a unique pre-dispatch effect intent.
    fn record_intent(&self, record: EffectRecord) -> Result<(), StoreError>;
    /// Replace an exact intent with its validated post-dispatch result.
    fn record_result(&self, key: &IdempotencyKey, record: EffectRecord) -> Result<(), StoreError>;
    /// Read the current effect record for an idempotency key.
    fn effect(&self, key: &IdempotencyKey) -> Result<Option<EffectRecord>, StoreError>;
    /// Enumerate a bounded set of explicitly uncertain effects.
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
    /// Resolve one uncertain effect using its retained digest domain.
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

/// Versioned per-object provider state and atomic mutation batches.
pub trait ProviderStateStore: AuditSink + Send + Sync {
    /// Assert exact provider reads, full current execution and original Intent
    /// under one physical lock/transaction, then insert one receipt and its audit.
    /// Audit-only Deny is allowed; exact dependencies are never mutated.
    /// Unsupported adapters refuse without sequential read/write/audit fallback.
    fn publish_provider_read_audited(
        &self,
        _request: crate::CheckedProviderReadPublication,
    ) -> Result<(), StoreError> {
        Err(StoreError::InvalidTransition)
    }
    /// Assert current exact read/receipt/root/original occurrence observations
    /// without any row, audit, clock or epoch writes. Returns no dispatch permit.
    /// Completed uses its real completion metadata, never Intent publication.
    /// Unsupported adapters refuse; callers must not recompute or redispatch.
    fn assert_provider_replay(
        &self,
        _request: crate::ProviderReplayAssertion,
    ) -> Result<(), StoreError> {
        Err(StoreError::InvalidTransition)
    }
    /// Atomically assert an exact live canonical coordinator intent and publish
    /// bounded provider rows plus their typed audit. Core completion remains
    /// coordinator-owned. No transaction may span provider/SAF dispatch.
    ///
    /// Empty mutations permit an audit-only decision; denial cannot mutate rows.
    /// Unsupported adapters fail without a sequential rows/audit fallback.
    fn publish_provider_states_audited(
        &self,
        _request: crate::AuditedProviderPublication,
    ) -> Result<(), StoreError> {
        Err(StoreError::InvalidTransition)
    }
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
    /// Read one provider-owned object.
    fn get_provider_state(
        &self,
        namespace: &str,
        key: &str,
    ) -> Result<Option<ProviderStateRecord>, StoreError>;
    /// List a bounded, stable prefix of one provider namespace.
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
    /// Create or compare-and-swap one provider-owned object.
    fn put_provider_state(
        &self,
        record: ProviderStateRecord,
        expected_version: Option<u64>,
    ) -> Result<(), StoreError>;
    /// Delete one provider-owned object at an exact version.
    fn delete_provider_state(
        &self,
        namespace: &str,
        key: &str,
        expected_version: u64,
    ) -> Result<(), StoreError>;
    /// Atomically move an object between keys in the same namespace.
    fn move_provider_state(
        &self,
        record: ProviderStateRecord,
        old_key: &str,
        expected_version: u64,
    ) -> Result<(), StoreError>;
    /// Atomically apply a bounded batch of object writes.
    fn put_provider_states_atomic(&self, writes: Vec<ProviderStateWrite>)
    -> Result<(), StoreError>;
    /// Atomically apply mixed puts, deletes, and moves.
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
    /// Atomically archive private container replay rows and decrement their
    /// command-data capacity CAS row. Other targets and namespaces are refused.
    fn archive_provider_state_deletion_with_capacity(
        &self,
        _request: ProviderStateArchiveDeletionWithCapacity,
    ) -> Result<RetentionArchive, StoreError> {
        Err(StoreError::InvalidTransition)
    }
}

/// Complete storage authority required by the product coordinator.
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
