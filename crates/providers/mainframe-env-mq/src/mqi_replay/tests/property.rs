use super::*;
fn result(
    call: MqMqiCall,
    completion: &str,
    reason: &str,
    observation: MqPropertyObservation,
) -> MqMqiResult {
    MqMqiResult {
        call,
        outcome: MqMqiOutcome::ReviewedOutput {
            status: MqReviewedStatus::from_symbols(call, completion, reason).unwrap(),
            output: MqMqiOutput::PropertyObservation(observation),
        },
    }
}
fn partial() -> MqMqiResult {
    result(
        MqMqiCall::InquireProperty,
        "MQCC_FAILED",
        "MQRC_PROPERTY_VALUE_TOO_BIG",
        MqPropertyObservation::Inquired(MqPropertyInquiryObservation {
            descriptor: MqPropertyDescriptor {
                struc_id: *b"PD  ",
                version: 1,
                options: 0,
                support: 1,
                context: 0,
                copy_options: 22,
            },
            kind: MqPropertyType::ByteString,
            returned_encoding: 785,
            returned_ccsid: None,
            returned_name: b"invoice.id".to_vec(),
            name_length: 10,
            name_ccsid: 1208,
            data_length: 4,
            copied_value: vec![0, 255],
        }),
    )
}
#[test]
fn reviewed_property_partial_storage_three_has_a_frozen_full_result_vector() {
    let value = partial();
    // Fixed storage/canonical identity for the independently specified failed
    // byte-string fixture; expected bytes are not calculated by the encoder.
    let golden = concat!(
        r#"{"schema_version":"mainframe-env.mq-mqi-result-storage@3","call":"MQINQMP","outcome":{"kind":"ReviewedOutput","completion":"MQCC_FAILED","reason":"MQRC_PROPERTY_VALUE_TOO_BIG","output":{"kind":"PropertyObservation","observation":{"kind":"Inquired","descriptor":{"struc_id":[80,68,32,32],"version":1,"options":0,"support":1,"context":0,"copy_options":22},"property_type":"ByteString","returned_encoding":785,"returned_ccsid":null,"returned_name":[105,110,118,111,105,99,101,46,105,100],"name_length":10,"name_ccsid":1208,"data_length":4,"copied_value":[0,255]}}},"host_result_digest":["#,
        "40,102,65,223,166,77,92,250,36,71,128,248,252,249,84,74,",
        "153,247,90,156,186,69,174,251,116,180,140,38,167,15,106,146]}"
    );
    assert_eq!(stored(&value), golden.as_bytes());
    assert_eq!(restore(golden.as_bytes()).unwrap(), value);
}
#[test]
fn storage_three_roundtrips_defined_failed_prefix_and_each_property_observation_with_full_host_digest()
 {
    let values = vec![
        partial(),
        result(
            MqMqiCall::SetProperty,
            "MQCC_OK",
            "MQRC_NONE",
            MqPropertyObservation::Set(MqPropertyDescriptor::descriptor_output()),
        ),
        result(
            MqMqiCall::DeleteProperty,
            "MQCC_OK",
            "MQRC_NONE",
            MqPropertyObservation::PropertyDeleted,
        ),
        result(
            MqMqiCall::DeleteProperty,
            "MQCC_WARNING",
            "MQRC_PROPERTY_NOT_AVAILABLE",
            MqPropertyObservation::Absent,
        ),
        result(
            MqMqiCall::InquireProperty,
            "MQCC_FAILED",
            "MQRC_PROPERTY_NOT_AVAILABLE",
            MqPropertyObservation::Absent,
        ),
        result(
            MqMqiCall::DeleteMessageHandle,
            "MQCC_OK",
            "MQRC_NONE",
            MqPropertyObservation::HandleDeleted,
        ),
    ];
    for value in values {
        let bytes = stored(&value);
        let json: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            json["schema_version"],
            "mainframe-env.mq-mqi-result-storage@3"
        );
        assert_eq!(restore(&bytes).unwrap(), value);
        assert_eq!(stored(&restore(&bytes).unwrap()), bytes);
        let expected = canonical_result_digest(&host(&value, MqMqiLimits::default())).unwrap();
        assert_eq!(
            serde_json::from_value::<[u8; 32]>(json["host_result_digest"].clone()).unwrap(),
            expected
        );
    }
}
#[test]
fn strict_schema_dispatch_pins_and_bounded_observations_reject_mutations_before_restoration() {
    let original: Value = serde_json::from_slice(&stored(&partial())).unwrap();
    for case in 0..15 {
        let mut v = original.clone();
        let o = &mut v["outcome"]["output"]["observation"];
        match case {
            0 => v["schema_version"] = json!(SCHEMA),
            1 => v["schema_version"] = json!(FULL_SCHEMA),
            2 => v["schema_version"] = json!("mainframe-env.mq-mqi-result-storage@4"),
            3 => o["extra"] = json!(0),
            4 => o["returned_name"] = json!(vec![0; 257]),
            5 => o["returned_ccsid"] = json!(1208),
            6 => {
                o.as_object_mut().unwrap().remove("returned_ccsid");
            }
            7 => o["descriptor"]["support"] = json!(0),
            8 => o["data_length"] = json!(-1),
            9 => o["data_length"] = json!(i32::MAX as i64 + 1),
            10 => o["copied_value"] = json!([0, 255, 1, 2, 3]),
            11 => o["property_type"] = json!("Float64"),
            12 => v["outcome"]["completion"] = json!("MQCC_OK"),
            13 => v["host_result_digest"] = json!(vec![0; 32]),
            _ => o["descriptor"]["struc_id"] = json!([80, 68, 32]),
        }
        assert!(
            restore(&serde_json::to_vec(&v).unwrap()).is_err(),
            "case {case}"
        );
    }
    let text = String::from_utf8(stored(&partial())).unwrap();
    let duplicate = text.replace("\"data_length\":4", "\"data_length\":4,\"data_length\":4");
    assert_ne!(text, duplicate);
    assert!(restore(duplicate.as_bytes()).is_err());
    let wrong = result(
        MqMqiCall::InquireProperty,
        "MQCC_FAILED",
        "MQRC_PROPERTY_VALUE_TOO_BIG",
        MqPropertyObservation::HandleDeleted,
    );
    assert!(encode(&wrong, HostLimits::default(), MqMqiLimits::default(), BYTES).is_err());
}
