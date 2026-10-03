//! Compiler-generated capture/writeback, private frame/replies: engine evidence only.
use super::*;

fn binary_correlation() -> [u8; 24] {
    let mut bytes = [0; 24];
    for (i, byte) in bytes.iter_mut().enumerate() {
        *byte = [0, 0xff, 0x80, b' '][i % 4];
    }
    bytes
}

#[test]
fn compiled_md1_md2_put_put1_preserve_zero_and_binary_supplied_correlation() {
    for one in [false, true] {
        for v2 in [false, true] {
            for correlation in [[0; 24], binary_correlation()] {
                let (mut m, frame, _, connection, object) = started(one, v2);
                m.write("MD-MSGID", &[0x9a; 24]).unwrap();
                m.write("MD-CORRELID", &correlation).unwrap();
                let before_md = m.read("MESSAGE-DESC").unwrap();
                let before_pmo = m.read("PUT-OPTS").unwrap();
                let body = m.read("MESSAGE-BUFFER").unwrap();
                let od = m.read("OBJECT-DESC").unwrap();
                let hconn = m.read("HCONN").unwrap();
                let hobj = m.read("HOBJ").unwrap();
                let length = m.read("BUFFER-LENGTH").unwrap();
                let e = effect(&mut m);
                assert_eq!(e.sequence, if one { 2 } else { 3 });
                assert_eq!(e.run_unit, m.invocation.run_unit_id);
                assert_eq!(e.deadline_tick, m.invocation.deadline_tick);
                let HostRequest::MqMqi(host) = &e.request else {
                    panic!()
                };
                assert_eq!(host.envelope.context, context());
                assert_eq!(host.mutation.sequence, e.sequence);
                assert_eq!(
                    Some(&host.mutation.idempotency_key),
                    e.idempotency_key.as_ref()
                );
                assert_eq!(request(&e).message.descriptor.fields().msg_id, [0x9a; 24]);
                assert_eq!(
                    request(&e).message.descriptor.fields().correl_id,
                    correlation
                );
                assert_eq!(
                    request(&e).message.descriptor.version(),
                    if v2 { 2 } else { 1 }
                );
                assert_eq!(request(&e).message.body, b"HELLO");
                let out = produced(&e);
                assert_eq!(out.descriptor.fields().correl_id, correlation);
                reply(&mut m, &e, ok(MqMqiOutput::Produced(out))).unwrap();
                assert_eq!(m.read("MD-CORRELID").unwrap(), correlation);
                assert_eq!(m.read("MD-MSGID").unwrap(), [0x9a; 24]);
                assert_eq!(m.read("CC").unwrap(), 0_i32.to_be_bytes());
                assert_eq!(m.read("REASON").unwrap(), 0_i32.to_be_bytes());
                for (name, bytes) in [
                    ("MESSAGE-BUFFER", body),
                    ("OBJECT-DESC", od),
                    ("HCONN", hconn),
                    ("HOBJ", hobj),
                    ("BUFFER-LENGTH", length),
                ] {
                    assert_eq!(m.read(name).unwrap(), bytes, "{name}");
                }
                for (kind, before, after, changed) in [
                    (
                        if v2 {
                            MqRawLayoutKind::Md2
                        } else {
                            MqRawLayoutKind::Md1
                        },
                        before_md,
                        m.read("MESSAGE-DESC").unwrap(),
                        vec![
                            "UserIdentifier",
                            "AccountingToken",
                            "ApplIdentityData",
                            "PutApplType",
                            "PutApplName",
                            "PutDate",
                            "PutTime",
                            "ApplOriginData",
                        ],
                    ),
                    (
                        MqRawLayoutKind::Pmo1,
                        before_pmo,
                        m.read("PUT-OPTS").unwrap(),
                        vec!["ResolvedQName", "ResolvedQMgrName"],
                    ),
                ] {
                    for (i, (&input, &output)) in before.iter().zip(&after).enumerate() {
                        if !mq_raw_layout(kind).fields.iter().any(|f| {
                            changed.contains(&f.name) && (f.offset..f.offset + f.width).contains(&i)
                        }) {
                            assert_eq!(input, output, "unowned {kind:?} byte{i}");
                        }
                    }
                }
                assert_eq!(frame.scope.connection(1), Ok(connection));
                if let Some(object) = object {
                    assert_eq!(frame.scope.object(2, connection), Ok(object));
                }
            }
        }
    }
}

#[test]
fn supplied_correlation_never_admits_generation_or_mixed_pending_flags() {
    for one in [false, true] {
        for v2 in [false, true] {
            // Independent source fixtures q092190_: NEW_MSG_ID64, NEW_CORREL_ID128.
            for flags in [64_i32, 128, 64 | 128] {
                let (mut m, frame, _, _, _) = started(one, v2);
                m.write("MD-CORRELID", &[0; 24]).unwrap();
                m.write("PMO-OPTIONS", &(147460_i32 | flags).to_be_bytes())
                    .unwrap();
                let before = (m.bases.clone(), m.effect_sequence);
                assert!(call(&mut m, one).is_err());
                assert_eq!((m.bases.clone(), m.effect_sequence), before);
                frame.scope.require_context(context()).unwrap();
            }
            let (mut m, _, _, _, _) = started(one, v2);
            m.write("MD-MSGID", &[0; 24]).unwrap();
            m.write("MD-CORRELID", &[0; 24]).unwrap();
            let before = (m.bases.clone(), m.effect_sequence);
            assert!(call(&mut m, one).is_err());
            assert_eq!((m.bases.clone(), m.effect_sequence), before);
        }
    }
}

#[test]
fn zero_correlation_does_not_relax_reply_class_pairing_or_late_storage_fences() {
    for one in [false, true] {
        for v2 in [false, true] {
            for defect in 0..6 {
                let (mut m, frame, _, _, _) = started(one, v2);
                m.write("MD-CORRELID", &[0; 24]).unwrap();
                let e = effect(&mut m);
                let mut out = produced(&e);
                let outcome = match defect {
                    0 => {
                        match &mut out.descriptor {
                            MqMdValue::V1 { fields, .. } | MqMdValue::V2 { fields, .. } => {
                                fields.correl_id = [1; 24]
                            }
                        }
                        ok(MqMqiOutput::Produced(out))
                    }
                    1 => MqMqiOutcome::ReviewedOutput {
                        status: MqReviewedStatus::from_symbols(
                            if one {
                                MqMqiCall::PutOne
                            } else {
                                MqMqiCall::Put
                            },
                            "MQCC_WARNING",
                            "MQRC_PRIORITY_EXCEEDS_MAXIMUM",
                        )
                        .unwrap(),
                        output: MqMqiOutput::Produced(out),
                    },
                    2 => MqMqiOutcome::StatusPending {
                        output: MqMqiOutput::Produced(out),
                    },
                    3 => MqMqiOutcome::DuplicatePossible,
                    4 => {
                        m.write("MD-CORRELID", &[1; 24]).unwrap();
                        ok(MqMqiOutput::Produced(out))
                    }
                    _ => {
                        m.write("MD-SUFFIX", b"CHANGED!").unwrap();
                        ok(MqMqiOutput::Produced(out))
                    }
                };
                let before = m.bases.clone();
                assert_eq!(
                    reply(&mut m, &e, outcome),
                    Err(MachineProblem::Host(HostProblem::UnknownOutcome)),
                    "defect{defect}"
                );
                assert_eq!(m.bases, before);
                assert_eq!(frame.scope.connection(1), Err(HostProblem::UnknownOutcome));
            }
        }
    }
}
