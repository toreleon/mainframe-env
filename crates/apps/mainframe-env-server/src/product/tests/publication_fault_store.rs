//! Test-only forwarding of existing store ports; no publication authority lives here.
//! Failure behavior follows FailDatasetCommitOnceStore and UnknownOutcomeStore patterns.
use mainframe_env_execution_api::{
    ArtifactRef, AuditRecord, ExecutionId, IdempotencyKey, LifecycleEvent,
};
use mainframe_env_store_api::*;
use std::sync::{Arc, Mutex};

#[derive(Clone, Copy, Debug)]
pub(super) enum Outcome {
    Before,
    After,
    ExitAfter,
}

#[derive(Clone, Debug)]
pub(super) struct Point {
    pub namespace: &'static str,
    pub field: Option<(&'static str, serde_json::Value)>,
    pub outcome: Outcome,
}

type ReadHook = dyn Fn(&str, &str) + Send + Sync;
type WriteHook = dyn Fn(&[ProviderStateRecord]) + Send + Sync;

pub(super) struct FaultStore<S> {
    pub inner: Arc<S>,
    point: Mutex<Option<Point>>,
    read_hook: Mutex<Option<Arc<ReadHook>>>,
    write_hook: Mutex<Option<Arc<WriteHook>>>,
    pub hits: Mutex<usize>,
}

impl<S: PlatformStore> FaultStore<S> {
    pub fn new(inner: Arc<S>) -> Self {
        Self {
            inner,
            point: Mutex::new(None),
            read_hook: Mutex::new(None),
            write_hook: Mutex::new(None),
            hits: Mutex::new(0),
        }
    }
    pub(super) fn on_read(&self, hook: impl Fn(&str, &str) + Send + Sync + 'static) {
        *self.read_hook.lock().unwrap() = Some(Arc::new(hook));
    }
    pub(super) fn on_write(&self, hook: impl Fn(&[ProviderStateRecord]) + Send + Sync + 'static) {
        *self.write_hook.lock().unwrap() = Some(Arc::new(hook));
    }
    pub fn arm(&self, point: Point) {
        assert!(self.point.lock().unwrap().replace(point).is_none());
    }
    pub fn armed(&self) -> bool {
        self.point.lock().unwrap().is_some()
    }
    fn take(&self, records: &[ProviderStateRecord]) -> Option<Outcome> {
        let mut slot = self.point.lock().unwrap();
        let point = slot.as_ref()?;
        let matches = records.iter().any(|record| {
            record.namespace == point.namespace
                && point.field.as_ref().is_none_or(|(field, expected)| {
                    serde_json::from_slice::<serde_json::Value>(&record.payload)
                        .is_ok_and(|value| &value[*field] == expected)
                })
        });
        if !matches {
            return None;
        }
        *self.hits.lock().unwrap() += 1;
        Some(slot.take().unwrap().outcome)
    }
    fn inject(
        &self,
        records: &[ProviderStateRecord],
        operation: impl FnOnce() -> Result<(), StoreError>,
    ) -> Result<(), StoreError> {
        let outcome = self.take(records);
        if matches!(outcome, Some(Outcome::Before)) {
            return Err(StoreError::Infrastructure(
                "publication test: before write".into(),
            ));
        }
        operation()?;
        let hook = self.write_hook.lock().unwrap().clone();
        if let Some(hook) = hook {
            hook(records);
        }
        match outcome {
            Some(Outcome::After) => Err(StoreError::Infrastructure(
                "publication test: durable write, unknown return".into(),
            )),
            Some(Outcome::ExitAfter) => std::process::exit(86),
            _ => Ok(()),
        }
    }
}

impl<S: PlatformStore> AuditSink for FaultStore<S> {
    fn audit_subject_records(
        &self,
        _execution_id: &ExecutionId,
        _max: usize,
    ) -> Result<Vec<mainframe_env_execution_api::AuditSubjectRecord>, StoreError> {
        mainframe_env_store_api::AuditSink::audit_subject_records(
            self.inner.as_ref(),
            _execution_id,
            _max,
        )
    }
    fn record_audit(&self, record: AuditRecord) -> Result<(), StoreError> {
        mainframe_env_store_api::AuditSink::record_audit(self.inner.as_ref(), record)
    }
    fn audit_records(
        &self,
        execution_id: &ExecutionId,
        start_effect_sequence: u64,
        max: usize,
    ) -> Result<Vec<AuditRecord>, StoreError> {
        mainframe_env_store_api::AuditSink::audit_records(
            self.inner.as_ref(),
            execution_id,
            start_effect_sequence,
            max,
        )
    }
}

impl<S: PlatformStore> ExecutionStore for FaultStore<S> {
    fn create_execution(&self, record: ExecutionRecord) -> Result<(), StoreError> {
        mainframe_env_store_api::ExecutionStore::create_execution(self.inner.as_ref(), record)
    }
    fn get_execution(&self, id: &ExecutionId) -> Result<Option<ExecutionRecord>, StoreError> {
        mainframe_env_store_api::ExecutionStore::get_execution(self.inner.as_ref(), id)
    }
    fn transition_execution(
        &self,
        id: &ExecutionId,
        expected_version: u64,
        next: ExecutionState,
        now_tick: u64,
    ) -> Result<ExecutionRecord, StoreError> {
        mainframe_env_store_api::ExecutionStore::transition_execution(
            self.inner.as_ref(),
            id,
            expected_version,
            next,
            now_tick,
        )
    }
}

impl<S: PlatformStore> EventStore for FaultStore<S> {
    fn append_event(&self, event: LifecycleEvent) -> Result<(), StoreError> {
        mainframe_env_store_api::EventStore::append_event(self.inner.as_ref(), event)
    }
    fn events(
        &self,
        id: &ExecutionId,
        start_sequence: u64,
        max: usize,
    ) -> Result<Vec<LifecycleEvent>, StoreError> {
        mainframe_env_store_api::EventStore::events(self.inner.as_ref(), id, start_sequence, max)
    }
}

impl<S: PlatformStore> WorkStore for FaultStore<S> {
    fn get_work(&self, work_id: &str) -> Result<Option<WorkRecord>, StoreError> {
        mainframe_env_store_api::WorkStore::get_work(self.inner.as_ref(), work_id)
    }
    fn enqueue(&self, work: WorkRecord) -> Result<(), StoreError> {
        mainframe_env_store_api::WorkStore::enqueue(self.inner.as_ref(), work)
    }
    fn claim(
        &self,
        worker: &str,
        required_generation: Option<&str>,
        now_tick: u64,
        lease_ticks: u64,
    ) -> Result<Option<WorkRecord>, StoreError> {
        mainframe_env_store_api::WorkStore::claim(
            self.inner.as_ref(),
            worker,
            required_generation,
            now_tick,
            lease_ticks,
        )
    }
    fn heartbeat(
        &self,
        work_id: &str,
        lease_id: &str,
        lease_epoch: u64,
        now_tick: u64,
        lease_ticks: u64,
    ) -> Result<WorkRecord, StoreError> {
        mainframe_env_store_api::WorkStore::heartbeat(
            self.inner.as_ref(),
            work_id,
            lease_id,
            lease_epoch,
            now_tick,
            lease_ticks,
        )
    }
    fn release(
        &self,
        work_id: &str,
        lease_id: &str,
        lease_epoch: u64,
        now_tick: u64,
        available_tick: u64,
    ) -> Result<WorkRecord, StoreError> {
        mainframe_env_store_api::WorkStore::release(
            self.inner.as_ref(),
            work_id,
            lease_id,
            lease_epoch,
            now_tick,
            available_tick,
        )
    }
    fn request_cancellation(&self, work_id: &str) -> Result<WorkRecord, StoreError> {
        mainframe_env_store_api::WorkStore::request_cancellation(self.inner.as_ref(), work_id)
    }
    fn dead_letter(
        &self,
        work_id: &str,
        lease_id: &str,
        lease_epoch: u64,
        now_tick: u64,
    ) -> Result<WorkRecord, StoreError> {
        mainframe_env_store_api::WorkStore::dead_letter(
            self.inner.as_ref(),
            work_id,
            lease_id,
            lease_epoch,
            now_tick,
        )
    }
    fn complete(
        &self,
        work_id: &str,
        lease_id: &str,
        lease_epoch: u64,
        now_tick: u64,
    ) -> Result<(), StoreError> {
        mainframe_env_store_api::WorkStore::complete(
            self.inner.as_ref(),
            work_id,
            lease_id,
            lease_epoch,
            now_tick,
        )
    }
}

impl<S: PlatformStore> OutboxStore for FaultStore<S> {
    fn append_notification(&self, record: OutboxRecord) -> Result<(), StoreError> {
        mainframe_env_store_api::OutboxStore::append_notification(self.inner.as_ref(), record)
    }
    fn pending_notifications(&self, max: usize) -> Result<Vec<OutboxRecord>, StoreError> {
        mainframe_env_store_api::OutboxStore::pending_notifications(self.inner.as_ref(), max)
    }
    fn mark_notification_delivered(
        &self,
        notification_id: &str,
        expected_version: u64,
        delivered_tick: u64,
    ) -> Result<OutboxRecord, StoreError> {
        mainframe_env_store_api::OutboxStore::mark_notification_delivered(
            self.inner.as_ref(),
            notification_id,
            expected_version,
            delivered_tick,
        )
    }
}

impl<S: PlatformStore> RetentionStore for FaultStore<S> {
    fn retention_capacity_health(
        &self,
        _policy: RetentionPolicy,
    ) -> Result<mainframe_env_store_api::RetentionCapacityHealth, StoreError> {
        mainframe_env_store_api::RetentionStore::retention_capacity_health(
            self.inner.as_ref(),
            _policy,
        )
    }
    fn retention_forecast(
        &self,
        target: RetentionTarget,
        policy: RetentionPolicy,
        now_tick: u64,
        observed_growth_per_tick: u64,
    ) -> Result<RetentionForecast, StoreError> {
        mainframe_env_store_api::RetentionStore::retention_forecast(
            self.inner.as_ref(),
            target,
            policy,
            now_tick,
            observed_growth_per_tick,
        )
    }
    fn retention_forecast_with_dependencies(
        &self,
        _target: RetentionTarget,
        _policy: RetentionPolicy,
        _now_tick: u64,
        _observed_growth_per_tick: u64,
        _dependencies: &mainframe_env_store_api::CoreRetentionDependencySnapshot,
    ) -> Result<RetentionForecast, StoreError> {
        mainframe_env_store_api::RetentionStore::retention_forecast_with_dependencies(
            self.inner.as_ref(),
            _target,
            _policy,
            _now_tick,
            _observed_growth_per_tick,
            _dependencies,
        )
    }
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
        mainframe_env_store_api::RetentionStore::provider_validated_retention_forecast(
            self.inner.as_ref(),
            _target,
            _policy,
            _now_tick,
            _observed_growth_per_tick,
            _active_records,
            _eligible_records,
            _source_capacity,
        )
    }
    fn archive_and_prune(
        &self,
        policy: RetentionPolicy,
        request: RetentionRequest,
    ) -> Result<RetentionReceipt, StoreError> {
        mainframe_env_store_api::RetentionStore::archive_and_prune(
            self.inner.as_ref(),
            policy,
            request,
        )
    }
    fn archive_and_prune_with_dependencies(
        &self,
        _policy: RetentionPolicy,
        _request: RetentionRequest,
        _dependencies: &mainframe_env_store_api::CoreRetentionDependencySnapshot,
    ) -> Result<RetentionReceipt, StoreError> {
        mainframe_env_store_api::RetentionStore::archive_and_prune_with_dependencies(
            self.inner.as_ref(),
            _policy,
            _request,
            _dependencies,
        )
    }
    fn retention_archives(
        &self,
        target: RetentionTarget,
        max: usize,
    ) -> Result<Vec<RetentionArchive>, StoreError> {
        mainframe_env_store_api::RetentionStore::retention_archives(
            self.inner.as_ref(),
            target,
            max,
        )
    }
    fn prune_retention_archives(
        &self,
        policy: RetentionPolicy,
        now_tick: u64,
        max: usize,
    ) -> Result<usize, StoreError> {
        mainframe_env_store_api::RetentionStore::prune_retention_archives(
            self.inner.as_ref(),
            policy,
            now_tick,
            max,
        )
    }
    fn prune_retention_archives_authorized(
        &self,
        _policy: RetentionPolicy,
        _request: mainframe_env_store_api::RetentionArchivePruneRequest,
    ) -> Result<mainframe_env_store_api::RetentionArchivePruneOutcome, StoreError> {
        mainframe_env_store_api::RetentionStore::prune_retention_archives_authorized(
            self.inner.as_ref(),
            _policy,
            _request,
        )
    }
    fn reconcile_retention_age(
        &self,
        request: RetentionAgeReconciliation,
        now_tick: u64,
    ) -> Result<RetentionReconciliationReceipt, StoreError> {
        mainframe_env_store_api::RetentionStore::reconcile_retention_age(
            self.inner.as_ref(),
            request,
            now_tick,
        )
    }
    fn retention_legacy_rows(
        &self,
        target: RetentionTarget,
        max: usize,
    ) -> Result<Vec<RetentionLegacyRow>, StoreError> {
        mainframe_env_store_api::RetentionStore::retention_legacy_rows(
            self.inner.as_ref(),
            target,
            max,
        )
    }
}

impl<S: PlatformStore> JournalStore for FaultStore<S> {
    fn commit_checked_replay_refusal(
        &self,
        _request: mainframe_env_store_api::CheckedReplayRefusalStep,
    ) -> Result<ExecutionRecord, StoreError> {
        mainframe_env_store_api::JournalStore::commit_checked_replay_refusal(
            self.inner.as_ref(),
            _request,
        )
    }
    fn mutate_root_preparation_states(
        &self,
        _request: mainframe_env_store_api::RootPreparationPublication,
    ) -> Result<(), StoreError> {
        mainframe_env_store_api::JournalStore::mutate_root_preparation_states(
            self.inner.as_ref(),
            _request,
        )
    }
    fn mutate_root_provider_states(
        &self,
        _request: mainframe_env_store_api::RootProviderPublication,
    ) -> Result<(), StoreError> {
        mainframe_env_store_api::JournalStore::mutate_root_provider_states(
            self.inner.as_ref(),
            _request,
        )
    }
    fn admit_root_driver(
        &self,
        _admission: mainframe_env_store_api::RootDriverAdmission,
    ) -> Result<mainframe_env_store_api::RootDriverClaim, StoreError> {
        mainframe_env_store_api::JournalStore::admit_root_driver(self.inner.as_ref(), _admission)
    }
    fn admit_root_child(
        &self,
        _admission: mainframe_env_store_api::RootChildAdmission,
    ) -> Result<(), StoreError> {
        mainframe_env_store_api::JournalStore::admit_root_child(self.inner.as_ref(), _admission)
    }
    fn register_root_provider_row(
        &self,
        _admission: mainframe_env_store_api::RootProviderRowAdmission,
    ) -> Result<(), StoreError> {
        mainframe_env_store_api::JournalStore::register_root_provider_row(
            self.inner.as_ref(),
            _admission,
        )
    }
    fn fence_root_driver(
        &self,
        _claim: &mainframe_env_store_api::RootDriverClaim,
        _execution: &ExecutionRecord,
        _observed_tick: u64,
    ) -> Result<ProviderStateRecord, StoreError> {
        mainframe_env_store_api::JournalStore::fence_root_driver(
            self.inner.as_ref(),
            _claim,
            _execution,
            _observed_tick,
        )
    }
    fn close_root_driver(
        &self,
        _claim: &mainframe_env_store_api::RootDriverClaim,
        _execution: &ExecutionRecord,
        _observed_tick: u64,
    ) -> Result<mainframe_env_store_api::RootClosureSnapshot, StoreError> {
        mainframe_env_store_api::JournalStore::close_root_driver(
            self.inner.as_ref(),
            _claim,
            _execution,
            _observed_tick,
        )
    }
    fn commit_root_terminal_step(
        &self,
        _request: mainframe_env_store_api::RootTerminalPublication,
    ) -> Result<mainframe_env_store_api::RootTerminalCommit, StoreError> {
        mainframe_env_store_api::JournalStore::commit_root_terminal_step(
            self.inner.as_ref(),
            _request,
        )
    }
    fn admit_execution(
        &self,
        execution: ExecutionRecord,
        event: LifecycleEvent,
        notification: OutboxRecord,
    ) -> Result<(), StoreError> {
        mainframe_env_store_api::JournalStore::admit_execution(
            self.inner.as_ref(),
            execution,
            event,
            notification,
        )
    }
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
        mainframe_env_store_api::JournalStore::commit_execution_step(
            self.inner.as_ref(),
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

impl<S: PlatformStore> CheckpointStore for FaultStore<S> {
    fn put_checkpoint(&self, record: CheckpointRecord) -> Result<(), StoreError> {
        mainframe_env_store_api::CheckpointStore::put_checkpoint(self.inner.as_ref(), record)
    }
    fn get_checkpoint(&self, id: &ExecutionId) -> Result<Option<CheckpointRecord>, StoreError> {
        mainframe_env_store_api::CheckpointStore::get_checkpoint(self.inner.as_ref(), id)
    }
    fn delete_checkpoint(&self, id: &ExecutionId) -> Result<(), StoreError> {
        mainframe_env_store_api::CheckpointStore::delete_checkpoint(self.inner.as_ref(), id)
    }
}

impl<S: PlatformStore> SessionStore for FaultStore<S> {
    fn put_session(
        &self,
        record: SessionRecord,
        expected_version: Option<u64>,
    ) -> Result<(), StoreError> {
        mainframe_env_store_api::SessionStore::put_session(
            self.inner.as_ref(),
            record,
            expected_version,
        )
    }
    fn get_session(&self, id: &str) -> Result<Option<SessionRecord>, StoreError> {
        mainframe_env_store_api::SessionStore::get_session(self.inner.as_ref(), id)
    }
}

impl<S: PlatformStore> ArtifactStore for FaultStore<S> {
    fn health(&self) -> Result<ArtifactStoreHealth, StoreError> {
        mainframe_env_store_api::ArtifactStore::health(self.inner.as_ref())
    }
    fn put_artifact(&self, record: ArtifactRecord) -> Result<(), StoreError> {
        mainframe_env_store_api::ArtifactStore::put_artifact(self.inner.as_ref(), record)
    }
    fn get_artifact(&self, id: &ArtifactRef) -> Result<Option<ArtifactRecord>, StoreError> {
        mainframe_env_store_api::ArtifactStore::get_artifact(self.inner.as_ref(), id)
    }
    fn delete_artifact(&self, id: &ArtifactRef) -> Result<(), StoreError> {
        mainframe_env_store_api::ArtifactStore::delete_artifact(self.inner.as_ref(), id)
    }
}

impl<S: PlatformStore> GenerationStore for FaultStore<S> {
    fn publish_generation(&self, record: GenerationRecord) -> Result<(), StoreError> {
        mainframe_env_store_api::GenerationStore::publish_generation(self.inner.as_ref(), record)
    }
    fn generation(&self, provider: &str) -> Result<Option<GenerationRecord>, StoreError> {
        mainframe_env_store_api::GenerationStore::generation(self.inner.as_ref(), provider)
    }
}

impl<S: PlatformStore> IdempotencyStore for FaultStore<S> {
    fn record_intent(&self, record: EffectRecord) -> Result<(), StoreError> {
        mainframe_env_store_api::IdempotencyStore::record_intent(self.inner.as_ref(), record)
    }
    fn record_result(&self, key: &IdempotencyKey, record: EffectRecord) -> Result<(), StoreError> {
        mainframe_env_store_api::IdempotencyStore::record_result(self.inner.as_ref(), key, record)
    }
    fn effect(&self, key: &IdempotencyKey) -> Result<Option<EffectRecord>, StoreError> {
        mainframe_env_store_api::IdempotencyStore::effect(self.inner.as_ref(), key)
    }
    fn unknown_effects(&self, max: usize) -> Result<Vec<EffectRecord>, StoreError> {
        mainframe_env_store_api::IdempotencyStore::unknown_effects(self.inner.as_ref(), max)
    }
    fn unresolved_effects(&self, _max: usize) -> Result<Vec<EffectRecord>, StoreError> {
        mainframe_env_store_api::IdempotencyStore::unresolved_effects(self.inner.as_ref(), _max)
    }
    fn stale_intents(
        &self,
        now_tick: u64,
        minimum_age_ticks: u64,
        max: usize,
    ) -> Result<Vec<EffectRecord>, StoreError> {
        mainframe_env_store_api::IdempotencyStore::stale_intents(
            self.inner.as_ref(),
            now_tick,
            minimum_age_ticks,
            max,
        )
    }
    fn claim_stale_intent(
        &self,
        key: &IdempotencyKey,
        expected_intent_epoch: u64,
        recovery_owner: &str,
        now_tick: u64,
        minimum_age_ticks: u64,
        lease_ticks: u64,
    ) -> Result<EffectRecord, StoreError> {
        mainframe_env_store_api::IdempotencyStore::claim_stale_intent(
            self.inner.as_ref(),
            key,
            expected_intent_epoch,
            recovery_owner,
            now_tick,
            minimum_age_ticks,
            lease_ticks,
        )
    }
    fn reconcile_stale_intent(
        &self,
        key: &IdempotencyKey,
        recovery_owner: &str,
        recovery_epoch: u64,
        now_tick: u64,
        final_state: mainframe_env_store_api::EffectState,
        format: mainframe_env_store_api::EffectDigestFormat,
        result_digest: [u8; 32],
    ) -> Result<EffectRecord, StoreError> {
        mainframe_env_store_api::IdempotencyStore::reconcile_stale_intent(
            self.inner.as_ref(),
            key,
            recovery_owner,
            recovery_epoch,
            now_tick,
            final_state,
            format,
            result_digest,
        )
    }
    fn reconcile_unknown(
        &self,
        key: &IdempotencyKey,
        final_state: mainframe_env_store_api::EffectState,
        result_digest: [u8; 32],
    ) -> Result<EffectRecord, StoreError> {
        mainframe_env_store_api::IdempotencyStore::reconcile_unknown(
            self.inner.as_ref(),
            key,
            final_state,
            result_digest,
        )
    }
    fn reconcile_unknown_versioned(
        &self,
        key: &IdempotencyKey,
        final_state: mainframe_env_store_api::EffectState,
        format: mainframe_env_store_api::EffectDigestFormat,
        result_digest: [u8; 32],
    ) -> Result<EffectRecord, StoreError> {
        mainframe_env_store_api::IdempotencyStore::reconcile_unknown_versioned(
            self.inner.as_ref(),
            key,
            final_state,
            format,
            result_digest,
        )
    }
}

impl<S: PlatformStore> ProviderStateStore for FaultStore<S> {
    fn publish_provider_read_audited(
        &self,
        _request: mainframe_env_store_api::CheckedProviderReadPublication,
    ) -> Result<(), StoreError> {
        mainframe_env_store_api::ProviderStateStore::publish_provider_read_audited(
            self.inner.as_ref(),
            _request,
        )
    }
    fn assert_provider_replay(
        &self,
        _request: mainframe_env_store_api::ProviderReplayAssertion,
    ) -> Result<(), StoreError> {
        mainframe_env_store_api::ProviderStateStore::assert_provider_replay(
            self.inner.as_ref(),
            _request,
        )
    }
    fn publish_provider_states_audited(
        &self,
        _request: mainframe_env_store_api::AuditedProviderPublication,
    ) -> Result<(), StoreError> {
        mainframe_env_store_api::ProviderStateStore::publish_provider_states_audited(
            self.inner.as_ref(),
            _request,
        )
    }
    fn advance_logical_clock(&self, _observed_floor: u64) -> Result<u64, StoreError> {
        mainframe_env_store_api::ProviderStateStore::advance_logical_clock(
            self.inner.as_ref(),
            _observed_floor,
        )
    }
    fn get_provider_state(
        &self,
        namespace: &str,
        key: &str,
    ) -> Result<Option<ProviderStateRecord>, StoreError> {
        let row = self.inner.get_provider_state(namespace, key)?;
        let hook = self.read_hook.lock().unwrap().clone();
        if let Some(hook) = hook {
            hook(namespace, key);
        }
        Ok(row)
    }
    fn list_provider_state(
        &self,
        namespace: &str,
        max: usize,
    ) -> Result<Vec<ProviderStateRecord>, StoreError> {
        mainframe_env_store_api::ProviderStateStore::list_provider_state(
            self.inner.as_ref(),
            namespace,
            max,
        )
    }
    fn list_provider_state_bounded(
        &self,
        _namespace: &str,
        _max: usize,
        _max_bytes: usize,
    ) -> Result<Vec<ProviderStateRecord>, StoreError> {
        mainframe_env_store_api::ProviderStateStore::list_provider_state_bounded(
            self.inner.as_ref(),
            _namespace,
            _max,
            _max_bytes,
        )
    }
    fn list_provider_state_prefix(
        &self,
        _namespace_prefix: &str,
        _max: usize,
    ) -> Result<Vec<ProviderStateRecord>, StoreError> {
        mainframe_env_store_api::ProviderStateStore::list_provider_state_prefix(
            self.inner.as_ref(),
            _namespace_prefix,
            _max,
        )
    }
    fn put_provider_state(
        &self,
        record: ProviderStateRecord,
        expected_version: Option<u64>,
    ) -> Result<(), StoreError> {
        let observed = record.clone();
        self.inject(std::slice::from_ref(&observed), || {
            self.inner.put_provider_state(record, expected_version)
        })
    }
    fn delete_provider_state(
        &self,
        namespace: &str,
        key: &str,
        expected_version: u64,
    ) -> Result<(), StoreError> {
        mainframe_env_store_api::ProviderStateStore::delete_provider_state(
            self.inner.as_ref(),
            namespace,
            key,
            expected_version,
        )
    }
    fn move_provider_state(
        &self,
        record: ProviderStateRecord,
        old_key: &str,
        expected_version: u64,
    ) -> Result<(), StoreError> {
        mainframe_env_store_api::ProviderStateStore::move_provider_state(
            self.inner.as_ref(),
            record,
            old_key,
            expected_version,
        )
    }
    fn put_provider_states_atomic(
        &self,
        writes: Vec<ProviderStateWrite>,
    ) -> Result<(), StoreError> {
        let records = writes
            .iter()
            .map(|write| write.record.clone())
            .collect::<Vec<_>>();
        self.inject(&records, || self.inner.put_provider_states_atomic(writes))
    }
    fn mutate_provider_states_atomic(
        &self,
        mutations: Vec<ProviderStateMutation>,
    ) -> Result<(), StoreError> {
        let records = mutations
            .iter()
            .filter_map(|mutation| match mutation {
                ProviderStateMutation::Put(write) => Some(write.record.clone()),
                ProviderStateMutation::Move { record, .. } => Some(record.clone()),
                ProviderStateMutation::Delete { .. } => None,
            })
            .collect::<Vec<_>>();
        self.inject(&records, || {
            self.inner.mutate_provider_states_atomic(mutations)
        })
    }
    fn archive_provider_state_replacement(
        &self,
        _request: ProviderStateArchiveReplacement,
    ) -> Result<RetentionArchive, StoreError> {
        mainframe_env_store_api::ProviderStateStore::archive_provider_state_replacement(
            self.inner.as_ref(),
            _request,
        )
    }
    fn provider_retention_archive_usage(
        &self,
        _target: RetentionTarget,
    ) -> Result<(usize, u64), StoreError> {
        mainframe_env_store_api::ProviderStateStore::provider_retention_archive_usage(
            self.inner.as_ref(),
            _target,
        )
    }
    fn provider_retention_authority_usage(
        &self,
        _target: RetentionTarget,
    ) -> Result<mainframe_env_store_api::RetentionAuthorityUsage, StoreError> {
        mainframe_env_store_api::ProviderStateStore::provider_retention_authority_usage(
            self.inner.as_ref(),
            _target,
        )
    }
    fn provider_state_retention_epoch(&self) -> Result<u64, StoreError> {
        mainframe_env_store_api::ProviderStateStore::provider_state_retention_epoch(
            self.inner.as_ref(),
        )
    }
    fn provider_retention_observations(
        &self,
        _target: RetentionTarget,
        _max: usize,
    ) -> Result<Vec<mainframe_env_store_api::RetentionObservation>, StoreError> {
        mainframe_env_store_api::ProviderStateStore::provider_retention_observations(
            self.inner.as_ref(),
            _target,
            _max,
        )
    }
    fn provider_retention_observation_page(
        &self,
        _target: RetentionTarget,
        _after: Option<&mainframe_env_store_api::ProviderStateIdentity>,
        _max: usize,
    ) -> Result<Vec<(u64, mainframe_env_store_api::RetentionObservation)>, StoreError> {
        mainframe_env_store_api::ProviderStateStore::provider_retention_observation_page(
            self.inner.as_ref(),
            _target,
            _after,
            _max,
        )
    }
    fn delete_provider_retention_observation(
        &self,
        _request: mainframe_env_store_api::ProviderRetentionObservationDeletion,
    ) -> Result<(), StoreError> {
        mainframe_env_store_api::ProviderStateStore::delete_provider_retention_observation(
            self.inner.as_ref(),
            _request,
        )
    }
    fn record_provider_retention_observation(
        &self,
        _source: ProviderStateRecord,
        _expected_epoch: u64,
        _observation: mainframe_env_store_api::RetentionObservation,
    ) -> Result<RetentionReconciliationReceipt, StoreError> {
        mainframe_env_store_api::ProviderStateStore::record_provider_retention_observation(
            self.inner.as_ref(),
            _source,
            _expected_epoch,
            _observation,
        )
    }
    fn archive_provider_state_deletion(
        &self,
        _request: ProviderStateArchiveDeletion,
    ) -> Result<RetentionArchive, StoreError> {
        mainframe_env_store_api::ProviderStateStore::archive_provider_state_deletion(
            self.inner.as_ref(),
            _request,
        )
    }
    fn archive_provider_state_deletion_with_capacity(
        &self,
        _request: ProviderStateArchiveDeletionWithCapacity,
    ) -> Result<RetentionArchive, StoreError> {
        mainframe_env_store_api::ProviderStateStore::archive_provider_state_deletion_with_capacity(
            self.inner.as_ref(),
            _request,
        )
    }
}
