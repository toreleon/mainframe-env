use super::*;
use crate::publication::tests::{prepare, put};
use mainframe_env_store_api::{AuditSink, ExecutionStore, IdempotencyStore};

fn request(store: &SqliteStateStore) -> CheckedProviderReadPublication {
    let mut p = prepare(store);
    p.mutations = vec![put("receipt", 1, None, b"exact")];
    CheckedProviderReadPublication {
        execution: store
            .get_execution(&p.intent.execution_id)
            .unwrap()
            .unwrap(),
        publication: p,
        dependencies: vec![TerminalRowDependency::Absent {
            namespace: "publication-fixture-v1".into(),
            key: "receipt".into(),
        }],
    }
}
type RawSnapshot = (Vec<(String, String, i64, Vec<u8>)>, (i64, i64));

fn raw(store: &SqliteStateStore) -> RawSnapshot {
    (store.run(sqlx::query_as("SELECT namespace,key,version,CAST(payload AS BLOB) FROM provider_state ORDER BY namespace,key").fetch_all(&store.pool)).unwrap(),
        store.run(sqlx::query_as("SELECT epoch,clock_tick FROM retention_lock WHERE singleton=1").fetch_one(&store.pool)).unwrap())
}
#[test]
fn checked_read_sqlite_late_insert_audit_clock_quota_and_epoch_faults_roll_back_whole_tx() {
    for fault in 0..6 {
        let mut store = SqliteStateStore::open("sqlite::memory:", 8192, 100).unwrap();
        let r = request(&store);
        match fault {
            0 => {
                store.run(sqlx::query("CREATE TRIGGER refuse_receipt BEFORE INSERT ON provider_state WHEN NEW.key='receipt' BEGIN SELECT RAISE(ABORT,'late receipt'); END").execute(&store.pool)).unwrap();
            }
            1 => {
                store.run(sqlx::query("CREATE TRIGGER refuse_audit BEFORE INSERT ON provider_state WHEN NEW.namespace='durable-audit-v1' BEGIN SELECT RAISE(ABORT,'late audit'); END").execute(&store.pool)).unwrap();
            }
            2 => {
                store.run(sqlx::query("CREATE TRIGGER refuse_clock BEFORE UPDATE OF clock_tick ON retention_lock WHEN NEW.clock_tick=9 BEGIN SELECT RAISE(ABORT,'late clock'); END").execute(&store.pool)).unwrap();
            }
            3 => store.max_rows = 3,
            4 => {
                store
                    .run(
                        sqlx::query("UPDATE retention_lock SET epoch=?")
                            .bind(i64::MAX - 1)
                            .execute(&store.pool),
                    )
                    .unwrap();
            }
            5 => {
                let key = crate::durable::audit_storage_key(
                    &r.execution.execution_id,
                    &format!("direct:{:020}", u64::MAX),
                );
                store.run(sqlx::query("INSERT INTO provider_state(namespace,key,version,payload) VALUES('durable-audit-v1',?,1,?)").bind(key).bind(crate::durable::encode_audit(&r.publication.audit).unwrap()).execute(&store.pool)).unwrap();
            }
            _ => unreachable!(),
        }
        let before = raw(&store);
        assert!(store.publish_provider_read_audited(r).is_err());
        assert_eq!(raw(&store), before, "fault {fault}");
    }
}
#[test]
fn checked_read_sqlite_replay_intent_and_completed_write_no_rows_counters_or_clock() {
    let store = SqliteStateStore::open("sqlite::memory:", 8192, 100).unwrap();
    let r = request(&store);
    store.publish_provider_read_audited(r.clone()).unwrap();
    let mut replay = ProviderReplayAssertion {
        effect: r.publication.intent,
        execution: r.execution,
        observed_tick: 10,
        receipt: mainframe_env_store_api::ProviderStateIdentity {
            namespace: "publication-fixture-v1".into(),
            key: "receipt".into(),
        },
        dependencies: vec![TerminalRowDependency::Exact(
            store
                .get_provider_state("publication-fixture-v1", "receipt")
                .unwrap()
                .unwrap(),
        )],
    };
    let before = raw(&store);
    store.assert_provider_replay(replay.clone()).unwrap();
    assert_eq!(raw(&store), before);
    replay.effect.state = mainframe_env_store_api::EffectState::Completed;
    replay.effect.result_digest = Some([0x31; 32]);
    replay.effect.resolved_tick = Some(9);
    store
        .record_result(&replay.effect.key, replay.effect.clone())
        .unwrap();
    let before = raw(&store);
    store.assert_provider_replay(replay).unwrap();
    assert_eq!(raw(&store), before);
}
#[test]
fn checked_read_sqlite_oversized_core_root_index_and_provider_blobs_refuse_before_decode() {
    for (namespace, key) in [
        ("durable-effect", "effect:publication:3"),
        ("durable-execution", "publication-exec"),
        (ACTOR_NAMESPACE, "publication-exec"),
        (SCOPE_NAMESPACE, "publication-fixture-v1"),
        ("publication-fixture-v1", "receipt"),
    ] {
        let store = SqliteStateStore::open("sqlite::memory:", 8192, 100).unwrap();
        let r = request(&store);
        store.run(sqlx::query("INSERT INTO provider_state(namespace,key,version,payload) VALUES(?,?,1,zeroblob(8193)) ON CONFLICT(namespace,key) DO UPDATE SET payload=excluded.payload")
            .bind(namespace).bind(key).execute(&store.pool)).unwrap();
        let before = raw(&store);
        assert_eq!(
            store.publish_provider_read_audited(r),
            Err(StoreError::PayloadTooLarge)
        );
        assert_eq!(raw(&store), before);
    }
}

#[test]
fn checked_read_sqlite_oversized_audit_ordinal_key_refuses_before_key_allocation() {
    let store = SqliteStateStore::open("sqlite::memory:", 8192, 100).unwrap();
    let r = request(&store);
    let key = crate::durable::audit_storage_key(
        &r.execution.execution_id,
        &format!("direct:{}", "1".repeat(1025)),
    );
    store.run(sqlx::query("INSERT INTO provider_state(namespace,key,version,payload) VALUES('durable-audit-v1',?,1,?)")
        .bind(key).bind(crate::durable::encode_audit(&r.publication.audit).unwrap()).execute(&store.pool)).unwrap();
    let before = raw(&store);
    assert_eq!(
        store.publish_provider_read_audited(r),
        Err(StoreError::PayloadTooLarge)
    );
    assert_eq!(raw(&store), before);
}

#[test]
fn checked_read_sqlite_text_payload_counts_bytes_before_fetch_even_with_nul() {
    for text in ["\u{20ac}".repeat(3000), format!("x\0{}", "a".repeat(8192))] {
        let store = SqliteStateStore::open("sqlite::memory:", 8192, 100).unwrap();
        let r = request(&store);
        store.run(sqlx::query("UPDATE provider_state SET payload=? WHERE namespace='durable-effect' AND key='effect:publication:3'")
            .bind(&text).execute(&store.pool)).unwrap();
        let (kind, chars, bytes): (String, i64, i64) = store.run(sqlx::query_as(
            "SELECT typeof(payload),length(payload),length(CAST(payload AS BLOB)) FROM provider_state WHERE namespace='durable-effect' AND key='effect:publication:3'")
            .fetch_one(&store.pool)).unwrap();
        assert_eq!(kind, "text");
        assert!(chars < 8192);
        assert_eq!(bytes as usize, text.len());
        assert!(bytes > 8192);
        let before = raw(&store);
        assert_eq!(
            store.publish_provider_read_audited(r),
            Err(StoreError::PayloadTooLarge)
        );
        assert_eq!(raw(&store), before);
    }
}

struct BudgetFile(std::path::PathBuf);
impl Drop for BudgetFile {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}
fn budget_observations(count: usize) -> Vec<TerminalRowDependency> {
    (0..count)
        .map(|i| TerminalRowDependency::Absent {
            namespace: "publication-fixture-v1".into(),
            key: if i == 0 {
                "receipt".into()
            } else {
                format!("absent{i}")
            },
        })
        .collect()
}
#[test]
fn checked_read_owned_sqlite_operation_budget_counts_audit_and_preserves_all_rows_clock_epoch() {
    for deny in [false, true] {
        let path =
            std::env::temp_dir().join(format!("inquiry-budget-{}-{deny}", std::process::id()));
        std::fs::create_dir(&path).unwrap();
        let owned = BudgetFile(path);
        let store = SqliteStateStore::open(
            &format!("sqlite://{}/state.db?mode=rwc", owned.0.display()),
            8192,
            100,
        )
        .unwrap();
        let mut r = request(&store);
        r.dependencies = budget_observations(if deny { 4095 } else { 4094 });
        if deny {
            r.publication.mutations.clear();
            r.publication.audit.decision = mainframe_env_execution_api::AuditDecision::Deny;
        }
        let mut excess = r.clone();
        excess.dependencies.push(TerminalRowDependency::Absent {
            namespace: "publication-fixture-v1".into(),
            key: "excess".into(),
        });
        let before = raw(&store);
        assert_eq!(
            store.publish_provider_read_audited(excess),
            Err(StoreError::CapacityExceeded)
        );
        assert_eq!(raw(&store), before);
        store.publish_provider_read_audited(r.clone()).unwrap();
        assert_eq!(
            store
                .audit_records(&r.execution.execution_id, 1, 8)
                .unwrap(),
            vec![r.publication.audit.clone()]
        );
        let receipt = store
            .get_provider_state("publication-fixture-v1", "receipt")
            .unwrap();
        assert_eq!(receipt.is_some(), !deny);
        if deny {
            continue;
        }
        let mut replay = ProviderReplayAssertion {
            effect: r.publication.intent,
            execution: r.execution,
            observed_tick: 9,
            receipt: mainframe_env_store_api::ProviderStateIdentity {
                namespace: "publication-fixture-v1".into(),
                key: "receipt".into(),
            },
            dependencies: budget_observations(4096),
        };
        replay.dependencies[0] = TerminalRowDependency::Exact(receipt.unwrap());
        for completed in [false, true] {
            if completed {
                replay.effect.state = mainframe_env_store_api::EffectState::Completed;
                replay.effect.result_digest = Some([0x21; 32]);
                replay.effect.resolved_tick = Some(9);
                store
                    .record_result(&replay.effect.key, replay.effect.clone())
                    .unwrap();
            }
            let before = raw(&store);
            store.assert_provider_replay(replay.clone()).unwrap();
            assert_eq!(raw(&store), before);
            let mut excess = replay.clone();
            excess.dependencies.push(TerminalRowDependency::Absent {
                namespace: "publication-fixture-v1".into(),
                key: "excess".into(),
            });
            assert_eq!(
                store.assert_provider_replay(excess),
                Err(StoreError::CapacityExceeded)
            );
            assert_eq!(raw(&store), before);
        }
    }
}
