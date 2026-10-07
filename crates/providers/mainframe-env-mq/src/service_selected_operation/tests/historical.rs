use super::*;

#[test]
fn memory_sqlite_replay_refuses_time_before_original_publication_without_saf_or_writes() {
    for sqlite in [false, true] {
        let f = Fixture::new(sqlite);
        let e = f.effect(
            1,
            MqMqiRequest::Connect(MqMqiConnect {
                manager: None,
                sharing: MqHandleSharing::NonShared,
                options: MqMqiOptions::ContractDefault,
            }),
        );
        f.seed(&e);
        f.execute(&e).unwrap();
        let rows = f.rows();
        let audits = f.store.audit_records(&f.inv.execution_id, 0, 128).unwrap();
        let saf = f.saf.calls.load(Ordering::SeqCst);
        f.clock.0.store(19, Ordering::SeqCst);
        assert_eq!(f.execute(&e), Err(HostProblem::Malformed));
        assert_eq!(f.rows(), rows);
        assert_eq!(
            f.store.audit_records(&f.inv.execution_id, 0, 128).unwrap(),
            audits
        );
        assert_eq!(f.saf.calls.load(Ordering::SeqCst), saf);
    }
}

#[test]
fn memory_sqlite_reviewed_get_complete_and_accepted_truncation_replay_losslessly() {
    for sqlite in [false, true] {
        for (mode, capacity, truncated) in [
            (MqGetMode::Remove, 3, false),
            (MqGetMode::Remove, 1, true),
            (MqGetMode::BrowseFirst, 1, true),
        ] {
            let f = Fixture::new(sqlite);
            let c = f.connect();
            let o = f.open(c);
            f.call(
                3,
                MqMqiRequest::Put {
                    connection: c,
                    object: o,
                    put: put(MqMqiUnitOfWork::NoSyncpoint),
                },
            );
            let mut request = get(c, o, MqMqiUnitOfWork::NoSyncpoint, capacity, mode);
            let MqMqiRequest::Get(get) = &mut request else {
                panic!()
            };
            get.get.truncation = MqTruncation::Accept;
            let result = f.call(4, request); // Exact occurrence replay is checked by call.
            let Ok(HostResult::MqMqi(result)) = result.outcome else {
                panic!()
            };
            let MqMqiOutcome::ReviewedOutput {
                status,
                output:
                    MqMqiOutput::Got {
                        disposition,
                        message: Some(message),
                        cursor,
                    },
            } = result.result.outcome
            else {
                panic!("lossless reviewed reply")
            };
            assert_eq!(message.body, vec![0, 255, 41][..capacity]);
            assert_eq!(message.descriptor, super::message().descriptor);
            assert_eq!(
                status.completion().symbol(),
                if truncated { "MQCC_WARNING" } else { "MQCC_OK" }
            );
            assert_eq!(
                status.reason_symbol(),
                if truncated {
                    "MQRC_TRUNCATED_MSG_ACCEPTED"
                } else {
                    "MQRC_NONE"
                }
            );
            match (mode, truncated) {
                (MqGetMode::BrowseFirst, true) => {
                    assert_eq!(
                        disposition,
                        MqGetDisposition::Message(MqTruncationDisposition::AcceptedBrowsed {
                            required: 3,
                            copied: 1
                        })
                    );
                    assert!(cursor.is_some());
                    assert_eq!(f.depth(), 1);
                }
                (MqGetMode::Remove, true) => {
                    assert_eq!(
                        disposition,
                        MqGetDisposition::Message(MqTruncationDisposition::AcceptedRemoved {
                            required: 3,
                            copied: 1
                        })
                    );
                    assert!(cursor.is_none());
                    assert_eq!(f.depth(), 0);
                }
                (MqGetMode::Remove, false) => {
                    assert_eq!(
                        disposition,
                        MqGetDisposition::Message(MqTruncationDisposition::Complete { length: 3 })
                    );
                    assert!(cursor.is_none());
                    assert_eq!(f.depth(), 0);
                }
                _ => panic!(),
            }
        }
    }
}

#[test]
fn memory_sqlite_browse_cursor_and_no_message_observations_have_exact_occurrence_replay() {
    for sqlite in [false, true] {
        let f = Fixture::new(sqlite);
        let c = f.connect();
        let o = f.open(c);
        f.call(
            3,
            MqMqiRequest::Put {
                connection: c,
                object: o,
                put: put(MqMqiUnitOfWork::NoSyncpoint),
            },
        );
        let mut second = put(MqMqiUnitOfWork::NoSyncpoint);
        second.message.body = vec![19];
        f.call(
            4,
            MqMqiRequest::PutOne {
                connection: c,
                lookup: lookup(),
                alternate_user: None,
                put: second,
            },
        );
        let first = output(f.call(
            5,
            get(
                c,
                o,
                MqMqiUnitOfWork::NoSyncpoint,
                1024,
                MqGetMode::BrowseFirst,
            ),
        ));
        let MqMqiOutput::Got {
            message: Some(m),
            cursor: Some(cursor),
            ..
        } = first
        else {
            panic!()
        };
        assert_eq!(m.body, message().body);
        let next = output(f.call(
            6,
            get(
                c,
                o,
                MqMqiUnitOfWork::NoSyncpoint,
                1024,
                MqGetMode::BrowseNext { cursor },
            ),
        ));
        let MqMqiOutput::Got {
            message: Some(m), ..
        } = next
        else {
            panic!()
        };
        assert_eq!(m.body, vec![19]);
        let last = f.call(
            7,
            get(
                c,
                o,
                MqMqiUnitOfWork::NoSyncpoint,
                1024,
                MqGetMode::BrowseNext { cursor },
            ),
        );
        let HostResult::MqMqi(last) = last.outcome.unwrap() else {
            panic!()
        };
        assert!(matches!(
            last.result.outcome,
            MqMqiOutcome::ReviewedOutput {
                status,
                output: MqMqiOutput::Got {
                    disposition: MqGetDisposition::NoMessage,
                    message: None,
                    ..
                }
            } if status.completion().symbol() == "MQCC_FAILED"
                && status.reason_symbol() == "MQRC_NO_MSG_AVAILABLE"
        ));
        assert_eq!(f.depth(), 2);
    }
}

#[test]
fn memory_sqlite_cold_incarnation_exhaustion_never_wraps_or_writes_authority() {
    for sqlite in [false, true] {
        let f = Fixture::new(sqlite);
        f.connect();
        let mut record = f
            .store
            .get_provider_state(ownership::CONTROL_NAMESPACE, ownership::CONTROL_KEY)
            .unwrap()
            .unwrap();
        let old = record.version;
        record.version += 1;
        let mut value: serde_json::Value = serde_json::from_slice(&record.payload).unwrap();
        value["value"]["registry_epoch"] = serde_json::json!(i64::MAX as u64);
        record.payload = serde_json::to_vec(&value).unwrap();
        f.store.put_provider_state(record, Some(old)).unwrap();
        let before = f.rows();
        let reopened = MqService::open_selected_mqi(
            f.store.clone(),
            MqLimits::default(),
            3,
            5,
            f.saf.clone(),
            f.clock.clone(),
        )
        .unwrap();
        assert_eq!(
            reopened.mint_selected_process(&f.inv).err(),
            Some(HostProblem::ResourceExhausted)
        );
        assert_eq!(f.rows(), before);
    }
}

#[test]
fn memory_sqlite_historical_decode_has_no_command_authority_and_live_replay_allocates_nothing() {
    for sqlite in [false, true] {
        let f = Fixture::new(sqlite);
        let c = f.connect();
        let o = f.open(c);
        let before = f.rows();
        let audits = f.store.audit_records(&f.inv.execution_id, 0, 128).unwrap();
        let state = f.service.lock_selected().unwrap();
        let rich_state::StoredAuthority::Rich(s) = &*state else {
            panic!()
        };
        let historical_c = output(
            s.receipts["effect-1"]
                .replay(HostLimits::default(), MqMqiLimits::default())
                .unwrap(),
        );
        let historical_o = output(
            s.receipts["effect-2"]
                .replay(HostLimits::default(), MqMqiLimits::default())
                .unwrap(),
        );
        drop(state);
        let MqMqiOutput::Connected(hc) = historical_c else {
            panic!()
        };
        let MqMqiOutput::Opened {
            object: ho,
            dynamic: None,
        } = historical_o
        else {
            panic!()
        };
        assert!(hc.is_historical());
        assert!(MqHandle::Object(ho).is_historical());
        assert_ne!(hc, c);
        assert_ne!(ho, o);
        let mut state = f.service.lock_selected().unwrap();
        let rich_state::StoredAuthority::Rich(s) = &mut *state else {
            panic!()
        };
        let runtime = s.runtime.as_mut().unwrap();
        assert!(
            runtime
                .handles
                .handles_mut()
                .validate_connection(f.owner, hc)
                .is_err()
        );
        assert!(
            runtime
                .handles
                .handles_mut()
                .validate(f.owner, c, MqHandle::Object(ho), MqHandleKind::Object)
                .is_err()
        );
        assert_eq!(runtime.connections.len(), 1);
        assert_eq!(runtime.objects.len(), 1);
        drop(state);
        let e = f.effect(
            1,
            MqMqiRequest::Connect(MqMqiConnect {
                manager: None,
                sharing: MqHandleSharing::NonShared,
                options: MqMqiOptions::ContractDefault,
            }),
        );
        assert!(
            matches!(output(f.execute(&e).unwrap()), MqMqiOutput::Connected(live) if live == c && !live.is_historical())
        );
        assert_eq!(f.rows(), before);
        assert_eq!(
            f.store.audit_records(&f.inv.execution_id, 0, 128).unwrap(),
            audits
        );
        f.call(
            3,
            MqMqiRequest::Close(
                MqObjectCloseRequest::new(
                    c,
                    MqRouteCloseTarget::Object {
                        handle: o,
                        lifecycle: MqRouteCloseLifecycle::Predefined,
                    },
                    MqRouteCloseMode::None,
                )
                .unwrap(),
            ),
        );
        let e = f.effect(
            2,
            MqMqiRequest::Open(
                MqObjectOpenRequest::new(
                    c,
                    lookup(),
                    &[
                        MqRouteOpenAccess::InputShared,
                        MqRouteOpenAccess::Output,
                        MqRouteOpenAccess::Browse,
                    ],
                    Default::default(),
                )
                .unwrap(),
            ),
        );
        let rows = f.rows();
        assert_eq!(f.execute(&e), Err(HostProblem::UnknownOutcome));
        assert_eq!(f.rows(), rows);
    }
}

#[test]
fn memory_sqlite_independent_incarnation_and_uow_version_conflicts_roll_back_no_syncpoint_put() {
    for sqlite in [false, true] {
        for owner_row in [false, true] {
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
            let namespace = if owner_row {
                ownership::UOW_NAMESPACE
            } else {
                ownership::CONTROL_NAMESPACE
            };
            let key = if owner_row {
                "1"
            } else {
                ownership::CONTROL_KEY
            };
            let mut record = f.store.get_provider_state(namespace, key).unwrap().unwrap();
            let version = record.version;
            record.version += 1;
            f.store.put_provider_state(record, Some(version)).unwrap();
            let rows = f.rows();
            let epoch = f.store.provider_state_retention_epoch().unwrap();
            let audits = f.store.audit_records(&f.inv.execution_id, 0, 128).unwrap();
            assert!(f.execute(&e).is_err());
            assert_eq!(f.depth(), 0);
            assert_eq!(f.rows(), rows);
            assert_eq!(f.store.provider_state_retention_epoch().unwrap(), epoch);
            assert_eq!(
                f.store.audit_records(&f.inv.execution_id, 0, 128).unwrap(),
                audits
            );
        }
    }
}
