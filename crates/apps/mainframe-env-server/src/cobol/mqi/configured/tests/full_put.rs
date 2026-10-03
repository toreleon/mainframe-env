//! Genuine compiled complete PUT composition; setup is the approved empty fixture.
use super::native_point::{assert_receipts, native};
use super::setup::*;
use super::*;
use mainframe_env_execution_api::*;
use mainframe_env_host_api::mq_md_value::*;
use mainframe_env_host_api::mq_mqi::*;
use mainframe_env_host_api::*;
use mainframe_env_store_api::*;

mod refusals;

pub(super) fn source(version: i32, one: bool, abnormal: bool) -> String {
    let mut text = if version == 1 {
        include_str!("full_put/put1.cbl")
    } else {
        include_str!("full_put/put2.cbl")
    }
    .to_owned();
    if one {
        text = text
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
        text = text.replace("GOBACK.", "CALL 'CEE3ABD'. GOBACK.");
    }
    text
}

pub(super) fn expected_md(version: i32, stored: bool) -> MqMdValue {
    let fields = MqMdFields {
        struc_id: *b"MD  ",
        report: 0,
        msg_type: 8,
        expiry: -1,
        feedback: 0,
        encoding: 785,
        coded_char_set_id: 819,
        format: [b' '; 8],
        priority: 0,
        persistence: 1,
        msg_id: *b"ABCDEFGHIJKLMNOPQRSTUVWX",
        correl_id: *b"ZYXWVUTSRQPONMLKJIHGFEDC",
        backout_count: if stored { 0 } else { 73 },
        reply_to_q: [b' '; 48],
        reply_to_q_mgr: [b' '; 48],
        user_identifier: [b' '; 12],
        accounting_token: [0; 32],
        appl_identity_data: [b' '; 32],
        put_appl_type: 0,
        put_appl_name: [b' '; 28],
        put_date: [b' '; 8],
        put_time: [b' '; 8],
        appl_origin_data: [b' '; 4],
    };
    if version == 1 {
        MqMdValue::V1 {
            characters: MqMdCharacterEncoding::AsciiCompatible,
            fields,
        }
    } else {
        MqMdValue::V2 {
            characters: MqMdCharacterEncoding::AsciiCompatible,
            fields,
            extension: MqMdV2Fields {
                group_id: [0; 24],
                msg_seq_number: 1,
                offset: 0,
                msg_flags: 0,
                original_length: -1,
            },
        }
    }
}
fn value(row: &ProviderStateRecord) -> serde_json::Value {
    serde_json::from_slice::<serde_json::Value>(&row.payload).unwrap()["value"].clone()
}
fn input_md(version: i32) -> MqMdValue {
    fn input<const N: usize>() -> [u8; N] {
        let mut bytes = [b' '; N];
        bytes[..5].copy_from_slice(b"INPUT");
        bytes
    }
    let mut md = expected_md(version, false);
    let f = match &mut md {
        MqMdValue::V1 { fields, .. } | MqMdValue::V2 { fields, .. } => fields,
    };
    f.user_identifier = input();
    f.accounting_token = input();
    f.appl_identity_data = input();
    f.put_appl_name = input();
    f.put_date = input();
    f.put_time = input();
    f.appl_origin_data = *b"INPU";
    md
}
fn assert_original_put(f: &Fixture, actor: &Invocation, version: i32, one: bool) {
    let calls = f.mq.originals.lock().unwrap();
    let (original,effect,reply)=calls.iter().find(|(inv,e,_)| inv==actor && matches!(&e.request,
        HostRequest::MqMqi(host) if matches!(host.envelope.request,MqMqiRequest::FullPut{..}|MqMqiRequest::FullPutOne{..}))).unwrap();
    assert_eq!(original, actor);
    assert_eq!(effect.run_unit, actor.run_unit_id);
    assert_eq!(effect.deadline_tick, actor.deadline_tick);
    let occurrence = effect
        .mq_mqi_occurrence(Default::default())
        .unwrap()
        .unwrap();
    let envelope = occurrence.envelope();
    let put = match &envelope.request {
        MqMqiRequest::FullPut {
            connection, put, ..
        } if !one => {
            assert!(matches!(connection, MqHconn::Issued(_)));
            put
        }
        MqMqiRequest::FullPutOne {
            connection,
            lookup,
            alternate_user,
            put,
        } if one => {
            assert!(matches!(connection, MqHconn::Issued(_)));
            assert!(alternate_user.is_none());
            assert!(
                matches!(lookup,mainframe_env_host_api::mq_object_route::MqRouteLookup::Queue{name,manager:None,dynamic_pattern:None} if name.as_str()=="Q")
            );
            put
        }
        _ => panic!("wrong original request"),
    };
    let unit = value(
        &f.store
            .list_provider_state("mq-selected-v1-uow-owner", 128)
            .unwrap()[0],
    )["unit"]
        .as_u64()
        .unwrap();
    assert_eq!(
        put,
        &MqMqiFullPut {
            message: MqFullMessage {
                descriptor: input_md(version),
                body: b"HELLO".to_vec(),
                properties: vec![]
            },
            message_handle: None,
            context: MqMqiMessageContext::NoContext,
            options: MqMqiOptions::PutV1Synchronous,
            unit: MqMqiUnitOfWork::Local { unit },
        }
    );
    let core = f
        .store
        .effect(effect.idempotency_key.as_ref().unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(core.sequence, effect.sequence);
    assert_eq!(core.execution_id, actor.execution_id);
    assert_eq!(
        core.request_digest,
        canonical_request_digest(&effect.request).unwrap()
    );
    assert_eq!(
        core.result_digest,
        Some(canonical_result_digest(&reply.outcome).unwrap())
    );
    assert_eq!(core.intent.owner, actor.execution_id);
    assert_eq!(
        core.intent.audit_invocation_key.as_ref(),
        Some(&actor.idempotency_key)
    );
    let Ok(HostResult::MqMqi(result)) = &reply.outcome else {
        panic!("real typed reply")
    };
    let MqMqiOutcome::ReviewedOutput {
        status,
        output: MqMqiOutput::Produced(produced),
    } = &result.result.outcome
    else {
        panic!("lossless Produced")
    };
    assert_eq!(status.wire_pair(), (0, 0));
    assert_eq!(produced.descriptor, expected_md(version, false));
    assert_eq!(produced.outcome, MqDeliveryOutcome::Pending);
    assert_eq!(
        produced.backout_count,
        MqMqiIgnoredCounter::PreservedIgnoredInput
    );
    for count in [
        produced.known_dest_count,
        produced.unknown_dest_count,
        produced.invalid_dest_count,
    ] {
        assert_eq!(count, MqMqiDestinationCount::UndefinedZos);
    }
    let mut queue = [b' '; 48];
    queue[0] = b'Q';
    assert_eq!(produced.resolved_queue, queue);
    let mut manager = [b' '; 48];
    manager[..2].copy_from_slice(b"QM");
    assert_eq!(produced.resolved_manager, manager);
}
fn assert_message(entry: &serde_json::Value, version: i32) {
    assert_eq!(entry["expires_at"], serde_json::Value::Null);
    assert_eq!(entry["persistent"], true);
    assert_eq!(entry["message"]["kind"], "complete");
    let m = &entry["message"]["value"];
    let md: Vec<u8> = serde_json::from_value(m["md"].clone()).unwrap();
    assert_eq!(
        mq_md_value_decode(&md, 2048).unwrap(),
        expected_md(version, true)
    );
    assert_eq!(
        serde_json::from_value::<Vec<u8>>(m["body"].clone()).unwrap(),
        b"HELLO"
    );
    assert_eq!(m["properties"], serde_json::json!([]));
}
fn pending(store: &dyn PlatformStore, version: i32) {
    let queues = store
        .list_provider_state("mq-delivery-live-v1-queue", 128)
        .unwrap();
    assert_eq!(queues.len(), 1);
    assert_eq!(value(&queues[0])["messages"], serde_json::json!([]));
    let units = store
        .list_provider_state("mq-delivery-live-v1-pending", 128)
        .unwrap();
    assert_eq!(units.len(), 1);
    let unit = value(&units[0]);
    let ops = unit["operations"].as_array().unwrap();
    assert_eq!(ops.len(), 1);
    assert_eq!(ops[0]["queue"], "Q");
    assert_eq!(ops[0]["put"], true);
    assert_message(&ops[0]["entry"], version);
}
fn terminal(f: &Fixture, original: &Invocation, version: i32, abnormal: bool) {
    assert!(
        f.store
            .list_provider_state("mq-delivery-live-v1-pending", 128)
            .unwrap()
            .is_empty()
    );
    let rows = f
        .store
        .list_provider_state("mq-delivery-live-v1-queue", 128)
        .unwrap();
    let q = value(&rows[0]);
    let messages = q["messages"].as_array().unwrap();
    assert_eq!(messages.len(), usize::from(!abnormal));
    if !abnormal {
        assert_message(&messages[0], version);
    }
    let owners = f
        .store
        .list_provider_state("mq-selected-v1-uow-owner", 128)
        .unwrap();
    assert_eq!(owners.len(), 1, "no post-terminal unit allocation");
    let owner = value(&owners[0]);
    assert_eq!(owner["execution"], original.execution_id.as_str());
    assert_eq!(
        owner["state"],
        if abnormal { "rolled-back" } else { "committed" }
    );
    let execution = f
        .store
        .get_execution(&original.execution_id)
        .unwrap()
        .unwrap();
    assert_eq!(
        execution.state,
        if abnormal {
            ExecutionState::Failed
        } else {
            ExecutionState::Completed
        }
    );
    let events = f.store.events(&original.execution_id, 1, 128).unwrap();
    assert_eq!(events.last().unwrap().sequence, execution.version);
    assert!(if abnormal {
        matches!(events.last().unwrap().kind, LifecycleEventKind::Abend)
    } else {
        matches!(
            events.last().unwrap().kind,
            LifecycleEventKind::Completed { return_code: 0 }
        )
    });
    let outbox = f.store.pending_notifications(128).unwrap();
    for event in &events {
        let rows: Vec<_> = outbox
            .iter()
            .filter(|n| n.execution_id == event.execution_id && n.sequence == event.sequence)
            .collect();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].payload, lifecycle_notification_payload(&event.kind));
    }
    let audits = f
        .store
        .audit_subject_records(&original.execution_id, 128)
        .unwrap();
    let terminal: Vec<_> = audits
        .iter()
        .filter_map(|a| {
            if let AuditSubjectRecord::RootTerminal(a) = a {
                Some(a)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(terminal.len(), 2);
    assert_eq!(terminal[0].role, RootTerminalAuditRole::ProviderSettlement);
    assert_eq!(terminal[1].role, RootTerminalAuditRole::CoreClosure);
    assert_eq!(terminal[0].resource, terminal[1].resource);
    assert!(
        terminal.iter().all(
            |a| a.principal == *original.principal.id() && a.decision == AuditDecision::Success
        )
    );
    let claim = f
        .store
        .get_provider_state(ROOT_DRIVER_NAMESPACE, original.execution_id.as_str())
        .unwrap()
        .unwrap();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&claim.payload).unwrap()["phase"],
        2
    );
    let frozen = (f.rows(), events, outbox, audits);
    // A completed opposite machine invocation cannot acquire the old winner.
    assert!(
        matches!(f.router.execute_native_mq_root(&f.mq,original,"MQROOT"),Ok(ExecutionOutcome::ProviderFailure(ref p)) if p.has_unknown_outcome())
    );
    assert_eq!(
        (
            f.rows(),
            f.store.events(&original.execution_id, 1, 128).unwrap(),
            f.store.pending_notifications(128).unwrap(),
            f.store
                .audit_subject_records(&original.execution_id, 128)
                .unwrap()
        ),
        frozen
    );
    if !f.url.is_empty() && f.url.contains("configured.db") {
        // Memory fixtures share a URL string too; only open an existing owned DB.
        if f.root.0.join("configured.db").is_file() {
            let reopened =
                mainframe_env_store::SqliteStateStore::open(&f.url, 64 << 20, 65536).unwrap();
            assert_eq!(
                reopened.get_execution(&original.execution_id).unwrap(),
                Some(execution)
            );
            assert_eq!(
                reopened.list_provider_state_prefix("mq-", 4096).unwrap(),
                frozen.0
            );
            assert_eq!(
                reopened.events(&original.execution_id, 1, 128).unwrap(),
                frozen.1
            );
            assert_eq!(reopened.pending_notifications(128).unwrap(), frozen.2);
            assert_eq!(
                reopened
                    .audit_subject_records(&original.execution_id, 128)
                    .unwrap(),
                frozen.3
            );
        }
    }
}

#[test]
fn actual_compiled_full_put_pending_then_normal_or_cee3abd_terminal_memory_sqlite() {
    for sqlite in [false, true] {
        for version in [1, 2] {
            for one in [false, true] {
                for abnormal in [false, true] {
                    let f = native(sqlite, version);
                    f.install("MQROOT", &source(version, one, abnormal));
                    let original = super::native_root::original(&f);
                    let before = original.clone();
                    let store = f.store.clone();
                    let observed = Arc::new(std::sync::atomic::AtomicBool::new(false));
                    let flag = observed.clone();
                    *f.saf.terminal_hook.lock().unwrap() = Some(Box::new(move || {
                        let rows = store
                            .list_provider_state("mq-delivery-live-v1-pending", 128)
                            .unwrap();
                        if !rows.is_empty() {
                            pending(&*store, version);
                            flag.store(true, std::sync::atomic::Ordering::Release);
                        }
                    }));
                    let outcome = f
                        .router
                        .execute_native_mq_root(&f.mq, &original, "MQROOT")
                        .unwrap();
                    assert!(
                        if abnormal {
                            matches!(outcome, ExecutionOutcome::Abend(_))
                        } else {
                            matches!(outcome,ExecutionOutcome::Completed(ref done) if done.output.bytes()==b"PUT-DONE\n")
                        },
                        "{outcome:?}"
                    );
                    assert_eq!(original, before);
                    assert!(
                        observed.load(std::sync::atomic::Ordering::Acquire),
                        "real pending PUT before terminal"
                    );
                    assert_receipts(&f, &original, if one { 2 } else { 3 });
                    assert_original_put(&f, &original, version, one);
                    terminal(&f, &original, version, abnormal);
                }
            }
        }
    }
}

#[test]
fn separately_compiled_same_task_child_put_preserves_origin_until_real_root_terminal() {
    for sqlite in [false, true] {
        for version in [1, 2] {
            for one in [false, true] {
                let f = native(sqlite, version);
                f.install(
                    "MQLEAF",
                    &source(version, one, false)
                        .replace("PROGRAM-ID. MQROOT", "PROGRAM-ID. MQLEAF"),
                );
                f.install("MQROOT", "IDENTIFICATION DIVISION. PROGRAM-ID. MQROOT. PROCEDURE DIVISION. CALL 'MQLEAF'. DISPLAY 'ROOT-DONE'. GOBACK.");
                let original = super::native_root::original(&f);
                let frozen = original.clone();
                let store = f.store.clone();
                let observed = Arc::new(std::sync::atomic::AtomicBool::new(false));
                let flag = observed.clone();
                *f.saf.terminal_hook.lock().unwrap() = Some(Box::new(move || {
                    if !store
                        .list_provider_state("mq-delivery-live-v1-pending", 128)
                        .unwrap()
                        .is_empty()
                    {
                        pending(&*store, version);
                        flag.store(true, std::sync::atomic::Ordering::Release);
                    }
                }));
                let outcome = f
                    .router
                    .execute_native_mq_root(&f.mq, &original, "MQROOT")
                    .unwrap();
                assert!(
                    matches!(outcome,ExecutionOutcome::Completed(ref done) if done.output.bytes()==b"ROOT-DONE\n"),
                    "{outcome:?}"
                );
                assert_eq!(original, frozen);
                assert!(observed.load(std::sync::atomic::Ordering::Acquire));
                let child = f.factory.children.lock().unwrap()[0].clone();
                assert_eq!(
                    child.parent_execution_id,
                    Some(original.execution_id.clone())
                );
                assert_receipts(&f, &child, if one { 2 } else { 3 });
                assert_original_put(&f, &child, version, one);
                assert_eq!(
                    f.store
                        .get_execution(&child.execution_id)
                        .unwrap()
                        .unwrap()
                        .state,
                    ExecutionState::Completed
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
                terminal(&f, &original, version, false);
            }
        }
    }
}
