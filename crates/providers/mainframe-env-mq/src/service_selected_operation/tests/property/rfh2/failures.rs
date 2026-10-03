use super::*;

#[test]
fn rfh2_malformed_import_publishes_exact_failed_observation_without_associated_md_mutation() {
    for sqlite in [false, true] {
        let (_db, mut f) = both(sqlite);
        configure(&mut f);
        let c = f.connect();
        let h = hmsg(f.call(2, create(c)));
        let descriptor =
            mainframe_env_host_api::mq_mqi::rfh2::mq_rfh2_outer_descriptor(&md()).unwrap();
        let state = snapshot(&f);
        let reply = f.call(3, import(c, h, descriptor, vec![0; 36]));
        assert_eq!(pair(&reply), (MqCompletion::Failed, 2334));
        assert_eq!(
            observed(reply),
            MqRfh2Observation {
                descriptor: None,
                data_length: None,
                buffer: MqRfh2BufferObservation::Unchanged
            }
        );
        assert_eq!(snapshot(&f), state);
        assert_eq!(
            value(f.call(4, inquire(c, h, "Root.MQMD.MsgId", 128, 24))).copied_value,
            vec![0; 24]
        );
    }
}

#[test]
fn rfh2_real_saf_deny_and_error_audit_only_without_descriptor_or_property_mutation() {
    for sqlite in [false, true] {
        for mode in [1, 2] {
            for importing in [false, true] {
                let (_db, mut f) = both(sqlite);
                configure(&mut f);
                let policy = install_policy(&mut f);
                let c = f.connect();
                let h = hmsg(f.call(2, create(c)));
                f.call(
                    3,
                    set(c, h, "invoice.id", MqPropertyType::String, b"abc".to_vec()),
                );
                let empty = hmsg(f.call(4, create(c)));
                let request = if importing {
                    import(c, empty, md(), vec![])
                } else {
                    export(c, h, 3, 4096)
                };
                let effect = f.effect(5, request);
                f.seed(&effect);
                let state = snapshot(&f);
                let rows = f.rows();
                let audits = f.store.audit_records(&f.inv.execution_id, 0, 128).unwrap();
                policy.mode.store(mode, Ordering::SeqCst);
                assert_eq!(
                    f.execute(&effect),
                    Err(if mode == 1 {
                        HostProblem::Unauthorized
                    } else {
                        HostProblem::InfrastructureFailure
                    })
                );
                assert_eq!(snapshot(&f), state);
                assert_eq!(f.rows(), rows);
                let records = f.store.audit_records(&f.inv.execution_id, 0, 128).unwrap();
                assert_eq!(records.len(), audits.len() + 1);
                let last = records.last().unwrap();
                assert_eq!(last.effect_sequence, 5);
                assert_eq!(
                    last.resource,
                    canonical_audit_resource_digest(&effect.request)
                );
                assert_eq!(
                    last.decision,
                    if mode == 1 {
                        AuditDecision::Deny
                    } else {
                        AuditDecision::InfrastructureFailure
                    }
                );
                assert_eq!(
                    policy.observed.lock().unwrap().last().unwrap().1.intent,
                    AccessIntent::Update
                );
            }
        }
    }
}
#[test]
fn rfh2_late_catalog_cas_and_audit_saturation_abort_import_delete_and_retain() {
    for sqlite in [false, true] {
        for action in 0..3 {
            for failure in 0..2 {
                let (_db, mut f) = both(sqlite);
                configure(&mut f);
                let c = f.connect();
                let h = hmsg(f.call(2, create(c)));
                f.call(
                    3,
                    set(c, h, "invoice.id", MqPropertyType::String, b"abc".to_vec()),
                );
                let empty = hmsg(f.call(4, create(c)));
                let request = match action {
                    0 => import(c, empty, md(), vec![]),
                    1 => export(c, h, 3, 4096),
                    _ => export(c, h, 1, 4096),
                };
                let e = f.effect(5, request);
                f.seed(&e);
                if failure == 0 {
                    super::super::super::bounds::fill_audits(&f);
                } else {
                    let mut row = f
                        .store
                        .get_provider_state(CATALOG_NAMESPACE, CATALOG_KEY)
                        .unwrap()
                        .unwrap();
                    let version = row.version;
                    row.version += 1;
                    f.store.put_provider_state(row, Some(version)).unwrap();
                }
                let state = snapshot(&f);
                let rows = f.rows();
                let audits = f.store.audit_records(&f.inv.execution_id, 0, 256).unwrap();
                assert!(f.execute(&e).is_err());
                assert_eq!(snapshot(&f), state);
                assert_eq!(f.rows(), rows);
                assert_eq!(
                    f.store.audit_records(&f.inv.execution_id, 0, 256).unwrap(),
                    audits
                );
                assert!(
                    f.store
                        .get_provider_state(receipt::NAMESPACE, "effect-5")
                        .unwrap()
                        .is_none()
                );
            }
        }
    }
}
#[test]
fn rfh2_cancel_deadline_clock_recovery_changes_abort_before_adoption() {
    for sqlite in [false, true] {
        for case in 0..4 {
            let (_db, mut f) = both(sqlite);
            configure(&mut f);
            let policy = install_policy(&mut f);
            let c = f.connect();
            let h = hmsg(f.call(2, create(c)));
            f.call(
                3,
                set(c, h, "invoice.id", MqPropertyType::String, b"abc".to_vec()),
            );
            let e = f.effect(4, export(c, h, 3, 4096));
            f.seed(&e);
            let state = snapshot(&f);
            let rows = f.rows();
            let clock = f.clock.clone();
            let probe = f.inv.cancellation_probe.clone().unwrap();
            let store = f.store.clone();
            let key = e.idempotency_key.clone().unwrap();
            *policy.hook.lock().unwrap() = Some(Box::new(move || match case {
                0 => probe.request(),
                1 => clock.0.store(901, Ordering::SeqCst),
                2 => clock.0.store(19, Ordering::SeqCst),
                _ => {
                    store
                        .claim_stale_intent(&key, 8, "recovery", 900, 1, 10)
                        .unwrap();
                }
            }));
            assert!(f.execute(&e).is_err());
            assert_eq!(snapshot(&f), state);
            assert_eq!(f.rows(), rows);
            assert_eq!(
                f.store
                    .audit_records(&f.inv.execution_id, 0, 128)
                    .unwrap()
                    .len(),
                3
            );
        }
    }
}
#[test]
fn rfh2_postpersist_unknown_keeps_one_receipt_without_import_or_delete_or_redispatch() {
    for sqlite in [false, true] {
        for importing in [false, true] {
            let (_db, mut f) = both(sqlite);
            let source = configure(&mut f);
            let c = f.connect();
            let h = hmsg(f.call(2, create(c)));
            f.call(
                3,
                set(c, h, "invoice.id", MqPropertyType::String, b"abc".to_vec()),
            );
            let empty = hmsg(f.call(4, create(c)));
            let e = f.effect(
                5,
                if importing {
                    import(c, empty, md(), vec![])
                } else {
                    export(c, h, 3, 4096)
                },
            );
            f.seed(&e);
            let state = snapshot(&f);
            f.service
                .unknown_after_persist
                .store(true, Ordering::SeqCst);
            assert_eq!(f.execute(&e), Err(HostProblem::UnknownOutcome));
            assert_eq!(snapshot(&f), state);
            assert!(
                f.store
                    .get_provider_state(receipt::NAMESPACE, "effect-5")
                    .unwrap()
                    .is_some()
            );
            let rows = f.rows();
            let audits = f.store.audit_records(&f.inv.execution_id, 0, 128).unwrap();
            assert_eq!(f.execute(&e), Err(HostProblem::UnknownOutcome));
            assert_eq!(f.rows(), rows);
            assert_eq!(
                f.store.audit_records(&f.inv.execution_id, 0, 128).unwrap(),
                audits
            );
            assert_eq!(audits.len(), 5);
            assert_eq!(source.calls.load(Ordering::SeqCst), 1);
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
}
#[test]
fn rfh2_foreign_historical_closed_in_use_and_unattested_connections_refuse_without_saf() {
    for sqlite in [false, true] {
        let (_db, mut f) = both(sqlite);
        configure(&mut f);
        let c = f.connect();
        let h = hmsg(f.call(2, create(c)));
        let mut other = Fixture::new(false);
        configure(&mut other);
        let c2 = other.connect();
        let h2 = hmsg(other.call(2, create(c2)));
        let history = MqHandleObservation::from(MqHandle::Message(h))
            .historical_message()
            .unwrap();
        for (seq, token) in [(3, h2), (4, history)] {
            let e = f.effect(seq, export(c, token, 1, 4096));
            f.seed(&e);
            let rows = f.rows();
            let state = snapshot(&f);
            let calls = f.saf.calls.load(Ordering::SeqCst);
            assert!(f.execute(&e).is_err());
            assert_eq!(f.rows(), rows);
            assert_eq!(snapshot(&f), state);
            assert_eq!(f.saf.calls.load(Ordering::SeqCst), calls);
        }
        {
            let mut g = f.service.lock_selected().unwrap();
            let rich_state::StoredAuthority::Rich(s) = &mut *g else {
                panic!()
            };
            s.runtime
                .as_mut()
                .unwrap()
                .handles
                .message_handles_mut()
                .begin_io(f.owner, c, h)
                .unwrap();
        }
        let e = f.effect(5, export(c, h, 1, 4096));
        f.seed(&e);
        let rows = f.rows();
        let state = snapshot(&f);
        assert!(f.execute(&e).is_err());
        assert_eq!(f.rows(), rows);
        assert_eq!(snapshot(&f), state);
        {
            let mut g = f.service.lock_selected().unwrap();
            let rich_state::StoredAuthority::Rich(s) = &mut *g else {
                panic!()
            };
            s.runtime
                .as_mut()
                .unwrap()
                .handles
                .message_handles_mut()
                .end_io(f.owner, c, h)
                .unwrap();
        }
        f.call(6, retire(c, h));
        let e = f.effect(7, export(c, h, 1, 4096));
        f.seed(&e);
        assert!(f.execute(&e).is_err());
        let plain = Fixture::new(false);
        let pc = plain.connect();
        let ph = hmsg(plain.call(2, create(pc)));
        let e = plain.effect(3, import(pc, ph, md(), vec![]));
        plain.seed(&e);
        let rows = plain.rows();
        assert_eq!(plain.execute(&e), Err(HostProblem::Unsupported));
        assert_eq!(plain.rows(), rows);
    }
}
#[test]
fn rfh2_populated_merge_and_unrepresented_properties_fail_before_mutation() {
    let mut f = Fixture::new(false);
    configure(&mut f);
    let c = f.connect();
    let h = hmsg(f.call(2, create(c)));
    f.call(
        3,
        set(
            c,
            h,
            "invoice.id",
            MqPropertyType::String,
            b"embedded\0".to_vec(),
        ),
    );
    for (seq, req) in [(4, export(c, h, 1, 4096)), (5, import(c, h, md(), vec![]))] {
        let e = f.effect(seq, req);
        f.seed(&e);
        let state = snapshot(&f);
        let rows = f.rows();
        assert_eq!(f.execute(&e), Err(HostProblem::Unsupported));
        assert_eq!(snapshot(&f), state);
        assert_eq!(f.rows(), rows);
    }
}
#[test]
fn rfh2_concurrent_original_delete_has_one_actual_property_winner_and_exact_failure() {
    let mut f = Fixture::new(false);
    configure(&mut f);
    let c = f.connect();
    let h = hmsg(f.call(2, create(c)));
    f.call(3, set(c, h, "invoice.id", MqPropertyType::Int8, vec![255]));
    let first = f.effect(4, export(c, h, 3, 4096));
    let second = f.effect(5, export(c, h, 3, 4096));
    f.seed(&first);
    f.seed(&second);
    let f = Arc::new(f);
    let left = f.clone();
    let right = f.clone();
    let gate = Arc::new(std::sync::Barrier::new(2));
    let gate2 = gate.clone();
    let a = std::thread::spawn(move || {
        gate.wait();
        left.execute(&first).unwrap()
    });
    let b = std::thread::spawn(move || {
        gate2.wait();
        right.execute(&second).unwrap()
    });
    let mut pairs = vec![pair(&a.join().unwrap()), pair(&b.join().unwrap())];
    pairs.sort_by_key(|v| v.1);
    assert_eq!(
        pairs,
        vec![(MqCompletion::Ok, 0), (MqCompletion::Failed, 2471)]
    );
    assert_eq!(
        f.store
            .audit_records(&f.inv.execution_id, 0, 128)
            .unwrap()
            .len(),
        5
    );
}
