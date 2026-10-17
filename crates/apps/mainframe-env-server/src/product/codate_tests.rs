//! Synthetic controls exercise private receivers, not native acceptance receipts.
use super::*;
use mainframe_env_execution_api::AuditRecord;
use mainframe_env_store_api::AuditSink;

fn key() -> IdempotencyKey {
    IdempotencyKey::new("synthetic-date-wire-control", InvocationLimits::default()).unwrap()
}
fn invocation() -> Invocation {
    authorization_invocation(
        "synthetic-date-clock-control",
        true,
        ServiceClass::Interactive,
    )
    .unwrap()
}
fn effect(invocation: &Invocation, sequence: u64) -> EffectRequest {
    EffectRequest {
        run_unit: invocation.run_unit_id.clone(),
        sequence,
        deadline_tick: invocation.deadline_tick,
        idempotency_key: None,
        request: HostRequest::Clock(ClockRequest::UtcTimestamp),
    }
}

#[test]
fn private_clock_default_and_instances_do_not_share_override_state() {
    let limits = InvocationLimits::default();
    let default = RuntimeClocks::default().host_clock(limits).unwrap();
    assert_eq!(
        default.descriptor(),
        &SystemClockProvider::new(limits).descriptor
    );
    // Do not invoke the system clock in a pure control.
    let first = Arc::new(clock::SourceClock::new(oracle::UTC_INPUT).unwrap());
    let second = Arc::new(clock::SourceClock::new(oracle::UTC_INPUT).unwrap());
    let selected = RuntimeClocks {
        jes: None,
        host: Some(first.clone()),
    }
    .host_clock(limits)
    .unwrap();
    let other = RuntimeClocks {
        jes: None,
        host: Some(second.clone()),
    }
    .host_clock(limits)
    .unwrap();
    assert!(!Arc::ptr_eq(&selected, &other));
    let call = invocation();
    assert_eq!(
        selected.invoke(&call, effect(&call, 1)).outcome,
        Ok(HostResult::Clock(oracle::UTC_INPUT.into()))
    );
    assert_eq!(first.observations().unwrap().len(), 1);
    assert!(second.observations().unwrap().is_empty());
    struct RejectedClock(CapabilityDescriptor);
    impl HostProvider for RejectedClock {
        fn descriptor(&self) -> &CapabilityDescriptor {
            &self.0
        }
        fn invoke(&self, _: &Invocation, _: EffectRequest) -> EffectResult {
            panic!("invalid private clock must refuse before dispatch")
        }
    }
    for change in 0..6 {
        let mut descriptor = first.descriptor().clone();
        match change {
            0 => descriptor.capability = CapabilityId::new("host.mq.read", limits).unwrap(),
            1 => descriptor.request_schema = "other".into(),
            2 => descriptor.result_schema = "other".into(),
            3 => descriptor.max_request_bytes = 257,
            4 => descriptor.max_result_bytes = 257,
            _ => descriptor.ready = false,
        }
        assert!(
            RuntimeClocks {
                jes: None,
                host: Some(Arc::new(RejectedClock(descriptor)))
            }
            .host_clock(limits)
            .is_err()
        );
    }
}

#[test]
fn declared_clock_input_requires_real_matching_owner_request_and_success_audit() {
    assert!(clock::SourceClock::new("20260830120000001").is_err());
    let clock = clock::SourceClock::new(oracle::UTC_INPUT).unwrap();
    let call = invocation();
    let store = MemoryStore::new(Default::default());
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
    assert!(clock_proof(&store, &owner, &clock).is_err());
    let request = effect(&call, 1);
    let resource = mainframe_env_host_api::canonical_audit_resource_digest(&request.request);
    assert_eq!(
        clock.invoke(&call, request).outcome,
        Ok(HostResult::Clock(oracle::UTC_INPUT.into()))
    );
    assert!(clock_proof(&store, &owner, &clock).is_err());
    store
        .record_audit(AuditRecord {
            execution_id: call.execution_id.clone(),
            run_unit_id: call.run_unit_id.clone(),
            attempt: call.attempt,
            effect_sequence: 1,
            observed_tick: 1,
            principal: call.principal.id().clone(),
            invocation_key: call.idempotency_key.clone(),
            capability: CapabilityId::new("host.clock", InvocationLimits::default()).unwrap(),
            resource,
            decision: AuditDecision::Success,
        })
        .unwrap();
    clock_proof(&store, &owner, &clock).unwrap();
    // Direct and journal effects can share a sequence/capability while
    // addressing distinct canonical resources. Require one exact resource.
    let valid = store
        .audit_records(&owner.execution_id, 0, MAX_ROWS + 1)
        .unwrap()
        .pop()
        .unwrap();
    let distinct_resource = mainframe_env_host_api::canonical_audit_resource_digest(
        &HostRequest::Clock(ClockRequest::Date),
    );
    assert_ne!(distinct_resource, resource);
    let mut unrelated = valid.clone();
    unrelated.resource = distinct_resource;
    store.record_audit(unrelated.clone()).unwrap();
    clock_proof(&store, &owner, &clock).unwrap();
    for change in 0..8 {
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
            6 => changed.invocation_key = key(),
            _ => {
                changed.execution_id =
                    ExecutionId::new("wrong-audit-execution", InvocationLimits::default()).unwrap();
            }
        }
        negative.record_audit(changed).unwrap();
        assert!(
            audit(
                &negative,
                &owner,
                1,
                "host.clock",
                &call.idempotency_key,
                &resource,
            )
            .is_err()
        );
    }
    // The optional failure receipt reports only a public compile-time site.
    let expected_failure_line = line!() + 1;
    let failure = need(false).unwrap_err();
    assert_eq!(failure.source_file, file!());
    assert_eq!(failure.source_line, expected_failure_line);
    assert!(failure.source_column > 0);
    let mut wrong = owner.clone();
    wrong.attempt += 1;
    assert!(clock_proof(&store, &wrong, &clock).is_err());
    wrong = owner.clone();
    wrong.run_unit_id = RunUnitId::new("wrong-run", InvocationLimits::default()).unwrap();
    assert!(clock_proof(&store, &wrong, &clock).is_err());
    // No per-request result substitution: a second clock observation refuses
    // the exact single ASKTIME contract, and the receiver has a hard32 bound.
    assert!(clock.invoke(&call, effect(&call, 2)).outcome.is_ok());
    assert!(clock_proof(&store, &owner, &clock).is_err());
    for sequence in 3..=32 {
        assert!(clock.invoke(&call, effect(&call, sequence)).outcome.is_ok());
    }
    assert_eq!(
        clock.invoke(&call, effect(&call, 33)).outcome,
        Err(HostProblem::ResourceExhausted)
    );
    assert_eq!(clock.observations().unwrap().len(), 32);
}

#[test]
fn complete_date_reply_comes_from_fixed_source_input_and_refuses_field_or_tail_drift() {
    let offered = oracle::offer().unwrap();
    assert_eq!(expected_date_reply(&offered).unwrap(), offered.reply);
    assert_eq!(offered.reply.len(), 1000);
    for index in [0, 14, 17, 20, 22, 36, 40, 42, 999] {
        let mut wrong = offered.reply.clone();
        wrong[index] ^= 1;
        assert!(oracle::compare_reply(&wrong).is_err());
    }
    assert!(oracle::compare_reply(&offered.reply[..999]).is_err());
    let mut longer = offered.reply.clone();
    longer.push(b' ');
    assert!(oracle::compare_reply(&longer).is_err());
    let mut changed = offered;
    changed.body[999] ^= 1;
    assert!(expected_date_reply(&changed).is_err());
}

#[test]
fn original_start_layout_and_nine_date_requests_keep_complete_wire_and_audit_binding() {
    let offered = oracle::offer().unwrap();
    let frame = start_payload(&offered.input_queue).unwrap();
    let mut independent = vec![b' '; 684];
    independent[..4].copy_from_slice(b"TM  ");
    independent[4..8].copy_from_slice(&[0, 0, 0, 1]);
    independent[8..30].copy_from_slice(b"CARD.DEMO.REQUEST.DATE");
    independent[168..172].copy_from_slice(&[0; 4]);
    assert_eq!(frame, independent);
    assert!(start_payload("CARD.DEMO.REQUEST.ACCT").is_err());
    for index in 0..9 {
        let value = expected_mq(index, 11, key(), [3, 4, 5], &offered, &offered.reply).unwrap();
        assert_eq!(
            HostRequest::Mq(value.clone())
                .required_capability(InvocationLimits::default())
                .as_str(),
            "host.mq.write"
        );
        let digest =
            mainframe_env_host_api::canonical_request_digest(&HostRequest::Mq(value.clone()))
                .unwrap();
        let resource = mainframe_env_host_api::canonical_audit_resource_digest(&HostRequest::Mq(
            value.clone(),
        ));
        bind_request_digests(&value, digest, &resource).unwrap();
        if matches!(index, 3 | 5) {
            assert!(value.message_id.is_none() && value.correlation_id.is_none());
            for message_id in [true, false] {
                let mut zero_filter = value.clone();
                if message_id {
                    zero_filter.message_id = Some(vec![0; 24]);
                } else {
                    zero_filter.correlation_id = Some(vec![0; 24]);
                }
                assert!(bind_request_digests(&zero_filter, digest, &resource).is_err());
            }
            assert_eq!(
                (value.options, value.wait_ticks, value.max_message_bytes),
                (24579, 5000, 1000)
            );
        }
        if index == 4 {
            assert_eq!(value.message, offered.reply);
            assert_eq!(
                value.message_id.as_deref(),
                Some(offered.message_id.as_slice())
            );
        }
        for change in 0..5 {
            let mut wrong = value.clone();
            match change {
                0 => wrong.message.push(0),
                1 => wrong.correlation_id = Some(vec![0x22; 24]),
                2 => wrong.wait_ticks += 1,
                3 => wrong.mutation.as_mut().unwrap().sequence += 1,
                _ => wrong.mutation.as_mut().unwrap().transaction = Some(TRANSACTION.into()),
            }
            assert!(bind_request_digests(&wrong, digest, &resource).is_err());
        }
        assert_eq!(
            expected_mq_result(index, [3, 4, 5], &offered)
                .unwrap()
                .reason_code,
            if index == 5 { 2033 } else { 0 }
        );
    }
}

#[test]
fn complete_queue_images_refuse_missing_extra_or_substituted_source_payloads() {
    let offered = oracle::offer().unwrap();
    compare_queues(
        std::slice::from_ref(&offered.body),
        &[],
        &[],
        &offered,
        None,
    )
    .unwrap();
    compare_queues(
        &[],
        std::slice::from_ref(&offered.reply),
        &[],
        &offered,
        Some(&offered.reply),
    )
    .unwrap();
    assert!(compare_queues(&[], &[], &[], &offered, None).is_err());
    assert!(
        compare_queues(
            &[],
            &[offered.reply.clone(), offered.reply.clone()],
            &[],
            &offered,
            Some(&offered.reply)
        )
        .is_err()
    );
    assert!(
        compare_queues(
            &[],
            std::slice::from_ref(&offered.body),
            &[],
            &offered,
            Some(&offered.reply)
        )
        .is_err()
    );
    assert!(
        compare_queues(
            &[],
            std::slice::from_ref(&offered.reply),
            &[Vec::new()],
            &offered,
            Some(&offered.reply)
        )
        .is_err()
    );
}

#[test]
fn completed_work_requires_both_activity_counters_before_complete_source_proof() {
    assert!(ready_to_compare(WorkState::Completed, 0, 0));
    for state in [WorkState::Queued, WorkState::Claimed, WorkState::DeadLetter] {
        assert!(!ready_to_compare(state, 0, 0));
    }
    for (active, jes) in [(1, 0), (0, 1), (1, 1)] {
        assert!(!ready_to_compare(WorkState::Completed, active, jes));
    }
}

#[test]
#[ignore = "requires pinned first-party source and Root's separate original owning native slot"]
fn installed_codate_start_uses_declared_clock_and_compares_full_reply_restart_and_teardown() {
    let receipt = verify_from_env().unwrap();
    for field in [
        "actual_installed_guest_compared",
        "host_clock_input_compared",
        "pending_request_restart_compared",
        "post_guest_restart_compared",
        "guest_proof_rechecked_after_physical_reopen",
        "reopened_reply_provider_client_read_compared",
        "seed_server_drain",
        "guest_server_drain",
        "reopened_server_drain",
        "cleanup_verified",
    ] {
        assert_eq!(receipt[field], true);
    }
    assert_eq!(receipt["declared_host_clock_input"], oracle::UTC_INPUT);
    assert_eq!(receipt["physical_sqlite_reopens"], 2);
    for field in [
        "actual_wait_duration_claimed",
        "automatic_mq_trigger_listener_claimed",
        "whole_corpus_git_checked",
    ] {
        assert_eq!(receipt[field], false);
    }
    for field in ["application_credit", "vendor_credit", "acceptance_credit"] {
        assert_eq!(receipt[field], 0);
    }
}
