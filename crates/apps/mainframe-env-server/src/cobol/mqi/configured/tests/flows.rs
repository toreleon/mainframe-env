use super::setup::*;
use super::*;
use mainframe_env_execution_api::*;
use mainframe_env_host_api::*;
use mainframe_env_store::SqliteStateStore;
use mainframe_env_store_api::*;

#[test]
fn real_compiled_selected_call_commits_original_receipts_and_both_audit_layers() {
    for sqlite in [false, true] {
        let f = Fixture::new(sqlite);
        f.install("MQFLOW", SOURCE);
        let original = f.batch_effect(1, "MQFLOW");
        let parent = f.parent.clone();
        let (outcome, reply) = f.run(original.clone());
        assert!(
            matches!(outcome, ExecutionOutcome::Completed(_)),
            "{outcome:?}"
        );
        let Ok(HostResult::Program(payload)) = reply.outcome else {
            panic!("real selected reply {reply:?}")
        };
        let output: mainframe_env_batch::ProgramOutput =
            serde_json::from_slice(payload.bytes()).unwrap();
        assert_eq!(output.records, vec![b"DONE".to_vec()]);
        assert_eq!(f.parent, parent);
        assert!(!parent.bindings.contains_key("mq.host-context"));
        let children = f.factory.children.lock().unwrap();
        assert_eq!(children.len(), 1);
        let child = children[0].clone();
        drop(children);
        assert_eq!(
            child.parent_execution_id.as_ref(),
            Some(&parent.execution_id)
        );
        let receipts = f
            .store
            .list_provider_state("mq-selected-v1-occurrence", 100)
            .unwrap();
        assert_eq!(receipts.len(), 5);
        for r in &receipts {
            let wrapper: serde_json::Value = serde_json::from_slice(&r.payload).unwrap();
            let v = &wrapper["value"];
            assert_eq!(v["execution"], child.execution_id.as_str());
            let key = IdempotencyKey::new(v["key"].as_str().unwrap(), Default::default()).unwrap();
            let core = f.store.effect(&key).unwrap().unwrap();
            assert_eq!(core.state, EffectState::Completed);
            assert_eq!(core.execution_id, child.execution_id);
            assert_eq!(
                serde_json::to_value(core.request_digest).unwrap(),
                v["request_digest"]
            );
        }
        let audits = f.store.audit_records(&child.execution_id, 0, 100).unwrap();
        assert_eq!(
            audits.len(),
            10,
            "atomic provider and outer coordinator observations"
        );
        for sequence in 1..=5 {
            let pair: Vec<_> = audits
                .iter()
                .filter(|a| a.effect_sequence == sequence)
                .collect();
            assert_eq!(pair.len(), 2);
            assert!(
                pair.iter()
                    .all(|a| a.decision == AuditDecision::Success && a.observed_tick == 20)
            );
            assert_eq!(pair[0].principal, parent.principal.id().clone());
            assert_eq!(pair[0].resource, pair[1].resource);
        }
        assert_eq!(
            f.store
                .effect(original.idempotency_key.as_ref().unwrap())
                .unwrap()
                .unwrap()
                .request_digest,
            canonical_request_digest(&original.request).unwrap()
        );
        let notifications = f.store.pending_notifications(128).unwrap();
        for execution in [&parent.execution_id, &child.execution_id] {
            let record = f.store.get_execution(execution).unwrap().unwrap();
            assert_eq!(record.state, ExecutionState::Completed);
            let events = f.store.events(execution, 0, 128).unwrap();
            assert!(events.iter().any(|event| matches!(
                event.kind,
                LifecycleEventKind::Completed { return_code: 0 }
            )));
            for event in events {
                let entries: Vec<_> = notifications
                    .iter()
                    .filter(|notification| {
                        notification.execution_id == *execution
                            && notification.sequence == event.sequence
                    })
                    .collect();
                assert_eq!(
                    entries.len(),
                    1,
                    "one actual outbox entry for each committed lifecycle event"
                );
                assert!(!entries[0].delivered);
                assert_eq!(entries[0].delivered_tick, None);
                assert!(!entries[0].payload.is_empty());
            }
        }
        assert!(!f.saf.resources.lock().unwrap().is_empty());
        assert!(
            f.factory.observations.lock().unwrap()[0]
                .profile(&child)
                .is_err()
        );
        let before = f.rows();
        // Completed original CALL uses its existing core/cache protocol; no new
        // child setup, MQ redispatch, aliases or provider audit is minted.
        assert!(f.router.invoke(&parent, original.clone()).outcome.is_ok());
        assert_eq!(f.rows(), before);
        assert_eq!(f.factory.children.lock().unwrap().len(), 1);
        assert_eq!(f.store.pending_notifications(128).unwrap(), notifications);
        if sqlite {
            let reopened = SqliteStateStore::open(&f.url, 64 << 20, 65536).unwrap();
            assert_eq!(
                reopened
                    .list_provider_state("mq-selected-v1-occurrence", 100)
                    .unwrap(),
                receipts
            );
            assert_eq!(
                reopened.audit_records(&child.execution_id, 0, 100).unwrap(),
                audits
            );
            assert_eq!(reopened.pending_notifications(128).unwrap(), notifications);
        }
    }
}

#[test]
fn successive_real_children_reuse_task_connection_after_normal_nonfinal_return() {
    for sqlite in [false, true] {
        let f = Fixture::new(sqlite);
        let first = "IDENTIFICATION DIVISION. PROGRAM-ID. MQFIRST. DATA DIVISION. WORKING-STORAGE SECTION. 01 QM PIC X(48) VALUE SPACES. 01 HC PIC S9(9) BINARY. 01 CC PIC S9(9) BINARY. 01 RC PIC S9(9) BINARY. LINKAGE SECTION. 01 OUTCOME PIC X(8). PROCEDURE DIVISION USING OUTCOME. MOVE 'FIRST' TO OUTCOME. CALL 'MQCONN' USING QM HC CC RC. IF CC NOT = 0 OR RC NOT = 0 MOVE 'BAD' TO OUTCOME END-IF. GOBACK.";
        f.install("MQFIRST", first);
        let second = SOURCE
            .replace("PROGRAM-ID. MQFLOW", "PROGRAM-ID. MQSECOND")
            .replace("PROCEDURE DIVISION.", "LINKAGE SECTION. 01 OUTCOME PIC X(8). PROCEDURE DIVISION USING OUTCOME. MOVE 'DONE' TO OUTCOME.")
            .replace("DISPLAY 'BAD-CONN'", "MOVE 'BAD' TO OUTCOME")
            .replace("DISPLAY 'BAD-WARNING'", "MOVE 'BAD' TO OUTCOME")
            .replace("DISPLAY 'BAD-CMIT'", "MOVE 'BAD' TO OUTCOME")
            .replace("DISPLAY 'BAD-BACK'", "MOVE 'BAD' TO OUTCOME")
            .replace("DISPLAY 'BAD-DISC'", "MOVE 'BAD' TO OUTCOME")
            .replace("DISPLAY 'DONE'.", "")
            .replacen(
                "IF CC NOT = 0 OR RC NOT = 0 MOVE 'BAD' TO OUTCOME",
                "IF CC NOT = 1 OR RC NOT = 2002 MOVE 'BAD' TO OUTCOME",
                1,
            );
        f.install("MQSECOND", &second);
        let (outcome, replies) = f.run_calls(vec![
            f.call_effect(1, "MQFIRST", &[vec![b' '; 8]]),
            f.call_effect(2, "MQSECOND", &[vec![b' '; 8]]),
        ]);
        assert!(
            matches!(outcome, ExecutionOutcome::Completed(_)),
            "{outcome:?}"
        );
        assert_eq!(replies.len(), 2);
        for (reply, expected) in replies
            .iter()
            .zip([b"FIRST   ".as_slice(), b"DONE    ".as_slice()])
        {
            let Ok(HostResult::Program(p)) = &reply.outcome else {
                panic!("{reply:?}")
            };
            assert_eq!(
                p,
                &mainframe_env_interpreter::encode_cobol_call_result(&[expected.to_vec()]).unwrap()
            );
        }
        assert_eq!(f.factory.children.lock().unwrap().len(), 2);
        assert_eq!(f.mq.topology.lock().unwrap().roots.len(), 1);
        let receipts = f
            .store
            .list_provider_state("mq-selected-v1-occurrence", 100)
            .unwrap();
        assert_eq!(receipts.len(), 6);
        // Inspect captured storage observations without reconstructing a token
        // or claiming that stored identity confers live registry authority.
        let mut connection = None;
        let mut connected = 0;
        for row in receipts {
            let receipt: serde_json::Value = serde_json::from_slice(&row.payload).unwrap();
            let value = &receipt["value"];
            if value["call"] != "MQCONN" && value["call"] != "MQCONNX" {
                continue;
            }
            let bytes: Vec<u8> = serde_json::from_value(value["reply"]["bytes"].clone()).unwrap();
            let result: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            let observed = result["outcome"]["output"]["connection"].clone();
            assert!(!observed.is_null());
            if let Some(prior) = &connection {
                assert_eq!(&observed, prior);
            } else {
                connection = Some(observed);
            }
            if value["call"] == "MQCONNX"
                || value["sequence"] == 1
                    && value["execution"]
                        == f.factory.children.lock().unwrap()[1].execution_id.as_str()
            {
                assert_eq!(result["outcome"]["kind"], "ReviewedOutput");
                assert_eq!(result["outcome"]["completion"], "MQCC_WARNING");
                assert_eq!(result["outcome"]["reason"], "MQRC_ALREADY_CONNECTED");
            }
            connected += 1;
        }
        assert_eq!(connected, 3);
    }
}
