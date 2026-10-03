//! Genuine installed child PUT production and original qualified GET under one root.
use super::native_point::{assert_receipts, native};
use super::setup::*;
use super::*;
use mainframe_env_execution_api::*;
use mainframe_env_host_api::mq_md_value::*;
use mainframe_env_host_api::mq_mqi::*;
use mainframe_env_host_api::*;
use mainframe_env_store_api::*;
use std::sync::atomic::{AtomicBool, Ordering};

mod refusals;

fn value(row: &ProviderStateRecord) -> serde_json::Value {
    serde_json::from_slice::<serde_json::Value>(&row.payload).unwrap()["value"].clone()
}
fn original(f: &Fixture, program: &str, label: &str) -> Invocation {
    let admitted = f
        .router
        .cobol
        .preflight_installed_program(program, true)
        .unwrap();
    let v = &f.parent;
    let limits = InvocationLimits::default();
    let mut i = Invocation::new(
        v.request_id.clone(),
        ExecutionId::new(label, limits).unwrap(),
        RunUnitId::new(format!("{label}-run"), limits).unwrap(),
        None,
        Selector::new(format!("program:{program}"), limits).unwrap(),
        admitted.artifact,
        v.principal.clone(),
        v.service_class,
        v.priority,
        v.deadline_tick,
        v.trace_id.clone(),
        IdempotencyKey::new(format!("{label}-key"), limits).unwrap(),
        v.attempt,
        v.limits,
        v.bindings.clone(),
        limits,
    )
    .unwrap()
    .with_provider_generations(v.provider_generations.clone(), limits)
    .unwrap();
    i.cancellation_probe = Some(CancellationProbe::new());
    crate::cobol::bind_compatible_runtime_services(&mut i).unwrap();
    i
}
fn source(version: i32, case: usize, abnormal: bool, count: i32) -> String {
    let mut s = if version == 1 {
        include_str!("full_get/get1.cbl")
    } else {
        include_str!("full_get/get2.cbl")
    }
    .to_owned();
    let (cc, rc, body, length, qname, backout, id, context) = match case {
        0 => (
            0,
            0,
            "HELLOTAIL",
            5,
            "Q",
            count,
            "ABCDEFGHIJKLMNOPQRSTUVWX",
            "SPACES",
        ),
        1 => (
            1,
            2079,
            "HELxxTAIL",
            5,
            "Q",
            count,
            "ABCDEFGHIJKLMNOPQRSTUVWX",
            "SPACES",
        ),
        2 => (
            1,
            2080,
            "HELxxTAIL",
            5,
            "INPUT-Q",
            count,
            "ABCDEFGHIJKLMNOPQRSTUVWX",
            "SPACES",
        ),
        3 => (
            2,
            2033,
            "xxxxxTAIL",
            29,
            "INPUT-Q",
            73,
            "LOW-VALUES",
            "'INPUT'",
        ),
        _ => panic!(),
    };
    s = s.replace(
        "GMO-OPTIONS PIC S9(9) BINARY VALUE 2.",
        &format!(
            "GMO-OPTIONS PIC S9(9) BINARY VALUE {}.",
            if case == 1 { 66 } else { 2 }
        ),
    );
    if case == 1 || case == 2 {
        s = s.replace(
            "BUFFER-LENGTH PIC S9(9) BINARY VALUE 5.",
            "BUFFER-LENGTH PIC S9(9) BINARY VALUE 3.",
        );
    }
    let msg_id = if case == 3 {
        "EXPECTED-EMPTY-ID".into()
    } else {
        format!("'{id}'")
    };
    let correlation = if case == 3 {
        "EXPECTED-EMPTY-ID"
    } else {
        "'ZYXWVUTSRQPONMLKJIHGFEDC'"
    };
    let checks = format!(
        "IF CC NOT = {cc} OR REASON NOT = {rc} DISPLAY 'BAD-STATUS' END-IF.\n\
        IF DATA-LENGTH NOT = {length} OR MESSAGE-BUFFER NOT = '{body}' DISPLAY 'BAD-BODY' END-IF.\n\
        IF GMO-RESOLVEDQNAME NOT = '{qname}' DISPLAY 'BAD-QNAME' END-IF.\n\
        IF MD-MSGID NOT = {msg_id} OR MD-CORRELID NOT = {correlation} DISPLAY 'BAD-IDS' END-IF.\n\
        IF MD-BACKOUTCOUNT NOT = {backout} OR MD-USERIDENTIFIER NOT = {context} DISPLAY 'BAD-MD' END-IF.\n\
        IF GMO-SIGNAL1 NOT = 73 OR GMO-SIGNAL2 NOT = -77 DISPLAY 'BAD-SIGNALS' END-IF.\n\
        IF MD-SUFFIX NOT = 'TAIL0123' OR GMO-SUFFIX NOT = 'TAIL0123' OR OD-SUFFIX NOT = 'TAIL0123' DISPLAY 'BAD-SUFFIX' END-IF.\n\
        DISPLAY 'GET-DONE'.\n"
    );
    // A bad compiled storage observation must not disappear into an ABEND.
    let checks = if abnormal {
        checks.replace(" END-IF.", " GOBACK END-IF.")
    } else {
        checks
    };
    if case != 3 {
        s = s.replace(
            "PROCEDURE DIVISION.\n",
            "PROCEDURE DIVISION.\nCALL 'MQPUTER'.\n",
        );
    }
    s.replace(
        "GOBACK.",
        &(checks
            + if abnormal {
                "CALL 'CEE3ABD'. GOBACK."
            } else {
                "GOBACK."
            }),
    )
}
fn produce(f: &Fixture, version: i32, one: bool) {
    // Actual separately compiled child, explicit original local MQCMIT. No
    // completed root scope is recycled, and no queued/pending message is seeded.
    let mut text = super::full_put::source(version, one, false)
        .replace("PROGRAM-ID. MQROOT", "PROGRAM-ID. MQPUTER");
    let mut finish = "CALL 'MQCMIT' USING BY REFERENCE HCONN CC REASON.\nIF CC NOT = 0 OR REASON NOT = 0 DISPLAY 'BAD-COMMIT' END-IF.\n".to_owned();
    if !one {
        finish += "CALL 'MQCLOSE' USING BY REFERENCE HCONN HOBJ CLOSE-OPTIONS CC REASON.\nIF CC NOT = 0 OR REASON NOT = 0 DISPLAY 'BAD-CLOSE' END-IF.\n";
    }
    finish += "GOBACK.";
    text = text.replace("GOBACK.", &finish);
    f.install("MQPUTER", &text);
}
fn message(entry: &serde_json::Value, version: i32, count: i32) {
    assert_eq!(entry["persistent"], true);
    assert_eq!(entry["expires_at"], serde_json::Value::Null);
    assert_eq!(entry["message"]["kind"], "complete");
    let m = &entry["message"]["value"];
    let raw: Vec<u8> = serde_json::from_value(m["md"].clone()).unwrap();
    let mut expected = super::full_put::expected_md(version, true);
    match &mut expected {
        MqMdValue::V1 { fields, .. } | MqMdValue::V2 { fields, .. } => fields.backout_count = count,
    };
    assert_eq!(mq_md_value_decode(&raw, 2048).unwrap(), expected);
    assert_eq!(
        serde_json::from_value::<Vec<u8>>(m["body"].clone()).unwrap(),
        b"HELLO"
    );
    assert_eq!(m["properties"], serde_json::json!([]));
}
fn queue(f: &Fixture, version: i32, count: Option<i32>) {
    let rows = f
        .store
        .list_provider_state("mq-delivery-live-v1-queue", 128)
        .unwrap();
    assert_eq!(rows.len(), 1);
    let q = value(&rows[0]);
    let entries = q["messages"].as_array().unwrap();
    assert_eq!(entries.len(), usize::from(count.is_some()));
    if let Some(n) = count {
        message(&entries[0], version, n);
    }
}
fn pending(store: &dyn PlatformStore, version: i32) {
    let rows = store
        .list_provider_state("mq-delivery-live-v1-pending", 128)
        .unwrap();
    assert_eq!(rows.len(), 1);
    let unit = value(&rows[0]);
    let ops = unit["operations"].as_array().unwrap();
    assert_eq!(ops.len(), 1);
    assert_eq!(ops[0]["queue"], "Q");
    assert_eq!(ops[0]["put"], false);
    message(&ops[0]["entry"], version, 0);
    let rows = store
        .list_provider_state("mq-delivery-live-v1-queue", 128)
        .unwrap();
    assert_eq!(value(&rows[0])["messages"], serde_json::json!([]));
}
fn removed_pending(store: &dyn PlatformStore) -> bool {
    store
        .list_provider_state("mq-delivery-live-v1-pending", 128)
        .unwrap()
        .iter()
        .any(|r| {
            value(r)["operations"]
                .as_array()
                .unwrap()
                .iter()
                .any(|op| op["put"] == false)
        })
}
fn assert_original(
    f: &Fixture,
    actor: &Invocation,
    root: &Invocation,
    version: i32,
    case: usize,
    count: i32,
) {
    let originals = f.mq.originals.lock().unwrap();
    let (i, e, reply) = originals
        .iter()
        .filter(|(i, e, _)| {
            i == actor
                && matches!(&e.request,
        HostRequest::MqMqi(h) if matches!(h.envelope.request,MqMqiRequest::QualifiedFullGet(_)))
        })
        .nth(count as usize)
        .unwrap();
    assert_eq!(i, actor);
    assert_eq!(e.run_unit, actor.run_unit_id);
    assert_eq!(e.deadline_tick, actor.deadline_tick);
    let occurrence = e.mq_mqi_occurrence(Default::default()).unwrap().unwrap();
    let MqMqiRequest::QualifiedFullGet(g) = &occurrence.envelope().request else {
        panic!()
    };
    assert!(matches!(g.connection, MqHconn::Issued(_)));
    assert_eq!(
        g.buffer_capacity,
        if case == 1 || case == 2 { 3 } else { 5 }
    );
    assert_eq!(g.descriptor.version(), version);
    assert_eq!(g.descriptor.fields().msg_id, [0; 24]);
    assert_eq!(g.descriptor.fields().correl_id, [0; 24]);
    assert_eq!(g.descriptor.fields().backout_count, 73);
    assert_eq!(g.mode, MqGetMode::Remove);
    assert_eq!(g.wait, MqWait::NoWait);
    assert_eq!(
        g.truncation,
        if case == 1 {
            MqTruncation::Accept
        } else {
            MqTruncation::Reject
        }
    );
    assert_eq!(g.options, MqMqiOptions::ContractDefault);
    assert_eq!(g.message_handle, None);
    let owner = f
        .store
        .list_provider_state("mq-selected-v1-uow-owner", 128)
        .unwrap()
        .into_iter()
        .find(|r| matches!(g.unit,MqMqiUnitOfWork::Local{unit} if value(r)["unit"]==unit))
        .unwrap();
    assert_eq!(value(&owner)["execution"], root.execution_id.as_str());
    assert_eq!(
        g.unit,
        MqMqiUnitOfWork::Local {
            unit: value(&owner)["unit"].as_u64().unwrap()
        }
    );
    let core = f
        .store
        .effect(e.idempotency_key.as_ref().unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(core.execution_id, actor.execution_id);
    assert_eq!(core.intent.owner, actor.execution_id);
    assert_eq!(core.sequence, e.sequence);
    assert_eq!(
        core.request_digest,
        canonical_request_digest(&e.request).unwrap()
    );
    assert_eq!(
        core.result_digest,
        Some(canonical_result_digest(&reply.outcome).unwrap())
    );
    assert_eq!(
        core.intent.audit_invocation_key.as_ref(),
        Some(&actor.idempotency_key)
    );
    let Ok(HostResult::MqMqi(h)) = &reply.outcome else {
        panic!("actual typed output")
    };
    let MqMqiOutcome::ReviewedOutput {
        status,
        output: MqMqiOutput::QualifiedFullGot(got),
    } = &h.result.outcome
    else {
        panic!("request-bound observed result")
    };
    let (pair, disposition) = match case {
        0 => (
            (0, 0),
            MqGetDisposition::Message(MqTruncationDisposition::Complete { length: 5 }),
        ),
        1 => (
            (1, 2079),
            MqGetDisposition::Message(MqTruncationDisposition::AcceptedRemoved {
                required: 5,
                copied: 3,
            }),
        ),
        2 => (
            (1, 2080),
            MqGetDisposition::Message(MqTruncationDisposition::RejectedRetained {
                required: 5,
                copied: 3,
            }),
        ),
        3 => ((2, 2033), MqGetDisposition::NoMessage),
        _ => panic!(),
    };
    assert_eq!(status.wire_pair(), pair);
    assert_eq!(got.disposition, disposition);
    assert_eq!(got.data_length, if case == 3 { None } else { Some(5) });
    assert_eq!(got.cursor, None);
    let mut q = [b' '; 48];
    q[0] = b'Q';
    assert_eq!(got.resolved_queue, if case < 2 { Some(q) } else { None });
    if case == 3 {
        assert_eq!(got.message, None);
    } else {
        let mut md = super::full_put::expected_md(version, true);
        match &mut md {
            MqMdValue::V1 { fields, .. } | MqMdValue::V2 { fields, .. } => {
                fields.backout_count = count
            }
        };
        assert_eq!(
            got.message,
            Some(MqFullMessage {
                descriptor: md,
                body: if case == 0 {
                    b"HELLO".to_vec()
                } else {
                    b"HEL".to_vec()
                },
                properties: vec![]
            })
        );
    }
}
fn terminal(f: &Fixture, i: &Invocation, abnormal: bool) {
    assert!(
        f.store
            .list_provider_state("mq-delivery-live-v1-pending", 128)
            .unwrap()
            .is_empty()
    );
    let owners = f
        .store
        .list_provider_state("mq-selected-v1-uow-owner", 128)
        .unwrap();
    let owned: Vec<_> = owners
        .iter()
        .filter(|r| value(r)["execution"] == i.execution_id.as_str())
        .collect();
    let produced = !f.factory.children.lock().unwrap().is_empty();
    let backs =
        f.mq.originals
            .lock()
            .unwrap()
            .iter()
            .filter(|(actor, e, _)| {
                actor == i
                    && matches!(&e.request,
        HostRequest::MqMqi(h) if matches!(h.envelope.request,MqMqiRequest::Back{..}))
            })
            .count();
    assert_eq!(
        owned.len(),
        if produced { 2 + backs } else { 1 },
        "no post-terminal new unit"
    );
    assert_eq!(
        owned
            .iter()
            .filter(|r| value(r)["state"] == if abnormal { "rolled-back" } else { "committed" })
            .count(),
        if abnormal {
            1 + backs
        } else {
            owned.len() - backs
        }
    );
    let execution = f.store.get_execution(&i.execution_id).unwrap().unwrap();
    assert_eq!(
        execution.state,
        if abnormal {
            ExecutionState::Failed
        } else {
            ExecutionState::Completed
        }
    );
    let events = f.store.events(&i.execution_id, 1, 128).unwrap();
    assert_eq!(events.last().unwrap().sequence, execution.version);
    assert!(if abnormal {
        matches!(events.last().unwrap().kind, LifecycleEventKind::Abend)
    } else {
        matches!(
            events.last().unwrap().kind,
            LifecycleEventKind::Completed { return_code: 0 }
        )
    });
    let notifications = f.store.pending_notifications(128).unwrap();
    for e in &events {
        let n: Vec<_> = notifications
            .iter()
            .filter(|n| n.execution_id == e.execution_id && n.sequence == e.sequence)
            .collect();
        assert_eq!(n.len(), 1);
        assert_eq!(n[0].payload, lifecycle_notification_payload(&e.kind));
    }
    let audits = f.store.audit_subject_records(&i.execution_id, 128).unwrap();
    let terms: Vec<_> = audits
        .iter()
        .filter_map(|a| {
            if let AuditSubjectRecord::RootTerminal(a) = a {
                Some(a)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(terms.len(), 2);
    assert_eq!(terms[0].role, RootTerminalAuditRole::ProviderSettlement);
    assert_eq!(terms[1].role, RootTerminalAuditRole::CoreClosure);
    assert_eq!(terms[0].resource, terms[1].resource);
    assert!(
        terms
            .iter()
            .all(|a| a.principal == *i.principal.id() && a.decision == AuditDecision::Success)
    );
    let claim = f
        .store
        .get_provider_state(ROOT_DRIVER_NAMESPACE, i.execution_id.as_str())
        .unwrap()
        .unwrap();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&claim.payload).unwrap()["phase"],
        2
    );
    let frozen = (f.rows(), events, notifications, audits);
    assert!(
        matches!(f.router.execute_native_mq_root(&f.mq,i,"MQGETER"),Ok(ExecutionOutcome::ProviderFailure(ref p))if p.has_unknown_outcome())
    );
    assert_eq!(
        (
            f.rows(),
            f.store.events(&i.execution_id, 1, 128).unwrap(),
            f.store.pending_notifications(128).unwrap(),
            f.store.audit_subject_records(&i.execution_id, 128).unwrap()
        ),
        frozen
    );
    if f.root.0.join("configured.db").is_file() {
        let db = mainframe_env_store::SqliteStateStore::open(&f.url, 64 << 20, 65536).unwrap();
        assert_eq!(db.get_execution(&i.execution_id).unwrap(), Some(execution));
        assert_eq!(
            db.list_provider_state_prefix("mq-", 4096).unwrap(),
            frozen.0
        );
        assert_eq!(db.events(&i.execution_id, 1, 128).unwrap(), frozen.1);
        assert_eq!(db.pending_notifications(128).unwrap(), frozen.2);
        assert_eq!(
            db.audit_subject_records(&i.execution_id, 128).unwrap(),
            frozen.3
        );
    }
}
fn run(f: &Fixture, i: &Invocation, abnormal: bool) {
    let frozen = i.clone();
    let out = f
        .router
        .execute_native_mq_root(&f.mq, i, "MQGETER")
        .unwrap();
    assert!(
        if abnormal {
            matches!(out, ExecutionOutcome::Abend(_))
        } else {
            matches!(out,ExecutionOutcome::Completed(ref c)if c.output.bytes()==b"GET-DONE\n")
        },
        "{out:?}"
    );
    assert_eq!(i, &frozen);
}

#[test]
fn actual_put_then_original_qualified_get_complete_truncation_absent_and_terminal_memory_sqlite() {
    for sqlite in [false, true] {
        for version in [1, 2] {
            for case in 0..4 {
                for abnormal in [false, true] {
                    let f = native(sqlite, version);
                    if case != 3 {
                        produce(&f, version, version == 2);
                    }
                    f.install("MQGETER", &source(version, case, abnormal, 0));
                    let i = original(&f, "MQGETER", "get-consumer");
                    let store = f.store.clone();
                    let observed = Arc::new(AtomicBool::new(false));
                    let flag = observed.clone();
                    *f.saf.terminal_hook.lock().unwrap() = Some(Box::new(move || {
                        if removed_pending(&*store) {
                            pending(&*store, version);
                            flag.store(true, Ordering::SeqCst);
                        }
                    }));
                    run(&f, &i, abnormal);
                    assert_eq!(observed.load(Ordering::SeqCst), case < 2);
                    assert_receipts(&f, &i, 3);
                    if case != 3 {
                        let producer = f.factory.children.lock().unwrap()[0].clone();
                        assert_eq!(producer.parent_execution_id, Some(i.execution_id.clone()));
                        assert_receipts(&f, &producer, if version == 2 { 3 } else { 5 });
                        assert_eq!(
                            f.store
                                .get_execution(&producer.execution_id)
                                .unwrap()
                                .unwrap()
                                .state,
                            ExecutionState::Completed
                        );
                    }
                    assert_original(&f, &i, &i, version, case, 0);
                    queue(
                        &f,
                        version,
                        if case == 3 || (case < 2 && !abnormal) {
                            None
                        } else {
                            Some(i32::from(case < 2))
                        },
                    );
                    terminal(&f, &i, abnormal);
                }
            }
        }
    }
}

#[test]
fn genuine_same_task_get_child_retains_full_removed_work_and_one_root_abi_until_terminal() {
    for sqlite in [false, true] {
        for version in [1, 2] {
            for abnormal in [false, true] {
                let f = native(sqlite, version);
                produce(&f, version, true);
                f.install(
                    "MQLEAF",
                    &source(version, 0, false, 0)
                        .replace("PROGRAM-ID. MQGETER", "PROGRAM-ID. MQLEAF")
                        .replace("CALL 'MQPUTER'.\n", ""),
                );
                f.install("MQGETER",if abnormal{
            "IDENTIFICATION DIVISION. PROGRAM-ID. MQGETER. PROCEDURE DIVISION. CALL 'MQPUTER'. CALL 'MQLEAF'. CALL 'CEE3ABD'. GOBACK."
        }else{"IDENTIFICATION DIVISION. PROGRAM-ID. MQGETER. PROCEDURE DIVISION. CALL 'MQPUTER'. CALL 'MQLEAF'. DISPLAY 'GET-DONE'. GOBACK."});
                let i = original(&f, "MQGETER", "get-parent");
                let store = f.store.clone();
                let observed = Arc::new(AtomicBool::new(false));
                let flag = observed.clone();
                *f.saf.terminal_hook.lock().unwrap() = Some(Box::new(move || {
                    if removed_pending(&*store) {
                        pending(&*store, version);
                        flag.store(true, Ordering::SeqCst);
                    }
                }));
                run(&f, &i, abnormal);
                assert!(observed.load(Ordering::SeqCst));
                let child = f.factory.children.lock().unwrap()[1].clone();
                assert_eq!(child.parent_execution_id, Some(i.execution_id.clone()));
                assert_receipts(&f, &child, 3);
                assert_original(&f, &child, &i, version, 0, 0);
                let map = f.mq.topology.lock().unwrap();
                let RootEntry::Retained { frame: root, .. } =
                    map.roots.get(&i.execution_id).unwrap()
                else {
                    panic!()
                };
                let FrameEntry::Retained(frame) = map.frames.get(&child.execution_id).unwrap()
                else {
                    panic!()
                };
                assert!(Arc::ptr_eq(
                    &root.abi.as_ref().unwrap().scope,
                    &frame.abi.as_ref().unwrap().scope
                ));
                assert_eq!(frame.original(), &child);
                assert!(frame.check_original(&child).is_err());
                drop(map);
                queue(&f, version, if abnormal { Some(1) } else { None });
                terminal(&f, &i, abnormal);
            }
        }
    }
}

#[test]
fn actual_compiled_back_then_next_original_get_observes_source_counter_without_new_root() {
    for sqlite in [false, true] {
        for version in [1, 2] {
            let f = native(sqlite, version);
            produce(&f, version, false);
            let get = "CALL 'MQGET' USING BY REFERENCE HCONN HOBJ MESSAGE-DESC GET-OPTS BUFFER-LENGTH MESSAGE-BUFFER DATA-LENGTH CC REASON.";
            let again = format!(
                "{get}\nIF MD-BACKOUTCOUNT NOT = 0 DISPLAY 'BAD-FIRST-COUNT' END-IF.\n\
                CALL 'MQBACK' USING BY REFERENCE HCONN CC REASON.\n\
                IF CC NOT = 0 OR REASON NOT = 0 DISPLAY 'BAD-BACK' END-IF.\n\
                MOVE LOW-VALUES TO MD-MSGID MD-CORRELID.\nMOVE 73 TO MD-BACKOUTCOUNT.\n{get}"
            );
            f.install(
                "MQGETER",
                &source(version, 0, false, 1).replace(get, &again),
            );
            let i = original(&f, "MQGETER", "get-backed-out");
            run(&f, &i, false);
            assert_original(&f, &i, &i, version, 0, 0);
            assert_original(&f, &i, &i, version, 0, 1);
            assert_receipts(&f, &i, 5);
            queue(&f, version, None);
            terminal(&f, &i, false);
        }
    }
}

#[test]
fn completed_root_scope_cannot_be_recycled_as_a_new_get_driver() {
    for sqlite in [false, true] {
        let f = native(sqlite, 1);
        produce(&f, 1, false);
        f.install("MQGETER", &source(1, 0, true, 0));
        let first = original(&f, "MQGETER", "first-completed-root");
        run(&f, &first, true);
        queue(&f, 1, Some(1));
        terminal(&f, &first, true);
        let before = f.rows();
        let next = original(&f, "MQGETER", "second-root");
        assert!(
            matches!(f.router.execute_native_mq_root(&f.mq,&next,"MQGETER"),
            Ok(ExecutionOutcome::ProviderFailure(ref p))if p.has_unknown_outcome())
        );
        assert_eq!(f.rows(), before);
        assert_eq!(f.store.get_execution(&next.execution_id).unwrap(), None);
    }
}
