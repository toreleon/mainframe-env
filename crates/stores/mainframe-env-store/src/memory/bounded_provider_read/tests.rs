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

#[test]
fn bounded_memory_exact_namespace_ignores_adjacent_prefixes_and_foreign_corruption() {
    let store = MemoryStore::new(StoreLimits::default());
    for row in [
        record("z", b"last"),
        record("a", b"first"),
        record("é", b"utf8"),
    ] {
        store.put_provider_state(row, None).unwrap();
    }
    // These namespaces sort before/after the exact page. Their oversized, invalid
    // rows must neither enter its result nor consume its byte/validation budget.
    for namespace in ["pag", "page\0", "page-", "page0", "pageé", "pagé", "é"] {
        let mut foreign = record("a", &[42; 128]);
        foreign.namespace = namespace.into();
        foreign.version = 0;
        store
            .lock()
            .unwrap()
            .provider_state
            .insert((namespace.into(), "a".into()), foreign);
    }
    let before = footprint(&store);
    let expected = vec![
        record("a", b"first"),
        record("z", b"last"),
        record("é", b"utf8"),
    ];
    assert_eq!(
        store.list_provider_state_bounded("page", 3, 29).unwrap(),
        expected
    );
    assert_eq!(store.list_provider_state("page", 3).unwrap(), expected);
    assert_eq!(
        store.list_provider_state_bounded("page", 1, 10).unwrap(),
        expected[..1]
    );
    for absent in ["paf", "page!", "page1", "zz"] {
        assert!(
            store
                .list_provider_state_bounded(absent, 1, 1)
                .unwrap()
                .is_empty()
        );
        assert!(store.list_provider_state(absent, 1).unwrap().is_empty());
    }
    assert_eq!(footprint(&store), before);
}

#[test]
fn bounded_memory_range_still_refuses_an_empty_first_physical_key() {
    let store = MemoryStore::new(StoreLimits::default());
    store
        .put_provider_state(record("a", b"good"), None)
        .unwrap();
    let malformed = record("", b"bad");
    store
        .lock()
        .unwrap()
        .provider_state
        .insert(("page".into(), String::new()), malformed.clone());
    let before = footprint(&store);
    assert_eq!(
        store.list_provider_state_bounded("page", 1, 7),
        Err(StoreError::IncompatibleVersion)
    );
    // Legacy reads retain their existing unvalidated row behavior.
    assert_eq!(store.list_provider_state("page", 1).unwrap(), [malformed]);
    assert_eq!(footprint(&store), before);
}

#[test]
fn legacy_memory_range_preserves_empty_and_unicode_namespace_identity() {
    let store = MemoryStore::new(StoreLimits::default());
    let mut empty = record("a", b"empty-namespace");
    empty.namespace.clear();
    store
        .lock()
        .unwrap()
        .provider_state
        .insert((String::new(), "a".into()), empty.clone());
    let mut unicode = record("\0", b"unicode-namespace");
    unicode.namespace = "é".into();
    store.put_provider_state(unicode.clone(), None).unwrap();
    let mut neighbor = record("a", b"neighbor");
    neighbor.namespace = "é\0".into();
    store.put_provider_state(neighbor, None).unwrap();
    let before = footprint(&store);
    assert_eq!(store.list_provider_state("", 2).unwrap(), [empty]);
    assert_eq!(
        store.list_provider_state("é", 2).unwrap(),
        [unicode.clone()]
    );
    let bytes = unicode.namespace.len() + unicode.key.len() + unicode.payload.len();
    assert_eq!(
        store.list_provider_state_bounded("é", 2, bytes).unwrap(),
        [unicode]
    );
    assert_eq!(
        store.list_provider_state_bounded("", 2, bytes),
        Err(StoreError::IncompatibleVersion)
    );
    assert_eq!(footprint(&store), before);
}

#[test]
fn legacy_memory_oversized_namespace_keeps_borrowed_physical_prefix() {
    let store = MemoryStore::new(StoreLimits::default());
    let namespace = "x".repeat(mainframe_env_store_api::MAX_PROVIDER_NAMESPACE_BYTES + 1);
    let mut expected = Vec::new();
    for key in ["a", "z"] {
        let mut row = record(key, b"physical");
        row.namespace = namespace.clone();
        store
            .lock()
            .unwrap()
            .provider_state
            .insert((namespace.clone(), key.into()), row.clone());
        expected.push(row);
    }
    let before = footprint(&store);
    assert_eq!(
        store.list_provider_state(&namespace, 1).unwrap(),
        expected[..1]
    );
    assert_eq!(store.list_provider_state(&namespace, 2).unwrap(), expected);
    assert!(
        store
            .list_provider_state(&(namespace.clone() + "!"), 1)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        store.list_provider_state_bounded(&namespace, 1, 4096),
        Err(StoreError::IncompatibleVersion)
    );
    assert_eq!(
        store.list_provider_state(&namespace, 0),
        Err(StoreError::CapacityExceeded)
    );
    assert_eq!(footprint(&store), before);
}
