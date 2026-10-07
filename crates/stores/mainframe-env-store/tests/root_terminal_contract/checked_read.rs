//! Actual Open-root structural store observations, not host or MQ permissions.
use super::*;
fn checked(store: &dyn PlatformStore) -> (RootProviderPublication, CheckedProviderReadPublication) {
    let (owner, effect) = original(store);
    store
        .mutate_provider_states_atomic(vec![put("native-root-a", "queue", 1, None)])
        .unwrap();
    let publication = audit(
        &owner,
        &effect,
        vec![put("native-root-a", "receipt", 1, None)],
    );
    let checked = CheckedProviderReadPublication {
        publication,
        execution: owner.occurrence.execution.clone(),
        dependencies: vec![
            TerminalRowDependency::Exact(
                store
                    .get_provider_state("native-root-a", "queue")
                    .unwrap()
                    .unwrap(),
            ),
            TerminalRowDependency::Absent {
                namespace: "native-root-a".into(),
                key: "receipt".into(),
            },
        ],
    };
    (owner, checked)
}
fn success(store: &dyn PlatformStore) {
    let (_, r) = checked(store);
    store.publish_provider_read_audited(r.clone()).unwrap();
    let row = match &r.dependencies[0] {
        TerminalRowDependency::Exact(r) => r,
        _ => unreachable!(),
    };
    assert_eq!(
        store.get_provider_state(&row.namespace, &row.key).unwrap(),
        Some(row.clone())
    );
    let receipt = store
        .get_provider_state("native-root-a", "receipt")
        .unwrap()
        .unwrap();
    let mut replay = ProviderReplayAssertion {
        execution: r.execution.clone(),
        effect: r.publication.intent.clone(),
        observed_tick: 10,
        receipt: ProviderStateIdentity {
            namespace: "native-root-a".into(),
            key: "receipt".into(),
        },
        dependencies: vec![
            r.dependencies[0].clone(),
            TerminalRowDependency::Exact(receipt),
        ],
    };
    let epoch = store.provider_state_retention_epoch().unwrap();
    store.assert_provider_replay(replay.clone()).unwrap();
    assert_eq!(store.provider_state_retention_epoch().unwrap(), epoch);
    replay.effect.state = EffectState::Completed;
    replay.effect.result_digest = Some([7; 32]);
    replay.effect.resolved_tick = Some(10);
    store
        .record_result(&replay.effect.key, replay.effect.clone())
        .unwrap();
    let epoch = store.provider_state_retention_epoch().unwrap();
    store.assert_provider_replay(replay).unwrap();
    assert_eq!(store.provider_state_retention_epoch().unwrap(), epoch);
}
#[test]
fn memory_checked_read_root_success_and_completed_replay() {
    success(&MemoryStore::new(StoreLimits::default()));
}
#[test]
fn sqlite_checked_read_root_success_and_completed_replay() {
    let f = OwnedSqlite::new();
    success(f.store());
}
fn refuse(store: &dyn PlatformStore) {
    let (owner, r) = checked(store);
    for namespace in ["unregistered", "exact-shared"] {
        let mut bad = r.clone();
        bad.dependencies.push(TerminalRowDependency::Absent {
            namespace: namespace.into(),
            key: "foreign".into(),
        });
        let before = observed(store, &owner);
        assert!(store.publish_provider_read_audited(bad).is_err());
        assert_eq!(observed(store, &owner), before);
    }
    // A second real root's Open namespace is still foreign.
    start(store, "other-root", vec!["other-scope".into()], vec![]);
    let mut bad = r.clone();
    bad.dependencies.push(TerminalRowDependency::Absent {
        namespace: "other-scope".into(),
        key: "foreign".into(),
    });
    assert!(store.publish_provider_read_audited(bad).is_err());
    let mut late = r.clone();
    late.publication.observed_tick = 100;
    late.publication.audit.observed_tick = 100;
    assert!(store.publish_provider_read_audited(late).is_err());
    // Real Closing requires known original effects, not an unresolved Intent.
    let mut completed = r.publication.intent.clone();
    completed.state = EffectState::Completed;
    completed.result_digest = Some([9; 32]);
    completed.resolved_tick = Some(10);
    store
        .record_result(&completed.key, completed.clone())
        .unwrap();
    let replay = ProviderReplayAssertion {
        effect: completed,
        execution: r.execution.clone(),
        observed_tick: 10,
        receipt: ProviderStateIdentity {
            namespace: "native-root-a".into(),
            key: "queue".into(),
        },
        dependencies: vec![r.dependencies[0].clone()],
    };
    let before = store.provider_state_retention_epoch().unwrap();
    store
        .close_root_driver(&owner.occurrence.claim, &r.execution, 10)
        .unwrap();
    let closed = store.provider_state_retention_epoch().unwrap();
    assert!(closed > before);
    assert!(store.publish_provider_read_audited(r).is_err());
    assert!(store.assert_provider_replay(replay).is_err());
    assert_eq!(store.provider_state_retention_epoch().unwrap(), closed);
}
#[test]
fn memory_checked_read_foreign_unregistered_deadline_and_closing_refuse() {
    refuse(&MemoryStore::new(StoreLimits::default()));
}
#[test]
fn sqlite_checked_read_foreign_unregistered_deadline_and_closing_refuse() {
    let f = OwnedSqlite::new();
    refuse(f.store());
}
fn unindexed(store: &dyn PlatformStore) {
    let (_, r) = checked(store);
    let mut legacy = crate_legacy(store);
    legacy.dependencies.push(TerminalRowDependency::Absent {
        namespace: "native-root-a".into(),
        key: "other".into(),
    });
    let epoch = store.provider_state_retention_epoch().unwrap();
    assert!(store.publish_provider_read_audited(legacy).is_err());
    assert_eq!(store.provider_state_retention_epoch().unwrap(), epoch);
    assert!(
        store
            .get_provider_state("native-root-a", "receipt")
            .unwrap()
            .is_none()
    );
    assert_eq!(
        store.effect(&r.publication.intent.key).unwrap(),
        Some(r.publication.intent)
    );
}
fn crate_legacy(store: &dyn PlatformStore) -> CheckedProviderReadPublication {
    let mut execution = admission().execution;
    let l = InvocationLimits::default();
    execution.execution_id = ExecutionId::new("legacy-read", l).unwrap();
    execution.run_unit_id = RunUnitId::new("legacy-run", l).unwrap();
    store.create_execution(execution.clone()).unwrap();
    store
        .transition_execution(&execution.execution_id, 1, ExecutionState::Queued, 8)
        .unwrap();
    execution = store
        .transition_execution(&execution.execution_id, 2, ExecutionState::Running, 9)
        .unwrap();
    let effect = intent(store, &execution);
    let audit = AuditRecord {
        execution_id: execution.execution_id.clone(),
        run_unit_id: execution.run_unit_id.clone(),
        principal: execution.principal.clone(),
        attempt: effect.intent.attempt,
        effect_sequence: effect.sequence,
        observed_tick: 10,
        invocation_key: effect.intent.audit_invocation_key.clone().unwrap(),
        capability: effect.intent.capability.clone().unwrap(),
        resource: effect.intent.audit_resource.unwrap(),
        decision: AuditDecision::Success,
    };
    CheckedProviderReadPublication {
        publication: AuditedProviderPublication {
            intent: effect,
            audit,
            observed_tick: 10,
            mutations: vec![put("legacy-scope", "receipt", 1, None)],
        },
        execution,
        dependencies: vec![TerminalRowDependency::Absent {
            namespace: "legacy-scope".into(),
            key: "receipt".into(),
        }],
    }
}
#[test]
fn memory_checked_read_unindexed_actor_cannot_borrow_open_root() {
    unindexed(&MemoryStore::new(StoreLimits::default()));
}
#[test]
fn sqlite_checked_read_unindexed_actor_cannot_borrow_open_root() {
    let f = OwnedSqlite::new();
    unindexed(f.store());
}

fn uncertain(store: &dyn PlatformStore) {
    let (owner, r) = checked(store);
    store.publish_provider_read_audited(r.clone()).unwrap();
    let replay = ProviderReplayAssertion {
        execution: r.execution.clone(),
        effect: r.publication.intent.clone(),
        observed_tick: 10,
        receipt: ProviderStateIdentity {
            namespace: "native-root-a".into(),
            key: "receipt".into(),
        },
        dependencies: vec![
            r.dependencies[0].clone(),
            TerminalRowDependency::Exact(
                store
                    .get_provider_state("native-root-a", "receipt")
                    .unwrap()
                    .unwrap(),
            ),
        ],
    };
    store
        .fence_root_driver(&owner.occurrence.claim, &r.execution, 10)
        .unwrap();
    let before = observed(store, &owner);
    assert!(store.assert_provider_replay(replay).is_err());
    assert_eq!(observed(store, &owner), before);
}
#[test]
fn memory_checked_read_uncertain_root_refuses_replay_without_audit() {
    uncertain(&MemoryStore::new(StoreLimits::default()));
}
#[test]
fn sqlite_checked_read_uncertain_root_refuses_replay_without_audit() {
    let f = OwnedSqlite::new();
    uncertain(f.store());
}

#[test]
fn sqlite_checked_read_missing_foreign_malformed_and_overlapping_current_indexes_refuse() {
    use sqlx::Connection;
    for fault in 0..7 {
        let fixture = OwnedSqlite::new();
        let store = fixture.store();
        let (owner, r) = checked(store);
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let mut connection = sqlx::SqliteConnection::connect(&format!("sqlite://{}?mode=rw", fixture.path.display())).await.unwrap();
            match fault {
                0..=2 => {
                    let (n, k) = match fault { 0 => ("durable-root-actor-v1", "writer-a"), 1 => ("durable-root-run-v1", "writer-a-run"), _ => ("durable-root-scope-v1", "native-root-a") };
                    sqlx::query("DELETE FROM provider_state WHERE namespace=? AND key=?").bind(n).bind(k).execute(&mut connection).await.unwrap();
                },
                3 => { sqlx::query("UPDATE provider_state SET payload='foreign' WHERE namespace='durable-root-scope-v1' AND key='native-root-a'").execute(&mut connection).await.unwrap(); },
                4 => { sqlx::query("UPDATE provider_state SET version=2 WHERE namespace='durable-root-scope-v1' AND key='native-root-a'").execute(&mut connection).await.unwrap(); },
                5 => {
                    // Independent fixture for the frozen core row-scope framing.
                    use sha2::{Digest, Sha256};
                    let mut h = Sha256::new();
                    h.update(b"mainframe-env.core-root-row-scope@1\0");
                    for field in ["native-root-a", "queue"] { h.update((field.len() as u64).to_be_bytes()); h.update(field.as_bytes()); }
                    let key = format!("{:x}", h.finalize());
                    sqlx::query("INSERT INTO provider_state(namespace,key,version,payload) VALUES('durable-root-row-scope-v1',?,1,?)").bind(key).bind(b"writer-a".as_slice()).execute(&mut connection).await.unwrap();
                },
                6 => { sqlx::query("UPDATE provider_state SET payload=zeroblob(1048577) WHERE namespace=?").bind(ROOT_DRIVER_NAMESPACE).execute(&mut connection).await.unwrap(); },
                _ => unreachable!(),
            }
            connection.close().await.unwrap();
        });
        let before = (
            store.provider_state_retention_epoch().unwrap(),
            store
                .get_provider_state("native-root-a", "receipt")
                .unwrap(),
            store
                .audit_records(&owner.occurrence.execution.execution_id, 1, 64)
                .unwrap(),
        );
        assert!(
            store.publish_provider_read_audited(r).is_err(),
            "index fault {fault}"
        );
        assert_eq!(
            (
                store.provider_state_retention_epoch().unwrap(),
                store
                    .get_provider_state("native-root-a", "receipt")
                    .unwrap(),
                store
                    .audit_records(&owner.occurrence.execution.execution_id, 1, 64)
                    .unwrap()
            ),
            before
        );
    }
}
