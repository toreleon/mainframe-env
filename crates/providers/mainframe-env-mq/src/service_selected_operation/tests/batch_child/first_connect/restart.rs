use super::*;

#[test]
fn memory_sqlite_completed_child_first_connect_replay_is_fenced_by_new_incarnation() {
    for sqlite in [false, true] {
        let f = Fixture::new(sqlite);
        let child = Child::new(&f);
        let e = child.effect(&f, 10, request());
        child.seed(&f, &e);
        let reply = child.execute(&f, &e).unwrap();
        let key = e.idempotency_key.as_ref().unwrap();
        let mut completed = f.store.effect(key).unwrap().unwrap();
        completed.state = EffectState::Completed;
        completed.result_digest = Some(canonical_result_digest(&reply.outcome).unwrap());
        completed.resolved_tick = Some(20);
        f.store.record_result(key, completed.clone()).unwrap();
        assert_eq!(child.execute(&f, &e).unwrap(), reply);
        let cold = MqService::open_selected_mqi(
            f.store.clone(),
            MqLimits::default(),
            3,
            5,
            f.saf.clone(),
            f.clock.clone(),
        )
        .unwrap();
        let process = cold.mint_selected_process(&f.inv).unwrap();
        let (frame, owner) = cold.bind_selected_root(process, &f.inv).unwrap();
        let mut fresh = f.effect(20, request());
        let HostRequest::MqMqi(h) = &mut fresh.request else {
            panic!()
        };
        h.envelope.context.owner = owner;
        f.seed(&fresh);
        cold.execute_selected_mqi(
            frame,
            &f.inv,
            fresh
                .mq_mqi_occurrence(HostLimits::default())
                .unwrap()
                .unwrap(),
            &f.provider,
            HostLimits::default(),
        )
        .unwrap();
        let rows = f.rows();
        let calls = f.saf.calls.load(Ordering::SeqCst);
        let audits = f
            .store
            .audit_records(&child.inv.execution_id, 0, 128)
            .unwrap();
        assert_eq!(child.execute(&f, &e), Err(HostProblem::UnknownOutcome));
        assert_eq!(f.rows(), rows);
        assert_eq!(f.saf.calls.load(Ordering::SeqCst), calls);
        assert_eq!(
            f.store
                .audit_records(&child.inv.execution_id, 0, 128)
                .unwrap(),
            audits
        );
        assert_eq!(f.store.effect(key).unwrap(), Some(completed));
    }
}

#[test]
fn sqlite_child_first_connect_return_reopen_preserves_pending_origin_without_live_tokens() {
    let db = super::super::super::restart::Database::new();
    let (parent, child_inv, old_frame, old_conn, rows, audit, core, unit) = {
        let f = Fixture::from_store(db.open());
        let child = Child::new(&f);
        let c = connect(&f, &child);
        let o = open(&f, &child, c);
        let unit = f.unit();
        let e = child.effect(
            &f,
            12,
            MqMqiRequest::Put {
                connection: c,
                object: o,
                put: put(MqMqiUnitOfWork::Local { unit }),
            },
        );
        child.seed(&f, &e);
        child.execute(&f, &e).unwrap();
        f.service
            .return_selected_batch_child(child.binding.frame(), &child.inv)
            .unwrap();
        assert_pending(&f, unit);
        (
            f.inv.clone(),
            child.inv.clone(),
            child.binding.frame(),
            c,
            f.rows(),
            f.store
                .audit_records(&child.inv.execution_id, 0, 128)
                .unwrap(),
            f.store
                .effect(e.idempotency_key.as_ref().unwrap())
                .unwrap()
                .unwrap(),
            unit,
        )
    };
    let store = db.open();
    assert_eq!(store.list_provider_state_prefix("mq-", 4096).unwrap(), rows);
    assert_eq!(
        store
            .audit_records(&child_inv.execution_id, 0, 128)
            .unwrap(),
        audit
    );
    assert_eq!(store.effect(&core.key).unwrap(), Some(core));
    let clock = Arc::new(Clock(std::sync::atomic::AtomicU64::new(20)));
    let saf = Arc::new(Saf::default());
    let service =
        MqService::open_selected_mqi(store.clone(), MqLimits::default(), 3, 5, saf.clone(), clock)
            .unwrap();
    assert!(service.selected_batch_owner(old_frame, &child_inv).is_err());
    let process = service.mint_selected_process(&parent).unwrap();
    let (frame, _) = service.bind_selected_root(process, &parent).unwrap();
    assert!(
        service
            .selected_local_unit(frame, &parent, old_conn)
            .is_err()
    );
    assert_eq!(saf.calls.load(Ordering::SeqCst), 0);
    let guard = service.lock_selected().unwrap();
    let rich_state::StoredAuthority::Rich(s) = &*guard else {
        panic!()
    };
    assert_eq!(s.delivery.unit_outcome(unit), MqDeliveryOutcome::Pending);
    assert_eq!(
        s.ownership.units[&unit].state,
        ownership::UnitState::Pending
    );
    assert_eq!(s.ownership.units[&unit].connection_key, "child-effect-10");
    assert_eq!(s.runtime.as_ref().unwrap().control.registry_epoch, 2);
    assert!(s.runtime.as_ref().unwrap().connections.is_empty());
    let historical = output(
        s.receipts["child-effect-10"]
            .replay(HostLimits::default(), MqMqiLimits::default())
            .unwrap(),
    );
    let MqMqiOutput::Connected(historical) = historical else {
        panic!()
    };
    assert!(historical.is_historical());
    assert_ne!(historical, old_conn);
    assert_eq!(store.list_provider_state_prefix("mq-", 4096).unwrap(), rows);
}
