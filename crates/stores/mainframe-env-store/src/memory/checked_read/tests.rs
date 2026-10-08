use super::*;
use crate::publication::tests::{prepare, put};

fn request(store: &MemoryStore) -> CheckedProviderReadPublication {
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
fn counters(store: &MemoryStore) -> (usize, u64, u64, u64, usize, usize) {
    let s = store.lock().unwrap();
    (
        s.blob_bytes,
        s.provider_epoch,
        s.logical_tick,
        s.next_audit_ordinal,
        s.provider_state.len(),
        s.audits.len(),
    )
}
#[test]
fn checked_read_memory_late_faults_restore_all_counters_without_state_clone() {
    for fault in 0..5 {
        let mut store = MemoryStore::new(StoreLimits::default());
        let r = request(&store);
        match fault {
            0 => store.limits.max_audits = 0,
            1 => store.limits.max_provider_state = 0,
            2 => store.limits.max_total_blob_bytes = 0,
            3 => store.lock().unwrap().provider_epoch = u64::MAX - 1,
            4 => store.lock().unwrap().next_audit_ordinal = u64::MAX,
            _ => unreachable!(),
        }
        let before = counters(&store);
        let clones = store.clone_count();
        assert_eq!(
            store.publish_provider_read_audited(r.clone()),
            Err(StoreError::CapacityExceeded)
        );
        assert_eq!(counters(&store), before);
        assert_eq!(store.clone_count(), clones);
        assert_eq!(
            store.effect(&r.publication.intent.key).unwrap(),
            Some(r.publication.intent)
        );
    }
}
#[test]
fn checked_read_memory_replay_has_zero_clock_epoch_and_audit_writes() {
    let store = MemoryStore::new(StoreLimits::default());
    let r = request(&store);
    store.publish_provider_read_audited(r.clone()).unwrap();
    let replay = ProviderReplayAssertion {
        execution: r.execution,
        effect: r.publication.intent,
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
    let before = counters(&store);
    store.assert_provider_replay(replay).unwrap();
    assert_eq!(counters(&store), before);
}
#[test]
fn checked_read_memory_current_lease_floor_and_encoded_core_size_are_physical_fences() {
    for fault in 0..3 {
        let mut store = MemoryStore::new(StoreLimits::default());
        let mut r = request(&store);
        match fault {
            0 => {
                store
                    .lock()
                    .unwrap()
                    .executions
                    .get_mut(&r.execution.execution_id)
                    .unwrap()
                    .lease_expiry_tick = Some(9);
                r.execution.lease_expiry_tick = Some(9);
            }
            1 => store.lock().unwrap().logical_tick = 10,
            2 => store.limits.max_blob_bytes = 300,
            _ => unreachable!(),
        }
        let before = counters(&store);
        assert!(store.publish_provider_read_audited(r).is_err());
        assert_eq!(counters(&store), before);
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
type ReadSnapshot = (
    Vec<ProviderStateRecord>,
    Vec<AuditRecord>,
    Vec<EffectRecord>,
    Vec<ExecutionRecord>,
    (usize, u64, u64, u64, usize, usize),
);

fn budget_snapshot(store: &MemoryStore) -> ReadSnapshot {
    let counters = counters(store);
    let state = store.lock().unwrap();
    (
        state.provider_state.values().cloned().collect(),
        state.audits.values().cloned().collect(),
        state.effects.values().cloned().collect(),
        state.executions.values().cloned().collect(),
        counters,
    )
}
#[test]
fn checked_read_memory_operation_budget_counts_audit_and_preserves_full_state() {
    for deny in [false, true] {
        let store = MemoryStore::new(StoreLimits::default());
        let mut r = request(&store);
        r.dependencies = budget_observations(if deny { 4095 } else { 4094 });
        if deny {
            r.publication.mutations.clear();
            r.publication.audit.decision = mainframe_env_execution_api::AuditDecision::Deny;
        }
        let mut too_many = r.clone();
        too_many.dependencies.push(TerminalRowDependency::Absent {
            namespace: "publication-fixture-v1".into(),
            key: "excess".into(),
        });
        let before = budget_snapshot(&store);
        assert_eq!(
            store.publish_provider_read_audited(too_many),
            Err(StoreError::CapacityExceeded)
        );
        assert_eq!(budget_snapshot(&store), before);
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
            let before = budget_snapshot(&store);
            store.assert_provider_replay(replay.clone()).unwrap();
            assert_eq!(budget_snapshot(&store), before);
            let mut excess = replay.clone();
            excess.dependencies.push(TerminalRowDependency::Absent {
                namespace: "publication-fixture-v1".into(),
                key: "excess".into(),
            });
            assert_eq!(
                store.assert_provider_replay(excess),
                Err(StoreError::CapacityExceeded)
            );
            assert_eq!(budget_snapshot(&store), before);
        }
    }
}
