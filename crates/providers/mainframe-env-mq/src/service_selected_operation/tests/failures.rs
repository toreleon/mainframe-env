use super::*;

#[test]
fn memory_sqlite_retained_output_replay_rechecks_revoked_saf_without_redispatch() {
    for sqlite in [false, true] {
        let f = Fixture::new(sqlite);
        let c = f.connect();
        let o = f.open(c);
        let e = f.effect(
            3,
            MqMqiRequest::Put {
                connection: c,
                object: o,
                put: put(MqMqiUnitOfWork::NoSyncpoint),
            },
        );
        f.seed(&e);
        f.execute(&e).unwrap();
        let rows = f.rows();
        let audits = f.store.audit_records(&f.inv.execution_id, 0, 128).unwrap();
        f.saf.deny.store(true, Ordering::SeqCst);
        assert_eq!(f.execute(&e), Err(HostProblem::Unauthorized));
        assert_eq!(f.rows(), rows);
        assert_eq!(f.depth(), 1);
        assert_eq!(
            f.store.audit_records(&f.inv.execution_id, 0, 128).unwrap(),
            audits
        );
    }
}

#[test]
fn memory_sqlite_foreign_default_stale_put1_handles_fail_before_saf_or_publication() {
    for sqlite in [false, true] {
        let f = Fixture::new(sqlite);
        let c = f.connect();
        let other = Fixture::new(sqlite);
        let foreign = other.connect();
        for (n, handle) in [MqHconn::Default, MqHconn::Unassociated, foreign]
            .into_iter()
            .enumerate()
        {
            let e = f.effect(
                2 + n as u64,
                MqMqiRequest::PutOne {
                    connection: handle,
                    lookup: lookup(),
                    alternate_user: None,
                    put: put(MqMqiUnitOfWork::NoSyncpoint),
                },
            );
            if e.mq_mqi_occurrence(HostLimits::default()).is_err() {
                continue;
            }
            f.seed(&e);
            let rows = f.rows();
            let calls = f.saf.calls.load(Ordering::SeqCst);
            assert!(f.execute(&e).is_err());
            assert_eq!(f.rows(), rows);
            assert_eq!(f.saf.calls.load(Ordering::SeqCst), calls);
        }
        f.call(6, MqMqiRequest::Disconnect { connection: c });
        let e = f.effect(
            7,
            MqMqiRequest::PutOne {
                connection: c,
                lookup: lookup(),
                alternate_user: None,
                put: put(MqMqiUnitOfWork::NoSyncpoint),
            },
        );
        f.seed(&e);
        let rows = f.rows();
        let calls = f.saf.calls.load(Ordering::SeqCst);
        assert!(f.execute(&e).is_err());
        assert_eq!(f.rows(), rows);
        assert_eq!(f.saf.calls.load(Ordering::SeqCst), calls);
    }
}

#[test]
fn memory_sqlite_late_catalog_cas_failure_keeps_pending_uow_and_every_audit() {
    for sqlite in [false, true] {
        let f = Fixture::new(sqlite);
        let c = f.connect();
        let o = f.open(c);
        let unit = f.unit();
        f.call(
            3,
            MqMqiRequest::Put {
                connection: c,
                object: o,
                put: put(MqMqiUnitOfWork::Local { unit }),
            },
        );
        let e = f.effect(
            4,
            MqMqiRequest::Commit {
                connection: c,
                unit,
            },
        );
        f.seed(&e);
        let mut record = f
            .store
            .get_provider_state(CATALOG_NAMESPACE, CATALOG_KEY)
            .unwrap()
            .unwrap();
        let old = record.version;
        record.version += 1;
        f.store.put_provider_state(record, Some(old)).unwrap();
        let rows = f.rows();
        let audits = f.store.audit_records(&f.inv.execution_id, 0, 128).unwrap();
        assert!(f.execute(&e).is_err());
        assert_eq!(f.rows(), rows);
        assert_eq!(f.depth(), 0);
        assert_eq!(f.unit(), unit);
        assert_eq!(
            f.store.audit_records(&f.inv.execution_id, 0, 128).unwrap(),
            audits
        );
        let state = f.service.lock_selected().unwrap();
        let rich_state::StoredAuthority::Rich(s) = &*state else {
            panic!()
        };
        assert_eq!(s.delivery.unit_outcome(unit), MqDeliveryOutcome::Pending);
        assert_eq!(
            s.ownership.units[&unit].state,
            ownership::UnitState::Pending
        );
    }
}

#[test]
fn memory_sqlite_live_controls_changed_by_authorizer_roll_back_before_publication() {
    for sqlite in [false, true] {
        for cancel in [false, true] {
            let f = Fixture::new(sqlite);
            let c = f.connect();
            let o = f.open(c);
            let e = f.effect(
                3,
                MqMqiRequest::Put {
                    connection: c,
                    object: o,
                    put: put(MqMqiUnitOfWork::NoSyncpoint),
                },
            );
            f.seed(&e);
            let rows = f.rows();
            let audits = f.store.audit_records(&f.inv.execution_id, 0, 128).unwrap();
            let clock = f.clock.clone();
            let probe = f.inv.cancellation_probe.clone().unwrap();
            *f.saf.hook.lock().unwrap() = Some(Box::new(move || {
                if cancel {
                    probe.request();
                } else {
                    clock.0.store(901, Ordering::SeqCst);
                }
            }));
            assert!(f.execute(&e).is_err());
            assert_eq!(f.rows(), rows);
            assert_eq!(f.depth(), 0);
            assert_eq!(
                f.store.audit_records(&f.inv.execution_id, 0, 128).unwrap(),
                audits
            );
        }
    }
}

#[test]
fn memory_sqlite_core_intent_recovered_or_terminal_after_observation_refuses_atomic_batch() {
    for sqlite in [false, true] {
        for case in 0..3 {
            let f = Fixture::new(sqlite);
            let c = f.connect();
            let o = f.open(c);
            let e = f.effect(
                3,
                MqMqiRequest::Put {
                    connection: c,
                    object: o,
                    put: put(MqMqiUnitOfWork::NoSyncpoint),
                },
            );
            f.seed(&e);
            let rows = f.rows();
            let audits = f.store.audit_records(&f.inv.execution_id, 0, 128).unwrap();
            let store = f.store.clone();
            let key = e.idempotency_key.clone().unwrap();
            let execution = f.inv.execution_id.clone();
            *f.saf.hook.lock().unwrap() = Some(Box::new(move || match case {
                0 => {
                    store
                        .claim_stale_intent(&key, 8, "recovery", 900, 1, 10)
                        .unwrap();
                }
                1 => {
                    let mut record = store.effect(&key).unwrap().unwrap();
                    record.state = EffectState::Completed;
                    record.result_digest = Some([9; 32]);
                    record.resolved_tick = Some(20);
                    store.record_result(&key, record).unwrap();
                }
                _ => {
                    store
                        .transition_execution(&execution, 3, ExecutionState::Failed, 20)
                        .unwrap();
                }
            }));
            assert!(f.execute(&e).is_err());
            assert_eq!(f.rows(), rows);
            assert_eq!(f.depth(), 0);
            assert_eq!(
                f.store.audit_records(&f.inv.execution_id, 0, 128).unwrap(),
                audits
            );
        }
    }
}

#[test]
fn memory_sqlite_reply_uncertainty_commits_once_then_fences_every_new_dispatch() {
    for sqlite in [false, true] {
        let f = Fixture::new(sqlite);
        let c = f.connect();
        let o = f.open(c);
        let e = f.effect(
            3,
            MqMqiRequest::Put {
                connection: c,
                object: o,
                put: put(MqMqiUnitOfWork::NoSyncpoint),
            },
        );
        f.seed(&e);
        f.service
            .unknown_after_persist
            .store(true, Ordering::SeqCst);
        assert_eq!(f.execute(&e), Err(HostProblem::UnknownOutcome));
        assert_eq!(f.depth(), 1);
        let rows = f.rows();
        let audits = f.store.audit_records(&f.inv.execution_id, 0, 128).unwrap();
        assert_eq!(f.execute(&e), Err(HostProblem::UnknownOutcome));
        let next = f.effect(
            4,
            MqMqiRequest::Put {
                connection: c,
                object: o,
                put: put(MqMqiUnitOfWork::NoSyncpoint),
            },
        );
        f.seed(&next);
        assert_eq!(f.execute(&next), Err(HostProblem::UnknownOutcome));
        assert_eq!(f.rows(), rows);
        assert_eq!(f.depth(), 1);
        assert_eq!(
            f.store.audit_records(&f.inv.execution_id, 0, 128).unwrap(),
            audits
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
fn memory_sqlite_disconnect_commits_actual_retained_local_work_in_same_batch() {
    for sqlite in [false, true] {
        let f = Fixture::new(sqlite);
        let c = f.connect();
        let o = f.open(c);
        let unit = f.unit();
        f.call(
            3,
            MqMqiRequest::Put {
                connection: c,
                object: o,
                put: put(MqMqiUnitOfWork::Local { unit }),
            },
        );
        assert_eq!(f.depth(), 0);
        f.call(4, MqMqiRequest::Disconnect { connection: c });
        assert_eq!(f.depth(), 1);
        let state = f.service.lock_selected().unwrap();
        let rich_state::StoredAuthority::Rich(s) = &*state else {
            panic!()
        };
        assert_eq!(
            s.ownership.units[&unit].state,
            ownership::UnitState::Committed
        );
        assert!(s.runtime.as_ref().unwrap().connections.is_empty());
    }
}
