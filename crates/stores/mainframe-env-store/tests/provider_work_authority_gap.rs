//! Gap witnesses, not a proposed API implementation or TM acceptance suite.
//! Replace these expectations with fenced publication tests when ADR-0028 is owned.
use mainframe_env_execution_api::{ArtifactRef, ExecutionId, InvocationLimits, Selector};
use mainframe_env_store::{MemoryStore, SqliteStateStore};
use mainframe_env_store_api::{
    PlatformStore, ProviderStateMutation, ProviderStateRecord, ProviderStateStore,
    ProviderStateWrite, StoreError, WorkRecord, WorkState, WorkStore,
};
use std::sync::{Arc, Barrier};

const NAMESPACE: &str = "contract-gap-provider";

fn work() -> WorkRecord {
    let limits = InvocationLimits::default();
    WorkRecord {
        work_id: "authority-gap-work".into(),
        execution_id: ExecutionId::new("authority-gap-execution", limits).unwrap(),
        required_selector: Selector::new("program:GAP", limits).unwrap(),
        required_generation: "authority-gap@1".into(),
        artifact: ArtifactRef::new("artifact:gap", limits).unwrap(),
        state: WorkState::Queued,
        priority: 1,
        attempt: 0,
        max_attempts: 4,
        available_tick: 1,
        deadline_tick: 100,
        cancellation_requested: false,
        worker_id: None,
        lease_id: None,
        lease_epoch: 0,
        lease_expiry_tick: None,
        heartbeat_tick: None,
        terminal_tick: None,
        checkpoint_id: None,
        effect_sequence: 1,
        payload: vec![1],
    }
}

fn put(key: &str, version: u64, value: u8) -> ProviderStateMutation {
    ProviderStateMutation::Put(ProviderStateWrite {
        record: ProviderStateRecord {
            namespace: NAMESPACE.into(),
            key: key.into(),
            version,
            payload: vec![value],
        },
        expected_version: (version > 1).then_some(version - 1),
    })
}

fn seed(store: &dyn PlatformStore) -> WorkRecord {
    store.enqueue(work()).unwrap();
    store
        .mutate_provider_states_atomic(vec![put("image", 1, 0)])
        .unwrap();
    store
        .claim("old-owner", Some("authority-gap@1"), 1, 4)
        .unwrap()
        .unwrap()
}

fn reclaim_then_publish(first: &dyn PlatformStore, second: &dyn PlatformStore, stale: &WorkRecord) {
    // The first owner has observed valid work and its provider row. A real second
    // store/owner replaces the lease before that owner's provider publication.
    let observed = first.get_work(&stale.work_id).unwrap().unwrap();
    assert_eq!(observed, *stale);
    let current = second
        .claim("new-owner", Some("authority-gap@1"), 5, 10)
        .unwrap()
        .unwrap();
    assert_eq!(current.lease_epoch, stale.lease_epoch + 1);
    assert_ne!(current.lease_id, stale.lease_id);
    first
        .mutate_provider_states_atomic(vec![put("image", 2, 1), put("receipt", 1, 1)])
        .unwrap();
    assert_eq!(
        first.complete(
            &stale.work_id,
            stale.lease_id.as_deref().unwrap(),
            stale.lease_epoch,
            5
        ),
        Err(StoreError::LeaseConflict)
    );
    assert_eq!(first.get_work(&stale.work_id).unwrap(), Some(current));
    assert_eq!(
        first
            .get_provider_state(NAMESPACE, "image")
            .unwrap()
            .unwrap()
            .payload,
        vec![1]
    );
    assert!(
        first
            .get_provider_state(NAMESPACE, "receipt")
            .unwrap()
            .is_some()
    );
}

fn competing_provider_cas(first: Arc<dyn PlatformStore>, second: Arc<dyn PlatformStore>) {
    let barrier = Arc::new(Barrier::new(2));
    let handles: Vec<_> = [first.clone(), second]
        .into_iter()
        .enumerate()
        .map(|(i, store)| {
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                // Both owners hold the same actual provider version, not a fake error.
                assert_eq!(
                    store
                        .get_provider_state(NAMESPACE, "image")
                        .unwrap()
                        .unwrap()
                        .version,
                    2
                );
                barrier.wait();
                store.mutate_provider_states_atomic(vec![
                    put("image", 3, u8::try_from(i + 2).unwrap()),
                    put(&format!("contender-{i}"), 1, 1),
                ])
            })
        })
        .collect();
    let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|r| **r == Err(StoreError::Conflict))
            .count(),
        1
    );
    let rows = first.list_provider_state(NAMESPACE, 16).unwrap();
    assert_eq!(
        rows.iter()
            .filter(|r| r.key.starts_with("contender-"))
            .count(),
        1
    );
    assert_eq!(
        first
            .get_provider_state(NAMESPACE, "image")
            .unwrap()
            .unwrap()
            .version,
        3
    );
    // Even a current WorkStore cancellation observation cannot be supplied as
    // an atomic predicate to this API. The provider receipt still publishes.
    let cancelled = first.request_cancellation("authority-gap-work").unwrap();
    first
        .mutate_provider_states_atomic(vec![put("image", 4, 4), put("cancel-gap", 1, 4)])
        .unwrap();
    assert_eq!(
        first.get_work("authority-gap-work").unwrap(),
        Some(cancelled)
    );
}

#[test]
fn memory_provider_cas_does_not_fence_work_authority() {
    let store = Arc::new(MemoryStore::new(Default::default()));
    let stale = seed(&*store);
    reclaim_then_publish(&*store, &*store, &stale);
    competing_provider_cas(store.clone(), store);
}

#[test]
fn sqlite_provider_cas_does_not_fence_work_authority() {
    let dir = std::env::temp_dir().join(format!("provider-work-gap-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let url = format!("sqlite:{}?mode=rwc", dir.join("state.db").display());
    let first = Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262144).unwrap());
    let second = Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262144).unwrap());
    let stale = seed(&*first);
    reclaim_then_publish(&*first, &*second, &stale);
    competing_provider_cas(first, second);
    let reopened = SqliteStateStore::open(&url, 64 * 1024 * 1024, 262144).unwrap();
    assert_eq!(
        reopened
            .get_provider_state(NAMESPACE, "image")
            .unwrap()
            .unwrap()
            .version,
        4
    );
    assert_eq!(
        reopened
            .get_work("authority-gap-work")
            .unwrap()
            .unwrap()
            .lease_epoch,
        2
    );
    drop(reopened);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn sqlite_work_publication_gap_survives_processes() {
    let dir =
        std::env::temp_dir().join(format!("provider-work-process-gap-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let url = format!("sqlite:{}?mode=rwc", dir.join("state.db").display());
    for phase in ["seed", "reclaim-publish", "observe"] {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "sqlite_gap_process_phase", "--nocapture"])
            .env("MAINFRAME_ENV_WORK_GAP_URL", &url)
            .env("MAINFRAME_ENV_WORK_GAP_PHASE", phase)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
        assert!(
            String::from_utf8_lossy(&output.stdout)
                .contains(&format!("substantive gap phase: {phase}"))
        );
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn sqlite_gap_process_phase() {
    let Ok(url) = std::env::var("MAINFRAME_ENV_WORK_GAP_URL") else {
        return;
    };
    let phase = std::env::var("MAINFRAME_ENV_WORK_GAP_PHASE").unwrap();
    let store = SqliteStateStore::open(&url, 64 * 1024 * 1024, 262144).unwrap();
    match phase.as_str() {
        "seed" => {
            seed(&store);
        }
        "reclaim-publish" => {
            let stale = store.get_work("authority-gap-work").unwrap().unwrap();
            reclaim_then_publish(&store, &store, &stale);
        }
        "observe" => {
            let retained = store.get_work("authority-gap-work").unwrap().unwrap();
            assert_eq!(
                (retained.state, retained.lease_epoch),
                (WorkState::Claimed, 2)
            );
            assert_eq!(
                store
                    .get_provider_state(NAMESPACE, "image")
                    .unwrap()
                    .unwrap()
                    .version,
                2
            );
            assert!(
                store
                    .get_provider_state(NAMESPACE, "receipt")
                    .unwrap()
                    .is_some()
            );
        }
        _ => panic!("unknown process phase"),
    }
    println!("substantive gap phase: {phase}");
}
