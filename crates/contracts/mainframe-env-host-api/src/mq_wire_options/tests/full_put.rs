use super::*;
#[test]
fn complete_pmo1_wire_retains_full_value_and_explicit_context_response_units() {
    for v2 in [false, true] {
        for cp in [false, true] {
            for no_context in [false, true] {
                let p = crate::mq_mqi::producer::tests::input(v2, cp);
                let b = Bindings {
                    zos: true,
                    unit: Some(MqMqiUnitOfWork::Local { unit: 19 }),
                    ..Default::default()
                };
                for sync in [2, 4] {
                    let input = MqWireFullPut {
                        message: p.message.clone(),
                        pmo_version: 1,
                        options: 131072 | sync | if no_context { 16384 } else { 32 },
                    };
                    let got = put_full(handles().0, input, &b, Default::default()).unwrap();
                    assert_eq!(got.message, p.message);
                    assert_eq!(got.options, MqMqiOptions::PutV1Synchronous);
                    assert_eq!(
                        got.context,
                        if no_context {
                            MqMqiMessageContext::NoContext
                        } else {
                            MqMqiMessageContext::Default
                        }
                    );
                    assert_eq!(
                        got.unit,
                        if sync == 2 {
                            MqMqiUnitOfWork::Local { unit: 19 }
                        } else {
                            MqMqiUnitOfWork::NoSyncpoint
                        }
                    );
                }
            }
        }
    }
}
#[test]
fn complete_pmo_rejects_implicit_response_context_new_ids_and_unrepresented_bits() {
    for flags in [
        0,
        4 | 32,
        4 | 131072,
        4 | 32 | 131072 | 64,
        4 | 32 | 16384 | 131072,
        2 | 4 | 32 | 131072,
        -1,
        i64::MAX,
    ] {
        let p = crate::mq_mqi::producer::tests::input(true, false);
        assert!(
            put_full(
                handles().0,
                MqWireFullPut {
                    message: p.message,
                    pmo_version: 1,
                    options: flags
                },
                &Bindings::default(),
                Default::default()
            )
            .is_err()
        );
    }
}

#[test]
fn queue_default_response_requires_exact_target_existing_binding_and_finite_sync_behavior() {
    let (c, o) = handles();
    let q = MqRouteLookup::Queue {
        name: MqRouteName::new("Q").unwrap(),
        manager: None,
        dynamic_pattern: None,
    };
    for one in [false, true] {
        for sync in [2, 4] {
            for represented in [false, true] {
                let mut p = crate::mq_mqi::producer::tests::input(false, false);
                let crate::mq_md_value::MqMdValue::V1 { fields, .. } = &mut p.message.descriptor
                else {
                    panic!()
                };
                fields.priority = -1;
                fields.persistence = 2;
                let b = Bindings {
                    zos: true,
                    policy: represented,
                    unit: Some(MqMqiUnitOfWork::Local { unit: 19 }),
                    ..Default::default()
                };
                let build = |response| MqWireFullPut {
                    message: p.message.clone(),
                    pmo_version: 1,
                    options: i64::from(sync | 16384 | response),
                };
                let result = put_full_for_target(
                    c,
                    (!one).then_some(o),
                    one.then_some(&q),
                    build(0),
                    &b,
                    Default::default(),
                );
                assert_eq!(result.is_ok(), represented && (!one || sync == 4));
                if let Ok(got) = result {
                    assert_eq!(got.message, p.message);
                    assert_eq!(got.options, MqMqiOptions::PutV1Synchronous);
                }
                // Explicit synchronous response overrides missing/async defaults.
                put_full_for_target(
                    c,
                    (!one).then_some(o),
                    one.then_some(&q),
                    build(131072),
                    &b,
                    Default::default(),
                )
                .unwrap();
                assert!(
                    put_full_for_target(
                        c,
                        (!one).then_some(o),
                        one.then_some(&q),
                        build(65536),
                        &b,
                        Default::default()
                    )
                    .is_err()
                );
                assert!(
                    put_full_for_target(
                        c,
                        (!one).then_some(o),
                        one.then_some(&q),
                        build(65536 | 131072),
                        &b,
                        Default::default()
                    )
                    .is_err()
                );
                assert!(
                    put_full_for_target(c, None, None, build(0), &b, Default::default()).is_err()
                );
                assert!(
                    put_full_for_target(c, Some(o), Some(&q), build(0), &b, Default::default())
                        .is_err()
                );
            }
        }
    }
}
