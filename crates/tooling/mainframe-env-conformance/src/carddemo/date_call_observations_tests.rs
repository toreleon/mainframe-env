use super::*;
use mainframe_env_execution_api::AuditResourceDigestFormat;
use mainframe_env_store_api::EffectIntentMetadata;

fn actor() -> ExecutionRecord {
    let limits = InvocationLimits::default();
    ExecutionRecord {
        execution_id: ExecutionId::new("actual-source-caller", limits).unwrap(),
        run_unit_id: RunUnitId::new("actual-source-run", limits).unwrap(),
        selector: Selector::new("program:COTRN02C", limits).unwrap(),
        artifact: ArtifactRef::new("sha256:source-published", limits).unwrap(),
        principal: PrincipalId::new("WEBUSER", limits).unwrap(),
        state: ExecutionState::Completed,
        attempt: 1,
        version: 20,
        owner_lease: None,
        lease_expiry_tick: None,
        terminal_tick: Some(100),
    }
}
fn sample() -> (
    ExecutionRecord,
    AuditRecord,
    EffectRecord,
    HostRequest,
    BoundedPayload,
    IdempotencyKey,
) {
    let limits = InvocationLimits::default();
    let actor = actor();
    let request = utility_request(FEB2026).unwrap();
    let result = utility_result(FEB2026).unwrap();
    let invocation_key = IdempotencyKey::new("caller-invocation", limits).unwrap();
    let key = IdempotencyKey::new("caller-invocation:6", limits).unwrap();
    let capability = CapabilityId::new("host.program.invoke", limits).unwrap();
    let resource = canonical_audit_resource_digest(&request);
    let audit = AuditRecord {
        execution_id: actor.execution_id.clone(),
        run_unit_id: actor.run_unit_id.clone(),
        principal: actor.principal.clone(),
        attempt: actor.attempt,
        invocation_key: invocation_key.clone(),
        effect_sequence: 6,
        observed_tick: 90,
        capability: capability.clone(),
        resource,
        decision: AuditDecision::Success,
    };
    let effect = EffectRecord {
        execution_id: actor.execution_id.clone(),
        run_unit_id: actor.run_unit_id.clone(),
        sequence: 6,
        key: key.clone(),
        digest_format: EffectDigestFormat::CanonicalHostV1,
        request_digest: canonical_request_digest(&request).unwrap(),
        intent: EffectIntentMetadata {
            owner: actor.execution_id.clone(),
            attempt: 1,
            capability: Some(capability),
            audit_resource: Some(resource),
            audit_invocation_key: Some(invocation_key),
            created_tick: 80,
            recovery_after_tick: 80,
            epoch: 1,
            recovery_lease: None,
        },
        state: EffectState::Completed,
        result_digest: Some(
            canonical_result_digest(&Ok(HostResult::Program(result.clone()))).unwrap(),
        ),
        resolved_tick: Some(90),
    };
    (actor, audit, effect, request, result, key)
}

#[test]
fn complete_date_result_keeps_binary_prefix_and_refuses_bad_bytes_or_argument_order() {
    // Independent complete 80-byte source field layout: 4/11/4/1/15/1/9/10/1/10/10/1/3.
    let expected =
        b"0000Mesg Code: 0000 Date is valid   TstDate: \x00\x0a2026-02- Mask used:YYYY-MM-DD    ";
    assert_eq!(expected.len(), 80);
    assert_eq!(message(FEB2026), expected);
    let invalid =
        b"0003Mesg Code: 0001 Date is invalid TstDate: \x00\x0a2026-13- Mask used:YYYY-MM-DD    ";
    assert_eq!(invalid.len(), 80);
    assert_eq!(message(MONTH13), invalid);
    let (actor, audit, effect, request, result, key) = sample();
    effect_matches(&effect, &actor, &audit, &request, &result, &key).unwrap();
    for at in [0, 4, 14, 18, 20, 35, 44, 45, 54, 64, 75, 79] {
        let mut changed = expected.to_vec();
        changed[at] ^= 1;
        let wrong = result_payload(&[FEB2026.text.to_vec(), MASK.to_vec(), changed]).unwrap();
        assert!(effect_matches(&effect, &actor, &audit, &request, &wrong, &key).is_err());
    }
    let text_substitution = result_payload(&[
        FEB2026.text.to_vec(),
        MASK.to_vec(),
        b"0000Mesg Code: 0000 Date is valid   TstDate: 2026-02-28 Mask used:YYYY-MM-DD    "
            .to_vec(),
    ])
    .unwrap();
    assert!(effect_matches(&effect, &actor, &audit, &request, &text_substitution, &key).is_err());
    let reordered =
        result_payload(&[MASK.to_vec(), FEB2026.text.to_vec(), expected.to_vec()]).unwrap();
    assert!(effect_matches(&effect, &actor, &audit, &request, &reordered, &key).is_err());
    let short = result_payload(&[
        FEB2026.text.to_vec(),
        MASK.to_vec(),
        expected[..79].to_vec(),
    ])
    .unwrap();
    assert!(effect_matches(&effect, &actor, &audit, &request, &short, &key).is_err());
}

#[test]
fn exact_date_effect_refuses_foreign_owner_unknown_outcome_and_changed_offer() {
    let (actor, audit, effect, request, result, key) = sample();
    for case in 0..12 {
        let mut changed = effect.clone();
        match case {
            0 => {
                changed.execution_id =
                    ExecutionId::new("foreign-caller", InvocationLimits::default()).unwrap()
            }
            1 => {
                changed.run_unit_id =
                    RunUnitId::new("foreign-run", InvocationLimits::default()).unwrap()
            }
            2 => {
                changed.intent.owner =
                    ExecutionId::new("foreign-owner", InvocationLimits::default()).unwrap()
            }
            3 => changed.intent.attempt += 1,
            4 => changed.state = EffectState::UnknownOutcome,
            5 => changed.state = EffectState::Intent,
            6 => changed.digest_format = EffectDigestFormat::LegacyDebug,
            7 => changed.sequence += 1,
            8 => changed.result_digest = None,
            9 => changed.resolved_tick = Some(101),
            10 => {
                changed.intent.audit_resource.as_mut().unwrap().format =
                    AuditResourceDigestFormat::CanonicalHostOversizedResourceV1
            }
            _ => changed.intent.audit_invocation_key = None,
        }
        assert!(
            effect_matches(&changed, &actor, &audit, &request, &result, &key).is_err(),
            "case {case}"
        );
    }
    let mut changed = request.clone();
    if let HostRequest::Program(ProgramRequest::Call { payload, .. }) = &mut changed {
        *payload = argument_payload(
            NAMES,
            &[MAR2026.text.to_vec(), MASK.to_vec(), vec![b' '; 80]],
        )
        .unwrap();
    }
    assert!(effect_matches(&effect, &actor, &audit, &changed, &result, &key).is_err());
    let wrong_key = IdempotencyKey::new("copied-effect:6", InvocationLimits::default()).unwrap();
    assert!(effect_matches(&effect, &actor, &audit, &request, &result, &wrong_key).is_err());
}

#[test]
fn source_case_order_and_freshness_refuse_unoffered_dates_or_historical_mutation() {
    assert!(cases("2026/02/28", "2026-02-28").unwrap().is_empty());
    assert_eq!(cases("2026-13-01", "2026-02-28").unwrap().len(), 1);
    let expected = cases("2026-02-28", "2026-03-01").unwrap();
    assert_eq!(expected[0].text, b"2026-02-28");
    assert_eq!(expected[1].text, b"2026-03-01");
    assert!(cases("2026-02-28", "2026-02-28").is_err());
    let row = ProviderStateRecord {
        namespace: CALLS.into(),
        key: "original".into(),
        version: 2,
        payload: vec![1],
    };
    let before = BTreeMap::from([(row.key.clone(), row.clone())]);
    assert!(fresh(&before, &before).unwrap().is_empty());
    assert!(fresh(&before, &BTreeMap::new()).is_err());
    for case in 0..2 {
        let mut changed = row.clone();
        if case == 0 {
            changed.version += 1;
        } else {
            changed.payload[0] ^= 1;
        }
        assert!(fresh(&before, &BTreeMap::from([(changed.key.clone(), changed)])).is_err());
    }
    let first = cee_request(FEB2026, 0).unwrap();
    let second = cee_request(FEB2026, 1).unwrap();
    assert_ne!(
        canonical_request_digest(&first).unwrap(),
        canonical_request_digest(&second).unwrap()
    );
}

#[test]
fn terminal_cics_actor_binding_refuses_foreign_pending_or_unattributed_effects() {
    let (actor, _, mut effect, _, _, _) = sample();
    effect.intent.capability =
        Some(CapabilityId::new("host.cics.execute", InvocationLimits::default()).unwrap());
    assert!(cics_effect_matches(&effect, &actor));
    for case in 0..12 {
        let mut changed = effect.clone();
        match case {
            0 => {
                changed.execution_id =
                    ExecutionId::new("copied-terminal-owner", InvocationLimits::default()).unwrap()
            }
            1 => {
                changed.run_unit_id =
                    RunUnitId::new("old-terminal-run", InvocationLimits::default()).unwrap()
            }
            2 => changed.sequence = 0,
            3 => changed.state = EffectState::UnknownOutcome,
            4 => changed.digest_format = EffectDigestFormat::LegacyDebug,
            5 => changed.result_digest = None,
            6 => changed.resolved_tick = Some(101),
            7 => {
                changed.intent.owner =
                    ExecutionId::new("foreign-intent-owner", InvocationLimits::default()).unwrap()
            }
            8 => changed.intent.attempt += 1,
            9 => {
                changed.intent.capability = Some(
                    CapabilityId::new("host.program.invoke", InvocationLimits::default()).unwrap(),
                )
            }
            10 => {
                changed.intent.audit_resource.as_mut().unwrap().format =
                    AuditResourceDigestFormat::CanonicalHostOversizedResourceV1
            }
            _ => changed.intent.audit_invocation_key = None,
        }
        assert!(!cics_effect_matches(&changed, &actor), "case {case}");
    }
}
