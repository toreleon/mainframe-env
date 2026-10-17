//! Exact forwarding inventory of all current PlatformStore ports.
//! Source: contracts/mainframe-env-store-api/src/traits.rs; no default-method fallback.
use super::*;

macro_rules! forward_store_port {
    ($port:ident { $( $(#[$attr:meta])* fn $method:ident(&self $(, $arg:ident: $ty:ty)* $(,)?) -> $result:ty;)* }) => {
        impl $port for CoordinatedStore {
            $( $(#[$attr])* fn $method(&self $(, $arg: $ty)*) -> $result {
                $port::$method(self.inner.as_ref() $(, $arg)*)
            })*
        }
    };
}

forward_store_port! { AuditSink {
    fn audit_subject_records( &self, _execution_id: &ExecutionId, _max: usize, ) -> Result<Vec<mainframe_env_execution_api::AuditSubjectRecord>, StoreError>;
    fn record_audit(&self, record: AuditRecord) -> Result<(), StoreError>;
    fn audit_records( &self, execution_id: &ExecutionId, start_effect_sequence: u64, max: usize, ) -> Result<Vec<AuditRecord>, StoreError>;
} }

impl ExecutionStore for CoordinatedStore {
    fn create_execution(&self, record: ExecutionRecord) -> Result<(), StoreError> {
        ExecutionStore::create_execution(self.inner.as_ref(), record)
    }
    fn get_execution(&self, id: &ExecutionId) -> Result<Option<ExecutionRecord>, StoreError> {
        ExecutionStore::get_execution(self.inner.as_ref(), id)
    }
    fn transition_execution(
        &self,
        id: &ExecutionId,
        expected_version: u64,
        next: ExecutionState,
        now_tick: u64,
    ) -> Result<ExecutionRecord, StoreError> {
        self.unobserved_lifecycle(|inner| {
            ExecutionStore::transition_execution(inner, id, expected_version, next, now_tick)
        })
    }
}

impl EventStore for CoordinatedStore {
    fn append_event(&self, event: LifecycleEvent) -> Result<(), StoreError> {
        self.unobserved_lifecycle(|inner| EventStore::append_event(inner, event))
    }
    fn events(
        &self,
        id: &ExecutionId,
        start_sequence: u64,
        max: usize,
    ) -> Result<Vec<LifecycleEvent>, StoreError> {
        EventStore::events(self.inner.as_ref(), id, start_sequence, max)
    }
}

forward_store_port! { WorkStore {
    fn get_work(&self, work_id: &str) -> Result<Option<WorkRecord>, StoreError>;
    fn enqueue(&self, work: WorkRecord) -> Result<(), StoreError>;
    fn claim( &self, worker: &str, required_generation: Option<&str>, now_tick: u64, lease_ticks: u64, ) -> Result<Option<WorkRecord>, StoreError>;
    fn heartbeat( &self, work_id: &str, lease_id: &str, lease_epoch: u64, now_tick: u64, lease_ticks: u64, ) -> Result<WorkRecord, StoreError>;
    fn release( &self, work_id: &str, lease_id: &str, lease_epoch: u64, now_tick: u64, available_tick: u64, ) -> Result<WorkRecord, StoreError>;
    fn request_cancellation(&self, work_id: &str) -> Result<WorkRecord, StoreError>;
    fn dead_letter( &self, work_id: &str, lease_id: &str, lease_epoch: u64, now_tick: u64, ) -> Result<WorkRecord, StoreError>;
    fn complete( &self, work_id: &str, lease_id: &str, lease_epoch: u64, now_tick: u64, ) -> Result<(), StoreError>;
} }

forward_store_port! { OutboxStore {
    fn append_notification(&self, record: OutboxRecord) -> Result<(), StoreError>;
    fn pending_notifications(&self, max: usize) -> Result<Vec<OutboxRecord>, StoreError>;
    fn mark_notification_delivered( &self, notification_id: &str, expected_version: u64, delivered_tick: u64, ) -> Result<OutboxRecord, StoreError>;
} }

forward_store_port! { RetentionStore {
    fn retention_capacity_health( &self, _policy: RetentionPolicy, ) -> Result<mainframe_env_store_api::RetentionCapacityHealth, StoreError>;
    fn retention_forecast( &self, target: RetentionTarget, policy: RetentionPolicy, now_tick: u64, observed_growth_per_tick: u64, ) -> Result<RetentionForecast, StoreError>;
    fn retention_forecast_with_dependencies( &self, _target: RetentionTarget, _policy: RetentionPolicy, _now_tick: u64, _observed_growth_per_tick: u64, _dependencies: &mainframe_env_store_api::CoreRetentionDependencySnapshot, ) -> Result<RetentionForecast, StoreError>;
    #[allow(clippy::too_many_arguments, reason = "exact forwarding of the existing store port")]
    fn provider_validated_retention_forecast( &self, _target: RetentionTarget, _policy: RetentionPolicy, _now_tick: u64, _observed_growth_per_tick: u64, _active_records: usize, _eligible_records: usize, _source_capacity: usize, ) -> Result<RetentionForecast, StoreError>;
    fn archive_and_prune( &self, policy: RetentionPolicy, request: RetentionRequest, ) -> Result<RetentionReceipt, StoreError>;
    fn archive_and_prune_with_dependencies( &self, _policy: RetentionPolicy, _request: RetentionRequest, _dependencies: &mainframe_env_store_api::CoreRetentionDependencySnapshot, ) -> Result<RetentionReceipt, StoreError>;
    fn retention_archives( &self, target: RetentionTarget, max: usize, ) -> Result<Vec<RetentionArchive>, StoreError>;
    fn prune_retention_archives( &self, policy: RetentionPolicy, now_tick: u64, max: usize, ) -> Result<usize, StoreError>;
    fn prune_retention_archives_authorized( &self, _policy: RetentionPolicy, _request: mainframe_env_store_api::RetentionArchivePruneRequest, ) -> Result<mainframe_env_store_api::RetentionArchivePruneOutcome, StoreError>;
    fn reconcile_retention_age( &self, request: RetentionAgeReconciliation, now_tick: u64, ) -> Result<RetentionReconciliationReceipt, StoreError>;
    fn retention_legacy_rows( &self, target: RetentionTarget, max: usize, ) -> Result<Vec<RetentionLegacyRow>, StoreError>;
} }

impl JournalStore for CoordinatedStore {
    fn commit_checked_replay_refusal(
        &self,
        _request: mainframe_env_store_api::CheckedReplayRefusalStep,
    ) -> Result<ExecutionRecord, StoreError> {
        self.unobserved_lifecycle(|inner| {
            JournalStore::commit_checked_replay_refusal(inner, _request)
        })
    }
    fn mutate_root_preparation_states(
        &self,
        _request: mainframe_env_store_api::RootPreparationPublication,
    ) -> Result<(), StoreError> {
        JournalStore::mutate_root_preparation_states(self.inner.as_ref(), _request)
    }
    fn mutate_root_provider_states(
        &self,
        _request: mainframe_env_store_api::RootProviderPublication,
    ) -> Result<(), StoreError> {
        JournalStore::mutate_root_provider_states(self.inner.as_ref(), _request)
    }
    fn admit_root_driver(
        &self,
        _admission: mainframe_env_store_api::RootDriverAdmission,
    ) -> Result<mainframe_env_store_api::RootDriverClaim, StoreError> {
        JournalStore::admit_root_driver(self.inner.as_ref(), _admission)
    }
    fn admit_root_child(
        &self,
        _admission: mainframe_env_store_api::RootChildAdmission,
    ) -> Result<(), StoreError> {
        JournalStore::admit_root_child(self.inner.as_ref(), _admission)
    }
    fn register_root_provider_row(
        &self,
        _admission: mainframe_env_store_api::RootProviderRowAdmission,
    ) -> Result<(), StoreError> {
        JournalStore::register_root_provider_row(self.inner.as_ref(), _admission)
    }
    fn fence_root_driver(
        &self,
        _claim: &mainframe_env_store_api::RootDriverClaim,
        _execution: &ExecutionRecord,
        _observed_tick: u64,
    ) -> Result<ProviderStateRecord, StoreError> {
        JournalStore::fence_root_driver(self.inner.as_ref(), _claim, _execution, _observed_tick)
    }
    fn close_root_driver(
        &self,
        _claim: &mainframe_env_store_api::RootDriverClaim,
        _execution: &ExecutionRecord,
        _observed_tick: u64,
    ) -> Result<mainframe_env_store_api::RootClosureSnapshot, StoreError> {
        JournalStore::close_root_driver(self.inner.as_ref(), _claim, _execution, _observed_tick)
    }
    fn commit_root_terminal_step(
        &self,
        _request: mainframe_env_store_api::RootTerminalPublication,
    ) -> Result<mainframe_env_store_api::RootTerminalCommit, StoreError> {
        self.unobserved_lifecycle(|inner| JournalStore::commit_root_terminal_step(inner, _request))
    }
    fn admit_execution(
        &self,
        execution: ExecutionRecord,
        event: LifecycleEvent,
        notification: OutboxRecord,
    ) -> Result<(), StoreError> {
        JournalStore::admit_execution(self.inner.as_ref(), execution, event, notification)
    }
    #[allow(
        clippy::too_many_arguments,
        reason = "exact forwarding of the existing journal port"
    )]
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
    ) -> Result<ExecutionRecord, StoreError> {
        self.observe_commit(
            execution_id,
            expected_version,
            next_state,
            event,
            effect,
            audit,
            checkpoint,
            notification,
        )
    }
}

forward_store_port! { CheckpointStore {
    fn put_checkpoint(&self, record: CheckpointRecord) -> Result<(), StoreError>;
    fn get_checkpoint(&self, id: &ExecutionId) -> Result<Option<CheckpointRecord>, StoreError>;
    fn delete_checkpoint(&self, id: &ExecutionId) -> Result<(), StoreError>;
} }

forward_store_port! { SessionStore {
    fn put_session( &self, record: SessionRecord, expected_version: Option<u64>, ) -> Result<(), StoreError>;
    fn get_session(&self, id: &str) -> Result<Option<SessionRecord>, StoreError>;
} }

forward_store_port! { ArtifactStore {
    fn health(&self) -> Result<ArtifactStoreHealth, StoreError>;
    fn put_artifact(&self, record: ArtifactRecord) -> Result<(), StoreError>;
    fn get_artifact(&self, id: &ArtifactRef) -> Result<Option<ArtifactRecord>, StoreError>;
    fn delete_artifact(&self, id: &ArtifactRef) -> Result<(), StoreError>;
} }

forward_store_port! { GenerationStore {
    fn publish_generation(&self, record: GenerationRecord) -> Result<(), StoreError>;
    fn generation(&self, provider: &str) -> Result<Option<GenerationRecord>, StoreError>;
} }

forward_store_port! { IdempotencyStore {
    fn record_intent(&self, record: EffectRecord) -> Result<(), StoreError>;
    fn record_result(&self, key: &IdempotencyKey, record: EffectRecord) -> Result<(), StoreError>;
    fn effect(&self, key: &IdempotencyKey) -> Result<Option<EffectRecord>, StoreError>;
    fn unknown_effects(&self, max: usize) -> Result<Vec<EffectRecord>, StoreError>;
    fn unresolved_effects(&self, _max: usize) -> Result<Vec<EffectRecord>, StoreError>;
    fn stale_intents( &self, now_tick: u64, minimum_age_ticks: u64, max: usize, ) -> Result<Vec<EffectRecord>, StoreError>;
    fn claim_stale_intent( &self, key: &IdempotencyKey, expected_intent_epoch: u64, recovery_owner: &str, now_tick: u64, minimum_age_ticks: u64, lease_ticks: u64, ) -> Result<EffectRecord, StoreError>;
    #[allow(clippy::too_many_arguments, reason = "exact forwarding of the existing store port")]
    fn reconcile_stale_intent( &self, key: &IdempotencyKey, recovery_owner: &str, recovery_epoch: u64, now_tick: u64, final_state: mainframe_env_store_api::EffectState, format: mainframe_env_store_api::EffectDigestFormat, result_digest: [u8; 32], ) -> Result<EffectRecord, StoreError>;
    fn reconcile_unknown( &self, key: &IdempotencyKey, final_state: mainframe_env_store_api::EffectState, result_digest: [u8; 32], ) -> Result<EffectRecord, StoreError>;
    fn reconcile_unknown_versioned( &self, key: &IdempotencyKey, final_state: mainframe_env_store_api::EffectState, format: mainframe_env_store_api::EffectDigestFormat, result_digest: [u8; 32], ) -> Result<EffectRecord, StoreError>;
} }

forward_store_port! { ProviderStateStore {
    fn publish_provider_read_audited( &self, _request: mainframe_env_store_api::CheckedProviderReadPublication, ) -> Result<(), StoreError>;
    fn assert_provider_replay( &self, _request: mainframe_env_store_api::ProviderReplayAssertion, ) -> Result<(), StoreError>;
    fn publish_provider_states_audited( &self, _request: mainframe_env_store_api::AuditedProviderPublication, ) -> Result<(), StoreError>;
    fn advance_logical_clock(&self, _observed_floor: u64) -> Result<u64, StoreError>;
    fn get_provider_state( &self, namespace: &str, key: &str, ) -> Result<Option<ProviderStateRecord>, StoreError>;
    fn list_provider_state( &self, namespace: &str, max: usize, ) -> Result<Vec<ProviderStateRecord>, StoreError>;
    fn list_provider_state_bounded( &self, _namespace: &str, _max: usize, _max_bytes: usize, ) -> Result<Vec<ProviderStateRecord>, StoreError>;
    fn list_provider_state_prefix( &self, _namespace_prefix: &str, _max: usize, ) -> Result<Vec<ProviderStateRecord>, StoreError>;
    fn put_provider_state( &self, record: ProviderStateRecord, expected_version: Option<u64>, ) -> Result<(), StoreError>;
    fn delete_provider_state( &self, namespace: &str, key: &str, expected_version: u64, ) -> Result<(), StoreError>;
    fn move_provider_state( &self, record: ProviderStateRecord, old_key: &str, expected_version: u64, ) -> Result<(), StoreError>;
    fn put_provider_states_atomic(&self, writes: Vec<ProviderStateWrite>) -> Result<(), StoreError>;
    fn mutate_provider_states_atomic( &self, mutations: Vec<ProviderStateMutation>, ) -> Result<(), StoreError>;
    fn archive_provider_state_replacement( &self, _request: ProviderStateArchiveReplacement, ) -> Result<RetentionArchive, StoreError>;
    fn provider_retention_archive_usage( &self, _target: RetentionTarget, ) -> Result<(usize, u64), StoreError>;
    fn provider_retention_authority_usage( &self, _target: RetentionTarget, ) -> Result<mainframe_env_store_api::RetentionAuthorityUsage, StoreError>;
    fn provider_state_retention_epoch(&self) -> Result<u64, StoreError>;
    fn provider_retention_observations( &self, _target: RetentionTarget, _max: usize, ) -> Result<Vec<mainframe_env_store_api::RetentionObservation>, StoreError>;
    fn provider_retention_observation_page( &self, _target: RetentionTarget, _after: Option<&mainframe_env_store_api::ProviderStateIdentity>, _max: usize, ) -> Result<Vec<(u64, mainframe_env_store_api::RetentionObservation)>, StoreError>;
    fn delete_provider_retention_observation( &self, _request: mainframe_env_store_api::ProviderRetentionObservationDeletion, ) -> Result<(), StoreError>;
    fn record_provider_retention_observation( &self, _source: ProviderStateRecord, _expected_epoch: u64, _observation: mainframe_env_store_api::RetentionObservation, ) -> Result<RetentionReconciliationReceipt, StoreError>;
    fn archive_provider_state_deletion( &self, _request: ProviderStateArchiveDeletion, ) -> Result<RetentionArchive, StoreError>;
    fn archive_provider_state_deletion_with_capacity( &self, _request: ProviderStateArchiveDeletionWithCapacity, ) -> Result<RetentionArchive, StoreError>;
} }
