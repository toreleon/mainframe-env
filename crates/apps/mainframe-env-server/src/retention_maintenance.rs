//! Offline retention authority that opens only durable state.

pub(crate) mod provider;

use mainframe_env_host_api::HostProblem;
use mainframe_env_store_api::{
    PlatformStore, RetentionAgeReconciliation, RetentionArchivePruneOutcome,
    RetentionArchivePruneRequest, RetentionForecast, RetentionLegacyRow, RetentionPolicy,
    RetentionReceipt, RetentionReconciliationReceipt, RetentionTarget, StoreError,
};
use provider::RetentionPlanner;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

/// Maintenance-only retention facade.
///
/// Construction validates codecs and opens the optional RACF aggregate read-only. It does not
/// advance logical time, initialize providers, rebuild caches, or recover serving state.
pub struct RetentionMaintenance {
    planner: RetentionPlanner,
}

/// One explicit maintenance pass sharing a single durable clock observation.
pub struct RetentionMaintenancePass<'a> {
    planner: &'a RetentionPlanner,
    now_tick: u64,
}

impl RetentionMaintenance {
    /// Open a maintenance-only view over an already migrated platform store.
    pub fn open(
        store: Arc<dyn PlatformStore>,
        policy: RetentionPolicy,
    ) -> Result<Self, HostProblem> {
        policy.validate().map_err(store_problem)?;
        Ok(Self {
            planner: RetentionPlanner::open(store, policy)?,
        })
    }

    /// Begin one explicit action using a lazily sampled, persisted logical tick.
    pub fn begin_pass(&self) -> Result<RetentionMaintenancePass<'_>, HostProblem> {
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| HostProblem::InfrastructureFailure)?
            .as_millis();
        let observed = u64::try_from(millis).map_err(|_| HostProblem::ResourceExhausted)?;
        let now_tick = self
            .planner
            .store()
            .advance_logical_clock(observed.max(1))
            .map_err(store_problem)?;
        if now_tick == 0 {
            return Err(HostProblem::InfrastructureFailure);
        }
        Ok(RetentionMaintenancePass {
            planner: &self.planner,
            now_tick,
        })
    }

    /// List protected legacy rows without advancing the durable clock.
    pub fn legacy_rows(
        &self,
        target: RetentionTarget,
        max_records: usize,
    ) -> Result<Vec<RetentionLegacyRow>, HostProblem> {
        self.planner.legacy_rows(target, max_records)
    }
}

impl RetentionMaintenancePass<'_> {
    /// Tick shared by every operation in this action.
    #[must_use]
    pub const fn now_tick(&self) -> u64 {
        self.now_tick
    }

    /// Forecast one target at this pass's stable observation tick.
    pub fn forecast(
        &self,
        target: RetentionTarget,
        observed_growth_per_tick: u64,
    ) -> Result<RetentionForecast, HostProblem> {
        self.planner
            .forecast(target, self.now_tick, observed_growth_per_tick)
    }

    /// Archive and prune one target at this pass's stable observation tick.
    pub fn archive_and_prune(
        &self,
        target: RetentionTarget,
        max_records: usize,
    ) -> Result<RetentionReceipt, HostProblem> {
        self.planner
            .archive_and_prune(target, self.now_tick, max_records)
    }

    /// Prune expired archives, requiring the exact reviewed identity for an oversized batch.
    pub fn prune_archives(
        &self,
        max_records: usize,
        authorized_oversized_archive_id: Option<String>,
    ) -> Result<RetentionArchivePruneOutcome, HostProblem> {
        self.planner
            .store()
            .prune_retention_archives_authorized(
                self.planner.policy(),
                RetentionArchivePruneRequest {
                    now_tick: self.now_tick,
                    max_records,
                    authorized_oversized_archive_id,
                },
            )
            .map_err(store_problem)
    }

    /// Record a conservative age for one exact operator-reviewed legacy row.
    pub fn reconcile(
        &self,
        request: RetentionAgeReconciliation,
    ) -> Result<RetentionReconciliationReceipt, HostProblem> {
        self.planner.reconcile(request, self.now_tick)
    }

    /// List legacy rows without taking a second clock sample.
    pub fn legacy_rows(
        &self,
        target: RetentionTarget,
        max_records: usize,
    ) -> Result<Vec<RetentionLegacyRow>, HostProblem> {
        self.planner.legacy_rows(target, max_records)
    }
}

pub(super) fn store_problem(problem: StoreError) -> HostProblem {
    match problem {
        StoreError::Conflict | StoreError::AlreadyExists | StoreError::LeaseConflict => {
            HostProblem::IdempotencyConflict
        }
        StoreError::CapacityExceeded | StoreError::PayloadTooLarge => {
            HostProblem::ResourceExhausted
        }
        StoreError::NotFound => HostProblem::NotFound,
        StoreError::InvalidTransition
        | StoreError::InvalidSequence
        | StoreError::IncompatibleVersion => HostProblem::InfrastructureFailure,
        StoreError::Poisoned | StoreError::Infrastructure(_) => HostProblem::InfrastructureFailure,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::console_retention::encode_console_log;
    use mainframe_env_store::{MemoryStore, SqliteStateStore, StoreLimits};
    use mainframe_env_store_api::{ProviderStateRecord, ProviderStateStore, RetentionObservation};
    use sha2::{Digest, Sha256};
    use std::path::PathBuf;

    fn policy() -> RetentionPolicy {
        RetentionPolicy {
            lifecycle_ticks: 1,
            idempotency_ticks: 1,
            audit_ticks: 1,
            archive_ticks: 1,
            low_watermark_percent: 70,
            high_watermark_percent: 85,
            max_batch: 8,
        }
    }

    #[test]
    fn open_and_legacy_listing_do_not_advance_the_clock() {
        let store: Arc<dyn PlatformStore> = Arc::new(MemoryStore::new(StoreLimits::default()));
        let maintenance = RetentionMaintenance::open(store.clone(), policy()).unwrap();
        assert!(
            maintenance
                .legacy_rows(RetentionTarget::TerminalExecutions, 1)
                .unwrap()
                .is_empty()
        );
        assert_eq!(store.advance_logical_clock(1).unwrap(), 1);
    }

    #[test]
    fn stale_observation_returns_a_successful_observation_only_receipt() {
        let store: Arc<dyn PlatformStore> = Arc::new(MemoryStore::new(StoreLimits {
            max_retention_archive_rows: 1,
            ..StoreLimits::default()
        }));
        let planner =
            provider::RetentionPlanner::from_existing(store.clone(), policy(), None).unwrap();
        let filler_key = "0000000000000009";
        store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: "console-log".into(),
                    key: filler_key.into(),
                    version: 1,
                    payload: encode_console_log(
                        filler_key,
                        "MVS",
                        b"fills archive capacity",
                        "IBMUSER",
                        "direct-console",
                        1,
                    )
                    .unwrap(),
                },
                None,
            )
            .unwrap();
        assert_eq!(
            planner
                .archive_and_prune(RetentionTarget::ConsoleLog, 100, 8)
                .unwrap()
                .pruned,
            1
        );
        let original = ProviderStateRecord {
            namespace: "console-log".into(),
            key: "0000000000000001".into(),
            version: 1,
            payload: b"MVS\0old".to_vec(),
        };
        store.put_provider_state(original.clone(), None).unwrap();
        let epoch = store.provider_state_retention_epoch().unwrap();
        store
            .record_provider_retention_observation(
                original.clone(),
                epoch,
                RetentionObservation {
                    target: RetentionTarget::ConsoleLog,
                    namespace: original.namespace.clone(),
                    key: original.key.clone(),
                    source_version: original.version,
                    source_digest: Sha256::digest(&original.payload).into(),
                    observed_tick: 1,
                    owner_execution: None,
                },
            )
            .unwrap();
        store
            .delete_provider_state(&original.namespace, &original.key, original.version)
            .unwrap();
        store
            .put_provider_state(
                ProviderStateRecord {
                    payload: b"MVS\0replacement".to_vec(),
                    ..original
                },
                None,
            )
            .unwrap();
        let eligible_key = "0000000000000002";
        store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: "console-log".into(),
                    key: eligible_key.into(),
                    version: 1,
                    payload: encode_console_log(
                        eligible_key,
                        "MVS",
                        b"eligible but deferred",
                        "IBMUSER",
                        "direct-console",
                        1,
                    )
                    .unwrap(),
                },
                None,
            )
            .unwrap();
        let receipt = planner
            .archive_and_prune(RetentionTarget::ConsoleLog, 100, 8)
            .unwrap();
        assert_eq!(receipt.stale_observations_removed, 1);
        assert_eq!((receipt.examined, receipt.protected), (2, 1));
        assert_eq!((receipt.archived, receipt.pruned), (0, 0));
        assert!(
            store
                .get_provider_state("console-log", eligible_key)
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn every_target_has_an_empty_headless_route() {
        let store: Arc<dyn PlatformStore> = Arc::new(MemoryStore::new(StoreLimits::default()));
        let maintenance = RetentionMaintenance::open(store, policy()).unwrap();
        let pass = maintenance.begin_pass().unwrap();
        for target in RetentionTarget::ALL {
            let forecast = pass.forecast(target, 0).unwrap();
            assert_eq!(forecast.target, target);
            let receipt = pass.archive_and_prune(target, 1).unwrap();
            assert_eq!(receipt.target, target);
            assert_eq!((receipt.archived, receipt.pruned), (0, 0));
            assert!(maintenance.legacy_rows(target, 1).unwrap().is_empty());
        }
    }

    #[test]
    fn provider_forecast_rejects_a_mixed_epoch_snapshot() {
        let store: Arc<dyn PlatformStore> = Arc::new(MemoryStore::new(StoreLimits::default()));
        let mut planner =
            provider::RetentionPlanner::from_existing(store.clone(), policy(), None).unwrap();
        planner.set_forecast_epoch_hook(Arc::new(move || {
            store
                .put_provider_state(
                    ProviderStateRecord {
                        namespace: "forecast-race".into(),
                        key: "late-writer".into(),
                        version: 1,
                        payload: b"late".to_vec(),
                    },
                    None,
                )
                .unwrap();
        }));
        assert_eq!(
            planner.forecast(RetentionTarget::ConsoleLog, 100, 0),
            Err(HostProblem::IdempotencyConflict)
        );
    }

    #[test]
    fn racf_forecast_rejects_observations_from_a_mixed_epoch() {
        let store: Arc<dyn PlatformStore> = Arc::new(MemoryStore::new(StoreLimits::default()));
        let mut planner =
            provider::RetentionPlanner::from_existing(store.clone(), policy(), None).unwrap();
        planner.set_forecast_epoch_hook(Arc::new(move || {
            store
                .put_provider_state(
                    ProviderStateRecord {
                        namespace: "racf-forecast-race".into(),
                        key: "late-writer".into(),
                        version: 1,
                        payload: b"late".to_vec(),
                    },
                    None,
                )
                .unwrap();
        }));
        assert_eq!(
            planner.forecast(RetentionTarget::RacfEvidence, 100, 0),
            Err(HostProblem::IdempotencyConflict)
        );
    }

    #[test]
    fn corrupt_legacy_cobol_protocol_is_not_listed_as_reconcilable() {
        let store: Arc<dyn PlatformStore> = Arc::new(MemoryStore::new(StoreLimits::default()));
        store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: "cobol-call-protocol@1".into(),
                    key: crate::cobol::retention::protocol_key("corrupt-run"),
                    version: 1,
                    payload: b"not-the-legacy-protocol-marker".to_vec(),
                },
                None,
            )
            .unwrap();
        let maintenance = RetentionMaintenance::open(store, policy()).unwrap();
        assert_eq!(
            maintenance.legacy_rows(RetentionTarget::CobolLifecycle, 1),
            Err(HostProblem::InfrastructureFailure)
        );
    }

    #[test]
    fn legacy_only_racf_is_target_local_and_does_not_block_other_reclamation() {
        let store: Arc<dyn PlatformStore> = Arc::new(MemoryStore::new(StoreLimits {
            max_provider_state: 2,
            ..StoreLimits::default()
        }));
        store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: "racf-group".into(),
                    key: "GROUP1".into(),
                    version: 1,
                    payload: Vec::new(),
                },
                None,
            )
            .unwrap();
        let console_key = "0000000000000001";
        store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: "console-log".into(),
                    key: console_key.into(),
                    version: 1,
                    payload: encode_console_log(
                        console_key,
                        "MVS",
                        b"eligible",
                        "IBMUSER",
                        "direct-console",
                        1,
                    )
                    .unwrap(),
                },
                None,
            )
            .unwrap();
        let maintenance = RetentionMaintenance::open(store.clone(), policy()).unwrap();
        assert_eq!(
            maintenance
                .legacy_rows(RetentionTarget::RacfEvidence, 8)
                .unwrap()
                .len(),
            1
        );
        let pass = maintenance.begin_pass().unwrap();
        let racf = pass
            .archive_and_prune(RetentionTarget::RacfEvidence, 1)
            .unwrap();
        assert_eq!((racf.examined, racf.protected, racf.pruned), (1, 1, 0));
        let console = pass
            .archive_and_prune(RetentionTarget::ConsoleLog, 1)
            .unwrap();
        assert_eq!(console.pruned, 1);
        assert!(
            store
                .get_provider_state("racf-group", "GROUP1")
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn sqlite_full_quota_headless_open_and_forecast_preserve_serving_rows() {
        let directory = TestDirectory::new("headless-full-read");
        let url = directory.sqlite_url();
        let store = Arc::new(SqliteStateStore::open(&url, 1024 * 1024, 2).unwrap());
        let rows = [
            ProviderStateRecord {
                namespace: "application-publication".into(),
                key: "pending".into(),
                version: 1,
                payload: b"pending-publication-byte-for-byte".to_vec(),
            },
            ProviderStateRecord {
                namespace: "outbox-recovery".into(),
                key: "pending".into(),
                version: 1,
                payload: b"pending-outbox-byte-for-byte".to_vec(),
            },
        ];
        for row in &rows {
            store.put_provider_state(row.clone(), None).unwrap();
        }
        let before_epoch = store.provider_state_retention_epoch().unwrap();
        drop(store);

        let reopened: Arc<dyn PlatformStore> =
            Arc::new(SqliteStateStore::open(&url, 1024 * 1024, 2).unwrap());
        let maintenance = RetentionMaintenance::open(reopened.clone(), policy()).unwrap();
        assert_eq!(
            reopened.provider_state_retention_epoch().unwrap(),
            before_epoch
        );
        let pass = maintenance.begin_pass().unwrap();
        for target in RetentionTarget::ALL {
            assert_eq!(pass.forecast(target, 0).unwrap().target, target);
        }
        assert_eq!(
            reopened.provider_state_retention_epoch().unwrap(),
            before_epoch
        );
        for expected in rows {
            assert_eq!(
                reopened
                    .get_provider_state(&expected.namespace, &expected.key)
                    .unwrap(),
                Some(expected)
            );
        }
    }

    #[test]
    fn sqlite_headless_prune_releases_exact_full_provider_capacity_after_reopen() {
        let directory = TestDirectory::new("headless-capacity-reuse");
        let url = directory.sqlite_url();
        let key = "0000000000000001";
        {
            let store = SqliteStateStore::open(&url, 1024 * 1024, 1).unwrap();
            store
                .put_provider_state(
                    ProviderStateRecord {
                        namespace: "console-log".into(),
                        key: key.into(),
                        version: 1,
                        payload: encode_console_log(
                            key,
                            "MVS",
                            b"old terminal row",
                            "IBMUSER",
                            "direct-console",
                            1,
                        )
                        .unwrap(),
                    },
                    None,
                )
                .unwrap();
        }
        let store: Arc<dyn PlatformStore> =
            Arc::new(SqliteStateStore::open(&url, 1024 * 1024, 1).unwrap());
        let maintenance = RetentionMaintenance::open(store.clone(), policy()).unwrap();
        let receipt = maintenance
            .begin_pass()
            .unwrap()
            .archive_and_prune(RetentionTarget::ConsoleLog, 1)
            .unwrap();
        assert_eq!((receipt.archived, receipt.pruned), (1, 1));
        store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: "replacement".into(),
                    key: "capacity-reused".into(),
                    version: 1,
                    payload: b"replacement".to_vec(),
                },
                None,
            )
            .unwrap();
    }

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new(label: &str) -> Self {
            let unique = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "mainframe-env-{label}-{}-{unique}",
                std::process::id()
            ));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn sqlite_url(&self) -> String {
            format!("sqlite://{}?mode=rwc", self.0.join("state.db").display())
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}
