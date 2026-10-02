use super::*;

#[test]
fn independently_opened_byte_identical_snapshots_do_not_share_property_authority() {
    for sqlite in [false, true] {
        let first_db = super::super::restart::Database::new();
        let second_db = super::super::restart::Database::new();
        let first = if sqlite {
            Fixture::from_store(first_db.open())
        } else {
            Fixture::new(false)
        };
        let second = if sqlite {
            Fixture::from_store(second_db.open())
        } else {
            Fixture::new(false)
        };
        assert_eq!(first.rows(), second.rows());
        assert!(!Arc::ptr_eq(&first.store, &second.store));
        let c = first.connect();
        let h = hmsg(first.call(2, create(c)));
        let own = second.connect();
        for (sequence, request) in [(2, create(c)), (3, inquire(own, h, "a", 64, 64))] {
            let e = second.effect(sequence, request);
            second.seed(&e);
            let rows = second.rows();
            let state = snapshot(&second);
            let calls = second.saf.calls.load(Ordering::SeqCst);
            assert!(second.execute(&e).is_err());
            assert_eq!(second.rows(), rows);
            assert_eq!(snapshot(&second), state);
            assert_eq!(second.saf.calls.load(Ordering::SeqCst), calls);
        }
    }
}

#[test]
fn actual_resource_saf_deny_and_error_publish_only_matching_typed_audit() {
    for sqlite in [false, true] {
        for mode in [1, 2] {
            let db = super::super::restart::Database::new();
            let mut f = if sqlite {
                Fixture::from_store(db.open())
            } else {
                Fixture::new(false)
            };
            let policy = install_policy(&mut f);
            let c = f.connect();
            let h = hmsg(f.call(2, create(c)));
            let e = f.effect(3, set(c, h, "a", MqPropertyType::Int8, vec![255]));
            f.seed(&e);
            let rows = f.rows();
            let state = snapshot(&f);
            let n = f
                .store
                .audit_records(&f.inv.execution_id, 0, 128)
                .unwrap()
                .len();
            policy.mode.store(mode, Ordering::SeqCst);
            assert_eq!(
                f.execute(&e),
                Err(if mode == 1 {
                    HostProblem::Unauthorized
                } else {
                    HostProblem::InfrastructureFailure
                })
            );
            assert_eq!(f.rows(), rows);
            assert_eq!(snapshot(&f), state);
            let audits = f.store.audit_records(&f.inv.execution_id, 0, 128).unwrap();
            assert_eq!(audits.len(), n + 1);
            let last = audits.last().unwrap();
            assert_eq!(last.effect_sequence, 3);
            assert_eq!(last.resource, canonical_audit_resource_digest(&e.request));
            assert_eq!(
                last.decision,
                if mode == 1 {
                    AuditDecision::Deny
                } else {
                    AuditDecision::InfrastructureFailure
                }
            );
            let observed = policy.observed.lock().unwrap();
            let (principal, resource) = observed.last().unwrap();
            assert_eq!(principal, f.inv.principal.id());
            assert_eq!(resource.class, EnterpriseResourceClass::MqUnitOfWork);
            assert_eq!(resource.name.as_str(), "CURRENT");
            assert_eq!(resource.intent, AccessIntent::Update);
        }
    }
}

#[test]
fn late_audit_capacity_and_row_cas_abort_create_set_and_delete_without_generation_changes() {
    for sqlite in [false, true] {
        for action in 0..3 {
            for failure in 0..2 {
                let db = super::super::restart::Database::new();
                let f = if sqlite {
                    Fixture::from_store(db.open())
                } else {
                    Fixture::new(false)
                };
                let c = f.connect();
                let h = hmsg(f.call(2, create(c)));
                f.call(3, set(c, h, "a", MqPropertyType::ByteString, vec![0, 255]));
                let request = match action {
                    0 => create(c),
                    1 => set(c, h, "a", MqPropertyType::ByteString, vec![99]),
                    _ => retire(c, h),
                };
                let e = f.effect(4, request);
                f.seed(&e);
                if failure == 0 {
                    super::super::bounds::fill_audits(&f);
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
                let rows = f.rows();
                let state = snapshot(&f);
                let audits = f.store.audit_records(&f.inv.execution_id, 0, 256).unwrap();
                assert!(f.execute(&e).is_err());
                assert_eq!(f.rows(), rows);
                assert_eq!(snapshot(&f), state);
                assert_eq!(
                    f.store.audit_records(&f.inv.execution_id, 0, 256).unwrap(),
                    audits
                );
                let MqMqiOutput::PropertyObservation(MqPropertyObservation::Inquired(v)) =
                    peek(&f, inquire(c, h, "a", 128, 128))
                else {
                    panic!()
                };
                assert_eq!(v.copied_value, vec![0, 255]);
            }
        }
    }
}

#[test]
fn live_cancel_deadline_backwards_clock_and_recovery_win_without_property_adoption() {
    for sqlite in [false, true] {
        for case in 0..4 {
            let db = super::super::restart::Database::new();
            let mut f = if sqlite {
                Fixture::from_store(db.open())
            } else {
                Fixture::new(false)
            };
            let policy = install_policy(&mut f);
            let c = f.connect();
            let h = hmsg(f.call(2, create(c)));
            let e = f.effect(3, set(c, h, "a", MqPropertyType::Null, vec![]));
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
            assert_eq!(f.rows(), rows);
            assert_eq!(snapshot(&f), state);
            assert_eq!(
                f.store
                    .audit_records(&f.inv.execution_id, 0, 128)
                    .unwrap()
                    .len(),
                2
            );
        }
    }
}

#[test]
fn unknown_after_persist_retains_receipt_without_live_create_set_or_retirement_adoption() {
    for sqlite in [false, true] {
        for action in 0..3 {
            let db = super::super::restart::Database::new();
            let f = if sqlite {
                Fixture::from_store(db.open())
            } else {
                Fixture::new(false)
            };
            let c = f.connect();
            let h = hmsg(f.call(2, create(c)));
            f.call(3, set(c, h, "a", MqPropertyType::ByteString, vec![1]));
            let e = f.effect(
                4,
                match action {
                    0 => create(c),
                    1 => set(c, h, "a", MqPropertyType::ByteString, vec![2]),
                    _ => retire(c, h),
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
                    .get_provider_state(receipt::NAMESPACE, "effect-4")
                    .unwrap()
                    .is_some()
            );
            let rows = f.rows();
            assert_eq!(f.execute(&e), Err(HostProblem::UnknownOutcome));
            assert_eq!(f.rows(), rows);
            let original = f
                .store
                .effect(e.idempotency_key.as_ref().unwrap())
                .unwrap()
                .unwrap();
            assert_eq!(original.state, EffectState::Intent);
            assert_eq!(
                f.store
                    .audit_records(&f.inv.execution_id, 0, 128)
                    .unwrap()
                    .len(),
                4
            );
        }
    }
}

#[test]
fn in_use_foreign_historical_and_closed_handles_cannot_publish_or_ask_saf() {
    for sqlite in [false, true] {
        let db = super::super::restart::Database::new();
        let mut f = if sqlite {
            Fixture::from_store(db.open())
        } else {
            Fixture::new(false)
        };
        let policy = install_policy(&mut f);
        let c = f.connect();
        let h = hmsg(f.call(2, create(c)));
        let other = Fixture::new(false);
        let c2 = other.connect();
        let h2 = hmsg(other.call(2, create(c2)));
        let historical = MqHandleObservation::from(MqHandle::Message(h))
            .historical_message()
            .unwrap();
        for (n, token) in [h2, historical].into_iter().enumerate() {
            let e = f.effect(3 + n as u64, inquire(c, token, "a", 64, 64));
            f.seed(&e);
            let rows = f.rows();
            let calls = policy.observed.lock().unwrap().len();
            assert!(f.execute(&e).is_err());
            assert_eq!(f.rows(), rows);
            assert_eq!(policy.observed.lock().unwrap().len(), calls);
        }
        {
            let mut guard = f.service.lock_selected().unwrap();
            let rich_state::StoredAuthority::Rich(state) = &mut *guard else {
                panic!()
            };
            state
                .runtime
                .as_mut()
                .unwrap()
                .handles
                .message_handles_mut()
                .begin_io(f.owner, c, h)
                .unwrap();
        }
        let e = f.effect(5, retire(c, h));
        f.seed(&e);
        let state = snapshot(&f);
        let rows = f.rows();
        assert!(f.execute(&e).is_err());
        assert_eq!(snapshot(&f), state);
        assert_eq!(f.rows(), rows);
        {
            let mut guard = f.service.lock_selected().unwrap();
            let rich_state::StoredAuthority::Rich(state) = &mut *guard else {
                panic!()
            };
            state
                .runtime
                .as_mut()
                .unwrap()
                .handles
                .message_handles_mut()
                .end_io(f.owner, c, h)
                .unwrap();
        }
        f.call(6, MqMqiRequest::Disconnect { connection: c });
        let e = f.effect(7, inquire(c, h, "a", 64, 64));
        f.seed(&e);
        let rows = f.rows();
        assert!(f.execute(&e).is_err());
        assert_eq!(f.rows(), rows);
    }
}
