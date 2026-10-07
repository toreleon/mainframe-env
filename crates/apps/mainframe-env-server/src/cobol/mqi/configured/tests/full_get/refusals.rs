//! Original installed GET refusal and uncertain removed work.
use super::*;

fn unknown(outcome: ExecutionOutcome) {
    assert!(
        matches!(outcome,ExecutionOutcome::ProviderFailure(ref p)if p.has_unknown_outcome()),
        "{outcome:?}"
    );
}
fn occurrences(f: &Fixture) -> usize {
    f.store
        .list_provider_state("mq-selected-v1-occurrence", 128)
        .unwrap()
        .len()
}
fn no_retry(f: &Fixture, i: &Invocation) {
    let frozen = (
        f.rows(),
        f.store.events(&i.execution_id, 1, 128).unwrap(),
        f.store.pending_notifications(128).unwrap(),
        f.store.audit_subject_records(&i.execution_id, 128).unwrap(),
    );
    assert!(match f.router.execute_native_mq_root(&f.mq, i, "MQGETER") {
        Err(_) => true,
        Ok(ExecutionOutcome::ProviderFailure(ref p)) => p.has_unknown_outcome(),
        _ => false,
    });
    assert_eq!(
        (
            f.rows(),
            f.store.events(&i.execution_id, 1, 128).unwrap(),
            f.store.pending_notifications(128).unwrap(),
            f.store.audit_subject_records(&i.execution_id, 128).unwrap()
        ),
        frozen
    );
}
#[test]
fn installed_original_get_saf_cancel_catalog_core_and_physical_epoch_races_do_not_remove_message() {
    for sqlite in [false, true] {
        for mode in 0..5 {
            let f = native(sqlite, 1);
            produce(&f, 1, false);
            f.install("MQGETER", &source(1, 0, false, 0));
            let i = original(&f, "MQGETER", "get-denied");
            let store = f.store.clone();
            let saf = f.saf.clone();
            let probe = i.cancellation_probe.clone().unwrap();
            let changed = Arc::new(AtomicBool::new(false));
            let flag = changed.clone();
            let captured = Arc::new(Mutex::new(vec![]));
            let target = captured.clone();
            *f.saf.queue_observer.lock().unwrap() = Some(Box::new(move || {
                if store
                    .list_provider_state("mq-selected-v1-occurrence", 128)
                    .unwrap()
                    .len()
                    != 7
                    || flag.swap(true, Ordering::SeqCst)
                {
                    return;
                }
                match mode {
                    0 => saf.deny.store(true, Ordering::SeqCst),
                    1 => probe.request(),
                    2 => {
                        let mut row = store
                            .get_provider_state("mq-v1-object-catalog", "catalog")
                            .unwrap()
                            .unwrap();
                        let old = row.version;
                        row.version += 1;
                        store.put_provider_state(row, Some(old)).unwrap();
                    }
                    3 => {
                        let e = store
                            .get_execution(
                                &ExecutionId::new("get-denied", Default::default()).unwrap(),
                            )
                            .unwrap()
                            .unwrap();
                        store
                            .transition_execution(
                                &e.execution_id,
                                e.version,
                                ExecutionState::Suspended,
                                20,
                            )
                            .unwrap();
                    }
                    _ => {
                        let mut row = store
                            .list_provider_state("mq-selected-v1-control", 128)
                            .unwrap()
                            .remove(0);
                        let old = row.version;
                        row.version += 1;
                        store.put_provider_state(row, Some(old)).unwrap();
                    }
                }
                *target.lock().unwrap() = store.list_provider_state_prefix("mq-", 4096).unwrap();
            }));
            unknown(
                f.router
                    .execute_native_mq_root(&f.mq, &i, "MQGETER")
                    .unwrap(),
            );
            assert!(changed.load(Ordering::SeqCst));
            assert_eq!(f.rows(), *captured.lock().unwrap());
            assert_eq!(occurrences(&f), 7);
            queue(&f, 1, Some(0));
            assert!(
                f.store
                    .list_provider_state("mq-delivery-live-v1-pending", 128)
                    .unwrap()
                    .is_empty()
            );
            no_retry(&f, &i);
        }
    }
}
#[test]
fn real_removed_get_late_panic_cancel_expiry_catalog_and_terminal_races_retain_pending_once() {
    for sqlite in [false, true] {
        for mode in 0..7 {
            let f = native(sqlite, 2);
            produce(&f, 2, false);
            f.install("MQGETER", &source(2, 0, false, 0));
            let i = original(&f, "MQGETER", "get-uncertain");
            let store = f.store.clone();
            let probe = i.cancellation_probe.clone().unwrap();
            let clock = f.clock.clone();
            let saf = f.saf.clone();
            let changed = Arc::new(AtomicBool::new(false));
            let flag = changed.clone();
            let hook = move || {
                if !removed_pending(&*store) || flag.swap(true, Ordering::SeqCst) {
                    return;
                }
                if mode < 4 {
                    match mode {
                        0 => panic!("real removed GET clock panic"),
                        1 => probe.request(),
                        2 => clock.0.store(1000, Ordering::SeqCst),
                        _ => {
                            let mut row = store
                                .get_provider_state("mq-v1-object-catalog", "catalog")
                                .unwrap()
                                .unwrap();
                            let old = row.version;
                            row.version += 1;
                            store.put_provider_state(row, Some(old)).unwrap();
                        }
                    }
                } else {
                    match mode {
                        4 => saf.deny.store(true, Ordering::SeqCst),
                        5 => probe.request(),
                        _ => {
                            store
                                .put_provider_state(
                                    ProviderStateRecord {
                                        namespace: "test-get-terminal-race".into(),
                                        key: "x".into(),
                                        version: 1,
                                        payload: vec![8],
                                    },
                                    None,
                                )
                                .unwrap();
                        }
                    }
                }
            };
            if mode < 4 {
                *f.clock.1.lock().unwrap() = Some(Box::new(hook));
            } else {
                *f.saf.terminal_hook.lock().unwrap() = Some(Box::new(hook));
            }
            unknown(
                f.router
                    .execute_native_mq_root(&f.mq, &i, "MQGETER")
                    .unwrap(),
            );
            assert!(changed.load(Ordering::SeqCst));
            pending(&*f.store, 2);
            assert_eq!(occurrences(&f), 8);
            let audits = f.store.audit_subject_records(&i.execution_id, 128).unwrap();
            assert!(
                audits
                    .iter()
                    .all(|a| !matches!(a, AuditSubjectRecord::RootTerminal(_)))
            );
            assert_eq!(
                f.store
                    .get_execution(&i.execution_id)
                    .unwrap()
                    .unwrap()
                    .state,
                ExecutionState::Running
            );
            no_retry(&f, &i);
            if sqlite {
                let db =
                    mainframe_env_store::SqliteStateStore::open(&f.url, 64 << 20, 65536).unwrap();
                assert_eq!(
                    db.list_provider_state_prefix("mq-", 4096).unwrap(),
                    f.rows()
                );
                pending(&db, 2);
            }
        }
    }
}
#[test]
fn malformed_or_unrepresented_complete_get_never_dispatches_original_get() {
    for sqlite in [false, true] {
        for version in [1, 2] {
            for mode in 0..12 {
                let f = native(sqlite, version);
                produce(&f, version, false);
                let text = source(version, 0, false, 0);
                let text = match mode {
                    0 => text.replace(
                        "GMO-VERSION PIC S9(9) BINARY VALUE 1.",
                        "GMO-VERSION PIC S9(9) BINARY VALUE 2.",
                    ),
                    1 => text.replace(
                        "GMO-STRUCID PIC X(4) VALUE 'GMO '.",
                        "GMO-STRUCID PIC X(4) VALUE 'BAD '.",
                    ),
                    2 => text.replace(
                        "BUFFER-LENGTH PIC S9(9) BINARY VALUE 5.",
                        "BUFFER-LENGTH PIC S9(9) BINARY VALUE 2049.",
                    ),
                    3 => text.replace(
                        "BUFFER-LENGTH PIC S9(9) BINARY VALUE 5.",
                        "BUFFER-LENGTH PIC S9(9) BINARY VALUE -1.",
                    ),
                    4 => text.replace(
                        "MD-MSGID PIC X(24) VALUE LOW-VALUES.",
                        "MD-MSGID PIC X(24) VALUE 'SELECTOR'.",
                    ),
                    5 => text.replace(
                        "MD-FORMAT PIC X(8) VALUE SPACES.",
                        "MD-FORMAT PIC X(8) VALUE 'MQSTR'.",
                    ),
                    _ => text.replace(
                        "GMO-OPTIONS PIC S9(9) BINARY VALUE 2.",
                        &format!(
                            "GMO-OPTIONS PIC S9(9) BINARY VALUE {}.",
                            [8, 16, 16384, 1, 4096, 6][mode - 6]
                        ),
                    ),
                };
                assert_ne!(text, source(version, 0, false, 0));
                f.install("MQGETER", &text);
                let i = original(&f, "MQGETER", "get-malformed");
                unknown(
                    f.router
                        .execute_native_mq_root(&f.mq, &i, "MQGETER")
                        .unwrap(),
                );
                assert_eq!(occurrences(&f), 7);
                queue(&f, version, Some(0));
                assert!(f.mq.originals.lock().unwrap().iter().all(|(_,e,_)|!matches!(&e.request,HostRequest::MqMqi(h)if matches!(h.envelope.request,MqMqiRequest::QualifiedFullGet(_)))));
                no_retry(&f, &i);
            }
        }
    }
}

#[test]
fn actual_get_audit_quota_rolls_back_full_message_unit_and_receipt_publication() {
    for sqlite in [false, true] {
        let f =
            Fixture::native_points_quota(sqlite, include_bytes!("../native_rich_md1.json"), 128);
        produce(&f, 1, false);
        f.install("MQGETER", &source(1, 0, false, 0));
        let i = original(&f, "MQGETER", "get-quota");
        let store = f.store.clone();
        let execution = i.execution_id.clone();
        let changed = Arc::new(AtomicBool::new(false));
        let flag = changed.clone();
        let captured = Arc::new(Mutex::new(vec![]));
        let target = captured.clone();
        *f.saf.queue_observer.lock().unwrap() = Some(Box::new(move || {
            if store
                .list_provider_state("mq-selected-v1-occurrence", 128)
                .unwrap()
                .len()
                != 7
                || flag.swap(true, Ordering::SeqCst)
            {
                return;
            }
            let audit = store
                .audit_subject_records(&execution, 128)
                .unwrap()
                .into_iter()
                .find_map(|a| {
                    if let AuditSubjectRecord::Effect(a) = a {
                        Some(a)
                    } else {
                        None
                    }
                })
                .unwrap();
            let mut inserted = 0;
            for _ in 0..=128 {
                if store.record_audit(audit.clone()).is_err() {
                    break;
                }
                inserted += 1;
            }
            assert!(inserted > 0);
            *target.lock().unwrap() = store.list_provider_state_prefix("mq-", 4096).unwrap();
        }));
        unknown(
            f.router
                .execute_native_mq_root(&f.mq, &i, "MQGETER")
                .unwrap(),
        );
        assert!(changed.load(Ordering::SeqCst));
        assert_eq!(f.rows(), *captured.lock().unwrap());
        queue(&f, 1, Some(0));
        assert_eq!(occurrences(&f), 7);
        assert!(!removed_pending(&*f.store));
        no_retry(&f, &i);
    }
}

#[test]
fn genuine_retired_unknown_object_alias_and_dropped_get_child_cannot_authorize_removal() {
    for sqlite in [false, true] {
        for mode in 0..3 {
            let f = native(sqlite, 1);
            produce(&f, 1, false);
            let text = source(1, 0, false, 0);
            let get = "CALL 'MQGET' USING BY REFERENCE HCONN HOBJ";
            let text=match mode {
            0=>text.replace(get,&format!("CALL 'MQCLOSE' USING BY REFERENCE HCONN HOBJ CLOSE-OPTIONS CC REASON.\n{get}")),
            1=>text.replace(get,&format!("MOVE 777 TO HOBJ.\n{get}")),_=>text};
            if mode == 2 {
                f.install(
                    "MQLEAF",
                    &text
                        .replace("PROGRAM-ID. MQGETER", "PROGRAM-ID. MQLEAF")
                        .replace("CALL 'MQPUTER'.\n", ""),
                );
                f.install("MQGETER","IDENTIFICATION DIVISION. PROGRAM-ID. MQGETER. PROCEDURE DIVISION. CALL 'MQPUTER'. CALL 'MQLEAF'. GOBACK.");
                let factory = Arc::downgrade(&f.factory);
                let store = f.store.clone();
                *f.clock.1.lock().unwrap() = Some(Box::new(move || {
                    if store
                        .list_provider_state("mq-selected-v1-occurrence", 128)
                        .unwrap()
                        .len()
                        >= 5
                    {
                        factory
                            .upgrade()
                            .unwrap()
                            .drop_at_admission
                            .store(true, Ordering::SeqCst);
                    }
                }));
            } else {
                f.install("MQGETER", &text);
            }
            let i = original(&f, "MQGETER", "get-stale-alias");
            unknown(
                f.router
                    .execute_native_mq_root(&f.mq, &i, "MQGETER")
                    .unwrap(),
            );
            queue(&f, 1, Some(0));
            assert!(!removed_pending(&*f.store));
            assert!(f.mq.originals.lock().unwrap().iter().all(|(_,e,_)|!matches!(&e.request,HostRequest::MqMqi(h)if matches!(h.envelope.request,MqMqiRequest::QualifiedFullGet(_)))));
            no_retry(&f, &i);
        }
    }
}
