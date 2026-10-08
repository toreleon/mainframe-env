//! Reuses independent byte fixture assembly, never product output expectations.
use super::*;
#[test]
fn qualified_get_independent_request_and_full_host_result_preimage() {
    for v2 in [false, true] {
        let e = envelope(MqMqiRequest::QualifiedFullGet(get(v2)));
        let mut expected = request(v2, 2);
        let old = text("FullGet");
        let new = text("QualifiedFullGet");
        let offset = expected.windows(old.len()).position(|b| b == old).unwrap();
        expected.splice(offset..offset + old.len(), new);
        assert_eq!(mq_mqi_request_bytes(&e).unwrap(), expected);
        assert_eq!(
            mq_mqi_request_digest(&e).unwrap(),
            <[u8; 32]>::from(Sha256::digest(&expected))
        );
        let out = MqMqiQualifiedGot {
            characters: crate::mq_md_value::MqMdCharacterEncoding::AsciiCompatible,
            disposition: MqGetDisposition::Message(MqTruncationDisposition::Complete { length: 3 }),
            message: Some(message(v2)),
            data_length: Some(3),
            cursor: None,
            resolved_queue: Some([b' '; 48]),
        };
        let r = super::super::result(MqMqiCall::Get, MqMqiOutput::QualifiedFullGot(out));
        let mut some_md = vec![0x21];
        some_md.extend(full_message(v2));
        let mut some_length = vec![0x21];
        some_length.extend(long(3));
        let mut some_name = vec![0x21];
        some_name.extend(bytes(&[b' '; 48]));
        let tree = object(
            "MqMqiResult",
            vec![
                ("call", call(2)),
                (
                    "outcome",
                    variant(
                        "MqMqiOutcome",
                        "StatusPending",
                        vec![(
                            "output",
                            variant(
                                "MqMqiOutput",
                                "QualifiedFullGot",
                                vec![(
                                    "0",
                                    object(
                                        "MqMqiQualifiedGot",
                                        vec![
                                            (
                                                "characters",
                                                unit("MqMdCharacterEncoding", "AsciiCompatible"),
                                            ),
                                            ("cursor", vec![0x20]),
                                            ("data_length", some_length),
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
                                            ("message", some_md),
                                            ("resolved_queue", some_name),
                                        ],
                                    ),
                                )],
                            ),
                        )],
                    ),
                ),
            ],
        );
        let mut canonical = b"mainframe-env.mq-mqi-result@1\0".to_vec();
        canonical.extend(text("mainframe-env.effect-canonical@1"));
        canonical.extend(text("mainframe-env.mq-mqi-boundary@1"));
        canonical.extend(object(
            "MqMqiResultEnvelope",
            vec![("limits", limits()), ("result", tree.clone())],
        ));
        assert_eq!(
            mq_mqi_result_bytes(&r, MqMqiLimits::default()).unwrap(),
            canonical
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
                    vec![("limits", limits()), ("result", tree)],
                ),
            )],
        ));
        let host = Ok(HostResult::MqMqi(Box::new(MqMqiHostResult {
            result: r,
            limits: MqMqiLimits::default(),
        })));
        assert_eq!(
            canonical_result_digest(&host).unwrap(),
            <[u8; 32]>::from(Sha256::digest(&core))
        );
        assert_eq!(canonical_result_size(&host, 8388608).unwrap(), core.len());
    }
}
