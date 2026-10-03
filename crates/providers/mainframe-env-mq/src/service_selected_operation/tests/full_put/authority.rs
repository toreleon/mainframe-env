use super::*;
#[test]
fn same_store_unique_inactive_source_setup_and_equal_rows_never_authorize_replay() {
    for sqlite in [false, true] {
        let f = ProducerFixture::new(sqlite, 2, true);
        let c = f.connect();
        let e = f.effect(
            3,
            put(
                c,
                None,
                message(2, true, -4),
                false,
                MqMqiUnitOfWork::NoSyncpoint,
            ),
        );
        f.seed(&e);
        f.execute(&e).unwrap();
        let rows = f.rows();
        let db = sqlite.then(Database::new);
        let copy: Arc<dyn PlatformStore> = db.as_ref().map_or_else(
            || Arc::new(MemoryStore::new(Default::default())) as Arc<dyn PlatformStore>,
            |d| d.open(256),
        );
        for r in &rows {
            for version in 1..=r.version {
                let mut next = r.clone();
                next.version = version;
                copy.put_provider_state(next, (version > 1).then_some(version - 1))
                    .unwrap();
            }
        }
        assert_eq!(copy.list_provider_state_prefix("mq-", 4096).unwrap(), rows);
        let running = f.store.get_execution(&f.inv.execution_id).unwrap().unwrap();
        let mut initial = running.clone();
        initial.state = ExecutionState::Admitted;
        initial.version = 1;
        copy.create_execution(initial).unwrap();
        copy.transition_execution(&f.inv.execution_id, 1, ExecutionState::Queued, 6)
            .unwrap();
        copy.transition_execution(&f.inv.execution_id, 2, ExecutionState::Running, 7)
            .unwrap();
        copy.record_intent(
            f.store
                .effect(e.idempotency_key.as_ref().unwrap())
                .unwrap()
                .unwrap(),
        )
        .unwrap();
        let mut service = MqService::open_selected_mqi(
            copy.clone(),
            MqLimits::default(),
            3,
            5,
            f.saf.clone(),
            f.clock.clone(),
        )
        .unwrap();
        let ports = Arc::new(Ports::default());
        assert_eq!(
            MqService::configure_producer_sources(&mut service, &f.store, ports.clone()),
            Err(HostProblem::Unauthorized)
        );
        let shared = service.clone();
        assert_eq!(
            MqService::configure_producer_sources(&mut service, &copy, ports.clone()),
            Err(HostProblem::Unsupported)
        );
        drop(shared);
        MqService::configure_producer_sources(&mut service, &copy, ports.clone()).unwrap();
        assert_eq!(
            MqService::configure_producer_sources(&mut service, &copy, ports.clone()),
            Err(HostProblem::Unauthorized)
        );
        let process = service.mint_selected_process(&f.inv).unwrap();
        let (fresh, _) = service.bind_selected_root(process, &f.inv).unwrap();
        for frame in [f.frame, fresh] {
            assert!(
                service
                    .execute_selected_mqi(
                        frame,
                        &f.inv,
                        e.mq_mqi_occurrence(HostLimits::default()).unwrap().unwrap(),
                        &f.provider,
                        HostLimits::default()
                    )
                    .is_err()
            );
        }
        assert_eq!(ports.context_calls.load(Ordering::SeqCst), 0);
        assert_eq!(copy.list_provider_state_prefix("mq-", 4096).unwrap(), rows);
        assert_eq!(f.rows(), rows);
        drop(service);
        drop(copy);
        drop(db);
    }
}
#[test]
fn producer_gmt_and_batch_observation_domain_no_normalization() {
    for date in [
        (0, 1, 1),
        (10000, 1, 1),
        (2025, 2, 29),
        (1900, 2, 29),
        (2024, 4, 31),
        (2024, 0, 1),
        (2024, 13, 1),
        (2024, 1, 0),
    ] {
        assert!(ProducerGmt::new(date.0, date.1, date.2, 0, 0, 0, 0).is_err());
    }
    for time in [(24, 0, 0, 0), (0, 60, 0, 0), (0, 0, 60, 0), (0, 0, 0, 100)] {
        assert!(ProducerGmt::new(2000, 2, 29, time.0, time.1, time.2, time.3).is_err());
    }
    ProducerGmt::new(2000, 2, 29, 23, 59, 59, 99).unwrap();
    for job in ["", "LONGERTHAN8", "WITH SPACE", "é"] {
        assert!(ProducerBatchContext::new(job.into(), None, None).is_err());
    }
    assert!(ProducerBatchContext::new("JOB".into(), Some("LONGERTHAN0123".into()), None).is_err());
}

#[test]
fn original_full_put_resource_saf_update_deny_errors_and_replay_reauthorization() {
    for sqlite in [false, true] {
        for mode in [0, 1, 2] {
            let f = ProducerFixture::new(sqlite, 2, false);
            let c = f.connect();
            let unit = f.unit();
            let e = f.effect(
                3,
                put(
                    c,
                    None,
                    message(2, false, 0),
                    false,
                    MqMqiUnitOfWork::Local { unit },
                ),
            );
            f.seed(&e);
            let rows = f.rows();
            let audits = f.store.audit_records(&f.inv.execution_id, 0, 256).unwrap();
            f.saf.deny.store(mode == 0, Ordering::SeqCst);
            f.saf.error.store(mode, Ordering::SeqCst);
            let error = match mode {
                0 => HostProblem::Unauthorized,
                1 => HostProblem::ProviderFailure,
                _ => HostProblem::InfrastructureFailure,
            };
            assert_eq!(f.execute(&e), Err(error));
            assert_eq!(f.rows(), rows);
            let after = f.store.audit_records(&f.inv.execution_id, 0, 256).unwrap();
            assert_eq!(after.len(), audits.len() + 1);
            assert_eq!(
                after.last().unwrap().decision,
                match mode {
                    0 => AuditDecision::Deny,
                    1 => AuditDecision::ProviderFailure,
                    _ => AuditDecision::InfrastructureFailure,
                }
            );
            assert_eq!(
                after.last().unwrap().resource,
                canonical_audit_resource_digest(&e.request)
            );
            let obs = f.saf.observations.lock().unwrap().last().unwrap().clone();
            assert_eq!(obs.0, f.inv.principal.id().clone());
            assert_eq!(obs.1.class, EnterpriseResourceClass::MqQueue);
            assert_eq!(obs.1.name.as_str(), "Q");
            assert_eq!(obs.1.intent, AccessIntent::Update);
            assert_eq!(f.ports.gmt_calls.load(Ordering::SeqCst), 0);
            f.saf.deny.store(false, Ordering::SeqCst);
            f.saf.error.store(0, Ordering::SeqCst);
            f.execute(&e).unwrap();
            let rows = f.rows();
            let calls = f.ports.context_calls.load(Ordering::SeqCst);
            f.saf.deny.store(true, Ordering::SeqCst);
            assert_eq!(f.execute(&e), Err(HostProblem::Unauthorized));
            assert_eq!(f.rows(), rows);
            assert_eq!(f.ports.context_calls.load(Ordering::SeqCst), calls);
        }
    }
}

#[test]
fn concurrent_original_full_put_publishes_once_and_replay_never_resamples() {
    for sqlite in [false, true] {
        let f = ProducerFixture::new(sqlite, 2, false);
        let c = f.connect();
        let e = f.effect(
            3,
            put(
                c,
                None,
                message(2, false, -1),
                false,
                MqMqiUnitOfWork::NoSyncpoint,
            ),
        );
        f.seed(&e);
        let barrier = std::sync::Barrier::new(3);
        let replies = std::thread::scope(|scope| {
            let a = scope.spawn(|| {
                barrier.wait();
                f.execute(&e)
            });
            let b = scope.spawn(|| {
                barrier.wait();
                f.execute(&e)
            });
            barrier.wait();
            [a.join().unwrap(), b.join().unwrap()]
        });
        assert!(replies.iter().any(Result::is_ok));
        for reply in &replies {
            assert!(reply.is_ok() || *reply == Err(HostProblem::Unsupported));
        }
        let reply = f.execute(&e).unwrap();
        let rows = f.rows();
        assert_eq!(f.execute(&e).unwrap(), reply);
        assert_eq!(f.ports.gmt_calls.load(Ordering::SeqCst), 1);
        assert_eq!(f.ports.context_calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            rows.iter()
                .filter(|r| r.namespace == receipt::NAMESPACE && r.key == "effect-3")
                .count(),
            1
        );
        let guard = f.service.lock_selected().unwrap();
        let rich_state::StoredAuthority::Rich(s) = &*guard else {
            panic!()
        };
        assert_eq!(
            s.delivery
                .depth(&crate::MqObjectName::new("Q").unwrap())
                .unwrap(),
            1
        );
    }
}
