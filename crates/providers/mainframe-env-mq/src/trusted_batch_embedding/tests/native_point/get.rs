//! Private selected/core/store fixtures, not installed/JES/native ABI evidence.
use super::*;
use mainframe_env_host_api::mq_wire_options::{self, MqWireFullGet};

fn bit(name: &str) -> i64 {
    i64::from(
        mq_wire_options::numeric_identities()
            .iter()
            .find(|(n, _)| *n == name)
            .unwrap()
            .1,
    )
}
fn descriptor(version: i32, cp: bool) -> MqMdValue {
    let MqMdValue::V1 { mut fields, .. } = complete_message().descriptor else {
        panic!()
    };
    fields.msg_id = [0; 24];
    if cp {
        fields.struc_id = [0xd4, 0xc4, 0x40, 0x40];
        fields.format = [0x40; 8];
        fields.reply_to_q = [0x40; 48];
        fields.reply_to_q_mgr = [0x40; 48];
        fields.user_identifier = [0x40; 12];
        fields.appl_identity_data = [0x40; 32];
        fields.put_appl_name = [0x40; 28];
        fields.put_date = [0x40; 8];
        fields.put_time = [0x40; 8];
        fields.appl_origin_data = [0x40; 4];
    }
    let characters = if cp {
        MqMdCharacterEncoding::OwnedCp037
    } else {
        MqMdCharacterEncoding::AsciiCompatible
    };
    if version == 1 {
        MqMdValue::V1 { characters, fields }
    } else {
        let initial = |name| {
            let field = mainframe_env_host_api::mq_raw_layout::mq_raw_layout(
                mainframe_env_host_api::mq_raw_layout::MqRawLayoutKind::Md2,
            )
            .fields
            .iter()
            .find(|f| f.name == name)
            .unwrap();
            let mainframe_env_host_api::mq_raw_layout::MqRawInitialValue::Long(n) = field.initial
            else {
                panic!()
            };
            n
        };
        MqMdValue::V2 {
            characters,
            fields,
            extension: MqMdV2Fields {
                group_id: [0; 24],
                msg_seq_number: initial("MsgSeqNumber"),
                offset: initial("Offset"),
                msg_flags: 0,
                original_length: initial("OriginalLength"),
            },
        }
    }
}
fn decode(
    point: &MqTrustedBatchPointProfile,
    c: MqHconn,
    o: MqHobj,
    version: i32,
    cp: bool,
    flags: i64,
    capacity: i64,
) -> Result<MqMqiFullGet, mq_wire_options::MqWireProblem> {
    mq_wire_options::get_full(
        MqWireFullGet {
            connection: c,
            object: o,
            descriptor: descriptor(version, cp),
            gmo_version: 1,
            options: flags,
            wait_milliseconds: 0,
            buffer_capacity: capacity,
        },
        point.wire_bindings(),
        Default::default(),
    )
}

#[test]
fn get_profiles_precede_decode_and_all_finite_controls_are_read_only() {
    for sqlite in [false, true] {
        for version in [1, 2] {
            for cp in [false, true] {
                let f = NativeFixture::new(sqlite, version, cp);
                let root = f.root();
                let parent = root.frame();
                let mut child = f.child(&parent, "child");
                let (c, o) = connected(&f, &mut child);
                let rows = f.rows();
                let current = unit(&child, c);
                let saf = f.saf.calls.load(Ordering::SeqCst);
                let audits = f
                    .store
                    .audit_records(&child.original().execution_id, 0, 128)
                    .unwrap();
                // Bootstrap needs no decoded MD/GMO or caller-selected character set.
                let s = child.structure_profile(MqMqiCall::Get, c).unwrap();
                assert_eq!(
                    s.characters(),
                    if cp {
                        MqMdCharacterEncoding::OwnedCp037
                    } else {
                        MqMdCharacterEncoding::AsciiCompatible
                    }
                );
                let p = child.point_profile(&s, object_target(o)).unwrap();
                assert_eq!(p.descriptor_version(), version);
                assert_eq!(p.max_message_bytes(), 2048);
                assert!(
                    p.wire_bindings()
                        .queue_defaults_are_represented(c, Some(o), None)
                );
                let callbacks = f.source.live.load(Ordering::SeqCst);
                assert!(!p.wire_bindings().queue_defaults_are_represented(
                    c,
                    None,
                    Some(&lookup())
                ));
                assert!(!p.wire_bindings().queue_defaults_are_represented(
                    MqHconn::Default,
                    Some(o),
                    None
                ));
                assert_eq!(f.source.live.load(Ordering::SeqCst), callbacks);
                for sync in [0, bit("MQGMO_SYNCPOINT"), bit("MQGMO_NO_SYNCPOINT")] {
                    for truncate in [0, bit("MQGMO_ACCEPT_TRUNCATED_MSG")] {
                        for capacity in [0, 1, 2048] {
                            let g =
                                decode(&p, c, o, version, cp, sync | truncate, capacity).unwrap();
                            assert_eq!(g.descriptor, descriptor(version, cp));
                            assert_eq!(g.mode, MqGetMode::Remove);
                            assert_eq!(g.wait, MqWait::NoWait);
                            assert_eq!(g.buffer_capacity, capacity as usize);
                            assert_eq!(
                                g.unit,
                                if sync == bit("MQGMO_NO_SYNCPOINT") {
                                    MqMqiUnitOfWork::NoSyncpoint
                                } else {
                                    MqMqiUnitOfWork::Local { unit: current }
                                }
                            );
                        }
                    }
                }
                for unsupported in [
                    "MQGMO_WAIT",
                    "MQGMO_BROWSE_FIRST",
                    "MQGMO_CONVERT",
                    "MQGMO_PROPERTIES_IN_HANDLE",
                    "MQGMO_SYNCPOINT_IF_PERSISTENT",
                ] {
                    assert!(decode(&p, c, o, version, cp, bit(unsupported), 3).is_err());
                }
                assert_eq!(p.wire_bindings().existing_cursor(c, o), None);
                assert_eq!(p.wire_bindings().milliseconds_to_ticks(1), None);
                assert_eq!(f.rows(), rows);
                assert_eq!(unit(&child, c), current);
                assert_eq!(f.saf.calls.load(Ordering::SeqCst), saf);
                assert_eq!(
                    f.store
                        .audit_records(&child.original().execution_id, 0, 128)
                        .unwrap(),
                    audits
                );
                assert_eq!(f.source.gmt.load(Ordering::SeqCst), 0);
                assert_eq!(f.source.batch.load(Ordering::SeqCst), 0);
                // Genuine selected original empty GET; no seeded payload/PUT assertion.
                let g = decode(&p, c, o, version, cp, bit("MQGMO_NO_SYNCPOINT"), 0).unwrap();
                let out = output(f.call(&mut child, 3, MqMqiRequest::FullGet(g)));
                assert!(matches!(
                    out,
                    MqMqiOutput::FullGot {
                        disposition: MqGetDisposition::NoMessage,
                        message: None,
                        data_length: None,
                        cursor: None
                    }
                ));
                child.recheck_structure_profile(&s).unwrap();
                child.recheck_point_profile(&p).unwrap();
            }
        }
    }
}

fn produce(
    f: &NativeFixture,
    frame: &mut MqTrustedBatchFrame,
    c: MqHconn,
    o: MqHobj,
    version: i32,
) {
    let mut message = complete_message();
    message.descriptor = descriptor(version, false);
    match &mut message.descriptor {
        MqMdValue::V1 { fields, .. } | MqMdValue::V2 { fields, .. } => fields.msg_id = [7; 24],
    }
    let out = output(f.call(
        frame,
        3,
        MqMqiRequest::FullPut {
            connection: c,
            object: o,
            put: MqMqiFullPut {
                message,
                message_handle: None,
                context: MqMqiMessageContext::NoContext,
                options: MqMqiOptions::PutV1Synchronous,
                unit: MqMqiUnitOfWork::NoSyncpoint,
            },
        },
    ));
    assert!(matches!(out, MqMqiOutput::Produced(_)));
}

#[test]
fn actual_produced_get_prefix_truncation_and_pending_publications_keep_abi_facts() {
    for sqlite in [false, true] {
        for version in [1, 2] {
            for sync in [false, true] {
                for (capacity, accept) in [(0, false), (1, false), (0, true), (1, true), (3, false)]
                {
                    let f = NativeFixture::new(sqlite, version, false);
                    let root = f.root();
                    let mut frame = root.frame();
                    let (c, o) = connected(&f, &mut frame);
                    produce(&f, &mut frame, c, o, version);
                    let s = frame.structure_profile(MqMqiCall::Get, c).unwrap();
                    let p = frame.point_profile(&s, object_target(o)).unwrap();
                    let current = unit(&frame, c);
                    let flags = bit(if sync {
                        "MQGMO_SYNCPOINT"
                    } else {
                        "MQGMO_NO_SYNCPOINT"
                    }) | if accept {
                        bit("MQGMO_ACCEPT_TRUNCATED_MSG")
                    } else {
                        0
                    };
                    let g = decode(&p, c, o, version, false, flags, capacity).unwrap();
                    let result = f.call(&mut frame, 4, MqMqiRequest::FullGet(g));
                    let MqMqiOutput::FullGot {
                        message: Some(message),
                        data_length: Some(n),
                        disposition,
                        ..
                    } = output(result)
                    else {
                        panic!()
                    };
                    assert_eq!(n, 3);
                    assert_eq!(message.body, vec![0, 255, 37][..capacity as usize]);
                    assert_eq!(message.descriptor.version(), version);
                    assert_eq!(message.descriptor.fields().msg_id, [7; 24]);
                    assert_eq!(message.descriptor.fields().backout_count, 0);
                    assert_eq!(message.descriptor.fields().coded_char_set_id, 819);
                    assert!(message.properties.is_empty());
                    assert!(matches!(disposition, MqGetDisposition::Message(_)));
                    frame.recheck_structure_profile(&s).unwrap();
                    frame.recheck_point_profile(&p).unwrap();
                    assert_eq!(
                        p.wire_bindings().admitted_unit(c),
                        Some(MqMqiUnitOfWork::Local { unit: current })
                    );
                    assert_eq!(f.source.gmt.load(Ordering::SeqCst), 0);
                    assert_eq!(f.source.batch.load(Ordering::SeqCst), 0);
                    f.call(
                        &mut frame,
                        5,
                        MqMqiRequest::Back {
                            connection: c,
                            unit: current,
                        },
                    );
                    assert_eq!(p.wire_bindings().admitted_unit(c), None);
                    assert!(frame.recheck_point_profile(&p).is_err());
                    assert!(frame.structure_profile(MqMqiCall::Get, c).is_ok());
                }
            }
        }
    }
}

#[test]
fn get_requires_input_object_and_rejects_foreign_closed_aba_and_retired_frame() {
    for sqlite in [false, true] {
        let f = NativeFixture::new(sqlite, 1, false);
        let root = f.root();
        let mut frame = root.frame();
        let (c, o) = connected(&f, &mut frame);
        let s = frame.structure_profile(MqMqiCall::Get, c).unwrap();
        let p = frame.point_profile(&s, object_target(o)).unwrap();
        let request = MqMqiRequest::Open(
            MqObjectOpenRequest::new(
                c,
                lookup(),
                &[MqRouteOpenAccess::Output],
                Default::default(),
            )
            .unwrap(),
        );
        let MqMqiOutput::Opened {
            object: output_only,
            ..
        } = output(f.call(&mut frame, 3, request))
        else {
            panic!()
        };
        let before = f.rows();
        assert!(frame.point_profile(&s, object_target(output_only)).is_err());
        assert!(
            !p.wire_bindings()
                .queue_defaults_are_represented(c, Some(output_only), None)
        );
        assert!(frame.point_profile(&s, open_target()).is_err());
        let other = NativeFixture::new(false, 1, false);
        let other_root = other.root();
        let mut other_frame = other_root.frame();
        let (foreign, foreign_object) = connected(&other, &mut other_frame);
        assert!(frame.structure_profile(MqMqiCall::Get, foreign).is_err());
        assert!(
            frame
                .point_profile(&s, object_target(foreign_object))
                .is_err()
        );
        assert!(other_frame.recheck_point_profile(&p).is_err());
        assert_eq!(f.rows(), before);
        f.call(
            &mut frame,
            4,
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
        let MqMqiOutput::Opened { object: new, .. } = output(f.call(&mut frame, 5, object_open(c)))
        else {
            panic!()
        };
        assert_ne!(new, o);
        assert!(frame.recheck_point_profile(&p).is_err());
        assert!(
            !p.wire_bindings()
                .queue_defaults_are_represented(c, Some(o), None)
        );
        let parent = root.frame();
        let mut child = f.child(&parent, "child");
        let s = child.structure_profile(MqMqiCall::Get, c).unwrap();
        let p = child.point_profile(&s, object_target(new)).unwrap();
        let rows = f.rows();
        child.abort_preparation().unwrap();
        assert!(child.recheck_point_profile(&p).is_err());
        assert_eq!(p.wire_bindings().admitted_unit(c), None);
        drop(p);
        drop(s);
        assert_eq!(f.rows(), rows);
    }
}

#[test]
fn get_late_source_probe_clock_and_physical_dependency_changes_fail_without_writes() {
    for sqlite in [false, true] {
        for case in 0..7 {
            let f = NativeFixture::new(sqlite, 1, false);
            let root = f.root();
            let parent = root.frame();
            let mut child = f.child(&parent, "child");
            let (c, o) = connected(&f, &mut child);
            let s = child.structure_profile(MqMqiCall::Get, c).unwrap();
            let p = child.point_profile(&s, object_target(o)).unwrap();
            let rows = f.rows();
            let store = f.store.clone();
            let clock = f.clock.clone();
            let probe = child.original().cancellation_probe.clone().unwrap();
            let execution = child.original().execution_id.clone();
            *f.source.reentry.lock().unwrap() = Some(Arc::downgrade(&f.runtime.inner.service));
            if case == 0 {
                f.source.panic.store(true, Ordering::SeqCst);
            } else {
                *f.source.hook.lock().unwrap() = Some(Box::new(move || match case {
                    1 => probe.request(),
                    2 => clock.0.store(1000, Ordering::SeqCst),
                    3 => {
                        store
                            .transition_execution(&execution, 3, ExecutionState::Failed, 20)
                            .unwrap();
                    }
                    _ => {
                        let namespace = match case {
                            4 => "mq-v1-object-catalog",
                            5 => "mq-selected-v1-control",
                            _ => "mq-selected-v1-uow-owner",
                        };
                        let old = store
                            .list_provider_state_prefix(namespace, 1024)
                            .unwrap()
                            .remove(0);
                        let mut new = old.clone();
                        new.version += 1;
                        store.put_provider_state(new, Some(old.version)).unwrap();
                    }
                }));
            }
            assert!(child.recheck_point_profile(&p).is_err(), "case {case}");
            let after = f.rows();
            if case < 4 {
                assert_eq!(after, rows);
            } else {
                assert_eq!(after.iter().filter(|r| !rows.contains(r)).count(), 1);
            }
            f.source.panic.store(false, Ordering::SeqCst);
            assert_eq!(f.source.gmt.load(Ordering::SeqCst), 0);
            assert_eq!(f.source.batch.load(Ordering::SeqCst), 0);
            assert_eq!(f.rows(), after);
        }
    }
}

#[test]
fn get_postpersist_unknown_keeps_receipt_and_blocks_escaped_observation() {
    for sqlite in [false, true] {
        let f = NativeFixture::new(sqlite, 1, false);
        let root = f.root();
        let parent = root.frame();
        let mut child = f.child(&parent, "child");
        let (c, o) = connected(&f, &mut child);
        produce(&f, &mut child, c, o, 1);
        let s = child.structure_profile(MqMqiCall::Get, c).unwrap();
        let p = child.point_profile(&s, object_target(o)).unwrap();
        let e = effect(
            &child,
            4,
            MqMqiRequest::FullGet(
                decode(&p, c, o, 1, false, bit("MQGMO_NO_SYNCPOINT"), 3).unwrap(),
            ),
        );
        seed(&*f.store, child.original(), &e);
        f.runtime
            .inner
            .service
            .trusted_batch_test_reply_uncertainty();
        assert_eq!(dispatch(&mut child, &e), Err(HostProblem::UnknownOutcome));
        let rows = f.rows();
        let calls = f.source.live.load(Ordering::SeqCst);
        assert!(child.recheck_point_profile(&p).is_err());
        assert_eq!(p.wire_bindings().admitted_unit(c), None);
        assert_eq!(dispatch(&mut child, &e), Err(HostProblem::Unauthorized));
        assert_eq!(f.source.live.load(Ordering::SeqCst), calls);
        drop(p);
        drop(s);
        drop(child);
        assert_eq!(f.rows(), rows);
    }
}

#[test]
fn get_owned_sqlite_reopen_does_not_restore_object_or_frame_observation() {
    let NativeFixture { f, source, _db: db } = NativeFixture::new(true, 2, false);
    let db = db.unwrap();
    let (parent, c, o, rows) = {
        let root = f.root();
        let mut frame = root.frame();
        let (c, o) = connected(&f, &mut frame);
        let s = frame.structure_profile(MqMqiCall::Get, c).unwrap();
        let p = frame.point_profile(&s, object_target(o)).unwrap();
        assert_eq!(p.descriptor_version(), 2);
        (f.parent.clone(), c, o, f.rows())
    };
    drop(f);
    drop(source);
    let store = db.open();
    assert_eq!(store.list_provider_state_prefix("mq-", 4096).unwrap(), rows);
    let mut runtime = open(
        store.clone(),
        Arc::new(Saf::default()),
        Arc::new(Clock(AtomicU64::new(20))),
    )
    .unwrap();
    runtime
        .configure_producer_source(&store, Arc::new(Source::default()))
        .unwrap();
    let root = runtime.admit_root(parent).unwrap();
    let mut frame = root.frame();
    let before = store.list_provider_state_prefix("mq-", 4096).unwrap();
    assert!(frame.structure_profile(MqMqiCall::Get, c).is_err());
    assert_eq!(
        store.list_provider_state_prefix("mq-", 4096).unwrap(),
        before
    );
    let e = effect(&frame, 9, connect());
    seed(&*store, frame.original(), &e);
    let MqMqiOutput::Connected(new) = output(dispatch(&mut frame, &e).unwrap()) else {
        panic!()
    };
    let s = frame.structure_profile(MqMqiCall::Get, new).unwrap();
    assert!(frame.point_profile(&s, object_target(o)).is_err());
    drop(s);
    drop(frame);
    drop(root);
    drop(runtime);
    drop(store);
}
