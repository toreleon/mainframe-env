use super::*;
use crate::cobol::hardening::parent;
use mainframe_env_store::MemoryStore;
use mainframe_env_store_api::ProviderStateStore;

fn fixture() -> (
    MemoryStore,
    ScopedRun,
    Invocation,
    Entry,
    ProviderStateRecord,
) {
    let mut actor = parent();
    actor.selector = Selector::new("program:MAIN", InvocationLimits::default()).unwrap();
    actor.artifact = ArtifactRef::new(
        format!("sha256:{}", "1".repeat(64)),
        InvocationLimits::default(),
    )
    .unwrap();
    let key = run_key(&actor);
    let mut root = ScopedRun::fresh(&actor).unwrap();
    let call = "a".repeat(64);
    root.reserve_call_charge(&key, &call).unwrap();
    let mut target = actor.clone();
    super::super::super::super::replay::bind_protocol_owner(&actor, &mut target.bindings).unwrap();
    target.execution_id = ExecutionId::new(
        format!("online-call-execution-{call}"),
        InvocationLimits::default(),
    )
    .unwrap();
    target.parent_execution_id = Some(actor.execution_id.clone());
    target.selector = Selector::new("program:COUNT", InvocationLimits::default()).unwrap();
    target.artifact = ArtifactRef::new(
        format!("sha256:{}", "2".repeat(64)),
        InvocationLimits::default(),
    )
    .unwrap();
    let entry = root.root.native_call(&actor, &target, &call).unwrap();
    entry.bind(&mut target).unwrap();
    let name = entry.member_key("COUNT").unwrap();
    let mut instance = ScopedInstance {
        schema_version: 3,
        run_key: key.clone(),
        scope_entry: root.root.clone(),
        max_state_bytes: root.max_member_bytes,
        program: "COUNT".into(),
        artifact: target.artifact.as_str().into(),
        owner: Some(entry.clone()),
        initial: false,
        state: None,
        metadata_digest: String::new(),
    };
    instance.metadata_digest = instance.expected_digest(&name).unwrap();
    let row = ProviderStateRecord {
        namespace: namespace(&key),
        key: name.clone(),
        version: 1,
        payload: canonical(&instance).unwrap(),
    };
    root.members.insert(
        name,
        Member {
            scope: entry.scope_id().into(),
            program: "COUNT".into(),
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
    let store = MemoryStore::new(mainframe_env_store::StoreLimits::default());
    store
        .put_provider_states_atomic(vec![
            write(RUN_STATE_NAMESPACE, &key, &root, None).unwrap(),
            ProviderStateWrite {
                record: row.clone(),
                expected_version: None,
            },
        ])
        .unwrap();
    (store, root, target, entry, row)
}

fn root_update(root: &ScopedRun, fence: &SourceFence, expected: u64) -> ProviderStateMutation {
    let mut root = root.clone();
    let staged = fence.staged_write();
    let key = root.members.get_mut(&staged.record.key).unwrap();
    key.row_version = staged.record.version;
    root.refresh(
        &staged
            .record
            .namespace
            .strip_prefix(INSTANCE_NAMESPACE_PREFIX)
            .unwrap(),
    )
    .unwrap();
    ProviderStateMutation::Put(
        write(
            RUN_STATE_NAMESPACE,
            &staged
                .record
                .namespace
                .strip_prefix(INSTANCE_NAMESPACE_PREFIX)
                .unwrap(),
            &root,
            Some(expected),
        )
        .unwrap(),
    )
}

#[test]
fn source_guard_cannot_be_reconstructed_from_busy_rows_or_cross_thread() {
    let (store, _, actor, entry, row) = fixture();
    assert!(SourceFence::prepare(7, &actor, &store).is_err());
    let lease =
        LiveLease::register_known_reservation(7, actor.clone(), entry, row.clone()).unwrap();
    assert_eq!(lease.record().unwrap(), row);
    let actor_copy = actor.clone();
    std::thread::spawn(move || {
        let store = MemoryStore::new(mainframe_env_store::StoreLimits::default());
        assert!(SourceFence::prepare(7, &actor_copy, &store).is_err());
    })
    .join()
    .unwrap();
    let staged = SourceFence::prepare(7, &actor, &store).unwrap();
    drop(lease);
    assert!(staged.publish(&store, vec![]).is_err());
    assert!(SourceFence::prepare(7, &actor, &store).is_err());
    assert_eq!(
        store
            .get_provider_state(&row.namespace, &row.key)
            .unwrap()
            .unwrap(),
        row
    );
}

#[test]
fn known_source_cas_adopts_version_and_rejects_stale_prepared_guard() {
    let (store, root, actor, entry, row) = fixture();
    let lease =
        LiveLease::register_known_reservation(7, actor.clone(), entry, row.clone()).unwrap();
    let first = SourceFence::prepare(7, &actor, &store).unwrap();
    let stale = SourceFence::prepare(7, &actor, &store).unwrap();
    let update = root_update(&root, &first, 1);
    first.publish(&store, vec![update]).unwrap();
    let adopted = lease.record().unwrap();
    assert_eq!(adopted.version, 2);
    assert_eq!(adopted.payload, row.payload);
    assert_eq!(
        store
            .get_provider_state(&row.namespace, &row.key)
            .unwrap()
            .unwrap(),
        adopted
    );
    assert!(stale.publish(&store, vec![]).is_err());
    let indexed = store
        .get_provider_state(RUN_STATE_NAMESPACE, &run_key(&actor))
        .unwrap()
        .unwrap();
    ScopedRun::decode(&indexed)
        .unwrap()
        .validate_members(&run_key(&actor), &[adopted])
        .unwrap();
}

#[test]
fn atomic_root_conflict_preserves_source_row_and_live_token() {
    let (store, root, actor, entry, row) = fixture();
    let lease =
        LiveLease::register_known_reservation(7, actor.clone(), entry, row.clone()).unwrap();
    let staged = SourceFence::prepare(7, &actor, &store).unwrap();
    let update = root_update(&root, &staged, 999);
    assert!(staged.publish(&store, vec![update]).is_err());
    assert_eq!(*lease.0.row.borrow(), row);
    assert!(lease.record().is_err());
    assert!(SourceFence::prepare(7, &actor, &store).is_err());
    assert_eq!(
        store
            .get_provider_state(&row.namespace, &row.key)
            .unwrap()
            .unwrap(),
        row
    );
    let root_record = store
        .get_provider_state(RUN_STATE_NAMESPACE, &run_key(&actor))
        .unwrap()
        .unwrap();
    assert_eq!(root_record.version, 1);
}

#[test]
fn lost_acknowledgement_never_adopts_or_recovers_a_live_token() {
    let (store, root, actor, entry, row) = fixture();
    let lease =
        LiveLease::register_known_reservation(7, actor.clone(), entry, row.clone()).unwrap();
    let staged = SourceFence::prepare(7, &actor, &store).unwrap();
    let update = root_update(&root, &staged, 1);
    let result = staged.publish_with(vec![update], |mutations| {
        store.mutate_provider_states_atomic(mutations)?;
        Err(StoreError::Infrastructure(
            "injected acknowledgement loss after atomic commit".into(),
        ))
    });
    assert_eq!(result, Err(HostProblem::UnknownOutcome));
    assert_eq!(*lease.0.row.borrow(), row);
    assert!(lease.record().is_err());
    assert_eq!(
        store
            .get_provider_state(&row.namespace, &row.key)
            .unwrap()
            .unwrap()
            .version,
        2
    );
    assert!(SourceFence::prepare(7, &actor, &store).is_err());
}

#[test]
fn foreign_context_or_uncoupled_root_update_never_publishes_source() {
    let (store, _, actor, entry, row) = fixture();
    let lease =
        LiveLease::register_known_reservation(7, actor.clone(), entry, row.clone()).unwrap();
    assert!(SourceFence::prepare(8, &actor, &store).is_err());
    let mut changed = actor.clone();
    changed.priority += 1;
    assert!(SourceFence::prepare(7, &changed, &store).is_err());
    let staged = SourceFence::prepare(7, &actor, &store).unwrap();
    assert!(
        staged
            .publish_with(vec![], |_| panic!("uncoupled source must not publish"))
            .is_err()
    );
    assert_eq!(lease.record().unwrap(), row);
}

#[test]
fn lease_revocation_survives_registry_borrow_and_retained_observation() {
    let (store, root, actor, entry, row) = fixture();
    let lease =
        LiveLease::register_known_reservation(7, actor.clone(), entry, row.clone()).unwrap();
    let staged = SourceFence::prepare(7, &actor, &store).unwrap();
    let update = root_update(&root, &staged, 1);
    SOURCES.with(|sources| {
        let _borrow = sources.borrow_mut();
        drop(lease);
    });
    assert!(staged.publish(&store, vec![update]).is_err());
    assert_eq!(
        store
            .get_provider_state(&row.namespace, &row.key)
            .unwrap()
            .unwrap(),
        row
    );
}
