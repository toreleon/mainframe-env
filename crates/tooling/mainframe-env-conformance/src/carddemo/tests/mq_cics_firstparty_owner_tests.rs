use super::super::SeedImage;
use super::*;
use mainframe_env_execution_api::AuditRecord;
use mainframe_env_store::MemoryStore;
use mainframe_env_store_api::AuditSink;

fn offer() -> RequestOffer {
    let mut record = vec![b' '; 300];
    record[..11].copy_from_slice(b"00000050001");
    let mut body = vec![b' '; 1000];
    body[..15].copy_from_slice(b"INQA00000050001");
    RequestOffer {
        program: PROGRAM.into(),
        transaction: TRANSACTION.into(),
        input_queue: "CARD.DEMO.REQUEST.ACCT".into(),
        reply_queue: "CARD.DEMO.REPLY.ACCT".into(),
        body,
        message_id: [0x51; 24],
        correlation_id: [0x63; 24],
        required_clock: "not-consumed-by-this-account-control".into(),
        seeds: vec![SeedImage {
            dataset: "AWS.M2.CARDDEMO.ACCTDAT".into(),
            key: b"00000050001".to_vec(),
            record,
        }],
    }
}
fn key() -> IdempotencyKey {
    IdempotencyKey::new(
        "synthetic-account-wire-control",
        InvocationLimits::default(),
    )
    .unwrap()
}

#[test]
fn source_compatibility_start_frame_has_complete_fixed_layout_and_queue() {
    let bytes = start_payload("CARD.DEMO.REQUEST.ACCT").unwrap();
    let mut expected = vec![b' '; 684];
    expected[..4].copy_from_slice(b"TM  ");
    expected[4..8].copy_from_slice(&[0, 0, 0, 1]);
    expected[8..30].copy_from_slice(b"CARD.DEMO.REQUEST.ACCT");
    expected[168..172].copy_from_slice(&[0; 4]);
    assert_eq!(bytes, expected);
    for queue in [
        "CARD.DEMO.REQUEST.DATE",
        "CARD.DEMO.REQUEST.ACCT ",
        "",
        "../ACCT",
    ] {
        assert!(start_payload(queue).is_err());
    }
}

#[test]
fn account_source_requests_preserve_three_opens_two_gets_put_and_three_closes() {
    let offered = offer();
    let handles = [3, 4, 5];
    let reply = vec![b'R'; 1000];
    for index in 0..9 {
        let request = expected_mq(index, 11, key(), handles, &offered, &reply).unwrap();
        assert_eq!(request.mutation.as_ref().unwrap().transaction, None);
        assert_eq!(request.mutation.as_ref().unwrap().sequence, 11);
        match index {
            0..=2 => {
                assert_eq!(request.operation, MqOperation::Open);
                assert_eq!(request.options, if index == 1 { 8322 } else { 8720 });
                assert_eq!(request.handle, None);
                assert_eq!(request.max_message_bytes, 1);
            }
            3 | 5 => {
                assert_eq!(request.operation, MqOperation::Get);
                assert_eq!(request.options, 24579);
                assert_eq!(request.wait_ticks, 5000);
                assert_eq!(request.handle, Some(4));
                assert_eq!(request.max_message_bytes, 1000);
                assert_eq!(request.message_id, None);
                assert_eq!(request.correlation_id, None);
            }
            4 => {
                assert_eq!(request.operation, MqOperation::Put);
                assert_eq!(request.options, 8226);
                assert_eq!(request.handle, Some(5));
                assert_eq!(request.message, reply);
                assert_eq!(
                    request.message_id.as_deref(),
                    Some(offered.message_id.as_slice())
                );
                assert_eq!(
                    request.correlation_id.as_deref(),
                    Some(offered.correlation_id.as_slice())
                );
            }
            _ => {
                assert_eq!(request.operation, MqOperation::Close);
                assert_eq!(request.options, 0);
                assert_eq!(request.handle, Some([4, 5, 3][index - 6]));
            }
        }
        let result = expected_mq_result(index, handles, &offered).unwrap();
        assert_eq!(result.trigger_program, None);
        assert_eq!(result.completion_code, if index == 5 { 2 } else { 0 });
        assert_eq!(result.reason_code, if index == 5 { 2033 } else { 0 });
        assert_eq!(
            result.message,
            if index == 3 {
                offered.body.clone()
            } else {
                Vec::new()
            }
        );
        assert_eq!(
            result.handle,
            if index < 3 {
                Some(handles[index])
            } else {
                None
            }
        );
    }
    assert!(expected_mq(9, 11, key(), handles, &offered, &reply).is_err());
    assert!(expected_mq(0, 0, key(), handles, &offered, &reply).is_err());
    assert!(expected_mq(4, 11, key(), handles, &offered, &reply[..999]).is_err());
}

#[test]
fn canonical_account_call_and_audit_bind_full_request_not_result_counts() {
    let call = authorization_invocation(
        "synthetic-account-audit-control",
        true,
        ServiceClass::Interactive,
    )
    .unwrap();
    let owner = ExecutionRecord {
        execution_id: call.execution_id.clone(),
        run_unit_id: call.run_unit_id.clone(),
        selector: call.selector.clone(),
        artifact: call.artifact.clone(),
        principal: call.principal.id().clone(),
        state: ExecutionState::Completed,
        attempt: call.attempt,
        version: 1,
        owner_lease: None,
        lease_expiry_tick: None,
        terminal_tick: Some(1),
    };
    for index in 0..9 {
        let expected = expected_mq(index, 11, key(), [3, 4, 5], &offer(), &[b'R'; 1000]).unwrap();
        let host = HostRequest::Mq(expected.clone());
        let digest = mainframe_env_host_api::canonical_request_digest(&host).unwrap();
        let resource = mainframe_env_host_api::canonical_audit_resource_digest(&host);
        bind_request_digests(&expected, digest, &resource).unwrap();
        let capability = host.required_capability(InvocationLimits::default());
        assert_eq!(capability.as_str(), "host.mq.write");
        if matches!(index, 3 | 5) {
            assert_eq!(expected.message_id, None);
            assert_eq!(expected.correlation_id, None);
            for message_id in [true, false] {
                let mut explicit_zero = expected.clone();
                if message_id {
                    explicit_zero.message_id = Some(vec![0; 24]);
                } else {
                    explicit_zero.correlation_id = Some(vec![0; 24]);
                }
                assert!(bind_request_digests(&explicit_zero, digest, &resource).is_err());
            }
        }
        let valid = AuditRecord {
            execution_id: owner.execution_id.clone(),
            run_unit_id: owner.run_unit_id.clone(),
            attempt: owner.attempt,
            effect_sequence: 11,
            observed_tick: 1,
            principal: owner.principal.clone(),
            invocation_key: key(),
            capability,
            resource,
            decision: AuditDecision::Success,
        };
        let store = MemoryStore::new(Default::default());
        store.record_audit(valid.clone()).unwrap();
        audit(&store, &owner, 11, "host.mq.write", &key(), &resource).unwrap();
        // Direct nested and journal effects can share sequence/capability.
        // Only the exact canonical resource selects this effect's audit.
        let mut distinct = expected.clone();
        distinct.options ^= 1;
        let mut unrelated = valid.clone();
        unrelated.resource =
            mainframe_env_host_api::canonical_audit_resource_digest(&HostRequest::Mq(distinct));
        assert_ne!(unrelated.resource, resource);
        store.record_audit(unrelated.clone()).unwrap();
        audit(&store, &owner, 11, "host.mq.write", &key(), &resource).unwrap();
        for change in 0..9 {
            let negative = MemoryStore::new(Default::default());
            let mut changed = valid.clone();
            match change {
                0 => negative.record_audit(valid.clone()).unwrap(),
                1 => changed = unrelated.clone(),
                2 => changed.decision = AuditDecision::Deny,
                3 => {
                    changed.principal =
                        PrincipalId::new("WRONGUSR", InvocationLimits::default()).unwrap();
                }
                4 => {
                    changed.run_unit_id =
                        RunUnitId::new("wrong-audit-run", InvocationLimits::default()).unwrap();
                }
                5 => changed.attempt += 1,
                6 => {
                    changed.invocation_key =
                        IdempotencyKey::new("wrong-audit-key", InvocationLimits::default())
                            .unwrap();
                }
                7 => {
                    changed.execution_id =
                        ExecutionId::new("wrong-audit-execution", InvocationLimits::default())
                            .unwrap();
                }
                _ => {
                    changed.capability =
                        CapabilityId::new("host.mq.read", InvocationLimits::default()).unwrap();
                }
            }
            negative.record_audit(changed).unwrap();
            assert!(audit(&negative, &owner, 11, "host.mq.write", &key(), &resource,).is_err());
        }
        for change in 0..11 {
            let mut different = expected.clone();
            match change {
                0 => different.operation = MqOperation::PutOne,
                1 => different.queue = Some("OTHER.QUEUE".into()),
                2 => different.handle = Some(99),
                3 => different.options ^= 1,
                4 => different.wait_ticks += 1,
                5 => different.max_message_bytes += 1,
                6 => different.message.push(0),
                7 => different.message_id = Some(vec![1; 24]),
                8 => different.correlation_id = Some(vec![2; 24]),
                9 => different.mutation.as_mut().unwrap().sequence += 1,
                _ => different.mutation.as_mut().unwrap().transaction = Some(TRANSACTION.into()),
            }
            assert!(bind_request_digests(&different, digest, &resource).is_err());
        }
        let mut wrong_digest = digest;
        wrong_digest[31] ^= 1;
        assert!(bind_request_digests(&expected, wrong_digest, &resource).is_err());
    }
}

#[test]
fn complete_queue_images_refuse_extra_missing_reused_or_tail_changed_messages() {
    let offer = offer();
    let reply = vec![b'R'; 1000];
    compare_queues(std::slice::from_ref(&offer.body), &[], &[], &offer, None).unwrap();
    compare_queues(&[], std::slice::from_ref(&reply), &[], &offer, Some(&reply)).unwrap();
    assert!(compare_queues(&[], &[], &[], &offer, None).is_err());
    assert!(
        compare_queues(
            &[offer.body.clone(), offer.body.clone()],
            &[],
            &[],
            &offer,
            None
        )
        .is_err()
    );
    assert!(
        compare_queues(
            std::slice::from_ref(&offer.body),
            std::slice::from_ref(&reply),
            &[],
            &offer,
            Some(&reply)
        )
        .is_err()
    );
    assert!(
        compare_queues(
            &[],
            &[reply.clone(), reply.clone()],
            &[],
            &offer,
            Some(&reply)
        )
        .is_err()
    );
    assert!(
        compare_queues(
            &[],
            std::slice::from_ref(&reply),
            &[Vec::new()],
            &offer,
            Some(&reply)
        )
        .is_err()
    );
    let mut changed = reply.clone();
    changed[999] ^= 1;
    assert!(compare_queues(&[], &[changed], &[], &offer, Some(&reply)).is_err());
    assert!(
        compare_queues(
            &[],
            std::slice::from_ref(&offer.body),
            &[],
            &offer,
            Some(&reply)
        )
        .is_err()
    );
}

#[test]
fn complete_account_postimage_refuses_filler_key_neighbor_and_missing_record() {
    let offer = offer();
    compare_account_records(&[offer.seeds[0].record.clone()], &offer).unwrap();
    for offset in [0, 10, 11, 251, 299] {
        let mut changed = offer.seeds[0].record.clone();
        changed[offset] ^= 1;
        assert!(compare_account_records(&[changed], &offer).is_err());
    }
    assert!(compare_account_records(&[], &offer).is_err());
    assert!(
        compare_account_records(&[offer.seeds[0].record.clone(), vec![b' '; 300]], &offer).is_err()
    );
    assert!(compare_account_records(&[offer.seeds[0].record[..299].to_vec()], &offer).is_err());
}

#[test]
fn completed_work_is_only_ready_for_full_proof_after_both_activity_counters_clear() {
    assert!(ready_to_compare(WorkState::Completed, 0, 0));
    for state in [WorkState::Queued, WorkState::Claimed] {
        assert!(!ready_to_compare(state, 0, 0));
    }
    for (active, jes) in [(1, 0), (0, 1), (1, 1), (usize::MAX, 0)] {
        assert!(!ready_to_compare(WorkState::Completed, active, jes));
    }
    // No ready bit is an application proof: guest_proof still verifies the
    // genuine artifact/actor terminal, every canonical call/audit and UOW.
}

#[test]
#[ignore = "requires exact clean CardDemo corpus and Root's separately released owning native slot"]
fn installed_account_start_preserves_full_reply_ids_sqlite_restarts_and_zero_credit() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let receipt =
        verify_from_env(&root.join("conformance/profiles/carddemo/inventory/carddemo-corpus.json"))
            .unwrap();
    assert_eq!(receipt["actual_installed_guest_compared"], true);
    assert_eq!(receipt["pending_request_restart_compared"], true);
    assert_eq!(receipt["post_guest_restart_compared"], true);
    assert_eq!(receipt["guest_proof_rechecked_after_physical_reopen"], true);
    assert_eq!(
        receipt["reopened_reply_provider_client_read_compared"],
        true
    );
    assert_eq!(receipt["physical_sqlite_reopens"], 2);
    for field in [
        "seed_server_drain",
        "guest_server_drain",
        "reopened_server_drain",
        "cleanup_verified",
    ] {
        assert_eq!(receipt[field], true);
    }
    assert_eq!(receipt["actual_wait_duration_claimed"], false);
    assert_eq!(receipt["automatic_mq_trigger_listener_claimed"], false);
    assert_eq!(receipt["application_credit"], 0);
    assert_eq!(receipt["vendor_credit"], 0);
    assert_eq!(receipt["acceptance_credit"], 0);
}
