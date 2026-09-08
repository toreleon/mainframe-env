use mainframe_env_execution_api::{ArtifactRef, ExecutionId, InvocationLimits, Selector};
use mainframe_env_store::{MemoryStore, PostgresStateStore, SqliteStateStore, StoreLimits};
use mainframe_env_store_api::{
    PlatformStore, ProviderStateRecord, ProviderStateStore, StoreError, WorkRecord, WorkState,
    WorkStore,
};
use serde_json::json;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

fn work(label: &str, deadline_tick: u64) -> WorkRecord {
    let suffix = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let limits = InvocationLimits::default();
    WorkRecord {
        work_id: format!("lease-{label}-{suffix}"),
        execution_id: ExecutionId::new(format!("exec-{label}-{suffix}"), limits).unwrap(),
        required_selector: Selector::new("program:LEASE", limits).unwrap(),
        required_generation: "lease-contract@1".into(),
        artifact: ArtifactRef::new(format!("sha256:{}", "a".repeat(64)), limits).unwrap(),
        state: WorkState::Queued,
        attempt: 0,
        max_attempts: 4,
        available_tick: 1,
        deadline_tick,
        cancellation_requested: false,
        worker_id: None,
        lease_id: None,
        lease_epoch: 0,
        lease_expiry_tick: None,
        heartbeat_tick: None,
        checkpoint_id: None,
        effect_sequence: 0,
        payload: vec![1],
    }
}

fn assert_deadlines_and_fencing(first: &dyn PlatformStore, second: &dyn PlatformStore) {
    let expired = work("queued-deadline", 5);
    let expired_id = expired.work_id.clone();
    first.enqueue(expired).unwrap();
    assert!(first.claim("worker-a", 5, 10).unwrap().is_none());
    assert_eq!(
        first.get_work(&expired_id).unwrap().unwrap().state,
        WorkState::DeadLetter
    );

    let reclaimable = work("stale-owner", 100);
    let reclaimable_id = reclaimable.work_id.clone();
    first.enqueue(reclaimable).unwrap();
    let stale = first.claim("same-worker", 10, 5).unwrap().unwrap();
    assert_eq!(stale.lease_expiry_tick, Some(15));
    let current = second.claim("same-worker", 15, 10).unwrap().unwrap();
    assert_eq!((stale.lease_epoch, current.lease_epoch), (1, 2));
    assert_ne!(stale.lease_id, current.lease_id);
    let stale_id = stale.lease_id.as_deref().unwrap();
    for result in [
        first
            .heartbeat(&reclaimable_id, stale_id, stale.lease_epoch, 15, 5)
            .map(|_| ()),
        first
            .release(&reclaimable_id, stale_id, stale.lease_epoch, 15, 16)
            .map(|_| ()),
        first
            .dead_letter(&reclaimable_id, stale_id, stale.lease_epoch, 15)
            .map(|_| ()),
        first.complete(&reclaimable_id, stale_id, stale.lease_epoch, 15),
    ] {
        assert_eq!(result, Err(StoreError::LeaseConflict));
    }
    assert_eq!(
        first.heartbeat(
            &reclaimable_id,
            current.lease_id.as_deref().unwrap(),
            current.lease_epoch,
            14,
            5,
        ),
        Err(StoreError::LeaseConflict),
        "a regressed clock must not extend a lease"
    );
    second
        .complete(
            &reclaimable_id,
            current.lease_id.as_deref().unwrap(),
            current.lease_epoch,
            16,
        )
        .unwrap();

    let bounded = work("deadline-clamp", 22);
    let bounded_id = bounded.work_id.clone();
    first.enqueue(bounded).unwrap();
    let claimed = first.claim("worker-b", 20, 100).unwrap().unwrap();
    assert_eq!(claimed.lease_expiry_tick, Some(22));
    assert_eq!(
        first.complete(
            &bounded_id,
            claimed.lease_id.as_deref().unwrap(),
            claimed.lease_epoch,
            22,
        ),
        Err(StoreError::LeaseConflict)
    );
    assert!(second.claim("worker-c", 22, 5).unwrap().is_none());
    assert_eq!(
        first.get_work(&bounded_id).unwrap().unwrap().state,
        WorkState::DeadLetter
    );
}

#[test]
fn memory_work_deadlines_and_fencing_contract() {
    let store = MemoryStore::new(StoreLimits::default());
    assert_deadlines_and_fencing(&store, &store);
}

#[test]
fn sqlite_work_deadlines_and_fencing_contract() {
    let directory = std::env::temp_dir().join(format!(
        "mainframe-env-work-lease-{}-{}",
        std::process::id(),
        NEXT_ID.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let url = format!("sqlite://{}?mode=rwc", directory.join("state.db").display());
    let first = SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap();
    let second = SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap();
    assert_deadlines_and_fencing(&first, &second);
    drop((first, second));
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn legacy_claimed_work_migrates_to_an_explicit_fencing_epoch() {
    let store = SqliteStateStore::open("sqlite::memory:", 64 * 1024 * 1024, 262_144).unwrap();
    let payload = serde_json::to_vec(&json!({
        "schema": 1,
        "id": "legacy-work",
        "execution": "legacy-execution",
        "selector": "program:LEASE",
        "generation": "lease-contract@1",
        "artifact": format!("sha256:{}", "b".repeat(64)),
        "state": "claimed",
        "attempt": 1,
        "max_attempts": 4,
        "available": 1,
        "deadline": 100,
        "cancel": false,
        "worker": "legacy-worker",
        "lease": "legacy-worker:1",
        "expiry": 20,
        "heartbeat": 10,
        "checkpoint": null,
        "effect": 0,
        "payload": "AQ"
    }))
    .unwrap();
    store
        .put_provider_state(
            ProviderStateRecord {
                namespace: "durable-work".into(),
                key: "legacy-work".into(),
                version: 1,
                payload,
            },
            None,
        )
        .unwrap();
    let legacy = store.get_work("legacy-work").unwrap().unwrap();
    assert_eq!(legacy.lease_epoch, 1);
    store
        .heartbeat("legacy-work", "legacy-worker:1", 1, 11, 5)
        .unwrap();
    let migrated = store
        .get_provider_state("durable-work", "legacy-work")
        .unwrap()
        .unwrap();
    let migrated: serde_json::Value = serde_json::from_slice(&migrated.payload).unwrap();
    assert_eq!(migrated["schema"], 2);
    assert_eq!(migrated["lease_epoch"], 1);
}

#[test]
#[ignore = "requires isolated MAINFRAME_ENV_POSTGRES_TEST_URL"]
fn postgres_work_deadlines_and_fencing_contract() {
    let url = std::env::var("MAINFRAME_ENV_POSTGRES_TEST_URL")
        .expect("explicit PostgreSQL test URL required");
    let first = PostgresStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap();
    let second = PostgresStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap();
    assert_deadlines_and_fencing(&first, &second);
}
