use super::*;
use crate::publication::tests::{prepare, put};
use crate::{MemoryStore, SqliteStateStore, StoreLimits};
use mainframe_env_execution_api::AuditDecision;
use std::sync::{
    Arc, Barrier,
    atomic::{AtomicU64, Ordering},
};

struct Owned(std::path::PathBuf);
impl Owned {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "checked-read-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn url(&self) -> String {
        format!("sqlite://{}/state.db?mode=rwc", self.0.display())
    }
}
impl Drop for Owned {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}
fn backends() -> (Owned, Vec<Arc<dyn PlatformStore>>) {
    let owned = Owned::new();
    let stores: Vec<Arc<dyn PlatformStore>> = vec![
        Arc::new(MemoryStore::new(StoreLimits {
            max_blob_bytes: 8192,
            ..StoreLimits::default()
        })),
        Arc::new(SqliteStateStore::open(&owned.url(), 8192, 10000).unwrap()),
    ];
    (owned, stores)
}
fn request(store: &dyn PlatformStore) -> CheckedProviderReadPublication {
    let mut p = prepare(store);
    p.mutations = vec![put("receipt", 1, None, b"original full result")];
    store
        .mutate_provider_states_atomic(vec![
            put("queue", 1, None, b"queue"),
            put("control", 1, None, b"control"),
        ])
        .unwrap();
    CheckedProviderReadPublication {
        execution: store
            .get_execution(&p.intent.execution_id)
            .unwrap()
            .unwrap(),
        publication: p,
        dependencies: vec![
            TerminalRowDependency::Exact(
                store
                    .get_provider_state("publication-fixture-v1", "queue")
                    .unwrap()
                    .unwrap(),
            ),
            TerminalRowDependency::Exact(
                store
                    .get_provider_state("publication-fixture-v1", "control")
                    .unwrap()
                    .unwrap(),
            ),
            TerminalRowDependency::Absent {
                namespace: "publication-fixture-v1".into(),
                key: "receipt".into(),
            },
        ],
    }
}
fn replay(
    store: &dyn PlatformStore,
    r: &CheckedProviderReadPublication,
) -> ProviderReplayAssertion {
    let mut dependencies = r.dependencies[..2].to_vec();
    dependencies.push(TerminalRowDependency::Exact(
        store
            .get_provider_state("publication-fixture-v1", "receipt")
            .unwrap()
            .unwrap(),
    ));
    ProviderReplayAssertion {
        effect: r.publication.intent.clone(),
        execution: r.execution.clone(),
        observed_tick: 9,
        receipt: ProviderStateIdentity {
            namespace: "publication-fixture-v1".into(),
            key: "receipt".into(),
        },
        dependencies,
    }
}
fn snapshot(
    store: &dyn PlatformStore,
    r: &CheckedProviderReadPublication,
) -> (
    Vec<ProviderStateRecord>,
    u64,
    Vec<mainframe_env_execution_api::AuditRecord>,
    EffectRecord,
    ExecutionRecord,
) {
    (
        store
            .list_provider_state("publication-fixture-v1", 10000)
            .unwrap(),
        store.provider_state_retention_epoch().unwrap(),
        store
            .audit_records(&r.execution.execution_id, 1, 100)
            .unwrap(),
        store.effect(&r.publication.intent.key).unwrap().unwrap(),
        store
            .get_execution(&r.execution.execution_id)
            .unwrap()
            .unwrap(),
    )
}

#[test]
fn checked_read_publication_and_intent_completed_replay_leave_observed_rows_exact() {
    let (_owned, stores) = backends();
    for store in stores {
        let r = request(store.as_ref());
        store.publish_provider_read_audited(r.clone()).unwrap();
        for d in &r.dependencies[..2] {
            if let TerminalRowDependency::Exact(row) = d {
                assert_eq!(
                    store.get_provider_state(&row.namespace, &row.key).unwrap(),
                    Some(row.clone())
                );
            }
        }
        assert_eq!(
            store
                .audit_records(&r.execution.execution_id, 1, 100)
                .unwrap(),
            vec![r.publication.audit.clone()]
        );
        let mut replay = replay(store.as_ref(), &r);
        let before = snapshot(store.as_ref(), &r);
        store.assert_provider_replay(replay.clone()).unwrap();
        assert_eq!(snapshot(store.as_ref(), &r), before);
        replay.effect.state = EffectState::Completed;
        replay.effect.result_digest = Some([0x81; 32]);
        replay.effect.resolved_tick = Some(9);
        store
            .record_result(&replay.effect.key, replay.effect.clone())
            .unwrap();
        let before = snapshot(store.as_ref(), &r);
        store.assert_provider_replay(replay.clone()).unwrap();
        assert_eq!(snapshot(store.as_ref(), &r), before);
        let mut completed = r.clone();
        completed.publication.intent = replay.effect;
        assert!(store.publish_provider_read_audited(completed).is_err());
        assert_eq!(snapshot(store.as_ref(), &r), before);
    }
}
#[test]
fn checked_read_forged_original_execution_and_stale_dependencies_fail_without_writes() {
    let (_owned, stores) = backends();
    for store in stores {
        let r = request(store.as_ref());
        let before = snapshot(store.as_ref(), &r);
        for fault in 0..18 {
            let mut changed = r.clone();
            match fault {
                0 => changed.execution.version += 1,
                1 => changed.execution.owner_lease = Some("substituted".into()),
                2 => changed.execution.lease_expiry_tick = Some(50),
                3 => changed.publication.intent.request_digest[0] ^= 1,
                4 => changed.publication.intent.intent.epoch += 1,
                5 => changed.publication.intent.sequence += 1,
                6 => changed.publication.intent.intent.attempt += 1,
                7 => {
                    changed.publication.intent.intent.recovery_lease = Some(EffectRecoveryLease {
                        owner: "recoverer".into(),
                        attempt: 1,
                        epoch: 1,
                        expires_tick: 50,
                    })
                }
                8 => changed.publication.observed_tick = 30,
                9 => {
                    if let TerminalRowDependency::Exact(row) = &mut changed.dependencies[0] {
                        row.payload[0] ^= 1;
                    }
                }
                10 => {
                    if let TerminalRowDependency::Exact(row) = &mut changed.dependencies[0] {
                        row.version += 1;
                    }
                }
                11 => changed.dependencies.push(changed.dependencies[0].clone()),
                12 => changed.dependencies.push(TerminalRowDependency::Absent {
                    namespace: "publication-fixture-v1".into(),
                    key: "queue".into(),
                }),
                13 => {
                    changed.dependencies[0] = TerminalRowDependency::Absent {
                        namespace: "durable-effect".into(),
                        key: "forged".into(),
                    }
                }
                14 => {
                    changed.dependencies[0] = TerminalRowDependency::Absent {
                        namespace: "jes-worker-meta".into(),
                        key: "logical-clock".into(),
                    }
                }
                15 => changed.publication.mutations = vec![put("queue", 2, Some(1), b"no-op")],
                16 => changed.publication.audit.decision = AuditDecision::Deny,
                17 => changed.dependencies.pop().map(|_| ()).unwrap(),
                _ => unreachable!(),
            }
            assert!(
                store.publish_provider_read_audited(changed).is_err(),
                "fault {fault}"
            );
            assert_eq!(snapshot(store.as_ref(), &r), before, "fault {fault}");
        }
        store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: "publication-fixture-v1".into(),
                    key: "receipt".into(),
                    version: 1,
                    payload: b"raced".to_vec(),
                },
                None,
            )
            .unwrap();
        let before = snapshot(store.as_ref(), &r);
        assert!(store.publish_provider_read_audited(r.clone()).is_err());
        assert_eq!(snapshot(store.as_ref(), &r), before);
    }
}
#[test]
fn checked_read_audit_only_deny_cannot_publish_a_receipt() {
    let (_owned, stores) = backends();
    for store in stores {
        let mut r = request(store.as_ref());
        r.publication.audit.decision = AuditDecision::Deny;
        r.publication.mutations.clear();
        let rows = snapshot(store.as_ref(), &r).0;
        store.publish_provider_read_audited(r.clone()).unwrap();
        assert_eq!(snapshot(store.as_ref(), &r).0, rows);
        assert_eq!(
            store
                .audit_records(&r.execution.execution_id, 1, 100)
                .unwrap(),
            vec![r.publication.audit]
        );
    }
}
#[test]
fn checked_read_insert_race_has_one_receipt_and_one_audit() {
    let (_owned, stores) = backends();
    for store in stores {
        let r = request(store.as_ref());
        let barrier = Arc::new(Barrier::new(2));
        let workers: Vec<_> = (0..2)
            .map(|_| {
                let s = store.clone();
                let r = r.clone();
                let b = barrier.clone();
                std::thread::spawn(move || {
                    b.wait();
                    s.publish_provider_read_audited(r)
                })
            })
            .collect();
        let outcomes: Vec<_> = workers.into_iter().map(|w| w.join().unwrap()).collect();
        assert_eq!(outcomes.iter().filter(|r| r.is_ok()).count(), 1);
        assert_eq!(
            store
                .audit_records(&r.execution.execution_id, 1, 100)
                .unwrap()
                .len(),
            1
        );
    }
}
#[test]
fn checked_read_replay_rejects_receipt_replacement_foreign_metadata_and_uncertainty() {
    let (_owned, stores) = backends();
    for store in stores {
        let r = request(store.as_ref());
        store.publish_provider_read_audited(r.clone()).unwrap();
        let replay = replay(store.as_ref(), &r);
        let before = snapshot(store.as_ref(), &r);
        for fault in 0..9 {
            let mut changed = replay.clone();
            match fault {
                0 => changed.effect.state = EffectState::UnknownOutcome,
                1 => {
                    changed.effect.state = EffectState::Failed;
                    changed.effect.result_digest = Some([1; 32]);
                }
                2 => changed.effect.request_digest[0] ^= 1,
                3 => changed.execution.attempt += 1,
                4 => changed.observed_tick = 30,
                5 => {
                    if let TerminalRowDependency::Exact(row) = &mut changed.dependencies[2] {
                        row.payload[0] ^= 1;
                    }
                }
                6 => changed.receipt.key = "missing".into(),
                7 => changed.dependencies.push(changed.dependencies[2].clone()),
                8 => {
                    changed.effect.state = EffectState::Completed;
                    changed.effect.result_digest = Some([1; 32]);
                }
                _ => unreachable!(),
            }
            assert!(
                store.assert_provider_replay(changed).is_err(),
                "fault {fault}"
            );
            assert_eq!(snapshot(store.as_ref(), &r), before);
        }
    }
}
#[test]
fn checked_read_bounds_count_bytes_and_identity_before_backend_work() {
    let (_owned, stores) = backends();
    for store in stores {
        let r = request(store.as_ref());
        let mut bounded = r.clone();
        bounded.dependencies = (0..4094)
            .map(|i| TerminalRowDependency::Absent {
                namespace: "publication-fixture-v1".into(),
                key: if i == 0 {
                    "receipt".into()
                } else {
                    format!("absent{i}")
                },
            })
            .collect();
        assert!(bounded.validate_bounds(8192).is_ok());
        bounded.dependencies.push(TerminalRowDependency::Absent {
            namespace: "publication-fixture-v1".into(),
            key: "one-too-many".into(),
        });
        assert_eq!(
            bounded.validate_bounds(8192),
            Err(StoreError::CapacityExceeded)
        );
        let mut huge = r.clone();
        huge.dependencies[0] = TerminalRowDependency::Exact(ProviderStateRecord {
            namespace: "publication-fixture-v1".into(),
            key: "huge".into(),
            version: 1,
            payload: vec![0; MAX_ROOT_PAYLOAD_BYTES],
        });
        assert_eq!(
            huge.validate_bounds(MAX_ROOT_PAYLOAD_BYTES),
            Err(StoreError::CapacityExceeded)
        );
        let mut invalid = r.clone();
        invalid.dependencies[0] = TerminalRowDependency::Absent {
            namespace: "".into(),
            key: "x".into(),
        };
        assert!(invalid.validate_bounds(8192).is_err());
        invalid.dependencies[0] = TerminalRowDependency::Absent {
            namespace: "valid".into(),
            key: "x".repeat(1025),
        };
        assert!(invalid.validate_bounds(8192).is_err());
    }
}
#[test]
fn checked_read_orderly_sqlite_reopen_replays_exact_receipt_without_hydrating_runtime() {
    let owned = Owned::new();
    let r = {
        let store = SqliteStateStore::open(&owned.url(), 8192, 100).unwrap();
        let r = request(&store);
        store.publish_provider_read_audited(r.clone()).unwrap();
        r
    };
    let store = SqliteStateStore::open(&owned.url(), 8192, 100).unwrap();
    let replay = replay(&store, &r);
    let before = snapshot(&store, &r);
    store.assert_provider_replay(replay).unwrap();
    assert_eq!(snapshot(&store, &r), before);
}

// An adapter without this atomic contract must not fall back to any of its rows
// or audit methods. Panicking legacy methods independently detect such fallback.
struct Unsupported;
impl AuditSink for Unsupported {
    fn record_audit(&self, _: mainframe_env_execution_api::AuditRecord) -> Result<(), StoreError> {
        panic!("audit fallback")
    }
    fn audit_records(
        &self,
        _: &mainframe_env_execution_api::ExecutionId,
        _: u64,
        _: usize,
    ) -> Result<Vec<mainframe_env_execution_api::AuditRecord>, StoreError> {
        panic!("audit fallback")
    }
}
impl ProviderStateStore for Unsupported {
    fn get_provider_state(
        &self,
        _: &str,
        _: &str,
    ) -> Result<Option<ProviderStateRecord>, StoreError> {
        panic!("read fallback")
    }
    fn list_provider_state(
        &self,
        _: &str,
        _: usize,
    ) -> Result<Vec<ProviderStateRecord>, StoreError> {
        panic!("read fallback")
    }
    fn put_provider_state(&self, _: ProviderStateRecord, _: Option<u64>) -> Result<(), StoreError> {
        panic!("write fallback")
    }
    fn delete_provider_state(&self, _: &str, _: &str, _: u64) -> Result<(), StoreError> {
        panic!("write fallback")
    }
    fn move_provider_state(
        &self,
        _: ProviderStateRecord,
        _: &str,
        _: u64,
    ) -> Result<(), StoreError> {
        panic!("write fallback")
    }
    fn put_provider_states_atomic(&self, _: Vec<ProviderStateWrite>) -> Result<(), StoreError> {
        panic!("write fallback")
    }
    fn mutate_provider_states_atomic(
        &self,
        _: Vec<ProviderStateMutation>,
    ) -> Result<(), StoreError> {
        panic!("write fallback")
    }
}
#[test]
fn checked_read_defaults_refuse_without_row_or_audit_fallback() {
    let store = MemoryStore::new(StoreLimits::default());
    let r = request(&store);
    assert_eq!(
        Unsupported.publish_provider_read_audited(r.clone()),
        Err(StoreError::InvalidTransition)
    );
    store.publish_provider_read_audited(r.clone()).unwrap();
    assert_eq!(
        Unsupported.assert_provider_replay(replay(&store, &r)),
        Err(StoreError::InvalidTransition)
    );
}
#[test]
fn checked_read_physical_maximum_read_count_does_not_consume_row_capacity() {
    let (_owned, stores) = backends();
    for store in stores {
        let mut r = request(store.as_ref());
        r.dependencies = (0..4094)
            .map(|i| TerminalRowDependency::Absent {
                namespace: "publication-fixture-v1".into(),
                key: if i == 0 {
                    "receipt".into()
                } else {
                    format!("absent{i}")
                },
            })
            .collect();
        let mut too_many = r.clone();
        too_many.dependencies.push(TerminalRowDependency::Absent {
            namespace: "publication-fixture-v1".into(),
            key: "one-too-many".into(),
        });
        let before = snapshot(store.as_ref(), &r);
        assert_eq!(
            store.publish_provider_read_audited(too_many),
            Err(StoreError::CapacityExceeded)
        );
        assert_eq!(snapshot(store.as_ref(), &r), before);
        store.publish_provider_read_audited(r.clone()).unwrap();
        assert_eq!(
            store
                .list_provider_state("publication-fixture-v1", 10000)
                .unwrap()
                .len(),
            3
        );
    }
}
#[test]
fn checked_read_budget_checked_arithmetic_refuses_overflow() {
    let mut budget = Budget::new(1);
    assert_eq!(budget.add(usize::MAX), Err(StoreError::CapacityExceeded));
    assert_eq!(budget.bytes, 1);
    assert!(budget.add(MAX_ROOT_PAYLOAD_BYTES).is_err());
}
