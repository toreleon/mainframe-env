use super::*;

#[test]
fn every_other_shape_requires_fields_and_empty_records_deny_unknown_data() {
    let mut m = message();
    m.descriptor.expiry = MqExpiry::Unlimited;
    m.descriptor.priority = MqPriority::QueueDefault;
    let values = [
        result(
            MqMqiCall::Put,
            MqMqiOutput::Put {
                descriptor: m.descriptor.clone(),
                outcome: MqDeliveryOutcome::Pending,
            },
            false,
        ),
        result(
            MqMqiCall::PutOne,
            MqMqiOutput::Distribution(MqDistributionResult {
                items: vec![MqDistributionItemResult {
                    destination: "a".into(),
                    outcome: MqDeliveryOutcome::Accepted,
                }],
            }),
            true,
        ),
        result(
            MqMqiCall::InquireProperty,
            MqMqiOutput::Property(properties()[1].clone()),
            true,
        ),
        result(
            MqMqiCall::HandleToBuffer,
            MqMqiOutput::Buffer {
                descriptor: m.descriptor,
                bytes: vec![1],
                data_length: 1,
            },
            true,
        ),
        result(
            MqMqiCall::Inquire,
            MqMqiOutput::Attributes {
                integers: vec![1],
                characters: vec![255],
            },
            true,
        ),
        result(MqMqiCall::Begin, MqMqiOutput::UnitOfWork { unit: 1 }, true),
        result(
            MqMqiCall::SubscriptionRequest,
            MqMqiOutput::PublicationsRequested { count: 1 },
            true,
        ),
        result(MqMqiCall::Close, MqMqiOutput::NoOutput, true),
        MqMqiResult {
            call: MqMqiCall::Get,
            outcome: MqMqiOutcome::ReviewedStatus {
                status: MqReviewedStatus::from_symbols(MqMqiCall::Get, "MQCC_OK", "MQRC_NONE")
                    .unwrap(),
            },
        },
        MqMqiResult {
            call: MqMqiCall::Get,
            outcome: MqMqiOutcome::Pending(MqMqiPending::StatusMapping),
        },
        MqMqiResult {
            call: MqMqiCall::Get,
            outcome: MqMqiOutcome::UnknownOutcome,
        },
        MqMqiResult {
            call: MqMqiCall::Get,
            outcome: MqMqiOutcome::DuplicatePossible,
        },
        MqMqiResult {
            call: MqMqiCall::CallbackFunction,
            outcome: MqMqiOutcome::CallbackReturned {
                context: MqMqiOptions::ContractDefault,
            },
        },
    ];
    fn extras(value: &Value) -> Vec<Value> {
        let mut variants = vec![];
        match value {
            Value::Object(map) => {
                let mut extra = value.clone();
                extra["extra"] = json!(null);
                variants.push(extra);
                for (name, child) in map {
                    for replacement in extras(child) {
                        let mut extra = value.clone();
                        extra[name] = replacement;
                        variants.push(extra);
                    }
                }
            }
            Value::Array(items) => {
                for (i, child) in items.iter().enumerate() {
                    for replacement in extras(child) {
                        let mut extra = value.clone();
                        extra[i] = replacement;
                        variants.push(extra);
                    }
                }
            }
            _ => {}
        }
        variants
    }
    for value in values {
        let json = as_value(&value);
        for missing in omissions(&json) {
            reject(&missing);
        }
        for extra in extras(&json) {
            reject(&extra);
        }
    }
    let mut v = as_value(&got(message()));
    v["outcome"]["status"] = json!({"OkNone":null});
    reject(&v);
    v = as_value(&got(message()));
    v["outcome"]["output"]["message"]["persistence"] = json!({"NonPersistent":null});
    reject(&v);
}

#[test]
fn preflight_limits_each_typed_collection_and_aggregate_before_dto_allocation() {
    let mqi = MqMqiLimits::default();
    let host = HostLimits::default();
    let mut base = as_value(&got(message()));
    base["outcome"]["output"]["message"]["projection"]["properties"] = json!([]);
    base["outcome"]["output"]["message"]["projection"]["body"] = json!([1, 2, 3, 4]);
    assert!(
        budget::preflight(
            &serde_json::to_vec(&base).unwrap(),
            host,
            MqMqiLimits {
                message: MqMessageLimits {
                    body_bytes: 3,
                    ..Default::default()
                },
                ..mqi
            }
        )
        .is_err()
    );
    let mut v = as_value(&got(message()));
    v["outcome"]["output"]["message"]["projection"]["message_id"] = json!(vec![1; 65]);
    assert!(budget::preflight(&serde_json::to_vec(&v).unwrap(), host, mqi).is_err());
    let mut v = as_value(&result(
        MqMqiCall::InquireProperty,
        MqMqiOutput::Property(properties()[1].clone()),
        true,
    ));
    v["outcome"]["output"]["property"]["value"] = json!([1, 2, 3, 4]);
    assert!(
        budget::preflight(
            &serde_json::to_vec(&v).unwrap(),
            host,
            MqMqiLimits {
                message: MqMessageLimits {
                    property_value_bytes: 3,
                    ..Default::default()
                },
                ..mqi
            }
        )
        .is_err()
    );
    v = as_value(&got(message()));
    assert!(
        budget::preflight(
            &serde_json::to_vec(&v).unwrap(),
            host,
            MqMqiLimits {
                message: MqMessageLimits {
                    property_total_bytes: 1,
                    ..Default::default()
                },
                ..mqi
            }
        )
        .is_err()
    );
    let mut v = as_value(&result(
        MqMqiCall::PutOne,
        MqMqiOutput::Distribution(MqDistributionResult {
            items: vec![MqDistributionItemResult {
                destination: "a".into(),
                outcome: MqDeliveryOutcome::Accepted,
            }],
        }),
        true,
    ));
    v["outcome"]["output"]["items"] =
        json!([{"destination":"a","outcome":"Accepted"},{"destination":"b","outcome":"Pending"}]);
    assert!(
        budget::preflight(
            &serde_json::to_vec(&v).unwrap(),
            host,
            MqMqiLimits {
                message: MqMessageLimits {
                    distribution_items: 1,
                    ..Default::default()
                },
                ..mqi
            }
        )
        .is_err()
    );
    v = as_value(&result(
        MqMqiCall::Inquire,
        MqMqiOutput::Attributes {
            integers: vec![],
            characters: vec![1, 2],
        },
        true,
    ));
    assert!(
        budget::preflight(
            &serde_json::to_vec(&v).unwrap(),
            host,
            MqMqiLimits {
                attribute_bytes: 1,
                ..mqi
            }
        )
        .is_err()
    );
    v = as_value(&result(
        MqMqiCall::HandleToBuffer,
        MqMqiOutput::Buffer {
            descriptor: message().descriptor,
            bytes: vec![1, 2],
            data_length: 2,
        },
        true,
    ));
    assert!(
        budget::preflight(
            &serde_json::to_vec(&v).unwrap(),
            host,
            MqMqiLimits {
                buffer_bytes: 1,
                ..mqi
            }
        )
        .is_err()
    );
    let mut v = as_value(&got(message()));
    v["outcome"]["output"]["message"]["projection"]["properties"][0]["value"][0] = json!(256);
    reject(&v);
    v = as_value(&got(message()));
    v["host_result_digest"] = json!(vec![0; 31]);
    reject(&v);
    v = as_value(&got(message()));
    v["host_result_digest"] = json!(vec![0; 33]);
    reject(&v);
    v = as_value(&got(message()));
    v["outcome"]["output"]["message"]["priority"]["value"] = json!(1.5);
    reject(&v);
    let mut m = message();
    m.properties = vec![properties()[1].clone(), properties()[1].clone()];
    let mut v = as_value(&got(message()));
    v["outcome"]["output"]["message"]["projection"]["properties"] = json!([
        {"name":"bytes","kind":"byte-string","value":[0,255,10]},
        {"name":"bytes","kind":"byte-string","value":[0,255,10]}
    ]);
    coherent(&mut v, &got(m));
    reject(&v);
}
