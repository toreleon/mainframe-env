use super::*;

fn count_mut(message: &mut MqFullMessage) -> &mut i32 {
    match &mut message.descriptor {
        MqMdValue::V1 { fields, .. } | MqMdValue::V2 { fields, .. } => &mut fields.backout_count,
    }
}
fn input_count(request: &mut MqMqiRequest, count: i32) {
    let MqMqiRequest::FullGet(get) = request else {
        panic!()
    };
    match &mut get.descriptor {
        MqMdValue::V1 { fields, .. } | MqMdValue::V2 { fields, .. } => fields.backout_count = count,
    }
}
fn counted(version: i32, cp: bool, count: i32) -> MqFullMessage {
    let mut m = message(version, cp);
    *count_mut(&mut m) = count;
    m
}
fn returned(reply: EffectResult) -> MqFullMessage {
    full_output(reply, "MQCC_OK", "MQRC_NONE").1.unwrap()
}
// Read actual physical rich rows through their ONE validator and query a clone.
// This is a storage observation, not a restored executable handle/admission.
fn physical_message(store: &dyn PlatformStore, version: i32, cp: bool) -> MqFullMessage {
    let state = rich(store);
    let mut delivery = state.delivery.clone();
    delivery
        .get_full(
            &state.catalog,
            &crate::MqObjectName::new("Q").unwrap(),
            QueueProfile::Complete {
                version,
                characters: if cp {
                    MqMdCharacterEncoding::OwnedCp037
                } else {
                    MqMdCharacterEncoding::AsciiCompatible
                },
            },
            &mainframe_env_host_api::MqGetContract {
                selection: Default::default(),
                mode: MqGetMode::Remove,
                wait: MqWait::NoWait,
                truncation: MqTruncation::Reject,
                buffer_capacity: 32,
            },
            None,
        )
        .unwrap()
        .1
        .unwrap()
}

#[test]
fn memory_owned_sqlite_original_local_get_back_next_get_counts_and_cached_back_once() {
    for sqlite in [false, true] {
        for version in [1, 2] {
            for cp in [false, true] {
                for (prior, expected_count) in [(0, 1), (254, 255), (255, 255)] {
                    let original = counted(version, cp, prior);
                    let f = FullFixture::new(sqlite, version, cp, vec![original.clone()]);
                    let c = f.connect();
                    let o = f.open(c);
                    let unit = f.unit();
                    let mut get = request(
                        c,
                        o,
                        version,
                        cp,
                        32,
                        MqMqiUnitOfWork::Local { unit },
                        false,
                    );
                    // q097395_1507: ignored INPUT observation must not replace
                    // the actual queued output counter, even at signed extremes.
                    input_count(&mut get, i32::MIN);
                    assert_eq!(returned(f.call(3, get)), original);
                    assert_eq!(f.depth(), 0);
                    let back = f.effect(
                        4,
                        MqMqiRequest::Back {
                            connection: c,
                            unit,
                        },
                    );
                    f.seed(&back);
                    let reply = f.execute(&back).unwrap();
                    let mut expected = original;
                    *count_mut(&mut expected) = expected_count;
                    assert_eq!(physical_message(&*f.store, version, cp), expected);
                    let rows = f.rows();
                    let live = f.live();
                    let audits = f.audits();
                    assert_eq!(f.execute(&back).unwrap(), reply);
                    assert_eq!(f.rows(), rows);
                    assert_eq!(f.live(), live);
                    assert_eq!(f.audits(), audits);
                    assert_eq!(physical_message(&*f.store, version, cp), expected);
                    assert_eq!(
                        rich(&*f.store).delivery.unit_outcome(unit),
                        MqDeliveryOutcome::Rejected
                    );
                    assert_eq!(
                        audits.last().unwrap().resource,
                        canonical_audit_resource_digest(&back.request)
                    );
                    assert_eq!(audits.last().unwrap().decision, AuditDecision::Success);
                    assert_eq!(
                        f.store
                            .effect(back.idempotency_key.as_ref().unwrap())
                            .unwrap()
                            .unwrap()
                            .state,
                        EffectState::Intent
                    );
                    let next = f.unit();
                    assert_ne!(next, unit);
                    assert_eq!(
                        returned(f.call(
                            5,
                            request(
                                c,
                                o,
                                version,
                                cp,
                                32,
                                MqMqiUnitOfWork::Local { unit: next },
                                false
                            )
                        )),
                        expected
                    );
                }
            }
        }
    }
}

#[test]
fn memory_sqlite_local_accepted_zero_short_get_back_restores_whole_message_and_increments() {
    for sqlite in [false, true] {
        for capacity in [0, 1] {
            let original = counted(2, true, 254);
            let f = FullFixture::new(sqlite, 2, true, vec![original.clone()]);
            let c = f.connect();
            let o = f.open(c);
            let unit = f.unit();
            let (_, got, length) = full_output(
                f.call(
                    3,
                    request(
                        c,
                        o,
                        2,
                        true,
                        capacity,
                        MqMqiUnitOfWork::Local { unit },
                        true,
                    ),
                ),
                "MQCC_WARNING",
                "MQRC_TRUNCATED_MSG_ACCEPTED",
            );
            let mut prefix = original.clone();
            prefix.body.truncate(capacity);
            assert_eq!(got, Some(prefix));
            assert_eq!(length, Some(5));
            assert_eq!(f.depth(), 0);
            f.call(
                4,
                MqMqiRequest::Back {
                    connection: c,
                    unit,
                },
            );
            let mut expected = original;
            *count_mut(&mut expected) = 255;
            assert_eq!(physical_message(&*f.store, 2, true), expected);
        }
    }
}

#[test]
fn memory_sqlite_commit_no_syncpoint_reject_and_partial_do_not_increment_full_counts() {
    for sqlite in [false, true] {
        for case in 0..3 {
            let original = counted(2, false, 254);
            let f = FullFixture::new(sqlite, 2, false, vec![original.clone()]);
            let c = f.connect();
            let o = f.open(c);
            let unit = f.unit();
            let reply = f.call(
                3,
                request(
                    c,
                    o,
                    2,
                    false,
                    if case == 2 { 1 } else { 32 },
                    if case == 1 {
                        MqMqiUnitOfWork::NoSyncpoint
                    } else {
                        MqMqiUnitOfWork::Local { unit }
                    },
                    false,
                ),
            );
            if case == 2 {
                let (_, got, length) =
                    full_output(reply, "MQCC_WARNING", "MQRC_TRUNCATED_MSG_FAILED");
                let mut prefix = original.clone();
                prefix.body.truncate(1);
                assert_eq!(got, Some(prefix));
                assert_eq!(length, Some(5));
            } else {
                assert_eq!(returned(reply), original);
            }
            let rich_before = rich(&*f.store);
            let receipt_before = f
                .rows()
                .into_iter()
                .find(|r| r.namespace == receipt::NAMESPACE && r.key == "effect-3")
                .unwrap();
            f.call(
                4,
                if case == 0 {
                    MqMqiRequest::Commit {
                        connection: c,
                        unit,
                    }
                } else {
                    MqMqiRequest::Back {
                        connection: c,
                        unit,
                    }
                },
            );
            if case == 2 {
                assert_eq!(physical_message(&*f.store, 2, false), original);
            } else {
                assert_eq!(f.depth(), 0);
            }
            assert_eq!(
                f.rows()
                    .into_iter()
                    .find(|r| r.namespace == receipt::NAMESPACE && r.key == "effect-3")
                    .unwrap(),
                receipt_before
            );
            assert!(rich_before.receipts.contains_key("effect-3"));
        }
        let f = Fixture::new(sqlite);
        let c = f.connect();
        let o = f.open(c);
        let unit = f.unit();
        f.call(
            3,
            MqMqiRequest::Put {
                connection: c,
                object: o,
                put: super::super::put(MqMqiUnitOfWork::NoSyncpoint),
            },
        );
        let original = f.call(
            4,
            super::super::get(c, o, MqMqiUnitOfWork::Local { unit }, 32, MqGetMode::Remove),
        );
        f.call(
            5,
            MqMqiRequest::Back {
                connection: c,
                unit,
            },
        );
        let restored = f.call(
            6,
            super::super::get(c, o, MqMqiUnitOfWork::NoSyncpoint, 32, MqGetMode::Remove),
        );
        // Compare exact historical partial output, not a synthesized full MD.
        assert_eq!(output(original), output(restored));
    }
}

#[test]
fn memory_sqlite_stored_invalid_counts_and_structured_properties_refuse_without_adoption() {
    for sqlite in [false, true] {
        for version in [1, 2] {
            for case in 0..5 {
                let mut m = counted(
                    version,
                    false,
                    match case {
                        0 => -1,
                        1 => i32::MIN,
                        2 => 256,
                        3 => i32::MAX,
                        _ => 0,
                    },
                );
                if case == 4 {
                    m.properties = vec![
                        MqMessageProperty {
                            name: "usr.first".into(),
                            kind: MqPropertyType::ByteString,
                            value: vec![0, 255, 0],
                        },
                        MqMessageProperty {
                            name: "usr.second".into(),
                            kind: MqPropertyType::Int32,
                            value: (-17i32).to_be_bytes().to_vec(),
                        },
                    ];
                }
                let f = FullFixture::new(sqlite, version, false, vec![m.clone()]);
                let c = f.connect();
                let o = f.open(c);
                let unit = f.unit();
                let e = f.effect(
                    3,
                    request(
                        c,
                        o,
                        version,
                        false,
                        1,
                        MqMqiUnitOfWork::Local { unit },
                        true,
                    ),
                );
                f.seed(&e);
                let rows = f.rows();
                let live = f.live();
                let audits = f.audits();
                f.clock.0.store(25, Ordering::SeqCst);
                assert_eq!(f.execute(&e), Err(HostProblem::Unsupported));
                assert_eq!(f.rows(), rows);
                assert_eq!(f.live(), live);
                assert_eq!(f.audits(), audits);
                assert_eq!(f.unit(), unit);
                assert_eq!(physical_message(&*f.store, version, false), m);
            }
        }
    }
}

#[test]
fn memory_sqlite_live_back_late_cas_controls_audit_quota_never_adopt_increment() {
    for sqlite in [false, true] {
        for case in 0..9 {
            let f = FullFixture::new(sqlite, 2, true, vec![counted(2, true, 254)]);
            let c = f.connect();
            let o = f.open(c);
            let unit = f.unit();
            f.call(
                3,
                request(c, o, 2, true, 32, MqMqiUnitOfWork::Local { unit }, false),
            );
            let e = f.effect(
                4,
                MqMqiRequest::Back {
                    connection: c,
                    unit,
                },
            );
            f.seed(&e);
            if case == 8 {
                super::super::bounds::fill_audits(&f.f);
            }
            let mut expected = f.rows();
            let live = f.live();
            let audits = f.audits();
            if case < 5 {
                let namespace = [
                    CATALOG_NAMESPACE,
                    STATE_NAMESPACE,
                    ownership::CONTROL_NAMESPACE,
                    ownership::UOW_NAMESPACE,
                    "mq-delivery-live-v1-meta",
                ][case];
                let mut winner = expected
                    .iter()
                    .find(|r| r.namespace == namespace)
                    .unwrap()
                    .clone();
                let old = winner.version;
                winner.version += 1;
                *expected
                    .iter_mut()
                    .find(|r| r.namespace == winner.namespace && r.key == winner.key)
                    .unwrap() = winner.clone();
                let store = f.store.clone();
                *f.saf.hook.lock().unwrap() = Some(Box::new(move || {
                    store.put_provider_state(winner, Some(old)).unwrap();
                }));
            } else if case < 8 {
                let clock = f.clock.clone();
                let probe = f.inv.cancellation_probe.clone().unwrap();
                *f.saf.hook.lock().unwrap() = Some(Box::new(move || match case {
                    5 => probe.request(),
                    6 => clock.0.store(901, Ordering::SeqCst),
                    _ => clock.0.store(19, Ordering::SeqCst),
                }));
            }
            assert!(f.execute(&e).is_err());
            assert_eq!(f.rows(), expected);
            assert_eq!(f.live(), live);
            assert_eq!(f.audits(), audits);
            let state = f.service.lock_selected().unwrap();
            let rich_state::StoredAuthority::Rich(s) = &*state else {
                panic!()
            };
            assert_eq!(s.delivery.unit_outcome(unit), MqDeliveryOutcome::Pending);
            assert_eq!(s.runtime.as_ref().unwrap().connections[0].unit, unit);
        }
    }
}

#[test]
fn memory_sqlite_back_postpersist_unknown_retains_increment_receipt_and_never_recounts() {
    for sqlite in [false, true] {
        let f = FullFixture::new(sqlite, 2, false, vec![counted(2, false, 0)]);
        let c = f.connect();
        let o = f.open(c);
        let unit = f.unit();
        f.call(
            3,
            request(c, o, 2, false, 32, MqMqiUnitOfWork::Local { unit }, false),
        );
        let e = f.effect(
            4,
            MqMqiRequest::Back {
                connection: c,
                unit,
            },
        );
        f.seed(&e);
        f.service
            .unknown_after_persist
            .store(true, Ordering::SeqCst);
        assert_eq!(f.execute(&e), Err(HostProblem::UnknownOutcome));
        assert_eq!(physical_message(&*f.store, 2, false), counted(2, false, 1));
        let rows = f.rows();
        let live = f.live();
        let audits = f.audits();
        let calls = f.saf.observations.lock().unwrap().len();
        assert_eq!(f.execute(&e), Err(HostProblem::UnknownOutcome));
        assert_eq!(f.rows(), rows);
        assert_eq!(f.live(), live);
        assert_eq!(f.audits(), audits);
        assert_eq!(f.saf.observations.lock().unwrap().len(), calls);
        let state = rich(&*f.store);
        assert_eq!(
            state.delivery.unit_outcome(unit),
            MqDeliveryOutcome::Rejected
        );
        assert_eq!(
            output(
                state.receipts["effect-4"]
                    .replay(HostLimits::default(), Default::default())
                    .unwrap()
            ),
            MqMqiOutput::UnitOfWork { unit }
        );
        assert_eq!(
            audits.last().unwrap().resource,
            canonical_audit_resource_digest(&e.request)
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
fn owned_sqlite_committed_live_back_counter_survives_close_reopen_without_handle_resurrection() {
    let f = FullFixture::new(true, 1, true, vec![counted(1, true, 254)]);
    let c = f.connect();
    let o = f.open(c);
    let unit = f.unit();
    f.call(
        3,
        request(c, o, 1, true, 32, MqMqiUnitOfWork::Local { unit }, false),
    );
    let e = f.effect(
        4,
        MqMqiRequest::Back {
            connection: c,
            unit,
        },
    );
    f.seed(&e);
    let reply = f.execute(&e).unwrap();
    let rows = f.rows();
    let audits = f.audits();
    let inv = f.inv.clone();
    let provider = f.provider.clone();
    let old_frame = f.frame;
    let saf = f.saf.clone();
    let clock = f.clock.clone();
    let FullFixture { f, db, saf: _ } = f;
    drop(f);
    let db = db.unwrap();
    let store = db.open();
    assert_eq!(store.list_provider_state_prefix("mq-", 4096).unwrap(), rows);
    assert_eq!(
        store.audit_records(&inv.execution_id, 0, 256).unwrap(),
        audits
    );
    assert_eq!(physical_message(&*store, 1, true), counted(1, true, 255));
    assert_eq!(
        rich(&*store).receipts["effect-4"]
            .replay(HostLimits::default(), Default::default())
            .unwrap(),
        reply
    );
    let service =
        MqService::open_selected_mqi(store.clone(), MqLimits::default(), 3, 5, saf, clock).unwrap();
    let process = service.mint_selected_process(&inv).unwrap();
    let (fresh, _) = service.bind_selected_root(process, &inv).unwrap();
    for frame in [old_frame, fresh] {
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
    drop(service);
    drop(store);
    drop(db);
}
