//! Independent physical storage controls, not MQ or compiled execution credit.
#[cfg(unix)]
#[path = "successor_owned_file.rs"]
mod owned_file;
use super::*;

fn fixture(store: &dyn PlatformStore) -> (RootProviderPublication, RootCustodySuccessor) {
    fixture_with_padding(store, 0)
}
fn fixture_with_padding(
    store: &dyn PlatformStore,
    padding: usize,
) -> (RootProviderPublication, RootCustodySuccessor) {
    fixture_with_extra(store, padding, None)
}
fn fixture_with_extra(
    store: &dyn PlatformStore,
    padding: usize,
    extra: Option<ProviderStateIdentity>,
) -> (RootProviderPublication, RootCustodySuccessor) {
    let (mut original, _) = original(store);
    // Four exact infrastructure identities are structural test data only.
    original.mutations = [
        "control",
        "manifest",
        "catalog",
        "metadata",
        "unknown-unit",
        "removed-get",
    ]
    .into_iter()
    .map(|key| put("native-root-a", key, 1, None))
    .collect();
    store.mutate_root_provider_states(original.clone()).unwrap();
    if padding != 0 {
        let mut padded = original.clone();
        padded.mutations = (0..padding)
            .map(|index| put("native-root-a", &format!("padding-{index:04}"), 1, None))
            .collect();
        store.mutate_root_provider_states(padded).unwrap();
    }
    if let Some(identity) = extra {
        let mut occurrence = original.occurrence.clone();
        occurrence.identity = identity.clone();
        store.register_root_provider_row(occurrence).unwrap();
        let mut publication = original.clone();
        publication.mutations = vec![put(&identity.namespace, &identity.key, 1, None)];
        store.mutate_root_provider_states(publication).unwrap();
    }
    let mut absent = original.occurrence.clone();
    absent.identity = ProviderStateIdentity {
        namespace: "native-root-a".into(),
        key: "absent-slot".into(),
    };
    store.register_root_provider_row(absent).unwrap();
    let cutoff = store
        .capture_root_custody_cutoff(
            &original.occurrence.claim,
            &original.occurrence.execution,
            11,
        )
        .unwrap();
    let mut admission = admission();
    admission.execution.execution_id = ExecutionId::new("successor", Default::default()).unwrap();
    admission.execution.run_unit_id = RunUnitId::new("successor-run", Default::default()).unwrap();
    admission.invocation_key = IdempotencyKey::new("successor-key", Default::default()).unwrap();
    admission.provider_namespaces = vec!["native-root-a".into(), "native-root-successor".into()];
    admission.provider_rows = vec![];
    admission.event = event(&admission.execution, 1, LifecycleEventKind::Admitted);
    admission.event.tick = 12;
    admission.notification = outbox(&admission.event);
    (
        original,
        RootCustodySuccessor {
            cutoff,
            admission,
            handoff_rows: ["control", "manifest", "catalog", "metadata"]
                .into_iter()
                .map(|key| ProviderStateIdentity {
                    namespace: "native-root-a".into(),
                    key: key.into(),
                })
                .collect(),
        },
    )
}
fn refuses_successor(
    store: &dyn PlatformStore,
    original: &RootProviderPublication,
    bad: RootCustodySuccessor,
) {
    let before = observed(store, original);
    assert!(store.admit_root_successor(bad).is_err());
    assert_eq!(observed(store, original), before);
}
fn controls(store: &dyn PlatformStore) {
    let (original, request) = fixture(store);
    for fault in 0..12 {
        let mut bad = request.clone();
        match fault {
            0 => bad.cutoff.fenced_epoch += 1,
            1 => bad.cutoff.fenced.version += 1,
            2 => bad.cutoff.marker.payload.push(b' '),
            3 => bad.cutoff.preserved.actors[0].execution.attempt += 1,
            4 => {
                bad.admission.execution.execution_id =
                    original.occurrence.execution.execution_id.clone()
            }
            5 => {
                bad.admission.execution.run_unit_id =
                    original.occurrence.execution.run_unit_id.clone()
            }
            6 => {
                bad.admission.execution.principal =
                    PrincipalId::new("FOREIGN", Default::default()).unwrap()
            }
            7 => bad.admission.event.tick = 10,
            8 => bad.admission.provider_namespaces = vec!["foreign-scope".into()],
            9 => bad.handoff_rows[0].key = "absent-control".into(),
            10 => bad.handoff_rows.pop().map(|_| ()).unwrap(),
            11 => bad
                .cutoff
                .preserved
                .provider_dependencies
                .pop()
                .map(|_| ())
                .unwrap(),
            _ => unreachable!(),
        }
        refuses_successor(store, &original, bad);
    }
    let before = observed(store, &original);
    let claim = store.admit_root_successor(request.clone()).unwrap();
    assert_eq!(claim.admission(), &request.admission);
    assert_eq!(claim.inserted_row().version, 1);
    let doc: serde_json::Value = serde_json::from_slice(&claim.inserted_row().payload).unwrap();
    assert_eq!(doc["schema"], "mainframe-env.core-root-ownership@2");
    assert_eq!(doc["phase"], 0);
    assert_eq!(doc["actors"].as_array().unwrap().len(), 1);
    assert_eq!(
        doc["scope_versions"],
        serde_json::json!([["native-root-a", 2], ["native-root-successor", 1]])
    );
    let after = observed(store, &original);
    assert_eq!(
        (&after.0, &after.1, &after.2, &after.4),
        (&before.0, &before.1, &before.2, &before.4)
    );
    assert_eq!(
        after
            .3
            .iter()
            .filter(|notice| notice.execution_id == original.occurrence.execution.execution_id)
            .cloned()
            .collect::<Vec<_>>(),
        before.3
    );
    for row in before
        .5
        .iter()
        .filter(|row| row.namespace == "native-root-a" || row.namespace == ROOT_DRIVER_NAMESPACE)
    {
        assert_eq!(
            store.get_provider_state(&row.namespace, &row.key).unwrap(),
            Some(row.clone())
        );
    }
    assert_eq!(
        store
            .get_provider_state("durable-root-scope-v1", "native-root-a")
            .unwrap(),
        Some(ProviderStateRecord {
            namespace: "durable-root-scope-v1".into(),
            key: "native-root-a".into(),
            version: 2,
            payload: b"successor".to_vec(),
        })
    );
    assert_eq!(
        store
            .get_provider_state(
                ROOT_CUSTODY_NAMESPACE,
                original.occurrence.execution.execution_id.as_str()
            )
            .unwrap()
            .unwrap()
            .version,
        2
    );
    let admitted = store
        .get_execution(&claim.admission().execution.execution_id)
        .unwrap()
        .unwrap();
    assert_eq!(admitted, request.admission.execution);
    refuses_successor(store, &original, request.clone());
    let mut old = original.clone();
    old.occurrence.observed_tick = 12;
    old.mutations = vec![put("native-root-a", "control", 2, Some(1))];
    refuses(store, &original, old);
    // Primitive writers and actual next root writers share physical fences.
    for (key, payload) in [
        ("unknown-unit", b"exact-owned-bytes".as_slice()),
        ("manifest", b"changed-bytes".as_slice()),
    ] {
        let frozen = observed(store, &original);
        assert!(
            store
                .put_provider_state(
                    ProviderStateRecord {
                        namespace: "native-root-a".into(),
                        key: key.into(),
                        version: 2,
                        payload: payload.to_vec()
                    },
                    Some(1)
                )
                .is_err()
        );
        assert!(
            store
                .delete_provider_state("native-root-a", key, 1)
                .is_err()
        );
        assert_eq!(observed(store, &original), frozen);
    }
    // Both pins allow only exact bytes + exact version CAS; unknown remains
    // frozen even if its payload is unchanged. No unit disposition is inferred.
    store
        .put_provider_state(
            ProviderStateRecord {
                namespace: "native-root-a".into(),
                key: "manifest".into(),
                version: 2,
                payload: b"exact-owned-bytes".to_vec(),
            },
            Some(1),
        )
        .unwrap();
    assert_eq!(
        store
            .get_provider_state("native-root-a", "unknown-unit")
            .unwrap()
            .unwrap()
            .version,
        1
    );
    let mut current = admitted;
    for (sequence, state, kind) in [
        (2, ExecutionState::Queued, LifecycleEventKind::Queued),
        (3, ExecutionState::Running, LifecycleEventKind::Started),
    ] {
        let mut next = event(&current, sequence, kind);
        next.tick = 12;
        current = store
            .commit_execution_step(
                &current.execution_id,
                current.version,
                Some(state),
                next.clone(),
                None,
                None,
                None,
                outbox(&next),
            )
            .unwrap();
    }
    let actual_intent = intent(store, &current);
    let writer = RootProviderPublication {
        occurrence: RootProviderRowAdmission {
            claim,
            execution: current,
            effect_key: actual_intent.key.clone(),
            effect_sequence: actual_intent.sequence,
            request_digest: actual_intent.request_digest,
            identity: ProviderStateIdentity {
                namespace: "native-root-a".into(),
                key: "control".into(),
            },
            observed_tick: 12,
        },
        intent: actual_intent.intent.clone(),
        mutations: vec![put("native-root-a", "control", 2, Some(1))],
    };
    store.mutate_root_provider_states(writer.clone()).unwrap();
    for mutation in [
        put("native-root-a", "unknown-unit", 2, Some(1)),
        put("native-root-a", "manifest", 3, Some(2)),
        ProviderStateMutation::Delete {
            namespace: "native-root-a".into(),
            key: "metadata".into(),
            expected_version: 1,
        },
        put("native-root-a", "absent-slot", 1, None),
    ] {
        let mut bad = writer.clone();
        bad.mutations = vec![mutation];
        if let ProviderStateMutation::Put(write) = &mut bad.mutations[0]
            && write.record.key == "manifest"
        {
            write.record.payload = b"different-payload".to_vec();
        }
        refuses(store, &original, bad);
    }
    assert_eq!(
        store
            .get_provider_state("native-root-a", "control")
            .unwrap()
            .unwrap()
            .version,
        2
    );
    assert_eq!(
        store
            .get_provider_state("native-root-a", "removed-get")
            .unwrap()
            .unwrap()
            .version,
        1
    );
    let before = observed(store, &original);
    assert!(
        store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: "native-root-a".into(),
                    key: "absent-slot".into(),
                    version: 1,
                    payload: b"new-row".to_vec()
                },
                None
            )
            .is_err()
    );
    assert!(
        store
            .get_provider_state("native-root-a", "absent-slot")
            .unwrap()
            .is_none()
    );
    assert_eq!(observed(store, &original), before);
}
fn whole_observation(
    store: &dyn PlatformStore,
    original: &RootProviderPublication,
) -> ObservedSnapshot {
    let mut state = observed(store, original);
    state.5.clear();
    // The fixed producer contains <=4096 rows in every selected prefix. All
    // rows are read, including the last padding/fence; no truncated equality.
    for prefix in [
        "durable-",
        "native-root-",
        "exact-",
        "legacy-",
        "mq-",
        "cobol-",
    ] {
        state
            .5
            .extend(store.list_provider_state_prefix(prefix, 4096).unwrap());
    }
    state
}
#[test]
fn successor_original_operation_max_and_one_extra_scope_refusal() {
    for extra_scope in [false, true] {
        let store = MemoryStore::new(StoreLimits::default());
        let (original, mut request) = fixture_with_padding(&store, 2025);
        // Independent fixed input inventory: original capture27 + padding2025
        // + retained fence/marker2 =2054. Planned new root/scope/marker4,
        // fences2033 and actual admission+actor/run5 add2042 => literal4096.
        // One extra new scope gives4097; dropping the retained2 would wrongly
        // admit4095. Original physical Memory limits remain unchanged.
        assert_eq!(request.cutoff.preserved.captured_usage().unwrap().0, 2052);
        assert_eq!(request.cutoff.captured_usage().unwrap().0, 2054);
        if extra_scope {
            request
                .admission
                .provider_namespaces
                .push("native-root-extra".into());
        }
        let before = whole_observation(&store, &original);
        if extra_scope {
            assert_eq!(
                store.admit_root_successor(request),
                Err(StoreError::CapacityExceeded)
            );
            assert_eq!(whole_observation(&store, &original), before);
        } else {
            let claim = store.admit_root_successor(request).unwrap();
            assert_eq!(claim.inserted_row().version, 1);
            let after = whole_observation(&store, &original);
            assert_eq!(
                (&after.0, &after.1, &after.2, &after.4),
                (&before.0, &before.1, &before.2, &before.4)
            );
            for row in before.5.iter().filter(|row| {
                row.namespace == "native-root-a" || row.namespace == ROOT_DRIVER_NAMESPACE
            }) {
                assert_eq!(
                    store.get_provider_state(&row.namespace, &row.key).unwrap(),
                    Some(row.clone())
                );
            }
        }
    }
}
#[test]
fn custody_structural_aggregate_byte_max_and_plus_one_refuse() {
    let store = MemoryStore::new(StoreLimits::default());
    let (_, request) = fixture(&store);
    let mut cutoff = request.cutoff;
    let (rows, bytes) = cutoff.captured_usage().unwrap();
    // Structural quota control only; these deliberate padding bytes are not
    // a manager/native proof and are never submitted as a custody receipt.
    let size = cutoff.marker.payload.len() + (64 * 1024 * 1024 - bytes);
    cutoff.marker.payload.resize(size, 0);
    assert_eq!(cutoff.captured_usage().unwrap(), (rows, 64 * 1024 * 1024));
    cutoff.marker.payload.push(0);
    assert_eq!(cutoff.captured_usage(), Err(StoreError::CapacityExceeded));
}

fn independent_terminal_owner(store: &dyn PlatformStore) -> ExecutionRecord {
    let mut execution = admission().execution;
    execution.execution_id = ExecutionId::new("foreign-archive-owner", Default::default()).unwrap();
    execution.run_unit_id = RunUnitId::new("foreign-archive-run", Default::default()).unwrap();
    store.create_execution(execution.clone()).unwrap();
    for (state, tick) in [
        (ExecutionState::Queued, 1),
        (ExecutionState::Running, 2),
        (ExecutionState::Completing, 3),
        (ExecutionState::Completed, 4),
    ] {
        execution = store
            .transition_execution(&execution.execution_id, execution.version, state, tick)
            .unwrap();
    }
    execution
}
fn provider_graph_plan(
    store: &dyn PlatformStore,
    owner: &ExecutionRecord,
    target: RetentionTarget,
    row: ProviderStateRecord,
) -> ProviderStateArchiveDeletion {
    ProviderStateArchiveDeletion {
        expected_epoch: store.provider_state_retention_epoch().unwrap(),
        target,
        archived_tick: 30,
        watermark_tick: 20,
        rows: vec![ProviderRetentionRow {
            row,
            owner_execution: Some(owner.execution_id.clone()),
            owner_run_unit: Some(owner.run_unit_id.clone()),
            retention_tick: 15,
            observation: None,
            dependency: ProviderRetentionDependency::ProviderGraph {
                required_rows: vec![],
                required_executions: vec![],
            },
        }],
    }
}
fn retention_pin_control(store: &dyn PlatformStore, target: RetentionTarget, namespace: &str) {
    // An independently created terminal storage owner is deliberately foreign;
    // it never relabels or completes the actual uncertain predecessor.
    let foreign = independent_terminal_owner(store);
    let identity = ProviderStateIdentity {
        namespace: namespace.into(),
        key: "custody-replay".into(),
    };
    let (original, request) = fixture_with_extra(store, 0, Some(identity.clone()));
    store.admit_root_successor(request).unwrap();
    let pinned = store
        .get_provider_state(namespace, &identity.key)
        .unwrap()
        .unwrap();
    // Conditional guard control: original provider-retention behavior remains
    // usable for an actual unpinned row, with the exact original graph guards.
    let control = ProviderStateRecord {
        namespace: namespace.into(),
        key: "unowned-control".into(),
        version: 1,
        payload: b"unpinned-control-row".to_vec(),
    };
    store.put_provider_state(control.clone(), None).unwrap();
    let accepted = store
        .archive_provider_state_deletion(provider_graph_plan(
            store,
            &foreign,
            target,
            control.clone(),
        ))
        .unwrap();
    assert_eq!(accepted.rows.len(), 1);
    assert_eq!(accepted.rows[0].namespace, control.namespace);
    assert_eq!(accepted.rows[0].key, control.key);
    assert_eq!(accepted.rows[0].version, control.version);
    assert_eq!(accepted.rows[0].payload, control.payload);
    assert!(
        store
            .get_provider_state(namespace, "unowned-control")
            .unwrap()
            .is_none()
    );
    let before = whole_observation(store, &original);
    let archives = store.retention_archives(target, 256).unwrap();
    let usage = store.provider_retention_archive_usage(target).unwrap();
    let forged = provider_graph_plan(store, &foreign, target, pinned.clone());
    assert_eq!(
        store.archive_provider_state_deletion(forged),
        Err(StoreError::InvalidTransition)
    );
    assert_eq!(
        store.get_provider_state(namespace, &identity.key).unwrap(),
        Some(pinned)
    );
    assert_eq!(whole_observation(store, &original), before);
    assert_eq!(
        store.get_execution(&foreign.execution_id).unwrap(),
        Some(foreign)
    );
    assert_eq!(store.retention_archives(target, 256).unwrap(), archives);
    assert_eq!(
        store.provider_retention_archive_usage(target).unwrap(),
        usage
    );
}
#[test]
fn successor_provider_archive_cannot_substitute_terminal_owner_memory() {
    for (target, namespace) in [
        (RetentionTarget::MqReplay, "mq-v1-replay"),
        (RetentionTarget::CobolLifecycle, "cobol-call-protocol@2"),
    ] {
        retention_pin_control(&MemoryStore::new(StoreLimits::default()), target, namespace);
    }
}
#[test]
fn successor_provider_archive_cannot_substitute_terminal_owner_sqlite() {
    for (target, namespace) in [
        (RetentionTarget::MqReplay, "mq-v1-replay"),
        (RetentionTarget::CobolLifecycle, "cobol-call-protocol@2"),
    ] {
        let owned = OwnedSqlite::new();
        retention_pin_control(owned.store(), target, namespace);
    }
}

fn pin_codec_control(store: &dyn PlatformStore, mode: usize) {
    // Independent ASCII JSON inventory: fixed syntax94 + digest129 + quoted
    // schema34/owner10/namespace15/key quotes2 =284.635 U+0001 escapes3810
    // plus two literal ASCII bytes gives bound4096; one ASCII byte gives4097.
    // The third case's700 legal control bytes force encoded payload >4096.
    let mut key = "\u{1}".repeat(if mode == 2 { 700 } else { 635 });
    if mode != 2 {
        key.push_str(if mode == 0 { "aa" } else { "aaa" });
    }
    assert!(key.len() <= 1024);
    let (original, request) = fixture_with_extra(
        store,
        0,
        Some(ProviderStateIdentity {
            namespace: "native-root-a".into(),
            key: key.clone(),
        }),
    );
    let before = whole_observation(store, &original);
    if mode == 0 {
        store.admit_root_successor(request).unwrap();
        assert_eq!(
            store
                .get_provider_state("native-root-a", &key)
                .unwrap()
                .unwrap()
                .payload,
            b"exact-owned-bytes"
        );
        for pin in store
            .list_provider_state("durable-root-custody-pin-v1", 256)
            .unwrap()
        {
            assert!(pin.payload.len() <= 4096);
        }
        assert!(
            store
                .put_provider_state(
                    ProviderStateRecord {
                        namespace: "native-root-a".into(),
                        key,
                        version: 2,
                        payload: b"exact-owned-bytes".to_vec()
                    },
                    Some(1)
                )
                .is_err()
        );
    } else {
        assert_eq!(
            store.admit_root_successor(request),
            Err(StoreError::CapacityExceeded)
        );
        assert_eq!(whole_observation(store, &original), before);
    }
}
#[test]
fn successor_legal_json_control_identity_max_and_overflow_memory() {
    for mode in 0..3 {
        pin_codec_control(&MemoryStore::new(StoreLimits::default()), mode);
    }
}
#[test]
fn successor_legal_json_control_identity_max_and_overflow_sqlite() {
    for mode in 0..3 {
        let owned = OwnedSqlite::new();
        pin_codec_control(owned.store(), mode);
    }
}

fn actor_bound_controls(store: &dyn PlatformStore) {
    let (original, mut request) = fixture(store);
    assert_eq!(request.cutoff.preserved.actors.len(), 1);
    let template = request.cutoff.preserved.actors[0].clone();
    // Deliberate structural padding tests the aggregate guard only. These
    // actors are never enrolled, submitted as a valid capture or credited as
    // compiled/native ownership. The max case only validates bounded shape.
    for index in 1..255 {
        let mut actor = template.clone();
        actor.execution.execution_id =
            ExecutionId::new(format!("actor-bound-{index:03}"), Default::default()).unwrap();
        actor.parent = Some(original.occurrence.execution.execution_id.clone());
        actor.effects.clear();
        actor.last_event.execution_id = actor.execution.execution_id.clone();
        request.cutoff.preserved.actors.push(actor);
    }
    assert_eq!(request.cutoff.preserved.actors.len(), 255);
    request.validate_bounds().unwrap(); // literal aggregate256
    let mut extra = template;
    extra.execution.execution_id = ExecutionId::new("actor-bound-255", Default::default()).unwrap();
    extra.parent = Some(original.occurrence.execution.execution_id.clone());
    extra.effects.clear();
    extra.last_event.execution_id = extra.execution.execution_id.clone();
    request.cutoff.preserved.actors.push(extra);
    assert_eq!(request.cutoff.preserved.actors.len(), 256);
    request.cutoff.validate_bounds().unwrap(); // predecessor itself is bounded
    assert_eq!(request.validate_bounds(), Err(StoreError::CapacityExceeded));
    let before = whole_observation(store, &original);
    assert_eq!(
        store.admit_root_successor(request),
        Err(StoreError::CapacityExceeded)
    );
    assert_eq!(whole_observation(store, &original), before);
}
#[test]
fn successor_aggregate_actor_max_and_max_plus_one_memory() {
    actor_bound_controls(&MemoryStore::new(StoreLimits::default()));
}
#[test]
fn successor_aggregate_actor_max_and_max_plus_one_sqlite() {
    let owned = OwnedSqlite::new();
    actor_bound_controls(owned.store());
}

#[test]
fn successor_memory_atomic_versioned_scope_and_unknown_fences() {
    controls(&MemoryStore::new(StoreLimits::default()));
}
#[test]
fn successor_sqlite_atomic_versioned_scope_and_unknown_fences() {
    let owned = OwnedSqlite::new();
    controls(owned.store());
}
