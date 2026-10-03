//! Physical fixtures only: no compiled application or native MQ settlement claim.
use super::*;
use sqlx::Connection;

#[derive(Debug, PartialEq)]
struct Footprint {
    execution: Option<ExecutionRecord>,
    events: Vec<LifecycleEvent>,
    outbox: Vec<OutboxRecord>,
    audits: Vec<AuditSubjectRecord>,
    checkpoint: Option<CheckpointRecord>,
    rows: Vec<ProviderStateRecord>,
    epoch: u64,
    clock: u64,
}

fn footprint(store: &dyn PlatformStore) -> Footprint {
    footprint_bounded(store, 64, 64, 64)
}
fn footprint_bounded(
    store: &dyn PlatformStore,
    event_limit: usize,
    outbox_limit: usize,
    audit_limit: usize,
) -> Footprint {
    let id = admission().execution.execution_id;
    let mut rows = Vec::new();
    for prefix in ["durable-", "mq-test-", "native-root-", "unrelated-"] {
        let captured = store.list_provider_state_prefix(prefix, 256).unwrap();
        assert!(captured.len() < 256, "fixture scan must be complete");
        rows.extend(captured);
    }
    Footprint {
        execution: store.get_execution(&id).unwrap(),
        events: store.events(&id, 1, event_limit).unwrap(),
        outbox: store.pending_notifications(outbox_limit).unwrap(),
        audits: store.audit_subject_records(&id, audit_limit).unwrap(),
        checkpoint: store.get_checkpoint(&id).unwrap(),
        rows,
        epoch: store.provider_state_retention_epoch().unwrap(),
        clock: store.advance_logical_clock(1).unwrap(),
    }
}

fn row(key: &str, version: u64, payload: &[u8]) -> ProviderStateRecord {
    ProviderStateRecord {
        namespace: "mq-test-terminal".into(),
        key: key.into(),
        version,
        payload: payload.into(),
    }
}

fn prepared(store: &dyn PlatformStore, normal: bool) -> RootTerminalPublication {
    for (key, payload) in [
        ("update", b"old".as_slice()),
        ("delete", b"gone"),
        ("move", b"moved"),
    ] {
        store
            .put_provider_state(row(key, 1, payload), None)
            .unwrap();
    }
    store
        .put_provider_state(
            ProviderStateRecord {
                namespace: "unrelated-fixture".into(),
                key: "protected-original".into(),
                version: 1,
                payload: b"original replay/core reference fixture bytes".to_vec(),
            },
            None,
        )
        .unwrap();
    let (claim, execution) = running(store);
    let mut request = publication(store, &claim, &execution, normal);
    request.dependencies.extend(
        [
            ("update", b"old".as_slice()),
            ("delete", b"gone"),
            ("move", b"moved"),
        ]
        .into_iter()
        .map(|(key, payload)| TerminalRowDependency::Exact(row(key, 1, payload))),
    );
    request.dependencies.push(TerminalRowDependency::Absent {
        namespace: "mq-test-terminal".into(),
        key: "destination".into(),
    });
    request.mutations.extend([
        ProviderStateMutation::Put(ProviderStateWrite {
            record: row("update", 2, b"new"),
            expected_version: Some(1),
        }),
        ProviderStateMutation::Delete {
            namespace: "mq-test-terminal".into(),
            key: "delete".into(),
            expected_version: 1,
        },
        ProviderStateMutation::Move {
            record: row("destination", 2, b"moved"),
            old_key: "move".into(),
            expected_version: 1,
        },
    ]);
    request
}

fn verify_known(store: &dyn PlatformStore, request: &RootTerminalPublication) -> Footprint {
    let before = footprint(store);
    let committed = store.commit_root_terminal_step(request.clone()).unwrap();
    let after = footprint(store);
    assert_eq!(after.execution, Some(committed.execution.clone()));
    assert_eq!(after.execution.as_ref().unwrap().terminal_tick, Some(10));
    let mut expected_events = before.events;
    expected_events.extend(request.steps.iter().map(|s| s.event.clone()));
    assert_eq!(after.events, expected_events);
    let mut expected_outbox = before.outbox;
    expected_outbox.extend(request.steps.iter().map(|s| s.notification.clone()));
    assert_eq!(after.outbox, expected_outbox);
    assert_eq!(
        after.audits,
        request
            .audits
            .iter()
            .cloned()
            .map(AuditSubjectRecord::RootTerminal)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        store
            .get_provider_state(ROOT_DRIVER_NAMESPACE, committed.winner.key.as_str())
            .unwrap(),
        Some(committed.winner)
    );
    assert_eq!(
        store.list_provider_state("mq-test-terminal", 32).unwrap(),
        vec![
            row("destination", 2, b"moved"),
            row("settled", 1, b"actual-delta"),
            row("update", 2, b"new"),
        ]
    );
    assert_eq!(
        store.list_provider_state("unrelated-fixture", 8).unwrap(),
        before
            .rows
            .into_iter()
            .filter(|r| r.namespace == "unrelated-fixture")
            .collect::<Vec<_>>()
    );
    assert!(after.epoch > before.epoch);
    assert_eq!(after.clock, 10);
    // Deliberately do not retain the commit return as an acknowledgement authority.
    after
}

fn refuse_after_lost_ack(
    store: &dyn PlatformStore,
    request: &RootTerminalPublication,
    known: &Footprint,
) {
    assert!(store.commit_root_terminal_step(request.clone()).is_err());
    let mut opposite = request.clone();
    opposite.disposition = match request.disposition {
        RootTerminalDisposition::Normal { .. } => RootTerminalDisposition::KnownAbnormal,
        RootTerminalDisposition::KnownAbnormal => {
            RootTerminalDisposition::Normal { return_code: 4 }
        }
    };
    let execution = &request.closure.actors[0].execution;
    opposite.steps = match opposite.disposition {
        RootTerminalDisposition::KnownAbnormal => {
            vec![(ExecutionState::Failed, LifecycleEventKind::Abend)]
        }
        RootTerminalDisposition::Normal { return_code } => vec![
            (ExecutionState::Completing, LifecycleEventKind::Completing),
            (
                ExecutionState::Completed,
                LifecycleEventKind::Completed { return_code },
            ),
        ],
    }
    .into_iter()
    .enumerate()
    .map(|(i, (next_state, kind))| {
        let event = event(execution, execution.version + i as u64 + 1, kind);
        RootTerminalStep {
            notification: outbox(&event),
            event,
            next_state,
        }
    })
    .collect();
    assert!(store.commit_root_terminal_step(opposite).is_err());
    let actor = known.execution.as_ref().unwrap();
    assert!(
        store
            .fence_root_driver(&request.closure.claim, actor, 11)
            .is_err()
    );
    assert!(
        store
            .close_root_driver(&request.closure.claim, actor, 11)
            .is_err()
    );
    assert!(store.admit_root_driver(admission()).is_err());
    assert!(
        store
            .delete_provider_state(ROOT_DRIVER_NAMESPACE, actor.execution_id.as_str(), 3)
            .is_err()
    );
    assert_eq!(&footprint(store), known);
}

fn reopen(fixture: &mut OwnedSqlite, rows: usize) {
    drop(fixture.store.take());
    fixture.store = Some(
        SqliteStateStore::open(
            &format!("sqlite://{}?mode=rw", fixture.path.display()),
            1 << 20,
            rows,
        )
        .unwrap(),
    );
}

type PhysicalRows = Vec<(String, String, i64, Vec<u8>)>;
fn sql<T>(fixture: &OwnedSqlite, action: impl AsyncFnOnce(&mut sqlx::SqliteConnection) -> T) -> T {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let mut connection = sqlx::SqliteConnection::connect(&format!(
                "sqlite://{}?mode=rw",
                fixture.path.display()
            ))
            .await
            .unwrap();
            let result = action(&mut connection).await;
            connection.close().await.unwrap();
            result
        })
}
fn raw(fixture: &OwnedSqlite) -> (PhysicalRows, (i64, i64)) {
    sql(fixture, async |connection| {
        let rows = sqlx::query_as(
            "SELECT namespace,key,version,payload FROM provider_state ORDER BY namespace,key",
        )
        .fetch_all(&mut *connection)
        .await
        .unwrap();
        let counters =
            sqlx::query_as("SELECT epoch,clock_tick FROM retention_lock WHERE singleton=1")
                .fetch_one(&mut *connection)
                .await
                .unwrap();
        (rows, counters)
    })
}

#[test]
fn memory_failure_contract_known_winner_and_lost_ack() {
    for normal in [true, false] {
        let store = MemoryStore::new(StoreLimits::default());
        let request = prepared(&store, normal);
        let known = verify_known(&store, &request);
        refuse_after_lost_ack(&store, &request, &known);
    }
}

#[test]
fn sqlite_failure_contract_known_winner_lost_ack_exact_reopen() {
    for normal in [true, false] {
        let mut fixture = OwnedSqlite::new();
        let request = prepared(fixture.store(), normal);
        let known = verify_known(fixture.store(), &request);
        let physical = raw(&fixture);
        refuse_after_lost_ack(fixture.store(), &request, &known);
        assert_eq!(raw(&fixture), physical);
        reopen(&mut fixture, 1024);
        assert_eq!(footprint(fixture.store()), known);
        assert_eq!(raw(&fixture), physical);
        refuse_after_lost_ack(fixture.store(), &request, &known);
        assert_eq!(raw(&fixture), physical);
    }
}

fn late_cas(store: &dyn PlatformStore) {
    store
        .put_provider_state(
            ProviderStateRecord {
                namespace: "native-root-owned-test".into(),
                key: "last-cas".into(),
                version: 1,
                payload: vec![9],
            },
            None,
        )
        .unwrap();
    let mut request = prepared(store, true);
    request
        .dependencies
        .push(TerminalRowDependency::Exact(ProviderStateRecord {
            namespace: "native-root-owned-test".into(),
            key: "last-cas".into(),
            version: 1,
            payload: vec![9],
        }));
    let before = footprint(store);
    request.mutations.push(ProviderStateMutation::Delete {
        namespace: "native-root-owned-test".into(),
        key: "last-cas".into(),
        expected_version: 2,
    });
    assert!(store.commit_root_terminal_step(request.clone()).is_err());
    assert_eq!(footprint(store), before);
    request.mutations.pop();
    verify_known(store, &request);
}

#[test]
fn memory_failure_contract_final_cas_restores_whole_footprint() {
    late_cas(&MemoryStore::new(StoreLimits::default()));
}
#[test]
fn sqlite_failure_contract_final_cas_restores_whole_footprint() {
    let fixture = OwnedSqlite::new();
    late_cas(fixture.store());
}

#[test]
fn memory_failure_contract_each_late_quota_restores_closing() {
    for fault in 0..4 {
        let mut limits = StoreLimits::default();
        match fault {
            0 => limits.max_audits = 1,
            1 => limits.max_outbox = 4,
            2 => limits.max_events_per_execution = 4,
            3 => limits.max_total_blob_bytes = 32 * 1024,
            _ => unreachable!(),
        }
        let store = MemoryStore::new(limits);
        let mut request = prepared(&store, true);
        if fault == 3 {
            request.mutations[0] = ProviderStateMutation::Put(ProviderStateWrite {
                record: row("settled", 1, &vec![7; 64 * 1024]),
                expected_version: None,
            });
        }
        request.observed_tick = 20;
        for step in &mut request.steps {
            step.event.tick = 20;
        }
        for audit in &mut request.audits {
            audit.observed_tick = 20;
        }
        let before = footprint_bounded(
            &store,
            limits.max_events_per_execution.min(64),
            limits.max_outbox.min(64),
            limits.max_audits.min(64),
        );
        assert_eq!(
            store.commit_root_terminal_step(request.clone()),
            Err(StoreError::CapacityExceeded),
            "fault {fault}"
        );
        assert_eq!(
            footprint_bounded(
                &store,
                limits.max_events_per_execution.min(64),
                limits.max_outbox.min(64),
                limits.max_audits.min(64)
            ),
            before,
            "fault {fault}"
        );
        assert_eq!(
            store
                .get_provider_state(ROOT_DRIVER_NAMESPACE, "native-root")
                .unwrap(),
            Some(request.closure.closing)
        );
    }
}

#[test]
fn sqlite_failure_contract_late_physical_faults_rollback_and_reopen() {
    for fault in ["audit", "outbox", "clock"] {
        let mut fixture = OwnedSqlite::new();
        let mut request = prepared(fixture.store(), true);
        request.observed_tick = 20;
        for step in &mut request.steps {
            step.event.tick = 20;
        }
        for audit in &mut request.audits {
            audit.observed_tick = 20;
        }
        let trigger = match fault {
            "audit" => {
                "CREATE TRIGGER root_fault BEFORE INSERT ON provider_state WHEN NEW.namespace='durable-audit-v1' AND (SELECT COUNT(*) FROM provider_state WHERE namespace='durable-audit-v1')=1 BEGIN SELECT RAISE(ABORT,'second root audit fault'); END"
            }
            "outbox" => {
                "CREATE TRIGGER root_fault BEFORE INSERT ON provider_state WHEN NEW.namespace='durable-outbox' AND NEW.key LIKE '%00000000000000000005' BEGIN SELECT RAISE(ABORT,'root outbox fault'); END"
            }
            "clock" => {
                "CREATE TRIGGER root_fault BEFORE UPDATE OF clock_tick ON retention_lock WHEN NEW.clock_tick=20 BEGIN SELECT RAISE(ABORT,'root clock fault'); END"
            }
            _ => unreachable!(),
        };
        sql(&fixture, async |connection| {
            sqlx::query(trigger).execute(connection).await.unwrap();
        });
        let before = footprint(fixture.store());
        let physical = raw(&fixture);
        assert!(
            matches!(
                fixture.store().commit_root_terminal_step(request.clone()),
                Err(StoreError::Infrastructure(_))
            ),
            "fault {fault}"
        );
        assert_eq!(footprint(fixture.store()), before, "fault {fault}");
        assert_eq!(raw(&fixture), physical);
        reopen(&mut fixture, 1024);
        assert_eq!(footprint(fixture.store()), before);
        assert_eq!(raw(&fixture), physical);
        sql(&fixture, async |connection| {
            sqlx::query("DROP TRIGGER root_fault")
                .execute(connection)
                .await
                .unwrap();
        });
        let committed = fixture.store().commit_root_terminal_step(request).unwrap();
        assert_eq!(committed.execution.terminal_tick, Some(20));
    }
}

fn uncertain_setup(
    store: &dyn PlatformStore,
    closing: bool,
) -> (
    RootDriverClaim,
    ExecutionRecord,
    Option<RootTerminalPublication>,
    ProviderStateRecord,
) {
    let (claim, execution) = running(store);
    let request = closing.then(|| publication(store, &claim, &execution, true));
    let fence = store.fence_root_driver(&claim, &execution, 11).unwrap();
    (claim, execution, request, fence)
}
fn uncertain_refusals(
    store: &dyn PlatformStore,
    claim: &RootDriverClaim,
    execution: &ExecutionRecord,
    request: Option<&RootTerminalPublication>,
    fence: &ProviderStateRecord,
) {
    let before = footprint(store);
    assert_eq!(
        store.fence_root_driver(claim, execution, 11).unwrap(),
        *fence
    );
    assert!(store.close_root_driver(claim, execution, 11).is_err());
    assert!(store.admit_root_driver(admission()).is_err());
    if let Some(request) = request {
        assert!(store.commit_root_terminal_step(request.clone()).is_err());
    }
    assert!(
        store
            .put_provider_state(row("new", 1, b"no"), None)
            .is_err()
    );
    assert!(
        store
            .append_event(event(execution, 4, LifecycleEventKind::Abend))
            .is_err()
    );
    assert!(
        store
            .delete_provider_state(ROOT_DRIVER_NAMESPACE, fence.key.as_str(), fence.version)
            .is_err()
    );
    assert_eq!(footprint(store), before);
    let notification = before.outbox[0].clone();
    let delivered = store
        .mark_notification_delivered(&notification.notification_id, notification.version, 11)
        .unwrap();
    assert!(delivered.delivered);
    let after = footprint(store);
    assert_eq!(after.execution, before.execution);
    assert_eq!(after.events, before.events);
    assert_eq!(after.audits, before.audits);
    assert_eq!(
        store
            .get_provider_state(ROOT_DRIVER_NAMESPACE, fence.key.as_str())
            .unwrap(),
        Some(fence.clone())
    );
    assert_eq!(after.outbox, before.outbox[1..]);
}

#[test]
fn memory_failure_contract_uncertain_fence_no_guessed_settlement() {
    for closing in [false, true] {
        let store = MemoryStore::new(StoreLimits::default());
        let (claim, execution, request, fence) = uncertain_setup(&store, closing);
        uncertain_refusals(&store, &claim, &execution, request.as_ref(), &fence);
    }
}
#[test]
fn sqlite_failure_contract_uncertain_fence_exact_reopen_and_delivery_only() {
    for closing in [false, true] {
        let mut fixture = OwnedSqlite::new();
        let (claim, execution, request, fence) = uncertain_setup(fixture.store(), closing);
        let before = footprint(fixture.store());
        let physical = raw(&fixture);
        reopen(&mut fixture, 1024);
        assert_eq!(footprint(fixture.store()), before);
        assert_eq!(raw(&fixture), physical);
        uncertain_refusals(
            fixture.store(),
            &claim,
            &execution,
            request.as_ref(),
            &fence,
        );
        let delivered = footprint(fixture.store());
        let physical = raw(&fixture);
        reopen(&mut fixture, 1024);
        assert_eq!(footprint(fixture.store()), delivered);
        assert_eq!(raw(&fixture), physical);
    }
}

fn dependency_refusals(store: &dyn PlatformStore) {
    let request = prepared(store, true);
    let before = footprint(store);
    for dependency in [
        TerminalRowDependency::Exact(row("update", 2, b"old")),
        TerminalRowDependency::Exact(row("update", 1, b"different-bytes")),
        TerminalRowDependency::Absent {
            namespace: "mq-test-terminal".into(),
            key: "update".into(),
        },
    ] {
        let mut bad = request.clone();
        bad.dependencies.push(dependency);
        assert!(store.commit_root_terminal_step(bad).is_err());
        assert_eq!(footprint(store), before);
    }
    verify_known(store, &request);
}
#[test]
fn memory_failure_contract_exact_absent_dependencies_do_not_publish() {
    dependency_refusals(&MemoryStore::new(StoreLimits::default()));
}
#[test]
fn sqlite_failure_contract_exact_absent_dependencies_do_not_publish() {
    let fixture = OwnedSqlite::new();
    dependency_refusals(fixture.store());
}
#[test]
fn sqlite_failure_contract_final_row_quota_exact_reopen() {
    // Existing bounded fixture: admission/Running/Closing fit; final winner does not.
    let mut fixture = OwnedSqlite::with_rows(18);
    let (claim, execution) = running(fixture.store());
    let mut request = publication(fixture.store(), &claim, &execution, true);
    request.observed_tick = 20;
    for step in &mut request.steps {
        step.event.tick = 20;
    }
    for audit in &mut request.audits {
        audit.observed_tick = 20;
    }
    let before = footprint(fixture.store());
    let physical = raw(&fixture);
    assert_eq!(
        fixture.store().commit_root_terminal_step(request.clone()),
        Err(StoreError::CapacityExceeded)
    );
    assert_eq!(footprint(fixture.store()), before);
    assert_eq!(raw(&fixture), physical);
    reopen(&mut fixture, 18);
    assert_eq!(footprint(fixture.store()), before);
    assert_eq!(raw(&fixture), physical);
    assert_eq!(
        fixture
            .store()
            .get_provider_state(ROOT_DRIVER_NAMESPACE, "native-root")
            .unwrap(),
        Some(request.closure.closing)
    );
}
