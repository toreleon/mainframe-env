//! Original compiled PUT refusal and retained uncertainty proofs.
use super::*;
use std::sync::atomic::{AtomicBool, Ordering};

fn unknown(outcome: ExecutionOutcome) {
    assert!(
        matches!(outcome,ExecutionOutcome::ProviderFailure(ref p) if p.has_unknown_outcome()),
        "{outcome:?}"
    );
}
fn no_retry(f: &Fixture, original: &Invocation) {
    let before = (
        f.rows(),
        f.store.events(&original.execution_id, 1, 128).unwrap(),
        f.store.pending_notifications(128).unwrap(),
    );
    let result = f.router.execute_native_mq_root(&f.mq, original, "MQROOT");
    assert!(match result {
        Err(_) => true,
        Ok(ExecutionOutcome::ProviderFailure(ref p)) => p.has_unknown_outcome(),
        _ => false,
    });
    assert_eq!(
        (
            f.rows(),
            f.store.events(&original.execution_id, 1, 128).unwrap(),
            f.store.pending_notifications(128).unwrap()
        ),
        before
    );
}
fn occurrences(f: &Fixture) -> usize {
    f.store
        .list_provider_state("mq-selected-v1-occurrence", 128)
        .unwrap()
        .len()
}

#[test]
fn original_put_saf_cancel_and_catalog_cas_refuse_atomically_without_pending_work() {
    for sqlite in [false, true] {
        for one in [false, true] {
            for mode in 0..3 {
                let f = native(sqlite, 1);
                f.install("MQROOT", &source(1, one, false));
                let original = super::super::native_root::original(&f);
                let store = f.store.clone();
                let saf = f.saf.clone();
                let probe = original.cancellation_probe.clone().unwrap();
                let captured = Arc::new(Mutex::new(vec![]));
                let target = captured.clone();
                let changed = Arc::new(AtomicBool::new(false));
                let flag = changed.clone();
                *f.saf.terminal_hook.lock().unwrap() = Some(Box::new(move || {
                    if store
                        .list_provider_state("mq-selected-v1-occurrence", 128)
                        .unwrap()
                        .len()
                        != if one { 1 } else { 2 }
                        || flag.swap(true, Ordering::SeqCst)
                    {
                        return;
                    }
                    match mode {
                        0 => saf.deny.store(true, Ordering::SeqCst),
                        1 => probe.request(),
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
                    *target.lock().unwrap() =
                        store.list_provider_state_prefix("mq-", 4096).unwrap();
                }));
                unknown(
                    f.router
                        .execute_native_mq_root(&f.mq, &original, "MQROOT")
                        .unwrap(),
                );
                assert!(changed.load(Ordering::SeqCst));
                assert_eq!(f.rows(), *captured.lock().unwrap());
                assert_eq!(occurrences(&f), if one { 1 } else { 2 });
                assert!(
                    f.store
                        .list_provider_state("mq-delivery-live-v1-pending", 128)
                        .unwrap()
                        .is_empty()
                );
                no_retry(&f, &original);
            }
        }
    }
}

#[test]
fn post_put_clock_panic_cancel_expiry_and_source_drift_retain_once_without_task_decision() {
    for sqlite in [false, true] {
        for mode in 0..4 {
            let f = native(sqlite, 2);
            f.install("MQROOT", &source(2, false, false));
            let original = super::super::native_root::original(&f);
            let store = f.store.clone();
            let probe = original.cancellation_probe.clone().unwrap();
            let tick = f.clock.clone();
            let changed = Arc::new(AtomicBool::new(false));
            let flag = changed.clone();
            *f.clock.1.lock().unwrap() = Some(Box::new(move || {
                if store
                    .list_provider_state("mq-selected-v1-occurrence", 128)
                    .unwrap()
                    .len()
                    != 3
                    || flag.swap(true, Ordering::SeqCst)
                {
                    return;
                }
                match mode {
                    0 => panic!("actual completed PUT clock panic"),
                    1 => probe.request(),
                    2 => tick.0.store(1000, Ordering::SeqCst),
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
            }));
            unknown(
                f.router
                    .execute_native_mq_root(&f.mq, &original, "MQROOT")
                    .unwrap(),
            );
            assert!(changed.load(Ordering::SeqCst));
            assert_eq!(occurrences(&f), 3);
            pending(&*f.store, 2);
            assert_eq!(
                value(
                    &f.store
                        .list_provider_state("mq-selected-v1-uow-owner", 128)
                        .unwrap()[0]
                )["state"],
                "pending"
            );
            assert!(
                !f.store
                    .audit_subject_records(&original.execution_id, 128)
                    .unwrap()
                    .iter()
                    .any(|a| matches!(a, AuditSubjectRecord::RootTerminal(_)))
            );
            no_retry(&f, &original);
        }
    }
}

#[test]
fn pending_put_terminal_saf_cancel_and_physical_epoch_race_never_guess_backout() {
    for sqlite in [false, true] {
        for mode in 0..3 {
            let f = native(sqlite, 1);
            f.install("MQROOT", &source(1, false, false));
            let original = super::super::native_root::original(&f);
            let store = f.store.clone();
            let saf = f.saf.clone();
            let probe = original.cancellation_probe.clone().unwrap();
            let execution = original.execution_id.clone();
            let changed = Arc::new(AtomicBool::new(false));
            let flag = changed.clone();
            *f.saf.terminal_hook.lock().unwrap() = Some(Box::new(move || {
                let root = store
                    .get_provider_state(ROOT_DRIVER_NAMESPACE, execution.as_str())
                    .unwrap()
                    .unwrap();
                if serde_json::from_slice::<serde_json::Value>(&root.payload).unwrap()["phase"] != 1
                    || flag.swap(true, Ordering::SeqCst)
                {
                    return;
                }
                pending(&*store, 1);
                match mode {
                    0 => saf.deny.store(true, Ordering::SeqCst),
                    1 => probe.request(),
                    _ => store
                        .put_provider_state(
                            ProviderStateRecord {
                                namespace: "test-put-terminal-race".into(),
                                key: "once".into(),
                                version: 1,
                                payload: vec![7],
                            },
                            None,
                        )
                        .unwrap(),
                };
            }));
            unknown(
                f.router
                    .execute_native_mq_root(&f.mq, &original, "MQROOT")
                    .unwrap(),
            );
            assert!(changed.load(Ordering::SeqCst));
            pending(&*f.store, 1);
            assert_eq!(occurrences(&f), 3);
            assert_eq!(
                f.store
                    .get_execution(&original.execution_id)
                    .unwrap()
                    .unwrap()
                    .state,
                ExecutionState::Running
            );
            no_retry(&f, &original);
            if sqlite {
                let reopened =
                    mainframe_env_store::SqliteStateStore::open(&f.url, 64 << 20, 65536).unwrap();
                pending(&reopened, 1);
                assert_eq!(
                    reopened.list_provider_state_prefix("mq-", 4096).unwrap(),
                    f.rows()
                );
            }
        }
    }
}

#[test]
fn original_put_audit_saturation_rolls_back_all_message_unit_and_receipt_rows() {
    for sqlite in [false, true] {
        let f =
            Fixture::native_points_quota(sqlite, include_bytes!("../native_rich_md1.json"), 128);
        f.install("MQROOT", &source(1, false, false));
        let original = super::super::native_root::original(&f);
        let store = f.store.clone();
        let execution = original.execution_id.clone();
        let captured = Arc::new(Mutex::new(vec![]));
        let target = captured.clone();
        let once = Arc::new(AtomicBool::new(false));
        let flag = once.clone();
        *f.saf.terminal_hook.lock().unwrap() = Some(Box::new(move || {
            if store
                .list_provider_state("mq-selected-v1-occurrence", 128)
                .unwrap()
                .len()
                != 2
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
            let mut filled = 0;
            for _ in 0..=128 {
                if store.record_audit(audit.clone()).is_err() {
                    break;
                }
                filled += 1;
            }
            assert!(filled > 0);
            *target.lock().unwrap() = store.list_provider_state_prefix("mq-", 4096).unwrap();
        }));
        unknown(
            f.router
                .execute_native_mq_root(&f.mq, &original, "MQROOT")
                .unwrap(),
        );
        assert_eq!(f.rows(), *captured.lock().unwrap());
        assert_eq!(occurrences(&f), 2);
        assert!(
            f.store
                .list_provider_state("mq-delivery-live-v1-pending", 128)
                .unwrap()
                .is_empty()
        );
        no_retry(&f, &original);
    }
}

#[test]
fn malformed_complete_put_and_put1_profiles_refuse_before_dispatch() {
    for sqlite in [false, true] {
        for one in [false, true] {
            for mode in 0..5 {
                let f = native(sqlite, 1);
                let text = source(1, one, false);
                let text = match mode {
                    0 => text.replace("VALUE 147458.", "VALUE 0."),
                    1 => text.replace("VALUE 5.", "VALUE 2049."),
                    2 => text.replace("VALUE 'ABCDEFGHIJKLMNOPQRSTUVWX'", "VALUE LOW-VALUES"),
                    3 => text.replace(
                        "MD-PRIORITY PIC S9(9) BINARY VALUE 0",
                        "MD-PRIORITY PIC S9(9) BINARY VALUE -1",
                    ),
                    _ => text.replace(
                        "MD-VERSION PIC S9(9) BINARY VALUE 1",
                        "MD-VERSION PIC S9(9) BINARY VALUE 2",
                    ),
                };
                f.install("MQROOT", &text);
                let original = super::super::native_root::original(&f);
                unknown(
                    f.router
                        .execute_native_mq_root(&f.mq, &original, "MQROOT")
                        .unwrap(),
                );
                assert_eq!(occurrences(&f), if one { 1 } else { 2 });
                assert!(
                    f.store
                        .list_provider_state("mq-delivery-live-v1-pending", 128)
                        .unwrap()
                        .is_empty()
                );
                no_retry(&f, &original);
            }
        }
    }
}

#[test]
fn real_retired_object_wrong_put1_lookup_and_dropped_child_never_supply_put_authority() {
    for sqlite in [false, true] {
        for mode in 0..3 {
            let f = native(sqlite, 1);
            let text = source(1, mode == 1, false);
            let text = match mode {
                0 => text.replace(
                    "CALL 'MQPUT' USING",
                    "CALL 'MQCLOSE' USING HCONN HOBJ CLOSE-OPTIONS CC REASON.\nCALL 'MQPUT' USING",
                ),
                1 => text.replace("VALUE 'Q'", "VALUE 'MISSING.Q'"),
                _ => text.replace("PROGRAM-ID. MQROOT", "PROGRAM-ID. MQLEAF"),
            };
            if mode == 2 {
                f.install("MQLEAF", &text);
                f.install("MQROOT","IDENTIFICATION DIVISION. PROGRAM-ID. MQROOT. PROCEDURE DIVISION. CALL 'MQLEAF'. GOBACK.");
                f.factory.drop_at_admission.store(true, Ordering::SeqCst);
            } else {
                f.install("MQROOT", &text);
            }
            let original = super::super::native_root::original(&f);
            unknown(
                f.router
                    .execute_native_mq_root(&f.mq, &original, "MQROOT")
                    .unwrap(),
            );
            assert_eq!(
                occurrences(&f),
                match mode {
                    0 => 3,
                    1 => 1,
                    _ => 0,
                }
            );
            assert!(!f.mq.originals.lock().unwrap().iter().any(|(_,e,_)|matches!(&e.request,HostRequest::MqMqi(host) if matches!(host.envelope.request,MqMqiRequest::FullPut{..}|MqMqiRequest::FullPutOne{..}))));
            assert!(
                f.store
                    .list_provider_state("mq-delivery-live-v1-pending", 128)
                    .unwrap()
                    .is_empty()
            );
            no_retry(&f, &original);
        }
    }
}
