//! Read-only recovery protection; these tests do not admit retention or calls.

use super::*;
use crate::describe_mq_selected_retention_dependencies as describe;
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::atomic::AtomicU64;

static NEXT_DB: AtomicU64 = AtomicU64::new(1);

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
            "mq-selected-retention-{}-{}",
            std::process::id(),
            NEXT_DB.fetch_add(1, Ordering::Relaxed)
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

    fn reopen(&mut self) {
        if let Some(directory) = &self.directory {
            assert_eq!(Arc::strong_count(self.store.as_ref().unwrap()), 1);
            drop(self.store.take());
            let url = format!(
                "sqlite://{}?mode=rw",
                directory.join("state.sqlite").display()
            );
            self.store = Some(Arc::new(
                SqliteStateStore::open(&url, 64 << 20, 256).unwrap(),
            ));
        }
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

fn decode(rows: Vec<ProviderStateRecord>) -> crate::MqSelectedRetentionDependencies {
    describe(rows, MqLimits::default()).unwrap()
}

#[test]
fn memory_owned_sqlite_selected_graph_is_read_only_and_survives_reopen() {
    for sqlite in [false, true] {
        let mut backend = Backend::new(sqlite);
        let f = Fixture::from_store(backend.store());
        let c = f.connect();
        f.open(c);
        let rows = f.rows();
        let epoch = f.store.provider_state_retention_epoch().unwrap();
        let audits = f.store.audit_records(&f.inv.execution_id, 0, 256).unwrap();
        let dependencies = decode(rows.clone());
        assert_eq!(dependencies.executions, vec![f.inv.execution_id.clone()]);
        assert_eq!(dependencies.effect_keys.len(), 2);
        assert!(
            dependencies
                .effect_keys
                .iter()
                .any(|key| key.as_str() == "effect-1")
        );
        assert!(
            dependencies
                .effect_keys
                .iter()
                .any(|key| key.as_str() == "effect-2")
        );
        assert_eq!(f.rows(), rows);
        assert_eq!(f.store.provider_state_retention_epoch().unwrap(), epoch);
        assert_eq!(
            f.store.audit_records(&f.inv.execution_id, 0, 256).unwrap(),
            audits
        );
        // Optional explicit fixture generation is setup data, not call evidence.
        // The manager resolves its output path outside targets before requesting it.
        if !sqlite {
            if let Some(path) = std::env::var_os("MQ_SELECTED_RETENTION_FIXTURE_OUTPUT") {
                let value: Vec<_> = rows
                    .iter()
                    .map(|row| {
                        json!({
                            "namespace": row.namespace, "key": row.key,
                            "version": row.version, "payload": row.payload,
                        })
                    })
                    .collect();
                std::fs::write(path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
            }
        }
        drop(f);
        backend.reopen();
        let restored = backend
            .store()
            .list_provider_state_prefix("mq-", 4096)
            .unwrap();
        assert_eq!(restored, rows);
        assert_eq!(decode(restored), dependencies);
    }
}

#[test]
fn pending_empty_and_final_unit_owners_remain_protected_without_receipts() {
    for sqlite in [false, true] {
        let backend = Backend::new(sqlite);
        let f = Fixture::from_store(backend.store());
        let c = f.connect();
        let request = f.effect(
            2,
            MqMqiRequest::Commit {
                connection: c,
                unit: f.unit(),
            },
        );
        f.seed(&request);
        f.execute(&request).unwrap();
        let mut rows = f.rows();
        rows.retain(|row| row.namespace != receipt::NAMESPACE);
        let dependencies = decode(rows);
        assert_eq!(dependencies.executions, vec![f.inv.execution_id.clone()]);
        assert_eq!(dependencies.effect_keys.len(), 1);
        assert_eq!(dependencies.effect_keys[0].as_str(), "effect-1");
    }
}

#[test]
fn selected_snapshot_rejects_orphan_unknown_duplicate_and_corrupt_codec() {
    let f = Fixture::new(false);
    f.connect();
    let rows = f.rows();
    let mut cases = Vec::new();
    let mut orphan = rows.clone();
    orphan.retain(|row| row.namespace != "mq-selected-v1-control");
    cases.push(orphan);
    let mut duplicate = rows.clone();
    duplicate.push(
        rows.iter()
            .find(|row| row.namespace == receipt::NAMESPACE)
            .unwrap()
            .clone(),
    );
    cases.push(duplicate);
    let mut unknown = rows.clone();
    unknown.push(ProviderStateRecord {
        namespace: "mq-selected-unsupported".into(),
        key: "future".into(),
        version: 1,
        payload: b"{}".to_vec(),
    });
    cases.push(unknown);
    let mut corrupt = rows.clone();
    let row = corrupt
        .iter_mut()
        .find(|row| row.namespace == receipt::NAMESPACE)
        .unwrap();
    let mut value: Value = serde_json::from_slice(&row.payload).unwrap();
    value["value"]["reply"]["bytes"] = json!([255]);
    row.payload = serde_json::to_vec(&value).unwrap();
    cases.push(corrupt);
    for case in cases {
        assert!(describe(case, MqLimits::default()).is_err());
        assert_eq!(f.rows(), rows);
    }
    assert!(
        describe(
            rows,
            MqLimits {
                max_replays: 0,
                ..Default::default()
            }
        )
        .is_err()
    );
}

#[test]
fn selected_protection_delegates_full_storage_v2_without_execution_or_age() {
    let f = Fixture::new(false);
    f.connect();
    let original_rows = f.rows();
    let mut rows = original_rows.clone();
    let result = MqMqiResult {
        call: MqMqiCall::Get,
        outcome: MqMqiOutcome::StatusPending {
            output: MqMqiOutput::FullGot {
                disposition: MqGetDisposition::NoMessage,
                message: None,
                data_length: None,
                cursor: None,
            },
        },
    };
    let bytes = crate::mqi_replay::encode(
        &result,
        HostLimits::default(),
        MqMqiLimits::default(),
        MqMqiLimits::default().canonical_bytes,
    )
    .unwrap();
    let storage: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        storage["schema_version"],
        "mainframe-env.mq-mqi-result-storage@2"
    );
    let digest = canonical_result_digest(&Ok(HostResult::MqMqi(MqMqiHostResult {
        limits: MqMqiLimits::default(),
        result,
    })))
    .unwrap();
    // A shape-only receipt fixture, not a dispatched GET or core completion.
    let row = rows
        .iter_mut()
        .find(|row| row.namespace == receipt::NAMESPACE)
        .unwrap();
    let mut value: Value = serde_json::from_slice(&row.payload).unwrap();
    value["value"]["call"] = json!("MQGET");
    value["value"]["reply"]["bytes"] = json!(bytes);
    value["value"]["result_digest"] = json!(digest);
    row.payload = serde_json::to_vec(&value).unwrap();
    assert_eq!(decode(rows.clone()), decode(original_rows.clone()));
    let row = rows
        .iter_mut()
        .find(|row| row.namespace == receipt::NAMESPACE)
        .unwrap();
    value["value"]["reply"]["bytes"] = json!(b"{\"schema_version\":\"unknown\"}".to_vec());
    row.payload = serde_json::to_vec(&value).unwrap();
    assert!(describe(rows, MqLimits::default()).is_err());
    assert_eq!(f.rows(), original_rows);
}
