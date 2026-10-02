use super::*;

#[test]
fn local_uow_savepoint_backout_retains_ownership_and_rejects_an_old_uow_image() {
    let stores: Vec<Arc<dyn ProviderStateStore>> = vec![
        Arc::new(MemoryStore::new(StoreLimits::default())),
        Arc::new(SqliteStateStore::open("sqlite::memory:", 64 * 1024 * 1024, 262_144).unwrap()),
    ];
    for store in stores {
        let service = install(store.clone());
        let effects = MemoryStore::new(StoreLimits::default());
        let owner = "SAVEOWNER";
        let other = "SAVEOTHER";
        let invocation = generic::tests::invocation(owner);
        load(&service, &effects, owner);
        for run in [owner, other] {
            service
                .execute(
                    &generic::tests::invocation(run),
                    &generic::tests::request(run, ImsOperation::Schedule, 1, &[], b""),
                )
                .unwrap();
        }
        let resource = service
            .recovery_database_resource(&invocation, "APP", PACKAGE, "GENDB")
            .unwrap();
        let recovery = RecoverySession::load(&*store, owner, RecoveryLimits::default()).unwrap();
        let begin = recovery
            .begin_uow(&*store, "save-begin", vec![resource])
            .unwrap();
        let key = intent(&effects, owner, "save-begin-effect", [51; 32]);
        service
            .publish_database_recovery_transition(
                &invocation,
                "APP",
                PACKAGE,
                "GENDB",
                begin,
                &effects,
                &key,
                [51; 32],
            )
            .unwrap();
        service
            .execute(
                &invocation,
                &generic::tests::request(owner, ImsOperation::Insert, 2, &["ROOT"], b"C3Z"),
            )
            .unwrap();
        let recovery = RecoverySession::load(&*store, owner, RecoveryLimits::default()).unwrap();
        let sets = recovery
            .sets(
                &*store,
                "save-sets",
                BackoutPointKind::Sets,
                Some(*b"SAVE"),
                vec![],
                false,
            )
            .unwrap();
        let key = intent(&effects, owner, "save-sets-effect", [52; 32]);
        service
            .publish_database_recovery_transition(
                &invocation,
                "APP",
                PACKAGE,
                "GENDB",
                sets,
                &effects,
                &key,
                [52; 32],
            )
            .unwrap();
        service
            .execute(
                &invocation,
                &generic::tests::request(owner, ImsOperation::Insert, 3, &["ROOT"], b"E5Z"),
            )
            .unwrap();
        let recovery = RecoverySession::load(&*store, owner, RecoveryLimits::default()).unwrap();
        let rols = recovery.rols(&*store, "save-rols", *b"SAVE").unwrap();
        let key = intent(&effects, owner, "save-rols-effect", [53; 32]);
        service
            .publish_database_recovery_transition(
                &invocation,
                "APP",
                PACKAGE,
                "GENDB",
                rols.transition,
                &effects,
                &key,
                [53; 32],
            )
            .unwrap();
        assert_eq!(read_key(&service, owner, b"E5").status, "GE");
        assert_eq!(read_key(&service, owner, b"C3").segments[0].data, b"C3Z");
        let insert_other =
            generic::tests::request(other, ImsOperation::Insert, 2, &["ROOT"], b"D4Z");
        assert_eq!(
            service.execute(&generic::tests::invocation(other), &insert_other),
            Err(HostProblem::IdempotencyConflict)
        );
        service
            .execute(
                &invocation,
                &generic::tests::request(owner, ImsOperation::Commit, 4, &[], b""),
            )
            .unwrap();
        service
            .execute(&generic::tests::invocation(other), &insert_other)
            .unwrap();
        service
            .execute(
                &generic::tests::invocation(other),
                &generic::tests::request(other, ImsOperation::Commit, 3, &[], b""),
            )
            .unwrap();
        service
            .execute(
                &invocation,
                &generic::tests::request(owner, ImsOperation::Insert, 5, &["ROOT"], b"F6Z"),
            )
            .unwrap();
        let recovery = RecoverySession::load(&*store, owner, RecoveryLimits::default()).unwrap();
        let rols = recovery.rols(&*store, "old-uow-rols", *b"SAVE").unwrap();
        let key = intent(&effects, owner, "old-uow-rols-effect", [54; 32]);
        let before = store
            .get_provider_state(GENERIC_DATABASE_NAMESPACE, "GENDB")
            .unwrap();
        assert_eq!(
            service.publish_database_recovery_transition(
                &invocation,
                "APP",
                PACKAGE,
                "GENDB",
                rols.transition,
                &effects,
                &key,
                [54; 32]
            ),
            Err(RecoveryProblem::UnknownOutcome)
        );
        assert_eq!(
            store
                .get_provider_state(GENERIC_DATABASE_NAMESPACE, "GENDB")
                .unwrap(),
            before
        );
        service
            .execute(
                &invocation,
                &generic::tests::request(owner, ImsOperation::Rollback, 6, &[], b""),
            )
            .unwrap();
        assert_eq!(read_key(&service, owner, b"D4").segments[0].data, b"D4Z");
    }
}

#[test]
fn local_uow_utility_publication_refreshes_another_services_owner() {
    let stores: Vec<Arc<dyn ProviderStateStore>> = vec![
        Arc::new(MemoryStore::new(StoreLimits::default())),
        Arc::new(SqliteStateStore::open("sqlite::memory:", 64 * 1024 * 1024, 262_144).unwrap()),
    ];
    for store in stores {
        let writer = install(store.clone());
        let stale = ImsService::open(store.clone(), ImsLimits::default()).unwrap();
        let run = "UTILITYOWNER";
        let invocation = generic::tests::invocation(run);
        writer
            .execute(
                &invocation,
                &generic::tests::request(run, ImsOperation::Schedule, 1, &[], b""),
            )
            .unwrap();
        writer
            .execute(
                &invocation,
                &generic::tests::request(run, ImsOperation::Insert, 2, &["ROOT"], b"C3Z"),
            )
            .unwrap();
        let image = UtilityEngine::extract(
            &writer,
            &generic::tests::invocation("UTILITYOTHER"),
            "APP",
            PACKAGE,
            "GENDB",
            RecoveryLimits::default(),
        )
        .unwrap();
        UtilityEngine::stage_reorganization(
            &writer,
            &generic::tests::invocation("UTILITYOTHER"),
            "APP",
            PACKAGE,
            "isolation-load",
            UtilityPlan {
                kind: UtilityKind::Reorganize,
                database: "GENDB".into(),
                expected_input_digest: image.digest(),
                expected_records: image.records.len(),
            },
            RecoveryLimits::default(),
        )
        .unwrap();
        let effects = MemoryStore::new(StoreLimits::default());
        let key = intent(&effects, "UTILITYOTHER", "isolation-load-effect", [41; 32]);
        let before = store
            .get_provider_state(GENERIC_DATABASE_NAMESPACE, "GENDB")
            .unwrap();
        assert_eq!(
            UtilityEngine::publish(
                &stale,
                &generic::tests::invocation("UTILITYOTHER"),
                "APP",
                PACKAGE,
                &effects,
                &key,
                [41; 32],
                "isolation-load",
                "GENDB",
                RecoveryLimits::default()
            ),
            Err(RecoveryProblem::Conflict)
        );
        assert_eq!(
            store
                .get_provider_state(GENERIC_DATABASE_NAMESPACE, "GENDB")
                .unwrap(),
            before
        );
        assert!(
            store
                .get_provider_state(PUBLICATION_NAMESPACE, "isolation-load")
                .unwrap()
                .is_none()
        );
    }
}

#[test]
fn local_uow_recovery_backout_cannot_replace_another_runs_image() {
    let store = Arc::new(MemoryStore::new(StoreLimits::default()));
    let service = install(store.clone());
    let owner = "BACKOUTOWNER";
    let other = "BACKOUTOTHER";
    let invocation = generic::tests::invocation(other);
    let resource = service
        .recovery_database_resource(&invocation, "APP", PACKAGE, "GENDB")
        .unwrap();
    let recovery = RecoverySession::load(&*store, other, RecoveryLimits::default()).unwrap();
    let proposal = recovery
        .begin_uow(&*store, "isolation-begin", vec![resource])
        .unwrap();
    let key = intent(&*store, other, "isolation-begin-effect", [42; 32]);
    service
        .publish_database_recovery_transition(
            &invocation,
            "APP",
            PACKAGE,
            "GENDB",
            proposal,
            &*store,
            &key,
            [42; 32],
        )
        .unwrap();
    let recovery = RecoverySession::load(&*store, other, RecoveryLimits::default()).unwrap();
    let sets = recovery
        .sets(
            &*store,
            "isolation-sets",
            BackoutPointKind::Sets,
            Some(*b"SAVE"),
            vec![],
            false,
        )
        .unwrap();
    let key = intent(&*store, other, "isolation-sets-effect", [43; 32]);
    service
        .publish_database_recovery_transition(
            &invocation,
            "APP",
            PACKAGE,
            "GENDB",
            sets,
            &*store,
            &key,
            [43; 32],
        )
        .unwrap();
    let owner_invocation = generic::tests::invocation(owner);
    service
        .execute(
            &owner_invocation,
            &generic::tests::request(owner, ImsOperation::Schedule, 1, &[], b""),
        )
        .unwrap();
    service
        .execute(
            &owner_invocation,
            &generic::tests::request(owner, ImsOperation::Insert, 2, &["ROOT"], b"A1X"),
        )
        .unwrap();
    let recovery = RecoverySession::load(&*store, other, RecoveryLimits::default()).unwrap();
    let rols = recovery.rols(&*store, "isolation-rols", *b"SAVE").unwrap();
    let key = intent(&*store, other, "isolation-rols-effect", [44; 32]);
    let before = store
        .get_provider_state(GENERIC_DATABASE_NAMESPACE, "GENDB")
        .unwrap();
    assert_eq!(
        service.publish_database_recovery_transition(
            &invocation,
            "APP",
            PACKAGE,
            "GENDB",
            rols.transition,
            &*store,
            &key,
            [44; 32]
        ),
        Err(RecoveryProblem::Conflict)
    );
    assert_eq!(
        store
            .get_provider_state(GENERIC_DATABASE_NAMESPACE, "GENDB")
            .unwrap(),
        before
    );
}
