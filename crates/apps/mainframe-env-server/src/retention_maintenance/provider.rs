//! Store-only provider retention planning for offline maintenance.

mod dependencies;
mod operations;
mod rows;

use super::store_problem;
use crate::cobol::retention::{
    CobolRetentionDependency, CobolRetentionState, describe_cobol_retention_row,
};
use crate::console_retention::{
    ConsoleLogOwnerKind, ConsoleLogRetentionState, describe_console_log_row,
};
use mainframe_env_cics::{
    CicsReplayRetentionState, CicsUowDependencyState, describe_cics_replay_row,
    describe_cics_uow_row, validate_cics_container_replay_row,
};
use mainframe_env_dataset::{
    DATASET_REPLAY_NAMESPACE, DatasetLimits, DatasetReplayDependencyState,
    DatasetReplayRetentionState, describe_dataset_replay_row, validate_dataset_replay_effect,
};
use mainframe_env_db2::{
    Db2Limits, Db2ReplayDependency, Db2ReplayRetentionState, describe_db2_replay_row,
};
use mainframe_env_execution_api::{ExecutionId, IdempotencyKey, InvocationLimits, RunUnitId};
use mainframe_env_host_api::HostProblem;
use mainframe_env_ims::{
    ImsLimits, ImsReplayDependency, ImsReplayRetentionState, describe_ims_replay_row,
};
use mainframe_env_mq::{
    MqLimits, MqReplayDependency, MqReplayRetentionState, describe_mq_replay_row,
};
use mainframe_env_racf::{RacfRetentionPolicy, SecurityDatabase, SecurityDatabaseLimits};
use mainframe_env_spool::{SpoolLimits, SpoolRetentionState, describe_spool_retention_row};
use mainframe_env_store_api::{
    EffectDigestFormat, EffectRecord, EffectState, PlatformStore, ProviderRetentionDependency,
    ProviderRetentionObservationDeletion, ProviderRetentionObservationSource, ProviderRetentionRow,
    ProviderStateArchiveDeletion, ProviderStateArchiveDeletionWithCapacity, ProviderStateIdentity,
    ProviderStateRecord, ProviderStateWrite, RetentionAgeReconciliation, RetentionForecast,
    RetentionLegacyRow, RetentionObservation, RetentionObservationProof, RetentionPolicy,
    RetentionReceipt, RetentionReconciliationReceipt, RetentionRequest, RetentionTarget,
    StoreError,
};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

const MAX_SCAN: usize = mainframe_env_store_api::MAX_PROVIDER_STATE_SCAN;

type ObservationMap = BTreeMap<(String, String), (u64, RetentionObservation)>;

pub(crate) struct RetentionPlanner {
    store: Arc<dyn PlatformStore>,
    policy: RetentionPolicy,
    racf: Option<Arc<SecurityDatabase>>,
    racf_migration_pending: bool,
    #[cfg(test)]
    forecast_epoch_hook: Option<Arc<dyn Fn() + Send + Sync>>,
}

pub(crate) struct ProviderPlan {
    pub(crate) expected_epoch: u64,
    pub(crate) watermark_tick: u64,
    pub(crate) active_records: usize,
    pub(crate) eligible_records: usize,
    pub(crate) rows: Vec<ProviderRetentionRow>,
    pub(crate) stale_observations_removed: usize,
}

#[derive(Clone, Copy, Debug, Default)]
struct ObservationCounts {
    examined: usize,
    eligible: usize,
    watermark_tick: u64,
}

#[derive(Clone, Copy, Debug, Default)]
struct ObservationMaintenance {
    created: usize,
    reused: usize,
    stale_removed: usize,
    counts: Option<ObservationCounts>,
}

#[derive(Default)]
struct ProviderSafetyIndex {
    unresolved_executions: BTreeSet<ExecutionId>,
    unresolved_runs: BTreeSet<(ExecutionId, RunUnitId)>,
    execution_clear: BTreeMap<(ExecutionId, Option<RunUnitId>), bool>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ReplayState {
    Legacy,
    Pending,
    Terminal,
}

#[derive(Clone, Debug)]
enum ReplayDependency {
    Core,
    CicsNested { outer_effect_key: String },
    None,
}

struct ReplayView {
    state: ReplayState,
    owner_execution: Option<String>,
    owner_run_unit: Option<String>,
    terminal_tick: Option<u64>,
    request_digest: [u8; 32],
    result_digest: [u8; 32],
    payload_digest: [u8; 32],
    dependency: ReplayDependency,
}

#[derive(Default)]
struct CicsUowIndex {
    known_runs: BTreeSet<String>,
    has_unattributed_uow: bool,
    #[cfg(test)]
    scanned_rows: usize,
}

impl CicsUowIndex {
    fn blocks_undo(&self, run: &str) -> bool {
        self.has_unattributed_uow || self.known_runs.contains(run)
    }
}

#[derive(Default)]
struct CobolReplayIndex {
    runs: BTreeSet<String>,
    has_legacy_replay: bool,
    #[cfg(test)]
    scanned_rows: usize,
}

impl RetentionPlanner {
    pub(crate) fn open(
        store: Arc<dyn PlatformStore>,
        policy: RetentionPolicy,
    ) -> Result<Self, HostProblem> {
        let racf = SecurityDatabase::open_existing_for_retention(
            store.clone(),
            SecurityDatabaseLimits::default(),
        )?;
        let racf_migration_pending = racf.is_none()
            && !SecurityDatabase::legacy_rows_for_retention(
                store.as_ref(),
                SecurityDatabaseLimits::default(),
                1,
            )?
            .is_empty();
        policy.validate().map_err(store_problem)?;
        Ok(Self {
            store,
            policy,
            racf,
            racf_migration_pending,
            #[cfg(test)]
            forecast_epoch_hook: None,
        })
    }

    pub(crate) fn from_existing(
        store: Arc<dyn PlatformStore>,
        policy: RetentionPolicy,
        racf: Option<Arc<SecurityDatabase>>,
    ) -> Result<Self, HostProblem> {
        policy.validate().map_err(store_problem)?;
        Ok(Self {
            store,
            policy,
            racf,
            racf_migration_pending: false,
            #[cfg(test)]
            forecast_epoch_hook: None,
        })
    }

    pub(crate) fn store(&self) -> &Arc<dyn PlatformStore> {
        &self.store
    }

    pub(crate) const fn policy(&self) -> RetentionPolicy {
        self.policy
    }

    pub(crate) fn core_dependencies(
        &self,
    ) -> Result<mainframe_env_store_api::CoreRetentionDependencySnapshot, HostProblem> {
        dependencies::snapshot(self)
    }

    #[cfg(test)]
    pub(super) fn set_forecast_epoch_hook(&mut self, hook: Arc<dyn Fn() + Send + Sync>) {
        self.forecast_epoch_hook = Some(hook);
    }

    #[cfg(test)]
    pub(super) fn run_forecast_epoch_hook(&self) {
        if let Some(hook) = &self.forecast_epoch_hook {
            hook();
        }
    }

    pub(crate) fn forecast(
        &self,
        target: RetentionTarget,
        now_tick: u64,
        growth: u64,
    ) -> Result<RetentionForecast, HostProblem> {
        if target == RetentionTarget::RacfEvidence {
            return self.racf_forecast(now_tick, growth);
        }
        if dependencies::core_target(target) {
            let dependencies = self.core_dependencies()?;
            return self
                .store
                .retention_forecast_with_dependencies(
                    target,
                    self.policy,
                    now_tick,
                    growth,
                    &dependencies,
                )
                .map_err(store_problem);
        }
        if provider_target(target) {
            let plan = self.provider_plan(target, now_tick, 0)?;
            let shared_capacity = self
                .store
                .retention_capacity_health(self.policy)
                .map_err(store_problem)?
                .targets
                .into_iter()
                .find(|entry| entry.target == target)
                .map(|entry| entry.capacity)
                .ok_or(HostProblem::InfrastructureFailure)?;
            let local_capacity = match target {
                RetentionTarget::Db2Replay => Db2Limits::default().max_replays,
                RetentionTarget::ImsReplay => ImsLimits::default().max_replays,
                RetentionTarget::MqReplay => MqLimits::default().max_replays,
                RetentionTarget::DatasetReplay => DatasetLimits::default().max_idempotency,
                RetentionTarget::SpoolJobs => SpoolLimits::default().max_jobs,
                RetentionTarget::ConsoleLog => 65_536,
                _ => shared_capacity,
            };
            let forecast = self
                .store
                .provider_validated_retention_forecast(
                    target,
                    self.policy,
                    now_tick,
                    growth,
                    plan.active_records,
                    plan.rows.len(),
                    local_capacity.min(shared_capacity),
                )
                .map_err(store_problem)?;
            #[cfg(test)]
            self.run_forecast_epoch_hook();
            if self
                .store
                .provider_state_retention_epoch()
                .map_err(store_problem)?
                != plan.expected_epoch
            {
                return Err(HostProblem::IdempotencyConflict);
            }
            return Ok(forecast);
        }
        self.store
            .retention_forecast(target, self.policy, now_tick, growth)
            .map_err(store_problem)
    }

    pub(crate) fn archive_and_prune(
        &self,
        target: RetentionTarget,
        now_tick: u64,
        max_records: usize,
    ) -> Result<RetentionReceipt, HostProblem> {
        if max_records == 0 || max_records > self.policy.max_batch {
            return Err(HostProblem::Malformed);
        }
        if target == RetentionTarget::RacfEvidence {
            return self.racf_archive(now_tick, max_records);
        }
        if dependencies::core_target(target) {
            let dependencies = self.core_dependencies()?;
            return self
                .store
                .archive_and_prune_with_dependencies(
                    self.policy,
                    RetentionRequest {
                        target,
                        now_tick,
                        max_records,
                    },
                    &dependencies,
                )
                .map_err(store_problem);
        }
        if provider_target(target) {
            let mut plan = self.provider_plan(target, now_tick, max_records)?;
            let eligible = plan.eligible_records;
            if plan.stale_observations_removed != 0 {
                return Ok(RetentionReceipt {
                    target,
                    watermark_tick: plan.watermark_tick,
                    examined: plan.active_records,
                    archived: 0,
                    pruned: 0,
                    protected: plan.active_records.saturating_sub(eligible),
                    archive_id: None,
                    observations_created: 0,
                    observations_reused: 0,
                    stale_observations_removed: plan.stale_observations_removed,
                });
            }
            let remaining = max_records.saturating_sub(plan.stale_observations_removed);
            plan.rows.truncate(remaining);
            if plan.rows.is_empty() {
                return Ok(RetentionReceipt {
                    target,
                    watermark_tick: plan.watermark_tick,
                    examined: plan.active_records,
                    archived: 0,
                    pruned: 0,
                    protected: plan.active_records.saturating_sub(eligible),
                    archive_id: None,
                    observations_created: 0,
                    observations_reused: 0,
                    stale_observations_removed: plan.stale_observations_removed,
                });
            }
            if target == RetentionTarget::CicsReplay {
                for outer in &plan.rows {
                    let Some(private) = self
                        .store
                        .get_provider_state("cics-container-replay-v1", &outer.row.key)
                        .map_err(store_problem)?
                    else {
                        continue;
                    };
                    let effect = self
                        .effect(&outer.row.key)?
                        .ok_or(HostProblem::InfrastructureFailure)?;
                    let owner = outer
                        .owner_execution
                        .as_ref()
                        .ok_or(HostProblem::InfrastructureFailure)?;
                    let execution = self
                        .store
                        .get_execution(owner)
                        .map_err(store_problem)?
                        .ok_or(HostProblem::InfrastructureFailure)?;
                    validate_cics_container_replay_row(
                        &private,
                        &outer.row,
                        &effect,
                        &execution,
                        Default::default(),
                    )
                    .map_err(|_| HostProblem::InfrastructureFailure)?;
                    let capacity = self
                        .store
                        .get_provider_state("cics-container-capacity-v1", "global")
                        .map_err(store_problem)?
                        .ok_or(HostProblem::InfrastructureFailure)?;
                    let mut payload: serde_json::Value = serde_json::from_slice(&capacity.payload)
                        .map_err(|_| HostProblem::InfrastructureFailure)?;
                    let count = payload["replays"]
                        .as_u64()
                        .and_then(|count| count.checked_sub(1))
                        .ok_or(HostProblem::InfrastructureFailure)?;
                    payload["replays"] = count.into();
                    let replacement = ProviderStateWrite {
                        record: ProviderStateRecord {
                            version: capacity
                                .version
                                .checked_add(1)
                                .ok_or(HostProblem::ResourceExhausted)?,
                            payload: serde_json::to_vec(&payload)
                                .map_err(|_| HostProblem::InfrastructureFailure)?,
                            ..capacity.clone()
                        },
                        expected_version: Some(capacity.version),
                    };
                    let archive = self
                        .store
                        .archive_provider_state_deletion_with_capacity(
                            ProviderStateArchiveDeletionWithCapacity {
                                deletion: ProviderStateArchiveDeletion {
                                    expected_epoch: plan.expected_epoch,
                                    target,
                                    archived_tick: now_tick,
                                    watermark_tick: plan.watermark_tick,
                                    rows: vec![ProviderRetentionRow {
                                        row: private,
                                        observation: None,
                                        ..outer.clone()
                                    }],
                                },
                                outer_receipts: vec![outer.row.clone()],
                                capacity_source: capacity,
                                capacity_replacement: replacement,
                            },
                        )
                        .map_err(store_problem)?;
                    return Ok(RetentionReceipt {
                        target,
                        watermark_tick: plan.watermark_tick,
                        examined: plan.active_records,
                        archived: archive.rows.len(),
                        pruned: archive.rows.len(),
                        protected: plan.active_records.saturating_sub(eligible),
                        archive_id: Some(archive.archive_id),
                        observations_created: 0,
                        observations_reused: 0,
                        stale_observations_removed: plan.stale_observations_removed,
                    });
                }
            }
            let mut batch = plan.rows.len();
            let archive = loop {
                let request = ProviderStateArchiveDeletion {
                    expected_epoch: plan.expected_epoch,
                    target,
                    archived_tick: now_tick,
                    watermark_tick: plan.watermark_tick,
                    rows: plan.rows.iter().take(batch).cloned().collect(),
                };
                match self.store.archive_provider_state_deletion(request) {
                    Ok(archive) => break archive,
                    Err(StoreError::CapacityExceeded | StoreError::PayloadTooLarge)
                        if batch > 1 =>
                    {
                        batch = (batch / 2).max(1);
                    }
                    Err(problem) => return Err(store_problem(problem)),
                }
            };
            let observations_reused = archive
                .rows
                .iter()
                .filter(|archived| {
                    plan.rows.iter().any(|candidate| {
                        candidate.observation.is_some()
                            && candidate.row.namespace == archived.namespace
                            && candidate.row.key == archived.key
                    })
                })
                .count();
            return Ok(RetentionReceipt {
                target,
                watermark_tick: plan.watermark_tick,
                examined: plan.active_records,
                archived: archive.rows.len(),
                pruned: archive.rows.len(),
                protected: plan.active_records.saturating_sub(eligible),
                archive_id: Some(archive.archive_id),
                observations_created: 0,
                observations_reused,
                stale_observations_removed: plan.stale_observations_removed,
            });
        }
        self.store
            .archive_and_prune(
                self.policy,
                RetentionRequest {
                    target,
                    now_tick,
                    max_records,
                },
            )
            .map_err(store_problem)
    }

    pub(crate) fn legacy_rows(
        &self,
        target: RetentionTarget,
        max: usize,
    ) -> Result<Vec<RetentionLegacyRow>, HostProblem> {
        if max == 0 || max > mainframe_env_store_api::MAX_RETENTION_BATCH {
            return Err(HostProblem::Malformed);
        }
        if target == RetentionTarget::RacfEvidence {
            if self.racf_migration_pending {
                return SecurityDatabase::legacy_rows_for_retention(
                    self.store.as_ref(),
                    SecurityDatabaseLimits::default(),
                    max,
                );
            }
            return self.racf_legacy_rows(max);
        }
        if provider_target(target) {
            return self.provider_legacy_rows(target, max);
        }
        self.store
            .retention_legacy_rows(target, max)
            .map_err(store_problem)
    }

    pub(crate) fn reconcile(
        &self,
        request: RetentionAgeReconciliation,
        now_tick: u64,
    ) -> Result<RetentionReconciliationReceipt, HostProblem> {
        if request.target == RetentionTarget::RacfEvidence {
            if self.racf_migration_pending {
                return Err(HostProblem::UnsupportedCapability {
                    capability: "racf-retention-current-schema".into(),
                    detail: "legacy RACF rows require normal-startup migration before age reconciliation"
                        .into(),
                });
            }
            return self.reconcile_racf(request, now_tick);
        }
        if provider_target(request.target) {
            return self.reconcile_provider(request, now_tick);
        }
        self.store
            .reconcile_retention_age(request, now_tick)
            .map_err(store_problem)
    }

    fn observations(
        &self,
        target: RetentionTarget,
    ) -> Result<Vec<(u64, RetentionObservation)>, HostProblem> {
        let mut rows = Vec::new();
        let mut after = None;
        loop {
            let page = self
                .store
                .provider_retention_observation_page(
                    target,
                    after.as_ref(),
                    mainframe_env_store_api::MAX_RETENTION_BATCH,
                )
                .map_err(store_problem)?;
            if page.is_empty() {
                break;
            }
            after = page.last().map(|(_, row)| ProviderStateIdentity {
                namespace: row.namespace.clone(),
                key: row.key.clone(),
            });
            rows.extend(page);
            if rows.len() > MAX_SCAN {
                return Err(HostProblem::ResourceExhausted);
            }
        }
        Ok(rows)
    }

    fn bounded_rows(
        &self,
        target: RetentionTarget,
    ) -> Result<Vec<ProviderStateRecord>, HostProblem> {
        if target == RetentionTarget::CicsUnitOfWork {
            let mut rows = self
                .store
                .list_provider_state("cics-uow", MAX_SCAN)
                .map_err(store_problem)?;
            if rows.len() == MAX_SCAN {
                return Err(HostProblem::ResourceExhausted);
            }
            let remaining = MAX_SCAN.saturating_sub(rows.len());
            let undo = self
                .store
                .list_provider_state("cics-uow-undo", remaining)
                .map_err(store_problem)?;
            if undo.len() == remaining {
                return Err(HostProblem::ResourceExhausted);
            }
            rows.extend(undo);
            rows.sort_by(|left, right| {
                left.namespace
                    .cmp(&right.namespace)
                    .then_with(|| left.key.cmp(&right.key))
            });
            return Ok(rows);
        }
        let rows = match target {
            RetentionTarget::Db2Replay => self.store.list_provider_state("db2-v1-replay", MAX_SCAN),
            RetentionTarget::ImsReplay => self.store.list_provider_state("ims-v1-replay", MAX_SCAN),
            RetentionTarget::MqReplay => self.store.list_provider_state("mq-v1-replay", MAX_SCAN),
            RetentionTarget::DatasetReplay => self
                .store
                .list_provider_state(DATASET_REPLAY_NAMESPACE, MAX_SCAN),
            RetentionTarget::CicsReplay => self
                .store
                .list_provider_state("cics-effect-replay-v1", MAX_SCAN),
            RetentionTarget::CobolLifecycle => {
                self.store.list_provider_state_prefix("cobol-", MAX_SCAN)
            }
            RetentionTarget::SpoolJobs => self.store.list_provider_state("jes-spool", MAX_SCAN),
            RetentionTarget::ConsoleLog => self.store.list_provider_state("console-log", MAX_SCAN),
            RetentionTarget::CicsUnitOfWork => unreachable!(),
            _ => return Err(HostProblem::Unsupported),
        }
        .map_err(store_problem)?;
        if rows.len() == MAX_SCAN {
            return Err(HostProblem::ResourceExhausted);
        }
        Ok(rows)
    }

    pub(crate) fn provider_plan(
        &self,
        target: RetentionTarget,
        now_tick: u64,
        cleanup_budget: usize,
    ) -> Result<ProviderPlan, HostProblem> {
        let watermarks = self.policy.watermarks(now_tick).map_err(store_problem)?;
        let (watermark_tick, window_open) =
            provider_watermark(target, self.policy, now_tick, watermarks)?;
        let mut expected_epoch = self
            .store
            .provider_state_retention_epoch()
            .map_err(store_problem)?;
        let source = self.bounded_rows(target)?;
        let active_records = source.len();
        let observations = self.observations(target)?;
        let observation_map = observations
            .iter()
            .map(|(version, row)| {
                (
                    (row.namespace.clone(), row.key.clone()),
                    (*version, row.clone()),
                )
            })
            .collect::<ObservationMap>();
        let required_cobol_rows = if target == RetentionTarget::CobolLifecycle {
            cobol_required_rows(&source)?
        } else {
            BTreeSet::new()
        };
        let cics_uow_index = if target == RetentionTarget::CicsUnitOfWork {
            self.cics_uow_index(&source)?
        } else {
            CicsUowIndex::default()
        };
        let cobol_replay_index = if target == RetentionTarget::CobolLifecycle {
            self.cobol_replay_index(&source)?
        } else {
            CobolReplayIndex::default()
        };
        let (nested_cics_effects, unattributed_nested_cics) =
            if target == RetentionTarget::CicsUnitOfWork {
                dependencies::nested_cics_effects(self, expected_epoch)?
            } else {
                (BTreeSet::new(), false)
            };
        let mut rows = Vec::new();
        for row in source.iter().cloned() {
            if required_cobol_rows.contains(&(row.namespace.clone(), row.key.clone())) {
                continue;
            }
            let candidate = self.candidate(
                target,
                &row,
                &observation_map,
                &nested_cics_effects,
                unattributed_nested_cics,
                &cics_uow_index,
                &cobol_replay_index,
            )?;
            if let Some(candidate) = candidate
                && window_open
                && candidate.retention_tick <= watermark_tick
            {
                rows.push(candidate);
            }
        }
        rows = self.retain_safe_provider_candidates(rows)?;
        rows.sort_by(|left, right| {
            left.retention_tick
                .cmp(&right.retention_tick)
                .then_with(|| left.row.namespace.cmp(&right.row.namespace))
                .then_with(|| left.row.key.cmp(&right.row.key))
        });
        let eligible_records = rows.len();
        let stale_observations_removed = if cleanup_budget == 0 {
            0
        } else {
            let cleaned =
                self.clean_observations(target, expected_epoch, &source, cleanup_budget)?;
            expected_epoch = cleaned.1;
            cleaned.0
        };
        if stale_observations_removed != 0 {
            return Ok(ProviderPlan {
                expected_epoch,
                watermark_tick,
                active_records,
                eligible_records,
                rows: Vec::new(),
                stale_observations_removed,
            });
        }
        if self
            .store
            .provider_state_retention_epoch()
            .map_err(store_problem)?
            != expected_epoch
        {
            return Err(HostProblem::IdempotencyConflict);
        }
        Ok(ProviderPlan {
            expected_epoch,
            watermark_tick,
            active_records,
            eligible_records,
            rows,
            stale_observations_removed,
        })
    }

    fn provider_safety_index(&self) -> Result<ProviderSafetyIndex, HostProblem> {
        let unresolved = self
            .store
            .unresolved_effects(MAX_SCAN)
            .map_err(store_problem)?;
        if unresolved.len() == MAX_SCAN {
            return Err(HostProblem::ResourceExhausted);
        }
        let mut index = ProviderSafetyIndex::default();
        for effect in unresolved {
            index
                .unresolved_executions
                .insert(effect.execution_id.clone());
            index
                .unresolved_runs
                .insert((effect.execution_id, effect.run_unit_id));
        }
        Ok(index)
    }

    fn retain_safe_provider_candidates(
        &self,
        rows: Vec<ProviderRetentionRow>,
    ) -> Result<Vec<ProviderRetentionRow>, HostProblem> {
        if !rows.iter().any(provider_candidate_needs_execution_safety) {
            return Ok(rows);
        }
        let mut safety = self.provider_safety_index()?;
        let mut safe_rows = Vec::with_capacity(rows.len());
        for candidate in rows {
            if self.provider_candidate_is_clear(&candidate, &mut safety)? {
                safe_rows.push(candidate);
            }
        }
        Ok(safe_rows)
    }

    fn provider_candidate_is_clear(
        &self,
        candidate: &ProviderRetentionRow,
        safety: &mut ProviderSafetyIndex,
    ) -> Result<bool, HostProblem> {
        match &candidate.dependency {
            ProviderRetentionDependency::None | ProviderRetentionDependency::DirectProduct => {
                Ok(true)
            }
            ProviderRetentionDependency::CoreEffect { .. } => {
                let (Some(owner), Some(run)) = (
                    candidate.owner_execution.as_ref(),
                    candidate.owner_run_unit.as_ref(),
                ) else {
                    return Ok(false);
                };
                self.provider_execution_is_clear(owner, Some(run), safety)
            }
            ProviderRetentionDependency::CicsNested {
                required_executions,
                ..
            } => {
                let (Some(owner), Some(run)) = (
                    candidate.owner_execution.as_ref(),
                    candidate.owner_run_unit.as_ref(),
                ) else {
                    return Ok(false);
                };
                for required in std::iter::once(owner).chain(required_executions) {
                    if !self.provider_execution_is_clear(required, Some(run), safety)? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
            ProviderRetentionDependency::ProviderGraph {
                required_executions,
                ..
            } => {
                let (Some(owner), Some(run)) = (
                    candidate.owner_execution.as_ref(),
                    candidate.owner_run_unit.as_ref(),
                ) else {
                    return Ok(false);
                };
                if !self.provider_execution_is_clear(owner, Some(run), safety)? {
                    return Ok(false);
                }
                for required in required_executions {
                    if !self.provider_execution_is_clear(required, None, safety)? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
        }
    }

    fn provider_execution_is_clear(
        &self,
        owner: &ExecutionId,
        expected_run: Option<&RunUnitId>,
        safety: &mut ProviderSafetyIndex,
    ) -> Result<bool, HostProblem> {
        let identity = (owner.clone(), expected_run.cloned());
        if let Some(clear) = safety.execution_clear.get(&identity) {
            return Ok(*clear);
        }
        let execution = self.store.get_execution(owner).map_err(store_problem)?;
        let checkpoint = self.store.get_checkpoint(owner).map_err(store_problem)?;
        let unresolved = expected_run.map_or_else(
            || safety.unresolved_executions.contains(owner),
            |run| {
                safety
                    .unresolved_runs
                    .contains(&(owner.clone(), run.clone()))
            },
        );
        let clear = execution.is_some_and(|execution| {
            execution.state.terminal()
                && expected_run.is_none_or(|run| execution.run_unit_id == *run)
        }) && checkpoint.is_none()
            && !unresolved;
        safety.execution_clear.insert(identity, clear);
        Ok(clear)
    }
}

fn provider_candidate_needs_execution_safety(candidate: &ProviderRetentionRow) -> bool {
    !matches!(
        candidate.dependency,
        ProviderRetentionDependency::None | ProviderRetentionDependency::DirectProduct
    )
}

fn provider_target(target: RetentionTarget) -> bool {
    matches!(
        target,
        RetentionTarget::Db2Replay
            | RetentionTarget::ImsReplay
            | RetentionTarget::MqReplay
            | RetentionTarget::DatasetReplay
            | RetentionTarget::CicsReplay
            | RetentionTarget::CicsUnitOfWork
            | RetentionTarget::CobolLifecycle
            | RetentionTarget::SpoolJobs
            | RetentionTarget::ConsoleLog
    )
}

fn replay_namespace(target: RetentionTarget) -> Option<&'static str> {
    match target {
        RetentionTarget::Db2Replay => Some("db2-v1-replay"),
        RetentionTarget::ImsReplay => Some("ims-v1-replay"),
        RetentionTarget::MqReplay => Some("mq-v1-replay"),
        RetentionTarget::DatasetReplay => Some(DATASET_REPLAY_NAMESPACE),
        RetentionTarget::CicsReplay => Some("cics-effect-replay-v1"),
        _ => None,
    }
}

fn cobol_namespace(namespace: &str) -> bool {
    matches!(
        namespace,
        "cobol-call-replay@1"
            | "cobol-call-protocol@1"
            | "cobol-call-protocol@2"
            | "cobol-run-state@1"
            | "cobol-cancel@1"
    ) || namespace.starts_with("cobol-instance@1:")
}

fn target_capacity(
    store: &Arc<dyn PlatformStore>,
    policy: RetentionPolicy,
    target: RetentionTarget,
) -> Result<usize, HostProblem> {
    store
        .retention_capacity_health(policy)
        .map_err(store_problem)?
        .targets
        .into_iter()
        .find(|entry| entry.target == target)
        .map(|entry| entry.capacity)
        .ok_or(HostProblem::InfrastructureFailure)
}

fn provider_watermark(
    target: RetentionTarget,
    policy: RetentionPolicy,
    now_tick: u64,
    watermarks: mainframe_env_store_api::RetentionWatermarks,
) -> Result<(u64, bool), HostProblem> {
    Ok(match target {
        RetentionTarget::Db2Replay
        | RetentionTarget::ImsReplay
        | RetentionTarget::MqReplay
        | RetentionTarget::DatasetReplay
        | RetentionTarget::CicsReplay
        | RetentionTarget::CicsUnitOfWork => (
            watermarks.idempotency_tick,
            now_tick >= policy.idempotency_ticks,
        ),
        RetentionTarget::SpoolJobs | RetentionTarget::CobolLifecycle => (
            watermarks.lifecycle_tick.min(watermarks.idempotency_tick),
            now_tick >= policy.lifecycle_ticks.max(policy.idempotency_ticks),
        ),
        RetentionTarget::ConsoleLog => (
            watermarks.lifecycle_tick,
            now_tick >= policy.lifecycle_ticks,
        ),
        _ => return Err(HostProblem::Unsupported),
    })
}

fn exact_observation<'a>(
    observations: &'a ObservationMap,
    row: &ProviderStateRecord,
    digest: [u8; 32],
) -> Option<(u64, &'a RetentionObservation)> {
    observations
        .get(&(row.namespace.clone(), row.key.clone()))
        .filter(|(_, observation)| {
            observation.source_version == row.version
                && observation.source_digest == digest
                && observation.observed_tick != 0
        })
        .map(|(version, observation)| (*version, observation))
}

fn parse_execution(value: Option<&str>) -> Result<ExecutionId, HostProblem> {
    ExecutionId::new(
        value.ok_or(HostProblem::InfrastructureFailure)?,
        InvocationLimits::default(),
    )
    .map_err(|_| HostProblem::InfrastructureFailure)
}

fn parse_run(value: Option<&str>) -> Result<RunUnitId, HostProblem> {
    RunUnitId::new(
        value.ok_or(HostProblem::InfrastructureFailure)?,
        InvocationLimits::default(),
    )
    .map_err(|_| HostProblem::InfrastructureFailure)
}

fn payload_digest(payload: &[u8]) -> [u8; 32] {
    Sha256::digest(payload).into()
}

fn cobol_required_rows(
    source: &[ProviderStateRecord],
) -> Result<BTreeSet<(String, String)>, HostProblem> {
    let mut rows = BTreeSet::new();
    for row in source {
        let descriptor =
            describe_cobol_retention_row(row).map_err(|_| HostProblem::InfrastructureFailure)?;
        for dependency in descriptor.dependencies {
            if let CobolRetentionDependency::ProviderRow { namespace, key } = dependency {
                rows.insert((namespace, key));
            }
        }
    }
    Ok(rows)
}

#[cfg(test)]
mod safety_tests;

#[cfg(test)]
mod container_tests;

#[cfg(test)]
mod selected_mq_tests;
