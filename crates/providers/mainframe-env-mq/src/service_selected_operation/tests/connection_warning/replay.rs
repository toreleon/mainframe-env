use super::*;

fn complete(f: &Fixture, e: &EffectRequest, result: &EffectResult) -> EffectRecord {
    let key = e.idempotency_key.as_ref().unwrap();
    let mut record = f.store.effect(key).unwrap().unwrap();
    record.state = EffectState::Completed;
    record.result_digest = Some(canonical_result_digest(&result.outcome).unwrap());
    record.resolved_tick = Some(20);
    f.store.record_result(key, record.clone()).unwrap();
    record
}

#[test]
fn memory_sqlite_completed_warning_and_core_receipt_collision_revoke_without_redispatch() {
    for sqlite in [false, true] {
        for case in 0..6 {
            let f = Fixture::new(sqlite);
            let c = f.connect();
            let e = f.effect(2, request(false));
            f.seed(&e);
            let reply = f.execute(&e).unwrap();
            warning(reply.clone(), c, MqMqiCall::Connect);
            if case == 0 {
                complete(&f, &e, &reply);
                assert_eq!(f.execute(&e).unwrap(), reply);
                f.saf.deny.store(true, Ordering::SeqCst);
            } else if case == 1 {
                let mut different = reply.clone();
                let Ok(HostResult::MqMqi(h)) = &mut different.outcome else {
                    panic!()
                };
                h.result.outcome = MqMqiOutcome::Completed {
                    status: MqMqiStatus::OkNone,
                    output: MqMqiOutput::Connected(c),
                };
                complete(&f, &e, &different);
            } else if case == 2 {
                // Coherent fixture substitution of stored codec bytes + both
                // result digests still cannot replace the runtime-issued reply.
                let mut substituted = reply.clone();
                let Ok(HostResult::MqMqi(h)) = &mut substituted.outcome else {
                    panic!()
                };
                h.result.outcome = MqMqiOutcome::Completed {
                    status: MqMqiStatus::OkNone,
                    output: MqMqiOutput::Connected(c),
                };
                let bytes = crate::mqi_replay::encode(
                    &h.result,
                    HostLimits::default(),
                    h.limits,
                    h.limits.canonical_bytes,
                )
                .unwrap();
                let digest = canonical_result_digest(&substituted.outcome).unwrap();
                let mut row = f
                    .store
                    .get_provider_state(receipt::NAMESPACE, "effect-2")
                    .unwrap()
                    .unwrap();
                let mut json: serde_json::Value = serde_json::from_slice(&row.payload).unwrap();
                json["value"]["result_digest"] = serde_json::json!(digest);
                json["value"]["reply"]["bytes"] = serde_json::json!(bytes);
                row.payload = serde_json::to_vec(&json).unwrap();
                let decoded: ObjectRow<receipt::OccurrenceReceipt> =
                    serde_json::from_slice(&row.payload).unwrap();
                {
                    let state = f.service.lock_selected().unwrap();
                    let rich_state::StoredAuthority::Rich(s) = &*state else {
                        panic!()
                    };
                    decoded
                        .value
                        .validate("effect-2", &s.runtime.as_ref().unwrap().control)
                        .unwrap();
                }
                assert_eq!(
                    canonical_result_digest(
                        &decoded
                            .value
                            .replay(Default::default(), Default::default())
                            .unwrap()
                            .outcome
                    )
                    .unwrap(),
                    digest
                );
                // Deliberately malicious fixture authority, not product writing.
                f.store
                    .delete_provider_state(&row.namespace, &row.key, row.version)
                    .unwrap();
                f.store.put_provider_state(row, None).unwrap();
                complete(&f, &e, &substituted);
            } else if case == 3 {
                let mut state = f.service.lock_selected().unwrap();
                let rich_state::StoredAuthority::Rich(s) = &mut *state else {
                    panic!()
                };
                s.runtime.as_mut().unwrap().connections[0].key = "substituted-origin".into();
            } else if case == 4 {
                f.clock.0.store(901, Ordering::SeqCst);
            } else {
                f.inv.cancellation_probe.as_ref().unwrap().request();
            }
            let rows = f.rows();
            let audits = f.store.audit_records(&f.inv.execution_id, 0, 128).unwrap();
            let result = f.execute(&e);
            assert!(result.is_err(), "case {case}");
            if case == 2 {
                assert_eq!(result, Err(HostProblem::UnknownOutcome));
            }
            assert_eq!(f.rows(), rows);
            assert_eq!(
                f.store.audit_records(&f.inv.execution_id, 0, 128).unwrap(),
                audits
            );
        }
    }
}

#[test]
fn memory_sqlite_completed_warning_refuses_after_physical_incarnation_advance() {
    for sqlite in [false, true] {
        let f = Fixture::new(sqlite);
        f.connect();
        let e = f.effect(2, request(true));
        f.seed(&e);
        let reply = f.execute(&e).unwrap();
        let core = complete(&f, &e, &reply);
        let cold = MqService::open_selected_mqi(
            f.store.clone(),
            Default::default(),
            3,
            5,
            f.saf.clone(),
            f.clock.clone(),
        )
        .unwrap();
        let process = cold.mint_selected_process(&f.inv).unwrap();
        let (frame, owner) = cold.bind_selected_root(process, &f.inv).unwrap();
        let mut fresh = f.effect(20, request(false));
        let HostRequest::MqMqi(h) = &mut fresh.request else {
            panic!()
        };
        h.envelope.context.owner = owner;
        f.seed(&fresh);
        cold.execute_selected_mqi(
            frame,
            &f.inv,
            fresh
                .mq_mqi_occurrence(Default::default())
                .unwrap()
                .unwrap(),
            &f.provider,
            Default::default(),
        )
        .unwrap();
        let rows = f.rows();
        let calls = f.saf.calls.load(Ordering::SeqCst);
        assert_eq!(f.execute(&e), Err(HostProblem::UnknownOutcome));
        assert_eq!(f.rows(), rows);
        assert_eq!(f.saf.calls.load(Ordering::SeqCst), calls);
        assert_eq!(
            f.store.effect(e.idempotency_key.as_ref().unwrap()).unwrap(),
            Some(core)
        );
    }
}

#[test]
fn sqlite_physical_reopen_retains_warning_pending_rows_but_never_rehydrates_handle() {
    let db = super::super::restart::Database::new();
    let (inv, provider, e, captured, old) = {
        let f = Fixture::from_store(db.open());
        let (c, _, _) = pending(&f);
        let e = f.effect(4, request(false));
        f.seed(&e);
        let reply = f.execute(&e).unwrap();
        complete(&f, &e, &reply);
        (f.inv.clone(), f.provider.clone(), e, f.rows(), c)
    };
    let store = db.open();
    assert_eq!(
        store.list_provider_state_prefix("mq-", 4096).unwrap(),
        captured
    );
    let saf = Arc::new(Saf::default());
    let cold = MqService::open_selected_mqi(
        store.clone(),
        Default::default(),
        3,
        5,
        saf.clone(),
        Arc::new(Clock(std::sync::atomic::AtomicU64::new(20))),
    )
    .unwrap();
    let process = cold.mint_selected_process(&inv).unwrap();
    let (frame, _) = cold.bind_selected_root(process, &inv).unwrap();
    assert!(cold.selected_local_unit(frame, &inv, old).is_err());
    assert!(
        cold.execute_selected_mqi(
            frame,
            &inv,
            e.mq_mqi_occurrence(Default::default()).unwrap().unwrap(),
            &provider,
            Default::default()
        )
        .is_err()
    );
    assert_eq!(
        store.list_provider_state_prefix("mq-", 4096).unwrap(),
        captured
    );
    assert_eq!(saf.calls.load(Ordering::SeqCst), 0);
}
