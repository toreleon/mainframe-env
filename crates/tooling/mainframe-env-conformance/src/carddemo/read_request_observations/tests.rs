//! Classifier negatives are zero journey/execution credit.
use super::*;
use mainframe_env_execution_api::{AuditResourceDigest, PrincipalId};
use mainframe_env_host_api::{HostRequest, canonical_audit_resource_digest};

fn owner(program: &str) -> ExecutionRecord {
    let limits = InvocationLimits::default();
    ExecutionRecord {
        execution_id: ExecutionId::new("selected-actor", limits).unwrap(),
        run_unit_id: RunUnitId::new("selected-stage", limits).unwrap(),
        selector: Selector::new(format!("program:{program}"), limits).unwrap(),
        artifact: ArtifactRef::new("sha256:fixture", limits).unwrap(),
        principal: PrincipalId::new("WEBUSER", limits).unwrap(),
        state: ExecutionState::Completed,
        attempt: 1,
        version: 17,
        owner_lease: None,
        lease_expiry_tick: None,
        terminal_tick: Some(100),
    }
}

fn key() -> IdempotencyKey {
    IdempotencyKey::new("selected-invocation", InvocationLimits::default()).unwrap()
}

fn audit(
    owner: &ExecutionRecord,
    sequence: u64,
    capability: &str,
    resource: AuditResourceDigest,
) -> AuditRecord {
    AuditRecord {
        execution_id: owner.execution_id.clone(),
        run_unit_id: owner.run_unit_id.clone(),
        principal: owner.principal.clone(),
        attempt: owner.attempt,
        effect_sequence: sequence,
        observed_tick: 1,
        invocation_key: key(),
        capability: CapabilityId::new(capability, InvocationLimits::default()).unwrap(),
        resource,
        decision: AuditDecision::Success,
    }
}

fn fixture() -> (ExecutionRecord, Vec<AuditRecord>) {
    let owner = owner("COACTVWC");
    let mut records = Vec::new();
    for (index, read) in oracles::ACCOUNT_READS.iter().enumerate() {
        records.push(audit(
            &owner,
            20 + index as u64,
            "host.cics.execute",
            read.cics_digest().unwrap(),
        ));
        records.push(audit(
            &owner,
            2 * index as u64 + 1,
            "host.dataset.read",
            read.attributes_digest().unwrap(),
        ));
        records.push(audit(
            &owner,
            2 * index as u64 + 2,
            "host.dataset.read",
            read.dataset_digest().unwrap(),
        ));
    }
    (owner, records)
}

#[test]
fn frozen_read_fingerprints_refuse_file_key_length_schema_and_policy_corruption() {
    for read in oracles::ACCOUNT_READS.iter().chain(&oracles::SIGNON_READ) {
        let expected = read.cics_digest().unwrap();
        for (name, schema, bytes) in [
            (
                "DATASET",
                "mainframe-env.cics.storage-value@1",
                b"OTHER   ".as_slice(),
            ),
            (
                "RIDFLD",
                "mainframe-env.cics.storage-value@1",
                b"00000000051".as_slice(),
            ),
            ("KEYLENGTH", "mainframe-env.cics.decimal@1", b"1".as_slice()),
            ("LENGTH", "mainframe-env.cics.decimal@1", b"1".as_slice()),
            (
                "INTO",
                "mainframe-env.cics.argument@1",
                b"OTHER-RECORD".as_slice(),
            ),
            (
                "RESP",
                "mainframe-env.cics.argument@1",
                b"OTHER-RESP".as_slice(),
            ),
            (
                "DATASET",
                "mainframe-env.cics.argument@1",
                read.logical.as_bytes(),
            ),
        ] {
            let HostRequest::Cics(mut request) = read.cics_request() else {
                unreachable!()
            };
            request.arguments.insert(
                name.into(),
                BoundedPayload::new(schema, bytes.to_vec(), InvocationLimits::default()).unwrap(),
            );
            assert_ne!(
                canonical_audit_resource_digest(&HostRequest::Cics(request)),
                expected
            );
        }
        let HostRequest::Cics(mut request) = read.cics_request() else {
            unreachable!()
        };
        request.condition_policy = CicsConditionPolicy::NoHandle;
        assert_ne!(
            canonical_audit_resource_digest(&HostRequest::Cics(request)),
            expected
        );
        for bad_key in [
            read.key.to_vec(),
            vec![0xf0],
            [read.encoded_key, b"\0"].concat(),
        ] {
            let HostRequest::Dataset(DatasetRequest::Read {
                dataset,
                member,
                max_records,
                control,
                ..
            }) = read.dataset_request()
            else {
                unreachable!()
            };
            assert_ne!(
                canonical_audit_resource_digest(&HostRequest::Dataset(DatasetRequest::Read {
                    dataset,
                    member,
                    max_records,
                    control,
                    key: Some(bad_key),
                })),
                read.dataset_digest().unwrap()
            );
        }
    }
}

#[test]
fn same_count_outer_or_inner_corruption_missing_duplicate_and_order_do_not_bind_reads() {
    let (owner, records) = fixture();
    compare_audits(&owner, &key(), 40, &records, &oracles::ACCOUNT_READS).unwrap();
    for index in 0..records.len() {
        let mut changed = records.clone();
        changed[index].resource.value[0] ^= 1;
        assert!(compare_audits(&owner, &key(), 40, &changed, &oracles::ACCOUNT_READS).is_err());
        let mut changed = records.clone();
        changed.remove(index);
        assert!(compare_audits(&owner, &key(), 40, &changed, &oracles::ACCOUNT_READS).is_err());
        let mut changed = records.clone();
        changed.push(records[index].clone());
        assert!(compare_audits(&owner, &key(), 40, &changed, &oracles::ACCOUNT_READS).is_err());
        let mut changed = records.clone();
        changed[index].resource.format =
            AuditResourceDigestFormat::CanonicalHostOversizedResourceV1;
        assert!(compare_audits(&owner, &key(), 40, &changed, &oracles::ACCOUNT_READS).is_err());
    }
    for (left, right) in [(0, 3), (1, 2), (2, 5)] {
        let mut changed = records.clone();
        let sequence = changed[left].effect_sequence;
        changed[left].effect_sequence = changed[right].effect_sequence;
        changed[right].effect_sequence = sequence;
        assert!(compare_audits(&owner, &key(), 40, &changed, &oracles::ACCOUNT_READS).is_err());
    }
    // Explicitly keep unrelated capability counts outside this selected read oracle.
    let mut extra = records;
    extra.push(audit(
        &owner,
        30,
        "host.clock",
        AuditResourceDigest {
            format: AuditResourceDigestFormat::CanonicalHostResourceV1,
            value: [9; 32],
        },
    ));
    compare_audits(&owner, &key(), 40, &extra, &oracles::ACCOUNT_READS).unwrap();
}

#[test]
fn foreign_audit_actor_run_principal_attempt_invocation_and_bad_decisions_refuse() {
    let (owner, records) = fixture();
    let limits = InvocationLimits::default();
    let mut variants = Vec::new();
    let mut foreign = records.clone();
    foreign[0].execution_id = ExecutionId::new("foreign", limits).unwrap();
    variants.push(foreign);
    let mut foreign = records.clone();
    foreign[0].run_unit_id = RunUnitId::new("foreign", limits).unwrap();
    variants.push(foreign);
    let mut foreign = records.clone();
    foreign[0].principal = PrincipalId::new("OTHER", limits).unwrap();
    variants.push(foreign);
    let mut foreign = records.clone();
    foreign[0].attempt += 1;
    variants.push(foreign);
    let mut foreign = records.clone();
    foreign[0].invocation_key = IdempotencyKey::new("foreign", limits).unwrap();
    variants.push(foreign);
    for decision in [
        AuditDecision::Deny,
        AuditDecision::Cancelled,
        AuditDecision::TimedOut,
        AuditDecision::Rejected,
        AuditDecision::ProviderFailure,
        AuditDecision::InfrastructureFailure,
        AuditDecision::UnknownOutcome,
    ] {
        let mut foreign = records.clone();
        foreign[0].decision = decision;
        variants.push(foreign);
    }
    for foreign in variants {
        assert!(compare_audits(&owner, &key(), 40, &foreign, &oracles::ACCOUNT_READS).is_err());
    }
    assert!(compare_audits(&owner, &key(), 20, &records, &oracles::ACCOUNT_READS).is_err());
    assert!(compare_audits(&owner, &key(), 40, &[], &oracles::ACCOUNT_READS).is_err());
    assert!(
        compare_audits(
            &owner,
            &key(),
            40,
            &vec![records[0].clone(); MAX_ACTOR_AUDITS + 1],
            &oracles::ACCOUNT_READS
        )
        .is_err()
    );
}

#[test]
fn completed_actor_binding_refuses_wrong_program_artifact_run_principal_attempt_or_state() {
    let owner = owner("COACTVWC");
    check_execution(&owner, "COACTVWC", &owner.artifact, &owner.run_unit_id).unwrap();
    let limits = InvocationLimits::default();
    let mut variants = Vec::new();
    let mut wrong = owner.clone();
    wrong.selector = Selector::new("program:OTHER", limits).unwrap();
    variants.push(wrong);
    let mut wrong = owner.clone();
    wrong.artifact = ArtifactRef::new("sha256:other", limits).unwrap();
    variants.push(wrong);
    let mut wrong = owner.clone();
    wrong.run_unit_id = RunUnitId::new("foreign", limits).unwrap();
    variants.push(wrong);
    let mut wrong = owner.clone();
    wrong.principal = PrincipalId::new("OTHER", limits).unwrap();
    variants.push(wrong);
    let mut wrong = owner.clone();
    wrong.attempt = 0;
    variants.push(wrong);
    let mut wrong = owner.clone();
    wrong.terminal_tick = None;
    variants.push(wrong);
    for state in [
        ExecutionState::Admitted,
        ExecutionState::Queued,
        ExecutionState::Running,
        ExecutionState::Suspended,
        ExecutionState::Completing,
        ExecutionState::Failed,
        ExecutionState::Cancelled,
        ExecutionState::TimedOut,
        ExecutionState::DeadLetter,
    ] {
        let mut wrong = owner.clone();
        wrong.state = state;
        variants.push(wrong);
    }
    for wrong in variants {
        assert!(check_execution(&wrong, "COACTVWC", &owner.artifact, &owner.run_unit_id).is_err());
    }
}

#[test]
fn historical_or_changed_snapshot_and_trace_prefix_refuse_freshness() {
    let row = ProviderStateRecord {
        namespace: REPLAY_NAMESPACE.into(),
        key: "old".into(),
        version: 2,
        payload: vec![1],
    };
    let before = BeforeStage {
        rows: BTreeMap::from([("old".into(), row)]),
        trace: vec![],
    };
    assert!(changed_rows(&before, &before).is_err());
    let mut after = before.clone();
    after.rows.insert(
        "fresh".into(),
        ProviderStateRecord {
            namespace: REPLAY_NAMESPACE.into(),
            key: "fresh".into(),
            version: 2,
            payload: vec![2],
        },
    );
    assert_eq!(changed_rows(&before, &after).unwrap().len(), 1);
    let mut altered = after.clone();
    altered.rows.get_mut("old").unwrap().payload.push(3);
    assert!(changed_rows(&before, &altered).is_err());
    let mut removed = after;
    removed.rows.remove("old");
    assert!(changed_rows(&before, &removed).is_err());
    let mut before_trace = before.clone();
    before_trace.trace.push(CicsTraceEntry {
        operation: CicsOperation::Read,
        outcome: "NORMAL".into(),
        response: 0,
        response2: 0,
        payload_bytes: 50,
    });
    assert!(changed_rows(&before_trace, &before).is_err());
}

fn terminal_event(actor: &ExecutionRecord, kind: LifecycleEventKind) -> LifecycleEvent {
    LifecycleEvent {
        execution_id: actor.execution_id.clone(),
        run_unit_id: actor.run_unit_id.clone(),
        sequence: actor.version,
        attempt: actor.attempt,
        tick: actor.terminal_tick.unwrap(),
        kind,
    }
}

#[test]
fn selected_terminal_kind_refuses_return_handoff_substitution_and_foreign_route_actors() {
    for (route, program, expected) in [
        (
            Route::Signon,
            "COSGN00C",
            LifecycleEventKind::HandoffCompleted,
        ),
        (
            Route::Signon,
            "COMEN01C",
            LifecycleEventKind::Completed { return_code: 0 },
        ),
        (
            Route::Account,
            "COACTVWC",
            LifecycleEventKind::Completed { return_code: 0 },
        ),
    ] {
        let actor = owner(program);
        assert!(terminal_events_match(
            route,
            program,
            &actor,
            &actor.run_unit_id,
            &[terminal_event(&actor, expected.clone())]
        ));
        for kind in [
            LifecycleEventKind::HandoffCompleted,
            LifecycleEventKind::Completed { return_code: 0 },
            LifecycleEventKind::Completed { return_code: 1 },
            LifecycleEventKind::Completed { return_code: -1 },
            LifecycleEventKind::Suspended,
            LifecycleEventKind::Completing,
            LifecycleEventKind::Cancelled,
            LifecycleEventKind::TimedOut,
            LifecycleEventKind::Condition,
            LifecycleEventKind::Abend,
            LifecycleEventKind::Failed,
        ] {
            if kind != expected {
                assert!(!terminal_events_match(
                    route,
                    program,
                    &actor,
                    &actor.run_unit_id,
                    &[terminal_event(&actor, kind)]
                ));
            }
        }
    }
    for (route, program) in [
        (Route::Account, "COSGN00C"),
        (Route::Account, "COMEN01C"),
        (Route::Signon, "COACTVWC"),
        (Route::Signon, "COADM01C"),
        (Route::Signon, "OTHER"),
        (Route::Account, "OTHER"),
    ] {
        let actor = owner(program);
        for kind in [
            LifecycleEventKind::HandoffCompleted,
            LifecycleEventKind::Completed { return_code: 0 },
        ] {
            assert!(!terminal_events_match(
                route,
                program,
                &actor,
                &actor.run_unit_id,
                &[terminal_event(&actor, kind)]
            ));
        }
    }
}

#[test]
fn terminal_event_identity_attempt_sequence_tick_missing_and_duplicate_corruptions_refuse() {
    for (route, program, kind) in [
        (
            Route::Signon,
            "COSGN00C",
            LifecycleEventKind::HandoffCompleted,
        ),
        (
            Route::Signon,
            "COMEN01C",
            LifecycleEventKind::Completed { return_code: 0 },
        ),
        (
            Route::Account,
            "COACTVWC",
            LifecycleEventKind::Completed { return_code: 0 },
        ),
    ] {
        let actor = owner(program);
        let original = terminal_event(&actor, kind);
        let limits = InvocationLimits::default();
        let mut bad = Vec::new();
        let mut e = original.clone();
        e.execution_id = ExecutionId::new("foreign", limits).unwrap();
        bad.push(e);
        let mut e = original.clone();
        e.run_unit_id = RunUnitId::new("foreign", limits).unwrap();
        bad.push(e);
        let mut e = original.clone();
        e.sequence += 1;
        bad.push(e);
        let mut e = original.clone();
        e.sequence = 0;
        bad.push(e);
        let mut e = original.clone();
        e.attempt += 1;
        bad.push(e);
        let mut e = original.clone();
        e.attempt = 0;
        bad.push(e);
        let mut e = original.clone();
        e.tick += 1;
        bad.push(e);
        let mut e = original.clone();
        e.tick = 0;
        bad.push(e);
        for event in bad {
            assert!(!terminal_events_match(
                route,
                program,
                &actor,
                &actor.run_unit_id,
                &[event]
            ));
        }
        assert!(!terminal_events_match(
            route,
            program,
            &actor,
            &actor.run_unit_id,
            &[]
        ));
        assert!(!terminal_events_match(
            route,
            program,
            &actor,
            &actor.run_unit_id,
            &[original.clone(), original.clone()]
        ));
        assert!(!terminal_events_match(
            route,
            program,
            &actor,
            &RunUnitId::new("foreign", limits).unwrap(),
            &[original]
        ));
    }
}

#[test]
fn completed_stage_surface_refuses_receive_suspension_or_other_session_principal_and_maps() {
    for (route, mapset, map) in [
        (Route::Signon, "COMEN01", "COMEN1A"),
        (Route::Account, "COACTVW", "CACTVWA"),
    ] {
        let terminal = serde_json::json!({"session":"selected-session","principal":"WEBUSER",
            "mapset":mapset,"map":map,"connected":true,"suspended":false});
        assert!(terminal_surface_matches(
            &terminal,
            "selected-session",
            route
        ));
        for (field, value) in [
            ("suspended", serde_json::json!(true)),
            ("suspended", serde_json::json!("false")),
            ("suspended", serde_json::Value::Null),
            ("connected", serde_json::json!(false)),
            ("session", serde_json::json!("foreign")),
            ("principal", serde_json::json!("OTHER")),
            ("mapset", serde_json::json!("OTHER")),
            ("map", serde_json::json!("OTHER")),
        ] {
            let mut bad = terminal.clone();
            bad[field] = value;
            assert!(!terminal_surface_matches(&bad, "selected-session", route));
        }
        assert!(!terminal_surface_matches(
            &terminal,
            "foreign-session",
            route
        ));
    }
}
