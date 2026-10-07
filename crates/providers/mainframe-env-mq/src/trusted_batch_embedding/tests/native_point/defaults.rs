//! Actual opaque retained point observations, not copied configuration evidence.
use super::*;
use mainframe_env_host_api::mq_wire_options::{MqWireFullPut, put_full_for_target};

#[test]
fn native_default_response_constructor_to_original_selected_put_put1_and_get_stored_policy() {
    for sqlite in [false, true] {
        for one in [false, true] {
            for synchronous in [false, true] {
                for explicit in [false, true] {
                    let f = NativeFixture::with_defaults(
                        sqlite,
                        1,
                        false,
                        None,
                        Some(crate::MqNativeProducerDefaults {
                            priority: 7,
                            persistence: crate::MqNativePersistence::Persistent,
                            response: if synchronous {
                                crate::MqNativePutResponse::Synchronous
                            } else {
                                crate::MqNativePutResponse::Asynchronous
                            },
                        }),
                        |_| {},
                    );
                    let root = f.root();
                    let mut frame = root.frame();
                    let (c, o) = connected(&f, &mut frame);
                    let structure = frame
                        .structure_profile(
                            if one {
                                MqMqiCall::PutOne
                            } else {
                                MqMqiCall::Put
                            },
                            c,
                        )
                        .unwrap();
                    let point = frame
                        .point_profile(
                            &structure,
                            if one {
                                MqTrustedBatchPointTarget::PutOne { lookup: lookup() }
                            } else {
                                object_target(o)
                            },
                        )
                        .unwrap();
                    let mut input = complete_message();
                    let MqMdValue::V1 { fields, .. } = &mut input.descriptor else {
                        panic!()
                    };
                    fields.priority = -1;
                    fields.persistence = 2;
                    let lookup = lookup();
                    let rows = f.rows();
                    let decoded = put_full_for_target(
                        c,
                        (!one).then_some(o),
                        one.then_some(&lookup),
                        MqWireFullPut {
                            message: input.clone(),
                            pmo_version: 1,
                            options: 4 | 16384 | if explicit { 131072 } else { 0 },
                        },
                        point.wire_bindings(),
                        Default::default(),
                    );
                    if !synchronous && !explicit {
                        assert!(decoded.is_err());
                        assert_eq!(f.rows(), rows);
                        continue;
                    }
                    let put = decoded.unwrap();
                    let original = if one {
                        MqMqiRequest::FullPutOne {
                            connection: c,
                            lookup,
                            alternate_user: None,
                            put,
                        }
                    } else {
                        MqMqiRequest::FullPut {
                            connection: c,
                            object: o,
                            put,
                        }
                    };
                    let e = effect(&frame, 3, original);
                    seed(&*f.store, frame.original(), &e);
                    let result = dispatch(&mut frame, &e).unwrap();
                    let MqMqiOutput::Produced(p) = output(result.clone()) else {
                        panic!()
                    };
                    assert_eq!(
                        (
                            p.descriptor.fields().priority,
                            p.descriptor.fields().persistence
                        ),
                        (-1, 2)
                    );
                    let rows = f.rows();
                    assert_eq!(dispatch(&mut frame, &e).unwrap(), result);
                    assert_eq!(f.rows(), rows);
                    let mut descriptor = input.descriptor;
                    let MqMdValue::V1 { fields, .. } = &mut descriptor else {
                        panic!()
                    };
                    fields.msg_id = [0; 24];
                    let MqMqiOutput::FullGot {
                        message: Some(stored),
                        ..
                    } = output(f.call(
                        &mut frame,
                        4,
                        MqMqiRequest::FullGet(MqMqiFullGet {
                            connection: c,
                            object: o,
                            descriptor,
                            mode: MqGetMode::Remove,
                            wait: MqWait::NoWait,
                            truncation: MqTruncation::Reject,
                            buffer_capacity: 2048,
                            message_handle: None,
                            options: MqMqiOptions::ContractDefault,
                            unit: MqMqiUnitOfWork::NoSyncpoint,
                        }),
                    ))
                    else {
                        panic!()
                    };
                    assert_eq!(
                        (
                            stored.descriptor.fields().priority,
                            stored.descriptor.fields().persistence
                        ),
                        (7, 1)
                    );
                    assert_eq!(stored.body, input.body);
                    assert_eq!(f.source.gmt.load(Ordering::SeqCst), 0);
                    assert_eq!(f.source.batch.load(Ordering::SeqCst), 0);
                }
            }
        }
    }
}

#[test]
fn native_put_response_defaults_bind_exact_live_catalog_target_call_and_unit() {
    for sqlite in [false, true] {
        for response in [
            None,
            Some(crate::MqNativePutResponse::Synchronous),
            Some(crate::MqNativePutResponse::Asynchronous),
        ] {
            let f = NativeFixture::with_defaults(
                sqlite,
                1,
                false,
                None,
                response.map(|response| crate::MqNativeProducerDefaults {
                    priority: 7,
                    persistence: crate::MqNativePersistence::Persistent,
                    response,
                }),
                |_| {},
            );
            let root = f.root();
            let mut frame = root.frame();
            let (c, o) = connected(&f, &mut frame);
            let before = f.rows();
            for one in [false, true] {
                let call = if one {
                    MqMqiCall::PutOne
                } else {
                    MqMqiCall::Put
                };
                let structure = frame.structure_profile(call, c).unwrap();
                let target = if one {
                    MqTrustedBatchPointTarget::PutOne { lookup: lookup() }
                } else {
                    object_target(o)
                };
                let point = frame.point_profile(&structure, target).unwrap();
                let b = point.wire_bindings();
                let lookup = lookup();
                let exact = response == Some(crate::MqNativePutResponse::Synchronous);
                assert_eq!(
                    b.queue_defaults_are_represented(
                        c,
                        (!one).then_some(o),
                        one.then_some(&lookup)
                    ),
                    exact
                );
                assert!(!b.queue_defaults_are_represented(
                    MqHconn::Default,
                    (!one).then_some(o),
                    one.then_some(&lookup)
                ));
                for sync in [2, 4] {
                    for explicit in [false, true] {
                        let input = MqWireFullPut {
                            message: complete_message(),
                            pmo_version: 1,
                            options: sync | 16384 | if explicit { 131072 } else { 0 },
                        };
                        assert_eq!(
                            put_full_for_target(
                                c,
                                (!one).then_some(o),
                                one.then_some(&lookup),
                                input,
                                b,
                                Default::default()
                            )
                            .is_ok(),
                            explicit || (exact && (!one || sync == 4))
                        );
                    }
                }
                // A later physical catalog change invalidates the captured query,
                // even though its typed defaults and current queue name are equal.
                if one {
                    let mut row = f
                        .store
                        .get_provider_state("mq-v1-object-catalog", "catalog")
                        .unwrap()
                        .unwrap();
                    let old = row.version;
                    row.version += 1;
                    f.store.put_provider_state(row, Some(old)).unwrap();
                    assert!(!b.queue_defaults_are_represented(c, None, Some(&lookup)));
                }
            }
            assert!(f.rows().len() >= before.len());
            assert_eq!(f.source.gmt.load(Ordering::SeqCst), 0);
            assert_eq!(f.source.batch.load(Ordering::SeqCst), 0);
        }
    }
}
