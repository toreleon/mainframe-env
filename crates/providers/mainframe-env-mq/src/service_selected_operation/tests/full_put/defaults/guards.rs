//! Physical failure boundaries on actual default-policy selected occurrences.
use super::*;

fn snapshot(f: &ProducerFixture) -> Vec<u8> {
    let state = f.service.lock_selected().unwrap();
    let rich_state::StoredAuthority::Rich(s) = &*state else {
        panic!()
    };
    s.delivery.encode_live_checkpoint().unwrap()
}

fn pending(f: &ProducerFixture, c: MqHconn) -> EffectRequest {
    f.effect(
        3,
        put(
            c,
            None,
            policy_message(2, -1, 2),
            true,
            MqMqiUnitOfWork::Local { unit: f.unit() },
        ),
    )
}

#[test]
fn default_real_queue_and_unit_saf_denial_errors_and_replay_remain_mandatory() {
    for sqlite in [false, true] {
        for error in 0..3 {
            let f = fixture(sqlite, 2, true);
            let c = f.connect();
            let e = pending(&f, c);
            f.seed(&e);
            let rows = f.rows();
            let live = snapshot(&f);
            f.saf.observations.lock().unwrap().clear();
            f.saf.deny.store(error == 0, Ordering::SeqCst);
            f.saf.error.store(error, Ordering::SeqCst);
            let expected = match error {
                0 => HostProblem::Unauthorized,
                1 => HostProblem::ProviderFailure,
                _ => HostProblem::InfrastructureFailure,
            };
            assert_eq!(f.execute(&e), Err(expected));
            assert_eq!(f.rows(), rows);
            assert_eq!(snapshot(&f), live);
            assert_eq!(f.ports.gmt_calls.load(Ordering::SeqCst), 0);
            assert!(f.saf.observations.lock().unwrap().iter().any(|(p, r)| p
                == f.inv.principal.id()
                && r.class == EnterpriseResourceClass::MqQueue
                && r.name.as_str() == "Q"
                && r.intent == AccessIntent::Update));
            let audit = f.store.audit_records(&f.inv.execution_id, 0, 256).unwrap();
            assert_eq!(
                audit.last().unwrap().decision,
                match error {
                    0 => AuditDecision::Deny,
                    1 => AuditDecision::ProviderFailure,
                    _ => AuditDecision::InfrastructureFailure,
                }
            );
            f.saf.deny.store(false, Ordering::SeqCst);
            f.saf.error.store(0, Ordering::SeqCst);
            let result = f.execute(&e).unwrap();
            assert!(
                f.saf
                    .observations
                    .lock()
                    .unwrap()
                    .iter()
                    .any(|(_, r)| r.class == EnterpriseResourceClass::MqUnitOfWork
                        && r.name.as_str() == "CURRENT")
            );
            let rows = f.rows();
            f.saf.deny.store(true, Ordering::SeqCst);
            assert_eq!(f.execute(&e), Err(HostProblem::Unauthorized));
            assert_eq!(f.rows(), rows);
            f.saf.deny.store(false, Ordering::SeqCst);
            assert_eq!(f.execute(&e).unwrap(), result);
        }
    }
}

#[test]
fn default_audit_capacity_boundary_and_row_payload_quotas_preserve_atomic_publication() {
    for sqlite in [false, true] {
        // The retained publication API appends one typed audit, not a private
        // second audit. Zero remaining slots rejects; one admits exactly one.
        // SQLite counts audits and provider mutations in its physical row cap.
        for remaining in [0, 1] {
            let f = fixture(sqlite, 2, true);
            let c = f.connect();
            let e = pending(&f, c);
            f.seed(&e);
            let used = if sqlite {
                f.rows().len()
                    + f.store
                        .list_provider_state_prefix("durable-", 4096)
                        .unwrap()
                        .len()
            } else {
                f.store
                    .audit_records(&f.inv.execution_id, 0, 256)
                    .unwrap()
                    .len()
            };
            for n in used..(256 - remaining) {
                f.store
                    .record_audit(AuditRecord {
                        execution_id: f.inv.execution_id.clone(),
                        run_unit_id: f.inv.run_unit_id.clone(),
                        attempt: 1,
                        effect_sequence: 1000 + n as u64,
                        observed_tick: 20,
                        principal: f.inv.principal.id().clone(),
                        invocation_key: IdempotencyKey::new(
                            format!("default-quota-{n}"),
                            InvocationLimits::default(),
                        )
                        .unwrap(),
                        capability: f.provider.capability.clone(),
                        resource: canonical_audit_resource_digest(&e.request),
                        decision: AuditDecision::Success,
                    })
                    .unwrap();
            }
            if remaining == 0 || sqlite {
                // SQLite also needs new receipt/payload rows; one free physical
                // slot is insufficient even though one audit alone would fit.
                unchanged_failed_publication(&f, &e);
            } else {
                let audit = f.store.audit_records(&f.inv.execution_id, 0, 256).unwrap();
                let reply = f.execute(&e).unwrap();
                assert_eq!(produced(&reply).descriptor.fields().priority, -1);
                let after = f.store.audit_records(&f.inv.execution_id, 0, 256).unwrap();
                assert_eq!(
                    after
                        .iter()
                        .filter(|a| a.effect_sequence != 3)
                        .cloned()
                        .collect::<Vec<_>>(),
                    audit
                );
                assert_eq!(after.len(), 256);
                assert_eq!(
                    after
                        .iter()
                        .find(|a| a.effect_sequence == 3)
                        .unwrap()
                        .decision,
                    AuditDecision::Success
                );
            }
        }
        for payload in [false, true] {
            let f = fixture_limits(sqlite, 2, true, if payload { 16384 } else { 64 << 20 });
            let c = f.connect();
            let mut m = policy_message(2, -1, 2);
            if payload {
                m.body = vec![255; 12000];
            }
            let e = f.effect(
                3,
                put(c, None, m, true, MqMqiUnitOfWork::Local { unit: f.unit() }),
            );
            f.seed(&e);
            if !payload {
                let used = f.rows().len()
                    + if sqlite {
                        f.store
                            .list_provider_state_prefix("durable-", 4096)
                            .unwrap()
                            .len()
                    } else {
                        0
                    };
                for n in used..256 {
                    f.store
                        .put_provider_state(
                            ProviderStateRecord {
                                namespace: "default-quota".into(),
                                key: format!("{n}"),
                                version: 1,
                                payload: vec![1],
                            },
                            None,
                        )
                        .unwrap();
                }
            }
            unchanged_failed_publication(&f, &e);
        }
    }
}

fn unchanged_failed_publication(f: &ProducerFixture, e: &EffectRequest) {
    let rows = f.rows();
    let audits = f.store.audit_records(&f.inv.execution_id, 0, 256).unwrap();
    let live = snapshot(f);
    assert!(f.execute(e).is_err());
    assert_eq!(f.rows(), rows);
    assert_eq!(
        f.store.audit_records(&f.inv.execution_id, 0, 256).unwrap(),
        audits
    );
    assert_eq!(snapshot(f), live);
    assert!(
        f.store
            .get_provider_state(receipt::NAMESPACE, "effect-3")
            .unwrap()
            .is_none()
    );
    let unit = f.unit();
    let state = f.service.lock_selected().unwrap();
    let rich_state::StoredAuthority::Rich(s) = &*state else {
        panic!()
    };
    assert!(s.ownership.units[&unit].queues.is_empty());
}

#[test]
fn default_failure_audit_capacity_does_not_publish_candidate_or_partial_audit() {
    for sqlite in [false, true] {
        let f = fixture(sqlite, 2, true);
        let c = f.connect();
        let e = f.effect(
            3,
            put(
                c,
                None,
                policy_message(2, -1, 2),
                false,
                MqMqiUnitOfWork::Local { unit: f.unit() },
            ),
        );
        f.seed(&e);
        super::super::super::bounds::fill_audits(&f.f);
        f.ports.mode.store(1, Ordering::SeqCst);
        unchanged_failed_publication(&f, &e);
        assert_eq!(f.ports.gmt_calls.load(Ordering::SeqCst), 1);
        assert_eq!(f.ports.context_calls.load(Ordering::SeqCst), 0);
    }
}

#[test]
fn same_original_concurrent_default_put_has_one_physical_winner_and_no_reresolution() {
    for sqlite in [false, true] {
        let f = fixture(sqlite, 2, true);
        let c = f.connect();
        let o = f.open(c);
        let e = f.effect(
            3,
            put(
                c,
                None,
                policy_message(2, -1, 2),
                true,
                MqMqiUnitOfWork::NoSyncpoint,
            ),
        );
        f.seed(&e);
        let results = std::thread::scope(|scope| {
            let a = scope.spawn(|| f.execute(&e));
            let b = scope.spawn(|| f.execute(&e));
            (a.join().unwrap(), b.join().unwrap())
        });
        assert_eq!(results.0.as_ref().unwrap(), results.1.as_ref().unwrap());
        let stored = f.get(c, o, 4, 2, false).unwrap();
        assert_eq!(
            (
                stored.descriptor.fields().priority,
                stored.descriptor.fields().persistence
            ),
            (7, 1)
        );
        assert!(f.get(c, o, 5, 2, false).is_none());
    }
}
