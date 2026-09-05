//! One behavioral suite for all provider-state move implementations.
use mainframe_env_store::{MemoryStore, PostgresStateStore, SqliteStateStore, StoreLimits};
use mainframe_env_store_api::{
    ProviderStateMutation, ProviderStateRecord, ProviderStateStore, ProviderStateWrite, StoreError,
};
use std::sync::atomic::{AtomicU64, Ordering};

const MAX_PAYLOAD: usize = 8;
static NEXT_NAMESPACE: AtomicU64 = AtomicU64::new(1);

fn namespace() -> String {
    format!(
        "hardening-51-{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        NEXT_NAMESPACE.fetch_add(1, Ordering::Relaxed)
    )
}

fn record(namespace: &str, key: &str, version: u64) -> ProviderStateRecord {
    ProviderStateRecord {
        namespace: namespace.into(),
        key: key.into(),
        version,
        payload: b"state".to_vec(),
    }
}

fn move_record(
    store: &dyn ProviderStateStore,
    record: ProviderStateRecord,
    old_key: &str,
    expected: u64,
    atomic: bool,
) -> Result<(), StoreError> {
    if atomic {
        store.mutate_provider_states_atomic(vec![ProviderStateMutation::Move {
            record,
            old_key: old_key.into(),
            expected_version: expected,
        }])
    } else {
        store.move_provider_state(record, old_key, expected)
    }
}

fn run_contract(store: &dyn ProviderStateStore) {
    for atomic in [false, true] {
        for case in [
            "empty-namespace",
            "empty-destination",
            "empty-source",
            "same-key",
            "zero-version",
            "wrong-successor",
            "overflow",
            "sql-version-overflow",
            "missing-source",
            "stale-source",
            "destination-collision",
            "payload-bound",
            "invalid-shape-and-payload",
        ] {
            let ns = namespace();
            let original = record(&ns, "A", 1);
            let occupied = record(&ns, "TAKEN", 1);
            store.put_provider_state(original.clone(), None).unwrap();
            store.put_provider_state(occupied.clone(), None).unwrap();
            let before = store.list_provider_state(&ns, 128).unwrap();
            let mut next = record(&ns, "B", 2);
            let mut old = "A";
            let mut expected = 1;
            let mut error = StoreError::Conflict;
            match case {
                "empty-namespace" => next.namespace.clear(),
                "empty-destination" => next.key.clear(),
                "empty-source" => old = "",
                "same-key" => next.key = "A".into(),
                "zero-version" => {
                    expected = 0;
                    next.version = 1;
                }
                "wrong-successor" => next.version = 3,
                "overflow" => {
                    expected = u64::MAX;
                    next.version = 0;
                }
                "sql-version-overflow" => {
                    expected = i64::MAX as u64;
                    next.version = expected + 1;
                }
                "missing-source" => old = "MISSING",
                "stale-source" => {
                    expected = 2;
                    next.version = 3;
                }
                "destination-collision" => next.key = "TAKEN".into(),
                "payload-bound" => {
                    next.payload = vec![0; MAX_PAYLOAD + 1];
                    error = StoreError::PayloadTooLarge;
                }
                "invalid-shape-and-payload" => {
                    next.key.clear();
                    next.payload = vec![0; MAX_PAYLOAD + 1];
                }
                _ => unreachable!(),
            }
            assert_eq!(
                move_record(store, next, old, expected, atomic),
                Err(error),
                "{case}, atomic={atomic}"
            );
            assert_eq!(
                store.list_provider_state(&ns, 128).unwrap(),
                before,
                "{case} mutated state"
            );
            assert!(store.list_provider_state("", 128).unwrap().is_empty());
            store.delete_provider_state(&ns, "A", 1).unwrap();
            store.delete_provider_state(&ns, "TAKEN", 1).unwrap();
        }

        let ns = namespace();
        store.put_provider_state(record(&ns, "A", 1), None).unwrap();
        let mut second = record(&ns, "B", 2);
        second.payload = vec![0x0a; MAX_PAYLOAD];
        move_record(store, second.clone(), "A", 1, atomic).unwrap();
        assert!(store.get_provider_state(&ns, "A").unwrap().is_none());
        assert_eq!(
            store.get_provider_state(&ns, "B").unwrap(),
            Some(second.clone())
        );
        // Retrying a completed CAS does not move or overwrite anything again.
        assert_eq!(
            move_record(store, second, "A", 1, atomic),
            Err(StoreError::Conflict)
        );
        let third = record(&ns, "C", 3);
        move_record(store, third.clone(), "B", 2, atomic).unwrap();
        assert_eq!(store.list_provider_state(&ns, 128).unwrap(), vec![third]);
        store.delete_provider_state(&ns, "C", 3).unwrap();
    }

    // A late failure must roll back earlier puts AND moves in the transaction.
    for failure in [
        "invalid-shape",
        "stale-source",
        "occupied-destination",
        "payload-bound",
    ] {
        let ns = namespace();
        let original = record(&ns, "A", 1);
        store.put_provider_state(original.clone(), None).unwrap();
        let mut invalid = record(&ns, "C", 3);
        let mut expected = 2;
        let mut error = StoreError::Conflict;
        match failure {
            "invalid-shape" => invalid.key.clear(),
            "stale-source" => {
                expected = 99;
                invalid.version = 100;
            }
            "occupied-destination" => invalid.key = "MARKER".into(),
            "payload-bound" => {
                invalid.payload = vec![0; MAX_PAYLOAD + 1];
                error = StoreError::PayloadTooLarge;
            }
            _ => unreachable!(),
        }
        assert_eq!(
            store.mutate_provider_states_atomic(vec![
                ProviderStateMutation::Put(ProviderStateWrite {
                    record: record(&ns, "MARKER", 1),
                    expected_version: None
                }),
                ProviderStateMutation::Move {
                    record: record(&ns, "B", 2),
                    old_key: "A".into(),
                    expected_version: 1
                },
                ProviderStateMutation::Move {
                    record: invalid,
                    old_key: "B".into(),
                    expected_version: expected
                },
            ]),
            Err(error),
            "{failure}"
        );
        assert_eq!(store.list_provider_state(&ns, 128).unwrap(), vec![original]);
        store.delete_provider_state(&ns, "A", 1).unwrap();
    }
}

#[test]
fn memory_move_contract() {
    run_contract(&MemoryStore::new(StoreLimits {
        max_blob_bytes: MAX_PAYLOAD,
        ..Default::default()
    }));
}

#[test]
fn sqlite_move_contract() {
    run_contract(&SqliteStateStore::open("sqlite::memory:", MAX_PAYLOAD, 128).unwrap());
}

#[test]
#[ignore = "requires a disposable PostgreSQL 18 server in MAINFRAME_ENV_TEST_POSTGRES_URL"]
fn postgres_move_contract() {
    let url = std::env::var("MAINFRAME_ENV_TEST_POSTGRES_URL")
        .expect("PostgreSQL parity cannot be credited without a real test database");
    run_contract(&PostgresStateStore::open(&url, MAX_PAYLOAD, 128).unwrap());
}
