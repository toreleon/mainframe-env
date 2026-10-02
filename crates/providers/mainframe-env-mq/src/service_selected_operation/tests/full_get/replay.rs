use super::*;

#[test]
fn memory_sqlite_full_get_and_cold_connect_have_one_cas_winner_in_both_orders() {
    for sqlite in [false, true] {
        for get_first in [false, true] {
            let f = FullFixture::new(sqlite, 2, false, vec![message(2, false)]);
            let c = f.connect();
            let o = f.open(c);
            let get = f.effect(
                3,
                request(c, o, 2, false, 32, MqMqiUnitOfWork::NoSyncpoint, false),
            );
            f.seed(&get);
            // Both services capture the same physical prepublication rows.
            // The cold instance owns a DIFFERENT volatile registry/directory,
            // not a copied token or permission inferred from equal row bytes.
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
            let mut connect = f.effect(
                4,
                MqMqiRequest::Connect(MqMqiConnect {
                    manager: None,
                    sharing: MqHandleSharing::NonShared,
                    options: MqMqiOptions::ContractDefault,
                }),
            );
            let HostRequest::MqMqi(envelope) = &mut connect.request else {
                panic!()
            };
            envelope.envelope.context.owner = owner;
            f.seed(&connect);
            let call_cold = || {
                cold.execute_selected_mqi(
                    frame,
                    &f.inv,
                    connect
                        .mq_mqi_occurrence(HostLimits::default())
                        .unwrap()
                        .unwrap(),
                    &f.provider,
                    HostLimits::default(),
                )
            };
            if get_first {
                f.execute(&get).unwrap();
                let rows = f.rows();
                let audits = f.audits();
                assert!(call_cold().is_err());
                assert_eq!(f.rows(), rows);
                assert_eq!(f.audits(), audits);
                assert_eq!(
                    rich(&*f.store)
                        .delivery
                        .depth(&crate::MqObjectName::new("Q").unwrap())
                        .unwrap(),
                    0
                );
                assert!(
                    !rows
                        .iter()
                        .any(|r| r.namespace == receipt::NAMESPACE && r.key == "effect-4")
                );
            } else {
                call_cold().unwrap();
                let rows = f.rows();
                let audits = f.audits();
                let live = f.live();
                assert!(f.execute(&get).is_err());
                assert_eq!(f.rows(), rows);
                assert_eq!(f.audits(), audits);
                assert_eq!(f.live(), live);
                assert_eq!(
                    rich(&*f.store)
                        .delivery
                        .depth(&crate::MqObjectName::new("Q").unwrap())
                        .unwrap(),
                    1
                );
                assert!(
                    !rows
                        .iter()
                        .any(|r| r.namespace == receipt::NAMESPACE && r.key == "effect-3")
                );
            }
        }
    }
}

#[test]
fn memory_sqlite_full_get_receipt_version_generation_digest_and_unknown_schema_refuse_replay() {
    for sqlite in [false, true] {
        for case in 0..5 {
            let f = FullFixture::new(sqlite, 1, true, vec![message(1, true)]);
            let c = f.connect();
            let o = f.open(c);
            let e = f.effect(
                3,
                request(c, o, 1, true, 32, MqMqiUnitOfWork::NoSyncpoint, false),
            );
            f.seed(&e);
            f.execute(&e).unwrap();
            let mut row = f
                .store
                .get_provider_state(receipt::NAMESPACE, "effect-3")
                .unwrap()
                .unwrap();
            let mut payload: serde_json::Value = serde_json::from_slice(&row.payload).unwrap();
            match case {
                0 => row.version += 1,
                1 => payload["value"]["generation"] = serde_json::json!(4),
                2 => payload["value"]["result_digest"][0] = serde_json::json!(99),
                3 => payload["value"]["schema_version"] = serde_json::json!("unknown@99"),
                _ => payload["value"]["fence"] = serde_json::json!(6),
            }
            if case != 0 {
                row.payload = serde_json::to_vec(&payload).unwrap();
                row.version += 1;
            }
            f.store.put_provider_state(row, Some(1)).unwrap();
            let rows = f.rows();
            let live = f.live();
            let audits = f.audits();
            assert!(f.execute(&e).is_err());
            assert_eq!(f.rows(), rows);
            assert_eq!(f.live(), live);
            assert_eq!(f.audits(), audits);
        }
    }
}

#[test]
fn memory_sqlite_committed_full_get_replay_requires_actual_live_object_and_current_unit() {
    for sqlite in [false, true] {
        for case in 0..3 {
            let f = FullFixture::new(sqlite, 2, false, vec![message(2, false)]);
            let c = f.connect();
            let o = f.open(c);
            let e = f.effect(
                3,
                request(c, o, 2, false, 32, MqMqiUnitOfWork::NoSyncpoint, false),
            );
            f.seed(&e);
            f.execute(&e).unwrap();
            match case {
                0 => {
                    f.call(
                        4,
                        MqMqiRequest::Close(
                            MqObjectCloseRequest::new(
                                c,
                                MqRouteCloseTarget::Object {
                                    handle: o,
                                    lifecycle: MqRouteCloseLifecycle::Predefined,
                                },
                                MqRouteCloseMode::None,
                            )
                            .unwrap(),
                        ),
                    );
                }
                1 => {
                    f.call(4, MqMqiRequest::Disconnect { connection: c });
                }
                _ => {
                    let mut state = f.service.lock_selected().unwrap();
                    let rich_state::StoredAuthority::Rich(s) = &mut *state else {
                        panic!()
                    };
                    s.runtime
                        .as_mut()
                        .unwrap()
                        .handles
                        .handles_mut()
                        .end_processing_unit(f.owner)
                        .unwrap();
                }
            }
            let rows = f.rows();
            let live = f.live();
            let audits = f.audits();
            assert_eq!(f.execute(&e), Err(HostProblem::UnknownOutcome));
            assert_eq!(f.rows(), rows);
            assert_eq!(f.live(), live);
            assert_eq!(f.audits(), audits);
        }
    }
}

#[test]
fn memory_sqlite_full_get_foreign_handles_changed_original_frame_and_receipt_identity_refuse() {
    for sqlite in [false, true] {
        let f = FullFixture::new(sqlite, 1, false, vec![message(1, false)]);
        let c = f.connect();
        let o = f.open(c);
        let other = FullFixture::new(sqlite, 1, false, vec![]);
        let foreign = other.connect();
        let foreign_o = other.open(foreign);
        for (n, (connection, object)) in [
            (foreign, o),
            (c, foreign_o),
            (MqHconn::Default, o),
            (MqHconn::Unassociated, o),
        ]
        .into_iter()
        .enumerate()
        {
            let e = f.effect(
                3 + n as u64,
                request(
                    connection,
                    object,
                    1,
                    false,
                    32,
                    MqMqiUnitOfWork::NoSyncpoint,
                    false,
                ),
            );
            if e.mq_mqi_occurrence(HostLimits::default()).is_err() {
                continue;
            }
            f.seed(&e);
            let rows = f.rows();
            let live = f.live();
            assert!(f.execute(&e).is_err());
            assert_eq!(f.rows(), rows);
            assert_eq!(f.live(), live);
        }
        let e = f.effect(
            10,
            request(c, o, 1, false, 32, MqMqiUnitOfWork::NoSyncpoint, false),
        );
        f.seed(&e);
        f.execute(&e).unwrap();
        let rows = f.rows();
        let live = f.live();
        let mut changed = e.clone();
        let HostRequest::MqMqi(r) = &mut changed.request else {
            panic!()
        };
        let MqMqiRequest::FullGet(g) = &mut r.envelope.request else {
            panic!()
        };
        g.buffer_capacity = 31;
        assert_eq!(f.execute(&changed), Err(HostProblem::IdempotencyConflict));
        assert_eq!(f.rows(), rows);
        assert_eq!(f.live(), live);
        let mut inv = f.inv.clone();
        inv.audit_correlation.push('x');
        assert!(
            f.service
                .execute_selected_mqi(
                    f.frame,
                    &inv,
                    e.mq_mqi_occurrence(HostLimits::default()).unwrap().unwrap(),
                    &f.provider,
                    HostLimits::default()
                )
                .is_err()
        );
        assert_eq!(f.rows(), rows);
    }
}

#[test]
fn memory_sqlite_postpersist_full_get_unknown_retains_exact_receipt_once_and_fences_redispatch() {
    for sqlite in [false, true] {
        let f = FullFixture::new(sqlite, 2, true, vec![message(2, true)]);
        let c = f.connect();
        let o = f.open(c);
        let unit = f.unit();
        let e = f.effect(
            3,
            request(c, o, 2, true, 1, MqMqiUnitOfWork::Local { unit }, true),
        );
        f.seed(&e);
        f.service
            .unknown_after_persist
            .store(true, Ordering::SeqCst);
        assert_eq!(f.execute(&e), Err(HostProblem::UnknownOutcome));
        assert_eq!(f.depth(), 0);
        let rows = f.rows();
        let live = f.live();
        let audits = f.audits();
        let calls = f.saf.observations.lock().unwrap().len();
        assert!(
            rows.iter()
                .any(|r| r.namespace == receipt::NAMESPACE && r.key == "effect-3")
        );
        assert_eq!(f.execute(&e), Err(HostProblem::UnknownOutcome));
        assert_eq!(f.rows(), rows);
        assert_eq!(f.live(), live);
        assert_eq!(f.audits(), audits);
        assert_eq!(f.saf.observations.lock().unwrap().len(), calls);
        let state = rich(&*f.store);
        let stored = state.receipts["effect-3"]
            .replay(HostLimits::default(), Default::default())
            .unwrap();
        let (_, got, length) = full_output(stored, "MQCC_WARNING", "MQRC_TRUNCATED_MSG_ACCEPTED");
        let mut expected = message(2, true);
        expected.body.truncate(1);
        assert_eq!(got, Some(expected));
        assert_eq!(length, Some(5));
        assert_eq!(
            state.delivery.unit_outcome(unit),
            MqDeliveryOutcome::Pending
        );
        assert_eq!(
            f.store
                .effect(e.idempotency_key.as_ref().unwrap())
                .unwrap()
                .unwrap()
                .state,
            EffectState::Intent
        );
    }
}

#[test]
fn owned_sqlite_reopen_preserves_full_pending_rows_receipt_audit_core_and_refuses_cold_handles() {
    let f = FullFixture::new(true, 2, true, vec![message(2, true)]);
    let c = f.connect();
    let o = f.open(c);
    let unit = f.unit();
    let e = f.effect(
        3,
        request(c, o, 2, true, 32, MqMqiUnitOfWork::Local { unit }, false),
    );
    f.seed(&e);
    let reply = f.execute(&e).unwrap();
    let rows = f.rows();
    let audits = f.audits();
    let inv = f.inv.clone();
    let provider = f.provider.clone();
    let old_frame = f.frame;
    let clock = f.clock.clone();
    let saf = f.saf.clone();
    let FullFixture { f, db, saf: _ } = f;
    drop(f);
    let db = db.unwrap();
    let store = db.open();
    assert_eq!(store.list_provider_state_prefix("mq-", 4096).unwrap(), rows);
    assert_eq!(
        store.audit_records(&inv.execution_id, 0, 256).unwrap(),
        audits
    );
    let state = rich(&*store);
    assert_eq!(
        state.receipts["effect-3"]
            .replay(HostLimits::default(), Default::default())
            .unwrap(),
        reply
    );
    assert_eq!(
        state.delivery.unit_outcome(unit),
        MqDeliveryOutcome::Pending
    );
    assert_eq!(
        store
            .effect(e.idempotency_key.as_ref().unwrap())
            .unwrap()
            .unwrap()
            .state,
        EffectState::Intent
    );
    let service =
        MqService::open_selected_mqi(store.clone(), MqLimits::default(), 3, 5, saf, clock).unwrap();
    let process = service.mint_selected_process(&inv).unwrap();
    let (frame, _) = service.bind_selected_root(process, &inv).unwrap();
    for frame in [old_frame, frame] {
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
        store.audit_records(&inv.execution_id, 0, 256).unwrap(),
        audits
    );
    drop(service);
    drop(store);
    drop(db);
}
