use super::*;

#[test]
fn rfh2_owned_sqlite_reopen_preserves_exact_full_result_without_reviving_profile_handles_or_properties()
 {
    let db = super::super::super::restart::Database::new();
    let (inv, provider, old_frame, e, reply, rows, audits) = {
        let mut f = Fixture::from_store(db.open());
        configure(&mut f);
        let c = f.connect();
        let h = hmsg(f.call(2, create(c)));
        f.call(
            3,
            set(
                c,
                h,
                "invoice.id",
                MqPropertyType::ByteString,
                vec![0, 255, 128],
            ),
        );
        let e = f.effect(4, export(c, h, 3, 4096));
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
        Arc::new(Clock(AtomicU64::new(20))),
    )
    .unwrap();
    let process = service.mint_selected_process(&inv).unwrap();
    let (fresh, _) = service.bind_selected_root(process, &inv).unwrap();
    {
        let mut guard = service.lock_selected().unwrap();
        let rich_state::StoredAuthority::Rich(state) = &mut *guard else {
            panic!()
        };
        assert_eq!(
            state.receipts["effect-4"]
                .replay(HostLimits::default(), MqMqiLimits::default())
                .unwrap(),
            reply
        );
        let runtime = state.runtime.as_mut().unwrap();
        assert!(runtime.connections.is_empty());
        assert_eq!(runtime.handles.handles_mut().active_handles(), 0);
    }
    for frame in [old_frame, fresh] {
        assert!(
            service
                .execute_selected_mqi(
                    frame,
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
}
