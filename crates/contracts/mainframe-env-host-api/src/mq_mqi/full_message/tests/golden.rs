//! Independent byte assembly, with fixed fixture values and no production codec.
use super::*;
use sha2::{Digest, Sha256};
fn text(s: &str) -> Vec<u8> {
    let mut b = vec![1];
    b.extend_from_slice(&(s.len() as u64).to_le_bytes());
    b.extend_from_slice(s.as_bytes());
    b
}
fn bytes(v: &[u8]) -> Vec<u8> {
    let mut b = vec![2];
    b.extend_from_slice(&(v.len() as u64).to_le_bytes());
    b.extend_from_slice(v);
    b
}
fn number(tag: u8, v: u64) -> Vec<u8> {
    let mut b = vec![tag];
    b.extend_from_slice(&v.to_le_bytes());
    b
}
fn long(v: i32) -> Vec<u8> {
    let mut b = vec![0x1a];
    b.extend_from_slice(&v.to_le_bytes());
    b
}
fn object(s: &str, fields: Vec<(&str, Vec<u8>)>) -> Vec<u8> {
    let mut b = vec![0x40];
    b.extend(text(s));
    b.extend((fields.len() as u64).to_le_bytes());
    for (n, v) in fields {
        b.extend(text(n));
        b.extend(v);
    }
    b
}
fn variant(s: &str, v: &str, fields: Vec<(&str, Vec<u8>)>) -> Vec<u8> {
    let mut b = vec![0x41];
    b.extend(text(s));
    b.extend(text(v));
    b.extend((fields.len() as u64).to_le_bytes());
    for (n, v) in fields {
        b.extend(text(n));
        b.extend(v);
    }
    b
}
fn unit(s: &str, v: &str) -> Vec<u8> {
    variant(s, v, vec![])
}
fn sequence(values: Vec<Vec<u8>>) -> Vec<u8> {
    let mut b = vec![0x30];
    b.extend((values.len() as u64).to_le_bytes());
    for v in values {
        b.extend(v);
    }
    b
}
fn md(v2: bool) -> Vec<u8> {
    let d = value(v2, false);
    let f = d.fields();
    let common = object(
        "MqMdFields",
        vec![
            ("accounting_token", bytes(&f.accounting_token)),
            ("appl_identity_data", bytes(&f.appl_identity_data)),
            ("appl_origin_data", bytes(&f.appl_origin_data)),
            ("backout_count", long(f.backout_count)),
            ("coded_char_set_id", long(f.coded_char_set_id)),
            ("correl_id", bytes(&f.correl_id)),
            ("encoding", long(f.encoding)),
            ("expiry", long(f.expiry)),
            ("feedback", long(f.feedback)),
            ("format", bytes(&f.format)),
            ("msg_id", bytes(&f.msg_id)),
            ("msg_type", long(f.msg_type)),
            ("persistence", long(f.persistence)),
            ("priority", long(f.priority)),
            ("put_appl_name", bytes(&f.put_appl_name)),
            ("put_appl_type", long(f.put_appl_type)),
            ("put_date", bytes(&f.put_date)),
            ("put_time", bytes(&f.put_time)),
            ("reply_to_q", bytes(&f.reply_to_q)),
            ("reply_to_q_mgr", bytes(&f.reply_to_q_mgr)),
            ("report", long(f.report)),
            ("struc_id", bytes(&f.struc_id)),
            ("user_identifier", bytes(&f.user_identifier)),
        ],
    );
    let mut fields = vec![(
        "characters",
        unit("MqMdCharacterEncoding", "AsciiCompatible"),
    )];
    if v2 {
        fields.push((
            "extension",
            object(
                "MqMdV2Fields",
                vec![
                    (
                        "group_id",
                        bytes(&(0..24).map(|i| 0xc0 + i).collect::<Vec<_>>()),
                    ),
                    ("msg_flags", long(i32::MIN)),
                    ("msg_seq_number", long(-31)),
                    ("offset", long(i32::MAX)),
                    ("original_length", long(-1)),
                ],
            ),
        ));
    }
    fields.push(("fields", common));
    variant("MqMdValue", if v2 { "V2" } else { "V1" }, fields)
}
fn limits() -> Vec<u8> {
    let message = object(
        "MqMessageLimits",
        vec![
            ("body_bytes", number(0x15, 1048576)),
            ("destination_bytes", number(0x15, 256)),
            ("distribution_items", number(0x15, 256)),
            ("format_bytes", number(0x15, 32)),
            ("identifier_bytes", number(0x15, 64)),
            ("properties", number(0x15, 128)),
            ("property_name_bytes", number(0x15, 256)),
            ("property_total_bytes", number(0x15, 1048576)),
            ("property_value_bytes", number(0x15, 65536)),
            ("wait_ticks", number(0x13, 1000000)),
        ],
    );
    object(
        "MqMqiLimits",
        vec![
            ("attribute_bytes", number(0x15, 1048576)),
            ("buffer_bytes", number(0x15, 4194304)),
            ("canonical_bytes", number(0x15, 8388608)),
            ("message", message),
            ("selectors", number(0x15, 256)),
        ],
    )
}
fn handle(s: &str, slot: u32) -> Vec<u8> {
    let mut n = vec![0x12];
    n.extend(slot.to_le_bytes());
    object(
        s,
        vec![
            ("epoch", number(0x13, 4)),
            ("generation", number(0x13, 3)),
            ("registry", number(0x13, 77)),
            ("slot", n),
        ],
    )
}
fn con() -> Vec<u8> {
    variant(
        "MqHconn",
        "Issued",
        vec![("0", handle("MqConnectionId", 1))],
    )
}
fn full_message(v2: bool) -> Vec<u8> {
    object(
        "MqFullMessage",
        vec![
            ("body", bytes(&[0, 255, 10])),
            ("descriptor", md(v2)),
            (
                "properties",
                sequence(vec![object(
                    "MqMessageProperty",
                    vec![
                        ("kind", unit("MqPropertyType", "ByteString")),
                        ("name", text("bytes")),
                        ("value", bytes(&[0, 255])),
                    ],
                )]),
            ),
        ],
    )
}
fn full_put(v2: bool) -> Vec<u8> {
    object(
        "MqMqiFullPut",
        vec![
            ("context", unit("MqMqiMessageContext", "Default")),
            ("message", full_message(v2)),
            ("message_handle", vec![0x20]),
            ("options", unit("MqMqiOptions", "ContractDefault")),
            ("unit", unit("MqMqiUnitOfWork", "NoSyncpoint")),
        ],
    )
}
fn context() -> Vec<u8> {
    object(
        "MqMqiContext",
        vec![
            (
                "owner",
                object(
                    "MqHandleOwner",
                    vec![
                        ("environment", unit("MqHostEnvironment", "ZosBatch")),
                        ("host_id", number(0x13, 1)),
                        ("process_id", number(0x13, 2)),
                        ("syncpoint_epoch", number(0x13, 5)),
                        ("task_id", number(0x13, 4)),
                        ("thread_id", number(0x13, 3)),
                    ],
                ),
            ),
            ("syncpoint_owner", unit("MqSyncpointOwner", "QueueManager")),
        ],
    )
}
fn request(v2: bool, kind: u8) -> Vec<u8> {
    let payload = match kind {
        0 => variant(
            "MqMqiRequest",
            "FullPut",
            vec![
                ("connection", con()),
                ("object", handle("MqHobj", 2)),
                ("put", full_put(v2)),
            ],
        ),
        1 => {
            let mut name = vec![0x42];
            name.extend(text("MqRouteName"));
            name.extend(text("QUEUE"));
            variant(
                "MqMqiRequest",
                "FullPutOne",
                vec![
                    ("alternate_user", vec![0x20]),
                    ("connection", con()),
                    (
                        "lookup",
                        variant(
                            "MqRouteLookup",
                            "Queue",
                            vec![
                                ("dynamic_pattern", vec![0x20]),
                                ("manager", vec![0x20]),
                                ("name", name),
                            ],
                        ),
                    ),
                    ("put", full_put(v2)),
                ],
            )
        }
        _ => variant(
            "MqMqiRequest",
            "FullGet",
            vec![(
                "0",
                object(
                    "MqMqiFullGet",
                    vec![
                        ("buffer_capacity", number(0x15, 3)),
                        ("connection", con()),
                        ("descriptor", md(v2)),
                        ("message_handle", vec![0x20]),
                        ("mode", unit("MqGetMode", "Remove")),
                        ("object", handle("MqHobj", 2)),
                        ("options", unit("MqMqiOptions", "ContractDefault")),
                        ("truncation", unit("MqTruncation", "Reject")),
                        ("unit", unit("MqMqiUnitOfWork", "NoSyncpoint")),
                        ("wait", unit("MqWait", "NoWait")),
                    ],
                ),
            )],
        ),
    };
    let mut b = b"mainframe-env.mq-mqi-request@1\0".to_vec();
    b.extend(text("mainframe-env.effect-canonical@1"));
    b.extend(text("mainframe-env.mq-mqi-boundary@1"));
    b.extend(object(
        "MqMqiRequestEnvelope",
        vec![
            ("call", call(kind)),
            ("context", context()),
            ("limits", limits()),
            ("request", payload),
        ],
    ));
    b
}
fn call(kind: u8) -> Vec<u8> {
    let (label, row, topic, hash) = match kind {
        0 => (
            "MQPUT",
            20u16,
            "SSFKSJ_9.4.0/refdev/q101880_.html",
            "47f73a96d6926573dd0a09a33a2e48d8389aedef7a2ded58d5d3e7562f393aab",
        ),
        1 => (
            "MQPUT1",
            21u16,
            "SSFKSJ_9.4.0/refdev/q101890_.html",
            "6b51813a2e99c74f04c22c599d4ad82d0b0eec31924abb6dc8e03e1ed7438dcb",
        ),
        _ => (
            "MQGET",
            15u16,
            "SSFKSJ_9.4.0/refdev/q101830_.html",
            "290b8af3acbe4a87f007ab9e3b67d0a797f835066118c9c6150ff0570e430b62",
        ),
    };
    let mut pos = vec![0x11];
    pos.extend(row.to_le_bytes());
    object(
        "MqMqiCall",
        vec![
            ("label", text(label)),
            (
                "official_row",
                text(&format!(
                    "ibm-mq-9.4-mqi-2026-08-31:mqi-calls-unique:{row:04}"
                )),
            ),
            ("source_positions", sequence(vec![pos])),
            ("topic_path", text(topic)),
            ("topic_sha256", text(hash)),
        ],
    )
}
fn result_tree(v2: bool) -> Vec<u8> {
    object(
        "MqMqiResult",
        vec![
            ("call", call(0)),
            (
                "outcome",
                variant(
                    "MqMqiOutcome",
                    "StatusPending",
                    vec![(
                        "output",
                        variant(
                            "MqMqiOutput",
                            "FullPut",
                            vec![
                                ("descriptor", md(v2)),
                                ("outcome", unit("MqDeliveryOutcome", "Accepted")),
                            ],
                        ),
                    )],
                ),
            ),
        ],
    )
}
fn get_result_tree(v2: bool) -> Vec<u8> {
    let output = variant(
        "MqMqiOutput",
        "FullGot",
        vec![
            ("cursor", vec![0x20]),
            ("data_length", {
                let mut b = vec![0x21];
                b.extend(long(3));
                b
            }),
            (
                "disposition",
                variant(
                    "MqGetDisposition",
                    "Message",
                    vec![(
                        "0",
                        variant(
                            "MqTruncationDisposition",
                            "Complete",
                            vec![("length", number(0x15, 3))],
                        ),
                    )],
                ),
            ),
            ("message", {
                let mut b = vec![0x21];
                b.extend(full_message(v2));
                b
            }),
        ],
    );
    object(
        "MqMqiResult",
        vec![
            ("call", call(2)),
            (
                "outcome",
                variant("MqMqiOutcome", "StatusPending", vec![("output", output)]),
            ),
        ],
    )
}
#[test]
fn full_get_independent_result_preimage_and_core_result_golden() {
    for v2 in [false, true] {
        let r = super::result(
            MqMqiCall::Get,
            got(
                v2,
                MqGetDisposition::Message(MqTruncationDisposition::Complete { length: 3 }),
                Some(3),
            ),
        );
        let mut standalone = b"mainframe-env.mq-mqi-result@1\0".to_vec();
        standalone.extend(text("mainframe-env.effect-canonical@1"));
        standalone.extend(text("mainframe-env.mq-mqi-boundary@1"));
        standalone.extend(object(
            "MqMqiResultEnvelope",
            vec![("limits", limits()), ("result", get_result_tree(v2))],
        ));
        assert_eq!(
            mq_mqi_result_bytes(&r, MqMqiLimits::default()).unwrap(),
            standalone
        );
        let mut core = b"mainframe-env.effect-result@1\0".to_vec();
        core.push(0x22);
        core.extend(variant(
            "HostResult",
            "MqMqi",
            vec![(
                "0",
                object(
                    "MqMqiHostResult",
                    vec![("limits", limits()), ("result", get_result_tree(v2))],
                ),
            )],
        ));
        let host = Ok(HostResult::MqMqi(MqMqiHostResult {
            result: r,
            limits: MqMqiLimits::default(),
        }));
        assert_eq!(
            canonical_result_digest(&host).unwrap(),
            <[u8; 32]>::from(Sha256::digest(&core))
        );
        assert_eq!(canonical_result_size(&host, 8388608).unwrap(), core.len());
    }
}
#[test]
fn full_message_independent_request_result_preimages_and_host_digest_goldens() {
    for v2 in [false, true] {
        for kind in 0..3 {
            let r = match kind {
                0 => MqMqiRequest::FullPut {
                    connection: connection(),
                    object: super::object(),
                    put: put(v2),
                },
                1 => MqMqiRequest::FullPutOne {
                    connection: connection(),
                    lookup: MqRouteLookup::Queue {
                        name: MqRouteName::new("QUEUE").unwrap(),
                        manager: None,
                        dynamic_pattern: None,
                    },
                    alternate_user: None,
                    put: put(v2),
                },
                _ => MqMqiRequest::FullGet(get(v2)),
            };
            let e = envelope(r);
            let expected = request(v2, kind);
            assert_eq!(mq_mqi_request_bytes(&e).unwrap(), expected);
            assert_eq!(
                mq_mqi_request_digest(&e).unwrap(),
                <[u8; 32]>::from(Sha256::digest(&expected))
            );
            // Independently frame the same complete request in the original
            // host journal domain, including its original mutation identity.
            let skip = b"mainframe-env.mq-mqi-request@1\0".len()
                + text("mainframe-env.effect-canonical@1").len()
                + text("mainframe-env.mq-mqi-boundary@1").len();
            let mut key = vec![0x42];
            key.extend(text("IdempotencyKey"));
            key.extend(text("full-golden"));
            let mutation = object(
                "Mutation",
                vec![
                    ("idempotency_key", key),
                    ("sequence", number(0x13, 7)),
                    ("transaction", vec![0x20]),
                ],
            );
            let mut core = b"mainframe-env.effect-request@1\0".to_vec();
            core.extend(variant(
                "HostRequest",
                "MqMqi",
                vec![(
                    "0",
                    object(
                        "MqMqiHostRequest",
                        vec![
                            ("envelope", expected[skip..].to_vec()),
                            ("mutation", mutation),
                        ],
                    ),
                )],
            ));
            let host = HostRequest::MqMqi(MqMqiHostRequest {
                envelope: e,
                mutation: Mutation {
                    sequence: 7,
                    idempotency_key: mainframe_env_execution_api::IdempotencyKey::new(
                        "full-golden",
                        mainframe_env_execution_api::InvocationLimits::default(),
                    )
                    .unwrap(),
                    transaction: None,
                },
            });
            assert_eq!(
                canonical_request_digest(&host).unwrap(),
                <[u8; 32]>::from(Sha256::digest(&core))
            );
            assert_eq!(canonical_request_size(&host, 8388608).unwrap(), core.len());
        }
        let r = super::result(
            MqMqiCall::Put,
            MqMqiOutput::FullPut {
                descriptor: value(v2, false),
                outcome: MqDeliveryOutcome::Accepted,
            },
        );
        let mut expected = b"mainframe-env.mq-mqi-result@1\0".to_vec();
        expected.extend(text("mainframe-env.effect-canonical@1"));
        expected.extend(text("mainframe-env.mq-mqi-boundary@1"));
        expected.extend(object(
            "MqMqiResultEnvelope",
            vec![("limits", limits()), ("result", result_tree(v2))],
        ));
        assert_eq!(
            mq_mqi_result_bytes(&r, MqMqiLimits::default()).unwrap(),
            expected
        );
        let mut core = b"mainframe-env.effect-result@1\0".to_vec();
        core.push(0x22);
        core.extend(variant(
            "HostResult",
            "MqMqi",
            vec![(
                "0",
                object(
                    "MqMqiHostResult",
                    vec![("limits", limits()), ("result", result_tree(v2))],
                ),
            )],
        ));
        let host = Ok(HostResult::MqMqi(MqMqiHostResult {
            result: r,
            limits: MqMqiLimits::default(),
        }));
        assert_eq!(
            canonical_result_digest(&host).unwrap(),
            <[u8; 32]>::from(Sha256::digest(&core))
        );
        assert_eq!(canonical_result_size(&host, 8388608).unwrap(), core.len());
    }
}
