//! Private recovery presence uses the real inventory and existing store epoch.
use super::safety_tests::{policy, terminal_execution, unresolved_effect};
use super::*;
use crate::retention_maintenance::RetentionMaintenance;
use mainframe_env_execution_api::{LifecycleEvent, LifecycleEventKind};
use mainframe_env_store::{MemoryStore, PostgresStateStore, SqliteStateStore, StoreLimits};
use mainframe_env_store_api::{IdempotencyStore, RetentionStore, WorkRecord, WorkState};

const CORE: [RetentionTarget; 4] = [
    RetentionTarget::ResolvedEffects,
    RetentionTarget::TerminalWork,
    RetentionTarget::LifecycleEvents,
    RetentionTarget::TerminalExecutions,
];

fn private_row(namespace: &str) -> ProviderStateRecord {
    ProviderStateRecord {
        namespace: namespace.into(),
        key: "unknown-owner".into(),
        version: 1,
        payload: b"malformed protected edges, not a recovery decoder".to_vec(),
    }
}

fn seed_core(store: &dyn PlatformStore, label: &str) -> IdempotencyKey {
    let (owner, run) = terminal_execution(store, label);
    let effect = unresolved_effect(&owner, &run, &format!("{label}-effect"));
    store.record_intent(effect.clone()).unwrap();
    store
        .record_result(
            &effect.key,
            EffectRecord {
                state: EffectState::Completed,
                result_digest: Some([2; 32]),
                resolved_tick: Some(10),
                ..effect.clone()
            },
        )
        .unwrap();
    store
        .append_event(LifecycleEvent {
            execution_id: owner.clone(),
            run_unit_id: run,
            sequence: 1,
            attempt: 1,
            tick: 10,
            kind: LifecycleEventKind::Completed { return_code: 0 },
        })
        .unwrap();
    let execution = store.get_execution(&owner).unwrap().unwrap();
    store
        .enqueue(WorkRecord {
            work_id: format!("{label}-work"),
            execution_id: owner,
            required_selector: execution.selector,
            required_generation: "retention@1".into(),
            artifact: execution.artifact,
            state: WorkState::Queued,
            priority: 0,
            attempt: 0,
            max_attempts: 1,
            available_tick: 1,
            deadline_tick: 1000,
            cancellation_requested: false,
            worker_id: None,
            lease_id: None,
            lease_epoch: 0,
            lease_expiry_tick: None,
            heartbeat_tick: None,
            terminal_tick: None,
            checkpoint_id: None,
            effect_sequence: 0,
            payload: vec![],
        })
        .unwrap();
    let work = store
        .claim("retention-worker", Some("retention@1"), 10, 100)
        .unwrap()
        .unwrap();
    store
        .complete(
            &work.work_id,
            work.lease_id.as_deref().unwrap(),
            work.lease_epoch,
            11,
        )
        .unwrap();
    effect.key
}

fn exercise_presence(store: Arc<dyn PlatformStore>) {
    let planner = RetentionPlanner::from_existing(store.clone(), policy(), None).unwrap();
    assert!(!planner.core_dependencies().unwrap().unowned);
    seed_core(store.as_ref(), "empty-control");
    // Successful absence allows each actual family to make progress in contract order.
    for target in CORE {
        assert_eq!(
            planner.forecast(target, 100, 0).unwrap().eligible_records,
            1,
            "{target:?}"
        );
        assert_eq!(
            planner.archive_and_prune(target, 100, 8).unwrap().pruned,
            1,
            "{target:?}"
        );
    }
    let key = seed_core(store.as_ref(), "unrelated-owner");
    for namespace in [
        "ims-recovery-v1-session",
        "ims-recovery-v1-db-publication",
        "ims-recovery-v1-db-stage",
        "ims-recovery-v1-log-active",
        "ims-recovery-v1-log-stage",
        "ims-recovery-v1-future",
    ] {
        let row = private_row(namespace);
        store.put_provider_state(row.clone(), None).unwrap();
        let dependencies = planner.core_dependencies().unwrap();
        assert!(dependencies.unowned, "{namespace}");
        // This is a global fence, not fabricated attribution to the unrelated owner.
        assert!(dependencies.blocked_effect_keys.is_empty());
        for target in CORE {
            assert_eq!(
                planner.forecast(target, 100, 1).unwrap().eligible_records,
                0
            );
            let receipt = planner.archive_and_prune(target, 100, 8).unwrap();
            assert_eq!(receipt.pruned, 0);
            assert_eq!(receipt.archive_id, None);
        }
        let maintenance = RetentionMaintenance::open(store.clone(), policy()).unwrap();
        let pass = maintenance.begin_pass().unwrap();
        assert_eq!(
            pass.forecast(RetentionTarget::ResolvedEffects, 1)
                .unwrap()
                .eligible_records,
            0
        );
        assert_eq!(
            pass.archive_and_prune(RetentionTarget::ResolvedEffects, 8)
                .unwrap()
                .pruned,
            0
        );
        assert_eq!(
            store.get_provider_state(namespace, &row.key).unwrap(),
            Some(row.clone())
        );
        // Test fixture teardown at exact CAS, not a production private expiry operation.
        store
            .delete_provider_state(namespace, &row.key, row.version)
            .unwrap();
    }
    assert!(store.effect(&key).unwrap().is_some());
    assert!(!planner.core_dependencies().unwrap().unowned);
    assert_eq!(
        planner
            .forecast(RetentionTarget::ResolvedEffects, 100, 0)
            .unwrap()
            .eligible_records,
        1
    );
}

fn sqlite_case(name: &str, exercise: impl FnOnce(Arc<SqliteStateStore>, &std::path::Path)) {
    let path = std::env::temp_dir().join(format!(
        "ims-private-retention-{name}-{}.sqlite",
        std::process::id()
    ));
    let store = Arc::new(
        SqliteStateStore::open(
            &format!("sqlite:{}?mode=rwc", path.display()),
            64 * 1024 * 1024,
            262_144,
        )
        .unwrap(),
    );
    exercise(store.clone(), &path);
    drop(store);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn ims_private_recovery_retention_memory_presence_and_maintenance() {
    exercise_presence(Arc::new(MemoryStore::new(StoreLimits::default())));
}

#[test]
fn ims_private_recovery_retention_sqlite_presence_and_maintenance() {
    sqlite_case("presence", |store, _| exercise_presence(store));
}

fn exercise_stale_epoch(store: Arc<dyn PlatformStore>) {
    let key = seed_core(store.as_ref(), "stale-epoch");
    let planner = RetentionPlanner::from_existing(store.clone(), policy(), None).unwrap();
    let snapshot = planner.core_dependencies().unwrap();
    assert!(!snapshot.unowned);
    let writer = store.clone();
    std::thread::spawn(move || {
        writer
            .put_provider_state(private_row("ims-recovery-v1-session"), None)
            .unwrap()
    })
    .join()
    .unwrap();
    // Real insertion after actual empty inventory; transaction must reject stale epoch.
    assert_eq!(
        store.archive_and_prune_with_dependencies(
            policy(),
            RetentionRequest {
                target: RetentionTarget::ResolvedEffects,
                now_tick: 100,
                max_records: 8,
            },
            &snapshot
        ),
        Err(StoreError::Conflict)
    );
    assert!(store.effect(&key).unwrap().is_some());
    assert!(planner.core_dependencies().unwrap().unowned);
    store
        .delete_provider_state("ims-recovery-v1-session", "unknown-owner", 1)
        .unwrap();
    let mut planner = RetentionPlanner::from_existing(store.clone(), policy(), None).unwrap();
    let writer = store.clone();
    planner.set_forecast_epoch_hook(Arc::new(move || {
        let writer = writer.clone();
        std::thread::spawn(move || {
            writer
                .put_provider_state(private_row("ims-recovery-v1-future"), None)
                .unwrap()
        })
        .join()
        .unwrap();
    }));
    // Insertion between inventory and its closing epoch check also fails closed.
    assert_eq!(
        planner.core_dependencies(),
        Err(HostProblem::IdempotencyConflict)
    );
    assert!(store.effect(&key).unwrap().is_some());
}

#[test]
fn ims_private_recovery_retention_memory_real_epoch_interleaving() {
    exercise_stale_epoch(Arc::new(MemoryStore::new(StoreLimits::default())));
}

#[test]
fn ims_private_recovery_retention_sqlite_real_epoch_interleaving() {
    sqlite_case("epoch", |store, _| exercise_stale_epoch(store));
}

#[test]
fn ims_private_recovery_retention_sqlite_store_failure_is_not_absence() {
    sqlite_case("failure", |store, path| {
        let key = seed_core(store.as_ref(), "store-failure");
        let planner = RetentionPlanner::from_existing(store.clone(), policy(), None).unwrap();
        let output = std::process::Command::new("python3").args(["-B", "-c",
            "import sqlite3,sys; c=sqlite3.connect(sys.argv[1]); c.execute('ALTER TABLE provider_state RENAME TO unavailable_provider_state'); c.commit()"])
            .arg(path).output().unwrap();
        assert!(output.status.success(), "{output:?}");
        assert_eq!(
            planner.core_dependencies(),
            Err(HostProblem::InfrastructureFailure)
        );
        assert_eq!(
            planner.forecast(RetentionTarget::ResolvedEffects, 100, 0),
            Err(HostProblem::InfrastructureFailure)
        );
        assert_eq!(
            planner.archive_and_prune(RetentionTarget::ResolvedEffects, 100, 8),
            Err(HostProblem::InfrastructureFailure)
        );
        let output = std::process::Command::new("python3").args(["-B", "-c",
            "import sqlite3,sys; c=sqlite3.connect(sys.argv[1]); c.execute('ALTER TABLE unavailable_provider_state RENAME TO provider_state'); c.commit()"])
            .arg(path).output().unwrap();
        assert!(output.status.success(), "{output:?}");
        assert!(store.effect(&key).unwrap().is_some());
        assert!(
            store
                .retention_archives(RetentionTarget::ResolvedEffects, 8)
                .unwrap()
                .is_empty()
        );
    });
}

#[test]
#[ignore = "requires isolated MAINFRAME_ENV_POSTGRES_TEST_URL pointing at PostgreSQL 18"]
fn ims_private_recovery_retention_postgres_configured_gate() {
    let url =
        std::env::var("MAINFRAME_ENV_POSTGRES_TEST_URL").expect("isolated PostgreSQL URL required");
    let store = Arc::new(PostgresStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap());
    exercise_presence(store.clone());
    exercise_stale_epoch(store);
}
