use super::*;
use mainframe_env_host_api::ImsQClass;

#[test]
fn reservation_utility_reorganization_and_recovery_preserve_reserved_image() {
    let file = std::env::temp_dir().join(format!(
        "ims-q-utility-{}-{}.sqlite",
        std::process::id(),
        NEXT_FILE.fetch_add(1, Ordering::Relaxed)
    ));
    let stores: Vec<Arc<dyn ProviderStateStore>> = vec![
        Arc::new(MemoryStore::new(StoreLimits::default())),
        Arc::new(
            SqliteStateStore::open(
                &format!("sqlite://{}?mode=rwc", file.display()),
                64 * 1024 * 1024,
                262_144,
            )
            .unwrap(),
        ),
    ];
    for store in stores {
        let service = install(store.clone());
        let effects = MemoryStore::new(StoreLimits::default());
        let image = load(&service, &effects, "UTILITYOTHER");
        let run = "QOWNER";
        let owner = generic::tests::invocation(run);
        service
            .execute(
                &owner,
                &generic::tests::request(run, ImsOperation::Schedule, 1, &[], b""),
            )
            .unwrap();
        // This service opened before Q acquisition must refresh the system row.
        let stale = ImsService::open(store.clone(), ImsLimits::default()).unwrap();
        let mut q = generic::tests::request(run, ImsOperation::GetUnique, 2, &["ROOT"], b"");
        q.q_class = ImsQClass::new(b'A');
        service.execute(&owner, &q).unwrap();
        for kind in [UtilityKind::Reorganize, UtilityKind::DatabaseRecovery] {
            let invocation = generic::tests::invocation("UTILITYOTHER");
            let job = format!("q-fence-{kind:?}");
            let plan = UtilityPlan {
                kind,
                database: "GENDB".into(),
                expected_input_digest: image.digest(),
                expected_records: image.records.len(),
            };
            match kind {
                UtilityKind::Reorganize => UtilityEngine::stage_reorganization(
                    &stale,
                    &invocation,
                    "APP",
                    PACKAGE,
                    &job,
                    plan,
                    RecoveryLimits::default(),
                )
                .unwrap(),
                UtilityKind::DatabaseRecovery => UtilityEngine::stage_database_recovery(
                    &stale,
                    &invocation,
                    "APP",
                    PACKAGE,
                    &job,
                    plan,
                    image.clone(),
                    &[],
                    RecoveryLimits::default(),
                )
                .unwrap(),
                _ => unreachable!(),
            };
            let key = intent(&effects, "UTILITYOTHER", &format!("{job}-effect"), [61; 32]);
            let before = [
                GENERIC_DATABASE_NAMESPACE,
                GENERIC_PENDING_NAMESPACE,
                SYSTEM_NAMESPACE,
                SESSION_NAMESPACE,
                PUBLICATION_NAMESPACE,
                REPLAY_NAMESPACE,
            ]
            .into_iter()
            .flat_map(|ns| store.list_provider_state(ns, 4096).unwrap())
            .collect::<Vec<_>>();
            assert_eq!(
                UtilityEngine::publish(
                    &stale,
                    &invocation,
                    "APP",
                    PACKAGE,
                    &effects,
                    &key,
                    [61; 32],
                    &job,
                    "GENDB",
                    RecoveryLimits::default()
                ),
                Err(RecoveryProblem::Conflict)
            );
            let after = [
                GENERIC_DATABASE_NAMESPACE,
                GENERIC_PENDING_NAMESPACE,
                SYSTEM_NAMESPACE,
                SESSION_NAMESPACE,
                PUBLICATION_NAMESPACE,
                REPLAY_NAMESPACE,
            ]
            .into_iter()
            .flat_map(|ns| store.list_provider_state(ns, 4096).unwrap())
            .collect::<Vec<_>>();
            assert_eq!(after, before);
        }
        service
            .execute(
                &owner,
                &generic::tests::request(run, ImsOperation::Commit, 3, &[], b""),
            )
            .unwrap();
        let invocation = generic::tests::invocation("UTILITYOTHER");
        UtilityEngine::stage_reorganization(
            &stale,
            &invocation,
            "APP",
            PACKAGE,
            "q-released",
            UtilityPlan {
                kind: UtilityKind::Reorganize,
                database: "GENDB".into(),
                expected_input_digest: image.digest(),
                expected_records: image.records.len(),
            },
            RecoveryLimits::default(),
        )
        .unwrap();
        let key = intent(&effects, "UTILITYOTHER", "q-released-effect", [62; 32]);
        assert!(
            !UtilityEngine::publish(
                &stale,
                &invocation,
                "APP",
                PACKAGE,
                &effects,
                &key,
                [62; 32],
                "q-released",
                "GENDB",
                RecoveryLimits::default()
            )
            .unwrap()
            .replayed
        );
        assert!(
            UtilityEngine::publish(
                &stale,
                &invocation,
                "APP",
                PACKAGE,
                &effects,
                &key,
                [62; 32],
                "q-released",
                "GENDB",
                RecoveryLimits::default()
            )
            .unwrap()
            .replayed
        );
    }
    std::fs::remove_file(file).unwrap();
}
