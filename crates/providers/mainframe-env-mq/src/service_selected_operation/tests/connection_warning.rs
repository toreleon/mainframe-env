//! Independent source expectations; real physical fixtures, no public-route credit.
use super::*;

#[path = "connection_warning/publication.rs"]
mod publication;
#[path = "connection_warning/replay.rs"]
mod replay;

fn request(extended: bool) -> MqMqiRequest {
    let connect = MqMqiConnect {
        manager: None,
        sharing: MqHandleSharing::NonShared,
        options: MqMqiOptions::ContractDefault,
    };
    if extended {
        MqMqiRequest::ConnectExtended(connect)
    } else {
        MqMqiRequest::Connect(connect)
    }
}

pub(super) fn warning(reply: EffectResult, expected: MqHconn, call: MqMqiCall) {
    let Ok(HostResult::MqMqi(result)) = reply.outcome else {
        panic!("MQ reply")
    };
    assert_eq!(result.result.call, call);
    let MqMqiOutcome::ReviewedOutput {
        status,
        output: MqMqiOutput::Connected(connection),
    } = result.result.outcome
    else {
        panic!("source reviewed warning with defined Hconn")
    };
    // q101760_ usage 271 and q101770_ 76–78, independent of transition output.
    assert_eq!(status.wire_pair(), (1, 2002));
    assert_eq!(connection, expected);
}

fn delivery(f: &Fixture) -> Vec<u8> {
    let state = f.service.lock_selected().unwrap();
    let rich_state::StoredAuthority::Rich(s) = &*state else {
        panic!()
    };
    s.delivery.encode_live_checkpoint().unwrap()
}

fn pending(f: &Fixture) -> (MqHconn, MqHobj, u64) {
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
    (c, o, unit)
}

#[test]
fn memory_sqlite_warning_does_not_advance_expiry_or_change_pending_work_or_browse() {
    for sqlite in [false, true] {
        let f = Fixture::new(sqlite);
        let (c, o, unit) = pending(&f);
        let mut p = put(MqMqiUnitOfWork::NoSyncpoint);
        p.message.descriptor.expiry = MqExpiry::RelativeHostTicks(2);
        f.call(
            4,
            MqMqiRequest::Put {
                connection: c,
                object: o,
                put: p,
            },
        );
        let browse = f.call(
            5,
            get(
                c,
                o,
                MqMqiUnitOfWork::NoSyncpoint,
                1024,
                MqGetMode::BrowseFirst,
            ),
        );
        let MqMqiOutput::Got {
            cursor: Some(cursor),
            ..
        } = output(browse)
        else {
            panic!("browse")
        };
        let before = delivery(&f);
        let rows = f.rows();
        f.clock.0.store(30, Ordering::SeqCst);
        warning(f.call(6, request(false)), c, MqMqiCall::Connect);
        assert_eq!(delivery(&f), before);
        assert_eq!(f.unit(), unit);
        assert_eq!(f.depth(), 1);
        for old in rows.iter().filter(|r| r.namespace != receipt::NAMESPACE) {
            let new = f
                .store
                .get_provider_state(&old.namespace, &old.key)
                .unwrap()
                .unwrap();
            assert_eq!(new.payload, old.payload);
        }
        // Original browse/cursor reply remains lossless; warning did not touch it.
        assert!(cursor > 0);
        f.call(
            7,
            MqMqiRequest::Back {
                connection: c,
                unit,
            },
        );
        assert_ne!(f.unit(), unit);
        assert_eq!(f.depth(), 0); // Only this later queue operation performs expiry.
    }
}

#[test]
fn memory_sqlite_warning_binding_corruption_is_rejected_before_saf_without_allocation() {
    for sqlite in [false, true] {
        for case in 0..8 {
            let f = Fixture::new(sqlite);
            let c = f.connect();
            let foreign = Fixture::new(sqlite).connect();
            let e = f.effect(2, request(false));
            f.seed(&e);
            let active = {
                let mut state = f.service.lock_selected().unwrap();
                let rich_state::StoredAuthority::Rich(s) = &mut *state else {
                    panic!()
                };
                let r = s.runtime.as_mut().unwrap();
                match case {
                    0 => r.connections.push(r.connections[0].clone()),
                    1 => r.connections.clear(),
                    2 => {
                        r.handles.disconnect(f.owner, c).unwrap();
                    }
                    3 => r.connections[0].connection = foreign,
                    4 => r.connections[0].key = "wrong-original-connect".into(),
                    5 => r.connections[0].unit = 99,
                    6 => r.control.fence += 1,
                    _ => {
                        let second = r
                            .handles
                            .handles_mut()
                            .connect(f.owner, MqHandleSharing::NonShared)
                            .unwrap();
                        let mut binding = r.connections[0].clone();
                        binding.connection = second;
                        r.connections.push(binding);
                    }
                }
                r.handles.handles_mut().active_handles()
            };
            let rows = f.rows();
            let calls = f.saf.calls.load(Ordering::SeqCst);
            assert!(f.execute(&e).is_err(), "case {case}");
            assert_eq!(f.rows(), rows);
            assert_eq!(f.saf.calls.load(Ordering::SeqCst), calls);
            let mut state = f.service.lock_selected().unwrap();
            let rich_state::StoredAuthority::Rich(s) = &mut *state else {
                panic!()
            };
            assert_eq!(
                s.runtime
                    .as_mut()
                    .unwrap()
                    .handles
                    .handles_mut()
                    .active_handles(),
                active
            );
        }
    }
}

#[test]
fn memory_sqlite_warning_profile_context_and_original_probe_remain_closed() {
    for sqlite in [false, true] {
        for case in 0..5 {
            let f = Fixture::new(sqlite);
            f.connect();
            let mut e = f.effect(2, request(false));
            let HostRequest::MqMqi(h) = &mut e.request else {
                panic!()
            };
            let MqMqiRequest::Connect(c) = &mut h.envelope.request else {
                panic!()
            };
            match case {
                0 => c.manager = Some(MqRouteName::new("OTHER").unwrap()),
                1 => {
                    c.options = MqMqiOptions::PendingStructure {
                        requested_version: Some(99),
                    }
                }
                2 => c.sharing = MqHandleSharing::SharedBlock,
                3 => h.envelope.context.owner.task_id += 1,
                _ => {}
            }
            f.seed(&e);
            let rows = f.rows();
            let calls = f.saf.calls.load(Ordering::SeqCst);
            if case == 4 {
                let mut forged = f.inv.clone();
                forged.cancellation_probe = Some(CancellationProbe::new());
                assert!(
                    f.service
                        .execute_selected_mqi(
                            f.frame,
                            &forged,
                            e.mq_mqi_occurrence(Default::default()).unwrap().unwrap(),
                            &f.provider,
                            Default::default()
                        )
                        .is_err()
                );
            } else {
                assert!(f.execute(&e).is_err());
            }
            assert_eq!(f.rows(), rows);
            assert_eq!(f.saf.calls.load(Ordering::SeqCst), calls);
        }
    }
}

#[test]
fn memory_sqlite_repeated_connect_connx_preserves_token_unit_and_exact_warning_replay() {
    for sqlite in [false, true] {
        let f = Fixture::new(sqlite);
        let c = f.connect();
        let unit = f.unit();
        for (sequence, extended) in [(2, false), (3, true)] {
            let before = f
                .store
                .get_provider_state(ownership::UOW_NAMESPACE, &unit.to_string())
                .unwrap()
                .unwrap();
            let mut requested = request(extended);
            if extended {
                let state = f.service.lock_selected().unwrap();
                let rich_state::StoredAuthority::Rich(s) = &*state else {
                    panic!()
                };
                let MqMqiRequest::ConnectExtended(connect) = &mut requested else {
                    panic!()
                };
                connect.manager =
                    Some(MqRouteName::new(s.catalog.queue_manager().name.as_str()).unwrap());
            }
            warning(
                f.call(sequence, requested),
                c,
                if extended {
                    MqMqiCall::ConnectExtended
                } else {
                    MqMqiCall::Connect
                },
            );
            assert_eq!(f.unit(), unit);
            let after = f
                .store
                .get_provider_state(ownership::UOW_NAMESPACE, &unit.to_string())
                .unwrap()
                .unwrap();
            assert_eq!(before.payload, after.payload);
            assert_eq!(after.version, before.version + 1);
            let mut state = f.service.lock_selected().unwrap();
            let rich_state::StoredAuthority::Rich(s) = &mut *state else {
                panic!()
            };
            let runtime = s.runtime.as_mut().unwrap();
            assert_eq!(runtime.connections.len(), 1);
            assert_eq!(runtime.handles.handles_mut().active_handles(), 1);
            assert_eq!(runtime.connections[0].key, "effect-1");
        }
    }
}
