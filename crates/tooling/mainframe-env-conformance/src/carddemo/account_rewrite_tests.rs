//! Synthetic classifier controls; no application, issue or native gate credit.
use super::*;
use mainframe_env_cics::{
    CicsTraceEntry, CicsUowCodecVersion, CicsUowDependencyState, CicsUowRowDescriptor, CicsUowState,
};
use mainframe_env_execution_api::{LifecycleEvent, LifecycleEventKind, PrincipalId, Selector};
use mainframe_env_host_api::{
    CatalogEntryKind, CatalogListEntry, HostRequest, canonical_audit_resource_digest,
    canonical_request_digest,
};
use mainframe_env_store_api::{
    EffectDigestFormat, EffectIntentMetadata, EffectRecord, EffectState, ExecutionRecord,
    ExecutionState,
};

fn state() -> (data::Expected, State) {
    let mut state = State::new();
    let mut expected_accounts = Vec::new();
    let mut expected_customers = Vec::new();
    for (name, width, key, seed) in [
        (
            data::ACCOUNT,
            300usize,
            11usize,
            oracles::ACCOUNT_BEFORE.as_slice(),
        ),
        (
            data::CUSTOMER,
            500usize,
            9usize,
            oracles::CUSTOMER_BEFORE.as_slice(),
        ),
    ] {
        let mut records = Vec::new();
        for number in 1..=50 {
            let mut row = seed.to_vec();
            let identity = format!("{number:0key$}");
            for (target, digit) in row[..key].iter_mut().zip(identity.bytes()) {
                *target = 0xf0 + (digit - b'0');
            }
            records.push(row);
        }
        if name == data::ACCOUNT {
            expected_accounts = records.clone();
        } else {
            expected_customers = records.clone();
        }
        let identities = records.iter().map(|row| row[..key].to_vec()).collect();
        state.insert(
            name.into(),
            Rows {
                catalog: CatalogListEntry {
                    name: DatasetName::new(name, 128).unwrap(),
                    kind: CatalogEntryKind::Dataset,
                    related: None,
                    version: 3,
                },
                attributes: DatasetAttributes {
                    organization: DatasetOrganization::KeySequenced,
                    record_format: RecordFormat::Fixed,
                    logical_record_length: width as u32,
                    key_offset: Some(0),
                    key_length: Some(key as u32),
                    ccsid: Some(37),
                },
                attributes_version: 3,
                records,
                identities,
                version: 3,
            },
        );
    }
    for name in inserted::NAMES {
        if !state.contains_key(*name) {
            let mut rows = state[data::ACCOUNT].clone();
            rows.catalog.name = DatasetName::new(*name, 128).unwrap();
            state.insert((*name).into(), rows);
        }
    }
    (
        data::Expected::fixture(expected_accounts, expected_customers),
        state,
    )
}
fn rewritten(before: &State) -> State {
    let mut after = before.clone();
    for (name, replacement) in [
        (data::ACCOUNT, oracles::ACCOUNT_REWRITE.as_slice()),
        (data::CUSTOMER, oracles::CUSTOMER_REWRITE.as_slice()),
    ] {
        let rows = after.get_mut(name).unwrap();
        rows.records[48] = replacement.to_vec();
        rows.version += 1;
        rows.attributes_version += 1;
        rows.catalog.version += 1;
    }
    after
}
#[test]
fn source_frozen_complete_account_and_customer_bytes_keys_and_other_rows_refuse_drift() {
    let (expected, before) = state();
    let correct = rewritten(&before);
    data::immediate_delta(&expected, &before, &correct).unwrap();
    for name in [data::ACCOUNT, data::CUSTOMER] {
        for offset in 0..correct[name].records[48].len() {
            let mut after = correct.clone();
            after.get_mut(name).unwrap().records[48][offset] ^= 1;
            assert!(
                data::immediate_delta(&expected, &before, &after).is_err(),
                "{name} byte{offset}"
            );
        }
        for variant in 0..9 {
            let mut after = correct.clone();
            let rows = after.get_mut(name).unwrap();
            match variant {
                0 => {
                    rows.records.pop();
                }
                1 => rows.records.push(rows.records[0].clone()),
                2 => rows.identities[48][0] ^= 1,
                3 => rows.records[0][20] ^= 1,
                4 => rows.records.swap(0, 48),
                5 => rows.version -= 1,
                6 => rows.attributes_version -= 1,
                7 => rows.catalog.version -= 1,
                _ => rows.attributes.key_offset = Some(1),
            }
            assert!(data::immediate_delta(&expected, &before, &after).is_err());
        }
    }
    for name in inserted::NAMES {
        if ![data::ACCOUNT, data::CUSTOMER].contains(name) {
            let mut after = correct.clone();
            after.get_mut(*name).unwrap().records[0][0] ^= 1;
            assert!(data::immediate_delta(&expected, &before, &after).is_err());
        }
    }
    assert!(data::immediate_delta(&expected, &before, &before).is_err());
}
fn owner() -> ExecutionRecord {
    let limits = InvocationLimits::default();
    ExecutionRecord {
        execution_id: ExecutionId::new("account-actor", limits).unwrap(),
        run_unit_id: RunUnitId::new("account-run", limits).unwrap(),
        selector: Selector::new("program:COACTUPC", limits).unwrap(),
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
fn effect(owner: &ExecutionRecord) -> EffectRecord {
    EffectRecord {
        execution_id: owner.execution_id.clone(),
        run_unit_id: owner.run_unit_id.clone(),
        sequence: 12,
        key: IdempotencyKey::new("account-syncpoint", InvocationLimits::default()).unwrap(),
        digest_format: EffectDigestFormat::CanonicalHostV1,
        request_digest: [0; 32],
        intent: EffectIntentMetadata {
            owner: owner.execution_id.clone(),
            attempt: owner.attempt,
            capability: Some(
                CapabilityId::new("host.cics.execute", InvocationLimits::default()).unwrap(),
            ),
            audit_resource: None,
            audit_invocation_key: Some(
                IdempotencyKey::new("account-invocation", InvocationLimits::default()).unwrap(),
            ),
            created_tick: 1,
            recovery_after_tick: 1,
            epoch: 1,
            recovery_lease: None,
        },
        state: EffectState::Completed,
        result_digest: Some([1; 32]),
        resolved_tick: Some(100),
    }
}
#[test]
fn request_binding_refuses_wrong_file_from_length_policy_transaction_and_rollback() {
    let mut effect = effect(&owner());
    for customer in [false, true] {
        let request = evidence::rewrite_request(&effect, customer);
        effect.request_digest = canonical_request_digest(&request).unwrap();
        effect.intent.audit_resource = Some(canonical_audit_resource_digest(&request));
        assert!(evidence::same_request(&effect, &request));
        for (field, schema, value) in [
            (
                "FILE",
                "mainframe-env.cics.storage-value@1",
                b"OTHER   ".as_slice(),
            ),
            (
                "FROM",
                "mainframe-env.cics.storage-value@1",
                b"wrong bytes".as_slice(),
            ),
            ("LENGTH", "mainframe-env.cics.decimal@1", b"1".as_slice()),
            (
                "FILE",
                "mainframe-env.cics.argument@1",
                b"ACCTDAT ".as_slice(),
            ),
        ] {
            let HostRequest::Cics(mut wrong) = request.clone() else {
                unreachable!()
            };
            wrong.arguments.insert(
                field.into(),
                BoundedPayload::new(schema, value.to_vec(), InvocationLimits::default()).unwrap(),
            );
            assert!(!evidence::same_request(&effect, &HostRequest::Cics(wrong)));
        }
        let HostRequest::Cics(mut wrong) = request else {
            unreachable!()
        };
        wrong.condition_policy = CicsConditionPolicy::NoHandle;
        assert!(!evidence::same_request(&effect, &HostRequest::Cics(wrong)));
    }
    let request = evidence::commit_request(&effect);
    effect.request_digest = canonical_request_digest(&request).unwrap();
    effect.intent.audit_resource = Some(canonical_audit_resource_digest(&request));
    let HostRequest::Cics(mut wrong) = request.clone() else {
        unreachable!()
    };
    wrong.arguments.insert(
        "OPTION.ROLLBACK".into(),
        BoundedPayload::new(
            "mainframe-env.cics.option@1",
            vec![],
            InvocationLimits::default(),
        )
        .unwrap(),
    );
    assert!(!evidence::same_request(&effect, &HostRequest::Cics(wrong)));
    let HostRequest::Cics(mut wrong) = request else {
        unreachable!()
    };
    wrong.mutation.as_mut().unwrap().transaction = Some("OTHER".into());
    assert!(!evidence::same_request(&effect, &HostRequest::Cics(wrong)));
}
#[test]
fn completed_source_actor_refuses_wrong_artifact_principal_run_attempt_or_state() {
    let original = owner();
    let effect = effect(&original);
    let installed = BTreeMap::from([("COACTUPC".into(), original.artifact.clone())]);
    assert!(evidence::owner_matches(
        &original,
        &effect,
        &installed,
        &original.run_unit_id,
        false
    ));
    for variant in 0..7 {
        let mut changed = original.clone();
        match variant {
            0 => {
                changed.artifact =
                    ArtifactRef::new("sha256:other", InvocationLimits::default()).unwrap()
            }
            1 => {
                changed.principal = PrincipalId::new("WEBADM", InvocationLimits::default()).unwrap()
            }
            2 => {
                changed.run_unit_id =
                    RunUnitId::new("other-run", InvocationLimits::default()).unwrap()
            }
            3 => changed.attempt += 1,
            4 => changed.state = ExecutionState::Suspended,
            5 => changed.terminal_tick = None,
            _ => {
                changed.selector =
                    Selector::new("program:OTHER", InvocationLimits::default()).unwrap()
            }
        }
        assert!(!evidence::owner_matches(
            &changed,
            &effect,
            &installed,
            &original.run_unit_id,
            false
        ));
    }
    let mut wrong_effect = effect.clone();
    wrong_effect.run_unit_id = RunUnitId::new("other-run", InvocationLimits::default()).unwrap();
    assert!(!evidence::owner_matches(
        &original,
        &wrong_effect,
        &installed,
        &original.run_unit_id,
        false
    ));
}
#[test]
fn ordinary_return_and_xctl_require_their_distinct_success_terminal_events() {
    for (program, commit, kind) in [
        (
            "COACTUPC",
            false,
            LifecycleEventKind::Completed { return_code: 0 },
        ),
        ("COACTUPC", true, LifecycleEventKind::HandoffCompleted),
        (
            "COMEN01C",
            true,
            LifecycleEventKind::Completed { return_code: 0 },
        ),
    ] {
        let mut owner = owner();
        owner.selector =
            Selector::new(format!("program:{program}"), InvocationLimits::default()).unwrap();
        let original = LifecycleEvent {
            execution_id: owner.execution_id.clone(),
            run_unit_id: owner.run_unit_id.clone(),
            sequence: owner.version,
            attempt: owner.attempt,
            tick: owner.terminal_tick.unwrap(),
            kind,
        };
        assert!(evidence::terminal_event_matches(&owner, &original, commit));
        for kind in [
            LifecycleEventKind::Completed { return_code: 4 },
            LifecycleEventKind::Failed,
            LifecycleEventKind::Suspended,
            if commit && program == "COACTUPC" {
                LifecycleEventKind::Completed { return_code: 0 }
            } else {
                LifecycleEventKind::HandoffCompleted
            },
        ] {
            let mut wrong = original.clone();
            wrong.kind = kind;
            assert!(!evidence::terminal_event_matches(&owner, &wrong, commit));
        }
        for field in 0..5 {
            let mut wrong = original.clone();
            match field {
                0 => {
                    wrong.execution_id =
                        ExecutionId::new("OTHER", InvocationLimits::default()).unwrap()
                }
                1 => {
                    wrong.run_unit_id =
                        RunUnitId::new("OTHER", InvocationLimits::default()).unwrap()
                }
                2 => wrong.sequence += 1,
                3 => wrong.attempt += 1,
                _ => wrong.tick += 1,
            }
            assert!(!evidence::terminal_event_matches(&owner, &wrong, commit));
        }
    }
}
#[test]
fn explicit_uow_refuses_pending_rollback_legacy_dependency_and_wrong_effect_or_owner() {
    let owner = owner();
    let effect = effect(&owner);
    let original = CicsUowRowDescriptor {
        namespace: "cics-uow".into(),
        key: effect.key.as_str().into(),
        row_version: 2,
        payload_digest: [1; 32],
        codec: CicsUowCodecVersion::RetentionV2,
        state: CicsUowState::Committed,
        transaction: "CAUP".into(),
        effect_key: Some(effect.key.as_str().into()),
        owner_execution: Some(owner.execution_id.as_str().into()),
        task_owner_execution: None,
        owner_run_unit: Some(owner.run_unit_id.as_str().into()),
        deadline_tick: Some(50),
        terminal_tick: Some(100),
        dependency: CicsUowDependencyState::Clear,
    };
    evidence::require_uow(&original, &owner, &effect).unwrap();
    for variant in 0..14 {
        let mut row = original.clone();
        match variant {
            0 => row.state = CicsUowState::CommitPending,
            1 => row.state = CicsUowState::RollbackPending,
            2 => row.state = CicsUowState::RolledBack,
            3 => row.codec = CicsUowCodecVersion::LegacyV1,
            4 => row.dependency = CicsUowDependencyState::UndoLogPresent,
            5 => row.owner_execution = Some("OTHER".into()),
            6 => row.owner_run_unit = Some("OTHER".into()),
            7 => row.effect_key = Some("OTHER".into()),
            8 => row.transaction = "OTHER".into(),
            9 => row.terminal_tick = None,
            10 => row.terminal_tick = Some(49),
            11 => row.namespace = "OTHER".into(),
            12 => row.key = "OTHER".into(),
            _ => row.row_version = 1,
        }
        assert!(evidence::require_uow(&row, &owner, &effect).is_err());
    }
}
#[test]
fn exact_stage_trace_and_checked_reopen_are_required_before_any_observation() {
    let trace = [(CicsOperation::Syncpoint, 0), (CicsOperation::Xctl, 160)]
        .into_iter()
        .map(|(operation, payload_bytes)| CicsTraceEntry {
            operation,
            outcome: "NORMAL".into(),
            response: 0,
            response2: 0,
            payload_bytes,
        })
        .collect::<Vec<_>>();
    evidence::trace_matches(&trace, evidence::Stage::Commit).unwrap();
    for variant in 0..4 {
        let mut wrong = trace.clone();
        match variant {
            0 => wrong.reverse(),
            1 => wrong[0].response = 16,
            2 => wrong[1].payload_bytes = 0,
            _ => {
                wrong.pop();
            }
        }
        assert!(evidence::trace_matches(&wrong, evidence::Stage::Commit).is_err());
    }
    for retained_final in [None, Some(State::new())] {
        let proof = AccountProof {
            retained_final,
            reopened_verified: false,
        };
        let mut observations = RouteObservations::default();
        assert!(proof.bind(&mut observations).is_err());
        assert!(observations.journeys().is_empty());
        assert!(observations.issues().is_empty());
    }
}

#[test]
fn completed_account_rewrite_and_commit_surfaces_refuse_receive_suspension_and_stage_substitution()
{
    for (stage, mapset, map) in [
        (evidence::Stage::Rewrite, "COACTUP", "CACTUPA"),
        (evidence::Stage::Commit, "COMEN01", "COMEN1A"),
    ] {
        let original = serde_json::json!({
            "session":"selected-session", "principal":"WEBUSER", "connected":true,
            "suspended":false, "mapset":mapset, "map":map,
        });
        assert!(evidence::terminal_surface_matches(
            &original,
            "selected-session",
            stage
        ));
        for (field, value) in [
            ("session", serde_json::json!("foreign-session")),
            ("principal", serde_json::json!("OTHER")),
            ("connected", serde_json::json!(false)),
            ("suspended", serde_json::json!(true)),
            ("suspended", serde_json::json!("false")),
            ("suspended", serde_json::Value::Null),
            ("mapset", serde_json::json!("COSGN00")),
            ("map", serde_json::json!("WRONG")),
        ] {
            let mut wrong = original.clone();
            wrong[field] = value;
            assert!(!evidence::terminal_surface_matches(
                &wrong,
                "selected-session",
                stage
            ));
        }
        let mut absent = original.clone();
        absent.as_object_mut().unwrap().remove("suspended");
        assert!(!evidence::terminal_surface_matches(
            &absent,
            "selected-session",
            stage
        ));
        let other_stage = match stage {
            evidence::Stage::Rewrite => evidence::Stage::Commit,
            evidence::Stage::Commit => evidence::Stage::Rewrite,
        };
        assert!(!evidence::terminal_surface_matches(
            &original,
            "selected-session",
            other_stage
        ));
        assert!(!evidence::terminal_surface_matches(
            &original,
            "foreign-session",
            stage
        ));
    }
}
