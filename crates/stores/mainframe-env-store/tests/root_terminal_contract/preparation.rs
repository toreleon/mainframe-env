//! Physical initial admission fixtures, not server wiring or installed acceptance.
use super::*;
#[path = "preparation_physical.rs"]
mod physical;

fn request(store: &dyn PlatformStore) -> RootPreparationPublication {
    let mut original = admission();
    original.provider_rows = ["old", "new"]
        .into_iter()
        .map(|key| ProviderStateIdentity {
            namespace: "exact-preparation".into(),
            key: key.into(),
        })
        .collect();
    RootPreparationPublication {
        execution: original.execution.clone(),
        claim: store.admit_root_driver(original).unwrap(),
        anchor: ProviderStateIdentity {
            namespace: "native-root-owned-test".into(),
            key: "anchor".into(),
        },
        observed_tick: 11,
        mutations: vec![attributed::put("native-root-owned-test", "first", 1, None)],
    }
}
fn snapshot(
    store: &dyn PlatformStore,
    request: &RootPreparationPublication,
) -> (
    Option<ExecutionRecord>,
    Vec<LifecycleEvent>,
    Vec<OutboxRecord>,
    Vec<AuditSubjectRecord>,
    Vec<ProviderStateRecord>,
    u64,
    u64,
) {
    let mut rows = Vec::new();
    for prefix in ["durable-", "native-", "exact-", "legacy-", "audit"] {
        rows.extend(store.list_provider_state_prefix(prefix, 256).unwrap());
    }
    (
        store
            .get_execution(&request.execution.execution_id)
            .unwrap(),
        store
            .events(&request.execution.execution_id, 1, 32)
            .unwrap(),
        store.pending_notifications(64).unwrap(),
        store
            .audit_subject_records(&request.execution.execution_id, 64)
            .unwrap(),
        rows,
        store.provider_state_retention_epoch().unwrap(),
        store.advance_logical_clock(1).unwrap(),
    )
}
fn refuses(
    store: &dyn PlatformStore,
    original: &RootPreparationPublication,
    bad: RootPreparationPublication,
) {
    let before = snapshot(store, original);
    assert!(
        store.mutate_root_preparation_states(bad.clone()).is_err(),
        "accepted {bad:?}"
    );
    assert_eq!(snapshot(store, original), before);
}
fn current(store: &dyn PlatformStore) {
    let mut input = request(store);
    let before = snapshot(store, &input);
    store.mutate_root_preparation_states(input.clone()).unwrap();
    input.mutations = vec![attributed::put("exact-preparation", "old", 1, None)];
    store.mutate_root_preparation_states(input.clone()).unwrap();
    input.mutations = vec![
        ProviderStateMutation::Move {
            record: ProviderStateRecord {
                namespace: "exact-preparation".into(),
                key: "new".into(),
                version: 2,
                payload: b"exact-owned-bytes".to_vec(),
            },
            old_key: "old".into(),
            expected_version: 1,
        },
        ProviderStateMutation::Delete {
            namespace: "native-root-owned-test".into(),
            key: "first".into(),
            expected_version: 1,
        },
    ];
    store.mutate_root_preparation_states(input.clone()).unwrap();
    let after = snapshot(store, &input);
    assert_eq!(
        (after.0, after.1, after.2, after.3),
        (before.0, before.1, before.2, before.3)
    );
    assert!(
        store
            .get_provider_state("exact-preparation", "old")
            .unwrap()
            .is_none()
    );
    assert_eq!(
        store
            .get_provider_state("exact-preparation", "new")
            .unwrap()
            .unwrap()
            .version,
        2
    );
    assert_eq!(
        store
            .get_provider_state(ROOT_DRIVER_NAMESPACE, input.execution.execution_id.as_str())
            .unwrap()
            .as_ref(),
        Some(input.claim.inserted_row())
    );
    assert_eq!(after.6, 11);
}
fn substitutions(store: &dyn PlatformStore) {
    let input = request(store);
    for field in 0..19 {
        let mut bad = input.clone();
        let limits = InvocationLimits::default();
        match field {
            0 => bad.execution.execution_id = ExecutionId::new("foreign", limits).unwrap(),
            1 => bad.execution.run_unit_id = RunUnitId::new("foreign", limits).unwrap(),
            2 => bad.execution.principal = PrincipalId::new("OTHER", limits).unwrap(),
            3 => bad.execution.selector = Selector::new("program:OTHER", limits).unwrap(),
            4 => {
                bad.execution.artifact =
                    ArtifactRef::new(format!("sha256:{}", "b".repeat(64)), limits).unwrap()
            }
            5 => bad.execution.attempt += 1,
            6 => bad.execution.version += 1,
            7 => bad.execution.state = ExecutionState::Queued,
            8 => bad.execution.state = ExecutionState::Running,
            9 => bad.execution.owner_lease = Some("owner".into()),
            10 => bad.execution.lease_expiry_tick = Some(99),
            11 => bad.execution.terminal_tick = Some(10),
            12 => bad.observed_tick = 0,
            13 => bad.observed_tick = 9,
            14 => bad.observed_tick = 100,
            15 => bad.observed_tick = u64::MAX,
            16 => bad.anchor.key.clear(),
            17 => bad.anchor.namespace = "unregistered".into(),
            _ => {
                let mut admission = bad.claim.admission().clone();
                admission.configuration_digest[0] ^= 1;
                bad.claim = admission
                    .observe_inserted(bad.claim.inserted_row())
                    .unwrap();
            }
        }
        refuses(store, &input, bad);
    }
    for mutation in [
        attributed::put("unregistered", "x", 1, None),
        attributed::put("exact-preparation", "unregistered", 1, None),
        attributed::put("durable-effect", "x", 1, None),
        attributed::put("jes-worker-meta", "logical-clock", 1, None),
    ] {
        let mut bad = input.clone();
        bad.mutations.push(mutation);
        refuses(store, &input, bad);
    }
    for (old_key, key) in [("old", "foreign"), ("foreign", "new")] {
        let mut bad = input.clone();
        bad.mutations.push(ProviderStateMutation::Move {
            record: ProviderStateRecord {
                namespace: "exact-preparation".into(),
                key: key.into(),
                version: 2,
                payload: vec![1],
            },
            old_key: old_key.into(),
            expected_version: 1,
        });
        refuses(store, &input, bad);
    }
    let mut stale = input.clone();
    store.advance_logical_clock(12).unwrap();
    stale.observed_tick = 11;
    refuses(store, &input, stale);
}
fn phases(store: &dyn PlatformStore, uncertain: bool) {
    let input = request(store);
    if uncertain {
        store
            .fence_root_driver(&input.claim, &input.execution, 11)
            .unwrap();
    } else {
        let e = event(&input.execution, 2, LifecycleEventKind::Queued);
        store
            .commit_execution_step(
                &input.execution.execution_id,
                1,
                Some(ExecutionState::Queued),
                e.clone(),
                None,
                None,
                None,
                outbox(&e),
            )
            .unwrap();
    }
    refuses(store, &input, input.clone());
}
fn late_cas(store: &dyn PlatformStore) {
    let input = request(store);
    let mut bad = input.clone();
    bad.mutations.push(attributed::put(
        "native-root-owned-test",
        "missing",
        2,
        Some(1),
    ));
    refuses(store, &input, bad);
    store.mutate_root_preparation_states(input).unwrap();
}
fn bounds(store: &dyn PlatformStore) {
    let input = request(store);
    let mut bad = input.clone();
    bad.mutations.clear();
    refuses(store, &input, bad);
    let mut bad = input.clone();
    bad.mutations = vec![input.mutations[0].clone(); MAX_ROOT_OPERATIONS + 1];
    refuses(store, &input, bad);
    let mut bad = input.clone();
    bad.mutations = vec![
        ProviderStateMutation::Move {
            record: ProviderStateRecord {
                namespace: "exact-preparation".into(),
                key: "new".into(),
                version: 2,
                payload: vec![1]
            },
            old_key: "old".into(),
            expected_version: 1,
        };
        MAX_ROOT_OPERATIONS / 2 + 1
    ];
    refuses(store, &input, bad);
    assert_eq!(
        input.validate_bounds(1 << 20, MAX_ROOT_PAYLOAD_BYTES),
        Err(StoreError::CapacityExceeded)
    );
}

#[test]
fn memory_preparation_current_exact_namespace_row_move_delete() {
    current(&MemoryStore::new(StoreLimits::default()));
}
#[test]
fn sqlite_preparation_current_exact_namespace_row_move_delete() {
    current(OwnedSqlite::new().store());
}
#[test]
fn memory_preparation_all_original_substitutions_refuse() {
    substitutions(&MemoryStore::new(StoreLimits::default()));
}
#[test]
fn sqlite_preparation_all_original_substitutions_refuse() {
    substitutions(OwnedSqlite::new().store());
}
#[test]
fn memory_preparation_current_state_and_uncertain_refuse() {
    for uncertain in [false, true] {
        phases(&MemoryStore::new(StoreLimits::default()), uncertain);
    }
}
#[test]
fn sqlite_preparation_current_state_and_uncertain_refuse() {
    for uncertain in [false, true] {
        phases(OwnedSqlite::new().store(), uncertain);
    }
}
#[test]
fn memory_preparation_late_cas_whole_rollback() {
    late_cas(&MemoryStore::new(StoreLimits::default()));
}
#[test]
fn sqlite_preparation_late_cas_whole_rollback() {
    late_cas(OwnedSqlite::new().store());
}
#[test]
fn memory_preparation_max_plus_one_move_two_and_capture_bounds() {
    bounds(&MemoryStore::new(StoreLimits::default()));
}
#[test]
fn sqlite_preparation_max_plus_one_move_two_and_capture_bounds() {
    bounds(OwnedSqlite::new().store());
}
#[test]
fn memory_preparation_late_row_quota_rolls_back() {
    let store = MemoryStore::new(StoreLimits {
        max_provider_state: 8,
        ..StoreLimits::default()
    });
    let mut input = request(&store);
    input
        .mutations
        .push(attributed::put("native-root-owned-test", "second", 1, None));
    refuses(&store, &input, input.clone());
}
#[test]
fn sqlite_preparation_late_row_quota_rolls_back() {
    let fixture = OwnedSqlite::with_rows(11);
    let mut input = request(fixture.store());
    input
        .mutations
        .push(attributed::put("native-root-owned-test", "second", 1, None));
    refuses(fixture.store(), &input, input.clone());
}
#[test]
fn sqlite_preparation_owned_reopen_keeps_original_admitted_history() {
    let mut fixture = OwnedSqlite::new();
    let input = request(fixture.store());
    fixture
        .store()
        .mutate_root_preparation_states(input.clone())
        .unwrap();
    let before = snapshot(fixture.store(), &input);
    drop(fixture.store.take());
    fixture.store = Some(
        SqliteStateStore::open(
            &format!("sqlite://{}?mode=rw", fixture.path.display()),
            1 << 20,
            1024,
        )
        .unwrap(),
    );
    assert_eq!(snapshot(fixture.store(), &input), before);
}
#[test]
fn preparation_unsupported_adapter_refuses() {
    struct Unsupported;
    impl JournalStore for Unsupported {
        fn admit_execution(
            &self,
            _: ExecutionRecord,
            _: LifecycleEvent,
            _: OutboxRecord,
        ) -> Result<(), StoreError> {
            Err(StoreError::InvalidTransition)
        }
        fn commit_execution_step(
            &self,
            _: &ExecutionId,
            _: u64,
            _: Option<ExecutionState>,
            _: LifecycleEvent,
            _: Option<EffectRecord>,
            _: Option<AuditRecord>,
            _: Option<CheckpointRecord>,
            _: OutboxRecord,
        ) -> Result<ExecutionRecord, StoreError> {
            Err(StoreError::InvalidTransition)
        }
    }
    let input = request(&MemoryStore::new(StoreLimits::default()));
    assert_eq!(
        Unsupported.mutate_root_preparation_states(input),
        Err(StoreError::InvalidTransition)
    );
}

fn running_closing_terminal(store: &dyn PlatformStore, normal: bool) {
    let (claim, execution) = running(store);
    let input = RootPreparationPublication {
        execution: claim.admission().execution.clone(),
        claim: claim.clone(),
        anchor: ProviderStateIdentity {
            namespace: "native-root-owned-test".into(),
            key: "anchor".into(),
        },
        observed_tick: 10,
        mutations: vec![attributed::put("native-root-owned-test", "first", 1, None)],
    };
    refuses(store, &input, input.clone());
    // An actual retained original intent cannot provide an effect-free bypass.
    let mut effect = orphan_intent();
    effect.execution_id = execution.execution_id.clone();
    effect.run_unit_id = execution.run_unit_id.clone();
    effect.intent.owner = execution.execution_id.clone();
    store.record_intent(effect.clone()).unwrap();
    refuses(store, &input, input.clone());
    // Resolve using the existing effect authority before exercising terminal closure.
    effect.state = EffectState::Completed;
    effect.result_digest = Some([3; 32]);
    effect.resolved_tick = Some(10);
    store.record_result(&effect.key, effect.clone()).unwrap();
    let terminal = publication(store, &claim, &execution, normal);
    refuses(store, &input, input.clone());
    store.commit_root_terminal_step(terminal).unwrap();
    refuses(store, &input, input.clone());
    assert_eq!(store.effect(&effect.key).unwrap(), Some(effect));
}
#[test]
fn memory_preparation_actual_running_call_closing_and_both_terminal_phases_refuse() {
    for normal in [false, true] {
        running_closing_terminal(&MemoryStore::new(StoreLimits::default()), normal);
    }
}
#[test]
fn sqlite_preparation_actual_running_call_closing_and_both_terminal_phases_refuse() {
    for normal in [false, true] {
        running_closing_terminal(OwnedSqlite::new().store(), normal);
    }
}

#[test]
fn memory_preparation_narrow_backend_payload_limit_refuses() {
    let store = MemoryStore::new(StoreLimits {
        max_blob_bytes: 16384,
        ..StoreLimits::default()
    });
    let mut input = request(&store);
    if let ProviderStateMutation::Put(write) = &mut input.mutations[0] {
        write.record.payload = vec![0; 16385];
    }
    refuses(&store, &input, input.clone());
}
#[test]
fn sqlite_preparation_narrow_backend_payload_limit_refuses() {
    let fixture = OwnedSqlite::new();
    let mut input = request(fixture.store());
    if let ProviderStateMutation::Put(write) = &mut input.mutations[0] {
        write.record.payload = vec![0; (1 << 20) + 1];
    }
    refuses(fixture.store(), &input, input.clone());
}
