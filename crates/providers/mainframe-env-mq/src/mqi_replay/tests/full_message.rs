//! Full storage@2 composition; no durable publication or live authority claim.
use super::*;
use mainframe_env_host_api::mq_md_value::*;

fn md(v2: bool, cp037: bool) -> MqMdValue {
    let characters = if cp037 {
        MqMdCharacterEncoding::OwnedCp037
    } else {
        MqMdCharacterEncoding::AsciiCompatible
    };
    let fields = MqMdFields {
        struc_id: if cp037 {
            [0xd4, 0xc4, 0x40, 0x40]
        } else {
            *b"MD  "
        },
        report: i32::MIN,
        msg_type: i32::MAX,
        expiry: -42,
        feedback: -77,
        encoding: -7,
        coded_char_set_id: -19,
        format: [0, 255, 32, 64, 1, 2, 3, 4],
        priority: -3,
        persistence: -99,
        msg_id: [0; 24],
        correl_id: [255; 24],
        backout_count: i32::MAX,
        reply_to_q: [32; 48],
        reply_to_q_mgr: [64; 48],
        user_identifier: [0; 12],
        accounting_token: [255; 32],
        appl_identity_data: [17; 32],
        put_appl_type: i32::MIN,
        put_appl_name: [99; 28],
        put_date: [32; 8],
        put_time: [0; 8],
        appl_origin_data: [0, 255, 32, 64],
    };
    if v2 {
        MqMdValue::V2 {
            characters,
            fields,
            extension: MqMdV2Fields {
                group_id: [33; 24],
                msg_seq_number: -99,
                offset: i32::MIN,
                msg_flags: i32::MAX,
                original_length: -1,
            },
        }
    } else {
        MqMdValue::V1 { characters, fields }
    }
}
fn full(v2: bool, cp037: bool) -> MqFullMessage {
    MqFullMessage {
        descriptor: md(v2, cp037),
        body: vec![0, 255, 10],
        properties: properties(),
    }
}
fn get(v2: bool, cp037: bool) -> MqMqiResult {
    result(
        MqMqiCall::Get,
        MqMqiOutput::FullGot {
            disposition: MqGetDisposition::Message(MqTruncationDisposition::Complete { length: 3 }),
            message: Some(full(v2, cp037)),
            data_length: Some(3),
            cursor: Some(9),
        },
        true,
    )
}
#[test]
fn full_message_storage_roundtrips_every_version_profile_and_original_host_digest() {
    for v2 in [false, true] {
        for cp037 in [false, true] {
            let got = get(v2, cp037);
            roundtrip(&got);
            for call in [MqMqiCall::Put, MqMqiCall::PutOne] {
                for completed in [false, true] {
                    let value = result(
                        call,
                        MqMqiOutput::FullPut {
                            descriptor: md(v2, cp037),
                            outcome: MqDeliveryOutcome::Accepted,
                        },
                        completed,
                    );
                    roundtrip(&value);
                    let raw = stored(&value);
                    let v: Value = serde_json::from_slice(&raw).unwrap();
                    assert_eq!(v["schema_version"], FULL_SCHEMA);
                    assert_eq!(v["outcome"]["output"]["kind"], "FullPut");
                    assert_eq!(
                        v["outcome"]["output"]["md_value"],
                        json!(mq_md_value_bytes(&md(v2, cp037), 2048).unwrap())
                    );
                }
            }
            let out = MqMqiOutput::FullGot {
                disposition: MqGetDisposition::Message(MqTruncationDisposition::RejectedRetained {
                    required: 4,
                    copied: 3,
                }),
                message: Some(full(v2, cp037)),
                data_length: Some(4),
                cursor: None,
            };
            let status = MqReviewedStatus::from_symbols(
                MqMqiCall::Get,
                "MQCC_WARNING",
                "MQRC_TRUNCATED_MSG_FAILED",
            )
            .unwrap();
            roundtrip(&MqMqiResult {
                call: MqMqiCall::Get,
                outcome: MqMqiOutcome::ReviewedOutput {
                    status,
                    output: out,
                },
            });
        }
    }
}
#[test]
fn full_message_storage_marker_and_fixed_field_byte_shape_are_unambiguous() {
    for v2 in [false, true] {
        let r = result(
            MqMqiCall::Put,
            MqMqiOutput::FullPut {
                descriptor: md(v2, false),
                outcome: MqDeliveryOutcome::Accepted,
            },
            false,
        );
        let descriptor =
            serde_json::to_string(&mq_md_value_bytes(&md(v2, false), 2048).unwrap()).unwrap();
        let digest = serde_json::to_string(
            &canonical_result_digest(&host(&r, MqMqiLimits::default())).unwrap(),
        )
        .unwrap();
        let expected = format!(
            r#"{{"schema_version":"mainframe-env.mq-mqi-result-storage@2","call":"MQPUT","outcome":{{"kind":"StatusPending","output":{{"kind":"FullPut","md_value":{descriptor},"outcome":"Accepted"}}}},"host_result_digest":{digest}}}"#
        );
        assert_eq!(stored(&r), expected.into_bytes());
        let got = result(
            MqMqiCall::Get,
            MqMqiOutput::FullGot {
                disposition: MqGetDisposition::Message(MqTruncationDisposition::Complete {
                    length: 3,
                }),
                message: Some(MqFullMessage {
                    descriptor: md(v2, false),
                    body: vec![0, 255, 10],
                    properties: vec![MqMessageProperty {
                        name: "bytes".into(),
                        kind: MqPropertyType::ByteString,
                        value: vec![0, 255],
                    }],
                }),
                data_length: Some(3),
                cursor: None,
            },
            false,
        );
        let digest = serde_json::to_string(
            &canonical_result_digest(&host(&got, MqMqiLimits::default())).unwrap(),
        )
        .unwrap();
        let expected = format!(
            r#"{{"schema_version":"mainframe-env.mq-mqi-result-storage@2","call":"MQGET","outcome":{{"kind":"StatusPending","output":{{"kind":"FullGot","disposition":{{"kind":"Complete","length":3}},"message":{{"md_value":{descriptor},"body":[0,255,10],"properties":[{{"name":"bytes","kind":"byte-string","value":[0,255]}}]}},"data_length":3,"cursor":null}}}},"host_result_digest":{digest}}}"#
        );
        assert_eq!(stored(&got), expected.into_bytes());
        let mut v: Value = serde_json::from_slice(&stored(&r)).unwrap();
        v["schema_version"] = json!(SCHEMA);
        reject(&v);
        v["schema_version"] = json!("mainframe-env.mq-mqi-result-storage@3");
        reject(&v);
    }
    let old = got(message());
    let raw = stored(&old);
    assert_eq!(as_value(&old)["schema_version"], SCHEMA);
    roundtrip(&old);
    let mut v: Value = serde_json::from_slice(&raw).unwrap();
    v["schema_version"] = json!(FULL_SCHEMA);
    reject(&v);
}
#[test]
fn full_metadata_never_defaults_persistence_expiry_or_descriptor_fields() {
    for persistence in [0, 1, -1, i32::MIN] {
        let mut value = get(true, true);
        let MqMqiOutcome::Completed {
            output: MqMqiOutput::FullGot {
                message: Some(m), ..
            },
            ..
        } = &mut value.outcome
        else {
            unreachable!()
        };
        let MqMdValue::V2 {
            fields, extension, ..
        } = &mut m.descriptor
        else {
            unreachable!()
        };
        fields.persistence = persistence;
        fields.expiry = 7;
        fields.priority = 9;
        fields.encoding = 546;
        fields.coded_char_set_id = 1208;
        extension.offset = 33;
        extension.original_length = 77;
        roundtrip(&value);
    }
}
#[test]
fn full_message_strict_storage_rejects_fields_corruption_quota_and_coherent_bad_lengths() {
    let good = get(true, false);
    let raw = stored(&good);
    let root: Value = serde_json::from_slice(&raw).unwrap();
    for path in ["message", "data_length", "cursor", "disposition"] {
        let mut v = root.clone();
        v["outcome"]["output"].as_object_mut().unwrap().remove(path);
        reject(&v);
    }
    for path in ["md_value", "body", "properties"] {
        let mut v = root.clone();
        v["outcome"]["output"]["message"]
            .as_object_mut()
            .unwrap()
            .remove(path);
        reject(&v);
    }
    let mut v = root.clone();
    v["outcome"]["output"]["message"]["extra"] = json!(0);
    reject(&v);
    let md_bytes = mq_md_value_bytes(&md(true, false), 2048).unwrap();
    for size in 0..md_bytes.len() {
        let mut v = root.clone();
        v["outcome"]["output"]["message"]["md_value"] = json!(&md_bytes[..size]);
        reject(&v);
    }
    for bytes in [
        vec![0; 2049],
        vec![0; 2048],
        {
            let mut b = md_bytes.clone();
            b.push(0);
            b
        },
        {
            let mut b = md_bytes.clone();
            b[0] = 0;
            b
        },
    ] {
        let mut v = root.clone();
        v["outcome"]["output"]["message"]["md_value"] = json!(bytes);
        reject(&v);
    }
    let mut v = root.clone();
    v["outcome"]["output"]["message"]["md_value"][0] = json!(1.5);
    reject(&v);
    let duplicate = String::from_utf8(raw.clone()).unwrap().replacen(
        "\"data_length\":3",
        "\"data_length\":3,\"data_length\":3",
        1,
    );
    assert!(restore(duplicate.as_bytes()).is_err());
    let mut trailing = raw.clone();
    trailing.extend_from_slice(b" 0");
    assert!(restore(&trailing).is_err());
    for n in [-1, 0, 2, 4, i32::MAX] {
        let mut typed = good.clone();
        if let MqMqiOutcome::Completed {
            output: MqMqiOutput::FullGot { data_length, .. },
            ..
        } = &mut typed.outcome
        {
            *data_length = Some(n);
        }
        let mut v = root.clone();
        v["outcome"]["output"]["data_length"] = json!(n);
        coherent(&mut v, &typed);
        reject(&v);
    }
    let mut v = root.clone();
    v["outcome"]["output"]["message"]["properties"] = json!(vec![
        v["outcome"]["output"]["message"]
            ["properties"][0]
            .clone();
        129
    ]);
    reject(&v);
    for n in [0, 1, raw.len() - 1] {
        assert!(decode(&raw, HostLimits::default(), MqMqiLimits::default(), n).is_err());
    }
    let mut h = HostLimits::default();
    h.max_record_bytes = 2;
    assert!(decode(&raw, h, MqMqiLimits::default(), BYTES).is_err());
    let mut m = MqMqiLimits::default();
    m.message.identifier_bytes = 23;
    assert!(decode(&raw, HostLimits::default(), m, BYTES).is_err());
    // A well-shaped but changed descriptor cannot reuse the original core digest.
    let mut changed = md(true, false);
    if let MqMdValue::V2 { fields, .. } = &mut changed {
        fields.feedback += 1;
    }
    let mut v = root.clone();
    v["outcome"]["output"]["message"]["md_value"] =
        json!(mq_md_value_bytes(&changed, 2048).unwrap());
    reject(&v);
}
