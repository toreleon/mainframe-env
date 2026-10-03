//! Producer class in the one result codec; @4 is deliberately reserved.
use super::*;
use mainframe_env_host_api::mq_md_value::*;
fn value(v2: bool, cp: bool) -> MqMqiResult {
    let mut descriptor = super::full_message::md(v2, cp);
    let b = if cp { 0x40 } else { b' ' };
    let f = match &mut descriptor {
        MqMdValue::V1 { fields, .. } | MqMdValue::V2 { fields, .. } => fields,
    };
    // Codec-only signed observations, NOT native-positive producer inputs.
    f.backout_count = i32::MIN;
    result(
        MqMqiCall::PutOne,
        MqMqiOutput::Produced(MqMqiProduced {
            descriptor,
            outcome: MqDeliveryOutcome::Accepted,
            resolved_queue: {
                let mut a = [b; 48];
                a[0] = if cp { 0xd8 } else { b'Q' };
                a
            },
            resolved_manager: {
                let mut a = [b; 48];
                a[0] = if cp { 0xd4 } else { b'M' };
                a
            },
            known_dest_count: MqMqiDestinationCount::UndefinedZos,
            unknown_dest_count: MqMqiDestinationCount::UndefinedZos,
            invalid_dest_count: MqMqiDestinationCount::UndefinedZos,
            backout_count: MqMqiIgnoredCounter::PreservedIgnoredInput,
        }),
        true,
    )
}
#[test]
fn producer_schema5_exact_shape_old_classes_reserved4_and_full_host_identity() {
    for v2 in [false, true] {
        for cp in [false, true] {
            let r = value(v2, cp);
            roundtrip(&r);
            let v = as_value(&r);
            assert_eq!(v["schema_version"], PRODUCER_SCHEMA);
            assert_eq!(v["outcome"]["output"]["kind"], "Produced");
            let out = &v["outcome"]["output"]["value"];
            assert_eq!(out["backout_count"], "PreservedIgnoredInput");
            assert_eq!(out["known_dest_count"], "UndefinedZos");
            assert_eq!(out["resolved_queue"].as_array().unwrap().len(), 48);
            for schema in [
                SCHEMA,
                FULL_SCHEMA,
                PROPERTY_SCHEMA,
                "mainframe-env.mq-mqi-result-storage@4",
            ] {
                let mut wrong = v.clone();
                wrong["schema_version"] = json!(schema);
                reject(&wrong);
            }
            for field in [
                "md_value",
                "outcome",
                "resolved_queue",
                "resolved_manager",
                "known_dest_count",
                "unknown_dest_count",
                "invalid_dest_count",
                "backout_count",
            ] {
                let mut wrong = v.clone();
                wrong["outcome"]["output"]["value"]
                    .as_object_mut()
                    .unwrap()
                    .remove(field);
                reject(&wrong);
            }
            for field in ["resolved_queue", "resolved_manager"] {
                for len in [0, 47, 49] {
                    let mut wrong = v.clone();
                    wrong["outcome"]["output"]["value"][field] = json!(vec![32; len]);
                    reject(&wrong);
                }
            }
            let mut wrong = v.clone();
            wrong["outcome"]["output"]["value"]["extra"] = json!(0);
            reject(&wrong);
            let mut wrong = v.clone();
            wrong["outcome"]["output"]["value"]["known_dest_count"] = json!(1);
            reject(&wrong);
            let mut wrong = v.clone();
            wrong["outcome"]["output"]["value"]["backout_count"] = json!("Undefined");
            reject(&wrong);
            let mut changed = r.clone();
            if let MqMqiOutcome::Completed {
                output: MqMqiOutput::Produced(p),
                ..
            } = &mut changed.outcome
            {
                match &mut p.descriptor {
                    MqMdValue::V1 { fields, .. } | MqMdValue::V2 { fields, .. } => {
                        fields.correl_id[0] ^= 1
                    }
                };
            }
            let mut wrong = as_value(&changed);
            wrong["host_result_digest"] = v["host_result_digest"].clone();
            reject(&wrong);
            let bytes = stored(&r);
            assert!(restore(&[bytes.as_slice(), b" null"].concat()).is_err());
            assert!(restore(&bytes[..bytes.len() - 1]).is_err());
            assert!(
                decode(
                    &bytes,
                    HostLimits::default(),
                    MqMqiLimits::default(),
                    bytes.len() - 1
                )
                .is_err()
            );
            let text = String::from_utf8(bytes).unwrap();
            assert!(
                restore(
                    text.replace(
                        "\"known_dest_count\":",
                        "\"known_dest_count\":\"UndefinedZos\",\"known_dest_count\":"
                    )
                    .as_bytes()
                )
                .is_err()
            );
        }
    }
    let old = result(MqMqiCall::Close, MqMqiOutput::NoOutput, true);
    let mut wrong = as_value(&old);
    wrong["schema_version"] = json!(PRODUCER_SCHEMA);
    reject(&wrong);
}
