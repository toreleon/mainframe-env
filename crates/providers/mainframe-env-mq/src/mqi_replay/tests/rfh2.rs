use super::*;

fn short() -> MqMqiResult {
    MqMqiResult {
        call: MqMqiCall::HandleToBuffer,
        outcome: MqMqiOutcome::ReviewedOutput {
            status: MqReviewedStatus::from_symbols(
                MqMqiCall::HandleToBuffer,
                "MQCC_FAILED",
                "MQRC_PROPERTY_VALUE_TOO_BIG",
            )
            .unwrap(),
            output: MqMqiOutput::Rfh2Observation(MqRfh2Observation {
                descriptor: None,
                data_length: Some(116),
                buffer: MqRfh2BufferObservation::Unchanged,
            }),
        },
    }
}
#[test]
fn storage_four_preserves_defined_short_failure_and_complete_success_observations() {
    let value = short();
    roundtrip(&value);
    let json = as_value(&value);
    assert_eq!(
        json["schema_version"],
        "mainframe-env.mq-mqi-result-storage@4"
    );
    assert_eq!(
        json["outcome"]["output"]["observation"],
        json!({
        "md_value":null,"data_length":116,"buffer":{"kind":"Unchanged"}})
    );
    let mut md = mainframe_env_host_api::mq_mqi::property::mq_property_initial_descriptor();
    let mainframe_env_host_api::mq_md_value::MqMdValue::V1 { fields, .. } = &mut md else {
        panic!()
    };
    fields.encoding = 785;
    fields.coded_char_set_id = 1208;
    fields.msg_id = [255; 24];
    fields.correl_id = [128; 24];
    let name = MqPropertyName::checked("invoice.id".into(), MqMessageLimits::default()).unwrap();
    let bytes = mainframe_env_host_api::mq_mqi::rfh2::mq_rfh2_encode(
        &name,
        &MqPropertyDescriptor::source_default(),
        &MqPropertyData {
            kind: MqPropertyType::ByteString,
            encoding: 785,
            ccsid: 1208,
            bytes: vec![0, 255, 128],
        },
        MqMqiLimits::default(),
    )
    .unwrap();
    let value = MqMqiResult {
        call: MqMqiCall::HandleToBuffer,
        outcome: MqMqiOutcome::ReviewedOutput {
            status: MqReviewedStatus::from_symbols(
                MqMqiCall::HandleToBuffer,
                "MQCC_OK",
                "MQRC_NONE",
            )
            .unwrap(),
            output: MqMqiOutput::Rfh2Observation(MqRfh2Observation {
                descriptor: Some(
                    mainframe_env_host_api::mq_mqi::rfh2::mq_rfh2_outer_descriptor(&md).unwrap(),
                ),
                data_length: Some(bytes.len() as i32),
                buffer: MqRfh2BufferObservation::WrittenPrefix(bytes),
            }),
        },
    };
    roundtrip(&value);
}
#[test]
fn storage_four_strict_class_required_nullable_bounds_duplicates_and_digest_are_not_defaults() {
    let original = as_value(&short());
    for case in 0..13 {
        let mut v = original.clone();
        let o = &mut v["outcome"]["output"]["observation"];
        match case {
            0..=3 => {
                v["schema_version"] = json!(
                    [
                        SCHEMA,
                        FULL_SCHEMA,
                        PROPERTY_SCHEMA,
                        "mainframe-env.mq-mqi-result-storage@5"
                    ][case]
                )
            }
            4 => {
                o.as_object_mut().unwrap().remove("md_value");
            }
            5 => {
                o.as_object_mut().unwrap().remove("data_length");
            }
            6 => o["data_length"] = json!(-1),
            7 => o["data_length"] = json!(i32::MAX as i64 + 1),
            8 => o["buffer"] = json!({"kind":"WrittenPrefix","bytes":[0]}),
            9 => o["extra"] = json!(0),
            10 => v["host_result_digest"] = json!(vec![0; 32]),
            11 => v["outcome"]["reason"] = json!("MQRC_NONE"),
            _ => o["md_value"] = json!(vec![0; 365]),
        }
        reject(&v);
    }
    let text = String::from_utf8(stored(&short())).unwrap();
    let duplicate = text.replace(
        "\"data_length\":116",
        "\"data_length\":116,\"data_length\":116",
    );
    assert_ne!(text, duplicate);
    assert!(restore(duplicate.as_bytes()).is_err());
    let mut old = as_value(&result(
        MqMqiCall::Inquire,
        MqMqiOutput::Attributes {
            integers: vec![],
            characters: vec![],
        },
        true,
    ));
    old["schema_version"] = json!(RFH2_SCHEMA);
    reject(&old);
    let value = short();
    let limits = MqMqiLimits {
        buffer_bytes: 115,
        ..Default::default()
    };
    assert!(encode(&value, HostLimits::default(), limits, BYTES).is_err());
    assert!(decode(&stored(&value), HostLimits::default(), limits, BYTES).is_err());
}
