use crate::durable::{
    AUDIT_NAMESPACE, decode_audit, decode_effect, decode_event, decode_execution, decode_outbox,
    decode_work,
};
use crate::retention::{
    ForecastCounts, build_archive, cics_replay_metadata, forecast, replay_metadata, replay_schema,
    target_namespace, target_watermark, target_window_open, validate_request,
};
use crate::{PostgresStateStore, SqliteStateStore};
use mainframe_env_execution_api::{IdempotencyKey, InvocationLimits};
use mainframe_env_store_api::{
    ArchivedRetentionRow, CoreRetentionDependencySnapshot, EffectState, ProviderStateRecord,
    ProviderStateStore, RetentionAgeReconciliation, RetentionArchive, RetentionArchivePruneOutcome,
    RetentionArchivePruneReceipt, RetentionArchivePruneRequest, RetentionForecast,
    RetentionLegacyRow, RetentionObservation, RetentionPolicy, RetentionReceipt,
    RetentionReconciliationReceipt, RetentionRequest, RetentionStore, RetentionTarget, StoreError,
};
use std::collections::{BTreeMap, BTreeSet};

struct Candidate {
    tick: u64,
    row: ProviderStateRecord,
    owner_execution: Option<mainframe_env_execution_api::ExecutionId>,
}

struct CandidateSet {
    active: usize,
    eligible: Vec<Candidate>,
}

struct CoreDependencies {
    executions: BTreeSet<mainframe_env_execution_api::ExecutionId>,
    effect_keys: BTreeSet<String>,
    unowned: bool,
}

fn core_dependencies(
    store: &dyn DurableRetentionBackend,
    _capacity: usize,
    supplied: Option<&CoreRetentionDependencySnapshot>,
) -> Result<CoreDependencies, StoreError> {
    let Some(supplied) = supplied else {
        return Ok(CoreDependencies {
            executions: BTreeSet::new(),
            effect_keys: BTreeSet::new(),
            unowned: false,
        });
    };
    if supplied.expected_epoch != store.retention_epoch()?
        || supplied.blocked_executions.len()
            > mainframe_env_store_api::MAX_CORE_RETENTION_DEPENDENCIES
        || supplied.blocked_effect_keys.len()
            > mainframe_env_store_api::MAX_CORE_RETENTION_DEPENDENCIES
    {
        return Err(StoreError::Conflict);
    }
    let executions = supplied
        .blocked_executions
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    let effect_keys = supplied
        .blocked_effect_keys
        .iter()
        .map(|key| key.as_str().to_owned())
        .collect::<BTreeSet<_>>();
    if executions.len() != supplied.blocked_executions.len()
        || effect_keys.len() != supplied.blocked_effect_keys.len()
    {
        return Err(StoreError::IncompatibleVersion);
    }
    Ok(CoreDependencies {
        executions,
        effect_keys,
        unowned: supplied.unowned,
    })
}

type ObservationMap = BTreeMap<(String, String), (u64, RetentionObservation)>;

fn observations(
    store: &dyn DurableRetentionBackend,
    target: RetentionTarget,
) -> Result<ObservationMap, StoreError> {
    Ok(store
        .retention_load_observations(target)?
        .into_iter()
        .map(|(version, observation)| {
            (
                (observation.namespace.clone(), observation.key.clone()),
                (version, observation),
            )
        })
        .collect::<ObservationMap>())
}

fn observed_age<'a>(
    observations: &'a ObservationMap,
    row: &ProviderStateRecord,
) -> Option<&'a RetentionObservation> {
    observations
        .get(&(row.namespace.clone(), row.key.clone()))
        .map(|(_, observation)| observation)
        .filter(|observation| {
            crate::retention::observation_matches_source(observation, row.version, &row.payload)
        })
}

trait DurableRetentionBackend: ProviderStateStore {
    fn retention_max_payload_bytes(&self) -> usize;
    fn retention_max_archive_rows(&self) -> usize;
    fn retention_max_archive_bytes(&self) -> u64;
    fn retention_epoch(&self) -> Result<u64, StoreError>;
    fn retention_provider_usage(&self) -> Result<usize, StoreError>;
    fn retention_archive_usage(&self) -> Result<(usize, u64), StoreError>;
    fn retention_observation_usage(&self) -> Result<(usize, u64), StoreError>;
    fn retention_load_observations(
        &self,
        target: RetentionTarget,
    ) -> Result<Vec<(u64, RetentionObservation)>, StoreError>;
    fn retention_commit_archive(
        &self,
        archive: &RetentionArchive,
        expected_epoch: u64,
    ) -> Result<(), StoreError>;
    fn retention_commit_observation(
        &self,
        observation: RetentionObservation,
        source: ProviderStateRecord,
        expected_epoch: u64,
    ) -> Result<(u64, u64), StoreError>;
    fn retention_load_archives(
        &self,
        target: Option<RetentionTarget>,
        max: usize,
        through_tick: Option<u64>,
    ) -> Result<Vec<RetentionArchive>, StoreError>;
    fn retention_delete_archives(&self, archives: &[RetentionArchive]) -> Result<(), StoreError>;
}

macro_rules! durable_retention_backend {
    ($store:ty) => {
        impl DurableRetentionBackend for $store {
            fn retention_max_payload_bytes(&self) -> usize {
                self.max_payload_bytes()
            }

            fn retention_max_archive_rows(&self) -> usize {
                self.max_archive_rows()
            }

            fn retention_max_archive_bytes(&self) -> u64 {
                self.max_archive_bytes()
            }

            fn retention_epoch(&self) -> Result<u64, StoreError> {
                self.retention_epoch()
            }

            fn retention_provider_usage(&self) -> Result<usize, StoreError> {
                self.provider_state_usage()
            }

            fn retention_archive_usage(&self) -> Result<(usize, u64), StoreError> {
                self.retention_archive_usage()
            }

            fn retention_observation_usage(&self) -> Result<(usize, u64), StoreError> {
                self.retention_observation_usage()
            }

            fn retention_load_observations(
                &self,
                target: RetentionTarget,
            ) -> Result<Vec<(u64, RetentionObservation)>, StoreError> {
                self.load_retention_observations(target)
            }

            fn retention_commit_archive(
                &self,
                archive: &RetentionArchive,
                expected_epoch: u64,
            ) -> Result<(), StoreError> {
                self.commit_retention_archive(archive, expected_epoch)
            }

            fn retention_commit_observation(
                &self,
                observation: RetentionObservation,
                source: ProviderStateRecord,
                expected_epoch: u64,
            ) -> Result<(u64, u64), StoreError> {
                self.commit_retention_observation(observation, source, expected_epoch, false)
            }

            fn retention_load_archives(
                &self,
                target: Option<RetentionTarget>,
                max: usize,
                through_tick: Option<u64>,
            ) -> Result<Vec<RetentionArchive>, StoreError> {
                self.load_retention_archives(target, max, through_tick)
            }

            fn retention_delete_archives(
                &self,
                archives: &[RetentionArchive],
            ) -> Result<(), StoreError> {
                self.delete_retention_archives(archives)
            }
        }
    };
}

durable_retention_backend!(SqliteStateStore);
durable_retention_backend!(PostgresStateStore);

fn durable_forecast(
    store: &dyn DurableRetentionBackend,
    capacity: usize,
    target: RetentionTarget,
    policy: RetentionPolicy,
    now_tick: u64,
    observed_growth_per_tick: u64,
    dependencies: Option<&CoreRetentionDependencySnapshot>,
) -> Result<RetentionForecast, StoreError> {
    if dependencies.is_none() && crate::retention::dependency_sensitive_core_target(target)
        || dependencies.is_some() && !crate::retention::dependency_sensitive_core_target(target)
    {
        return Err(StoreError::InvalidTransition);
    }
    let before = store.retention_epoch()?;
    let watermarks = policy.watermarks(now_tick)?;
    let candidates = candidates(
        store,
        target,
        target_watermark(target, watermarks),
        target_window_open(target, policy, now_tick),
        capacity,
        dependencies,
    )?;
    let (archive_records, archive_bytes) = store.retention_archive_usage()?;
    let (observation_records, observation_bytes) = store.retention_observation_usage()?;
    let total_used = store.retention_provider_usage()?;
    let result = forecast(
        target,
        policy,
        now_tick,
        observed_growth_per_tick,
        ForecastCounts {
            active: candidates.active,
            eligible: candidates.eligible.len(),
            capacity,
            total_used,
            archive_records,
            archive_capacity: store.retention_max_archive_rows(),
            archive_total_used: archive_records,
            archive_bytes,
            archive_byte_capacity: store.retention_max_archive_bytes(),
            observation_records,
            observation_capacity: store.retention_max_archive_rows(),
            observation_bytes,
            observation_byte_capacity: store.retention_max_archive_bytes(),
            max_source_storage_bytes: crate::retention::worst_case_archived_row_storage_bytes(
                store.retention_max_payload_bytes(),
            ),
        },
    )?;
    if store.retention_epoch()? != before {
        return Err(StoreError::Conflict);
    }
    Ok(result)
}

fn durable_archive_and_prune(
    store: &dyn DurableRetentionBackend,
    capacity: usize,
    policy: RetentionPolicy,
    request: RetentionRequest,
    dependencies: Option<&CoreRetentionDependencySnapshot>,
) -> Result<RetentionReceipt, StoreError> {
    if dependencies.is_none() && crate::retention::dependency_sensitive_core_target(request.target)
        || dependencies.is_some()
            && !crate::retention::dependency_sensitive_core_target(request.target)
    {
        return Err(StoreError::InvalidTransition);
    }
    let watermarks = validate_request(policy, request)?;
    let watermark = target_watermark(request.target, watermarks);
    let before = store.retention_epoch()?;
    let candidates = candidates(
        store,
        request.target,
        watermark,
        target_window_open(request.target, policy, request.now_tick),
        capacity,
        dependencies,
    )?;
    let after = store.retention_epoch()?;
    if before != after {
        return Err(StoreError::Conflict);
    }
    let examined = candidates.active;
    let eligible = candidates.eligible.len();
    let rows = candidates
        .eligible
        .into_iter()
        .take(request.max_records)
        .map(|candidate| ArchivedRetentionRow {
            namespace: candidate.row.namespace,
            key: candidate.row.key,
            version: candidate.row.version,
            payload: candidate.row.payload,
            retention_tick: candidate.tick,
            owner_execution: candidate.owner_execution,
        })
        .collect::<Vec<_>>();
    if rows.is_empty() {
        return Ok(RetentionReceipt {
            target: request.target,
            watermark_tick: watermark,
            examined,
            archived: 0,
            pruned: 0,
            protected: examined.saturating_sub(eligible),
            observations_created: 0,
            observations_reused: 0,
            stale_observations_removed: 0,
            archive_id: None,
        });
    }
    let mut batch = rows.len();
    let archive = loop {
        let archive = build_archive(
            request.target,
            request.now_tick,
            watermark,
            rows.iter().take(batch).cloned().collect(),
        )?;
        match store.retention_commit_archive(&archive, after) {
            Ok(()) => break archive,
            Err(StoreError::CapacityExceeded | StoreError::PayloadTooLarge) if batch > 1 => {
                batch = (batch / 2).max(1);
            }
            Err(problem) => return Err(problem),
        }
    };
    Ok(RetentionReceipt {
        target: request.target,
        watermark_tick: watermark,
        examined,
        archived: archive.rows.len(),
        pruned: archive.rows.len(),
        protected: examined.saturating_sub(eligible),
        observations_created: 0,
        observations_reused: 0,
        stale_observations_removed: 0,
        archive_id: Some(archive.archive_id),
    })
}

fn durable_archives(
    store: &dyn DurableRetentionBackend,
    target: RetentionTarget,
    max: usize,
) -> Result<Vec<RetentionArchive>, StoreError> {
    if max == 0 || max > mainframe_env_store_api::MAX_RETENTION_BATCH {
        return Err(StoreError::CapacityExceeded);
    }
    store.retention_load_archives(Some(target), max, None)
}

fn durable_prune_archives(
    store: &dyn DurableRetentionBackend,
    policy: RetentionPolicy,
    request: RetentionArchivePruneRequest,
) -> Result<RetentionArchivePruneOutcome, StoreError> {
    let watermark = policy.watermarks(request.now_tick)?.archive_tick;
    if request.now_tick == 0
        || request.max_records == 0
        || request.max_records > policy.max_batch
        || request.max_records > mainframe_env_store_api::MAX_RETENTION_BATCH
    {
        return Err(StoreError::InvalidTransition);
    }
    if request.now_tick < policy.archive_ticks {
        return if request.authorized_oversized_archive_id.is_some() {
            Err(StoreError::Conflict)
        } else {
            Ok(RetentionArchivePruneOutcome::Pruned(
                RetentionArchivePruneReceipt {
                    pruned_source_rows: 0,
                    archive_ids: Vec::new(),
                    oversized_authorization_used: false,
                },
            ))
        };
    }
    let archives = store.retention_load_archives(None, request.max_records, Some(watermark))?;
    if archives.is_empty() {
        return if request.authorized_oversized_archive_id.is_some() {
            Err(StoreError::Conflict)
        } else {
            Ok(RetentionArchivePruneOutcome::Pruned(
                RetentionArchivePruneReceipt {
                    pruned_source_rows: 0,
                    archive_ids: Vec::new(),
                    oversized_authorization_used: false,
                },
            ))
        };
    }
    let first = archives.first().ok_or(StoreError::Conflict)?;
    let oversized = first.rows.len() > request.max_records;
    match (
        oversized,
        request.authorized_oversized_archive_id.as_deref(),
    ) {
        (true, None) => {
            return Ok(RetentionArchivePruneOutcome::AuthorizationRequired {
                archive_id: first.archive_id.clone(),
                source_rows: first.rows.len(),
                requested_max_records: request.max_records,
            });
        }
        (true, Some(authorized)) if authorized == first.archive_id.as_str() => {}
        (false, None) => {}
        (true, Some(_)) | (false, Some(_)) => return Err(StoreError::Conflict),
    }
    let count = archives.iter().try_fold(0usize, |count, archive| {
        count
            .checked_add(archive.rows.len())
            .ok_or(StoreError::CapacityExceeded)
    })?;
    let archive_ids = archives
        .iter()
        .map(|archive| archive.archive_id.clone())
        .collect();
    store.retention_delete_archives(&archives)?;
    Ok(RetentionArchivePruneOutcome::Pruned(
        RetentionArchivePruneReceipt {
            pruned_source_rows: count,
            archive_ids,
            oversized_authorization_used: oversized,
        },
    ))
}

fn durable_reconcile_retention_age(
    store: &dyn DurableRetentionBackend,
    request: RetentionAgeReconciliation,
    now_tick: u64,
) -> Result<RetentionReconciliationReceipt, StoreError> {
    if crate::retention::provider_owned_target(request.target) {
        return Err(StoreError::InvalidTransition);
    }
    if now_tick == 0
        || request.namespace.is_empty()
        || request.key.is_empty()
        || request.expected_version == 0
    {
        return Err(StoreError::InvalidTransition);
    }
    let before = store.retention_epoch()?;
    let namespace = match request.target {
        RetentionTarget::LifecycleEvents if request.namespace.starts_with("durable-event:") => {
            request.namespace.as_str()
        }
        RetentionTarget::Audit if request.namespace == AUDIT_NAMESPACE => {
            request.namespace.as_str()
        }
        target if target_namespace(target) == Some(request.namespace.as_str()) => {
            request.namespace.as_str()
        }
        _ => return Err(StoreError::InvalidTransition),
    };
    let row = store
        .get_provider_state(namespace, &request.key)?
        .ok_or(StoreError::NotFound)?;
    if row.version != request.expected_version {
        return Err(StoreError::Conflict);
    }
    let owner_execution = match request.target {
        RetentionTarget::TerminalExecutions => {
            if request.owner_execution.is_some() {
                return Err(StoreError::InvalidTransition);
            }
            let execution = decode_execution(&row.payload, row.version)?;
            if !execution.state.terminal() || execution.terminal_tick.is_some() {
                return Err(StoreError::Conflict);
            }
            Some(execution.execution_id)
        }
        RetentionTarget::TerminalWork => {
            if request.owner_execution.is_some() {
                return Err(StoreError::InvalidTransition);
            }
            let work = decode_work(&row.payload)?;
            if !work.state.terminal() || work.terminal_tick.is_some() {
                return Err(StoreError::Conflict);
            }
            Some(work.execution_id)
        }
        RetentionTarget::DeliveredOutbox => {
            if request.owner_execution.is_some() {
                return Err(StoreError::InvalidTransition);
            }
            let outbox = decode_outbox(&row.payload, row.version)?;
            if !outbox.delivered || outbox.delivered_tick.is_some() {
                return Err(StoreError::Conflict);
            }
            Some(outbox.execution_id)
        }
        RetentionTarget::Db2Replay
        | RetentionTarget::ImsReplay
        | RetentionTarget::MqReplay
        | RetentionTarget::CicsReplay => {
            let owner = request
                .owner_execution
                .as_ref()
                .ok_or(StoreError::InvalidTransition)?;
            if !execution_is_prunable(store, owner.as_str())? {
                return Err(StoreError::InvalidTransition);
            }
            validate_replay_reconciliation(store, &row.key, owner)?;
            let already_aged = if request.target == RetentionTarget::CicsReplay {
                cics_replay_metadata(&row.payload)?.is_some()
            } else {
                replay_metadata(
                    &row.payload,
                    &row.key,
                    replay_schema(request.target).ok_or(StoreError::InvalidTransition)?,
                )?
                .is_some()
            };
            if already_aged {
                return Err(StoreError::Conflict);
            }
            Some(owner.clone())
        }
        RetentionTarget::ResolvedEffects => {
            if request.owner_execution.is_some() {
                return Err(StoreError::InvalidTransition);
            }
            let key = IdempotencyKey::new(&row.key, InvocationLimits::default())
                .map_err(|_| StoreError::IncompatibleVersion)?;
            let effect = decode_effect(&key, &row.payload)?;
            if !matches!(effect.state, EffectState::Completed | EffectState::Failed)
                || effect.resolved_tick.is_some()
            {
                return Err(StoreError::Conflict);
            }
            Some(effect.execution_id)
        }
        RetentionTarget::LifecycleEvents => {
            if request.owner_execution.is_some() {
                return Err(StoreError::InvalidTransition);
            }
            let event = decode_event(&row.payload)?;
            if event.tick != 0
                || request.namespace != format!("durable-event:{}", event.execution_id)
                || row.key != format!("{:020}", event.sequence)
            {
                return Err(StoreError::Conflict);
            }
            Some(event.execution_id)
        }
        RetentionTarget::Audit => {
            if request.owner_execution.is_some() {
                return Err(StoreError::InvalidTransition);
            }
            let audit = decode_audit(&row.payload)?;
            if audit.observed_tick != 0 {
                return Err(StoreError::Conflict);
            }
            Some(audit.execution_id)
        }
        RetentionTarget::RacfEvidence
        | RetentionTarget::DatasetReplay
        | RetentionTarget::CicsUnitOfWork
        | RetentionTarget::CobolLifecycle
        | RetentionTarget::SpoolJobs
        | RetentionTarget::ConsoleLog => {
            return Err(StoreError::InvalidTransition);
        }
    };
    let after = store.retention_epoch()?;
    if before != after {
        return Err(StoreError::Conflict);
    }
    let (observation_version, reconciled_tick) = store.retention_commit_observation(
        RetentionObservation {
            target: request.target,
            namespace: row.namespace.clone(),
            key: row.key.clone(),
            source_version: row.version,
            source_digest: crate::retention::source_digest(&row.payload),
            observed_tick: now_tick,
            owner_execution,
        },
        row.clone(),
        after,
    )?;
    Ok(RetentionReconciliationReceipt {
        target: request.target,
        namespace: request.namespace,
        key: request.key,
        source_version: row.version,
        observation_version,
        reconciled_tick,
    })
}

fn durable_legacy_rows(
    store: &dyn DurableRetentionBackend,
    capacity: usize,
    target: RetentionTarget,
    max: usize,
) -> Result<Vec<RetentionLegacyRow>, StoreError> {
    if crate::retention::provider_owned_target(target) {
        return Err(StoreError::InvalidTransition);
    }
    if max == 0 || max > mainframe_env_store_api::MAX_RETENTION_BATCH {
        return Err(StoreError::CapacityExceeded);
    }
    if target == RetentionTarget::LifecycleEvents {
        let observations = observations(store, target)?;
        let mut legacy = Vec::new();
        for execution in store.list_provider_state("durable-execution", capacity)? {
            let execution = decode_execution(&execution.payload, execution.version)?;
            let namespace = format!("durable-event:{}", execution.execution_id);
            for row in store.list_provider_state(&namespace, capacity)? {
                let event = decode_event(&row.payload)?;
                if event.tick == 0 && observed_age(&observations, &row).is_none() {
                    legacy.push(RetentionLegacyRow {
                        target,
                        namespace: row.namespace,
                        key: row.key,
                        source_version: row.version,
                    });
                    if legacy.len() == max {
                        return Ok(legacy);
                    }
                }
            }
        }
        return Ok(legacy);
    }
    let namespace = match target {
        RetentionTarget::Audit => AUDIT_NAMESPACE,
        _ => target_namespace(target).ok_or(StoreError::InvalidTransition)?,
    };
    let rows = store.list_provider_state(namespace, capacity)?;
    let observations = observations(store, target)?;
    let mut legacy = Vec::new();
    for row in rows {
        let missing_age = match target {
            RetentionTarget::TerminalExecutions => {
                let record = decode_execution(&row.payload, row.version)?;
                record.state.terminal()
                    && record.terminal_tick.is_none()
                    && observed_age(&observations, &row).is_none()
            }
            RetentionTarget::TerminalWork => {
                let record = decode_work(&row.payload)?;
                record.state.terminal()
                    && record.terminal_tick.is_none()
                    && observed_age(&observations, &row).is_none()
            }
            RetentionTarget::DeliveredOutbox => {
                let record = decode_outbox(&row.payload, row.version)?;
                record.delivered
                    && record.delivered_tick.is_none()
                    && observed_age(&observations, &row).is_none()
            }
            RetentionTarget::ResolvedEffects => {
                let key = IdempotencyKey::new(&row.key, InvocationLimits::default())
                    .map_err(|_| StoreError::IncompatibleVersion)?;
                let record = decode_effect(&key, &row.payload)?;
                matches!(record.state, EffectState::Completed | EffectState::Failed)
                    && record.resolved_tick.is_none()
                    && observed_age(&observations, &row).is_none()
            }
            RetentionTarget::Db2Replay | RetentionTarget::ImsReplay | RetentionTarget::MqReplay => {
                replay_metadata(
                    &row.payload,
                    &row.key,
                    replay_schema(target).ok_or(StoreError::InvalidTransition)?,
                )?
                .is_none()
                    && observed_age(&observations, &row).is_none()
            }
            RetentionTarget::CicsReplay => {
                cics_replay_metadata(&row.payload)?.is_none()
                    && observed_age(&observations, &row).is_none()
            }
            RetentionTarget::Audit => {
                decode_audit(&row.payload)?.observed_tick == 0
                    && observed_age(&observations, &row).is_none()
            }
            RetentionTarget::LifecycleEvents
            | RetentionTarget::RacfEvidence
            | RetentionTarget::DatasetReplay
            | RetentionTarget::CicsUnitOfWork
            | RetentionTarget::CobolLifecycle
            | RetentionTarget::SpoolJobs
            | RetentionTarget::ConsoleLog => {
                return Err(StoreError::InvalidTransition);
            }
        };
        if missing_age {
            legacy.push(RetentionLegacyRow {
                target,
                namespace: row.namespace,
                key: row.key,
                source_version: row.version,
            });
            if legacy.len() == max {
                break;
            }
        }
    }
    Ok(legacy)
}

fn validate_replay_reconciliation(
    store: &dyn ProviderStateStore,
    idempotency_key: &str,
    owner: &mainframe_env_execution_api::ExecutionId,
) -> Result<(), StoreError> {
    let Some(effect_row) = store.get_provider_state("durable-effect", idempotency_key)? else {
        return Ok(());
    };
    let key = IdempotencyKey::new(idempotency_key, InvocationLimits::default())
        .map_err(|_| StoreError::IncompatibleVersion)?;
    let effect = decode_effect(&key, &effect_row.payload)?;
    if effect.execution_id != *owner {
        return Err(StoreError::Conflict);
    }
    if matches!(effect.state, EffectState::Completed | EffectState::Failed) {
        Ok(())
    } else {
        Err(StoreError::InvalidTransition)
    }
}

mod candidates;
use candidates::{candidates, execution_is_prunable};

mod implementations;
