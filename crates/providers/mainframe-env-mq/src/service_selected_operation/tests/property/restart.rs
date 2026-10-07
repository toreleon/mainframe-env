use super::*;

#[test]
fn owned_sqlite_close_reopen_retains_exact_property_results_but_never_revives_handles_or_values() {
    let db = super::super::restart::Database::new();
    let (inv, provider, frame, e, reply, rows, audits, old_h) = {
        let f = Fixture::from_store(db.open());
        let c = f.connect();
        let h = hmsg(f.call(2, create(c)));
        f.call(
            3,
            set(c, h, "a", MqPropertyType::String, b"stored\0 ".to_vec()),
        );
        let e = f.effect(4, inquire(c, h, "a", 1, 3));
        f.seed(&e);
        let reply = f.execute(&e).unwrap();
        (
            f.inv.clone(),
            f.provider.clone(),
            f.frame,
            e,
            reply,
            f.rows(),
            f.store.audit_records(&f.inv.execution_id, 0, 128).unwrap(),
            h,
        )
    };
    let store = db.open();
    assert_eq!(store.list_provider_state_prefix("mq-", 4096).unwrap(), rows);
    assert_eq!(
        store.audit_records(&inv.execution_id, 0, 128).unwrap(),
        audits
    );
    let service = MqService::open_selected_mqi(
        store.clone(),
        MqLimits::default(),
        3,
        5,
        Arc::new(Policy::default()),
        Arc::new(Clock(std::sync::atomic::AtomicU64::new(20))),
    )
    .unwrap();
    let process = service.mint_selected_process(&inv).unwrap();
    let (fresh, owner) = service.bind_selected_root(process, &inv).unwrap();
    {
        let guard = service.lock_selected().unwrap();
        let rich_state::StoredAuthority::Rich(state) = &*guard else {
            panic!()
        };
        assert_eq!(
            state.receipts["effect-4"]
                .replay(HostLimits::default(), MqMqiLimits::default())
                .unwrap(),
            reply
        );
        let historic = state.receipts["effect-2"]
            .replay(HostLimits::default(), MqMqiLimits::default())
            .unwrap();
        let MqMqiOutput::MessageHandle(token) = output(historic) else {
            panic!()
        };
        assert!(token.is_historical());
        assert_ne!(token, old_h);
        assert!(state.runtime.as_ref().unwrap().connections.is_empty());
    }
    for lease in [frame, fresh] {
        assert!(
            service
                .execute_selected_mqi(
                    lease,
                    &inv,
                    e.mq_mqi_occurrence(HostLimits::default()).unwrap().unwrap(),
                    &provider,
                    HostLimits::default()
                )
                .is_err()
        );
    }
    assert_eq!(store.list_provider_state_prefix("mq-", 4096).unwrap(), rows);
    assert_eq!(
        store
            .effect(e.idempotency_key.as_ref().unwrap())
            .unwrap()
            .unwrap()
            .state,
        EffectState::Intent
    );
    assert_ne!(
        owner,
        mainframe_env_host_api::MqHandleOwner {
            syncpoint_epoch: 0,
            ..owner
        }
    );
}
