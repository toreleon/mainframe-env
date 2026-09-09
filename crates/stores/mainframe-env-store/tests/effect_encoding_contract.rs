use mainframe_env_execution_api::{ExecutionId, IdempotencyKey, InvocationLimits, RunUnitId};
use mainframe_env_store::{MemoryStore, PostgresStateStore, SqliteStateStore};
use mainframe_env_store_api::{
    EffectDigestFormat, EffectIntentMetadata, EffectRecord, EffectState, IdempotencyStore,
    ProviderStateRecord, ProviderStateStore, StoreError,
};
use std::sync::Arc;

fn record(key: &str) -> EffectRecord {
    let l = InvocationLimits::default();
    EffectRecord {
        execution_id: ExecutionId::new("format-exec", l).unwrap(),
        run_unit_id: RunUnitId::new("format-run", l).unwrap(),
        sequence: 1,
        key: IdempotencyKey::new(key, l).unwrap(),
        digest_format: EffectDigestFormat::CanonicalHostV1,
        request_digest: [1; 32],
        intent: EffectIntentMetadata {
            owner: ExecutionId::new("format-exec", l).unwrap(),
            attempt: 1,
            capability: Some(
                mainframe_env_execution_api::CapabilityId::new("host.state.write", l).unwrap(),
            ),
            audit_resource: None,
            audit_invocation_key: None,
            created_tick: 5,
            recovery_after_tick: 10,
            epoch: 1,
            recovery_lease: None,
        },
        result_digest: None,
        resolved_tick: None,
        state: EffectState::Intent,
    }
}
fn contract(store: &dyn IdempotencyStore, key: &str) {
    let intent = record(key);
    store.record_intent(intent.clone()).unwrap();
    assert_eq!(
        store.stale_intents(10, 0, 8),
        Err(StoreError::InvalidTransition)
    );
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
    let prematurely_aged = EffectRecord {
        state: EffectState::Completed,
        result_digest: Some([2; 32]),
        resolved_tick: Some(intent.intent.created_tick - 1),
        ..intent.clone()
    };
    assert_eq!(
        store.record_result(&intent.key, prematurely_aged),
        Err(StoreError::InvalidTransition)
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

fn stale_recovery_contract(store: &dyn IdempotencyStore, key: &str) {
    let intent = record(key);
    store.record_intent(intent.clone()).unwrap();
    assert!(store.stale_intents(9, 5, 8).unwrap().is_empty());
    assert!(
        store.stale_intents(9, 1, 8).unwrap().is_empty(),
        "the durable recovery-not-before boundary outranks a shorter worker policy"
    );
    assert_eq!(store.stale_intents(10, 5, 8).unwrap(), vec![intent.clone()]);

    let first = store
        .claim_stale_intent(&intent.key, intent.intent.epoch, "worker-a", 10, 5, 5)
        .unwrap();
    let first_lease = first.intent.recovery_lease.as_ref().unwrap();
    assert_eq!((first_lease.attempt, first_lease.epoch), (1, 1));
    assert_eq!(first_lease.expires_tick, 15);
    assert!(store.stale_intents(14, 5, 8).unwrap().is_empty());
    assert_eq!(
        store.claim_stale_intent(&intent.key, intent.intent.epoch, "worker-b", 14, 5, 5),
        Err(StoreError::LeaseConflict)
    );

    let late_result = EffectRecord {
        state: EffectState::Completed,
        result_digest: Some([4; 32]),
        ..intent.clone()
    };
    assert_eq!(
        store.record_result(&intent.key, late_result),
        Err(StoreError::Conflict),
        "the stale owner is fenced after recovery claims the intent"
    );
    let forged_claim_result = EffectRecord {
        state: EffectState::Completed,
        result_digest: Some([4; 32]),
        ..first.clone()
    };
    assert_eq!(
        store.record_result(&intent.key, forged_claim_result),
        Err(StoreError::Conflict),
        "claimed intents can be finalized only through the fenced recovery API"
    );

    let second = store
        .claim_stale_intent(&intent.key, intent.intent.epoch, "worker-b", 15, 5, 5)
        .unwrap();
    let second_lease = second.intent.recovery_lease.as_ref().unwrap();
    assert_eq!((second_lease.attempt, second_lease.epoch), (2, 2));
    assert_eq!(
        store.reconcile_stale_intent(
            &intent.key,
            "worker-a",
            first_lease.epoch,
            16,
            EffectState::Completed,
            intent.digest_format,
            [5; 32],
        ),
        Err(StoreError::LeaseConflict)
    );
    assert_eq!(
        store.reconcile_stale_intent(
            &intent.key,
            "worker-b",
            second_lease.epoch,
            16,
            EffectState::Completed,
            EffectDigestFormat::LegacyDebug,
            [5; 32],
        ),
        Err(StoreError::InvalidTransition)
    );
    let completed = store
        .reconcile_stale_intent(
            &intent.key,
            "worker-b",
            second_lease.epoch,
            16,
            EffectState::Completed,
            intent.digest_format,
            [5; 32],
        )
        .unwrap();
    assert_eq!(completed.state, EffectState::Completed);
    assert_eq!(completed.result_digest, Some([5; 32]));
    assert!(store.stale_intents(100, 5, 8).unwrap().is_empty());
}

fn concurrent_claim_contract(
    first: Arc<dyn IdempotencyStore>,
    second: Arc<dyn IdempotencyStore>,
    key: &str,
) {
    let intent = record(key);
    first.record_intent(intent.clone()).unwrap();
    let mut workers = Vec::new();
    for (owner, store) in [("worker-one", first.clone()), ("worker-two", second)] {
        let key = intent.key.clone();
        workers.push(std::thread::spawn(move || {
            store.claim_stale_intent(&key, 1, owner, 10, 5, 5)
        }));
    }
    let mut successes = Vec::new();
    let mut conflicts = 0;
    for result in workers.into_iter().map(|worker| worker.join().unwrap()) {
        match result {
            Ok(record) => successes.push(record),
            Err(StoreError::Conflict | StoreError::LeaseConflict) => conflicts += 1,
            other => panic!("unexpected concurrent claim result: {other:?}"),
        }
    }
    assert_eq!(successes.len(), 1);
    assert_eq!(conflicts, 1);
    let claimed = successes.pop().unwrap();
    let lease = claimed.intent.recovery_lease.as_ref().unwrap();
    first
        .reconcile_stale_intent(
            &claimed.key,
            &lease.owner,
            lease.epoch,
            11,
            EffectState::Failed,
            claimed.digest_format,
            [6; 32],
        )
        .unwrap();
}

#[test]
fn memory_effect_domains_cannot_be_mixed() {
    let store = Arc::new(MemoryStore::new(Default::default()));
    contract(&*store, "memory-format-contract");
    stale_recovery_contract(&*store, "memory-stale-contract");
    concurrent_claim_contract(store.clone(), store, "memory-concurrent-stale-contract");
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
        let store = Arc::new(SqliteStateStore::open(&url, 1024 * 1024, 65536).unwrap());
        contract(&*store, "sqlite-format-contract");
        stale_recovery_contract(&*store, "sqlite-stale-contract");
        concurrent_claim_contract(
            store.clone(),
            Arc::new(SqliteStateStore::open(&url, 1024 * 1024, 65536).unwrap()),
            "sqlite-concurrent-stale-contract",
        );
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
        let prior_canonical_intent = serde_json::json!({
            "schema": 2,
            "digest_format": "mainframe-env.effect-canonical@1",
            "execution": "prior-canonical-exec",
            "run": "prior-canonical-run",
            "sequence": 7,
            "request_canonical_v1": "44".repeat(32),
            "result_canonical_v1": null,
            "state": "intent"
        });
        store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: "durable-effect".into(),
                    key: "prior-canonical-intent".into(),
                    version: 1,
                    payload: serde_json::to_vec(&prior_canonical_intent).unwrap(),
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

        let prior_key = record("prior-canonical-intent").key;
        let prior = store.effect(&prior_key).unwrap().unwrap();
        assert_eq!(prior.state, EffectState::Intent);
        assert_eq!(prior.intent.owner.as_str(), "prior-canonical-exec");
        assert_eq!(prior.intent.attempt, 1);
        assert_eq!(prior.intent.capability, None);
        assert_eq!(prior.intent.created_tick, 0);
        assert_eq!(prior.intent.recovery_after_tick, 0);
        assert_eq!(prior.intent.epoch, 7);
        assert!(
            store
                .stale_intents(1, 1, 64)
                .unwrap()
                .iter()
                .any(|intent| intent.key == prior_key),
            "a schema-2 orphan remains discoverable after upgrade"
        );
        let claimed = store
            .claim_stale_intent(&prior_key, 7, "migration-worker", 1, 1, 5)
            .unwrap();
        store
            .reconcile_stale_intent(
                &prior_key,
                "migration-worker",
                claimed.intent.recovery_lease.unwrap().epoch,
                2,
                EffectState::Failed,
                EffectDigestFormat::CanonicalHostV1,
                [0x55; 32],
            )
            .unwrap();
    }
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
#[ignore = "requires isolated MAINFRAME_ENV_POSTGRES_TEST_URL"]
fn postgres_effect_domains_cannot_be_mixed() {
    let url = std::env::var("MAINFRAME_ENV_POSTGRES_TEST_URL")
        .expect("explicit PostgreSQL test URL required");
    let store = Arc::new(PostgresStateStore::open(&url, 1024 * 1024, 65536).unwrap());
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let prefix = format!("pg-{}-{nonce}", std::process::id());
    contract(&*store, &format!("{prefix}-format"));
    stale_recovery_contract(&*store, &format!("{prefix}-stale"));
    concurrent_claim_contract(
        store,
        Arc::new(PostgresStateStore::open(&url, 1024 * 1024, 65536).unwrap()),
        &format!("{prefix}-concurrent"),
    );
}
