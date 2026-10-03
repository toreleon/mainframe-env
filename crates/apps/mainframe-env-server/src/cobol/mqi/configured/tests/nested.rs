//! Genuine compiled nested CALL, original core/CALL proof and selected backend.
use super::setup::*;
use super::*;
use mainframe_env_execution_api::*;
use mainframe_env_host_api::*;
use mainframe_env_store_api::*;

fn linked_source(name: &str) -> String {
    SOURCE
        .replace("PROGRAM-ID. MQFLOW", &format!("PROGRAM-ID. {name}"))
        .replace("PROCEDURE DIVISION.", "LINKAGE SECTION. 01 OUTCOME PIC X(8). PROCEDURE DIVISION USING OUTCOME. MOVE 'DONE' TO OUTCOME.")
        .replace("DISPLAY 'BAD-CONN'", "MOVE 'BAD' TO OUTCOME")
        .replace("DISPLAY 'BAD-WARNING'", "MOVE 'BAD' TO OUTCOME")
        .replace("DISPLAY 'BAD-CMIT'", "MOVE 'BAD' TO OUTCOME")
        .replace("DISPLAY 'BAD-BACK'", "MOVE 'BAD' TO OUTCOME")
        .replace("DISPLAY 'BAD-DISC'", "MOVE 'BAD' TO OUTCOME")
        .replace("DISPLAY 'DONE'.", "")
}
const MIDDLE: &str = "IDENTIFICATION DIVISION. PROGRAM-ID. MQMIDDLE. DATA DIVISION. LINKAGE SECTION. 01 OUTCOME PIC X(8). PROCEDURE DIVISION USING OUTCOME. CALL 'MQLEAF' USING OUTCOME. GOBACK.";

#[test]
fn physical_reopen_new_compiled_root_incarnation_fences_original_live_route_and_retry() {
    for sqlite in [false, true] {
        let f = Fixture::new(sqlite);
        f.install("MQFLOW", SOURCE);
        let store = f.store.clone();
        let url = f.url.clone();
        let advanced = Arc::new(Mutex::new(false));
        let marker = advanced.clone();
        let mut once = false;
        *f.clock.1.lock().unwrap() = Some(Box::new(move || {
            if once
                || store
                    .list_provider_state("mq-selected-v1-occurrence", 100)
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
            // Strict existing-state open on actual committed control. A real
            // compiled/coordinated different root publishes the cold incarnation;
            // setup does not reset counters or fabricate a control row.
            let mut cold = Fixture::same_store(physical, url.clone());
            cold.parent.execution_id = ExecutionId::new("cold-root", Default::default()).unwrap();
            cold.parent.run_unit_id = RunUnitId::new("cold-run", Default::default()).unwrap();
            cold.parent.idempotency_key =
                IdempotencyKey::new("cold-root-key", Default::default()).unwrap();
            cold.install("MQCOLD", "IDENTIFICATION DIVISION. PROGRAM-ID. MQCOLD. DATA DIVISION. WORKING-STORAGE SECTION. 01 QM PIC X(48) VALUE SPACES. 01 HC PIC S9(9) BINARY. 01 CC PIC S9(9) BINARY. 01 RC PIC S9(9) BINARY. LINKAGE SECTION. 01 OUTCOME PIC X(8). PROCEDURE DIVISION USING OUTCOME. MOVE 'DONE' TO OUTCOME. CALL 'MQCONN' USING QM HC CC RC. IF CC NOT = 0 OR RC NOT = 0 MOVE 'BAD' TO OUTCOME END-IF. GOBACK.");
            let mut effect = cold.call_effect(1, "MQCOLD", &[vec![b' '; 8]]);
            effect.idempotency_key =
                Some(IdempotencyKey::new("cold-original-call", Default::default()).unwrap());
            let (outcome, reply) = cold.run(effect);
            assert!(
                matches!(outcome, ExecutionOutcome::Completed(_)),
                "{outcome:?}"
            );
            assert_eq!(
                reply.outcome,
                Ok(HostResult::Program(
                    mainframe_env_interpreter::encode_cobol_call_result(&[b"DONE    ".to_vec()])
                        .unwrap()
                ))
            );
            *marker.lock().unwrap() = true;
        }));
        let original = f.effect(1, "MQFLOW");
        let (_, reply) = f.run_raw(original.clone());
        assert!(reply.is_none_or(|r| r.outcome.is_err()));
        assert!(*advanced.lock().unwrap());
        let children = f.factory.children.lock().unwrap().clone();
        assert_eq!(children.len(), 1);
        let receipts = f
            .store
            .list_provider_state("mq-selected-v1-occurrence", 100)
            .unwrap();
        assert_eq!(
            receipts.len(),
            2,
            "one old CONNECT and one genuinely cold CONNECT"
        );
        let old_count = receipts
            .iter()
            .filter(|r| {
                let v: serde_json::Value = serde_json::from_slice(&r.payload).unwrap();
                v["value"]["execution"] == children[0].execution_id.as_str()
            })
            .count();
        assert_eq!(old_count, 1);
        let before = f.rows();
        let saf = f.saf.resources.lock().unwrap().len();
        assert!(f.router.invoke(&f.parent, original).outcome.is_err());
        assert_eq!(f.rows(), before);
        assert_eq!(f.saf.resources.lock().unwrap().len(), saf);
        assert!(
            f.factory.observations.lock().unwrap()[0]
                .profile(&children[0])
                .is_err()
        );
    }
}

#[test]
fn actual_nested_original_core_call_catalog_controls_saf_and_late_cas_fail_closed() {
    for sqlite in [false, true] {
        for case in 0..7 {
            let f = Fixture::new(sqlite);
            f.install("MQLEAF", &linked_source("MQLEAF"));
            f.install("MQMIDDLE", MIDDLE);
            let before = f.rows();
            if case < 3 {
                let store = f.store.clone();
                *f.factory.nested_hook.lock().unwrap() = Some(Box::new(move |proof| {
                    assert!(proof.parent().parent_execution_id.is_some());
                    if case == 0 {
                        let mut core = proof.core_intent().clone();
                        core.state = EffectState::Completed;
                        core.result_digest =
                            Some(canonical_result_digest(&Err(HostProblem::Unauthorized)).unwrap());
                        core.resolved_tick = Some(20);
                        store.record_result(&core.key.clone(), core).unwrap();
                    } else {
                        let mut row = if case == 1 {
                            proof.call_reservation().clone()
                        } else {
                            proof.catalog_record().unwrap().clone()
                        };
                        let version = row.version;
                        row.version += 1;
                        store.put_provider_state(row, Some(version)).unwrap();
                    }
                }));
            } else if case == 3 {
                f.saf.deny.store(true, std::sync::atomic::Ordering::SeqCst);
            } else if case == 4 {
                let probe = f.parent.cancellation_probe.clone().unwrap();
                *f.saf.hook.lock().unwrap() = Some(Box::new(move || probe.request()));
            } else if case == 5 {
                let store = f.store.clone();
                *f.saf.hook.lock().unwrap() = Some(Box::new(move || {
                    let mut row = store
                        .get_provider_state("mq-v1-object-catalog", "catalog")
                        .unwrap()
                        .unwrap();
                    let version = row.version;
                    row.version += 1;
                    store.put_provider_state(row, Some(version)).unwrap();
                }));
            } else {
                let store = f.store.clone();
                let probe = f.parent.cancellation_probe.clone().unwrap();
                *f.clock.1.lock().unwrap() = Some(Box::new(move || {
                    if !store
                        .list_provider_state("mq-selected-v1-occurrence", 100)
                        .unwrap()
                        .is_empty()
                    {
                        probe.request();
                    }
                }));
            }
            let original = f.call_effect(1, "MQMIDDLE", &[vec![b' '; 8]]);
            let (_, reply) = f.run_raw(original.clone());
            assert!(reply.is_none_or(|r| r.outcome.is_err()), "case {case}");
            let children = f.factory.children.lock().unwrap().clone();
            assert_eq!(children.len(), 2, "case {case}");
            let receipts = f
                .store
                .list_provider_state("mq-selected-v1-occurrence", 100)
                .unwrap();
            assert_eq!(receipts.len(), usize::from(case == 6), "case {case}");
            if case != 5 && case != 6 {
                assert_eq!(f.rows(), before);
            }
            if case < 3 {
                assert!(f.saf.resources.lock().unwrap().is_empty());
            }
            if case == 5 {
                let audits = f
                    .store
                    .audit_records(&children[1].execution_id, 0, 100)
                    .unwrap();
                assert!(audits.iter().all(|a| a.decision != AuditDecision::Success));
            }
            for (observation, child) in f.factory.observations.lock().unwrap().iter().zip(&children)
            {
                assert!(observation.profile(child).is_err());
            }
            let retained = f.rows();
            assert!(f.router.invoke(&f.parent, original).outcome.is_err());
            assert_eq!(f.rows(), retained);
            assert_eq!(f.factory.children.lock().unwrap().len(), 2);
        }
    }
}

#[test]
fn genuinely_compiled_nested_call_uses_retained_parent_frame_original_core_and_two_audits() {
    for sqlite in [false, true] {
        let f = Fixture::new(sqlite);
        f.install("MQLEAF", &linked_source("MQLEAF"));
        f.install("MQMIDDLE", MIDDLE);
        let original = f.call_effect(1, "MQMIDDLE", &[vec![b' '; 8]]);
        let immutable = f.parent.clone();
        let (outcome, reply) = f.run(original.clone());
        assert!(
            matches!(outcome, ExecutionOutcome::Completed(_)),
            "{outcome:?}"
        );
        assert_eq!(
            reply.outcome,
            Ok(HostResult::Program(
                mainframe_env_interpreter::encode_cobol_call_result(&[b"DONE    ".to_vec()])
                    .unwrap()
            ))
        );
        assert_eq!(f.parent, immutable);
        let children = f.factory.children.lock().unwrap().clone();
        assert_eq!(children.len(), 2);
        assert_eq!(
            children[0].parent_execution_id.as_ref(),
            Some(&f.parent.execution_id)
        );
        assert_eq!(
            children[1].parent_execution_id.as_ref(),
            Some(&children[0].execution_id)
        );
        let receipts = f
            .store
            .list_provider_state("mq-selected-v1-occurrence", 100)
            .unwrap();
        assert_eq!(receipts.len(), 5);
        for row in &receipts {
            let stored: serde_json::Value = serde_json::from_slice(&row.payload).unwrap();
            let value = &stored["value"];
            assert_eq!(value["execution"], children[1].execution_id.as_str());
            let key =
                IdempotencyKey::new(value["key"].as_str().unwrap(), Default::default()).unwrap();
            let core = f.store.effect(&key).unwrap().unwrap();
            assert_eq!(core.execution_id, children[1].execution_id);
            assert_eq!(core.state, EffectState::Completed);
            assert_eq!(
                serde_json::to_value(core.request_digest).unwrap(),
                value["request_digest"]
            );
        }
        let audit = f
            .store
            .audit_records(&children[1].execution_id, 0, 100)
            .unwrap();
        assert_eq!(audit.len(), 10);
        for sequence in 1..=5 {
            assert_eq!(
                audit
                    .iter()
                    .filter(
                        |a| a.effect_sequence == sequence && a.decision == AuditDecision::Success
                    )
                    .count(),
                2
            );
        }
        let notifications = f.store.pending_notifications(128).unwrap();
        for original_actor in [&f.parent, &children[0], &children[1]] {
            let execution = &original_actor.execution_id;
            assert_eq!(
                f.store.get_execution(execution).unwrap().unwrap().state,
                ExecutionState::Completed
            );
            let events = f.store.events(execution, 0, 128).unwrap();
            assert!(events.iter().any(|event| matches!(
                event.kind,
                LifecycleEventKind::Completed { return_code: 0 }
            )));
            for event in events {
                assert_eq!(
                    notifications
                        .iter()
                        .filter(|n| n.execution_id == *execution && n.sequence == event.sequence)
                        .count(),
                    1
                );
            }
        }
        for (observation, child) in f.factory.observations.lock().unwrap().iter().zip(&children) {
            assert!(observation.profile(child).is_err());
        }
        let before = f.rows();
        assert!(f.router.invoke(&f.parent, original).outcome.is_ok());
        assert_eq!(f.rows(), before);
        assert_eq!(f.factory.children.lock().unwrap().len(), 2);
        assert_eq!(f.store.pending_notifications(128).unwrap(), notifications);
        if sqlite {
            let reopened =
                mainframe_env_store::SqliteStateStore::open(&f.url, 64 << 20, 65536).unwrap();
            assert_eq!(
                reopened
                    .list_provider_state("mq-selected-v1-occurrence", 100)
                    .unwrap(),
                receipts
            );
            assert_eq!(
                reopened
                    .audit_records(&children[1].execution_id, 0, 100)
                    .unwrap(),
                audit
            );
            assert_eq!(reopened.pending_notifications(128).unwrap(), notifications);
        }
    }
}
