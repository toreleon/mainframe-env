//! Representation/storage fixtures, not native-executable descriptor policy.
use super::*;
fn qualified(v2: bool, cp: bool) -> MqMqiResult {
    result(
        MqMqiCall::Get,
        MqMqiOutput::QualifiedFullGot(MqMqiQualifiedGot {
            characters: md(v2, cp).characters(),
            disposition: MqGetDisposition::Message(MqTruncationDisposition::Complete { length: 3 }),
            message: Some(full(v2, cp)),
            data_length: Some(3),
            cursor: Some(9),
            resolved_queue: Some([if cp { 0x40 } else { b' ' }; 48]),
        }),
        true,
    )
}
#[test]
fn qualified_storage6_exact_shape_roundtrip_and_host_identity_all_md_profiles() {
    for v2 in [false, true] {
        for cp in [false, true] {
            let value = qualified(v2, cp);
            roundtrip(&value);
            let bytes = stored(&value);
            let text = String::from_utf8(bytes.clone()).unwrap();
            assert!(
                text.starts_with("{\"schema_version\":\"mainframe-env.mq-mqi-result-storage@6\"")
            );
            let body = serde_json::to_string(&full(v2, cp).body).unwrap();
            let md = serde_json::to_string(
                &crate::mqi_replay::full_message::capture_md(&md(v2, cp)).unwrap(),
            )
            .unwrap();
            let props = serde_json::to_string(
                &full(v2, cp)
                    .properties
                    .iter()
                    .map(ReplayProperty::from_property)
                    .collect::<Vec<_>>(),
            )
            .unwrap();
            let name = serde_json::to_string(&vec![if cp { 0x40 } else { b' ' }; 48]).unwrap();
            let digest = serde_json::to_string(
                &canonical_result_digest(&host(&value, MqMqiLimits::default())).unwrap(),
            )
            .unwrap();
            let chars = if cp { "OwnedCp037" } else { "AsciiCompatible" };
            let expected = format!(
                r#"{{"schema_version":"mainframe-env.mq-mqi-result-storage@6","call":"MQGET","outcome":{{"kind":"Completed","status":"OkNone","output":{{"kind":"QualifiedFullGot","observation":{{"characters":"{chars}","disposition":{{"kind":"Complete","length":3}},"message":{{"md_value":{md},"body":{body},"properties":{props}}},"data_length":3,"cursor":9,"resolved_queue":{name}}}}}}},"host_result_digest":{digest}}}"#
            );
            assert_eq!(text, expected);
            for schema in [
                SCHEMA,
                FULL_SCHEMA,
                PROPERTY_SCHEMA,
                RFH2_SCHEMA,
                PRODUCER_SCHEMA,
            ] {
                assert!(restore(text.replace(QUALIFIED_GET_SCHEMA, schema).as_bytes()).is_err());
            }
            let old = stored(&get(v2, cp));
            assert!(
                restore(
                    String::from_utf8(old)
                        .unwrap()
                        .replace(FULL_SCHEMA, QUALIFIED_GET_SCHEMA)
                        .as_bytes()
                )
                .is_err()
            );
        }
    }
}
#[test]
fn qualified_strict_order_optional_fields_width_types_trailing_digest_and_quotas() {
    let original = stored(&qualified(false, false));
    let text = String::from_utf8(original.clone()).unwrap();
    // A coherent same-width name mutation still requires the full host digest.
    assert!(text.contains("\"resolved_queue\":[32,"));
    assert!(
        restore(
            text.replace("\"resolved_queue\":[32,", "\"resolved_queue\":[33,")
                .as_bytes()
        )
        .is_err()
    );
    for (from, to) in [
        ("\"cursor\":9,", ""),
        ("\"data_length\":3,", ""),
        (
            "\"characters\":\"AsciiCompatible\"",
            "\"characters\":\"Native\"",
        ),
        ("\"cursor\":9", "\"cursor\":9,\"cursor\":9"),
        (
            "\"data_length\":3,\"cursor\":9",
            "\"cursor\":9,\"data_length\":3",
        ),
        ("\"cursor\":9", "\"cursor\":\"9\""),
        ("\"cursor\":9", "\"cursor\":9223372036854775808"),
        (
            "\"kind\":\"QualifiedFullGot\"",
            "\"kind\":\"QualifiedFullGot\",\"extra\":null",
        ),
        ("\"data_length\":3", "\"data_length\":4"),
    ] {
        assert!(text.contains(from));
        assert!(
            restore(text.replace(from, to).as_bytes()).is_err(),
            "{from}"
        );
    }
    let mut map: Value = serde_json::from_slice(&original).unwrap();
    for n in [0, 47, 49, 128] {
        map["outcome"]["output"]["observation"]["resolved_queue"] = json!(vec![32; n]);
        assert!(restore(&serde_json::to_vec(&map).unwrap()).is_err());
    }
    for n in [0, 1, original.len() - 1] {
        assert!(restore(&original[..n]).is_err());
    }
    let mut trailing = original.clone();
    trailing.extend(b" {}");
    assert!(restore(&trailing).is_err());
    let mut whitespace = original.clone();
    whitespace.push(b' ');
    assert!(restore(&whitespace).is_err());
    let mut mqi = MqMqiLimits::default();
    mqi.message.body_bytes = 2;
    assert!(decode(&original, HostLimits::default(), mqi, BYTES).is_err());
    assert!(
        decode(
            &original,
            HostLimits::default(),
            MqMqiLimits::default(),
            original.len() - 1
        )
        .is_err()
    );
}
#[test]
fn qualified_no_message_unknown_and_rejected_prefix_absence_survive_sole_codec() {
    for disposition in [
        MqGetDisposition::NoMessage,
        MqGetDisposition::UnknownOutcome,
        MqGetDisposition::Message(MqTruncationDisposition::RejectedRetained {
            required: 4,
            copied: 3,
        }),
    ] {
        let meaningful = matches!(disposition, MqGetDisposition::Message(_));
        let value = result(
            MqMqiCall::Get,
            MqMqiOutput::QualifiedFullGot(MqMqiQualifiedGot {
                characters: MqMdCharacterEncoding::AsciiCompatible,
                disposition,
                message: meaningful.then(|| full(true, false)),
                data_length: meaningful.then_some(4),
                cursor: None,
                resolved_queue: None,
            }),
            false,
        );
        roundtrip(&value);
        assert!(
            String::from_utf8(stored(&value))
                .unwrap()
                .contains("\"resolved_queue\":null")
        );
    }
}
