use super::*;

#[test]
fn postpersist_unknown_retains_one_producer_receipt_without_resampling_or_redispatch() {
    for sqlite in [false, true] {
        let f = ProducerFixture::new(sqlite, 2, true);
        let c = f.connect();
        let e = f.effect(
            3,
            put(
                c,
                None,
                message(2, true, i32::MAX),
                false,
                MqMqiUnitOfWork::NoSyncpoint,
            ),
        );
        f.seed(&e);
        f.service
            .unknown_after_persist
            .store(true, Ordering::SeqCst);
        assert_eq!(f.execute(&e), Err(HostProblem::UnknownOutcome));
        let rows = f.rows();
        assert_eq!(
            rows.iter()
                .filter(|r| r.namespace == receipt::NAMESPACE && r.key == "effect-3")
                .count(),
            1
        );
        assert_eq!(f.execute(&e), Err(HostProblem::UnknownOutcome));
        assert_eq!(f.rows(), rows);
        assert_eq!(f.ports.gmt_calls.load(Ordering::SeqCst), 1);
        let g = f.service.lock_selected().unwrap();
        let rich_state::StoredAuthority::Rich(s) = &*g else {
            panic!()
        };
        assert_eq!(
            s.delivery
                .depth(&crate::MqObjectName::new("Q").unwrap())
                .unwrap(),
            1
        );
        assert!(s.runtime.as_ref().unwrap().fenced);
    }
}
#[test]
fn owned_sqlite_reopen_preserves_produced_bytes_receipt_and_refuses_historical_frame() {
    let mut f = ProducerFixture::new(true, 2, true);
    let c = f.connect();
    let e = f.effect(
        3,
        put(
            c,
            None,
            message(2, true, -99),
            false,
            MqMqiUnitOfWork::NoSyncpoint,
        ),
    );
    f.seed(&e);
    f.execute(&e).unwrap();
    let rows = f.rows();
    let db = f.db.take().unwrap();
    let (frame, inv, provider, saf, clock) = (
        f.frame,
        f.inv.clone(),
        f.provider.clone(),
        f.saf.clone(),
        f.clock.clone(),
    );
    drop(f); // Close original physical SQLite/store/service before reopen.
    let second = db.open(256);
    assert_eq!(
        second.list_provider_state_prefix("mq-", 4096).unwrap(),
        rows
    );
    let reopened =
        MqService::open_selected_mqi(second.clone(), MqLimits::default(), 3, 5, saf, clock)
            .unwrap();
    assert!(
        reopened
            .execute_selected_mqi(
                frame,
                &inv,
                e.mq_mqi_occurrence(HostLimits::default()).unwrap().unwrap(),
                &provider,
                HostLimits::default()
            )
            .is_err()
    );
    assert_eq!(
        second.list_provider_state_prefix("mq-", 4096).unwrap(),
        rows
    );
}
