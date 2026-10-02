use super::*;

#[test]
fn memory_sqlite_pending_modes_profiles_headers_and_partial_routes_do_not_mutate() {
    for sqlite in [false, true] {
        for case in 0..13 {
            let mut stored = message(2, false);
            if case == 10 {
                let MqMdValue::V2 { fields, .. } = &mut stored.descriptor else {
                    panic!()
                };
                fields.format = *b"MQHMDE  ";
            }
            if case == 11 {
                let MqMdValue::V2 { extension, .. } = &mut stored.descriptor else {
                    panic!()
                };
                extension.msg_flags = 8;
            }
            let f = FullFixture::new(sqlite, 2, false, vec![stored]);
            let c = f.connect();
            let o = f.open(c);
            let mut r = request(c, o, 2, false, 32, MqMqiUnitOfWork::NoSyncpoint, false);
            let MqMqiRequest::FullGet(g) = &mut r else {
                panic!()
            };
            match case {
                0 => g.mode = MqGetMode::BrowseFirst,
                1 => g.wait = MqWait::BoundedHostTicks(25),
                2 => {
                    g.options = MqMqiOptions::PendingStructure {
                        requested_version: Some(4),
                    }
                }
                3 => g.unit = MqMqiUnitOfWork::ExternalPending { unit: 1 },
                4 => g.descriptor = descriptor(1, false),
                5 => g.descriptor = descriptor(2, true),
                6 => {
                    let MqMdValue::V2 { extension, .. } = &mut g.descriptor else {
                        panic!()
                    };
                    extension.group_id = [1; 24];
                }
                7 => {
                    let MqMdValue::V2 { fields, .. } = &mut g.descriptor else {
                        panic!()
                    };
                    fields.format = *b"MQHRF2  ";
                }
                8 => g.unit = MqMqiUnitOfWork::Local { unit: f.unit() + 1 },
                9 => g.mode = MqGetMode::RemoveUnderCursor { cursor: 1 },
                12 => {
                    let mut state = f.service.lock_selected().unwrap();
                    let rich_state::StoredAuthority::Rich(s) = &mut *state else {
                        panic!()
                    };
                    let mut handles = s.runtime.as_mut().unwrap().handles.handles_mut();
                    let h = handles.create_message(f.owner, c).unwrap();
                    handles.begin_message_io(f.owner, c, h).unwrap();
                    g.message_handle = Some(h);
                }
                _ => {}
            }
            let e = f.effect(3, r);
            f.seed(&e);
            let rows = f.rows();
            let live = f.live();
            let audits = f.audits();
            f.clock.0.store(25, Ordering::SeqCst);
            assert!(f.execute(&e).is_err(), "case{case}");
            assert_eq!(f.rows(), rows);
            assert_eq!(f.live(), live);
            assert_eq!(f.audits(), audits);
            assert_eq!(f.depth(), 1);
        }
        let f = FullFixture::new(sqlite, 2, false, vec![message(2, false)]);
        let c = f.connect();
        let o = f.open(c);
        let e = f.effect(
            3,
            super::super::get(c, o, MqMqiUnitOfWork::NoSyncpoint, 32, MqGetMode::Remove),
        );
        f.seed(&e);
        let rows = f.rows();
        let live = f.live();
        assert_eq!(f.execute(&e), Err(HostProblem::Unsupported));
        assert_eq!(f.rows(), rows);
        assert_eq!(f.live(), live);
        let f = Fixture::new(sqlite);
        let c = f.connect();
        let o = f.open(c);
        let e = f.effect(
            3,
            request(c, o, 2, false, 32, MqMqiUnitOfWork::NoSyncpoint, false),
        );
        f.seed(&e);
        let rows = f.rows();
        assert_eq!(f.execute(&e), Err(HostProblem::Unsupported));
        assert_eq!(f.rows(), rows);
    }
}

#[test]
fn memory_sqlite_actual_read_saf_deny_error_and_replay_revocation_preserve_authority() {
    for sqlite in [false, true] {
        for mode in [1, 2, 3] {
            let f = FullFixture::new(sqlite, 1, false, vec![message(1, false)]);
            let c = f.connect();
            let o = f.open(c);
            let e = f.effect(
                3,
                request(
                    c,
                    o,
                    1,
                    false,
                    32,
                    MqMqiUnitOfWork::Local { unit: f.unit() },
                    false,
                ),
            );
            f.seed(&e);
            let rows = f.rows();
            let live = f.live();
            let audits = f.audits();
            f.saf.mode.store(mode, Ordering::SeqCst);
            assert_eq!(
                f.execute(&e),
                Err(if mode == 1 {
                    HostProblem::Unauthorized
                } else if mode == 2 {
                    HostProblem::ProviderFailure
                } else {
                    HostProblem::InfrastructureFailure
                })
            );
            assert_eq!(f.rows(), rows);
            assert_eq!(f.live(), live);
            let after = f.audits();
            assert_eq!(after.len(), audits.len() + 1);
            let a = after.last().unwrap();
            assert_eq!(
                a.decision,
                match mode {
                    1 => AuditDecision::Deny,
                    2 => AuditDecision::ProviderFailure,
                    _ => AuditDecision::InfrastructureFailure,
                }
            );
            assert_eq!(a.resource, canonical_audit_resource_digest(&e.request));
            let observation = f.saf.observations.lock().unwrap().last().unwrap().clone();
            assert_eq!(observation.1.class, EnterpriseResourceClass::MqQueue);
            assert_eq!(observation.1.name.as_str(), "Q");
            assert_eq!(observation.1.intent, AccessIntent::Read);
            f.saf.mode.store(0, Ordering::SeqCst);
            f.execute(&e).unwrap();
            let rows = f.rows();
            let live = f.live();
            let audits = f.audits();
            f.saf.mode.store(mode, Ordering::SeqCst);
            assert_eq!(
                f.execute(&e),
                Err(if mode == 1 {
                    HostProblem::Unauthorized
                } else if mode == 2 {
                    HostProblem::ProviderFailure
                } else {
                    HostProblem::InfrastructureFailure
                })
            );
            assert_eq!(f.rows(), rows);
            assert_eq!(f.live(), live);
            assert_eq!(f.audits(), audits);
        }
    }
}

#[test]
fn memory_sqlite_full_get_late_dependency_cas_has_no_partial_removal_receipt_audit_or_uow() {
    for sqlite in [false, true] {
        for namespace in [
            CATALOG_NAMESPACE,
            STATE_NAMESPACE,
            ownership::CONTROL_NAMESPACE,
            ownership::UOW_NAMESPACE,
            "mq-delivery-live-v1-meta",
        ] {
            let f = FullFixture::new(sqlite, 2, true, vec![message(2, true)]);
            let c = f.connect();
            let o = f.open(c);
            let unit = f.unit();
            let e = f.effect(
                3,
                request(c, o, 2, true, 32, MqMqiUnitOfWork::Local { unit }, false),
            );
            f.seed(&e);
            let mut winner = f
                .rows()
                .into_iter()
                .find(|r| r.namespace == namespace)
                .unwrap();
            let old = winner.version;
            winner.version += 1;
            let mut expected = f.rows();
            *expected
                .iter_mut()
                .find(|r| r.namespace == winner.namespace && r.key == winner.key)
                .unwrap() = winner.clone();
            let live = f.live();
            let audits = f.audits();
            let store = f.store.clone();
            *f.saf.hook.lock().unwrap() = Some(Box::new(move || {
                store.put_provider_state(winner, Some(old)).unwrap();
            }));
            assert!(f.execute(&e).is_err());
            assert_eq!(f.rows(), expected);
            assert_eq!(f.live(), live);
            assert_eq!(f.depth(), 1);
            assert_eq!(f.unit(), unit);
            assert_eq!(f.audits(), audits);
        }
    }
}

#[test]
fn memory_sqlite_full_get_late_controls_clock_and_audit_quota_leave_all_candidate_state_unadopted()
{
    for sqlite in [false, true] {
        for case in 0..4 {
            let f = FullFixture::new(sqlite, 2, false, vec![message(2, false)]);
            let c = f.connect();
            let o = f.open(c);
            let e = f.effect(
                3,
                request(
                    c,
                    o,
                    2,
                    false,
                    32,
                    MqMqiUnitOfWork::Local { unit: f.unit() },
                    false,
                ),
            );
            f.seed(&e);
            if case == 3 {
                super::super::bounds::fill_audits(&f.f);
            }
            let rows = f.rows();
            let live = f.live();
            let audits = f.audits();
            let clock = f.clock.clone();
            let probe = f.inv.cancellation_probe.clone().unwrap();
            if case < 3 {
                *f.saf.hook.lock().unwrap() = Some(Box::new(move || match case {
                    0 => probe.request(),
                    1 => clock.0.store(901, Ordering::SeqCst),
                    _ => clock.0.store(19, Ordering::SeqCst),
                }));
            }
            assert!(f.execute(&e).is_err());
            assert_eq!(f.rows(), rows);
            assert_eq!(f.live(), live);
            assert_eq!(f.audits(), audits);
            assert_eq!(f.depth(), 1);
        }
    }
}

#[test]
fn memory_sqlite_full_get_result_budget_and_corrupt_original_intent_fail_without_removal() {
    for sqlite in [false, true] {
        for case in 0..3 {
            let mut f = FullFixture::new(sqlite, 1, true, vec![message(1, true)]);
            let c = f.connect();
            let o = f.open(c);
            let e = f.effect(
                3,
                request(c, o, 1, true, 32, MqMqiUnitOfWork::NoSyncpoint, false),
            );
            f.seed(&e);
            if case == 0 {
                f.f.provider.max_result_bytes = 1;
            } else {
                let store = f.store.clone();
                let key = e.idempotency_key.clone().unwrap();
                let execution = f.inv.execution_id.clone();
                *f.saf.hook.lock().unwrap() = Some(Box::new(move || {
                    if case == 1 {
                        store
                            .claim_stale_intent(&key, 8, "recovery", 900, 1, 10)
                            .unwrap();
                    } else {
                        store
                            .transition_execution(&execution, 3, ExecutionState::Failed, 20)
                            .unwrap();
                    }
                }));
            }
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
