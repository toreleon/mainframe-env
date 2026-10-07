use super::*;
use crate::cobol::hardening::parent;
use serde_json::json;

fn root() -> Invocation {
    let mut actor = parent();
    actor.selector = Selector::new("program:MAIN", InvocationLimits::default()).unwrap();
    actor.artifact = ArtifactRef::new(
        format!("sha256:{}", "1".repeat(64)),
        InvocationLimits::default(),
    )
    .unwrap();
    actor
}

fn child(source: &Invocation, call: &str, program: &str) -> Invocation {
    let mut actor = source.clone();
    actor.bindings.remove(BINDING);
    replay::bind_protocol_owner(source, &mut actor.bindings).unwrap();
    actor.execution_id =
        ExecutionId::new(child_execution(call), InvocationLimits::default()).unwrap();
    actor.parent_execution_id = Some(source.execution_id.clone());
    actor.selector =
        Selector::new(format!("program:{program}"), InvocationLimits::default()).unwrap();
    actor.artifact = ArtifactRef::new(
        format!("sha256:{}", "2".repeat(64)),
        InvocationLimits::default(),
    )
    .unwrap();
    actor
}

fn selection(actor: &Invocation) -> ProgramLinkSelection {
    ProgramLinkSelection {
        artifact: actor.artifact.clone(),
        generation: 7,
        content_identity: format!("sha256:{}", "3".repeat(64)),
    }
}

#[test]
fn storage_scope_canonical_vectors_are_frozen_independently() {
    let actor = root();
    let entry = Entry::root(&actor).unwrap();
    assert_eq!(
        entry.scope_id(),
        "b9e9ccd39054ab3d261eb1f8e23ffdfd3303eb1053aa6d732f3dd33bbe78464d"
    );
    assert_eq!(
        entry.metadata_digest,
        "3f6c679f8ecd684c5714d125a12a578492aeda465e9df1658835f0ecf32d1b3f"
    );
    assert_eq!(
        entry.member_key("COUNT").unwrap(),
        "872e7fe509ca603211833d8985fbc09f60c7b8f584feeab81a9b88cd081e7827"
    );
    let call = "a".repeat(64);
    let target = child(&actor, &call, "COUNT");
    let linked = entry
        .cics_link(&actor, &target, &call, &selection(&target), 2)
        .unwrap();
    assert_eq!(
        linked.scope_id(),
        "7bdd49e2b44c3179d838a0b49aae75fcf916d1b029a7415b8d9eb29d5be529d0"
    );
    assert_eq!(
        linked.metadata_digest,
        "b82e55cddab24b4ccd1fc91f44d34f052776bc67a4ff5d0ae92048c7d0b1bb91"
    );
    assert_eq!(
        linked.member_key("COUNT").unwrap(),
        "03f1e9e63d564d0343ba6eb3e26edf9dfc366d01cb4ec7d5bd786117c3cb205f"
    );
}

#[test]
fn native_call_keeps_immutable_link_creator_while_advancing_entry_actor() {
    let actor = root();
    let root_entry = Entry::root(&actor).unwrap();
    let link_call = "a".repeat(64);
    let linked_actor = child(&actor, &link_call, "COUNT");
    let linked = root_entry
        .cics_link(
            &actor,
            &linked_actor,
            &link_call,
            &selection(&linked_actor),
            2,
        )
        .unwrap();
    let native_call = "b".repeat(64);
    let native_actor = child(&linked_actor, &native_call, "LEAF");
    let native = linked
        .native_call(&linked_actor, &native_actor, &native_call)
        .unwrap();
    assert_eq!(native.scope, linked.scope);
    assert_ne!(native.execution, linked.execution);
    assert_eq!(
        native.scope.creation_call.as_deref(),
        Some(link_call.as_str())
    );
    assert_eq!(native.call_key.as_deref(), Some(native_call.as_str()));
    assert_eq!(native.logical_level(), 2);
    assert_eq!(
        native.member_key("COUNT").unwrap(),
        linked.member_key("COUNT").unwrap()
    );
    assert_ne!(
        native.member_key("COUNT").unwrap(),
        root_entry.member_key("COUNT").unwrap()
    );
}

#[test]
fn separate_link_occurrences_have_separate_members_and_nested_scope_edges() {
    let actor = root();
    let entry = Entry::root(&actor).unwrap();
    let mut scopes = Vec::new();
    for digit in ["a", "b"] {
        let call = digit.repeat(64);
        let target = child(&actor, &call, "COUNT");
        scopes.push(
            entry
                .cics_link(&actor, &target, &call, &selection(&target), 2)
                .unwrap(),
        );
    }
    assert_ne!(scopes[0].scope_id(), scopes[1].scope_id());
    assert_ne!(
        scopes[0].member_key("COUNT").unwrap(),
        scopes[1].member_key("COUNT").unwrap()
    );
    let first_actor = child(&actor, &"a".repeat(64), "COUNT");
    let call = "c".repeat(64);
    let nested_actor = child(&first_actor, &call, "COUNT");
    let nested = scopes[0]
        .cics_link(
            &first_actor,
            &nested_actor,
            &call,
            &selection(&nested_actor),
            3,
        )
        .unwrap();
    assert_eq!(
        nested.scope.parent_scope.as_deref(),
        Some(scopes[0].scope_id())
    );
    assert_eq!(nested.logical_level(), 3);
    assert_eq!(nested.scope.root_execution, actor.execution_id.as_str());
    assert_eq!(nested.scope.task_run, actor.run_unit_id.as_str());
}

#[test]
fn rehashed_invalid_scope_creation_and_entry_phases_are_rejected() {
    let actor = root();
    let good = Entry::root(&actor).unwrap();
    for case in 0..20 {
        let mut value = good.clone();
        match case {
            0 => value.schema_version = 2,
            1 => value.scope.root_execution = "foreign-root".into(),
            2 => value.scope.task_run = "foreign-run".into(),
            3 => value.scope.principal = "FOREIGN".into(),
            4 => value.scope.owner_execution = "foreign-creator".into(),
            5 => value.scope.parent_scope = Some("a".repeat(64)),
            6 => value.scope.source_execution = Some("source".into()),
            7 => value.scope.creation_call = Some("a".repeat(64)),
            8 => value.scope.logical_level = 0,
            9 => value.scope.logical_level = 17,
            10 => value.execution = "foreign-actor".into(),
            11 => value.source_execution = Some("source".into()),
            12 => value.call_key = Some("a".repeat(64)),
            13 => value.program = "COUNT".into(),
            14 => value.artifact = format!("sha256:{}", "2".repeat(64)),
            15 => value.attempt = 2,
            16 => value.kind = EntryKind::NativeCall,
            17 => value.scope.owner_attempt = 2,
            18 => value.scope.owner_selector = "program:FOREIGN".into(),
            _ => value.scope.owner_artifact = format!("sha256:{}", "2".repeat(64)),
        }
        value.scope.id = value.scope.expected_id().unwrap();
        value.metadata_digest = value.expected_metadata().unwrap();
        assert!(value.validate_for(&actor).is_err(), "case {case}");
    }
    let call = "a".repeat(64);
    let target = child(&actor, &call, "COUNT");
    let linked = good
        .cics_link(&actor, &target, &call, &selection(&target), 2)
        .unwrap();
    for case in 0..8 {
        let mut value = linked.clone();
        match case {
            0 => value.scope.selection.as_mut().unwrap().generation = 0,
            1 => value.scope.selection.as_mut().unwrap().generation = u64::MAX,
            2 => value.scope.selection.as_mut().unwrap().content_identity = "sha256:BAD".into(),
            3 => value.scope.selection.as_mut().unwrap().artifact = actor.artifact.as_str().into(),
            4 => value.scope.source_execution = Some("foreign".into()),
            5 => value.scope.creation_call = Some("b".repeat(64)),
            6 => value.kind = EntryKind::NativeCall,
            _ => value.kind = EntryKind::Root,
        }
        value.scope.id = value.scope.expected_id().unwrap();
        value.metadata_digest = value.expected_metadata().unwrap();
        assert!(value.validate_for(&target).is_err(), "linked case {case}");
    }
}

#[test]
fn invalid_child_identity_selection_or_depth_never_mutates_source() {
    let actor = root();
    let entry = Entry::root(&actor).unwrap();
    let before = actor.clone();
    let call = "a".repeat(64);
    let good_target = child(&actor, &call, "COUNT");
    for case in 0..6 {
        let mut target = good_target.clone();
        match case {
            0 => target.parent_execution_id = None,
            1 => target.execution_id = actor.execution_id.clone(),
            2 => {
                target.run_unit_id = RunUnitId::new("foreign", InvocationLimits::default()).unwrap()
            }
            3 => target.attempt += 1,
            4 => {
                target.bindings.clear();
                replay::bind_run_owner("foreign", &mut target.bindings).unwrap();
            }
            _ => {
                target.selector =
                    Selector::new("program:lower", InvocationLimits::default()).unwrap()
            }
        }
        assert!(
            entry.native_call(&actor, &target, &call).is_err(),
            "case {case}"
        );
    }
    assert!(entry.native_call(&actor, &good_target, "bad-call").is_err());
    for level in [0, 1, 3, 17] {
        assert!(
            entry
                .cics_link(&actor, &good_target, &call, &selection(&good_target), level)
                .is_err()
        );
    }
    let mut wrong = selection(&good_target);
    wrong.artifact = actor.artifact.clone();
    assert!(
        entry
            .cics_link(&actor, &good_target, &call, &wrong, 2)
            .is_err()
    );
    assert_eq!(actor, before);
}

#[test]
fn canonical_binding_rejects_unknown_duplicate_noncanonical_or_oversized_bytes() {
    let mut actor = root();
    let entry = Entry::root(&actor).unwrap();
    entry.bind(&mut actor).unwrap();
    let valid = actor.clone();
    assert_eq!(Entry::read(&actor).unwrap(), Some(entry.clone()));
    for case in 0..7 {
        let mut bytes = encoded(&entry).unwrap();
        let mut schema = SCHEMA;
        match case {
            0 => schema = "mainframe-env.cobol.storage-entry@2",
            1 => bytes.push(b' '),
            2 => {
                let mut value = serde_json::to_value(&entry).unwrap();
                value["extra"] = json!(1);
                bytes = serde_json::to_vec(&value).unwrap();
            }
            3 => bytes = b"{}".to_vec(),
            4 => bytes = vec![b' '; MAX_BINDING_BYTES + 1],
            5 => {
                bytes.pop();
                bytes.extend_from_slice(b",\"schema_version\":1}");
            }
            _ => {
                let mut value = entry.clone();
                value.metadata_digest = "0".repeat(64);
                bytes = encoded(&value).unwrap();
            }
        }
        actor.bindings.insert(
            BINDING.into(),
            BoundedPayload::new(schema, bytes, InvocationLimits::default()).unwrap(),
        );
        let before = actor.clone();
        assert!(Entry::read(&actor).is_err(), "case {case}");
        assert_eq!(actor, before);
    }
    assert_eq!(Entry::read(&valid).unwrap(), Some(entry));
}

#[test]
fn binding_is_idempotent_and_capacity_or_conflicts_preserve_invocation() {
    let mut actor = root();
    let entry = Entry::root(&actor).unwrap();
    assert!(Entry::read(&actor).unwrap().is_none());
    for i in 0..InvocationLimits::default().max_bindings {
        actor.bindings.insert(
            format!("test-{i}"),
            BoundedPayload::new("test@1", vec![], InvocationLimits::default()).unwrap(),
        );
    }
    let before = actor.clone();
    assert!(matches!(
        entry.bind(&mut actor),
        Err(HostProblem::ResourceExhausted)
    ));
    assert_eq!(actor, before);
    actor.bindings.clear();
    entry.bind(&mut actor).unwrap();
    let before = actor.clone();
    entry.bind(&mut actor).unwrap();
    assert_eq!(actor, before);
    actor.bindings.insert(
        BINDING.into(),
        BoundedPayload::new("foreign@1", vec![], InvocationLimits::default()).unwrap(),
    );
    let before = actor.clone();
    assert!(entry.bind(&mut actor).is_err());
    assert_eq!(actor, before);
}

#[test]
fn hashes_distinguish_selection_without_claiming_live_authority() {
    let actor = root();
    let entry = Entry::root(&actor).unwrap();
    let call = "a".repeat(64);
    let target = child(&actor, &call, "COUNT");
    let selected = selection(&target);
    let good = entry
        .cics_link(&actor, &target, &call, &selected, 2)
        .unwrap();
    let mut other = selected.clone();
    other.generation += 1;
    let changed = entry.cics_link(&actor, &target, &call, &other, 2).unwrap();
    assert_ne!(good.scope_id(), changed.scope_id());
    assert_ne!(
        good.member_key("COUNT").unwrap(),
        changed.member_key("COUNT").unwrap()
    );
    // Both are syntactically valid. Only the live provider and immutable catalog
    // may authorize the actual tuple; this codec has no runtime writer.
    assert!(changed.validate_for(&target).is_ok());
}
