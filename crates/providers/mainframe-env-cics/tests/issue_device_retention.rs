use mainframe_env_cics::{
    CicsPartitionInput, bts_lifecycle::BtsReplayContext, prune_issue_device_receipts,
};
use mainframe_env_execution_api::{AuditRecord, ExecutionId};
use mainframe_env_store::{MemoryStore, SqliteStateStore};
use mainframe_env_store_api::{
    AuditSink, ProviderStateMutation, ProviderStateRecord, ProviderStateStore, ProviderStateWrite,
    StoreError,
};
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

const NAMESPACE: &str = "cics-issue-device-receipt-v1";
// Frozen historical bytes, independent of the private product serializer.
const HISTORICAL: &[u8] = br#"{"schema_version":1,"effect_key":"effect-1","owner_execution":"execution","owner_run_unit":"run","owner_principal":"IBMUSER","owner_epoch":1,"mutation_sequence":1,"request_digest":[7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,7],"deadline_tick":100,"retain_until_tick":120,"condition":"NORMAL","response":0,"response2":0}"#;

fn row(key: &str) -> ProviderStateRecord {
    ProviderStateRecord {
        namespace: NAMESPACE.into(),
        key: key.into(),
        version: 1,
        payload: std::str::from_utf8(HISTORICAL)
            .unwrap()
            .replace("effect-1", key)
            .into_bytes(),
    }
}

struct SqliteFixture {
    directory: PathBuf,
    url: String,
}

impl SqliteFixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let directory = std::env::temp_dir().join(format!(
            "cics-facade-retention-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed),
        ));
        std::fs::create_dir(&directory).unwrap();
        let url = format!(
            "sqlite://{}?mode=rwc",
            directory.join("state.sqlite").display()
        );
        Self { directory, url }
    }

    fn open(&self) -> SqliteStateStore {
        SqliteStateStore::open(&self.url, 4 * 1024 * 1024, 65_536).unwrap()
    }
}

impl Drop for SqliteFixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.directory).unwrap();
    }
}

fn on_backends(test: impl Fn(&dyn ProviderStateStore)) {
    test(&MemoryStore::new(Default::default()));
    let fixture = SqliteFixture::new();
    test(&fixture.open());
}

fn snapshot(store: &dyn ProviderStateStore) -> Vec<ProviderStateRecord> {
    store.list_provider_state(NAMESPACE, 4097).unwrap()
}

#[test]
fn facade_contexts_borrow_exact_inputs_and_retention_refuses_zero_watermark() {
    let replay = BtsReplayContext {
        run_unit: "run-input",
        owner_execution: "execution-input",
        owner_principal: "principal-input",
        replay_key: "effect-input",
        request_digest: [0x59; 32],
    };
    assert_eq!(replay.run_unit, "run-input");
    assert_eq!(replay.owner_execution, "execution-input");
    assert_eq!(replay.owner_principal, "principal-input");
    assert_eq!(replay.replay_key, "effect-input");
    assert_eq!(replay.request_digest, [0x59; 32]);
    let data = [0, 0xff, b'a'];
    let input = CicsPartitionInput {
        aid: 0x7d,
        partition: "P",
        data: &data,
        cursor: 17,
    };
    assert_eq!(input.aid, 0x7d);
    assert_eq!(input.partition, "P");
    assert_eq!(input.data, &[0, 0xff, b'a']);
    assert_eq!(input.cursor, 17);
    let store = MemoryStore::new(Default::default());
    assert_eq!(
        prune_issue_device_receipts(&store, 0, &BTreeSet::new()),
        Err(StoreError::InvalidTransition)
    );
}

#[test]
fn public_watermark_and_protection_preserve_exact_historical_bytes() {
    on_backends(|store| {
        store.put_provider_state(row("effect-1"), None).unwrap();
        assert_eq!(snapshot(store)[0].payload, HISTORICAL);
        let before = snapshot(store);
        assert_eq!(
            prune_issue_device_receipts(store, 119, &BTreeSet::new()),
            Ok(0)
        );
        assert_eq!(snapshot(store), before);
        assert_eq!(
            prune_issue_device_receipts(store, 120, &BTreeSet::from(["effect-1".into()])),
            Ok(0)
        );
        assert_eq!(snapshot(store), before);
        assert_eq!(
            prune_issue_device_receipts(store, 120, &BTreeSet::new()),
            Ok(1)
        );
        assert!(snapshot(store).is_empty());
    });
}

#[test]
fn public_protected_count_and_zero_watermark_refuse_before_delete() {
    on_backends(|store| {
        store.put_provider_state(row("effect-1"), None).unwrap();
        let before = snapshot(store);
        assert_eq!(
            prune_issue_device_receipts(store, 0, &BTreeSet::new()),
            Err(StoreError::InvalidTransition)
        );
        assert_eq!(snapshot(store), before);
        let mut protected: BTreeSet<String> = (0..4096).map(|n| format!("reference-{n}")).collect();
        assert_eq!(prune_issue_device_receipts(store, 119, &protected), Ok(0));
        assert_eq!(snapshot(store), before);
        protected.insert("one-too-many".into());
        assert_eq!(
            prune_issue_device_receipts(store, 120, &protected),
            Err(StoreError::InvalidTransition)
        );
        assert_eq!(snapshot(store), before);
    });
}

#[test]
fn public_scan_exact_bound_accepts_and_overflow_refuses_without_deleting() {
    on_backends(|store| {
        for batch in 0..64 {
            store
                .put_provider_states_atomic(
                    (0..64)
                        .map(|offset| ProviderStateWrite {
                            record: row(&format!("receipt-{:04}", batch * 64 + offset)),
                            expected_version: None,
                        })
                        .collect(),
                )
                .unwrap();
        }
        let before = snapshot(store);
        assert_eq!(before.len(), 4096);
        assert_eq!(
            prune_issue_device_receipts(store, 119, &BTreeSet::new()),
            Ok(0)
        );
        assert_eq!(snapshot(store), before);
        store.put_provider_state(row("receipt-4096"), None).unwrap();
        let full = snapshot(store);
        assert_eq!(full.len(), 4097);
        assert_eq!(
            prune_issue_device_receipts(store, 120, &BTreeSet::new()),
            Err(StoreError::CapacityExceeded)
        );
        assert_eq!(snapshot(store), full);
    });
}

#[test]
fn public_corrupt_unknown_key_and_noncanonical_rows_refuse_before_delete() {
    let text = std::str::from_utf8(HISTORICAL).unwrap();
    let mut variants = vec![
        ProviderStateRecord {
            version: 2,
            ..row("effect-1")
        },
        ProviderStateRecord {
            key: "different-key".into(),
            ..row("effect-1")
        },
        ProviderStateRecord {
            payload: vec![b'x'; 1025],
            ..row("effect-1")
        },
        ProviderStateRecord {
            payload: b"{".to_vec(),
            ..row("effect-1")
        },
        ProviderStateRecord {
            payload: format!(" {text}").into_bytes(),
            ..row("effect-1")
        },
    ];
    for (old, new) in [
        ("\"schema_version\":1", "\"schema_version\":2"),
        ("\"owner_epoch\":1", "\"owner_epoch\":0"),
        ("\"mutation_sequence\":1", "\"mutation_sequence\":0"),
        ("\"deadline_tick\":100", "\"deadline_tick\":0"),
        ("\"retain_until_tick\":120", "\"retain_until_tick\":99"),
        ("\"condition\":\"NORMAL\"", "\"condition\":\"UNKNOWN\""),
        ("\"response\":0", "\"response\":16"),
        ("\"response2\":0", "\"response2\":1"),
        (
            "\"owner_execution\":\"execution\"",
            "\"owner_execution\":\"\"",
        ),
        ("\"schema_version\":1", "\"extra\":0,\"schema_version\":1"),
        (
            "\"schema_version\":1",
            "\"schema_version\":1,\"schema_version\":1",
        ),
    ] {
        variants.push(ProviderStateRecord {
            payload: text.replace(old, new).into_bytes(),
            ..row("effect-1")
        });
    }
    on_backends(|store| {
        for invalid in &variants {
            if invalid.version == 2 {
                store.put_provider_state(row(&invalid.key), None).unwrap();
                store.put_provider_state(invalid.clone(), Some(1)).unwrap();
            } else {
                store.put_provider_state(invalid.clone(), None).unwrap();
            }
            let before = snapshot(store);
            let protected = BTreeSet::from([invalid.key.clone()]);
            assert_eq!(
                prune_issue_device_receipts(store, 120, &protected),
                Err(StoreError::IncompatibleVersion)
            );
            assert_eq!(snapshot(store), before);
            assert_eq!(
                prune_issue_device_receipts(store, 120, &BTreeSet::new()),
                Err(StoreError::IncompatibleVersion)
            );
            assert_eq!(snapshot(store), before);
            store
                .delete_provider_state(NAMESPACE, &invalid.key, invalid.version)
                .unwrap();
        }
    });
}

#[test]
fn public_payload_exact_bound_is_canonical_and_larger_payload_refuses() {
    on_backends(|store| {
        let mut exact = std::str::from_utf8(HISTORICAL)
            .unwrap()
            .replace("effect-1", &"k".repeat(256))
            .replace("\"execution\"", &format!("\"{}\"", "e".repeat(128)))
            .replace("\"run\"", &format!("\"{}\"", "r".repeat(128)))
            .replace("\"owner_epoch\":1", "\"owner_epoch\":18446744073709551615")
            .replace(
                &format!("[{}]", ["7"; 32].join(",")),
                &format!("[{}]", ["255"; 32].join(",")),
            );
        let remaining = 1024 - exact.len() + "IBMUSER".len();
        assert!(remaining <= 128);
        exact = exact.replace("IBMUSER", &"p".repeat(remaining));
        let mut retained = row(&"k".repeat(256));
        retained.payload = exact.into_bytes();
        assert_eq!(retained.payload.len(), 1024);
        store.put_provider_state(retained.clone(), None).unwrap();
        assert_eq!(
            prune_issue_device_receipts(store, 119, &BTreeSet::new()),
            Ok(0)
        );
        assert_eq!(snapshot(store), vec![retained.clone()]);
        store
            .delete_provider_state(NAMESPACE, &retained.key, 1)
            .unwrap();
        retained.payload.push(b' ');
        store.put_provider_state(retained.clone(), None).unwrap();
        assert_eq!(
            prune_issue_device_receipts(store, 120, &BTreeSet::new()),
            Err(StoreError::IncompatibleVersion)
        );
        assert_eq!(snapshot(store), vec![retained]);
    });
}

#[test]
fn public_later_corruption_keeps_prior_conditional_deletion() {
    on_backends(|store| {
        store.put_provider_state(row("a-first"), None).unwrap();
        let invalid = ProviderStateRecord {
            payload: b"corrupt".to_vec(),
            ..row("z-last")
        };
        store.put_provider_state(invalid.clone(), None).unwrap();
        assert_eq!(
            prune_issue_device_receipts(store, 120, &BTreeSet::new()),
            Err(StoreError::IncompatibleVersion)
        );
        assert_eq!(snapshot(store), vec![invalid]);
    });
}

struct DeleteConflict<'a> {
    store: &'a dyn ProviderStateStore,
}

impl AuditSink for DeleteConflict<'_> {
    fn record_audit(&self, record: AuditRecord) -> Result<(), StoreError> {
        self.store.record_audit(record)
    }
    fn audit_records(
        &self,
        execution: &ExecutionId,
        sequence: u64,
        max: usize,
    ) -> Result<Vec<AuditRecord>, StoreError> {
        self.store.audit_records(execution, sequence, max)
    }
}

impl ProviderStateStore for DeleteConflict<'_> {
    fn get_provider_state(
        &self,
        namespace: &str,
        key: &str,
    ) -> Result<Option<ProviderStateRecord>, StoreError> {
        self.store.get_provider_state(namespace, key)
    }
    fn list_provider_state(
        &self,
        namespace: &str,
        max: usize,
    ) -> Result<Vec<ProviderStateRecord>, StoreError> {
        self.store.list_provider_state(namespace, max)
    }
    fn put_provider_state(
        &self,
        record: ProviderStateRecord,
        expected: Option<u64>,
    ) -> Result<(), StoreError> {
        self.store.put_provider_state(record, expected)
    }
    fn delete_provider_state(
        &self,
        namespace: &str,
        key: &str,
        expected: u64,
    ) -> Result<(), StoreError> {
        if key == "z-last" {
            self.store
                .delete_provider_state(namespace, key, expected + 1)
        } else {
            self.store.delete_provider_state(namespace, key, expected)
        }
    }
    fn move_provider_state(
        &self,
        record: ProviderStateRecord,
        old: &str,
        expected: u64,
    ) -> Result<(), StoreError> {
        self.store.move_provider_state(record, old, expected)
    }
    fn put_provider_states_atomic(
        &self,
        writes: Vec<ProviderStateWrite>,
    ) -> Result<(), StoreError> {
        self.store.put_provider_states_atomic(writes)
    }
    fn mutate_provider_states_atomic(
        &self,
        mutations: Vec<ProviderStateMutation>,
    ) -> Result<(), StoreError> {
        self.store.mutate_provider_states_atomic(mutations)
    }
}

#[test]
fn public_later_delete_cas_conflict_keeps_prior_deletion_and_exact_remaining_row() {
    on_backends(|store| {
        store.put_provider_state(row("a-first"), None).unwrap();
        let remaining = row("z-last");
        store.put_provider_state(remaining.clone(), None).unwrap();
        assert_eq!(
            prune_issue_device_receipts(&DeleteConflict { store }, 120, &BTreeSet::new()),
            Err(StoreError::Conflict)
        );
        assert_eq!(snapshot(store), vec![remaining]);
    });
}

#[test]
fn public_historical_receipt_reopens_and_deleted_row_stays_absent_on_sqlite() {
    let fixture = SqliteFixture::new();
    {
        let store = fixture.open();
        store.put_provider_state(row("effect-1"), None).unwrap();
        assert_eq!(
            prune_issue_device_receipts(&store, 119, &BTreeSet::new()),
            Ok(0)
        );
    }
    {
        let store = fixture.open();
        assert_eq!(snapshot(&store)[0].payload, HISTORICAL);
        assert_eq!(
            prune_issue_device_receipts(&store, 120, &BTreeSet::new()),
            Ok(1)
        );
    }
    assert!(snapshot(&fixture.open()).is_empty());
}
