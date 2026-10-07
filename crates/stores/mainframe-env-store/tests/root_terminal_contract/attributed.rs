//! Physical original-record fixtures; not installed host/SAF/succession proof.
use super::*;
#[path = "checked_read.rs"]
mod checked_read;
#[path = "attributed_physical.rs"]
mod physical;
#[path = "replay_refusal.rs"]
mod replay_refusal;

fn start(
    store: &dyn PlatformStore,
    label: &str,
    scopes: Vec<String>,
    exact: Vec<ProviderStateIdentity>,
) -> (RootDriverClaim, ExecutionRecord) {
    let bounds = InvocationLimits::default();
    let mut original = admission();
    original.execution.execution_id = ExecutionId::new(label, bounds).unwrap();
    original.execution.run_unit_id = RunUnitId::new(format!("{label}-run"), bounds).unwrap();
    original.provider_namespaces = scopes;
    original.provider_rows = exact;
    original.event = event(&original.execution, 1, LifecycleEventKind::Admitted);
    original.notification = outbox(&original.event);
    let claim = store.admit_root_driver(original.clone()).unwrap();
    let mut execution = original.execution;
    for (sequence, state, kind) in [
        (2, ExecutionState::Queued, LifecycleEventKind::Queued),
        (3, ExecutionState::Running, LifecycleEventKind::Started),
    ] {
        let next = event(&execution, sequence, kind);
        execution = store
            .commit_execution_step(
                &execution.execution_id,
                execution.version,
                Some(state),
                next.clone(),
                None,
                None,
                None,
                outbox(&next),
            )
            .unwrap();
    }
    (claim, execution)
}
fn intent(store: &dyn PlatformStore, execution: &ExecutionRecord) -> EffectRecord {
    let mut actual = orphan_intent();
    actual.execution_id = execution.execution_id.clone();
    actual.run_unit_id = execution.run_unit_id.clone();
    actual.key = IdempotencyKey::new(
        format!("{}-effect", execution.execution_id),
        InvocationLimits::default(),
    )
    .unwrap();
    actual.intent.owner = execution.execution_id.clone();
    actual.intent.attempt = execution.attempt;
    store.record_intent(actual.clone()).unwrap();
    store.effect(&actual.key).unwrap().unwrap()
}
pub(super) fn put(
    namespace: &str,
    key: &str,
    version: u64,
    expected: Option<u64>,
) -> ProviderStateMutation {
    ProviderStateMutation::Put(ProviderStateWrite {
        record: ProviderStateRecord {
            namespace: namespace.into(),
            key: key.into(),
            version,
            payload: b"exact-owned-bytes".to_vec(),
        },
        expected_version: expected,
    })
}
fn original(store: &dyn PlatformStore) -> (RootProviderPublication, EffectRecord) {
    let (claim, execution) = start(
        store,
        "writer-a",
        vec!["native-root-a".into()],
        vec![
            ProviderStateIdentity {
                namespace: "exact-shared".into(),
                key: "a".into(),
            },
            ProviderStateIdentity {
                namespace: "exact-shared".into(),
                key: "moved".into(),
            },
        ],
    );
    let effect = intent(store, &execution);
    (
        RootProviderPublication {
            occurrence: RootProviderRowAdmission {
                claim,
                execution,
                effect_key: effect.key.clone(),
                effect_sequence: effect.sequence,
                request_digest: effect.request_digest,
                identity: ProviderStateIdentity {
                    namespace: "native-root-a".into(),
                    key: "first".into(),
                },
                observed_tick: 10,
            },
            intent: effect.intent.clone(),
            mutations: vec![put("native-root-a", "first", 1, None)],
        },
        effect,
    )
}
fn observed(
    store: &dyn PlatformStore,
    request: &RootProviderPublication,
) -> (
    Option<ExecutionRecord>,
    Option<EffectRecord>,
    Vec<LifecycleEvent>,
    Vec<OutboxRecord>,
    Vec<AuditSubjectRecord>,
    Vec<ProviderStateRecord>,
    u64,
    u64,
) {
    let mut rows = Vec::new();
    for prefix in ["durable-", "native-root-", "exact-", "legacy-"] {
        rows.extend(store.list_provider_state_prefix(prefix, 256).unwrap());
    }
    (
        store
            .get_execution(&request.occurrence.execution.execution_id)
            .unwrap(),
        store.effect(&request.occurrence.effect_key).unwrap(),
        store
            .events(&request.occurrence.execution.execution_id, 1, 32)
            .unwrap(),
        store.pending_notifications(64).unwrap(),
        store
            .audit_subject_records(&request.occurrence.execution.execution_id, 64)
            .unwrap(),
        rows,
        store.provider_state_retention_epoch().unwrap(),
        store.advance_logical_clock(1).unwrap(),
    )
}
fn refuses(
    store: &dyn PlatformStore,
    original: &RootProviderPublication,
    bad: RootProviderPublication,
) {
    let before = observed(store, original);
    assert!(store.mutate_root_provider_states(bad).is_err());
    assert_eq!(observed(store, original), before);
}
fn audit(
    request: &RootProviderPublication,
    effect: &EffectRecord,
    mutations: Vec<ProviderStateMutation>,
) -> AuditedProviderPublication {
    AuditedProviderPublication {
        intent: effect.clone(),
        observed_tick: request.occurrence.observed_tick,
        audit: AuditRecord {
            execution_id: effect.execution_id.clone(),
            run_unit_id: effect.run_unit_id.clone(),
            principal: request.occurrence.execution.principal.clone(),
            attempt: effect.intent.attempt,
            effect_sequence: effect.sequence,
            invocation_key: effect.intent.audit_invocation_key.clone().unwrap(),
            capability: effect.intent.capability.clone().unwrap(),
            resource: effect.intent.audit_resource.unwrap(),
            decision: AuditDecision::Success,
            observed_tick: request.occurrence.observed_tick,
        },
        mutations,
    }
}

fn current_writer(store: &dyn PlatformStore) {
    let (mut request, effect) = original(store);
    let original_core = observed(store, &request);
    store.mutate_root_provider_states(request.clone()).unwrap();
    request.mutations = vec![
        put("native-root-a", "first", 2, Some(1)),
        put("exact-shared", "a", 1, None),
    ];
    request.occurrence.observed_tick = 11;
    store.mutate_root_provider_states(request.clone()).unwrap();
    request.mutations = vec![
        ProviderStateMutation::Move {
            record: ProviderStateRecord {
                namespace: "exact-shared".into(),
                key: "moved".into(),
                version: 2,
                payload: b"exact-owned-bytes".to_vec(),
            },
            old_key: "a".into(),
            expected_version: 1,
        },
        ProviderStateMutation::Delete {
            namespace: "native-root-a".into(),
            key: "first".into(),
            expected_version: 2,
        },
    ];
    store.mutate_root_provider_states(request.clone()).unwrap();
    assert!(
        store
            .get_provider_state("exact-shared", "a")
            .unwrap()
            .is_none()
    );
    assert_eq!(
        store
            .get_provider_state("exact-shared", "moved")
            .unwrap()
            .unwrap()
            .payload,
        b"exact-owned-bytes"
    );
    assert!(
        store
            .get_provider_state("native-root-a", "first")
            .unwrap()
            .is_none()
    );
    assert_eq!(store.effect(&effect.key).unwrap(), Some(effect));
    let after = observed(store, &request);
    assert_eq!(
        (after.0, after.2, after.3, after.4),
        (
            original_core.0,
            original_core.2,
            original_core.3,
            original_core.4
        )
    );
    assert_eq!(after.7, 11);
}

fn substituted_originals(store: &dyn PlatformStore) {
    let (request, _) = original(store);
    for index in 0..28 {
        let mut bad = request.clone();
        let bounds = InvocationLimits::default();
        match index {
            0 => bad.occurrence.execution.version += 1,
            1 => bad.occurrence.execution.attempt += 1,
            2 => bad.occurrence.execution.principal = PrincipalId::new("FOREIGN", bounds).unwrap(),
            3 => {
                bad.occurrence.execution.run_unit_id =
                    RunUnitId::new("foreign-run", bounds).unwrap()
            }
            4 => {
                bad.occurrence.execution.execution_id =
                    ExecutionId::new("foreign-actor", bounds).unwrap()
            }
            5 => bad.occurrence.execution.owner_lease = Some("foreign-lease".into()),
            6 => bad.occurrence.execution.lease_expiry_tick = Some(10),
            7 => bad.occurrence.execution.state = ExecutionState::Suspended,
            8 => bad.occurrence.execution.terminal_tick = Some(10),
            9 => {
                bad.occurrence.execution.selector =
                    Selector::new("program:FOREIGN", bounds).unwrap()
            }
            10 => {
                bad.occurrence.execution.artifact =
                    ArtifactRef::new(format!("sha256:{}", "b".repeat(64)), bounds).unwrap()
            }
            11 => bad.occurrence.effect_key = IdempotencyKey::new("foreign-key", bounds).unwrap(),
            12 => bad.occurrence.effect_sequence += 1,
            13 => bad.occurrence.request_digest[0] ^= 1,
            14 => bad.intent.owner = ExecutionId::new("foreign-owner", bounds).unwrap(),
            15 => bad.intent.attempt += 1,
            16 => bad.intent.epoch += 1,
            17 => bad.intent.created_tick -= 1,
            18 => bad.intent.recovery_after_tick += 1,
            19 => bad.intent.capability = None,
            20 => bad.intent.audit_resource = None,
            21 => bad.intent.audit_invocation_key = None,
            22 => bad.occurrence.observed_tick = 0,
            23 => bad.occurrence.observed_tick = 100,
            24 => bad.occurrence.identity.key = "unregistered-anchor".into(),
            25 => {
                bad.intent.recovery_lease = Some(EffectRecoveryLease {
                    owner: "worker".into(),
                    attempt: 1,
                    epoch: 2,
                    expires_tick: 20,
                })
            }
            26 => bad.occurrence.observed_tick = i64::MAX as u64 + 1,
            27 => bad.intent.epoch = 0,
            _ => unreachable!(),
        }
        // The namespace anchor case above remains registered; make it exact-row only.
        if index == 24 {
            bad.occurrence.identity.namespace = "exact-shared".into();
        }
        refuses(store, &request, bad);
    }
    // A valid, Running legacy actor still cannot write another indexed root.
    let bounds = InvocationLimits::default();
    let mut unowned = request.occurrence.execution.clone();
    unowned.execution_id = ExecutionId::new("unowned", bounds).unwrap();
    unowned.run_unit_id = RunUnitId::new("unowned-run", bounds).unwrap();
    unowned.version = 1;
    unowned.state = ExecutionState::Admitted;
    store.create_execution(unowned.clone()).unwrap();
    for state in [ExecutionState::Queued, ExecutionState::Running] {
        unowned = store
            .transition_execution(&unowned.execution_id, unowned.version, state, 10)
            .unwrap();
    }
    let unowned_effect = intent(store, &unowned);
    let before = observed(store, &request);
    let mut foreign_audit = audit(
        &request,
        &unowned_effect,
        vec![put("native-root-a", "foreign-actor", 1, None)],
    );
    assert!(
        store
            .publish_provider_states_audited(foreign_audit.clone())
            .is_err()
    );
    assert_eq!(observed(store, &request), before);
    foreign_audit.mutations = vec![put("legacy-unowned", "unowned", 1, None)];
    store
        .publish_provider_states_audited(foreign_audit)
        .unwrap();
}

fn foreign_scopes(store: &dyn PlatformStore) {
    let (request, effect) = original(store);
    let (foreign, execution) = start(
        store,
        "writer-b",
        vec!["native-root-b".into()],
        vec![ProviderStateIdentity {
            namespace: "exact-shared".into(),
            key: "b".into(),
        }],
    );
    intent(store, &execution);
    let mut bad = request.clone();
    bad.occurrence.claim = foreign;
    refuses(store, &request, bad);
    for mutations in [
        vec![put("native-root-b", "foreign", 1, None)],
        vec![
            put("native-root-a", "first", 1, None),
            put("native-root-b", "mixed", 1, None),
        ],
        vec![put("exact-shared", "absent", 1, None)],
        vec![ProviderStateMutation::Move {
            record: ProviderStateRecord {
                namespace: "exact-shared".into(),
                key: "b".into(),
                version: 2,
                payload: vec![9],
            },
            old_key: "a".into(),
            expected_version: 1,
        }],
        vec![ProviderStateMutation::Move {
            record: ProviderStateRecord {
                namespace: "exact-shared".into(),
                key: "a".into(),
                version: 2,
                payload: vec![9],
            },
            old_key: "b".into(),
            expected_version: 1,
        }],
    ] {
        let mut bad = request.clone();
        bad.mutations = mutations.clone();
        refuses(store, &request, bad);
        let before = observed(store, &request);
        let unindexed = matches!(mutations.as_slice(), [ProviderStateMutation::Put(w)] if w.record.namespace == "exact-shared" && w.record.key == "absent");
        if unindexed {
            // This row is neither a namespace nor exact-row enrolled identity.
            // The explicit route refuses it; the legacy audited route stays unchanged.
            store
                .publish_provider_states_audited(audit(&request, &effect, mutations))
                .unwrap();
            assert_eq!(store.effect(&effect.key).unwrap(), Some(effect.clone()));
            continue;
        }
        assert!(
            store
                .publish_provider_states_audited(audit(&request, &effect, mutations))
                .is_err()
        );
        assert_eq!(observed(store, &request), before);
    }
    // Unowned rows and audit-only/denial retain their existing semantics.
    store
        .publish_provider_states_audited(audit(
            &request,
            &effect,
            vec![put("legacy-unowned", "ok", 1, None)],
        ))
        .unwrap();
    let mut denial = audit(&request, &effect, Vec::new());
    denial.audit.decision = AuditDecision::Deny;
    store
        .publish_provider_states_audited(denial.clone())
        .unwrap();
    denial.mutations = vec![put("native-root-a", "denied", 1, None)];
    assert_eq!(
        store.publish_provider_states_audited(denial),
        Err(StoreError::InvalidTransition)
    );
}

fn late_cas_and_bounds(store: &dyn PlatformStore) {
    let (request, _) = original(store);
    let mut bad = request.clone();
    bad.mutations.push(ProviderStateMutation::Delete {
        namespace: "native-root-a".into(),
        key: "absent".into(),
        expected_version: 1,
    });
    refuses(store, &request, bad);
    let mut bad = request.clone();
    bad.mutations.clear();
    refuses(store, &request, bad);
    let mut bad = request.clone();
    bad.mutations = vec![request.mutations[0].clone(); MAX_ROOT_OPERATIONS + 1];
    refuses(store, &request, bad);
    let mut bad = request.clone();
    bad.mutations = vec![
        ProviderStateMutation::Move {
            record: ProviderStateRecord {
                namespace: "native-root-a".into(),
                key: "new".into(),
                version: 2,
                payload: vec![9]
            },
            old_key: "old".into(),
            expected_version: 1
        };
        MAX_ROOT_OPERATIONS / 2 + 1
    ];
    refuses(store, &request, bad);
    assert_eq!(
        request.validate_bounds(usize::MAX, MAX_ROOT_PAYLOAD_BYTES),
        Err(StoreError::CapacityExceeded)
    );
    let mut bad = request.clone();
    bad.mutations = vec![put("durable-root-scope-v1", "forged", 1, None)];
    refuses(store, &request, bad);
    store.advance_logical_clock(11).unwrap();
    refuses(store, &request, request.clone());
}

fn physical_staleness(store: &dyn PlatformStore) {
    let (request, effect) = original(store);
    store
        .transition_execution(
            &request.occurrence.execution.execution_id,
            request.occurrence.execution.version,
            ExecutionState::Suspended,
            10,
        )
        .unwrap();
    refuses(store, &request, request.clone());
    // A completed real retained intent is never a mutation permit.
    let mut completed = effect.clone();
    completed.state = EffectState::Completed;
    completed.result_digest = Some([9; 32]);
    completed.resolved_tick = Some(10);
    store.record_result(&effect.key, completed).unwrap();
    refuses(store, &request, request.clone());
}

fn non_open(store: &dyn PlatformStore, uncertain: bool) {
    let (request, effect) = original(store);
    if uncertain {
        store
            .fence_root_driver(&request.occurrence.claim, &request.occurrence.execution, 10)
            .unwrap();
    } else {
        let mut completed = effect.clone();
        completed.state = EffectState::Completed;
        completed.result_digest = Some([9; 32]);
        completed.resolved_tick = Some(10);
        store.record_result(&effect.key, completed).unwrap();
        store
            .close_root_driver(&request.occurrence.claim, &request.occurrence.execution, 10)
            .unwrap();
    }
    refuses(store, &request, request.clone());
    if uncertain {
        let notification = store.pending_notifications(32).unwrap()[0].clone();
        store
            .mark_notification_delivered(&notification.notification_id, notification.version, 11)
            .unwrap();
    }
}

macro_rules! both {
    ($memory:ident,$sqlite:ident,$proof:expr) => {
        #[test]
        fn $memory() {
            $proof(&MemoryStore::new(StoreLimits::default()));
        }
        #[test]
        fn $sqlite() {
            let fixture = OwnedSqlite::new();
            $proof(fixture.store());
        }
    };
}
both!(memory_current, sqlite_current, current_writer);
both!(
    memory_substitutions,
    sqlite_substitutions,
    substituted_originals
);
both!(
    memory_foreign_mixed_move,
    sqlite_foreign_mixed_move,
    foreign_scopes
);
both!(
    memory_cas_bounds_clock,
    sqlite_cas_bounds_clock,
    late_cas_and_bounds
);
both!(
    memory_physical_stale,
    sqlite_physical_stale,
    physical_staleness
);
both!(memory_closing, sqlite_closing, |s: &dyn PlatformStore| {
    non_open(s, false)
});
both!(
    memory_uncertain_outbox,
    sqlite_uncertain_outbox,
    |s: &dyn PlatformStore| non_open(s, true)
);

fn terminal_refuses(store: &dyn PlatformStore, normal: bool) {
    let (request, effect) = original(store);
    let mut completed = effect.clone();
    completed.state = EffectState::Completed;
    completed.result_digest = Some([9; 32]);
    completed.resolved_tick = Some(10);
    store.record_result(&effect.key, completed).unwrap();
    let mut terminal = publication(
        store,
        &request.occurrence.claim,
        &request.occurrence.execution,
        normal,
    );
    terminal.dependencies = vec![TerminalRowDependency::Absent {
        namespace: "native-root-a".into(),
        key: "terminal".into(),
    }];
    terminal.mutations = vec![put("native-root-a", "terminal", 1, None)];
    store.commit_root_terminal_step(terminal).unwrap();
    refuses(store, &request, request.clone());
    let before = observed(store, &request);
    assert!(
        store
            .publish_provider_states_audited(audit(&request, &effect, request.mutations.clone()))
            .is_err()
    );
    assert_eq!(observed(store, &request), before);
}
both!(
    memory_terminal_normal,
    sqlite_terminal_normal,
    |s: &dyn PlatformStore| terminal_refuses(s, true)
);
both!(
    memory_terminal_abnormal,
    sqlite_terminal_abnormal,
    |s: &dyn PlatformStore| terminal_refuses(s, false)
);

fn recovered_refuses(store: &dyn PlatformStore) {
    let (request, effect) = original(store);
    store
        .claim_stale_intent(&effect.key, effect.intent.epoch, "recovery", 100, 1, 20)
        .unwrap();
    refuses(store, &request, request.clone());
}
both!(memory_recovered, sqlite_recovered, recovered_refuses);

fn overlap_refuses(store: &dyn PlatformStore) {
    let (request, effect) = original(store);
    start(store, "overlap", vec!["exact-shared".into()], Vec::new());
    let mut bad = request.clone();
    bad.mutations = vec![put("exact-shared", "a", 1, None)];
    refuses(store, &request, bad.clone());
    let before = observed(store, &request);
    assert!(
        store
            .publish_provider_states_audited(audit(&request, &effect, bad.mutations))
            .is_err()
    );
    assert_eq!(observed(store, &request), before);
}
both!(
    memory_overlapping_index,
    sqlite_overlapping_index,
    overlap_refuses
);

fn rooted_audit_batch(store: &dyn PlatformStore) {
    let (request, effect) = original(store);
    store
        .publish_provider_states_audited(audit(&request, &effect, request.mutations.clone()))
        .unwrap();
    assert_eq!(store.effect(&effect.key).unwrap(), Some(effect.clone()));
    assert_eq!(
        store
            .audit_records(&effect.execution_id, 1, 32)
            .unwrap()
            .len(),
        1
    );
    let before = observed(store, &request);
    let mutations = vec![
        ProviderStateMutation::Move {
            record: ProviderStateRecord {
                namespace: "native-root-a".into(),
                key: "destination".into(),
                version: 2,
                payload: vec![9]
            },
            old_key: "first".into(),
            expected_version: 1,
        };
        MAX_ROOT_OPERATIONS / 2 + 1
    ];
    assert_eq!(
        store.publish_provider_states_audited(audit(&request, &effect, mutations)),
        Err(StoreError::CapacityExceeded)
    );
    assert_eq!(observed(store, &request), before);
    let mut late = audit(
        &request,
        &effect,
        vec![
            put("native-root-a", "first", 2, Some(1)),
            ProviderStateMutation::Delete {
                namespace: "native-root-a".into(),
                key: "absent".into(),
                expected_version: 1,
            },
        ],
    );
    assert!(store.publish_provider_states_audited(late.clone()).is_err());
    assert_eq!(observed(store, &request), before);
    late.observed_tick = 100;
    late.audit.observed_tick = 100;
    assert!(store.publish_provider_states_audited(late).is_err());
    assert_eq!(observed(store, &request), before);
}
both!(
    memory_rooted_audit_batch,
    sqlite_rooted_audit_batch,
    rooted_audit_batch
);

fn legacy_domain_refuses(store: &dyn PlatformStore) {
    let (claim, execution) = start(
        store,
        "legacy-intent",
        vec!["native-root-a".into()],
        Vec::new(),
    );
    let mut effect = orphan_intent();
    effect.execution_id = execution.execution_id.clone();
    effect.run_unit_id = execution.run_unit_id.clone();
    effect.intent.owner = execution.execution_id.clone();
    effect.digest_format = EffectDigestFormat::LegacyDebug;
    store.record_intent(effect.clone()).unwrap();
    let request = RootProviderPublication {
        occurrence: RootProviderRowAdmission {
            claim,
            execution,
            effect_key: effect.key.clone(),
            effect_sequence: effect.sequence,
            request_digest: effect.request_digest,
            identity: ProviderStateIdentity {
                namespace: "native-root-a".into(),
                key: "first".into(),
            },
            observed_tick: 10,
        },
        intent: effect.intent.clone(),
        mutations: vec![put("native-root-a", "first", 1, None)],
    };
    refuses(store, &request, request.clone());
}
both!(
    memory_legacy_domain,
    sqlite_legacy_domain,
    legacy_domain_refuses
);

#[test]
fn memory_quota_rolls_back_batch() {
    let store = MemoryStore::new(StoreLimits {
        max_total_blob_bytes: 16_384,
        ..StoreLimits::default()
    });
    let (request, _) = original(&store);
    let mut bad = request.clone();
    if let ProviderStateMutation::Put(w) = &mut bad.mutations[0] {
        w.record.payload = vec![9; 16_384];
    }
    let before = observed(&store, &request);
    assert_eq!(
        store.mutate_root_provider_states(bad),
        Err(StoreError::CapacityExceeded)
    );
    assert_eq!(observed(&store, &request), before);
}

#[test]
fn sqlite_late_clock_failure_restores_whole_transaction() {
    use sqlx::Connection;
    let fixture = OwnedSqlite::new();
    let store = fixture.store();
    let (mut request, _) = original(store);
    request.occurrence.observed_tick = 11;
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let mut connection = sqlx::SqliteConnection::connect(&format!("sqlite://{}?mode=rw", fixture.path.display())).await.unwrap();
        sqlx::query("CREATE TRIGGER abort_writer_clock BEFORE UPDATE OF clock_tick ON retention_lock WHEN NEW.clock_tick=11 BEGIN SELECT RAISE(ABORT,'attributed clock fault'); END").execute(&mut connection).await.unwrap();
        connection.close().await.unwrap();
    });
    let before = observed(store, &request);
    assert!(matches!(
        store.mutate_root_provider_states(request.clone()),
        Err(StoreError::Infrastructure(_))
    ));
    assert_eq!(observed(store, &request), before);
}

#[test]
fn sqlite_row_quota_rolls_back_all_owned_rows() {
    let fixture = OwnedSqlite::with_rows(15);
    let store = fixture.store();
    let (request, _) = original(store);
    let mut bad = request.clone();
    bad.mutations
        .extend((0..16).map(|n| put("native-root-a", &format!("quota-{n}"), 1, None)));
    let before = observed(store, &request);
    assert_eq!(
        store.mutate_root_provider_states(bad),
        Err(StoreError::CapacityExceeded)
    );
    assert_eq!(observed(store, &request), before);
}
