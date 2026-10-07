//! Selected-store proofs for exact-version source handoff, without target dispatch.
use super::*;
use mainframe_env_execution_api::{ExecutionOutcome, LifecycleEventKind, Suspension};
use mainframe_env_store_api::{ExecutionRecord, ExecutionState};

struct SuspendedSource;

impl Machine for SuspendedSource {
    type Effect = EffectRequest;
    type EffectResult = EffectResult;

    fn drive(
        &mut self,
        resume: MachineResume<Self::EffectResult>,
        _: Quantum,
    ) -> MachineDrive<Self::Effect> {
        assert!(matches!(resume, MachineResume::Start));
        MachineDrive::Suspended(Suspension {
            kind: "handoff-test".into(),
            resume_token: "source-image".into(),
            state_bytes: 5,
        })
    }

    fn checkpoint(&self) -> Option<BoundedPayload> {
        Some(
            BoundedPayload::new(
                "handoff-source@1",
                b"image".to_vec(),
                InvocationLimits::default(),
            )
            .unwrap(),
        )
    }
}

fn context(label: &str) -> Invocation {
    let l = InvocationLimits::default();
    let id = unique(label);
    Invocation::new(
        RequestId::new(format!("{id}-request"), l).unwrap(),
        ExecutionId::new(format!("{id}-execution"), l).unwrap(),
        RunUnitId::new(format!("{id}-run"), l).unwrap(),
        None,
        Selector::new("program:SOURCE", l).unwrap(),
        ArtifactRef::new("immutable-source", l).unwrap(),
        Principal::new(
            PrincipalId::new("HANDOFF-OWNER", l).unwrap(),
            BTreeSet::new(),
            l,
        )
        .unwrap(),
        ServiceClass::System,
        0,
        100,
        TraceId::new(format!("{id}-trace"), l).unwrap(),
        IdempotencyKey::new(format!("{id}-key"), l).unwrap(),
        1,
        ResourceLimits::default(),
        BTreeMap::new(),
        l,
    )
    .unwrap()
}

fn coordinator(store: Arc<dyn PlatformStore>) -> ExecutionCoordinator {
    let host = Arc::new(ScopedHostService::new(
        Arc::new(RegistrySnapshot::new(1, Vec::new(), InvocationLimits::default()).unwrap()),
        HostLimits::default(),
    ));
    ExecutionCoordinator::durable(host, store, CoordinatorLimits::default())
}

fn suspended(store: Arc<dyn PlatformStore>, label: &str) -> (Invocation, ExecutionRecord) {
    let source = context(label);
    assert!(matches!(
        coordinator(store.clone()).execute(
            &mut SuspendedSource,
            &source,
            ExecutionControl {
                now_tick: 10,
                cancellation_requested: false
            },
        ),
        ExecutionOutcome::Suspended(_)
    ));
    let record = store.get_execution(&source.execution_id).unwrap().unwrap();
    assert_eq!(record.state, ExecutionState::Suspended);
    assert_eq!(record.version, 4);
    assert!(
        store
            .get_checkpoint(&source.execution_id)
            .unwrap()
            .is_some()
    );
    (source, record)
}

fn unchanged(store: &dyn PlatformStore, source: &Invocation, before: &ExecutionRecord) {
    assert_eq!(
        store.get_execution(&source.execution_id).unwrap().as_ref(),
        Some(before)
    );
    let events = store
        .events(&source.execution_id, 1, before.version as usize)
        .unwrap();
    assert_eq!(events.len(), before.version as usize);
    assert_eq!(events.last().unwrap().kind, LifecycleEventKind::Suspended);
    assert!(
        store
            .get_checkpoint(&source.execution_id)
            .unwrap()
            .is_some()
    );
    assert_eq!(
        store.pending_notifications(100).unwrap().len(),
        before.version as usize
    );
}

#[test]
fn versioned_handoff_rejects_absent_stale_foreign_and_invalid_tick_without_mutation() {
    let store: Arc<dyn PlatformStore> = Arc::new(MemoryStore::new(StoreLimits::default()));
    let c = coordinator(store.clone());
    let absent = context("absent");
    assert_eq!(
        c.complete_suspended_handoff_at_version(&absent, 4, 20),
        Err(StoreError::NotFound)
    );
    assert!(store.get_execution(&absent.execution_id).unwrap().is_none());
    assert!(
        store
            .events(&absent.execution_id, 1, 100)
            .unwrap()
            .is_empty()
    );
    assert!(store.pending_notifications(100).unwrap().is_empty());
    let (source, before) = suspended(store.clone(), "negative");
    let l = InvocationLimits::default();
    for case in 0..12 {
        let mut forged = source.clone();
        let mut version = before.version;
        let mut tick = 20;
        match case {
            0 => version = 0,
            1 => version -= 1,
            2 => version += 1,
            3 => version = i64::MAX as u64,
            4 => forged.run_unit_id = RunUnitId::new("foreign-run", l).unwrap(),
            5 => {
                forged.principal =
                    Principal::new(PrincipalId::new("FOREIGN", l).unwrap(), BTreeSet::new(), l)
                        .unwrap()
            }
            6 => forged.selector = Selector::new("program:OTHER", l).unwrap(),
            7 => forged.artifact = ArtifactRef::new("other-artifact", l).unwrap(),
            8 => forged.attempt += 1,
            9 => tick = 0,
            10 => tick = 9,
            11 => tick = i64::MAX as u64 + 1,
            _ => unreachable!(),
        }
        let expected = if case >= 9 {
            StoreError::InvalidSequence
        } else {
            StoreError::Conflict
        };
        assert_eq!(
            c.complete_suspended_handoff_at_version(&forged, version, tick),
            Err(expected),
            "case {case}"
        );
        unchanged(store.as_ref(), &source, &before);
    }
}

fn completed_proof(store: &dyn PlatformStore, source: &Invocation, version: u64) {
    let record = store.get_execution(&source.execution_id).unwrap().unwrap();
    assert_eq!(record.state, ExecutionState::Completed);
    assert_eq!(record.version, version + 1);
    assert_eq!(record.terminal_tick, Some(20));
    let events = store.events(&source.execution_id, 1, 100).unwrap();
    assert_eq!(events.len(), version as usize + 1);
    let last = events.last().unwrap();
    assert_eq!(last.kind, LifecycleEventKind::HandoffCompleted);
    assert_eq!(last.sequence, record.version);
    assert_eq!(last.tick, 20);
    assert_eq!(last.attempt, source.attempt);
    assert_eq!(last.run_unit_id, source.run_unit_id);
    assert!(
        store
            .get_checkpoint(&source.execution_id)
            .unwrap()
            .is_none()
    );
}

#[test]
fn versioned_handoff_rejects_non_suspended_or_unproven_journal() {
    let store: Arc<dyn PlatformStore> = Arc::new(MemoryStore::new(StoreLimits::default()));
    let (source, before) = suspended(store.clone(), "journal");
    let queued = store
        .transition_execution(
            &source.execution_id,
            before.version,
            ExecutionState::Queued,
            11,
        )
        .unwrap();
    assert_eq!(
        coordinator(store.clone()).complete_suspended_handoff_at_version(
            &source,
            queued.version,
            20
        ),
        Err(StoreError::InvalidTransition)
    );
    assert_eq!(
        store.get_execution(&source.execution_id).unwrap().unwrap(),
        queued
    );
    let running = store
        .transition_execution(
            &source.execution_id,
            queued.version,
            ExecutionState::Running,
            12,
        )
        .unwrap();
    let resumed = store
        .transition_execution(
            &source.execution_id,
            running.version,
            ExecutionState::Suspended,
            13,
        )
        .unwrap();
    // Standalone state transitions cannot substitute for an atomic journal proof.
    assert_eq!(
        coordinator(store.clone()).complete_suspended_handoff_at_version(
            &source,
            resumed.version,
            20
        ),
        Err(StoreError::IncompatibleVersion)
    );
    assert_eq!(
        store.get_execution(&source.execution_id).unwrap().unwrap(),
        resumed
    );
    assert_eq!(
        store.events(&source.execution_id, 1, 100).unwrap().len(),
        before.version as usize
    );
    assert_eq!(
        store.pending_notifications(100).unwrap().len(),
        before.version as usize
    );
    assert!(
        store
            .get_checkpoint(&source.execution_id)
            .unwrap()
            .is_some()
    );
}

#[test]
fn versioned_handoff_memory_race_commits_exactly_one_terminal_event() {
    let store: Arc<dyn PlatformStore> = Arc::new(MemoryStore::new(StoreLimits::default()));
    let (source, before) = suspended(store.clone(), "race");
    let barrier = Arc::new(std::sync::Barrier::new(2));
    let handles = (0..2)
        .map(|_| {
            let store = store.clone();
            let source = source.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                coordinator(store).complete_suspended_handoff_at_version(
                    &source,
                    before.version,
                    20,
                )
            })
        })
        .collect::<Vec<_>>();
    let results = handles
        .into_iter()
        .map(|h| h.join().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert!(
        results
            .iter()
            .filter_map(|r| r.as_ref().err())
            .all(|e| matches!(
                e,
                StoreError::Conflict
                    | StoreError::InvalidTransition
                    | StoreError::IncompatibleVersion
            ))
    );
    completed_proof(store.as_ref(), &source, before.version);
    assert_eq!(
        store.pending_notifications(100).unwrap().len(),
        before.version as usize + 1
    );
    assert_eq!(
        coordinator(store.clone()).complete_suspended_handoff_at_version(
            &source,
            before.version + 1,
            20
        ),
        Err(StoreError::InvalidTransition)
    );
    completed_proof(store.as_ref(), &source, before.version);
}

#[test]
fn versioned_handoff_capacity_failure_preserves_suspended_source_and_checkpoint() {
    let store: Arc<dyn PlatformStore> = Arc::new(MemoryStore::new(StoreLimits {
        max_events: 4,
        max_events_per_execution: 4,
        ..StoreLimits::default()
    }));
    let (source, before) = suspended(store.clone(), "quota");
    assert_eq!(
        coordinator(store.clone()).complete_suspended_handoff_at_version(
            &source,
            before.version,
            20
        ),
        Err(StoreError::CapacityExceeded)
    );
    unchanged(store.as_ref(), &source, &before);
}

#[test]
fn versioned_handoff_sqlite_survives_physical_reopen_without_repeat_terminalization() {
    let root = std::env::temp_dir().join(unique("handoff-sqlite"));
    std::fs::create_dir(&root).unwrap();
    let url = format!("sqlite://{}?mode=rwc", root.join("state.db").display());
    let (source, before) = {
        let store: Arc<dyn PlatformStore> =
            Arc::new(SqliteStateStore::open(&url, 1024 * 1024, 1000).unwrap());
        suspended(store, "sqlite")
    };
    {
        let store: Arc<dyn PlatformStore> =
            Arc::new(SqliteStateStore::open(&url, 1024 * 1024, 1000).unwrap());
        unchanged(store.as_ref(), &source, &before);
        coordinator(store.clone())
            .complete_suspended_handoff_at_version(&source, before.version, 20)
            .unwrap();
        completed_proof(store.as_ref(), &source, before.version);
    }
    {
        let store: Arc<dyn PlatformStore> =
            Arc::new(SqliteStateStore::open(&url, 1024 * 1024, 1000).unwrap());
        completed_proof(store.as_ref(), &source, before.version);
        assert_eq!(
            coordinator(store.clone()).complete_suspended_handoff_at_version(
                &source,
                before.version,
                20
            ),
            Err(StoreError::Conflict)
        );
        completed_proof(store.as_ref(), &source, before.version);
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
#[ignore = "requires isolated MAINFRAME_ENV_POSTGRES_TEST_URL"]
fn postgres_versioned_handoff_survives_reopen_without_repeat_terminalization() {
    let url =
        std::env::var("MAINFRAME_ENV_POSTGRES_TEST_URL").expect("explicit isolated PostgreSQL URL");
    let (source, before) = {
        let store: Arc<dyn PlatformStore> =
            Arc::new(PostgresStateStore::open(&url, 1024 * 1024, 1000).unwrap());
        suspended(store, "postgres")
    };
    {
        let store: Arc<dyn PlatformStore> =
            Arc::new(PostgresStateStore::open(&url, 1024 * 1024, 1000).unwrap());
        unchanged(store.as_ref(), &source, &before);
        coordinator(store.clone())
            .complete_suspended_handoff_at_version(&source, before.version, 20)
            .unwrap();
        completed_proof(store.as_ref(), &source, before.version);
    }
    {
        let store: Arc<dyn PlatformStore> =
            Arc::new(PostgresStateStore::open(&url, 1024 * 1024, 1000).unwrap());
        completed_proof(store.as_ref(), &source, before.version);
        assert_eq!(
            coordinator(store.clone()).complete_suspended_handoff_at_version(
                &source,
                before.version,
                20
            ),
            Err(StoreError::Conflict)
        );
        completed_proof(store.as_ref(), &source, before.version);
    }
}
