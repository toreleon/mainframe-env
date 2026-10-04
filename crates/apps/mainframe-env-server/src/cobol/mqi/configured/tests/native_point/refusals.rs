//! Original compiled negative routes on physical backends, never fake replies.
use super::*;
use mainframe_env_execution_api::{ExecutionId, IdempotencyKey, RunUnitId};
use mainframe_env_host_api::EnterpriseResourceClass;
use mainframe_env_interpreter::MqMqiProgramFrame;
use std::sync::atomic::AtomicBool;

fn unknown(result: &ExecutionOutcome) {
    assert!(
        matches!(result, ExecutionOutcome::ProviderFailure(p) if p.has_unknown_outcome()),
        "{result:?}"
    );
}
fn no_second_publication(f: &Fixture, original: &Invocation) {
    let rows = f.rows();
    let count = f
        .store
        .list_provider_state("mq-selected-v1-occurrence", 128)
        .unwrap()
        .len();
    assert!(
        f.router
            .execute_native_mq_root(&f.mq, original, "MQROOT")
            .map_or(true, |outcome| matches!(
                outcome,
                ExecutionOutcome::ProviderFailure(_)
            ))
    );
    assert_eq!(f.rows(), rows);
    assert_eq!(
        f.store
            .list_provider_state("mq-selected-v1-occurrence", 128)
            .unwrap()
            .len(),
        count
    );
}

#[test]
fn installed_retired_alias_and_malformed_od_refuse_without_second_close_or_open() {
    for sqlite in [false, true] {
        for retired in [false, true] {
            let f = native(sqlite, 1);
            let source = if retired {
                include_str!("../point.cbl").replace(
                    "CALL 'MQDISC' USING HC CC RC.",
                    "CALL 'MQCLOSE' USING HC HO CO CC RC. CALL 'MQDISC' USING HC CC RC.",
                )
            } else {
                include_str!("../point.cbl").replace("VALUE 'OD  '", "VALUE 'BAD '")
            };
            f.install("MQROOT", &source);
            let original = super::super::native_root::original(&f);
            unknown(
                &f.router
                    .execute_native_mq_root(&f.mq, &original, "MQROOT")
                    .unwrap(),
            );
            assert_eq!(
                f.store
                    .list_provider_state("mq-selected-v1-occurrence", 128)
                    .unwrap()
                    .len(),
                if retired { 3 } else { 1 }
            );
            no_second_publication(&f, &original);
        }
    }
}

#[test]
fn actual_open_saf_denial_cancellation_and_catalog_cas_roll_back_without_object_or_audit() {
    for sqlite in [false, true] {
        for mode in 0..3 {
            let f = native(sqlite, 1);
            f.install("MQROOT", include_str!("../point.cbl"));
            let captured = Arc::new(Mutex::new(vec![]));
            let target = captured.clone();
            let store = f.store.clone();
            let probe = f.parent.cancellation_probe.clone().unwrap();
            let saf = f.saf.clone();
            *f.saf.queue_hook.lock().unwrap() = Some(Box::new(move || {
                match mode {
                    0 => saf.deny.store(true, Ordering::SeqCst),
                    1 => probe.request(),
                    _ => {
                        let mut row = store
                            .get_provider_state("mq-v1-object-catalog", "catalog")
                            .unwrap()
                            .unwrap();
                        let version = row.version;
                        row.version += 1;
                        store.put_provider_state(row, Some(version)).unwrap();
                    }
                }
                *target.lock().unwrap() = store.list_provider_state_prefix("mq-", 4096).unwrap();
            }));
            let original = super::super::native_root::original(&f);
            unknown(
                &f.router
                    .execute_native_mq_root(&f.mq, &original, "MQROOT")
                    .unwrap(),
            );
            assert_eq!(f.rows(), *captured.lock().unwrap());
            assert_eq!(
                f.store
                    .list_provider_state("mq-selected-v1-occurrence", 128)
                    .unwrap()
                    .len(),
                1
            );
            let audits = f
                .store
                .audit_subject_records(&original.execution_id, 128)
                .unwrap();
            assert!(!audits.iter().any(|a| matches!(a, AuditSubjectRecord::Effect(a) if a.effect_sequence == 2 && a.decision == AuditDecision::Success)));
            no_second_publication(&f, &original);
        }
    }
}

#[test]
fn late_clock_source_catalog_panic_and_expiry_retain_one_original_connection() {
    for sqlite in [false, true] {
        for mode in 0..3 {
            let f = native(sqlite, 1);
            f.install("MQROOT", include_str!("../point.cbl"));
            let store = f.store.clone();
            let mut once = false;
            *f.clock.1.lock().unwrap() = Some(Box::new(move || {
                if once
                    || store
                        .list_provider_state("mq-selected-v1-occurrence", 128)
                        .unwrap()
                        .is_empty()
                {
                    return;
                }
                once = true;
                if mode == 1 {
                    panic!("actual point clock callback panic");
                }
                if mode == 2 {
                    return;
                }
                let mut row = store
                    .get_provider_state("mq-v1-object-catalog", "catalog")
                    .unwrap()
                    .unwrap();
                let version = row.version;
                row.version += 1;
                store.put_provider_state(row, Some(version)).unwrap();
            }));
            // Expiry is triggered only after a genuine committed connection.
            if mode == 2 {
                let store = f.store.clone();
                let tick = f.clock.clone();
                *f.clock.1.lock().unwrap() = Some(Box::new(move || {
                    if !store
                        .list_provider_state("mq-selected-v1-occurrence", 128)
                        .unwrap()
                        .is_empty()
                    {
                        tick.0.store(1000, Ordering::SeqCst);
                    }
                }));
            }
            let original = super::super::native_root::original(&f);
            unknown(
                &f.router
                    .execute_native_mq_root(&f.mq, &original, "MQROOT")
                    .unwrap(),
            );
            assert_eq!(
                f.store
                    .list_provider_state("mq-selected-v1-occurrence", 128)
                    .unwrap()
                    .len(),
                1
            );
            assert!(
                !f.saf
                    .resources
                    .lock()
                    .unwrap()
                    .iter()
                    .any(|r| r.class == EnterpriseResourceClass::MqQueue)
            );
            no_second_publication(&f, &original);
        }
    }
}

#[test]
fn genuine_child_admission_drop_and_foreign_runtime_refuse_before_native_dispatch() {
    for sqlite in [false, true] {
        for foreign in [false, true] {
            let f = native(sqlite, 1);
            let other = native(sqlite, 1);
            f.install(
                "MQLEAF",
                &include_str!("../point.cbl").replace("PROGRAM-ID. MQROOT", "PROGRAM-ID. MQLEAF"),
            );
            f.install("MQROOT", "IDENTIFICATION DIVISION. PROGRAM-ID. MQROOT. PROCEDURE DIVISION. CALL 'MQLEAF'. GOBACK.");
            if foreign {
                *f.factory.override_host.lock().unwrap() = Some(other.mq.clone());
            } else {
                f.factory.drop_at_admission.store(true, Ordering::SeqCst);
            }
            let before = f.rows();
            let original = super::super::native_root::original(&f);
            unknown(
                &f.router
                    .execute_native_mq_root(&f.mq, &original, "MQROOT")
                    .unwrap(),
            );
            assert_eq!(f.rows(), before);
            assert!(f.saf.resources.lock().unwrap().is_empty());
            assert!(
                f.store
                    .list_provider_state("mq-selected-v1-occurrence", 128)
                    .unwrap()
                    .is_empty()
            );
            if !foreign {
                let child = f.factory.children.lock().unwrap()[0].clone();
                assert!(
                    f.factory.observations.lock().unwrap()[0]
                        .abi_scope(&child)
                        .is_err()
                );
            }
            no_second_publication(&f, &original);
        }
    }
}

#[test]
fn actual_open_audit_quota_rolls_back_rows_receipt_and_object_on_both_backends() {
    for sqlite in [false, true] {
        let f =
            Fixture::native_points_quota(sqlite, include_bytes!("../native_rich_md1.json"), 128);
        f.install("MQROOT", include_str!("../point.cbl"));
        let original = super::super::native_root::original(&f);
        let execution = original.execution_id.clone();
        let store = f.store.clone();
        let captured = Arc::new(Mutex::new(vec![]));
        let target = captured.clone();
        *f.saf.queue_hook.lock().unwrap() = Some(Box::new(move || {
            let audit = store
                .audit_subject_records(&execution, 128)
                .unwrap()
                .into_iter()
                .find_map(|a| match a {
                    AuditSubjectRecord::Effect(a) => Some(a),
                    _ => None,
                })
                .unwrap();
            // Actual finite backend saturation after original OPEN admission and
            // before its physical publication. Filling is test setup, not a permit.
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
            &f.router
                .execute_native_mq_root(&f.mq, &original, "MQROOT")
                .unwrap(),
        );
        assert_eq!(f.rows(), *captured.lock().unwrap());
        assert_eq!(
            f.store
                .list_provider_state("mq-selected-v1-occurrence", 128)
                .unwrap()
                .len(),
            1
        );
        assert!(!f.store.audit_subject_records(&original.execution_id, 128).unwrap().iter().any(|a| matches!(a, AuditSubjectRecord::Effect(a) if a.effect_sequence == 2 && a.decision == AuditDecision::Success)));
        no_second_publication(&f, &original);
    }
}

#[test]
fn genuine_source_topology_contention_refuses_without_callback_reentry_or_changes() {
    for sqlite in [false, true] {
        let f = native(sqlite, 1);
        f.install(
            "MQLEAF",
            &include_str!("../point.cbl").replace("PROGRAM-ID. MQROOT", "PROGRAM-ID. MQLEAF"),
        );
        f.install("MQROOT", "IDENTIFICATION DIVISION. PROGRAM-ID. MQROOT. PROCEDURE DIVISION. CALL 'MQLEAF'. GOBACK.");
        let host = f.mq.clone();
        let store = f.store.clone();
        *f.factory.hook.lock().unwrap() = Some(Box::new(move |proof| {
            let source = super::super::super::native_point::Source::new(store.clone(), 48);
            source.bind(&host).unwrap();
            let before = store.list_provider_state_prefix("mq-", 4096).unwrap();
            let topology = host.topology.lock().unwrap();
            assert_eq!(
                source.check_live(proof.parent()),
                Err(HostProblem::UnknownOutcome)
            );
            drop(topology);
            assert_eq!(
                store.list_provider_state_prefix("mq-", 4096).unwrap(),
                before
            );
            assert_eq!(source.check_live(proof.parent()), Ok(()));
        }));
        let original = super::super::native_root::original(&f);
        let result = f
            .router
            .execute_native_mq_root(&f.mq, &original, "MQROOT")
            .unwrap();
        assert!(
            matches!(result, ExecutionOutcome::Completed(_)),
            "{result:?}"
        );
    }
}

#[test]
fn physical_reopen_genuine_cold_connect_fences_old_native_points_and_aliases() {
    for sqlite in [false, true] {
        let f = native(sqlite, 1);
        f.install("MQROOT", include_str!("../point.cbl"));
        let store = f.store.clone();
        let url = f.url.clone();
        let advanced = Arc::new(AtomicBool::new(false));
        let mark = advanced.clone();
        let mut once = false;
        *f.clock.1.lock().unwrap() = Some(Box::new(move || {
            if once
                || store
                    .list_provider_state("mq-selected-v1-occurrence", 128)
                    .unwrap()
                    .is_empty()
            {
                return;
            }
            once = true;
            let physical: Arc<dyn PlatformStore> = if sqlite {
                Arc::new(
                    mainframe_env_store::SqliteStateStore::open(&url, 64 << 20, 65536).unwrap(),
                )
            } else {
                store.clone()
            };
            // The existing genuine compiled connection-only CALL publishes its
            // new physical incarnation. No control row/token/table is fabricated;
            // this cold CONNECT is not claimed as a native point positive.
            let mut cold = Fixture::same_store(physical, url.clone());
            cold.parent.execution_id =
                ExecutionId::new("cold-point-root", Default::default()).unwrap();
            cold.parent.run_unit_id = RunUnitId::new("cold-point-run", Default::default()).unwrap();
            cold.parent.idempotency_key =
                IdempotencyKey::new("cold-point-root-key", Default::default()).unwrap();
            cold.install("MQCOLD", "IDENTIFICATION DIVISION. PROGRAM-ID. MQCOLD. DATA DIVISION. WORKING-STORAGE SECTION. 01 QM PIC X(48) VALUE SPACES. 01 HC PIC S9(9) BINARY. 01 CC PIC S9(9) BINARY. 01 RC PIC S9(9) BINARY. LINKAGE SECTION. 01 OUTCOME PIC X(8). PROCEDURE DIVISION USING OUTCOME. MOVE 'DONE' TO OUTCOME. CALL 'MQCONN' USING QM HC CC RC. IF CC NOT = 0 OR RC NOT = 0 MOVE 'BAD' TO OUTCOME END-IF. GOBACK.");
            let mut effect = cold.call_effect(1, "MQCOLD", &[vec![b' '; 8]]);
            effect.idempotency_key =
                Some(IdempotencyKey::new("cold-point-original-call", Default::default()).unwrap());
            let (outcome, reply) = cold.run(effect);
            assert!(
                matches!(outcome, ExecutionOutcome::Completed(_)),
                "{outcome:?}"
            );
            assert_eq!(
                reply.outcome,
                Ok(mainframe_env_host_api::HostResult::Program(
                    mainframe_env_interpreter::encode_cobol_call_result(&[b"DONE    ".to_vec()])
                        .unwrap()
                ))
            );
            mark.store(true, Ordering::Release);
        }));
        let original = super::super::native_root::original(&f);
        unknown(
            &f.router
                .execute_native_mq_root(&f.mq, &original, "MQROOT")
                .unwrap(),
        );
        assert!(advanced.load(Ordering::Acquire));
        let receipts = f
            .store
            .list_provider_state("mq-selected-v1-occurrence", 128)
            .unwrap();
        assert_eq!(
            receipts.len(),
            2,
            "one old and one actual cold CONNECT only"
        );
        assert_eq!(receipts.iter().filter(|r| serde_json::from_slice::<serde_json::Value>(&r.payload).unwrap()["value"]["execution"] == original.execution_id.as_str()).count(), 1);
        assert!(
            !f.saf
                .resources
                .lock()
                .unwrap()
                .iter()
                .any(|r| r.class == EnterpriseResourceClass::MqQueue)
        );
        let map = f.mq.topology.lock().unwrap();
        let RootEntry::Retained { frame, .. } = map.roots.get(&original.execution_id).unwrap()
        else {
            panic!()
        };
        assert!(
            frame::Observation(frame.clone())
                .abi_scope(&original)
                .is_err()
        );
        drop(map);
        no_second_publication(&f, &original);
    }
}
