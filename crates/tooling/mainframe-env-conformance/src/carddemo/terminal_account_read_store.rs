//! Test-only exact public store forwarding, passive counters and a known post-commit cut.
use super::*;

#[derive(Clone)]
pub(super) struct ArmedCut {
    artifact: ArtifactRef,
    handoff: Handoff,
    cics: std::sync::Weak<mainframe_env_cics::CicsService>,
}
impl ArmedCut {
    pub(super) fn handoff_at_cut(
        &self,
        current: &ExecutionRecord,
        effect: &EffectRecord,
    ) -> Result<Handoff, CorpusProblem> {
        let cics = self.cics.upgrade().ok_or_else(problem)?;
        public_handoff_at_cut(&cics, &self.handoff, current, effect)
    }
}

pub(super) struct ObservedStore {
    inner: Arc<dyn PlatformStore>,
    fetches: Mutex<BTreeMap<String, u64>>,
    armed: Mutex<Option<ArmedCut>>,
    completed: Mutex<Vec<EffectRecord>>,
    root: PathBuf,
    dataset_decisions: Mutex<u64>,
}
impl ObservedStore {
    pub(super) fn new(inner: Arc<dyn PlatformStore>, root: PathBuf) -> Self {
        Self {
            inner,
            fetches: Mutex::new(BTreeMap::new()),
            armed: Mutex::new(None),
            completed: Mutex::new(Vec::new()),
            root,
            dataset_decisions: Mutex::new(0),
        }
    }
    pub(super) fn arm(
        &self,
        artifact: ArtifactRef,
        handoff: Handoff,
        cics: std::sync::Weak<mainframe_env_cics::CicsService>,
    ) -> Result<(), CorpusProblem> {
        let mut armed = self.armed.lock().map_err(|_| problem())?;
        require(armed.is_none() && handoff.artifact == artifact.as_str())?;
        *armed = Some(ArmedCut {
            artifact,
            handoff,
            cics,
        });
        *self.dataset_decisions.lock().map_err(|_| problem())? = 0;
        Ok(())
    }
    pub(super) fn dataset_decisions(&self) -> Result<u64, CorpusProblem> {
        self.dataset_decisions
            .lock()
            .map(|value| *value)
            .map_err(|_| problem())
    }

    fn fetch(&self, namespace: &str) -> Result<(), StoreError> {
        let mut observed = self
            .fetches
            .lock()
            .map_err(|_| StoreError::Infrastructure("counter-lock".into()))?;
        if !observed.contains_key(namespace) && observed.len() >= 4096 {
            return Err(StoreError::CapacityExceeded);
        }
        let count = observed.entry(namespace.into()).or_default();
        *count = count.checked_add(1).ok_or(StoreError::CapacityExceeded)?;
        Ok(())
    }
    pub(super) fn snapshot(&self) -> Result<BTreeMap<String, u64>, CorpusProblem> {
        self.fetches
            .lock()
            .map(|value| value.clone())
            .map_err(|_| problem())
    }
    fn known_completed(
        &self,
        effect: Option<&EffectRecord>,
        current: &ExecutionRecord,
    ) -> Result<(), StoreError> {
        let Some(effect) = effect else {
            return Ok(());
        };
        let expected = self
            .armed
            .lock()
            .map_err(|_| StoreError::Infrastructure("cut-lock".into()))?
            .clone();
        let Some(expected) = expected else {
            return Ok(());
        };
        if expected.artifact != current.artifact
            || current.selector.as_str() != "program:COACTVWC"
            || current.state != ExecutionState::Running
            || effect.state != EffectState::Completed
            || effect
                .intent
                .capability
                .as_ref()
                .is_none_or(|value| value.as_str() != "host.cics.execute")
        {
            return Ok(());
        }
        let Some(row) = self
            .inner
            .get_provider_state("cics-effect-replay-v1", effect.key.as_str())?
        else {
            return Ok(());
        };
        if row.payload.get(..8) != Some(b"MECER004") {
            return Ok(());
        }
        let described = mainframe_env_cics::describe_cics_replay_row(
            &row,
            Some(effect),
            mainframe_env_cics::CicsLimits::default(),
        )
        .map_err(|_| StoreError::InvalidTransition)?;
        if described.retention != mainframe_env_cics::CicsReplayRetentionState::Terminal
            || described.codec != mainframe_env_cics::CicsReplayCodecVersion::PlainReadV4
            || described.owner_execution.as_deref() != Some(current.execution_id.as_str())
            || described.owner_run_unit.as_deref() != Some(current.run_unit_id.as_str())
        {
            return Err(StoreError::InvalidTransition);
        }
        let mut completed = self
            .completed
            .lock()
            .map_err(|_| StoreError::Infrastructure("cut-lock".into()))?;
        if completed.iter().any(|value| value.key == effect.key) {
            return Err(StoreError::Conflict);
        }
        completed.push(effect.clone());
        if completed.len() > 3 {
            return Err(StoreError::InvalidTransition);
        }
        if completed.len() == 3 {
            // The genuine coordinator transaction has returned success; no later machine
            // checkpoint/result delivery has happened. This deliberately exits the child.
            save_cut(
                &self.root,
                self.inner.as_ref(),
                current,
                &completed,
                self.snapshot().map_err(|_| StoreError::InvalidTransition)?,
                self.dataset_decisions()
                    .map_err(|_| StoreError::InvalidTransition)?,
                &expected,
            )
            .map_err(|_| StoreError::InvalidTransition)?;
            std::process::exit(77);
        }
        Ok(())
    }
}

impl AuditSink for ObservedStore {
    fn audit_subject_records(
        &self,
        _execution_id: &ExecutionId,
        _max: usize,
    ) -> Result<Vec<mainframe_env_execution_api::AuditSubjectRecord>, StoreError> {
        self.inner.audit_subject_records(_execution_id, _max)
    }
    fn record_audit(&self, record: AuditRecord) -> Result<(), StoreError> {
        let counted = record.capability.as_str() == "host.dataset.read";
        self.inner.record_audit(record)?;
        if counted {
            let mut value = self
                .dataset_decisions
                .lock()
                .map_err(|_| StoreError::Infrastructure("decision-lock".into()))?;
            *value = value.checked_add(1).ok_or(StoreError::CapacityExceeded)?;
        }
        Ok(())
    }
    fn audit_records(
        &self,
        execution_id: &ExecutionId,
        start_effect_sequence: u64,
        max: usize,
    ) -> Result<Vec<AuditRecord>, StoreError> {
        self.inner
            .audit_records(execution_id, start_effect_sequence, max)
    }
}

impl ExecutionStore for ObservedStore {
    fn create_execution(&self, record: ExecutionRecord) -> Result<(), StoreError> {
        self.inner.create_execution(record)
    }
    fn get_execution(&self, id: &ExecutionId) -> Result<Option<ExecutionRecord>, StoreError> {
        self.inner.get_execution(id)
    }
    fn transition_execution(
        &self,
        id: &ExecutionId,
        expected_version: u64,
        next: ExecutionState,
        now_tick: u64,
    ) -> Result<ExecutionRecord, StoreError> {
        self.inner
            .transition_execution(id, expected_version, next, now_tick)
    }
}

impl EventStore for ObservedStore {
    fn append_event(&self, event: LifecycleEvent) -> Result<(), StoreError> {
        self.inner.append_event(event)
    }
    fn events(
        &self,
        id: &ExecutionId,
        start_sequence: u64,
        max: usize,
    ) -> Result<Vec<LifecycleEvent>, StoreError> {
        self.inner.events(id, start_sequence, max)
    }
}

impl WorkStore for ObservedStore {
    fn get_work(&self, work_id: &str) -> Result<Option<WorkRecord>, StoreError> {
        self.inner.get_work(work_id)
    }
    fn enqueue(&self, work: WorkRecord) -> Result<(), StoreError> {
        self.inner.enqueue(work)
    }
    fn claim(
        &self,
        worker: &str,
        required_generation: Option<&str>,
        now_tick: u64,
        lease_ticks: u64,
    ) -> Result<Option<WorkRecord>, StoreError> {
        self.inner
            .claim(worker, required_generation, now_tick, lease_ticks)
    }
    fn heartbeat(
        &self,
        work_id: &str,
        lease_id: &str,
        lease_epoch: u64,
        now_tick: u64,
        lease_ticks: u64,
    ) -> Result<WorkRecord, StoreError> {
        self.inner
            .heartbeat(work_id, lease_id, lease_epoch, now_tick, lease_ticks)
    }
    fn release(
        &self,
        work_id: &str,
        lease_id: &str,
        lease_epoch: u64,
        now_tick: u64,
        available_tick: u64,
    ) -> Result<WorkRecord, StoreError> {
        self.inner
            .release(work_id, lease_id, lease_epoch, now_tick, available_tick)
    }
    fn request_cancellation(&self, work_id: &str) -> Result<WorkRecord, StoreError> {
        self.inner.request_cancellation(work_id)
    }
    fn dead_letter(
        &self,
        work_id: &str,
        lease_id: &str,
        lease_epoch: u64,
        now_tick: u64,
    ) -> Result<WorkRecord, StoreError> {
        self.inner
            .dead_letter(work_id, lease_id, lease_epoch, now_tick)
    }
    fn complete(
        &self,
        work_id: &str,
        lease_id: &str,
        lease_epoch: u64,
        now_tick: u64,
    ) -> Result<(), StoreError> {
        self.inner
            .complete(work_id, lease_id, lease_epoch, now_tick)
    }
}

impl OutboxStore for ObservedStore {
    fn append_notification(&self, record: OutboxRecord) -> Result<(), StoreError> {
        self.inner.append_notification(record)
    }
    fn pending_notifications(&self, max: usize) -> Result<Vec<OutboxRecord>, StoreError> {
        self.inner.pending_notifications(max)
    }
    fn mark_notification_delivered(
        &self,
        notification_id: &str,
        expected_version: u64,
        delivered_tick: u64,
    ) -> Result<OutboxRecord, StoreError> {
        self.inner
            .mark_notification_delivered(notification_id, expected_version, delivered_tick)
    }
}

impl RetentionStore for ObservedStore {
    fn retention_capacity_health(
        &self,
        _policy: RetentionPolicy,
    ) -> Result<mainframe_env_store_api::RetentionCapacityHealth, StoreError> {
        self.inner.retention_capacity_health(_policy)
    }
    fn retention_forecast(
        &self,
        target: RetentionTarget,
        policy: RetentionPolicy,
        now_tick: u64,
        observed_growth_per_tick: u64,
    ) -> Result<RetentionForecast, StoreError> {
        self.inner
            .retention_forecast(target, policy, now_tick, observed_growth_per_tick)
    }
    fn retention_forecast_with_dependencies(
        &self,
        _target: RetentionTarget,
        _policy: RetentionPolicy,
        _now_tick: u64,
        _observed_growth_per_tick: u64,
        _dependencies: &mainframe_env_store_api::CoreRetentionDependencySnapshot,
    ) -> Result<RetentionForecast, StoreError> {
        self.inner.retention_forecast_with_dependencies(
            _target,
            _policy,
            _now_tick,
            _observed_growth_per_tick,
            _dependencies,
        )
    }
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
        self.inner.provider_validated_retention_forecast(
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
        self.inner.archive_and_prune(policy, request)
    }
    fn archive_and_prune_with_dependencies(
        &self,
        _policy: RetentionPolicy,
        _request: RetentionRequest,
        _dependencies: &mainframe_env_store_api::CoreRetentionDependencySnapshot,
    ) -> Result<RetentionReceipt, StoreError> {
        self.inner
            .archive_and_prune_with_dependencies(_policy, _request, _dependencies)
    }
    fn retention_archives(
        &self,
        target: RetentionTarget,
        max: usize,
    ) -> Result<Vec<RetentionArchive>, StoreError> {
        self.inner.retention_archives(target, max)
    }
    fn prune_retention_archives(
        &self,
        policy: RetentionPolicy,
        now_tick: u64,
        max: usize,
    ) -> Result<usize, StoreError> {
        self.inner.prune_retention_archives(policy, now_tick, max)
    }
    fn prune_retention_archives_authorized(
        &self,
        _policy: RetentionPolicy,
        _request: mainframe_env_store_api::RetentionArchivePruneRequest,
    ) -> Result<mainframe_env_store_api::RetentionArchivePruneOutcome, StoreError> {
        self.inner
            .prune_retention_archives_authorized(_policy, _request)
    }
    fn reconcile_retention_age(
        &self,
        request: RetentionAgeReconciliation,
        now_tick: u64,
    ) -> Result<RetentionReconciliationReceipt, StoreError> {
        self.inner.reconcile_retention_age(request, now_tick)
    }
    fn retention_legacy_rows(
        &self,
        target: RetentionTarget,
        max: usize,
    ) -> Result<Vec<RetentionLegacyRow>, StoreError> {
        self.inner.retention_legacy_rows(target, max)
    }
}

impl JournalStore for ObservedStore {
    fn commit_checked_replay_refusal(
        &self,
        _request: mainframe_env_store_api::CheckedReplayRefusalStep,
    ) -> Result<ExecutionRecord, StoreError> {
        self.inner.commit_checked_replay_refusal(_request)
    }
    fn mutate_root_preparation_states(
        &self,
        _request: mainframe_env_store_api::RootPreparationPublication,
    ) -> Result<(), StoreError> {
        self.inner.mutate_root_preparation_states(_request)
    }
    fn mutate_root_provider_states(
        &self,
        _request: mainframe_env_store_api::RootProviderPublication,
    ) -> Result<(), StoreError> {
        self.inner.mutate_root_provider_states(_request)
    }
    fn admit_root_driver(
        &self,
        _admission: mainframe_env_store_api::RootDriverAdmission,
    ) -> Result<mainframe_env_store_api::RootDriverClaim, StoreError> {
        self.inner.admit_root_driver(_admission)
    }
    fn admit_root_child(
        &self,
        _admission: mainframe_env_store_api::RootChildAdmission,
    ) -> Result<(), StoreError> {
        self.inner.admit_root_child(_admission)
    }
    fn register_root_provider_row(
        &self,
        _admission: mainframe_env_store_api::RootProviderRowAdmission,
    ) -> Result<(), StoreError> {
        self.inner.register_root_provider_row(_admission)
    }
    fn fence_root_driver(
        &self,
        _claim: &mainframe_env_store_api::RootDriverClaim,
        _execution: &ExecutionRecord,
        _observed_tick: u64,
    ) -> Result<ProviderStateRecord, StoreError> {
        self.inner
            .fence_root_driver(_claim, _execution, _observed_tick)
    }
    fn root_custody_supported(&self) -> bool {
        self.inner.root_custody_supported()
    }
    fn admit_root_successor(
        &self,
        _request: mainframe_env_store_api::RootCustodySuccessor,
    ) -> Result<RootDriverClaim, StoreError> {
        self.inner.admit_root_successor(_request)
    }
    fn capture_root_custody_cutoff(
        &self,
        _claim: &mainframe_env_store_api::RootDriverClaim,
        _execution: &ExecutionRecord,
        _observed_tick: u64,
    ) -> Result<mainframe_env_store_api::RootCustodyCutoff, StoreError> {
        self.inner
            .capture_root_custody_cutoff(_claim, _execution, _observed_tick)
    }
    fn close_root_driver(
        &self,
        _claim: &mainframe_env_store_api::RootDriverClaim,
        _execution: &ExecutionRecord,
        _observed_tick: u64,
    ) -> Result<mainframe_env_store_api::RootClosureSnapshot, StoreError> {
        self.inner
            .close_root_driver(_claim, _execution, _observed_tick)
    }
    fn commit_root_terminal_step(
        &self,
        _request: mainframe_env_store_api::RootTerminalPublication,
    ) -> Result<mainframe_env_store_api::RootTerminalCommit, StoreError> {
        self.inner.commit_root_terminal_step(_request)
    }
    fn admit_execution(
        &self,
        execution: ExecutionRecord,
        event: LifecycleEvent,
        notification: OutboxRecord,
    ) -> Result<(), StoreError> {
        self.inner.admit_execution(execution, event, notification)
    }
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
    ) -> Result<ExecutionRecord, StoreError> {
        let observed = effect.clone();
        let cuttable = checkpoint.is_none()
            && next_state.is_none()
            && matches!(&event.kind, LifecycleEventKind::EffectResult { sequence } if observed.as_ref().is_some_and(|value| value.sequence == *sequence));
        let result = self.inner.commit_execution_step(
            execution_id,
            expected_version,
            next_state,
            event,
            effect,
            audit,
            checkpoint,
            notification,
        )?;
        if cuttable {
            self.known_completed(observed.as_ref(), &result)?;
        }
        Ok(result)
    }
}

impl CheckpointStore for ObservedStore {
    fn put_checkpoint(&self, record: CheckpointRecord) -> Result<(), StoreError> {
        self.inner.put_checkpoint(record)
    }
    fn get_checkpoint(&self, id: &ExecutionId) -> Result<Option<CheckpointRecord>, StoreError> {
        self.inner.get_checkpoint(id)
    }
    fn delete_checkpoint(&self, id: &ExecutionId) -> Result<(), StoreError> {
        self.inner.delete_checkpoint(id)
    }
}

impl SessionStore for ObservedStore {
    fn put_session(
        &self,
        record: SessionRecord,
        expected_version: Option<u64>,
    ) -> Result<(), StoreError> {
        self.inner.put_session(record, expected_version)
    }
    fn get_session(&self, id: &str) -> Result<Option<SessionRecord>, StoreError> {
        self.inner.get_session(id)
    }
}

impl ArtifactStore for ObservedStore {
    fn health(&self) -> Result<ArtifactStoreHealth, StoreError> {
        self.inner.health()
    }
    fn put_artifact(&self, record: ArtifactRecord) -> Result<(), StoreError> {
        self.inner.put_artifact(record)
    }
    fn get_artifact(&self, id: &ArtifactRef) -> Result<Option<ArtifactRecord>, StoreError> {
        self.inner.get_artifact(id)
    }
    fn delete_artifact(&self, id: &ArtifactRef) -> Result<(), StoreError> {
        self.inner.delete_artifact(id)
    }
}

impl GenerationStore for ObservedStore {
    fn publish_generation(&self, record: GenerationRecord) -> Result<(), StoreError> {
        self.inner.publish_generation(record)
    }
    fn generation(&self, provider: &str) -> Result<Option<GenerationRecord>, StoreError> {
        self.inner.generation(provider)
    }
}

impl IdempotencyStore for ObservedStore {
    fn record_intent(&self, record: EffectRecord) -> Result<(), StoreError> {
        self.inner.record_intent(record)
    }
    fn record_result(&self, key: &IdempotencyKey, record: EffectRecord) -> Result<(), StoreError> {
        self.inner.record_result(key, record)
    }
    fn effect(&self, key: &IdempotencyKey) -> Result<Option<EffectRecord>, StoreError> {
        self.inner.effect(key)
    }
    fn unknown_effects(&self, max: usize) -> Result<Vec<EffectRecord>, StoreError> {
        self.inner.unknown_effects(max)
    }
    fn unresolved_effects(&self, _max: usize) -> Result<Vec<EffectRecord>, StoreError> {
        self.inner.unresolved_effects(_max)
    }
    fn stale_intents(
        &self,
        now_tick: u64,
        minimum_age_ticks: u64,
        max: usize,
    ) -> Result<Vec<EffectRecord>, StoreError> {
        self.inner.stale_intents(now_tick, minimum_age_ticks, max)
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
        self.inner.claim_stale_intent(
            key,
            expected_intent_epoch,
            recovery_owner,
            now_tick,
            minimum_age_ticks,
            lease_ticks,
        )
    }
    #[allow(clippy::too_many_arguments)]
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
        self.inner.reconcile_stale_intent(
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
        self.inner
            .reconcile_unknown(key, final_state, result_digest)
    }
    fn reconcile_unknown_versioned(
        &self,
        key: &IdempotencyKey,
        final_state: mainframe_env_store_api::EffectState,
        format: mainframe_env_store_api::EffectDigestFormat,
        result_digest: [u8; 32],
    ) -> Result<EffectRecord, StoreError> {
        self.inner
            .reconcile_unknown_versioned(key, final_state, format, result_digest)
    }
}

impl ProviderStateStore for ObservedStore {
    fn publish_provider_read_audited(
        &self,
        _request: mainframe_env_store_api::CheckedProviderReadPublication,
    ) -> Result<(), StoreError> {
        self.inner.publish_provider_read_audited(_request)
    }
    fn assert_provider_replay(
        &self,
        _request: mainframe_env_store_api::ProviderReplayAssertion,
    ) -> Result<(), StoreError> {
        self.inner.assert_provider_replay(_request)
    }
    fn publish_provider_states_audited(
        &self,
        _request: mainframe_env_store_api::AuditedProviderPublication,
    ) -> Result<(), StoreError> {
        self.inner.publish_provider_states_audited(_request)
    }
    fn advance_logical_clock(&self, _observed_floor: u64) -> Result<u64, StoreError> {
        self.inner.advance_logical_clock(_observed_floor)
    }
    fn get_provider_state(
        &self,
        namespace: &str,
        key: &str,
    ) -> Result<Option<ProviderStateRecord>, StoreError> {
        self.fetch(namespace)?;
        self.inner.get_provider_state(namespace, key)
    }
    fn list_provider_state(
        &self,
        namespace: &str,
        max: usize,
    ) -> Result<Vec<ProviderStateRecord>, StoreError> {
        self.fetch(namespace)?;
        self.inner.list_provider_state(namespace, max)
    }
    fn list_provider_state_bounded(
        &self,
        _namespace: &str,
        _max: usize,
        _max_bytes: usize,
    ) -> Result<Vec<ProviderStateRecord>, StoreError> {
        self.fetch(_namespace)?;
        self.inner
            .list_provider_state_bounded(_namespace, _max, _max_bytes)
    }
    fn list_provider_state_prefix(
        &self,
        _namespace_prefix: &str,
        _max: usize,
    ) -> Result<Vec<ProviderStateRecord>, StoreError> {
        self.fetch(_namespace_prefix)?;
        self.inner
            .list_provider_state_prefix(_namespace_prefix, _max)
    }
    fn put_provider_state(
        &self,
        record: ProviderStateRecord,
        expected_version: Option<u64>,
    ) -> Result<(), StoreError> {
        self.inner.put_provider_state(record, expected_version)
    }
    fn delete_provider_state(
        &self,
        namespace: &str,
        key: &str,
        expected_version: u64,
    ) -> Result<(), StoreError> {
        self.inner
            .delete_provider_state(namespace, key, expected_version)
    }
    fn move_provider_state(
        &self,
        record: ProviderStateRecord,
        old_key: &str,
        expected_version: u64,
    ) -> Result<(), StoreError> {
        self.inner
            .move_provider_state(record, old_key, expected_version)
    }
    fn put_provider_states_atomic(
        &self,
        writes: Vec<ProviderStateWrite>,
    ) -> Result<(), StoreError> {
        self.inner.put_provider_states_atomic(writes)
    }
    fn mutate_provider_states_atomic(
        &self,
        mutations: Vec<ProviderStateMutation>,
    ) -> Result<(), StoreError> {
        self.inner.mutate_provider_states_atomic(mutations)
    }
    fn archive_provider_state_replacement(
        &self,
        _request: ProviderStateArchiveReplacement,
    ) -> Result<RetentionArchive, StoreError> {
        self.inner.archive_provider_state_replacement(_request)
    }
    fn provider_retention_archive_usage(
        &self,
        _target: RetentionTarget,
    ) -> Result<(usize, u64), StoreError> {
        self.inner.provider_retention_archive_usage(_target)
    }
    fn provider_retention_authority_usage(
        &self,
        _target: RetentionTarget,
    ) -> Result<mainframe_env_store_api::RetentionAuthorityUsage, StoreError> {
        self.inner.provider_retention_authority_usage(_target)
    }
    fn provider_state_retention_epoch(&self) -> Result<u64, StoreError> {
        self.inner.provider_state_retention_epoch()
    }
    fn provider_retention_observations(
        &self,
        _target: RetentionTarget,
        _max: usize,
    ) -> Result<Vec<mainframe_env_store_api::RetentionObservation>, StoreError> {
        self.inner.provider_retention_observations(_target, _max)
    }
    fn provider_retention_observation_page(
        &self,
        _target: RetentionTarget,
        _after: Option<&mainframe_env_store_api::ProviderStateIdentity>,
        _max: usize,
    ) -> Result<Vec<(u64, mainframe_env_store_api::RetentionObservation)>, StoreError> {
        self.inner
            .provider_retention_observation_page(_target, _after, _max)
    }
    fn delete_provider_retention_observation(
        &self,
        _request: mainframe_env_store_api::ProviderRetentionObservationDeletion,
    ) -> Result<(), StoreError> {
        self.inner.delete_provider_retention_observation(_request)
    }
    fn record_provider_retention_observation(
        &self,
        _source: ProviderStateRecord,
        _expected_epoch: u64,
        _observation: mainframe_env_store_api::RetentionObservation,
    ) -> Result<RetentionReconciliationReceipt, StoreError> {
        self.inner
            .record_provider_retention_observation(_source, _expected_epoch, _observation)
    }
    fn archive_provider_state_deletion(
        &self,
        _request: ProviderStateArchiveDeletion,
    ) -> Result<RetentionArchive, StoreError> {
        self.inner.archive_provider_state_deletion(_request)
    }
    fn archive_provider_state_deletion_with_capacity(
        &self,
        _request: ProviderStateArchiveDeletionWithCapacity,
    ) -> Result<RetentionArchive, StoreError> {
        self.inner
            .archive_provider_state_deletion_with_capacity(_request)
    }
}
