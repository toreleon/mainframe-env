use super::*;

#[test]
fn memory_owned_sqlite_equal_physical_rows_core_and_receipt_never_copy_live_full_get_authority() {
    for sqlite in [false, true] {
        let f = FullFixture::new(sqlite, 2, true, vec![message(2, true)]);
        let c = f.connect();
        let o = f.open(c);
        let effect = f.effect(
            3,
            request(c, o, 2, true, 32, MqMqiUnitOfWork::NoSyncpoint, false),
        );
        f.seed(&effect);
        f.execute(&effect).unwrap();
        let original = f.rows();
        let db = sqlite.then(Database::new);
        let copy: Arc<dyn PlatformStore> = db.as_ref().map_or_else(
            || {
                Arc::new(MemoryStore::new(mainframe_env_store::StoreLimits::default()))
                    as Arc<dyn PlatformStore>
            },
            Database::open,
        );
        assert!(!Arc::ptr_eq(&copy, &f.store));
        // Copy exact PHYSICAL record bytes AND versions through real bounded CAS
        // calls. This is an adversarial input fixture, not service restoration or
        // a migration/admission permit. No executable token is serialized.
        for record in &original {
            assert!(record.version <= 16);
            for version in 1..=record.version {
                let mut next = record.clone();
                next.version = version;
                copy.put_provider_state(next, (version > 1).then_some(version - 1))
                    .unwrap();
            }
        }
        assert_eq!(
            copy.list_provider_state_prefix("mq-", 4096).unwrap(),
            original
        );
        let running = f.store.get_execution(&f.inv.execution_id).unwrap().unwrap();
        let mut admitted = running.clone();
        admitted.state = ExecutionState::Admitted;
        admitted.version = 1;
        copy.create_execution(admitted).unwrap();
        copy.transition_execution(&f.inv.execution_id, 1, ExecutionState::Queued, 6)
            .unwrap();
        copy.transition_execution(&f.inv.execution_id, 2, ExecutionState::Running, 7)
            .unwrap();
        assert_eq!(
            copy.get_execution(&f.inv.execution_id).unwrap(),
            Some(running)
        );
        let intent = f
            .store
            .effect(effect.idempotency_key.as_ref().unwrap())
            .unwrap()
            .unwrap();
        copy.record_intent(intent.clone()).unwrap();
        assert_eq!(copy.effect(&intent.key).unwrap(), Some(intent));
        let service = MqService::open_selected_mqi(
            copy.clone(),
            MqLimits::default(),
            3,
            5,
            f.saf.clone(),
            f.clock.clone(),
        )
        .unwrap();
        let process = service.mint_selected_process(&f.inv).unwrap();
        let (fresh_frame, _) = service.bind_selected_root(process, &f.inv).unwrap();
        let calls = f.saf.observations.lock().unwrap().len();
        for frame in [f.frame, fresh_frame] {
            assert!(
                service
                    .execute_selected_mqi(
                        frame,
                        &f.inv,
                        effect
                            .mq_mqi_occurrence(HostLimits::default())
                            .unwrap()
                            .unwrap(),
                        &f.provider,
                        HostLimits::default()
                    )
                    .is_err()
            );
        }
        assert_eq!(
            copy.list_provider_state_prefix("mq-", 4096).unwrap(),
            original
        );
        assert!(
            copy.audit_records(&f.inv.execution_id, 0, 256)
                .unwrap()
                .is_empty()
        );
        assert_eq!(f.saf.observations.lock().unwrap().len(), calls);
        assert_eq!(f.rows(), original);
        drop(service);
        drop(copy);
        drop(db);
    }
}
