use super::*;

struct Database(std::path::PathBuf);
impl Database {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "mq-rfh2-quota-{}-{}.sqlite",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        assert!(!path.exists());
        Self(path)
    }
    fn url(&self) -> String {
        format!("sqlite://{}?mode=rwc", self.0.display())
    }
}
impl Drop for Database {
    fn drop(&mut self) {
        for suffix in ["", "-wal", "-shm"] {
            let path = std::path::PathBuf::from(format!("{}{suffix}", self.0.display()));
            if path.exists() {
                std::fs::remove_file(path).unwrap();
            }
        }
    }
}

#[test]
fn rfh2_actual_memory_owned_sqlite_receipt_row_and_payload_quotas_abort_prepared_delete() {
    for sqlite in [false, true] {
        for payload in [false, true] {
            let db = Database::new();
            let store: Arc<dyn PlatformStore> = if sqlite {
                Arc::new(
                    SqliteStateStore::open(&db.url(), if payload { 16384 } else { 64 << 20 }, 256)
                        .unwrap(),
                )
            } else {
                Arc::new(MemoryStore::new(mainframe_env_store::StoreLimits {
                    max_audits: 256,
                    max_provider_state: 256,
                    max_blob_bytes: if payload { 16384 } else { 64 << 20 },
                    ..Default::default()
                }))
            };
            let mut f = Fixture::from_store(store);
            configure(&mut f);
            let c = f.connect();
            let h = hmsg(f.call(2, create(c)));
            f.call(
                3,
                set(
                    c,
                    h,
                    "invoice.id",
                    MqPropertyType::ByteString,
                    vec![255; if payload { 2000 } else { 2 }],
                ),
            );
            let e = f.effect(4, export(c, h, 3, 8192));
            f.seed(&e);
            if !payload {
                let count = f.rows().len()
                    + f.store
                        .list_provider_state_prefix("durable-", 4096)
                        .unwrap()
                        .len();
                for n in count..256 {
                    f.store
                        .put_provider_state(
                            ProviderStateRecord {
                                namespace: "rfh2-quota".into(),
                                key: format!("{n}"),
                                version: 1,
                                payload: vec![1],
                            },
                            None,
                        )
                        .unwrap();
                }
            }
            let state = snapshot(&f);
            let rows = f.rows();
            let audits = f.store.audit_records(&f.inv.execution_id, 0, 128).unwrap();
            assert!(f.execute(&e).is_err(), "sqlite={sqlite},payload={payload}");
            assert_eq!(snapshot(&f), state);
            assert_eq!(f.rows(), rows);
            assert_eq!(
                f.store.audit_records(&f.inv.execution_id, 0, 128).unwrap(),
                audits
            );
            assert!(
                f.store
                    .get_provider_state(receipt::NAMESPACE, "effect-4")
                    .unwrap()
                    .is_none()
            );
        }
    }
}
