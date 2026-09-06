use mainframe_env_execution_api::{ExecutionId, IdempotencyKey, InvocationLimits, RunUnitId};
use mainframe_env_store::{MemoryStore, PostgresStateStore, SqliteStateStore};
use mainframe_env_store_api::{
    EffectDigestFormat, EffectRecord, EffectState, IdempotencyStore, ProviderStateRecord,
    ProviderStateStore, StoreError,
};

fn record(key: &str) -> EffectRecord {
    let l = InvocationLimits::default();
    EffectRecord {
        execution_id: ExecutionId::new("format-exec", l).unwrap(),
        run_unit_id: RunUnitId::new("format-run", l).unwrap(),
        sequence: 1,
        key: IdempotencyKey::new(key, l).unwrap(),
        digest_format: EffectDigestFormat::CanonicalHostV1,
        request_digest: [1; 32],
        result_digest: None,
        state: EffectState::Intent,
    }
}
fn contract(store: &dyn IdempotencyStore, key: &str) {
    let intent = record(key);
    store.record_intent(intent.clone()).unwrap();
    store.record_intent(intent.clone()).unwrap();
    let mixed = EffectRecord {
        digest_format: EffectDigestFormat::LegacyDebug,
        ..intent.clone()
    };
    assert_eq!(
        store.record_intent(mixed.clone()),
        Err(StoreError::Conflict)
    );
    let mixed = EffectRecord {
        state: EffectState::UnknownOutcome,
        result_digest: Some([2; 32]),
        ..mixed
    };
    assert_eq!(
        store.record_result(&intent.key, mixed),
        Err(StoreError::Conflict)
    );
    assert_eq!(store.effect(&intent.key).unwrap(), Some(intent.clone()));
    let unknown = EffectRecord {
        state: EffectState::UnknownOutcome,
        result_digest: Some([2; 32]),
        ..intent.clone()
    };
    store.record_result(&intent.key, unknown.clone()).unwrap();
    assert_eq!(
        store.reconcile_unknown_versioned(
            &intent.key,
            EffectState::Completed,
            EffectDigestFormat::LegacyDebug,
            [3; 32]
        ),
        Err(StoreError::Conflict)
    );
    assert_eq!(store.effect(&intent.key).unwrap(), Some(unknown));
    let final_record = store
        .reconcile_unknown_versioned(
            &intent.key,
            EffectState::Completed,
            EffectDigestFormat::CanonicalHostV1,
            [3; 32],
        )
        .unwrap();
    assert_eq!(final_record.digest_format, intent.digest_format);
    assert_eq!(final_record.request_digest, intent.request_digest);
    assert_eq!(final_record.result_digest, Some([3; 32]));
}
#[test]
fn memory_effect_domains_cannot_be_mixed() {
    contract(
        &MemoryStore::new(Default::default()),
        "memory-format-contract",
    );
}
#[test]
fn sqlite_domains_survive_close_reopen_and_preserve_legacy_receipts() {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("effect-format-{}-{nonce}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    let url = format!("sqlite://{}?mode=rwc", root.join("state.db").display());
    let expected;
    {
        let store = SqliteStateStore::open(&url, 1024 * 1024, 65536).unwrap();
        contract(&store, "sqlite-format-contract");
        expected = store.effect(&record("sqlite-format-contract").key).unwrap();
        let legacy = serde_json::json!({"schema":1,"execution":"legacy-exec","run":"legacy-run","sequence":1,"request":"11".repeat(32),"result":"22".repeat(32),"state":"unknown-outcome"});
        store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: "durable-effect".into(),
                    key: "legacy-format-contract".into(),
                    version: 1,
                    payload: serde_json::to_vec(&legacy).unwrap(),
                },
                None,
            )
            .unwrap();
        // A legacy unknown result has completed the intent(1) -> result(2) CAS.
        store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: "durable-effect".into(),
                    key: "legacy-format-contract".into(),
                    version: 2,
                    payload: serde_json::to_vec(&legacy).unwrap(),
                },
                Some(1),
            )
            .unwrap();
    }
    {
        let store = SqliteStateStore::open(&url, 1024 * 1024, 65536).unwrap();
        assert_eq!(
            store.effect(&record("sqlite-format-contract").key).unwrap(),
            expected
        );
        let key = record("legacy-format-contract").key;
        let old = store.effect(&key).unwrap().unwrap();
        assert_eq!(old.digest_format, EffectDigestFormat::LegacyDebug);
        assert_eq!(old.request_digest, [0x11; 32]);
        assert_eq!(old.result_digest, Some([0x22; 32]));
        let resolved = store
            .reconcile_unknown_versioned(
                &key,
                EffectState::Completed,
                EffectDigestFormat::LegacyDebug,
                [0x33; 32],
            )
            .unwrap();
        assert_eq!(resolved.digest_format, EffectDigestFormat::LegacyDebug);
        assert_eq!(resolved.key, key);
    }
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
#[ignore = "requires isolated MAINFRAME_ENV_POSTGRES_TEST_URL"]
fn postgres_effect_domains_cannot_be_mixed() {
    let url = std::env::var("MAINFRAME_ENV_POSTGRES_TEST_URL")
        .expect("explicit PostgreSQL test URL required");
    let store = PostgresStateStore::open(&url, 1024 * 1024, 65536).unwrap();
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    contract(&store, &format!("pg-format-{}-{nonce}", std::process::id()));
}
