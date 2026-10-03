//! Selected MQ history protection, not age reconciliation or MQI execution.

use super::safety_tests::{policy, unresolved_effect};
use super::*;
use mainframe_env_execution_api::{ArtifactRef, PrincipalId, Selector};
use mainframe_env_store::{MemoryStore, SqliteStateStore};
use mainframe_env_store_api::{ExecutionRecord, ExecutionState};
use serde::Deserialize;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_DB: AtomicU64 = AtomicU64::new(1);

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FixtureRow {
    namespace: String,
    key: String,
    version: u64,
    payload: String,
}

fn fixture() -> Vec<ProviderStateRecord> {
    let rows: Vec<FixtureRow> =
        serde_json::from_str(include_str!("selected_mq_fixture.json")).unwrap();
    rows.into_iter()
        .map(|row| ProviderStateRecord {
            namespace: row.namespace,
            key: row.key,
            version: row.version,
            payload: row.payload.into_bytes(),
        })
        .collect()
}

fn install_fixture(store: &dyn PlatformStore) {
    // Test setup reproduces physical versions through legal insert/successor
    // CAS calls. No force-version, production import or operator permission.
    for record in fixture() {
        assert!((1..=4).contains(&record.version));
        for version in 1..=record.version {
            store
                .put_provider_state(
                    ProviderStateRecord {
                        version,
                        ..record.clone()
                    },
                    if version == 1 {
                        None
                    } else {
                        Some(version - 1)
                    },
                )
                .unwrap();
        }
    }
}

struct Backend {
    store: Option<Arc<dyn PlatformStore>>,
    directory: Option<PathBuf>,
}

impl Backend {
    fn new(sqlite: bool) -> Self {
        if !sqlite {
            return Self {
                store: Some(Arc::new(MemoryStore::new(Default::default()))),
                directory: None,
            };
        }
        let directory = std::env::temp_dir().join(format!(
            "server-mq-retention-{}-{}",
            std::process::id(),
            NEXT_DB.fetch_add(1, Ordering::Relaxed),
        ));
        std::fs::create_dir(&directory).unwrap();
        let directory = directory.canonicalize().unwrap();
        let url = format!(
            "sqlite://{}?mode=rwc",
            directory.join("state.sqlite").display()
        );
        Self {
            store: Some(Arc::new(
                SqliteStateStore::open(&url, 64 << 20, 256).unwrap(),
            )),
            directory: Some(directory),
        }
    }

    fn store(&self) -> Arc<dyn PlatformStore> {
        self.store.as_ref().unwrap().clone()
    }
}

impl Drop for Backend {
    fn drop(&mut self) {
        drop(self.store.take());
        if let Some(directory) = &self.directory {
            for name in ["state.sqlite", "state.sqlite-wal", "state.sqlite-shm"] {
                let path = directory.join(name);
                if path.exists() {
                    std::fs::remove_file(path).unwrap();
                }
            }
            std::fs::remove_dir(directory).unwrap();
        }
    }
}

fn terminal_owner_and_effect(store: &dyn PlatformStore) -> EffectRecord {
    let limits = InvocationLimits::default();
    let owner = ExecutionId::new("execution", limits).unwrap();
    let run = RunUnitId::new("run", limits).unwrap();
    store
        .create_execution(ExecutionRecord {
            execution_id: owner.clone(),
            run_unit_id: run.clone(),
            selector: Selector::new("test", limits).unwrap(),
            artifact: ArtifactRef::new("artifact", limits).unwrap(),
            principal: PrincipalId::new("TEST", limits).unwrap(),
            state: ExecutionState::Admitted,
            attempt: 1,
            version: 1,
            owner_lease: None,
            lease_expiry_tick: None,
            terminal_tick: None,
        })
        .unwrap();
    let mut version = 1;
    for (state, tick) in [
        (ExecutionState::Queued, 2),
        (ExecutionState::Running, 3),
        (ExecutionState::Completing, 4),
        (ExecutionState::Completed, 5),
    ] {
        version = store
            .transition_execution(&owner, version, state, tick)
            .unwrap()
            .version;
    }
    let mut effect = unresolved_effect(&owner, &run, "effect-1");
    store.record_intent(effect.clone()).unwrap();
    // Deliberately synthetic core digests: protection must not require a
    // positive archival assertion about this source or claim MQ execution.
    effect.state = EffectState::Completed;
    effect.result_digest = Some([2; 32]);
    effect.resolved_tick = Some(6);
    store.record_result(&effect.key, effect.clone()).unwrap();
    effect
}

#[test]
fn memory_owned_sqlite_selected_receipts_and_units_protect_real_core_rows() {
    for sqlite in [false, true] {
        for selected in [false, true] {
            let backend = Backend::new(sqlite);
            let store = backend.store();
            let effect = terminal_owner_and_effect(store.as_ref());
            if selected {
                install_fixture(store.as_ref());
            }
            let rows = store.list_provider_state_prefix("mq-", MAX_SCAN).unwrap();
            let epoch = store.provider_state_retention_epoch().unwrap();
            let planner = RetentionPlanner::from_existing(store.clone(), policy(), None).unwrap();
            let dependencies = planner.core_dependencies().unwrap();
            assert_eq!(dependencies.expected_epoch, epoch);
            assert!(!dependencies.unowned);
            assert_eq!(
                dependencies
                    .blocked_executions
                    .contains(&effect.execution_id),
                selected
            );
            assert_eq!(
                dependencies.blocked_effect_keys.contains(&effect.key),
                selected
            );
            let receipt = planner
                .archive_and_prune(RetentionTarget::ResolvedEffects, 100, 8)
                .unwrap();
            assert_eq!(receipt.pruned, usize::from(!selected));
            assert_eq!(store.effect(&effect.key).unwrap().is_some(), selected);
            assert_eq!(
                store.list_provider_state_prefix("mq-", MAX_SCAN).unwrap(),
                rows
            );
            if selected {
                assert_eq!(store.provider_state_retention_epoch().unwrap(), epoch);
            }
        }
    }
}

#[test]
fn malformed_or_unknown_selected_footprint_sets_unowned_core_fence() {
    for namespace in ["mq-selected-v1-occurrence", "mq-selected-future"] {
        let backend = Backend::new(false);
        let store = backend.store();
        let effect = terminal_owner_and_effect(store.as_ref());
        store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: namespace.into(),
                    key: "orphan".into(),
                    version: 1,
                    payload: b"{}".to_vec(),
                },
                None,
            )
            .unwrap();
        let planner = RetentionPlanner::from_existing(store.clone(), policy(), None).unwrap();
        assert!(planner.core_dependencies().unwrap().unowned);
        assert_eq!(
            planner
                .archive_and_prune(RetentionTarget::ResolvedEffects, 100, 8)
                .unwrap()
                .pruned,
            0
        );
        assert!(store.effect(&effect.key).unwrap().is_some());
    }
}

#[test]
fn captured_selected_dependencies_cannot_authorize_a_stale_epoch() {
    let backend = Backend::new(false);
    let store = backend.store();
    terminal_owner_and_effect(store.as_ref());
    install_fixture(store.as_ref());
    let planner = RetentionPlanner::from_existing(store.clone(), policy(), None).unwrap();
    let dependencies = planner.core_dependencies().unwrap();
    store
        .put_provider_state(
            ProviderStateRecord {
                namespace: "unrelated-test".into(),
                key: "epoch".into(),
                version: 1,
                payload: b"new provider mutation".to_vec(),
            },
            None,
        )
        .unwrap();
    assert_eq!(
        store.archive_and_prune_with_dependencies(
            policy(),
            RetentionRequest {
                target: RetentionTarget::ResolvedEffects,
                now_tick: 100,
                max_records: 8,
            },
            &dependencies
        ),
        Err(StoreError::Conflict)
    );
}
