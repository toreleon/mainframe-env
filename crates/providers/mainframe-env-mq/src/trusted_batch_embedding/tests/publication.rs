use super::*;

#[test]
fn memory_sqlite_facet_saf_denial_and_revoked_receipt_never_repeat_mutation() {
    for sqlite in [false, true] {
        let f = Fixture::new(sqlite);
        let root = f.root();
        let parent = root.frame();
        let mut child = f.child(&parent, "child");
        let (c, o) = connected(&f, &mut child);
        let e = effect(&child, 3, put_request(c, o, MqMqiUnitOfWork::NoSyncpoint));
        seed(&*f.store, child.original(), &e);
        let rows = f.rows();
        f.saf.deny.store(true, Ordering::SeqCst);
        assert_eq!(dispatch(&mut child, &e), Err(HostProblem::Unauthorized));
        assert_eq!(f.rows(), rows);
        assert_eq!(f.depth(), 0);
        assert_eq!(
            f.store
                .audit_records(&child.original().execution_id, 0, 128)
                .unwrap()
                .last()
                .unwrap()
                .decision,
            AuditDecision::Deny
        );
        f.saf.deny.store(false, Ordering::SeqCst);
        dispatch(&mut child, &e).unwrap();
        assert_eq!(f.depth(), 1);
        let rows = f.rows();
        let audits = f
            .store
            .audit_records(&child.original().execution_id, 0, 128)
            .unwrap();
        f.saf.deny.store(true, Ordering::SeqCst);
        assert_eq!(dispatch(&mut child, &e), Err(HostProblem::Unauthorized));
        assert_eq!(f.rows(), rows);
        assert_eq!(f.depth(), 1);
        assert_eq!(
            f.store
                .audit_records(&child.original().execution_id, 0, 128)
                .unwrap(),
            audits
        );
    }
}

#[test]
fn memory_sqlite_late_controls_and_core_changes_roll_back_facet_publication() {
    for sqlite in [false, true] {
        for case in 0..4 {
            let f = Fixture::new(sqlite);
            let root = f.root();
            let parent = root.frame();
            let mut child = f.child(&parent, "child");
            let (c, o) = connected(&f, &mut child);
            let e = effect(&child, 3, put_request(c, o, MqMqiUnitOfWork::NoSyncpoint));
            seed(&*f.store, child.original(), &e);
            let rows = f.rows();
            let audits = f
                .store
                .audit_records(&child.original().execution_id, 0, 128)
                .unwrap();
            let store = f.store.clone();
            let clock = f.clock.clone();
            let probe = child.original().cancellation_probe.clone().unwrap();
            let key = e.idempotency_key.clone().unwrap();
            let execution = child.original().execution_id.clone();
            *f.saf.hook.lock().unwrap() = Some(Box::new(move || match case {
                0 => probe.request(),
                1 => clock.0.store(901, Ordering::SeqCst),
                2 => {
                    store
                        .claim_stale_intent(&key, 8, "recovery", 900, 1, 10)
                        .unwrap();
                }
                _ => {
                    store
                        .transition_execution(&execution, 3, ExecutionState::Failed, 20)
                        .unwrap();
                }
            }));
            assert!(dispatch(&mut child, &e).is_err(), "case {case}");
            assert_eq!(f.rows(), rows);
            assert_eq!(f.depth(), 0);
            assert_eq!(
                f.store
                    .audit_records(&child.original().execution_id, 0, 128)
                    .unwrap(),
                audits
            );
        }
    }
}

#[test]
fn memory_sqlite_late_catalog_or_uow_or_control_cas_preserves_pending_work() {
    for sqlite in [false, true] {
        for (namespace, key) in [
            ("mq-v1-object-catalog", "catalog"),
            ("mq-selected-v1-uow-owner", "1"),
            ("mq-selected-v1-control", "state"),
        ] {
            let f = Fixture::new(sqlite);
            let root = f.root();
            let parent = root.frame();
            let mut child = f.child(&parent, "child");
            let (c, o) = connected(&f, &mut child);
            let u = unit(&child, c);
            f.call(
                &mut child,
                3,
                put_request(c, o, MqMqiUnitOfWork::Local { unit: u }),
            );
            let e = effect(
                &child,
                4,
                MqMqiRequest::Commit {
                    connection: c,
                    unit: u,
                },
            );
            seed(&*f.store, child.original(), &e);
            let audits = f
                .store
                .audit_records(&child.original().execution_id, 0, 128)
                .unwrap();
            let store = f.store.clone();
            let raced = Arc::new(Mutex::new(None));
            let capture = raced.clone();
            *f.saf.hook.lock().unwrap() = Some(Box::new(move || {
                let mut row = store.get_provider_state(namespace, key).unwrap().unwrap();
                let old = row.version;
                row.version += 1;
                store.put_provider_state(row, Some(old)).unwrap();
                *capture.lock().unwrap() =
                    Some(store.list_provider_state_prefix("mq-", 4096).unwrap());
            }));
            assert!(dispatch(&mut child, &e).is_err(), "{namespace}");
            assert_eq!(f.rows(), raced.lock().unwrap().clone().unwrap());
            assert_eq!(f.depth(), 0);
            assert_eq!(unit(&parent, c), u);
            assert_eq!(
                f.store
                    .audit_records(&child.original().execution_id, 0, 128)
                    .unwrap(),
                audits
            );
        }
    }
}

#[test]
fn memory_sqlite_late_audit_quota_rolls_back_entire_pending_commit() {
    for sqlite in [false, true] {
        let f = Fixture::new(sqlite);
        let root = f.root();
        let parent = root.frame();
        let mut child = f.child(&parent, "child");
        let (c, o) = connected(&f, &mut child);
        let u = unit(&child, c);
        f.call(
            &mut child,
            3,
            put_request(c, o, MqMqiUnitOfWork::Local { unit: u }),
        );
        let e = effect(
            &child,
            4,
            MqMqiRequest::Commit {
                connection: c,
                unit: u,
            },
        );
        seed(&*f.store, child.original(), &e);
        for n in 0..256 {
            let audit = AuditRecord {
                execution_id: child.original().execution_id.clone(),
                run_unit_id: child.original().run_unit_id.clone(),
                attempt: 1,
                effect_sequence: 1000 + n,
                observed_tick: 20,
                principal: child.original().principal.id().clone(),
                invocation_key: IdempotencyKey::new(format!("quota-{n}"), Default::default())
                    .unwrap(),
                capability: descriptor().capability,
                resource: canonical_audit_resource_digest(&e.request),
                decision: AuditDecision::Success,
            };
            match f.store.record_audit(audit) {
                Ok(()) => {}
                Err(StoreError::CapacityExceeded) => break,
                Err(e) => panic!("quota: {e:?}"),
            }
        }
        let rows = f.rows();
        let audits = f
            .store
            .audit_records(&child.original().execution_id, 0, 256)
            .unwrap();
        assert!(dispatch(&mut child, &e).is_err());
        assert_eq!(f.rows(), rows);
        assert_eq!(f.depth(), 0);
        assert_eq!(unit(&parent, c), u);
        assert_eq!(
            f.store
                .audit_records(&child.original().execution_id, 0, 256)
                .unwrap(),
            audits
        );
    }
}

#[test]
fn memory_sqlite_reply_uncertainty_publishes_once_and_revokes_without_return() {
    for sqlite in [false, true] {
        let f = Fixture::new(sqlite);
        let root = f.root();
        let parent = root.frame();
        let mut child = f.child(&parent, "child");
        let (c, o) = connected(&f, &mut child);
        let u = unit(&child, c);
        let e = effect(
            &child,
            3,
            put_request(c, o, MqMqiUnitOfWork::Local { unit: u }),
        );
        seed(&*f.store, child.original(), &e);
        f.runtime
            .inner
            .service
            .trusted_batch_test_reply_uncertainty();
        assert_eq!(dispatch(&mut child, &e), Err(HostProblem::UnknownOutcome));
        let rows = f.rows();
        let audits = f
            .store
            .audit_records(&child.original().execution_id, 0, 128)
            .unwrap();
        assert_eq!(dispatch(&mut child, &e), Err(HostProblem::Unauthorized));
        assert_eq!(child.return_normal(), Err(HostProblem::Unauthorized));
        assert_eq!(child.abort_preparation(), Err(HostProblem::Unauthorized));
        assert_eq!(parent.context(), Err(HostProblem::UnknownOutcome));
        assert_eq!(f.rows(), rows);
        assert_eq!(f.depth(), 0);
        assert_eq!(
            f.store
                .audit_records(&child.original().execution_id, 0, 128)
                .unwrap(),
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
