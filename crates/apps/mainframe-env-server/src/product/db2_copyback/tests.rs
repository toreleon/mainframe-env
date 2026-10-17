//! Predicate/opaque-handle controls only; no synthesized product journey credit.
use super::*;
use mainframe_env_execution_api::{AuditResourceDigestFormat, ResourceLimits};
use mainframe_env_ir::{IrLimits, ModuleBuilder};
use mainframe_env_store_api::EffectIntentMetadata;

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
        capability: CapabilityId::new("host.db2.write", InvocationLimits::default()).unwrap(),
        resource: resource(),
        decision: AuditDecision::Success,
    }
}

fn state() -> Arc<ArmState> {
    let invocation = invocation();
    let mut builder = ModuleBuilder::new(IrLimits::default());
    let id = builder.add_storage("RESULT", 4, None).unwrap();
    let slot = mainframe_env_interpreter::Db2CopybackSlot::new(id, "RESULT".into(), 4).unwrap();
    let plan = Db2CopybackPlan::new(vec![Db2Operation::FetchCursor], vec![slot]).unwrap();
    let profile = SourceBoundDb2CopybackProfile::new(
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
    assert!(events_match(&expected, &execution, &[1, 2, 3, 4]));
    for kind in [
        LifecycleEventKind::HandoffCompleted,
        LifecycleEventKind::Suspended,
        LifecycleEventKind::Failed,
        LifecycleEventKind::Completed { return_code: 4 },
    ] {
        let mut changed = expected.clone();
        changed.last_mut().unwrap().kind = kind;
        assert!(!events_match(&changed, &execution, &[1, 2, 3, 4]));
    }
    let mut missing = expected.clone();
    missing.remove(1);
    assert!(!events_match(&missing, &execution, &[1, 2, 3, 4]));
    let mut duplicate = expected.clone();
    duplicate.insert(1, expected[0].clone());
    assert!(!events_match(&duplicate, &execution, &[1, 2, 3, 4]));
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
        assert!(!events_match(&changed, &execution, &[1, 2, 3, 4]));
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
        .map(|sequence| statement(&invocation, sequence))
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
            8 => changed[0].capability = CapabilityId::new("host.db2.read", limits).unwrap(),
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
}

#[test]
fn dropped_real_binding_latches_cleanup_refusal_and_cannot_publish_rows() {
    let state = state();
    let observation = ArmedDb2CopybackObservation {
        state: state.clone(),
    };
    assert!(matches!(
        observation.completed(),
        Err(Db2ObservationProblem::Pending)
    ));
    drop(BoundObservation {
        state,
        finished: false,
    });
    assert!(matches!(
        observation.completed(),
        Err(Db2ObservationProblem::Cleanup)
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
    let observation = ArmedDb2CopybackObservation {
        state: state.clone(),
    };
    assert!(matches!(
        observation.completed(),
        Err(Db2ObservationProblem::Pending)
    ));
    let _ = state.result.set(Err(Db2ObservationProblem::Ambiguous));
    assert!(matches!(
        observation.completed(),
        Err(Db2ObservationProblem::Ambiguous)
    ));
}

#[test]
fn bounded_event_and_audit_pages_refuse_cap_plus_one_without_truncation() {
    let invocation = invocation();
    let execution = execution(&invocation);
    let events = events(&invocation);
    let oversized = vec![events[0].clone(); MAX_EVENTS + 1];
    assert!(!events_match(&oversized, &execution, &[1, 2, 3, 4]));
    let oversized = vec![audit(&invocation, 1); MAX_AUDITS + 1];
    assert!(!audits_match(
        &oversized,
        &invocation,
        &[statement(&invocation, 1)],
        &events
    ));
}

const REQUEST: [u8; 32] = [5; 32];
const RESULT: [u8; 32] = [6; 32];
fn statement(invocation: &Invocation, sequence: u64) -> StatementIdentity<'_> {
    StatementIdentity {
        operation: Db2Operation::FetchCursor,
        sequence,
        resource: resource(),
        key: Some(&invocation.idempotency_key),
        request: &REQUEST,
        result: &RESULT,
    }
}
fn core(invocation: &Invocation) -> EffectRecord {
    EffectRecord {
        execution_id: invocation.execution_id.clone(),
        run_unit_id: invocation.run_unit_id.clone(),
        sequence: 1,
        key: invocation.idempotency_key.clone(),
        digest_format: EffectDigestFormat::CanonicalHostV1,
        request_digest: REQUEST,
        result_digest: Some(RESULT),
        state: EffectState::Completed,
        resolved_tick: Some(15),
        intent: EffectIntentMetadata {
            owner: invocation.execution_id.clone(),
            attempt: invocation.attempt,
            capability: Some(
                CapabilityId::new("host.db2.write", InvocationLimits::default()).unwrap(),
            ),
            audit_resource: Some(resource()),
            audit_invocation_key: Some(invocation.idempotency_key.clone()),
            created_tick: 10,
            recovery_after_tick: invocation.deadline_tick,
            epoch: 1,
            recovery_lease: None,
        },
    }
}
fn replay(invocation: &Invocation) -> Db2ReplayRetentionDescriptor {
    Db2ReplayRetentionDescriptor {
        namespace: "db2-v1-replay".into(),
        key: invocation.idempotency_key.as_str().into(),
        row_version: 2,
        payload_digest: [8; 32],
        retention: Db2ReplayRetentionState::Terminal,
        owner_execution: Some(invocation.execution_id.as_str().into()),
        owner_run_unit: Some(invocation.run_unit_id.as_str().into()),
        outer_effect_key: None,
        sequence: Some(1),
        recorded_deadline_tick: Some(invocation.deadline_tick),
        resolution_tick: Some(invocation.deadline_tick),
        terminal_tick: Some(invocation.deadline_tick),
        request_digest: REQUEST,
        result_digest: RESULT,
        dependency: Some(Db2ReplayDependency::CoreEffect),
    }
}
#[test]
fn canonical_mutating_core_owner_digests_state_resolution_and_audit_binding_refuse_drift() {
    let invocation = invocation();
    let selected = statement(&invocation, 1);
    let expected = core(&invocation);
    assert!(core_matches(&expected, &invocation, &selected, 20));
    for field in 0..15 {
        let mut changed = expected.clone();
        let limits = InvocationLimits::default();
        match field {
            0 => changed.execution_id = ExecutionId::new("foreign", limits).unwrap(),
            1 => changed.run_unit_id = RunUnitId::new("foreign", limits).unwrap(),
            2 => changed.sequence += 1,
            3 => changed.key = IdempotencyKey::new("foreign", limits).unwrap(),
            4 => changed.digest_format = EffectDigestFormat::LegacyDebug,
            5 => changed.request_digest[0] ^= 1,
            6 => changed.result_digest = None,
            7 => changed.result_digest.as_mut().unwrap()[0] ^= 1,
            8 => changed.state = EffectState::Intent,
            9 => changed.intent.owner = ExecutionId::new("foreign", limits).unwrap(),
            10 => changed.intent.attempt += 1,
            11 => changed.intent.audit_invocation_key = None,
            12 => changed.intent.audit_resource.as_mut().unwrap().value[0] ^= 1,
            13 => {
                changed.intent.capability =
                    Some(CapabilityId::new("host.db2.read", limits).unwrap())
            }
            14 => changed.resolved_tick = Some(21),
            _ => unreachable!(),
        }
        assert!(!core_matches(&changed, &invocation, &selected, 20));
    }
}
#[test]
fn exact_terminal_core_db2_replay_refuses_pending_foreign_nested_and_digest_drift() {
    let invocation = invocation();
    let selected = statement(&invocation, 1);
    let expected = replay(&invocation);
    assert!(replay_matches(&expected, &invocation, &selected));
    for field in 0..13 {
        let mut changed = expected.clone();
        match field {
            0 => changed.namespace.push('X'),
            1 => changed.key.push('X'),
            2 => changed.row_version = 0,
            3 => changed.retention = Db2ReplayRetentionState::PendingProtected,
            4 => changed.owner_execution = None,
            5 => changed.owner_run_unit = Some("foreign".into()),
            6 => changed.outer_effect_key = Some("foreign".into()),
            7 => changed.sequence = Some(2),
            8 => changed.recorded_deadline_tick = Some(invocation.deadline_tick + 1),
            9 => changed.resolution_tick = Some(invocation.deadline_tick - 1),
            10 => changed.terminal_tick = None,
            11 => changed.request_digest[0] ^= 1,
            12 => changed.result_digest[0] ^= 1,
            _ => unreachable!(),
        }
        assert!(!replay_matches(&changed, &invocation, &selected));
    }
    let mut nested = expected;
    nested.dependency = Some(Db2ReplayDependency::CicsNested {
        run_unit: invocation.run_unit_id.as_str().into(),
        sequence: 1,
        outer_effect_key: invocation.idempotency_key.as_str().into(),
    });
    assert!(!replay_matches(&nested, &invocation, &selected));
}
#[test]
fn read_only_select_uses_actual_paired_events_and_read_audit_without_fake_core_receipt() {
    let invocation = invocation();
    let events = events(&invocation);
    let selected = StatementIdentity {
        operation: Db2Operation::Select,
        key: None,
        ..statement(&invocation, 1)
    };
    let mut read = audit(&invocation, 1);
    read.capability = CapabilityId::new("host.db2.read", InvocationLimits::default()).unwrap();
    assert!(audits_match(
        std::slice::from_ref(&read),
        &invocation,
        &[selected],
        &events
    ));
    assert!(!core_matches(
        &core(&invocation),
        &invocation,
        &selected,
        20
    ));
    assert!(!replay_matches(
        &replay(&invocation),
        &invocation,
        &selected
    ));
    read.capability = CapabilityId::new("host.db2.write", InvocationLimits::default()).unwrap();
    assert!(!audits_match(&[read], &invocation, &[selected], &events));
}
