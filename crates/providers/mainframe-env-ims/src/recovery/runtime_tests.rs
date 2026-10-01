use super::*;
use mainframe_env_execution_api::{
    CapabilityId, ExecutionId, IdempotencyKey, InvocationLimits, RunUnitId,
};
use mainframe_env_store::{MemoryStore, SqliteStateStore, StoreLimits};
use mainframe_env_store_api::{
    EffectDigestFormat, EffectIntentMetadata, EffectRecord, EffectState, IdempotencyStore,
    ProviderStateMutation, ProviderStateRecord, ProviderStateStore, ProviderStateWrite, StoreError,
};

fn symbolic() -> CheckpointRequest {
    CheckpointRequest {
        id: "CHK00001".into(),
        kind: CheckpointKind::Symbolic,
        context: RecoveryContext::Batch,
        prior_xrst: true,
        user_areas: vec![],
        positions: vec![],
    }
}

#[test]
fn checkpoint_log_and_restart_are_persisted_without_duplicate_replay() {
    let store = MemoryStore::new(StoreLimits::default());
    let limits = RecoveryLimits::default();
    let session = RecoverySession::load(&store, "RUN001", limits).unwrap();
    session
        .xrst(
            "normal-xrst",
            1,
            RestartSelection::Normal,
            RecoveryContext::Batch,
            |_| unreachable!(),
        )
        .unwrap()
        .transition
        .publish(&store, Vec::new())
        .unwrap();
    let session = RecoverySession::load(&store, "RUN001", limits).unwrap();
    let checkpoint = session.checkpoint("effect-1", symbolic(), [3; 32]).unwrap();
    store
        .mutate_provider_states_atomic(checkpoint.mutations())
        .unwrap();
    let reopened = RecoverySession::load(&store, "RUN001", limits).unwrap();
    assert_eq!(
        reopened
            .checkpoint("effect-1", symbolic(), [3; 32])
            .unwrap()
            .replayed(),
        true
    );
    let log = reopened
        .log(
            "effect-2",
            LogRequest {
                code: 0xa0,
                data: b"hello".to_vec(),
            },
        )
        .unwrap();
    store
        .mutate_provider_states_atomic(log.mutations())
        .unwrap();
    let reopened = RecoverySession::load(&store, "RUN001", limits).unwrap();
    assert_eq!(reopened.log_count(), 1);
    assert_eq!(
        reopened
            .restart(
                RestartSelection::Id("CHK00001".into()),
                RecoveryContext::Batch
            )
            .unwrap()
            .checkpoint_id
            .as_deref(),
        Some("CHK00001")
    );
}

#[test]
fn sets_and_rols_restore_database_rows_through_shared_atomic_store() {
    let store = MemoryStore::new(StoreLimits::default());
    let limits = RecoveryLimits::default();
    store
        .put_provider_state(
            ProviderStateRecord {
                namespace: "ims-db-test".into(),
                key: "record-1".into(),
                version: 1,
                payload: b"before".to_vec(),
            },
            None,
        )
        .unwrap();
    let resource = TrackedResource {
        namespace: "ims-db-test".into(),
        key: "record-1".into(),
        kind: TrackedResourceKind::Database,
    };
    let nonexpress = TrackedResource {
        namespace: "ims-message-test".into(),
        key: "normal".into(),
        kind: TrackedResourceKind::NonExpressMessage,
    };
    let express = TrackedResource {
        namespace: "ims-message-test".into(),
        key: "express".into(),
        kind: TrackedResourceKind::ExpressMessage,
    };
    let session = RecoverySession::load(&store, "RUN002", limits).unwrap();
    let begin = session
        .begin_uow(
            &store,
            "effect-begin",
            vec![resource.clone(), nonexpress, express],
        )
        .unwrap();
    store
        .mutate_provider_states_atomic(begin.mutations())
        .unwrap();
    let session = RecoverySession::load(&store, "RUN002", limits).unwrap();
    let set = session
        .sets(
            &store,
            "effect-sets",
            BackoutPointKind::Sets,
            Some(*b"SAVE"),
            b"marker".to_vec(),
            false,
        )
        .unwrap();
    store
        .mutate_provider_states_atomic(set.mutations())
        .unwrap();
    store
        .put_provider_state(
            ProviderStateRecord {
                namespace: "ims-db-test".into(),
                key: "record-1".into(),
                version: 2,
                payload: b"after".to_vec(),
            },
            Some(1),
        )
        .unwrap();
    for key in ["normal", "express"] {
        store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: "ims-message-test".into(),
                    key: key.into(),
                    version: 1,
                    payload: b"message".to_vec(),
                },
                None,
            )
            .unwrap();
    }
    let session = RecoverySession::load(&store, "RUN002", limits).unwrap();
    let rollback = session.rols(&store, "effect-rols", *b"SAVE").unwrap();
    assert_eq!(rollback.returned_data, b"marker");
    store
        .mutate_provider_states_atomic(rollback.transition.mutations())
        .unwrap();
    assert_eq!(
        store
            .get_provider_state("ims-db-test", "record-1")
            .unwrap()
            .unwrap()
            .payload,
        b"before"
    );
    assert!(
        store
            .get_provider_state("ims-message-test", "normal")
            .unwrap()
            .is_none()
    );
    assert!(
        store
            .get_provider_state("ims-message-test", "express")
            .unwrap()
            .is_some()
    );
    let reopened = RecoverySession::load(&store, "RUN002", limits).unwrap();
    let replay = reopened.rols(&store, "effect-rols", *b"SAVE").unwrap();
    assert!(replay.transition.replayed());
    assert_eq!(replay.returned_data, b"marker");
}

#[test]
fn concurrent_proposals_conflict_atomically_and_corrupt_rows_fail_closed() {
    let store = MemoryStore::new(StoreLimits::default());
    let limits = RecoveryLimits::default();
    let session = RecoverySession::load(&store, "RUN003", limits).unwrap();
    let first = session
        .log(
            "effect-one",
            LogRequest {
                code: 0xa0,
                data: b"one".to_vec(),
            },
        )
        .unwrap();
    let stale = session
        .log(
            "effect-two",
            LogRequest {
                code: 0xa1,
                data: b"two".to_vec(),
            },
        )
        .unwrap();
    first.publish(&store, Vec::new()).unwrap();
    assert_eq!(
        stale.publish(&store, Vec::new()),
        Err(RecoveryProblem::Conflict)
    );
    let reopened = RecoverySession::load(&store, "RUN003", limits).unwrap();
    assert_eq!(reopened.log_count(), 1);
    assert_eq!(
        reopened
            .log(
                "effect-one",
                LogRequest {
                    code: 0xa0,
                    data: b"other".to_vec()
                }
            )
            .unwrap_err(),
        RecoveryProblem::Conflict
    );
    let row = store
        .get_provider_state("ims-recovery-v1-session", "RUN003")
        .unwrap()
        .unwrap();
    store
        .put_provider_state(
            ProviderStateRecord {
                version: row.version + 1,
                payload: b"corrupt".to_vec(),
                ..row.clone()
            },
            Some(row.version),
        )
        .unwrap();
    assert!(matches!(
        RecoverySession::load(&store, "RUN003", limits),
        Err(RecoveryProblem::CorruptImage)
    ));
    assert_eq!(
        store
            .get_provider_state("ims-recovery-v1-session", "RUN003")
            .unwrap()
            .unwrap()
            .payload,
        b"corrupt"
    );
}

#[test]
fn failed_atomic_backout_leaves_all_rows_unchanged() {
    let store = MemoryStore::new(StoreLimits::default());
    let limits = RecoveryLimits::default();
    store
        .put_provider_state(
            ProviderStateRecord {
                namespace: "ims-db-test".into(),
                key: "one".into(),
                version: 1,
                payload: b"before".to_vec(),
            },
            None,
        )
        .unwrap();
    let resource = TrackedResource {
        namespace: "ims-db-test".into(),
        key: "one".into(),
        kind: TrackedResourceKind::Database,
    };
    let session = RecoverySession::load(&store, "RUN004", limits).unwrap();
    session
        .begin_uow(&store, "begin", vec![resource])
        .unwrap()
        .publish(&store, Vec::new())
        .unwrap();
    let session = RecoverySession::load(&store, "RUN004", limits).unwrap();
    session
        .sets(
            &store,
            "set",
            BackoutPointKind::Sets,
            Some(*b"SAVE"),
            Vec::new(),
            false,
        )
        .unwrap()
        .publish(&store, Vec::new())
        .unwrap();
    store
        .put_provider_state(
            ProviderStateRecord {
                namespace: "ims-db-test".into(),
                key: "one".into(),
                version: 2,
                payload: b"after".to_vec(),
            },
            Some(1),
        )
        .unwrap();
    let session = RecoverySession::load(&store, "RUN004", limits).unwrap();
    let plan = session.rols(&store, "rols", *b"SAVE").unwrap();
    let mut mutations = plan.transition.mutations();
    mutations.push(ProviderStateMutation::Put(ProviderStateWrite {
        record: ProviderStateRecord {
            namespace: "ims-db-test".into(),
            key: "one".into(),
            version: 99,
            payload: b"bad".to_vec(),
        },
        expected_version: Some(1),
    }));
    assert_eq!(
        store.mutate_provider_states_atomic(mutations),
        Err(StoreError::Conflict)
    );
    assert_eq!(
        store
            .get_provider_state("ims-db-test", "one")
            .unwrap()
            .unwrap()
            .payload,
        b"after"
    );
    assert_eq!(
        RecoverySession::load(&store, "RUN004", limits)
            .unwrap()
            .backout_point_count(),
        1
    );
}

#[test]
fn sqlite_reopen_preserves_log_and_checkpoint_images() {
    let directory = std::env::temp_dir().join(format!(
        "mainframe-env-ims-recovery-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("recovery.sqlite");
    let url = format!("sqlite://{}?mode=rwc", path.display());
    let limits = RecoveryLimits::default();
    {
        let store = SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap();
        let session = RecoverySession::load(&store, "SQLRUN", limits).unwrap();
        session
            .xrst(
                "normal-xrst",
                1,
                RestartSelection::Normal,
                RecoveryContext::Batch,
                |_| unreachable!(),
            )
            .unwrap()
            .transition
            .publish(&store, Vec::new())
            .unwrap();
        let session = RecoverySession::load(&store, "SQLRUN", limits).unwrap();
        session
            .checkpoint("checkpoint", symbolic(), [7; 32])
            .unwrap()
            .publish(&store, Vec::new())
            .unwrap();
        let session = RecoverySession::load(&store, "SQLRUN", limits).unwrap();
        session
            .log(
                "log",
                LogRequest {
                    code: 0xa0,
                    data: b"durable".to_vec(),
                },
            )
            .unwrap()
            .publish(&store, Vec::new())
            .unwrap();
    }
    {
        let store = SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap();
        let session = RecoverySession::load(&store, "SQLRUN", limits).unwrap();
        assert_eq!(session.log_count(), 1);
        assert_eq!(session.checkpoint_count(), 1);
        assert_eq!(
            session
                .restart(
                    RestartSelection::Id("CHK00001".into()),
                    RecoveryContext::Batch
                )
                .unwrap()
                .checkpoint_id
                .as_deref(),
            Some("CHK00001")
        );
    }
    std::fs::remove_file(&path).unwrap();
    std::fs::remove_dir(&directory).unwrap();
}

#[test]
fn xrst_attempts_positions_once_per_generation_and_preserves_reported_status() {
    let store = MemoryStore::new(StoreLimits::default());
    let limits = RecoveryLimits::default();
    let session = RecoverySession::load(&store, "XRSTRUN", limits).unwrap();
    let normal = session
        .xrst(
            "normal-xrst",
            1,
            RestartSelection::Normal,
            RecoveryContext::Batch,
            |_| unreachable!(),
        )
        .unwrap();
    normal.transition.publish(&store, Vec::new()).unwrap();
    let session = RecoverySession::load(&store, "XRSTRUN", limits).unwrap();
    let mut checkpoint = symbolic();
    checkpoint.positions.push(SavedPcbPosition {
        pcb: "DBPCB".into(),
        database: "ACCOUNTS".into(),
        segment_key: b"K001".to_vec(),
    });
    session
        .checkpoint("checkpoint", checkpoint, [4; 32])
        .unwrap()
        .publish(&store, Vec::new())
        .unwrap();
    let session = RecoverySession::load(&store, "XRSTRUN", limits).unwrap();
    let restarted = session
        .xrst(
            "restart-xrst",
            2,
            RestartSelection::Id("CHK00001".into()),
            RecoveryContext::Batch,
            |position| {
                assert_eq!(position.segment_key, b"K001");
                Ok(PositionAttempt {
                    status: RepositionStatus::Reestablished,
                    mutation: Some(ProviderStateMutation::Put(ProviderStateWrite {
                        record: ProviderStateRecord {
                            namespace: "ims-pcb-test".into(),
                            key: position.pcb.clone(),
                            version: 1,
                            payload: position.segment_key.clone(),
                        },
                        expected_version: None,
                    })),
                })
            },
        )
        .unwrap();
    assert_eq!(
        restarted.result.positions[0].status,
        RepositionStatus::Reestablished
    );
    restarted.transition.publish(&store, Vec::new()).unwrap();
    assert_eq!(
        store
            .get_provider_state("ims-pcb-test", "DBPCB")
            .unwrap()
            .unwrap()
            .payload,
        b"K001"
    );
    let reopened = RecoverySession::load(&store, "XRSTRUN", limits).unwrap();
    let replay = reopened
        .xrst(
            "restart-xrst",
            2,
            RestartSelection::Id("CHK00001".into()),
            RecoveryContext::Batch,
            |_| unreachable!(),
        )
        .unwrap();
    assert!(replay.transition.replayed());
    assert_eq!(
        replay.result.positions[0].status,
        RepositionStatus::Reestablished
    );
    assert!(matches!(
        reopened.xrst(
            "duplicate-xrst",
            2,
            RestartSelection::Normal,
            RecoveryContext::Batch,
            |_| unreachable!()
        ),
        Err(RecoveryProblem::InvalidRequest)
    ));
    assert!(matches!(
        reopened.xrst(
            "missing-xrst",
            3,
            RestartSelection::Id("MISSING".into()),
            RecoveryContext::Batch,
            |_| unreachable!()
        ),
        Err(RecoveryProblem::NotFound)
    ));
    assert_eq!(
        RecoverySession::load(&store, "XRSTRUN", limits)
            .unwrap()
            .checkpoint_count(),
        1
    );
}

#[test]
fn canonical_effect_intent_is_required_before_recovery_publication() {
    let store = MemoryStore::new(StoreLimits::default());
    let limits = RecoveryLimits::default();
    let key = IdempotencyKey::new("recovery-log", InvocationLimits::default()).unwrap();
    let request = LogRequest {
        code: 0xa0,
        data: b"audit".to_vec(),
    };
    let session = RecoverySession::load(&store, "EFFECTRUN", limits).unwrap();
    let proposal = session.log("recovery-log", request.clone()).unwrap();
    assert_eq!(
        proposal.publish_with_intent(&store, &store, &key, [5; 32], Vec::new()),
        Err(RecoveryProblem::InvalidRequest)
    );
    assert_eq!(
        RecoverySession::load(&store, "EFFECTRUN", limits)
            .unwrap()
            .log_count(),
        0
    );

    let ids = InvocationLimits::default();
    let execution_id = ExecutionId::new("recovery-exec", ids).unwrap();
    let intent = EffectRecord {
        execution_id: execution_id.clone(),
        run_unit_id: RunUnitId::new("recovery-run", ids).unwrap(),
        sequence: 1,
        key: key.clone(),
        digest_format: EffectDigestFormat::CanonicalHostV1,
        request_digest: [5; 32],
        intent: EffectIntentMetadata {
            owner: execution_id,
            attempt: 1,
            capability: Some(CapabilityId::new("ims.recovery.log", ids).unwrap()),
            audit_resource: None,
            audit_invocation_key: None,
            created_tick: 5,
            recovery_after_tick: 10,
            epoch: 1,
            recovery_lease: None,
        },
        state: EffectState::Intent,
        result_digest: None,
        resolved_tick: None,
    };
    store.record_intent(intent.clone()).unwrap();
    let proposal = session.log("recovery-log", request.clone()).unwrap();
    assert_eq!(
        proposal.publish_with_intent(&store, &store, &key, [6; 32], Vec::new()),
        Err(RecoveryProblem::Conflict)
    );
    let proposal = session.log("recovery-log", request).unwrap();
    proposal
        .publish_with_intent(&store, &store, &key, [5; 32], Vec::new())
        .unwrap();
    assert_eq!(
        RecoverySession::load(&store, "EFFECTRUN", limits)
            .unwrap()
            .log_count(),
        1
    );
    store
        .record_result(
            &key,
            EffectRecord {
                state: EffectState::UnknownOutcome,
                result_digest: Some([7; 32]),
                ..intent
            },
        )
        .unwrap();
    let session = RecoverySession::load(&store, "EFFECTRUN", limits).unwrap();
    let replay = session
        .log(
            "recovery-log",
            LogRequest {
                code: 0xa0,
                data: b"audit".to_vec(),
            },
        )
        .unwrap();
    assert_eq!(
        replay.publish_with_intent(&store, &store, &key, [5; 32], Vec::new()),
        Err(RecoveryProblem::UnknownOutcome)
    );
}

#[test]
fn setu_warning_token_reuse_and_full_rollback_are_explicit() {
    let store = MemoryStore::new(StoreLimits::default());
    let limits = RecoveryLimits::default();
    let session = RecoverySession::load(&store, "POINTS", limits).unwrap();
    assert_eq!(
        session.restart(RestartSelection::Last, RecoveryContext::MessageDrivenBatch),
        Err(RecoveryProblem::NotFound)
    );
    session
        .begin_uow(&store, "begin", Vec::new())
        .unwrap()
        .publish(&store, Vec::new())
        .unwrap();
    let session = RecoverySession::load(&store, "POINTS", limits).unwrap();
    assert_eq!(
        session
            .sets(
                &store,
                "sets-bad",
                BackoutPointKind::Sets,
                Some(*b"FAIL"),
                Vec::new(),
                true
            )
            .unwrap_err(),
        RecoveryProblem::Unsupported
    );
    let warning = session
        .sets(
            &store,
            "setu-warning",
            BackoutPointKind::Setu,
            Some(*b"WARN"),
            Vec::new(),
            true,
        )
        .unwrap();
    assert!(warning.warning());
    assert!(warning.mutations().is_empty());
    session
        .sets(
            &store,
            "sets-one",
            BackoutPointKind::Sets,
            Some(*b"ONE1"),
            Vec::new(),
            false,
        )
        .unwrap()
        .publish(&store, Vec::new())
        .unwrap();
    let session = RecoverySession::load(&store, "POINTS", limits).unwrap();
    session
        .sets(
            &store,
            "sets-two",
            BackoutPointKind::Sets,
            Some(*b"TWO2"),
            Vec::new(),
            false,
        )
        .unwrap()
        .publish(&store, Vec::new())
        .unwrap();
    let session = RecoverySession::load(&store, "POINTS", limits).unwrap();
    session
        .sets(
            &store,
            "reuse-one",
            BackoutPointKind::Sets,
            Some(*b"ONE1"),
            b"later".to_vec(),
            false,
        )
        .unwrap()
        .publish(&store, Vec::new())
        .unwrap();
    let session = RecoverySession::load(&store, "POINTS", limits).unwrap();
    assert_eq!(session.backout_point_count(), 1);
    assert!(matches!(
        session.rols(&store, "missing", *b"TWO2"),
        Err(RecoveryProblem::NotFound)
    ));
    let roll = session.roll(&store, "roll").unwrap();
    assert!(roll.terminated);
    roll.transition.publish(&store, Vec::new()).unwrap();
    let reopened = RecoverySession::load(&store, "POINTS", limits).unwrap();
    assert_eq!(reopened.backout_point_count(), 0);
    assert!(matches!(
        reopened.rolb(&store, "rolb"),
        Err(RecoveryProblem::InvalidRequest)
    ));
}
