//! Real owning backend fixtures, not host/SAF/provider admission evidence.
use super::*;
use crate::publication::tests::prepare;
use crate::{MemoryStore, SqliteStateStore, StoreLimits};
use mainframe_env_execution_api::LifecycleEvent;
use std::sync::{Arc, Barrier};

pub(crate) fn request(store: &dyn PlatformStore) -> CheckedReplayRefusalStep {
    let p = prepare(store);
    let execution = store
        .get_execution(&p.intent.execution_id)
        .unwrap()
        .unwrap();
    for sequence in 1..=execution.version {
        store
            .append_event(LifecycleEvent {
                execution_id: execution.execution_id.clone(),
                run_unit_id: execution.run_unit_id.clone(),
                attempt: 1,
                sequence,
                tick: sequence + 4,
                kind: if sequence == 1 {
                    LifecycleEventKind::Admitted
                } else {
                    LifecycleEventKind::Started
                },
            })
            .unwrap();
    }
    let mut effect = p.intent;
    effect.state = EffectState::Completed;
    effect.result_digest = Some([0x81; 32]);
    effect.resolved_tick = Some(9);
    store.record_result(&effect.key, effect.clone()).unwrap();
    let receipt = ProviderStateRecord {
        namespace: "publication-fixture-v1".into(),
        key: "receipt".into(),
        version: 1,
        payload: b"original exact result".to_vec(),
    };
    store.put_provider_state(receipt.clone(), None).unwrap();
    let mut audit = p.audit;
    audit.decision = AuditDecision::Deny;
    audit.observed_tick = 10;
    let event = LifecycleEvent {
        execution_id: execution.execution_id.clone(),
        run_unit_id: execution.run_unit_id.clone(),
        sequence: execution.version + 1,
        attempt: 1,
        tick: 10,
        kind: LifecycleEventKind::EffectResult {
            sequence: effect.sequence,
        },
    };
    let notification = OutboxRecord {
        notification_id: format!("{}:{:020}", execution.execution_id, event.sequence),
        execution_id: execution.execution_id.clone(),
        sequence: event.sequence,
        topic: "execution.lifecycle.v1".into(),
        payload: mainframe_env_execution_api::lifecycle_notification_payload(&event.kind),
        attempt: 0,
        delivered: false,
        delivered_tick: None,
        version: 1,
    };
    CheckedReplayRefusalStep {
        effect,
        execution,
        dependencies: vec![TerminalRowDependency::Exact(receipt)],
        audit,
        event,
        notification,
    }
}

struct OwnedFile(std::path::PathBuf);
impl Drop for OwnedFile {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}
fn backends(label: &str) -> (OwnedFile, Vec<Arc<dyn PlatformStore>>) {
    let path = std::env::temp_dir().join(format!("checked-refusal-{}-{label}", std::process::id()));
    std::fs::create_dir(&path).unwrap();
    let sqlite = SqliteStateStore::open(
        &format!("sqlite://{}/state.db?mode=rwc", path.display()),
        8192,
        20_000,
    )
    .unwrap();
    (
        OwnedFile(path),
        vec![
            Arc::new(MemoryStore::new(StoreLimits::default())),
            Arc::new(sqlite),
        ],
    )
}
pub(crate) fn snapshot(
    store: &dyn PlatformStore,
    r: &CheckedReplayRefusalStep,
) -> (
    ExecutionRecord,
    Option<EffectRecord>,
    Vec<LifecycleEvent>,
    Vec<mainframe_env_execution_api::AuditRecord>,
    Vec<OutboxRecord>,
    Vec<ProviderStateRecord>,
    u64,
) {
    (
        store
            .get_execution(&r.execution.execution_id)
            .unwrap()
            .unwrap(),
        store.effect(&r.effect.key).unwrap(),
        store.events(&r.execution.execution_id, 1, 100).unwrap(),
        store
            .audit_records(&r.execution.execution_id, 1, 100)
            .unwrap(),
        store.pending_notifications(100).unwrap(),
        store
            .list_provider_state("publication-fixture-v1", 100)
            .unwrap(),
        store.provider_state_retention_epoch().unwrap(),
    )
}

#[test]
fn checked_replay_refusal_publishes_exact_audit_event_outbox_and_leaves_completed_receipt_exact() {
    let (_owned, stores) = backends("success");
    for store in stores {
        let r = request(store.as_ref());
        let old = snapshot(store.as_ref(), &r);
        let updated = store.commit_checked_replay_refusal(r.clone()).unwrap();
        assert_eq!(updated.version, 4);
        assert_eq!(updated.state, ExecutionState::Running);
        assert_eq!(store.effect(&r.effect.key).unwrap(), Some(r.effect.clone()));
        assert_eq!(
            store
                .list_provider_state("publication-fixture-v1", 100)
                .unwrap(),
            old.5
        );
        assert_eq!(
            store
                .audit_records(&r.execution.execution_id, 1, 100)
                .unwrap(),
            vec![r.audit.clone()]
        );
        assert_eq!(
            store
                .events(&r.execution.execution_id, 1, 100)
                .unwrap()
                .last(),
            Some(&r.event)
        );
        assert_eq!(
            store.pending_notifications(100).unwrap(),
            vec![r.notification.clone()]
        );
        let known = snapshot(store.as_ref(), &r);
        assert!(store.commit_checked_replay_refusal(r.clone()).is_err());
        assert_eq!(snapshot(store.as_ref(), &r), known);
    }
}
#[test]
fn checked_replay_refusal_exact_whole_metadata_read_clock_and_event_faults_leave_everything_unchanged()
 {
    let (_owned, stores) = backends("faults");
    for store in stores {
        let r = request(store.as_ref());
        let before = snapshot(store.as_ref(), &r);
        for fault in 0..28 {
            let mut x = r.clone();
            match fault {
                0 => x.execution.version += 1,
                1 => x.execution.owner_lease = Some("foreign".into()),
                2 => x.execution.lease_expiry_tick = Some(10),
                3 => x.execution.state = ExecutionState::Completed,
                4 => x.effect.request_digest[0] ^= 1,
                5 => x.effect.result_digest = Some([9; 32]),
                6 => x.effect.intent.epoch += 1,
                7 => x.effect.intent.attempt += 1,
                8 => x.effect.state = EffectState::Failed,
                9 => {
                    x.effect.state = EffectState::UnknownOutcome;
                    x.effect.resolved_tick = None;
                }
                10 => {
                    x.effect.state = EffectState::Intent;
                    x.effect.result_digest = None;
                    x.effect.resolved_tick = None;
                }
                11 => {
                    x.effect.intent.recovery_lease = Some(EffectRecoveryLease {
                        owner: "recovery".into(),
                        attempt: 1,
                        epoch: 1,
                        expires_tick: 99,
                    })
                }
                12 => x.audit.resource.value[0] ^= 1,
                13 => x.audit.decision = AuditDecision::Success,
                14 => x.audit.effect_sequence += 1,
                15 => x.audit.observed_tick += 1,
                16 => x.event.sequence += 1,
                17 => x.notification.payload.push(1),
                18 => x.notification.notification_id.push('x'),
                19 => x.dependencies.push(x.dependencies[0].clone()),
                20 => {
                    x.dependencies[0] = TerminalRowDependency::Absent {
                        namespace: "publication-fixture-v1".into(),
                        key: "receipt".into(),
                    }
                }
                21 => x.dependencies.push(TerminalRowDependency::Absent {
                    namespace: "durable-effect".into(),
                    key: "system".into(),
                }),
                22 => {
                    if let TerminalRowDependency::Exact(row) = &mut x.dependencies[0] {
                        row.version += 1;
                    }
                }
                23 => {
                    if let TerminalRowDependency::Exact(row) = &mut x.dependencies[0] {
                        row.payload.push(0);
                    }
                }
                24 => x.event.tick = 0,
                25 => {
                    x.audit.invocation_key = mainframe_env_execution_api::IdempotencyKey::new(
                        "wrong",
                        mainframe_env_execution_api::InvocationLimits::default(),
                    )
                    .unwrap()
                }
                26 => x.execution.attempt += 1,
                27 => x.notification.version = 2,
                _ => unreachable!(),
            }
            assert!(
                store.commit_checked_replay_refusal(x).is_err(),
                "fault {fault}"
            );
            assert_eq!(snapshot(store.as_ref(), &r), before, "fault {fault}");
        }
    }
}
#[test]
fn checked_replay_refusal_competing_cursor_has_one_winner_and_no_duplicate_audit() {
    let (_owned, stores) = backends("compete");
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
                    s.commit_checked_replay_refusal(r)
                })
            })
            .collect();
        let results: Vec<_> = workers.into_iter().map(|w| w.join().unwrap()).collect();
        assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
        assert_eq!(
            store
                .audit_records(&r.execution.execution_id, 1, 100)
                .unwrap(),
            vec![r.audit]
        );
    }
}
#[test]
fn checked_replay_refusal_operation_and_payload_budgets_preflight_no_writes() {
    let (_owned, stores) = backends("bounds");
    for store in stores {
        let mut r = request(store.as_ref());
        let before = snapshot(store.as_ref(), &r);
        let mut oversized = r.clone();
        oversized.dependencies = (0..65)
            .map(|i| {
                TerminalRowDependency::Exact(ProviderStateRecord {
                    namespace: "publication-fixture-v1".into(),
                    key: format!("large{i}"),
                    version: 1,
                    payload: vec![0; 1024 * 1024],
                })
            })
            .collect();
        assert_eq!(
            oversized.validate_bounds(usize::MAX),
            Err(StoreError::CapacityExceeded)
        );
        assert!(store.commit_checked_replay_refusal(oversized).is_err());
        assert_eq!(snapshot(store.as_ref(), &r), before);
        for i in 1..4086 {
            r.dependencies.push(TerminalRowDependency::Absent {
                namespace: "publication-fixture-v1".into(),
                key: format!("absent{i}"),
            });
        }
        let mut excess = r.clone();
        excess.dependencies.push(TerminalRowDependency::Absent {
            namespace: "publication-fixture-v1".into(),
            key: "excess".into(),
        });
        let before = snapshot(store.as_ref(), &r);
        assert_eq!(
            store.commit_checked_replay_refusal(excess),
            Err(StoreError::CapacityExceeded)
        );
        assert_eq!(snapshot(store.as_ref(), &r), before);
        store.commit_checked_replay_refusal(r).unwrap();
    }
}

#[test]
fn checked_replay_refusal_owned_sqlite_orderly_reopen_preserves_exact_settlement() {
    let (owned, stores) = backends("reopen");
    let store = stores.into_iter().nth(1).unwrap();
    let r = request(store.as_ref());
    store.commit_checked_replay_refusal(r.clone()).unwrap();
    let known = snapshot(store.as_ref(), &r);
    drop(store);
    let reopened = SqliteStateStore::open(
        &format!("sqlite://{}/state.db?mode=rwc", owned.0.display()),
        8192,
        20_000,
    )
    .unwrap();
    assert_eq!(snapshot(&reopened, &r), known);
    assert!(reopened.commit_checked_replay_refusal(r.clone()).is_err());
    assert_eq!(snapshot(&reopened, &r), known);
}

#[test]
fn checked_replay_refusal_default_adapter_never_falls_back_to_plain_journal() {
    struct Unsupported;
    impl JournalStore for Unsupported {
        fn admit_execution(
            &self,
            _: ExecutionRecord,
            _: LifecycleEvent,
            _: OutboxRecord,
        ) -> Result<(), StoreError> {
            panic!("ordinary admission fallback")
        }
        fn commit_execution_step(
            &self,
            _: &mainframe_env_execution_api::ExecutionId,
            _: u64,
            _: Option<ExecutionState>,
            _: LifecycleEvent,
            _: Option<EffectRecord>,
            _: Option<mainframe_env_execution_api::AuditRecord>,
            _: Option<CheckpointRecord>,
            _: OutboxRecord,
        ) -> Result<ExecutionRecord, StoreError> {
            panic!("plain journal fallback")
        }
    }
    let store = MemoryStore::new(StoreLimits::default());
    assert_eq!(
        Unsupported.commit_checked_replay_refusal(request(&store)),
        Err(StoreError::InvalidTransition)
    );
}
