use super::*;
use crate::provider_scan::tests::{pages, record};

fn footprint(store: &MemoryStore) -> String {
    let state = store.lock().unwrap();
    format!(
        "{:?}{:?}{:?}{:?}{:?}{:?}{:?}{:?}{:?}{:?}{:?}{:?}{:?}{:?}{:?}{:?}{:?}{:?}{:?}{:?}{:?}{:?}",
        state.executions,
        state.events,
        state.work,
        state.checkpoints,
        state.sessions,
        state.artifacts,
        state.generations,
        state.effects,
        state.audits,
        state.next_audit_ordinal,
        state.outbox,
        state.provider_state,
        state.blob_bytes,
        state.archives,
        state.archive_bytes,
        state.observations,
        state.observation_bytes,
        state.retention_cas_versions,
        state.provider_epoch,
        state.logical_tick,
        store.limits,
        store.clone_count.load(std::sync::atomic::Ordering::Relaxed)
    )
}

#[test]
fn bounded_memory_pages_and_legacy_parity() {
    pages(&MemoryStore::new(StoreLimits::default()));
}

#[test]
fn bounded_memory_whole_state_is_unchanged_on_success_and_failure() {
    let store = MemoryStore::new(StoreLimits::default());
    store
        .put_provider_state(record("a", b"first"), None)
        .unwrap();
    store.advance_logical_clock(99).unwrap();
    let before = footprint(&store);
    assert_eq!(
        store.list_provider_state_bounded("page", 2, 10).unwrap(),
        [record("a", b"first")]
    );
    assert_eq!(
        store.list_provider_state_bounded("page", 2, 9),
        Err(StoreError::CapacityExceeded)
    );
    assert_eq!(footprint(&store), before);
}

#[test]
fn bounded_memory_preflight_refuses_entire_malformed_page_before_clone() {
    for kind in 0..6 {
        let store = MemoryStore::new(StoreLimits {
            max_blob_bytes: 8,
            ..StoreLimits::default()
        });
        store
            .put_provider_state(record("a", b"good"), None)
            .unwrap();
        let mut malformed = record("z", b"bad");
        match kind {
            0 => malformed.version = 0,
            1 => malformed.version = i64::MAX as u64 + 1,
            2 => malformed.key = String::new(),
            3 => malformed.key = "x".repeat(mainframe_env_store_api::MAX_PROVIDER_KEY_BYTES + 1),
            4 => malformed.namespace = "other".into(),
            5 => malformed.payload = vec![0; 9],
            _ => unreachable!(),
        }
        // Corrupt physical fixture only; never passed as admission or executable state.
        store
            .lock()
            .unwrap()
            .provider_state
            .insert(("page".into(), "z".into()), malformed);
        let before = footprint(&store);
        let expected = if kind == 5 {
            StoreError::PayloadTooLarge
        } else {
            StoreError::IncompatibleVersion
        };
        assert_eq!(
            store.list_provider_state_bounded("page", 2, 2048),
            Err(expected)
        );
        assert_eq!(footprint(&store), before);
        // First page is independent of the later corrupt record.
        assert_eq!(
            store.list_provider_state_bounded("page", 1, 9).unwrap(),
            [record("a", b"good")]
        );
    }
}

#[test]
fn bounded_memory_byte_refusal_precedes_corrupt_version_and_payload_clone() {
    let store = MemoryStore::new(StoreLimits::default());
    let mut row = record("a", &[0; 32]);
    row.version = 0;
    store
        .lock()
        .unwrap()
        .provider_state
        .insert(("page".into(), "a".into()), row);
    let before = footprint(&store);
    assert_eq!(
        store.list_provider_state_bounded("page", 1, 36),
        Err(StoreError::CapacityExceeded)
    );
    assert_eq!(
        store.list_provider_state_bounded("page", 1, 37),
        Err(StoreError::IncompatibleVersion)
    );
    assert_eq!(footprint(&store), before);
}
