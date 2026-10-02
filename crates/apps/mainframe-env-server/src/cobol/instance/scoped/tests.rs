use super::*;
use crate::cobol::hardening::parent;
use serde_json::json;

fn root_actor() -> Invocation {
    let mut actor = parent();
    actor.limits.max_storage_bytes = 256;
    actor.selector = Selector::new("program:MAIN", InvocationLimits::default()).unwrap();
    actor.artifact = ArtifactRef::new(
        format!("sha256:{}", "1".repeat(64)),
        InvocationLimits::default(),
    )
    .unwrap();
    actor
}

fn child(source: &Invocation, call: &str, name: &str) -> Invocation {
    let mut actor = source.clone();
    super::super::super::replay::bind_protocol_owner(source, &mut actor.bindings).unwrap();
    actor.execution_id = ExecutionId::new(
        format!("online-call-execution-{call}"),
        InvocationLimits::default(),
    )
    .unwrap();
    actor.parent_execution_id = Some(source.execution_id.clone());
    actor.selector = Selector::new(format!("program:{name}"), InvocationLimits::default()).unwrap();
    actor.artifact = ArtifactRef::new(
        format!("sha256:{}", "2".repeat(64)),
        InvocationLimits::default(),
    )
    .unwrap();
    actor
}

fn root_row(root: &ScopedRun, key: &str) -> ProviderStateRecord {
    ProviderStateRecord {
        namespace: RUN_STATE_NAMESPACE.into(),
        key: key.into(),
        version: 1,
        payload: canonical(root).unwrap(),
    }
}

fn fixture() -> (ScopedRun, String, ProviderStateRecord) {
    let actor = root_actor();
    let key = run_key(&actor);
    let mut root = ScopedRun::fresh(&actor).unwrap();
    let call = "a".repeat(64);
    let target = child(&actor, &call, "COUNT");
    let entry = root.root.native_call(&actor, &target, &call).unwrap();
    let member_key = entry.member_key("COUNT").unwrap();
    let mut instance = ScopedInstance {
        schema_version: 3,
        run_key: key.clone(),
        scope_entry: root.root.clone(),
        max_state_bytes: root.max_member_bytes,
        program: "COUNT".into(),
        artifact: target.artifact.as_str().into(),
        owner: Some(entry),
        initial: false,
        state: None,
        metadata_digest: String::new(),
    };
    instance.metadata_digest = instance.expected_digest(&member_key).unwrap();
    let row = ProviderStateRecord {
        namespace: namespace(&key),
        key: member_key.clone(),
        version: 7,
        payload: canonical(&instance).unwrap(),
    };
    root.members.insert(
        member_key,
        Member {
            scope: instance.scope_entry.scope_id().into(),
            program: instance.program.clone(),
            artifact: instance.artifact.clone(),
            row_version: 7,
            payload_digest: payload_digest(&row.payload),
            charged_bytes: root.max_member_bytes,
            busy: true,
        },
    );
    root.calls.insert(call);
    root.receipt_charge = root.max_receipt_bytes;
    root.active = 1;
    root.charged_bytes = root.root_charge + root.max_member_bytes;
    root.refresh(&key).unwrap();
    (root, key, row)
}

#[test]
fn fresh_root_has_one_scope_and_charges_the_unmanaged_top_machine() {
    let actor = root_actor();
    let key = run_key(&actor);
    let root = ScopedRun::fresh(&actor).unwrap();
    assert_eq!(
        root.metadata_digest,
        "8f40bff4d30356fb04e34557513bb2a58dd44e054e6914aa263e7f8cb56610d3"
    );
    assert_eq!(root.scopes.len(), 1);
    assert_eq!(root.active, 0);
    assert_eq!(root.charged_bytes, actor.limits.max_storage_bytes);
    let row = root_row(&root, &key);
    let before = row.clone();
    let decoded = ScopedRun::decode(&row).unwrap();
    decoded.validate_members(&key, &[]).unwrap();
    assert_eq!(row, before);
    assert_eq!(canonical(&decoded).unwrap(), row.payload);
}

#[test]
fn exact_busy_member_versions_payloads_and_charges_agree() {
    let (root, key, row) = fixture();
    assert_eq!(
        payload_digest(&row.payload),
        "d8dd7218bb9798792ac098051d516d20d842a02e21280af9dede720621428987"
    );
    let instance: ScopedInstance = serde_json::from_slice(&row.payload).unwrap();
    assert_eq!(
        instance.metadata_digest,
        "75444c1aa9604556c0d0180b106e4db3a529d6f5bb8d65cfe2be3286a1276127"
    );
    ScopedRun::decode(&root_row(&root, &key))
        .unwrap()
        .validate_members(&key, &[row.clone()])
        .unwrap();
    assert!(root.validate_members(&key, &[]).is_err());
    assert!(
        root.validate_members(&key, &[row.clone(), row.clone()])
            .is_err()
    );
    let mut invalid = row.clone();
    invalid.version += 1;
    assert!(root.validate_members(&key, &[invalid]).is_err());
    let mut invalid = row.clone();
    invalid.namespace = namespace(&"e".repeat(64));
    assert!(root.validate_members(&key, &[invalid]).is_err());
    let mut invalid = row;
    invalid.key = "e".repeat(64);
    assert!(root.validate_members(&key, &[invalid]).is_err());
}

#[test]
fn rehashed_inconsistent_root_metadata_and_resource_charges_fail_closed() {
    let (root, key, _) = fixture();
    for case in 0..12 {
        let mut invalid = root.clone();
        match case {
            0 => invalid.schema_version = 2,
            1 => invalid.active = 0,
            2 => invalid.charged_bytes = 0,
            3 => invalid.root_charge = 0,
            4 => invalid.max_member_bytes = 0,
            5 => invalid.max_member_bytes = u64::MAX,
            6 => invalid.max_scopes = 0,
            7 => invalid.max_scopes = 17,
            8 => invalid.scopes.clear(),
            9 => invalid.ended_tick = Some(7),
            10 => invalid.members.values_mut().next().unwrap().charged_bytes -= 1,
            _ => invalid.members.values_mut().next().unwrap().row_version = 0,
        }
        invalid.refresh(&key).unwrap();
        assert!(
            ScopedRun::decode(&root_row(&invalid, &key)).is_err(),
            "case {case}"
        );
    }
    let wrong = "f".repeat(64);
    let mut invalid = root;
    invalid.refresh(&wrong).unwrap();
    assert!(ScopedRun::decode(&root_row(&invalid, &wrong)).is_err());
}

#[test]
fn instance_integrity_and_root_index_do_not_accept_rehashed_foreign_owner() {
    let (mut root, key, row) = fixture();
    for case in 0..9 {
        let mut value: ScopedInstance = serde_json::from_slice(&row.payload).unwrap();
        match case {
            0 => value.schema_version = 2,
            1 => value.run_key = "b".repeat(64),
            2 => value.scope_entry = value.owner.as_ref().unwrap().clone(),
            3 => value.program = "OTHER".into(),
            4 => value.artifact = format!("sha256:{}", "3".repeat(64)),
            5 => {
                value.initial = true;
                value.state = Some(vec![1]);
            }
            6 => value.owner = None,
            7 => value.state = Some(vec![0; usize::try_from(root.max_member_bytes).unwrap() + 1]),
            _ => value.artifact = "sha256:BAD".into(),
        }
        value.metadata_digest = value.expected_digest(&row.key).unwrap();
        let mut invalid = row.clone();
        invalid.payload = canonical(&value).unwrap();
        root.members.get_mut(&row.key).unwrap().payload_digest = payload_digest(&invalid.payload);
        root.refresh(&key).unwrap();
        assert!(
            root.validate_members(&key, &[invalid]).is_err(),
            "case {case}"
        );
    }
}

#[test]
fn idle_members_charge_actual_retained_bytes_and_cannot_claim_busy() {
    let (mut root, key, mut row) = fixture();
    let mut instance: ScopedInstance = serde_json::from_slice(&row.payload).unwrap();
    instance.owner = None;
    instance.state = Some(vec![3, 7]);
    instance.metadata_digest = instance.expected_digest(&row.key).unwrap();
    row.payload = canonical(&instance).unwrap();
    let member = root.members.get_mut(&row.key).unwrap();
    member.busy = false;
    member.charged_bytes = 2;
    member.payload_digest = payload_digest(&row.payload);
    root.active = 0;
    root.charged_bytes = root.root_charge + 2;
    root.refresh(&key).unwrap();
    root.validate_members(&key, &[row.clone()]).unwrap();
    root.members.get_mut(&row.key).unwrap().charged_bytes = 3;
    root.charged_bytes += 1;
    root.refresh(&key).unwrap();
    assert!(root.validate_members(&key, &[row]).is_err());
}

#[test]
fn child_scopes_require_existing_parent_and_exact_level_with_shared_root_cap() {
    let actor = root_actor();
    let key = run_key(&actor);
    let mut root = ScopedRun::fresh(&actor).unwrap();
    let call = "a".repeat(64);
    let target = child(&actor, &call, "MID");
    let selection = mainframe_env_host_api::ProgramLinkSelection {
        artifact: target.artifact.clone(),
        generation: 7,
        content_identity: format!("sha256:{}", "3".repeat(64)),
    };
    let linked = root
        .root
        .cics_link(&actor, &target, &call, &selection, 2)
        .unwrap();
    root.calls.insert(call);
    root.receipt_charge = root.max_receipt_bytes;
    root.scopes.insert(linked.scope_id().into(), linked.clone());
    let member_key = linked.member_key("MID").unwrap();
    let mut instance = ScopedInstance {
        schema_version: 3,
        run_key: key.clone(),
        scope_entry: linked.clone(),
        max_state_bytes: root.max_member_bytes,
        program: "MID".into(),
        artifact: target.artifact.as_str().into(),
        owner: Some(linked.clone()),
        initial: false,
        state: None,
        metadata_digest: String::new(),
    };
    instance.metadata_digest = instance.expected_digest(&member_key).unwrap();
    let row = ProviderStateRecord {
        namespace: namespace(&key),
        key: member_key.clone(),
        version: 1,
        payload: canonical(&instance).unwrap(),
    };
    root.members.insert(
        member_key,
        Member {
            scope: linked.scope_id().into(),
            program: "MID".into(),
            artifact: target.artifact.as_str().into(),
            row_version: 1,
            payload_digest: payload_digest(&row.payload),
            charged_bytes: root.max_member_bytes,
            busy: true,
        },
    );
    root.active = 1;
    root.charged_bytes += root.max_member_bytes;
    root.refresh(&key).unwrap();
    ScopedRun::decode(&root_row(&root, &key)).unwrap();
    assert_eq!(root.max_member_bytes, actor.limits.max_storage_bytes);
    assert_eq!(root.charged_bytes, root.root_charge + root.max_member_bytes);
    root.validate_members(&key, &[row]).unwrap();
    let native_actor = child(&target, &"b".repeat(64), "LEAF");
    let native = linked
        .native_call(&target, &native_actor, &"b".repeat(64))
        .unwrap();
    root.scopes.insert(native.scope_id().into(), native);
    root.refresh(&key).unwrap();
    assert!(
        ScopedRun::decode(&root_row(&root, &key)).is_err(),
        "native entry cannot replace creator"
    );
}

#[test]
fn ended_root_has_no_member_scope_or_storage_charge() {
    let actor = root_actor();
    let key = run_key(&actor);
    let mut root = ScopedRun::fresh(&actor).unwrap();
    root.scopes.clear();
    root.root_charge = 0;
    root.charged_bytes = 0;
    root.ended_tick = Some(9);
    root.refresh(&key).unwrap();
    ScopedRun::decode(&root_row(&root, &key))
        .unwrap()
        .validate_members(&key, &[])
        .unwrap();
    root.ended_tick = Some(0);
    root.refresh(&key).unwrap();
    assert!(ScopedRun::decode(&root_row(&root, &key)).is_err());
}

#[test]
fn canonical_rows_reject_unknown_duplicates_whitespace_legacy_and_wrong_namespace() {
    let actor = root_actor();
    let key = run_key(&actor);
    let root = ScopedRun::fresh(&actor).unwrap();
    let row = root_row(&root, &key);
    for payload in [
        {
            let mut v = serde_json::to_value(&root).unwrap();
            v["extra"] = json!(true);
            serde_json::to_vec(&v).unwrap()
        },
        {
            let mut v = row.payload.clone();
            v.pop();
            v.extend_from_slice(b",\"active\":0}");
            v
        },
        {
            let mut v = row.payload.clone();
            v.push(b' ');
            v
        },
        serde_json::to_vec_pretty(&root).unwrap(),
        br#"{"schema_version":2,"active":0,"instances":0,"programs":[],"ended":false}"#.to_vec(),
    ] {
        let mut invalid = row.clone();
        invalid.payload = payload;
        assert!(ScopedRun::decode(&invalid).is_err());
    }
    let mut invalid = row.clone();
    invalid.namespace = "other".into();
    assert!(ScopedRun::decode(&invalid).is_err());
    let mut invalid = row;
    invalid.version = 0;
    assert!(ScopedRun::decode(&invalid).is_err());
}

#[test]
fn receipt_allowance_is_root_wide_monotonic_and_failure_preserves_bytes() {
    let mut actor = root_actor();
    actor.limits.max_effects = 2;
    let key = run_key(&actor);
    let mut root = ScopedRun::fresh(&actor).unwrap();
    root.reserve_call_charge(&key, &"a".repeat(64)).unwrap();
    root.reserve_call_charge(&key, &"b".repeat(64)).unwrap();
    assert_eq!(root.calls.len(), 2);
    assert_eq!(root.receipt_charge, 2 * root.max_receipt_bytes);
    let before = canonical(&root).unwrap();
    assert!(root.reserve_call_charge(&key, &"c".repeat(64)).is_err());
    assert_eq!(canonical(&root).unwrap(), before);
    let mut changed = root.clone();
    changed.max_calls = 3;
    changed.refresh(&key).unwrap();
    assert!(
        ScopedRun::decode(&root_row(&changed, &key)).is_ok(),
        "integrity alone is not authority"
    );
    assert!(changed.validate_live_limits(&actor).is_err());
    root.validate_live_limits(&actor).unwrap();
    let mut corrupt = root;
    corrupt.receipt_charge += 1;
    corrupt.refresh(&key).unwrap();
    assert!(ScopedRun::decode(&root_row(&corrupt, &key)).is_err());
}

#[test]
fn scoped_retention_reads_keep_root_owner_and_busy_call_dependencies() {
    let (root, key, row) = fixture();
    let descriptor = super::super::describe_run_state_row(&root_row(&root, &key)).unwrap();
    assert_eq!(descriptor.kind, CobolRetentionRowKind::RunState);
    assert_eq!(descriptor.state, CobolRetentionState::Active);
    assert_eq!(
        descriptor.owner_execution.as_deref(),
        Some(root.root.creation_identity().0)
    );
    let descriptor = super::super::describe_instance_row(&row).unwrap();
    assert_eq!(descriptor.kind, CobolRetentionRowKind::Instance);
    assert_eq!(
        descriptor.owner_execution.as_deref(),
        Some(root.root.creation_identity().0)
    );
    assert!(
        descriptor
            .dependencies
            .contains(&provider_dependency(RUN_STATE_NAMESPACE, &key))
    );
    assert!(descriptor.dependencies.contains(&provider_dependency(
        super::super::super::retention::CALL_REPLAY_NAMESPACE,
        "a".repeat(64)
    )));
    let mut invalid = row;
    invalid.payload.push(b' ');
    assert!(super::super::describe_instance_row(&invalid).is_err());
}
