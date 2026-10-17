//! Predicate/opaque-handle controls only; no synthesized product journey credit.
use super::*;
use mainframe_env_execution_api::{AuditResourceDigestFormat, ResourceLimits};
use mainframe_env_ir::{CicsStorageSlot, IrLimits, ModuleBuilder};

mod journal;

fn invocation() -> Invocation {
    let limits = InvocationLimits::default();
    Invocation::new(
        RequestId::new("observer-request", limits).unwrap(),
        ExecutionId::new("observer-execution", limits).unwrap(),
        RunUnitId::new("observer-run", limits).unwrap(),
        None,
        Selector::new("program:FIXTURE", limits).unwrap(),
        ArtifactRef::new("observer-artifact", limits).unwrap(),
        Principal::new(
            PrincipalId::new("FIXTUREUSER", limits).unwrap(),
            BTreeSet::new(),
            limits,
        )
        .unwrap(),
        ServiceClass::Interactive,
        0,
        100,
        TraceId::new("observer-trace", limits).unwrap(),
        IdempotencyKey::new("observer-key", limits).unwrap(),
        1,
        ResourceLimits::default(),
        BTreeMap::new(),
        limits,
    )
    .unwrap()
}

fn execution(invocation: &Invocation) -> ExecutionRecord {
    ExecutionRecord {
        execution_id: invocation.execution_id.clone(),
        run_unit_id: invocation.run_unit_id.clone(),
        selector: invocation.selector.clone(),
        artifact: invocation.artifact.clone(),
        principal: invocation.principal.id().clone(),
        state: ExecutionState::Completed,
        attempt: 1,
        version: 9,
        owner_lease: None,
        lease_expiry_tick: None,
        terminal_tick: Some(20),
    }
}

fn events(invocation: &Invocation) -> Vec<LifecycleEvent> {
    let mut events = Vec::new();
    for effect in 1..=4 {
        for (kind, tick) in [
            (LifecycleEventKind::EffectIntent { sequence: effect }, 10),
            (LifecycleEventKind::EffectResult { sequence: effect }, 15),
        ] {
            events.push(LifecycleEvent {
                execution_id: invocation.execution_id.clone(),
                run_unit_id: invocation.run_unit_id.clone(),
                attempt: invocation.attempt,
                sequence: events.len() as u64 + 1,
                tick,
                kind,
            });
        }
    }
    // Keep this synthetic event clock monotone without deriving it from a live result.
    for (index, event) in events.iter_mut().enumerate() {
        event.tick = 10 + index as u64;
    }
    events.push(LifecycleEvent {
        execution_id: invocation.execution_id.clone(),
        run_unit_id: invocation.run_unit_id.clone(),
        sequence: 9,
        attempt: 1,
        tick: 20,
        kind: LifecycleEventKind::Completed { return_code: 0 },
    });
    events
}

fn resource() -> AuditResourceDigest {
    AuditResourceDigest {
        format: AuditResourceDigestFormat::CanonicalHostResourceV1,
        value: [7; 32],
    }
}

fn audit(invocation: &Invocation, sequence: u64) -> AuditRecord {
    AuditRecord {
        execution_id: invocation.execution_id.clone(),
        run_unit_id: invocation.run_unit_id.clone(),
        attempt: invocation.attempt,
        effect_sequence: sequence,
        observed_tick: 10 + (sequence - 1) * 2,
        principal: invocation.principal.id().clone(),
        invocation_key: invocation.idempotency_key.clone(),
        capability: CapabilityId::new("host.cics.execute", InvocationLimits::default()).unwrap(),
        resource: resource(),
        decision: AuditDecision::Success,
    }
}

fn state() -> Arc<ArmState> {
    let invocation = invocation();
    let mut builder = ModuleBuilder::new(IrLimits::default());
    let id = builder.add_storage("RESULT", 4, None).unwrap();
    let slot = mainframe_env_interpreter::CicsReadCopybackSlotSelection::new(
        CicsStorageSlot {
            storage: id,
            qualified_layout_name: "RESULT".into(),
        },
        4,
    )
    .unwrap();
    let plan = CicsReadCopybackPlan::new(vec![
        mainframe_env_interpreter::CicsReadCopybackSelection::new(slot, None, None),
    ])
    .unwrap();
    let profile = SourceBoundReadCopybackProfile::new(
        SessionId::new("observer-session", 128).unwrap(),
        invocation.selector,
        invocation.principal.id().clone(),
        invocation.artifact,
        "synthetic-semantic-identity".into(),
        [3; 32],
        plan,
    )
    .unwrap();
    Arc::new(ArmState {
        profile,
        owner: OnceLock::new(),
        result: OnceLock::new(),
    })
}

#[test]
fn completed_actor_identity_state_lease_and_clock_substitutions_refuse() {
    let invocation = invocation();
    let expected = execution(&invocation);
    assert!(owner_matches(&expected, &invocation));
    for field in 0..12 {
        let mut changed = expected.clone();
        let limits = InvocationLimits::default();
        match field {
            0 => changed.execution_id = ExecutionId::new("foreign", limits).unwrap(),
            1 => changed.run_unit_id = RunUnitId::new("foreign", limits).unwrap(),
            2 => changed.principal = PrincipalId::new("foreign", limits).unwrap(),
            3 => changed.selector = Selector::new("program:FOREIGN", limits).unwrap(),
            4 => changed.artifact = ArtifactRef::new("foreign", limits).unwrap(),
            5 => changed.attempt += 1,
            6 => changed.state = ExecutionState::Suspended,
            7 => changed.version = 0,
            8 => changed.owner_lease = Some("foreign".into()),
            9 => changed.lease_expiry_tick = Some(30),
            10 => changed.terminal_tick = None,
            11 => changed.terminal_tick = Some(invocation.deadline_tick + 1),
            _ => unreachable!(),
        }
        assert!(!owner_matches(&changed, &invocation));
    }
}

#[test]
fn exact_paired_effect_events_and_ordinary_terminal_refuse_cross_kind_or_extra_receipts() {
    let invocation = invocation();
    let execution = execution(&invocation);
    let expected = events(&invocation);
    assert!(events_match(&expected, &execution, &[1, 2, 3], 4));
    for kind in [
        LifecycleEventKind::HandoffCompleted,
        LifecycleEventKind::Suspended,
        LifecycleEventKind::Failed,
        LifecycleEventKind::Completed { return_code: 4 },
    ] {
        let mut changed = expected.clone();
        changed.last_mut().unwrap().kind = kind;
        assert!(!events_match(&changed, &execution, &[1, 2, 3], 4));
    }
    let mut missing = expected.clone();
    missing.remove(1);
    assert!(!events_match(&missing, &execution, &[1, 2, 3], 4));
    let mut duplicate = expected.clone();
    duplicate.insert(1, expected[0].clone());
    assert!(!events_match(&duplicate, &execution, &[1, 2, 3], 4));
    for field in 0..5 {
        let mut changed = expected.clone();
        match field {
            0 => {
                changed[0].execution_id =
                    ExecutionId::new("foreign", InvocationLimits::default()).unwrap()
            }
            1 => {
                changed[0].run_unit_id =
                    RunUnitId::new("foreign", InvocationLimits::default()).unwrap()
            }
            2 => changed[0].attempt += 1,
            3 => changed[0].sequence = 0,
            4 => changed.last_mut().unwrap().tick += 1,
            _ => unreachable!(),
        }
        assert!(!events_match(&changed, &execution, &[1, 2, 3], 4));
    }
}

#[test]
fn selected_outer_audit_identity_resource_decision_and_duplicate_corruptions_refuse() {
    let invocation = invocation();
    let events = events(&invocation);
    let expected = (1..=4)
        .map(|sequence| audit(&invocation, sequence))
        .collect::<Vec<_>>();
    let effects = (1..=4)
        .map(|sequence| (sequence, resource()))
        .collect::<Vec<_>>();
    assert!(audits_match(&expected, &invocation, &effects, &events));
    for field in 0..10 {
        let mut changed = expected.clone();
        let limits = InvocationLimits::default();
        match field {
            0 => changed[0].execution_id = ExecutionId::new("foreign", limits).unwrap(),
            1 => changed[0].run_unit_id = RunUnitId::new("foreign", limits).unwrap(),
            2 => changed[0].principal = PrincipalId::new("foreign", limits).unwrap(),
            3 => changed[0].invocation_key = IdempotencyKey::new("foreign", limits).unwrap(),
            4 => changed[0].attempt += 1,
            5 => changed[0].effect_sequence += 1,
            6 => changed[0].resource.value[0] ^= 1,
            7 => changed[0].decision = AuditDecision::UnknownOutcome,
            8 => changed[0].capability = CapabilityId::new("host.dataset.read", limits).unwrap(),
            9 => changed[0].observed_tick = 99,
            _ => unreachable!(),
        }
        assert!(!audits_match(&changed, &invocation, &effects, &events));
    }
    let mut duplicate = expected.clone();
    let mut conflicting = expected[0].clone();
    conflicting.resource.value[0] ^= 1;
    duplicate.push(conflicting);
    assert!(!audits_match(&duplicate, &invocation, &effects, &events));
    let mut nested = expected.clone();
    let mut inner = expected[0].clone();
    inner.capability = CapabilityId::new("host.dataset.read", InvocationLimits::default()).unwrap();
    inner.observed_tick = 50;
    nested.push(inner);
    assert!(audits_match(&nested, &invocation, &effects, &events));

    exact_captured_terminal_admission_uses_preclaim_selector_and_preserves_program_audits();
    terminal_admission_wrong_actor_key_resource_count_domain_and_program_zero_tick_refuse();
}

#[test]
fn dropped_real_binding_latches_cleanup_refusal_and_cannot_publish_rows() {
    let state = state();
    let observation = ArmedReadCopybackObservation {
        state: state.clone(),
    };
    assert!(matches!(
        observation.completed(),
        Err(CicsReadObservationProblem::Pending)
    ));
    drop(BoundObservation {
        state,
        terminal_admission: None,
        finished: false,
    });
    assert!(matches!(
        observation.completed(),
        Err(CicsReadObservationProblem::Cleanup)
    ));
}

#[test]
fn copied_profile_or_owner_strings_never_mint_a_completed_observation() {
    let state = state();
    let invocation = invocation();
    state
        .owner
        .set((
            invocation.execution_id,
            invocation.run_unit_id,
            invocation.idempotency_key,
            invocation.attempt,
        ))
        .unwrap();
    let observation = ArmedReadCopybackObservation {
        state: state.clone(),
    };
    assert!(matches!(
        observation.completed(),
        Err(CicsReadObservationProblem::Pending)
    ));
    let _ = state.result.set(Err(CicsReadObservationProblem::Ambiguous));
    assert!(matches!(
        observation.completed(),
        Err(CicsReadObservationProblem::Ambiguous)
    ));
}

#[test]
fn bounded_event_and_audit_pages_refuse_cap_plus_one_without_truncation() {
    let invocation = invocation();
    let execution = execution(&invocation);
    let events = events(&invocation);
    let oversized = vec![events[0].clone(); MAX_EVENTS + 1];
    assert!(!events_match(&oversized, &execution, &[1, 2, 3], 4));
    let oversized = vec![audit(&invocation, 1); MAX_AUDITS + 1];
    assert!(!audits_match(
        &oversized,
        &invocation,
        &[(1, resource())],
        &events
    ));
}

fn terminal_context_fixture() -> (Invocation, Invocation, TerminalAdmission, AuditRecord) {
    let limits = InvocationLimits::default();
    let mut context = invocation();
    context.selector = Selector::new("cics:TX00", limits).unwrap();
    // A continuation's resumed transaction is deliberately different. The
    // provider admitted TX00 before claiming NX00; the root selector is retained.
    context.bindings.insert(
        "cics.transaction".into(),
        BoundedPayload::new("mainframe-env.cics.transaction@1", b"NX00".to_vec(), limits).unwrap(),
    );
    let captured = TerminalAdmission::capture(&context).unwrap();
    let mut program = context.clone();
    program.selector = Selector::new("program:FIXTURE", limits).unwrap();
    program.idempotency_key = IdempotencyKey::new("observer-key:FIXTURE", limits).unwrap();
    let request = HostRequest::Security(SecurityRequest::Authorize {
        principal: context.principal.id().clone(),
        class: "TCICSTRN".into(),
        resource: ResourceName::new("CICS.TX00", 246).unwrap(),
        intent: AccessIntent::Execute,
    });
    let mut admission = audit(&context, 1);
    admission.observed_tick = 0;
    admission.capability = CapabilityId::new("host.security.authorize", limits).unwrap();
    admission.resource = canonical_audit_resource_digest(&request);
    (context, program, captured, admission)
}

fn exact_captured_terminal_admission_uses_preclaim_selector_and_preserves_program_audits() {
    let (context, program, captured, admission) = terminal_context_fixture();
    let events = events(&program);
    let effects = (1..=4)
        .map(|sequence| (sequence, resource()))
        .collect::<Vec<_>>();
    let mut records = (1..=4)
        .map(|sequence| audit(&program, sequence))
        .collect::<Vec<_>>();
    records.push(admission);
    assert!(captured.matches_actor(&program));
    assert_eq!(captured.invocation_key, context.idempotency_key);
    assert_eq!(captured.resource, records.last().unwrap().resource);
    assert!(audits_match_with_terminal_admission(
        &records,
        &program,
        &effects,
        &events,
        Some(&captured)
    ));
    assert!(!audits_match_with_terminal_admission(
        &records, &program, &effects, &events, None
    ));
    assert!(TerminalAdmission::capture(&program).is_none());
    for selector in [
        "cics:",
        "cics:lower",
        "cics:WITH.DOT",
        "cics:ABCDEFGHIJKLMNOPQ",
    ] {
        let mut changed = context.clone();
        changed.selector = Selector::new(selector, InvocationLimits::default()).unwrap();
        assert!(TerminalAdmission::capture(&changed).is_none());
    }
    let mut transferred = program.clone();
    transferred.execution_id =
        ExecutionId::new("transferred-actor", InvocationLimits::default()).unwrap();
    assert!(!captured.matches_actor(&transferred));
    assert!(!audits_match_with_terminal_admission(
        &records,
        &transferred,
        &effects,
        &events,
        Some(&captured)
    ));
    let mut zero = records;
    zero[0].observed_tick = 0;
    assert!(!audits_match_with_terminal_admission(
        &zero,
        &program,
        &effects,
        &events,
        Some(&captured)
    ));
}

fn terminal_admission_wrong_actor_key_resource_count_domain_and_program_zero_tick_refuse() {
    let (_, program, captured, admission) = terminal_context_fixture();
    let limits = InvocationLimits::default();
    let events = events(&program);
    let effects = (1..=4)
        .map(|sequence| (sequence, resource()))
        .collect::<Vec<_>>();
    let mut records = (1..=4)
        .map(|sequence| audit(&program, sequence))
        .collect::<Vec<_>>();
    records.push(admission);
    let last = records.len() - 1;
    let mut variants = Vec::new();
    let mut changed = records.clone();
    changed.pop();
    variants.push(changed);
    let mut changed = records.clone();
    changed.push(records[last].clone());
    variants.push(changed);
    for field in 0..10 {
        let mut changed = records.clone();
        match field {
            0 => changed[last].execution_id = ExecutionId::new("foreign", limits).unwrap(),
            1 => changed[last].run_unit_id = RunUnitId::new("foreign", limits).unwrap(),
            2 => changed[last].principal = PrincipalId::new("foreign", limits).unwrap(),
            3 => changed[last].attempt += 1,
            4 => changed[last].invocation_key = IdempotencyKey::new("foreign", limits).unwrap(),
            5 => changed[last].effect_sequence = 2,
            6 => changed[last].observed_tick = 1,
            7 => changed[last].decision = AuditDecision::UnknownOutcome,
            8 => changed[last].capability = CapabilityId::new("host.dataset.read", limits).unwrap(),
            9 => {
                changed[last].resource.format =
                    AuditResourceDigestFormat::CanonicalHostOversizedResourceV1
            }
            _ => unreachable!(),
        }
        variants.push(changed);
    }
    for (class, resource_name, intent) in [
        ("TCICSTRN", "CICS.NX00", AccessIntent::Execute),
        ("FACILITY", "CICS.TX00", AccessIntent::Execute),
        ("TCICSTRN", "CICS.TX00", AccessIntent::Read),
    ] {
        let mut changed = records.clone();
        changed[last].resource =
            canonical_audit_resource_digest(&HostRequest::Security(SecurityRequest::Authorize {
                principal: program.principal.id().clone(),
                class: class.into(),
                resource: ResourceName::new(resource_name, 246).unwrap(),
                intent,
            }));
        variants.push(changed);
    }
    let mut changed = records.clone();
    changed[0].invocation_key = captured.invocation_key.clone();
    variants.push(changed);
    let mut changed = records.clone();
    changed[0].observed_tick = 0;
    variants.push(changed);
    for changed in variants {
        assert!(!audits_match_with_terminal_admission(
            &changed,
            &program,
            &effects,
            &events,
            Some(&captured)
        ));
    }
    for field in 0..5 {
        let mut changed = captured.clone();
        match field {
            0 => changed.execution = ExecutionId::new("foreign", limits).unwrap(),
            1 => changed.run = RunUnitId::new("foreign", limits).unwrap(),
            2 => changed.principal = PrincipalId::new("foreign", limits).unwrap(),
            3 => changed.attempt += 1,
            4 => changed.invocation_key = IdempotencyKey::new("foreign", limits).unwrap(),
            _ => unreachable!(),
        }
        assert!(!audits_match_with_terminal_admission(
            &records,
            &program,
            &effects,
            &events,
            Some(&changed)
        ));
    }
}
