//! Supplied byte identifiers in genuine compiled artifacts, never seeded messages.
use super::*;
use std::sync::atomic::{AtomicBool, Ordering};

const BINARY: [u8; 24] = [
    0x00, 0xff, 0x20, 0x80, 0x01, 0x7f, 0x41, 0x00, 0xc1, 0xfe, 0x10, 0x90, 0x2e, 0x0d, 0x0a, 0x00,
    0x7e, 0x81, 0x11, 0x22, 0x33, 0xaa, 0x55, 0xff,
];
const HEX: &str = "X'00FF2080017F4100C1FE10902E0D0A007E81112233AA55FF'";

fn source(version: i32, one: bool, abnormal: bool, binary: bool) -> String {
    let mut s = if version == 1 {
        include_str!("supplied_correlation/put1.cbl")
    } else {
        include_str!("supplied_correlation/put2.cbl")
    }
    .to_owned();
    if binary {
        s = s
            .replace(
                "MD-CORRELID PIC X(24) VALUE LOW-VALUES.",
                &format!("MD-CORRELID PIC X(24) VALUE {HEX}."),
            )
            .replace(
                "EXPECTED-CORRELID PIC X(24) VALUE LOW-VALUES.",
                &format!("EXPECTED-CORRELID PIC X(24) VALUE {HEX}."),
            );
    }
    if one {
        s = s
            .replace(
                "CALL 'MQOPEN' USING BY REFERENCE HCONN OBJECT-DESC OPEN-OPTIONS HOBJ CC REASON.\n",
                "",
            )
            .replace(
                "CALL 'MQPUT' USING BY REFERENCE HCONN HOBJ",
                "CALL 'MQPUT1' USING BY REFERENCE HCONN OBJECT-DESC",
            );
    }
    if abnormal {
        // A failed source-side byte check must not disappear behind the ABEND.
        s = s
            .replace(" END-IF.", " GOBACK END-IF.")
            .replace("GOBACK.", "CALL 'CEE3ABD'. GOBACK.");
    }
    s
}

fn pending_observer(f: &Fixture, version: i32, correlation: [u8; 24]) -> Arc<AtomicBool> {
    let observed = Arc::new(AtomicBool::new(false));
    let flag = observed.clone();
    let store = f.store.clone();
    *f.saf.terminal_hook.lock().unwrap() = Some(Box::new(move || {
        if !store
            .list_provider_state("mq-delivery-live-v1-pending", 128)
            .unwrap()
            .is_empty()
        {
            pending_correlated(&*store, version, correlation);
            flag.store(true, Ordering::Release);
        }
    }));
    observed
}

fn root_cases(binary: bool) {
    let correlation = if binary { BINARY } else { [0; 24] };
    for sqlite in [false, true] {
        for version in [1, 2] {
            for one in [false, true] {
                for abnormal in [false, true] {
                    let f = native(sqlite, version);
                    let text = source(version, one, abnormal, binary);
                    assert!(text.contains(if binary {
                        HEX
                    } else {
                        "MD-CORRELID PIC X(24) VALUE LOW-VALUES."
                    }));
                    f.install("MQROOT", &text);
                    let original = super::super::native_root::original(&f);
                    let frozen = original.clone();
                    let observed = pending_observer(&f, version, correlation);
                    let outcome = f
                        .router
                        .execute_native_mq_root(&f.mq, &original, "MQROOT")
                        .unwrap();
                    assert_original_put_correlated(&f, &original, version, one, correlation);
                    assert!(
                        if abnormal {
                            matches!(outcome, ExecutionOutcome::Abend(_))
                        } else {
                            matches!(outcome, ExecutionOutcome::Completed(ref done) if done.output.bytes()==b"PUT-DONE\n")
                        },
                        "{outcome:?}"
                    );
                    assert_eq!(original, frozen);
                    assert!(
                        observed.load(Ordering::Acquire),
                        "actual pending PUT before root terminal"
                    );
                    assert_receipts(&f, &original, if one { 2 } else { 3 });
                    assert!(
                        f.saf
                            .resources
                            .lock()
                            .unwrap()
                            .iter()
                            .any(|r| r.class == EnterpriseResourceClass::MqQueue
                                && r.name.as_str() == "Q"
                                && r.intent == AccessIntent::Update)
                    );
                    terminal_correlated(&f, &original, version, abnormal, correlation);
                }
            }
        }
    }
}

#[test]
fn installed_zero_correlation_pending_put_and_put1_normal_or_cee3abd_md1_md2() {
    root_cases(false);
}

#[test]
fn installed_binary_correlation_pending_put_and_put1_normal_or_cee3abd_md1_md2() {
    root_cases(true);
}

#[test]
fn installed_same_task_child_preserves_supplied_zero_and_binary_correlation_and_call() {
    for sqlite in [false, true] {
        for version in [1, 2] {
            for one in [false, true] {
                for binary in [false, true] {
                    let correlation = if binary { BINARY } else { [0; 24] };
                    let f = native(sqlite, version);
                    f.install(
                        "MQLEAF",
                        &source(version, one, false, binary)
                            .replace("PROGRAM-ID. MQROOT", "PROGRAM-ID. MQLEAF"),
                    );
                    f.install("MQROOT", "IDENTIFICATION DIVISION. PROGRAM-ID. MQROOT. PROCEDURE DIVISION. CALL 'MQLEAF'. DISPLAY 'ROOT-DONE'. GOBACK.");
                    let original = super::super::native_root::original(&f);
                    let frozen = original.clone();
                    let call = Arc::new(Mutex::new(None));
                    let captured = call.clone();
                    *f.factory.hook.lock().unwrap() = Some(Box::new(move |proof| {
                        assert_eq!(
                            proof.core_intent().request_digest,
                            canonical_request_digest(&proof.original_call().request).unwrap()
                        );
                        *captured.lock().unwrap() = Some((
                            proof.original_call().clone(),
                            proof.call_reservation().clone(),
                        ));
                    }));
                    let observed = pending_observer(&f, version, correlation);
                    let outcome = f
                        .router
                        .execute_native_mq_root(&f.mq, &original, "MQROOT")
                        .unwrap();
                    assert!(
                        matches!(outcome,ExecutionOutcome::Completed(ref done) if done.output.bytes()==b"ROOT-DONE\n"),
                        "{outcome:?}"
                    );
                    assert_eq!(original, frozen);
                    assert!(observed.load(Ordering::Acquire));
                    let child = f.factory.children.lock().unwrap()[0].clone();
                    assert_eq!(
                        child.parent_execution_id,
                        Some(original.execution_id.clone())
                    );
                    assert_receipts(&f, &child, if one { 2 } else { 3 });
                    assert_original_put_correlated(&f, &child, version, one, correlation);
                    let (effect, reservation) = call.lock().unwrap().clone().unwrap();
                    assert_eq!(effect.run_unit, original.run_unit_id);
                    let core = f
                        .store
                        .effect(effect.idempotency_key.as_ref().unwrap())
                        .unwrap()
                        .unwrap();
                    assert_eq!(core.state, EffectState::Completed);
                    assert_eq!(core.execution_id, original.execution_id);
                    assert_eq!(core.intent.owner, original.execution_id);
                    assert_eq!(core.sequence, effect.sequence);
                    assert_eq!(
                        core.request_digest,
                        canonical_request_digest(&effect.request).unwrap()
                    );
                    assert!(core.result_digest.is_some());
                    let completed = f
                        .store
                        .get_provider_state(&reservation.namespace, &reservation.key)
                        .unwrap()
                        .unwrap();
                    let before: serde_json::Value =
                        serde_json::from_slice(&reservation.payload).unwrap();
                    let after: serde_json::Value =
                        serde_json::from_slice(&completed.payload).unwrap();
                    assert_eq!(before["fingerprint"], after["fingerprint"]);
                    assert_eq!(after["child_execution"], child.execution_id.as_str());
                    assert_eq!(after["owner_execution"], original.execution_id.as_str());
                    // Observe the actual retained CALL reply; never dispatch a
                    // manufactured reply or derive lifecycle permission from it.
                    let reply = &after["reply"];
                    let bytes: Vec<u8> = serde_json::from_value(reply["bytes"].clone()).unwrap();
                    let retained = BoundedPayload::new(
                        reply["schema"].as_str().unwrap(),
                        bytes,
                        InvocationLimits::default(),
                    )
                    .unwrap();
                    assert_eq!(
                        core.result_digest,
                        Some(canonical_result_digest(&Ok(HostResult::Program(retained))).unwrap())
                    );
                    let map = f.mq.topology.lock().unwrap();
                    let RootEntry::Retained { frame: root, .. } =
                        map.roots.get(&original.execution_id).unwrap()
                    else {
                        panic!()
                    };
                    let FrameEntry::Retained(child_frame) =
                        map.frames.get(&child.execution_id).unwrap()
                    else {
                        panic!()
                    };
                    assert!(Arc::ptr_eq(
                        &root.abi.as_ref().unwrap().scope,
                        &child_frame.abi.as_ref().unwrap().scope
                    ));
                    assert_eq!(child_frame.original(), &child);
                    assert!(child_frame.check_original(&child).is_err());
                    drop(map);
                    terminal_correlated(&f, &original, version, false, correlation);
                }
            }
        }
    }
}

#[test]
fn installed_generation_requests_and_zero_msgid_remain_refused_before_put_publication() {
    for sqlite in [false, true] {
        for version in [1, 2] {
            for one in [false, true] {
                for mode in 0..3 {
                    let f = native(sqlite, version);
                    let s = source(version, one, false, false);
                    let text = match mode {
                        0 => s.replace("VALUE 147458.", "VALUE 147522."), // NEW_MSG_ID64
                        1 => s.replace("VALUE 147458.", "VALUE 147586."), // NEW_CORREL_ID128
                        _ => s.replace("VALUE 'ABCDEFGHIJKLMNOPQRSTUVWX'", "VALUE LOW-VALUES"),
                    };
                    f.install("MQROOT", &text);
                    let original = super::super::native_root::original(&f);
                    let outcome = f
                        .router
                        .execute_native_mq_root(&f.mq, &original, "MQROOT")
                        .unwrap();
                    assert!(
                        matches!(outcome,ExecutionOutcome::ProviderFailure(ref p) if p.has_unknown_outcome()),
                        "{outcome:?}"
                    );
                    // Earlier genuine CONN/OPEN/core/audits are expected, not whole-store absence.
                    assert_receipts(&f, &original, if one { 1 } else { 2 });
                    assert!(!f.mq.originals.lock().unwrap().iter().any(|(_,e,_)| matches!(&e.request,HostRequest::MqMqi(h) if matches!(h.envelope.request,MqMqiRequest::FullPut{..}|MqMqiRequest::FullPutOne{..}))));
                    assert!(
                        f.store
                            .list_provider_state("mq-delivery-live-v1-pending", 128)
                            .unwrap()
                            .is_empty()
                    );
                    let queues = f
                        .store
                        .list_provider_state("mq-delivery-live-v1-queue", 128)
                        .unwrap();
                    assert_eq!(queues.len(), 1);
                    assert_eq!(value(&queues[0])["messages"], serde_json::json!([]));
                    let frozen = f.rows();
                    assert!(
                        matches!(f.router.execute_native_mq_root(&f.mq,&original,"MQROOT"), Ok(ExecutionOutcome::ProviderFailure(ref p)) if p.has_unknown_outcome())
                    );
                    assert_eq!(f.rows(), frozen);
                }
            }
        }
    }
}
