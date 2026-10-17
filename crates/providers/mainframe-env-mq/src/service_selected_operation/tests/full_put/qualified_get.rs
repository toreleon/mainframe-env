//! Actual selected producer/GET/core/store fixtures, not installed/JES/LE/RACF.
use super::*;
fn request(
    c: MqHconn,
    o: MqHobj,
    version: i32,
    cp: bool,
    capacity: usize,
    truncate: MqTruncation,
    unit: MqMqiUnitOfWork,
) -> MqMqiRequest {
    let mut descriptor = message(version, cp, -77).descriptor;
    match &mut descriptor {
        MqMdValue::V1 { fields, .. } | MqMdValue::V2 { fields, .. } => {
            fields.msg_id = [0; 24];
            fields.correl_id = [0; 24];
        }
    }
    MqMqiRequest::QualifiedFullGet(MqMqiFullGet {
        connection: c,
        object: o,
        descriptor,
        mode: MqGetMode::Remove,
        wait: MqWait::NoWait,
        truncation: truncate,
        buffer_capacity: capacity,
        message_handle: None,
        options: MqMqiOptions::ContractDefault,
        unit,
    })
}
fn observation(r: &EffectResult) -> (&MqMqiQualifiedGot, &MqReviewedStatus) {
    let Ok(HostResult::MqMqi(r)) = &r.outcome else {
        panic!("{r:?}")
    };
    let MqMqiOutcome::ReviewedOutput {
        status,
        output: MqMqiOutput::QualifiedFullGot(v),
    } = &r.result.outcome
    else {
        panic!()
    };
    (v, status)
}
fn live(f: &ProducerFixture) -> Vec<u8> {
    let g = f.service.lock_selected().unwrap();
    let rich_state::StoredAuthority::Rich(s) = &*g else {
        panic!()
    };
    s.delivery.encode_live_checkpoint().unwrap()
}
#[test]
fn qualified_actual_put_get_profiles_prefix_qname_context_free_exact_replay() {
    for sqlite in [false, true] {
        for version in [1, 2] {
            for cp in [false, true] {
                for (cap, truncate) in [
                    (0, MqTruncation::Reject),
                    (0, MqTruncation::Accept),
                    (2, MqTruncation::Reject),
                    (2, MqTruncation::Accept),
                    (5, MqTruncation::Reject),
                ] {
                    let f = ProducerFixture::new(sqlite, version, cp);
                    let c = f.connect();
                    let o = f.open(c);
                    let input = message(version, cp, i32::MIN);
                    f.call(
                        3,
                        put(
                            c,
                            Some(o),
                            input.clone(),
                            true,
                            MqMqiUnitOfWork::NoSyncpoint,
                        ),
                    );
                    let e = f.effect(
                        4,
                        request(
                            c,
                            o,
                            version,
                            cp,
                            cap,
                            truncate,
                            MqMqiUnitOfWork::NoSyncpoint,
                        ),
                    );
                    f.seed(&e);
                    let before = f.ports.encoder_calls.load(Ordering::SeqCst);
                    let r = f.execute(&e).unwrap();
                    let (v, status) = observation(&r);
                    assert_eq!(v.characters, input.descriptor.characters());
                    assert_eq!(v.data_length, Some(5));
                    assert_eq!(v.cursor, None);
                    let m = v.message.as_ref().unwrap();
                    assert_eq!(m.body, &input.body[..cap.min(5)]);
                    assert_eq!(m.descriptor.version(), version);
                    assert_eq!(m.descriptor.fields().backout_count, 0);
                    let removed = cap >= 5 || truncate == MqTruncation::Accept;
                    if removed {
                        let mut expected = [if cp { 0x40 } else { b' ' }; 48];
                        expected[0] = if cp { 0xd8 } else { b'Q' };
                        assert_eq!(v.resolved_queue, Some(expected));
                        assert_eq!(f.ports.encoder_calls.load(Ordering::SeqCst), before + 1);
                    } else {
                        assert_eq!(v.resolved_queue, None);
                        assert_eq!(f.ports.encoder_calls.load(Ordering::SeqCst), before);
                    }
                    assert_eq!(
                        status.reason_symbol(),
                        if cap >= 5 {
                            "MQRC_NONE"
                        } else if removed {
                            "MQRC_TRUNCATED_MSG_ACCEPTED"
                        } else {
                            "MQRC_TRUNCATED_MSG_FAILED"
                        }
                    );
                    let rows = f.rows();
                    let calls = f.ports.encoder_calls.load(Ordering::SeqCst);
                    assert_eq!(f.execute(&e).unwrap(), r);
                    assert_eq!(f.rows(), rows);
                    assert_eq!(f.ports.encoder_calls.load(Ordering::SeqCst), calls);
                    assert_eq!(f.ports.gmt_calls.load(Ordering::SeqCst), 0);
                    assert_eq!(f.ports.context_calls.load(Ordering::SeqCst), 0);
                    let bytes = rows
                        .iter()
                        .find(|r| {
                            r.namespace == receipt::NAMESPACE
                                && r.key == e.idempotency_key.as_ref().unwrap().as_str()
                        })
                        .unwrap();
                    let row: serde_json::Value = serde_json::from_slice(&bytes.payload).unwrap();
                    let payload: Vec<u8> =
                        serde_json::from_value(row["value"]["reply"]["bytes"].clone()).unwrap();
                    assert!(
                        String::from_utf8_lossy(&payload)
                            .contains("mainframe-env.mq-mqi-result-storage@6")
                    );
                    assert_eq!(f.get(c, o, 5, version, cp).is_some(), !removed);
                }
            }
        }
    }
}
#[test]
fn qualified_no_message_and_current_local_backout_commit_preserve_owners() {
    for sqlite in [false, true] {
        let f = ProducerFixture::new(sqlite, 2, false);
        let c = f.connect();
        let o = f.open(c);
        let no = f.call(
            3,
            request(
                c,
                o,
                2,
                false,
                0,
                MqTruncation::Reject,
                MqMqiUnitOfWork::NoSyncpoint,
            ),
        );
        let (v, status) = observation(&no);
        assert_eq!(status.reason_symbol(), "MQRC_NO_MSG_AVAILABLE");
        assert_eq!(v.resolved_queue, None);
        assert_eq!(v.message, None);
        assert_eq!(v.data_length, None);
        f.call(
            4,
            put(
                c,
                None,
                message(2, false, 0),
                true,
                MqMqiUnitOfWork::NoSyncpoint,
            ),
        );
        let unit = f.unit();
        let e = f.effect(
            5,
            request(
                c,
                o,
                2,
                false,
                5,
                MqTruncation::Reject,
                MqMqiUnitOfWork::Local { unit },
            ),
        );
        f.seed(&e);
        let r = f.execute(&e).unwrap();
        assert!(observation(&r).0.resolved_queue.is_some());
        assert_eq!(f.execute(&e).unwrap(), r);
        f.call(
            6,
            MqMqiRequest::Back {
                connection: c,
                unit,
            },
        );
        let unit = f.unit();
        let r = f.call(
            7,
            request(
                c,
                o,
                2,
                false,
                5,
                MqTruncation::Reject,
                MqMqiUnitOfWork::Local { unit },
            ),
        );
        assert_eq!(
            observation(&r)
                .0
                .message
                .as_ref()
                .unwrap()
                .descriptor
                .fields()
                .backout_count,
            1
        );
        f.call(
            8,
            MqMqiRequest::Commit {
                connection: c,
                unit,
            },
        );
        assert_eq!(f.get(c, o, 9, 2, false), None);
        // A stale Local replay fences; it cannot precede subsequent calls.
        let rows = f.rows();
        assert!(f.execute(&e).is_err());
        assert_eq!(f.rows(), rows);
    }
}
#[test]
fn qualified_encoder_error_panic_bad_width_reentry_and_last_cas_are_atomic() {
    for sqlite in [false, true] {
        for case in 0..8 {
            let f = ProducerFixture::new(sqlite, 1, false);
            let c = f.connect();
            let o = f.open(c);
            f.call(
                3,
                put(
                    c,
                    None,
                    message(1, false, 0),
                    true,
                    MqMqiUnitOfWork::NoSyncpoint,
                ),
            );
            let unit = f.unit();
            let e = f.effect(
                4,
                request(
                    c,
                    o,
                    1,
                    false,
                    5,
                    MqTruncation::Reject,
                    MqMqiUnitOfWork::Local { unit },
                ),
            );
            f.seed(&e);
            let before = live(&f);
            let baseline = f.rows();
            if case < 3 {
                f.ports.encoder_mode.store(case + 1, Ordering::SeqCst);
            } else {
                if case == 3 {
                    f.ports.encoder_mode.store(4, Ordering::SeqCst);
                }
                let store = f.store.clone();
                let clock = f.clock.clone();
                let probe = f.inv.cancellation_probe.clone().unwrap();
                let weak = Arc::downgrade(&f.service);
                *f.ports.encoder_hook.lock().unwrap() = Some(Box::new(move || match case {
                    3 => {
                        assert!(matches!(
                            weak.upgrade().unwrap().lock_selected(),
                            Err(HostProblem::Unsupported)
                        ));
                    }
                    4 | 5 => {
                        let (ns, key) = if case == 4 {
                            (CATALOG_NAMESPACE, CATALOG_KEY)
                        } else {
                            (ownership::CONTROL_NAMESPACE, ownership::CONTROL_KEY)
                        };
                        let mut row = store.get_provider_state(ns, key).unwrap().unwrap();
                        let old = row.version;
                        row.version += 1;
                        store.put_provider_state(row, Some(old)).unwrap();
                    }
                    6 => probe.request(),
                    _ => clock.0.store(901, Ordering::SeqCst),
                }));
            }
            let result = f.execute(&e);
            assert!(result.is_err(), "case{case}");
            assert_eq!(live(&f), before);
            if !(4..6).contains(&case) {
                assert_eq!(f.rows(), baseline);
            } else {
                let after = f.rows();
                assert_eq!(after.len(), baseline.len());
                let ns = if case == 4 {
                    CATALOG_NAMESPACE
                } else {
                    ownership::CONTROL_NAMESPACE
                };
                for old in &baseline {
                    let new = after
                        .iter()
                        .find(|r| r.namespace == old.namespace && r.key == old.key)
                        .unwrap();
                    if old.namespace == ns {
                        assert_eq!(new.version, old.version + 1);
                        assert_eq!(new.payload, old.payload);
                    } else {
                        assert_eq!(new, old);
                    }
                }
            }
            assert_eq!(f.ports.gmt_calls.load(Ordering::SeqCst), 0);
            assert_eq!(f.ports.context_calls.load(Ordering::SeqCst), 0);
        }
    }
}
#[test]
fn qualified_postcommit_unknown_and_owned_sqlite_reopen_never_reconstruct_authority() {
    for sqlite in [false, true] {
        let mut f = ProducerFixture::new(sqlite, 2, false);
        let c = f.connect();
        let o = f.open(c);
        f.call(
            3,
            put(
                c,
                None,
                message(2, false, 0),
                true,
                MqMqiUnitOfWork::NoSyncpoint,
            ),
        );
        let e = f.effect(
            4,
            request(
                c,
                o,
                2,
                false,
                5,
                MqTruncation::Reject,
                MqMqiUnitOfWork::NoSyncpoint,
            ),
        );
        f.seed(&e);
        f.service
            .unknown_after_persist
            .store(true, Ordering::SeqCst);
        assert_eq!(f.execute(&e), Err(HostProblem::UnknownOutcome));
        let rows = f.rows();
        let calls = f.ports.encoder_calls.load(Ordering::SeqCst);
        assert!(rows.iter().any(|r| r.namespace == receipt::NAMESPACE
            && r.key == e.idempotency_key.as_ref().unwrap().as_str()));
        assert!(f.execute(&e).is_err());
        assert_eq!(f.rows(), rows);
        assert_eq!(f.ports.encoder_calls.load(Ordering::SeqCst), calls);
        if sqlite {
            let db = f.db.take().unwrap();
            let (frame, inv, provider, saf, clock) = (
                f.frame,
                f.inv.clone(),
                f.provider.clone(),
                f.saf.clone(),
                f.clock.clone(),
            );
            drop(f);
            let store = db.open(256);
            assert_eq!(store.list_provider_state_prefix("mq-", 4096).unwrap(), rows);
            let reopened =
                MqService::open_selected_mqi(store.clone(), MqLimits::default(), 3, 5, saf, clock)
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
            assert_eq!(store.list_provider_state_prefix("mq-", 4096).unwrap(), rows);
            drop(reopened);
            drop(store);
            drop(db);
        }
    }
}

#[test]
fn qualified_real_saf_audit_capacity_original_and_profile_refusals_leave_delivery_unchanged() {
    for sqlite in [false, true] {
        for case in 0..8 {
            let f = ProducerFixture::new(sqlite, 1, false);
            let c = f.connect();
            let o = f.open(c);
            f.call(
                3,
                put(
                    c,
                    None,
                    message(1, false, 0),
                    true,
                    MqMqiUnitOfWork::NoSyncpoint,
                ),
            );
            let mut r = request(
                c,
                o,
                1,
                false,
                5,
                MqTruncation::Reject,
                MqMqiUnitOfWork::NoSyncpoint,
            );
            match case {
                0 => f.saf.deny.store(true, Ordering::SeqCst),
                1 => f.saf.error.store(1, Ordering::SeqCst),
                2 => f.saf.error.store(2, Ordering::SeqCst),
                4..=6 => {
                    let MqMqiRequest::QualifiedFullGet(g) = &mut r else {
                        panic!()
                    };
                    if case == 4 {
                        g.descriptor = message(2, false, 0).descriptor;
                    } else if case == 5 {
                        g.descriptor = message(1, true, 0).descriptor;
                    } else {
                        g.unit = MqMqiUnitOfWork::Local { unit: f.unit() + 1 };
                    }
                }
                _ => {}
            }
            let e = f.effect(4, r);
            f.seed(&e);
            if case == 3 {
                super::super::bounds::fill_audits(&f.f);
            }
            let rows = f.rows();
            let delivery = live(&f);
            let names = f.ports.encoder_calls.load(Ordering::SeqCst);
            let result = if case == 7 {
                let mut inv = f.inv.clone();
                inv.deadline_tick -= 1;
                f.service.execute_selected_mqi(
                    f.frame,
                    &inv,
                    e.mq_mqi_occurrence(HostLimits::default()).unwrap().unwrap(),
                    &f.provider,
                    HostLimits::default(),
                )
            } else {
                f.execute(&e)
            };
            assert!(result.is_err(), "case{case}");
            assert_eq!(f.rows(), rows);
            assert_eq!(live(&f), delivery);
            if case != 3 {
                assert_eq!(f.ports.encoder_calls.load(Ordering::SeqCst), names);
            }
            if case <= 2 {
                assert!(
                    f.saf
                        .observations
                        .lock()
                        .unwrap()
                        .iter()
                        .any(|(_, r)| r.class == EnterpriseResourceClass::MqQueue
                            && r.intent == AccessIntent::Read)
                );
                let audits = f.store.audit_records(&f.inv.execution_id, 0, 256).unwrap();
                assert_eq!(
                    audits.last().unwrap().decision,
                    match case {
                        0 => AuditDecision::Deny,
                        1 => AuditDecision::ProviderFailure,
                        _ => AuditDecision::InfrastructureFailure,
                    }
                );
            }
        }
    }
}
