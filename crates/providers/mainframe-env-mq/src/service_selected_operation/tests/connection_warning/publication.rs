use super::*;

#[test]
fn memory_sqlite_warning_late_catalog_uow_control_cas_race_rolls_back_whole_publication() {
    for sqlite in [false, true] {
        for namespace in [
            CATALOG_NAMESPACE,
            ownership::UOW_NAMESPACE,
            ownership::CONTROL_NAMESPACE,
        ] {
            let f = Fixture::new(sqlite);
            let (_, _, unit) = pending(&f);
            let key = if namespace == CATALOG_NAMESPACE {
                CATALOG_KEY.to_string()
            } else if namespace == ownership::UOW_NAMESPACE {
                unit.to_string()
            } else {
                ownership::CONTROL_KEY.to_string()
            };
            let mut raced = f
                .store
                .get_provider_state(namespace, &key)
                .unwrap()
                .unwrap();
            let old = raced.version;
            raced.version += 1;
            let expected = raced.clone();
            let store = f.store.clone();
            *f.saf.hook.lock().unwrap() = Some(Box::new(move || {
                store.put_provider_state(raced, Some(old)).unwrap()
            }));
            let e = f.effect(4, request(false));
            f.seed(&e);
            let mut expected_rows = f.rows();
            *expected_rows
                .iter_mut()
                .find(|r| r.namespace == namespace && r.key == key)
                .unwrap() = expected;
            let snapshot = delivery(&f);
            let audits = f.store.audit_records(&f.inv.execution_id, 0, 128).unwrap();
            assert!(f.execute(&e).is_err());
            assert_eq!(f.rows(), expected_rows);
            assert_eq!(delivery(&f), snapshot);
            assert_eq!(
                f.store.audit_records(&f.inv.execution_id, 0, 128).unwrap(),
                audits
            );
            assert_eq!(f.unit(), unit);
        }
    }
}

#[test]
fn memory_sqlite_warning_denial_controls_cas_audit_and_quota_failures_keep_pending_state() {
    for sqlite in [false, true] {
        // SAF denial, cancellation, deadline, catalog/UOW/control CAS, audit
        // capacity, terminal/recovered intent and running-execution loss.
        for case in 0..10 {
            let f = Fixture::new(sqlite);
            let (_, _, unit) = pending(&f);
            let e = f.effect(4, request(case % 2 == 0));
            f.seed(&e);
            match case {
                0 => f.saf.deny.store(true, Ordering::SeqCst),
                1 => {
                    let probe = f.inv.cancellation_probe.clone().unwrap();
                    *f.saf.hook.lock().unwrap() = Some(Box::new(move || probe.request()));
                }
                2 => {
                    let clock = f.clock.clone();
                    *f.saf.hook.lock().unwrap() =
                        Some(Box::new(move || clock.0.store(901, Ordering::SeqCst)));
                }
                3..=5 => {
                    let (namespace, key) = match case {
                        3 => (CATALOG_NAMESPACE, CATALOG_KEY.to_string()),
                        4 => (ownership::UOW_NAMESPACE, unit.to_string()),
                        _ => (
                            ownership::CONTROL_NAMESPACE,
                            ownership::CONTROL_KEY.to_string(),
                        ),
                    };
                    let mut row = f
                        .store
                        .get_provider_state(namespace, &key)
                        .unwrap()
                        .unwrap();
                    let old = row.version;
                    row.version += 1;
                    f.store.put_provider_state(row, Some(old)).unwrap();
                }
                6 => super::super::bounds::fill_audits(&f),
                _ => {
                    let store = f.store.clone();
                    let key = e.idempotency_key.clone().unwrap();
                    let actor = f.inv.execution_id.clone();
                    *f.saf.hook.lock().unwrap() = Some(Box::new(move || match case {
                        7 => {
                            let mut core = store.effect(&key).unwrap().unwrap();
                            core.state = EffectState::Completed;
                            core.result_digest = Some([9; 32]);
                            core.resolved_tick = Some(20);
                            store.record_result(&key, core).unwrap();
                        }
                        8 => {
                            store
                                .claim_stale_intent(&key, 8, "recovery", 900, 1, 10)
                                .unwrap();
                        }
                        _ => {
                            store
                                .transition_execution(&actor, 3, ExecutionState::Failed, 20)
                                .unwrap();
                        }
                    }));
                }
            }
            let rows = f.rows();
            let snapshot = delivery(&f);
            let audits = f.store.audit_records(&f.inv.execution_id, 0, 256).unwrap();
            assert!(f.execute(&e).is_err(), "case {case}");
            assert_eq!(f.rows(), rows, "case {case}");
            assert_eq!(delivery(&f), snapshot);
            let after = f.store.audit_records(&f.inv.execution_id, 0, 256).unwrap();
            if case == 0 {
                assert_eq!(after.len(), audits.len() + 1);
                assert_eq!(after.last().unwrap().decision, AuditDecision::Deny);
            } else {
                assert_eq!(after, audits);
            }
            let mut state = f.service.lock_selected().unwrap();
            let rich_state::StoredAuthority::Rich(s) = &mut *state else {
                panic!()
            };
            let r = s.runtime.as_mut().unwrap();
            assert_eq!(r.connections.len(), 1);
            assert_eq!(r.connections[0].unit, unit);
            assert_eq!(r.handles.handles_mut().active_handles(), 2);
        }
    }
}

#[test]
fn memory_sqlite_warning_reply_uncertainty_publishes_once_and_fences_without_new_unit() {
    for sqlite in [false, true] {
        let f = Fixture::new(sqlite);
        let (_, _, unit) = pending(&f);
        let e = f.effect(4, request(false));
        f.seed(&e);
        let before = delivery(&f);
        f.service
            .unknown_after_persist
            .store(true, Ordering::SeqCst);
        assert_eq!(f.execute(&e), Err(HostProblem::UnknownOutcome));
        assert_eq!(delivery(&f), before);
        let rows = f.rows();
        assert!(
            f.store
                .get_provider_state(receipt::NAMESPACE, "effect-4")
                .unwrap()
                .is_some()
        );
        let audits = f.store.audit_records(&f.inv.execution_id, 0, 128).unwrap();
        assert_eq!(f.execute(&e), Err(HostProblem::UnknownOutcome));
        let next = f.effect(5, request(true));
        f.seed(&next);
        assert_eq!(f.execute(&next), Err(HostProblem::UnknownOutcome));
        assert_eq!(f.rows(), rows);
        assert_eq!(
            f.store.audit_records(&f.inv.execution_id, 0, 128).unwrap(),
            audits
        );
        let state = f.service.lock_selected().unwrap();
        let rich_state::StoredAuthority::Rich(s) = &*state else {
            panic!()
        };
        assert_eq!(s.runtime.as_ref().unwrap().connections[0].unit, unit);
        assert_eq!(s.ownership.units.len(), 1);
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
