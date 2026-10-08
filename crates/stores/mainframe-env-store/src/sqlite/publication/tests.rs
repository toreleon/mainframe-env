use super::*;
use crate::publication::tests::{prepare, put};
use mainframe_env_store_api::{AuditSink, ExecutionStore, IdempotencyStore};
use std::sync::{
    Arc, Barrier,
    atomic::{AtomicU64, Ordering},
};

struct Fixture(std::path::PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        for _ in 0..64 {
            let path = std::env::temp_dir().join(format!(
                "audited-publication-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match std::fs::create_dir(&path) {
                Ok(()) => return Self(path),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => panic!("fixture allocation: {e}"),
            }
        }
        panic!("fixture allocation bound");
    }
    fn url(&self) -> String {
        format!("sqlite://{}/state.db?mode=rwc", self.0.display())
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

type RawSnapshot = (Vec<(String, String, i64, Vec<u8>)>, (i64, i64));

fn raw_state(store: &SqliteStateStore) -> RawSnapshot {
    let rows = store
        .run(
            sqlx::query_as(
                "SELECT namespace,key,version,payload FROM provider_state ORDER BY namespace,key",
            )
            .fetch_all(&store.pool),
        )
        .unwrap();
    let counters = store
        .run(
            sqlx::query_as("SELECT epoch,clock_tick FROM retention_lock WHERE singleton=1")
                .fetch_one(&store.pool),
        )
        .unwrap();
    (rows, counters)
}

#[test]
fn audited_publication_sql_faults_rollback_exact_rows_and_retention_counters() {
    for fault in 0..6 {
        let mut store = SqliteStateStore::open("sqlite::memory:", 8192, 64).unwrap();
        let mut request = prepare(&store);
        request.mutations = vec![put("queue", 1, None, b"x")];
        match fault {
            0 => {
                store.run(sqlx::query("CREATE TRIGGER abort_audit BEFORE INSERT ON provider_state WHEN NEW.namespace='durable-audit-v1' BEGIN SELECT RAISE(ABORT,'audit insert fault'); END").execute(&store.pool)).unwrap();
            }
            1 => store.max_payload_bytes = 1,
            2 => store.max_rows = 2,
            3 => {
                store
                    .run(
                        sqlx::query("UPDATE retention_lock SET epoch=? WHERE singleton=1")
                            .bind(i64::MAX)
                            .execute(&store.pool),
                    )
                    .unwrap();
            }
            4 => {
                store.run(sqlx::query("CREATE TRIGGER abort_clock BEFORE UPDATE OF clock_tick ON retention_lock WHEN NEW.clock_tick=9 BEGIN SELECT RAISE(ABORT,'clock write fault'); END").execute(&store.pool)).unwrap();
            }
            5 => store.max_rows = 3, // The provider row fits, its required audit does not.
            _ => unreachable!(),
        }
        let before = raw_state(&store);
        let error = store
            .publish_provider_states_audited(request.clone())
            .unwrap_err();
        match fault {
            0 | 4 => assert!(matches!(error, StoreError::Infrastructure(_))),
            1 => assert_eq!(error, StoreError::PayloadTooLarge),
            _ => assert_eq!(error, StoreError::CapacityExceeded),
        };
        assert_eq!(raw_state(&store), before, "fault {fault}");
        if fault == 2 {
            let mut denial = request.clone();
            denial.mutations.clear();
            denial.audit.decision = mainframe_env_execution_api::AuditDecision::Deny;
            assert_eq!(
                store.publish_provider_states_audited(denial),
                Err(StoreError::CapacityExceeded)
            );
            assert_eq!(raw_state(&store), before);
        }
        assert_eq!(
            store.effect(&request.intent.key).unwrap(),
            Some(request.intent.clone())
        );
        if fault == 0 {
            store
                .run(sqlx::query("DROP TRIGGER abort_audit").execute(&store.pool))
                .unwrap();
            request.observed_tick = 8;
            request.audit.observed_tick = 8;
            store
                .publish_provider_states_audited(request.clone())
                .unwrap();
            let keys: Vec<String> = store
                .run(
                    sqlx::query_scalar(
                        "SELECT key FROM provider_state WHERE namespace='durable-audit-v1'",
                    )
                    .fetch_all(&store.pool),
                )
                .unwrap();
            assert_eq!(keys.len(), 1);
            assert!(keys[0].ends_with("direct:00000000000000000001"));
            assert_eq!(
                store
                    .audit_records(&request.intent.execution_id, 1, 8)
                    .unwrap(),
                vec![request.audit]
            );
        }
    }
}

#[test]
fn audited_publication_sqlite_reopen_preserves_exact_rows_audit_and_live_intent() {
    let fixture = Fixture::new();
    let url = fixture.url();
    let store = SqliteStateStore::open(&url, 8192, 64).unwrap();
    let request = prepare(&store);
    store
        .publish_provider_states_audited(request.clone())
        .unwrap();
    let before = raw_state(&store);
    let execution = store.get_execution(&request.intent.execution_id).unwrap();
    drop(store);
    let reopened = SqliteStateStore::open(&url, 8192, 64).unwrap();
    assert_eq!(raw_state(&reopened), before);
    assert_eq!(
        reopened.effect(&request.intent.key).unwrap(),
        Some(request.intent.clone())
    );
    assert_eq!(
        reopened
            .get_execution(&request.intent.execution_id)
            .unwrap(),
        execution
    );
    assert_eq!(
        reopened
            .audit_records(&request.intent.execution_id, 1, 8)
            .unwrap(),
        vec![request.audit.clone()]
    );
    assert_eq!(
        reopened.publish_provider_states_audited(request),
        Err(StoreError::Conflict)
    );
    assert_eq!(raw_state(&reopened), before);
}

#[test]
fn audited_publication_distinct_sqlite_connections_cas_one_winner() {
    let fixture = Fixture::new();
    let url = fixture.url();
    let first = Arc::new(SqliteStateStore::open(&url, 8192, 64).unwrap());
    let mut request = prepare(first.as_ref());
    first
        .mutate_provider_states_atomic(vec![put("queue", 1, None, b"before")])
        .unwrap();
    request.mutations = vec![put("queue", 2, Some(1), b"after")];
    let second = Arc::new(SqliteStateStore::open(&url, 8192, 64).unwrap());
    let barrier = Arc::new(Barrier::new(3));
    let threads: Vec<_> = [first.clone(), second.clone()]
        .into_iter()
        .map(|store| {
            let request = request.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                store.publish_provider_states_audited(request)
            })
        })
        .collect();
    barrier.wait();
    let results: Vec<_> = threads.into_iter().map(|t| t.join().unwrap()).collect();
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|r| **r == Err(StoreError::Conflict))
            .count(),
        1
    );
    assert_eq!(
        first
            .audit_records(&request.intent.execution_id, 1, 8)
            .unwrap(),
        vec![request.audit]
    );
    assert_eq!(
        first
            .get_provider_state("publication-fixture-v1", "queue")
            .unwrap()
            .unwrap()
            .version,
        2
    );
}
