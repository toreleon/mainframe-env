use super::*;
use mainframe_env_host_api::mq_status::{MqStatusReview, mq_status_calls};
use serde_json::{Value, json};

mod bounds;
mod full_message;
mod historical_handles;
mod producer;
mod property;
mod reviewed_output;
mod rfh2;

const BYTES: usize = 8 << 20;
fn host(value: &MqMqiResult, limits: MqMqiLimits) -> Result<HostResult, HostProblem> {
    Ok(HostResult::MqMqi(MqMqiHostResult {
        result: value.clone(),
        limits,
    }))
}
fn stored(value: &MqMqiResult) -> Vec<u8> {
    encode(value, HostLimits::default(), MqMqiLimits::default(), BYTES).unwrap()
}
fn restore(bytes: &[u8]) -> Result<MqMqiResult, ReplayError> {
    decode(bytes, HostLimits::default(), MqMqiLimits::default(), BYTES)
}
fn roundtrip(value: &MqMqiResult) {
    let limits = MqMqiLimits::default();
    let before = mq_mqi_result_bytes(value, limits).unwrap();
    let digest = canonical_result_digest(&host(value, limits)).unwrap();
    let size = canonical_result_size(&host(value, limits), limits.canonical_bytes).unwrap();
    let bytes = stored(value);
    let restored = restore(&bytes).unwrap();
    assert_eq!(restored, *value);
    assert_eq!(mq_mqi_result_bytes(&restored, limits).unwrap(), before);
    assert_eq!(
        canonical_result_digest(&host(&restored, limits)).unwrap(),
        digest
    );
    assert_eq!(
        canonical_result_size(&host(&restored, limits), limits.canonical_bytes).unwrap(),
        size
    );
    assert_eq!(stored(&restored), bytes);
    assert_ne!(
        mq_mqi_result_digest(value, limits).unwrap(),
        digest,
        "full host identity is distinct"
    );
}
fn property(kind: MqPropertyType, name: &str, value: &[u8]) -> MqMessageProperty {
    MqMessageProperty {
        name: name.into(),
        kind,
        value: value.into(),
    }
}
fn properties() -> Vec<MqMessageProperty> {
    vec![
        property(MqPropertyType::Boolean, "bool", &[1, 0, 0, 0]),
        property(MqPropertyType::ByteString, "bytes", &[0, 255, 10]),
        property(MqPropertyType::Int8, "i8", &[255]),
        property(MqPropertyType::Int16, "i16", &[0, 255]),
        property(MqPropertyType::Int32, "i32", &[0, 1, 255, 0]),
        property(MqPropertyType::Int64, "i64", &[255; 8]),
        property(MqPropertyType::Float32, "f32", &[255; 4]),
        property(MqPropertyType::Float64, "f64", &[255; 8]),
        property(MqPropertyType::String, "string", &[0, 255, 10]),
        property(MqPropertyType::Null, "null", &[]),
    ]
}
fn message() -> MqMessage {
    MqMessage {
        descriptor: MqMessageDescriptor {
            identifiers: MqMessageIdentifiers {
                message_id: Some(vec![0, 255]),
                correlation_id: Some(vec![3, 0]),
                group_id: Some(vec![7]),
            },
            format: Some("BYTES".into()),
            expiry: MqExpiry::RelativeHostTicks(13),
            persistence: MqPersistence::NonPersistent,
            priority: MqPriority::PendingNumeric(-19),
            ordering: MqMessageOrdering {
                group_sequence: Some(2),
                last_in_group: true,
                segment_offset: Some(1),
                last_segment: true,
                segmentation_allowed: true,
            },
        },
        body: vec![0, 255, 10],
        properties: properties(),
    }
}
fn result(call: MqMqiCall, output: MqMqiOutput, completed: bool) -> MqMqiResult {
    MqMqiResult {
        call,
        outcome: if completed {
            MqMqiOutcome::Completed {
                status: MqMqiStatus::OkNone,
                output,
            }
        } else {
            MqMqiOutcome::StatusPending { output }
        },
    }
}
fn got(value: MqMessage) -> MqMqiResult {
    result(
        MqMqiCall::Get,
        MqMqiOutput::Got {
            disposition: MqGetDisposition::Message(MqTruncationDisposition::Complete {
                length: value.body.len(),
            }),
            message: Some(value),
            cursor: Some(19),
        },
        true,
    )
}
fn as_value(value: &MqMqiResult) -> Value {
    serde_json::from_slice(&stored(value)).unwrap()
}
fn reject(value: &Value) {
    assert!(
        restore(&serde_json::to_vec(value).unwrap()).is_err(),
        "unexpected acceptance: {value}"
    );
}
fn coherent(value: &mut Value, result: &MqMqiResult) {
    value["host_result_digest"] =
        json!(canonical_result_digest(&host(result, MqMqiLimits::default())).unwrap());
}

#[test]
fn all_non_handle_outputs_preserve_exact_completed_and_pending_observations() {
    let m = message();
    let outputs = vec![
        (
            MqMqiCall::Get,
            MqMqiOutput::Got {
                disposition: MqGetDisposition::Message(MqTruncationDisposition::Complete {
                    length: 3,
                }),
                message: Some(m.clone()),
                cursor: Some(1),
            },
        ),
        (
            MqMqiCall::Put,
            MqMqiOutput::Put {
                descriptor: m.descriptor.clone(),
                outcome: MqDeliveryOutcome::Accepted,
            },
        ),
        (
            MqMqiCall::PutOne,
            MqMqiOutput::Distribution(MqDistributionResult {
                items: vec![
                    MqDistributionItemResult {
                        destination: "first".into(),
                        outcome: MqDeliveryOutcome::Accepted,
                    },
                    MqDistributionItemResult {
                        destination: "second".into(),
                        outcome: MqDeliveryOutcome::Pending,
                    },
                ],
            }),
        ),
        (
            MqMqiCall::InquireProperty,
            MqMqiOutput::Property(properties()[1].clone()),
        ),
        (
            MqMqiCall::BufferToHandle,
            MqMqiOutput::Buffer {
                descriptor: m.descriptor.clone(),
                bytes: vec![0, 255],
                data_length: 2,
            },
        ),
        (
            MqMqiCall::HandleToBuffer,
            MqMqiOutput::Buffer {
                descriptor: m.descriptor.clone(),
                bytes: vec![],
                data_length: 0,
            },
        ),
        (
            MqMqiCall::Inquire,
            MqMqiOutput::Attributes {
                integers: vec![i32::MIN, 2, 2, i32::MAX],
                characters: vec![0, 255],
            },
        ),
        (
            MqMqiCall::Begin,
            MqMqiOutput::UnitOfWork {
                unit: i64::MAX as u64,
            },
        ),
        (
            MqMqiCall::SubscriptionRequest,
            MqMqiOutput::PublicationsRequested { count: 256 },
        ),
        (MqMqiCall::Close, MqMqiOutput::NoOutput),
    ];
    for (call, output) in outputs {
        for completed in [false, true] {
            roundtrip(&result(call, output.clone(), completed));
        }
    }
    for call in [MqMqiCall::Back, MqMqiCall::Begin, MqMqiCall::Commit] {
        roundtrip(&MqMqiResult {
            call,
            outcome: MqMqiOutcome::Completed {
                status: MqMqiStatus::FailedEnvironment,
                output: MqMqiOutput::NoOutput,
            },
        });
    }
}

#[test]
fn descriptor_policy_is_exact_including_abstract_pending_metadata() {
    for persistence in [
        MqPersistence::Persistent,
        MqPersistence::NonPersistent,
        MqPersistence::QueueDefault,
        MqPersistence::PendingSource,
    ] {
        for expiry in [
            MqExpiry::Unlimited,
            MqExpiry::RelativeHostTicks(u64::MAX),
            MqExpiry::PendingSource,
        ] {
            for priority in [
                MqPriority::QueueDefault,
                MqPriority::PendingNumeric(i32::MIN),
                MqPriority::PendingNumeric(i32::MAX),
            ] {
                let mut m = message();
                m.descriptor.persistence = persistence;
                m.descriptor.expiry = expiry;
                m.descriptor.priority = priority;
                roundtrip(&got(m));
            }
        }
    }
    for p in properties() {
        roundtrip(&result(
            MqMqiCall::InquireProperty,
            MqMqiOutput::Property(p),
            true,
        ));
    }
    let mut empty = message();
    empty.body.clear();
    empty.properties.clear();
    empty.descriptor.identifiers = Default::default();
    empty.descriptor.ordering = Default::default();
    empty.descriptor.format = None;
    roundtrip(&got(empty));
}

#[test]
fn truncation_lengths_and_uncertainty_are_not_reclassified_as_completed() {
    for disposition in [
        MqTruncationDisposition::RejectedRetained {
            required: 9,
            copied: 3,
        },
        MqTruncationDisposition::AcceptedRemoved {
            required: 9,
            copied: 3,
        },
        MqTruncationDisposition::AcceptedBrowsed {
            required: 9,
            copied: 3,
        },
    ] {
        roundtrip(&result(
            MqMqiCall::Get,
            MqMqiOutput::Got {
                disposition: MqGetDisposition::Message(disposition),
                message: Some(message()),
                cursor: Some(7),
            },
            false,
        ));
    }
    for disposition in [
        MqGetDisposition::NoMessage,
        MqGetDisposition::WaitExpired,
        MqGetDisposition::UnknownOutcome,
    ] {
        roundtrip(&result(
            MqMqiCall::Get,
            MqMqiOutput::Got {
                disposition,
                message: None,
                cursor: None,
            },
            false,
        ));
    }
    for outcome in [
        MqDeliveryOutcome::Rejected,
        MqDeliveryOutcome::UnknownOutcome,
        MqDeliveryOutcome::DuplicatePossible,
    ] {
        roundtrip(&result(
            MqMqiCall::Put,
            MqMqiOutput::Put {
                descriptor: message().descriptor,
                outcome: outcome.clone(),
            },
            false,
        ));
        roundtrip(&result(
            MqMqiCall::PutOne,
            MqMqiOutput::Distribution(MqDistributionResult {
                items: vec![MqDistributionItemResult {
                    destination: "only".into(),
                    outcome,
                }],
            }),
            false,
        ));
    }
    for call in MqMqiCall::ALL {
        for outcome in [
            MqMqiOutcome::UnknownOutcome,
            MqMqiOutcome::DuplicatePossible,
            MqMqiOutcome::Pending(MqMqiPending::PublicDispatch),
            MqMqiOutcome::Pending(MqMqiPending::StructureAndWireMapping),
            MqMqiOutcome::Pending(MqMqiPending::SelectorAndAttributeMapping),
            MqMqiOutcome::Pending(MqMqiPending::StatusMapping),
            MqMqiOutcome::Pending(MqMqiPending::TrustedContextAndAuthorization),
            MqMqiOutcome::Pending(MqMqiPending::ExternalUnitOfWork),
            MqMqiOutcome::Pending(MqMqiPending::CallbackContext),
        ] {
            roundtrip(&MqMqiResult { call, outcome });
        }
    }
}

#[test]
fn callback_storage_does_not_admit_a_public_host_effect() {
    for context in [
        MqMqiOptions::ContractDefault,
        MqMqiOptions::PendingStructure {
            requested_version: None,
        },
        MqMqiOptions::PendingStructure {
            requested_version: Some(i32::MIN),
        },
    ] {
        let value = MqMqiResult {
            call: MqMqiCall::CallbackFunction,
            outcome: MqMqiOutcome::CallbackReturned { context },
        };
        roundtrip(&value);
        assert_eq!(
            host(&restore(&stored(&value)).unwrap(), MqMqiLimits::default())
                .unwrap()
                .validate(HostLimits::default()),
            Err(HostProblem::Malformed)
        );
    }
}

#[test]
fn reviewed_status_uses_the_sole_admitted_table_and_pending_pairs_stay_pending() {
    let mut admitted = 0;
    let mut pending = 0;
    for call in mq_status_calls() {
        for pair in call.pairs() {
            let status = MqReviewedStatus::from_symbols(
                call.call,
                pair.completion.symbol(),
                pair.reason_symbol,
            );
            if pair.review == MqStatusReview::Admitted {
                roundtrip(&MqMqiResult {
                    call: call.call,
                    outcome: MqMqiOutcome::ReviewedStatus {
                        status: status.unwrap(),
                    },
                });
                admitted += 1;
            } else {
                assert!(status.is_err());
                let mut v = as_value(&MqMqiResult {
                    call: call.call,
                    outcome: MqMqiOutcome::UnknownOutcome,
                });
                v["outcome"] = json!({"kind":"ReviewedStatus","completion":pair.completion.symbol(),"reason":pair.reason_symbol});
                assert!(matches!(
                    restore(&serde_json::to_vec(&v).unwrap()),
                    Err(ReplayError::Status(_))
                ));
                pending += 1;
            }
        }
    }
    assert!(admitted > 0 && pending > 0);
}

fn omissions(value: &Value) -> Vec<Value> {
    let mut variants = vec![];
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                let mut v = value.clone();
                v.as_object_mut().unwrap().remove(key);
                variants.push(v);
                for new in omissions(child) {
                    let mut v = value.clone();
                    v[key] = new;
                    variants.push(v);
                }
            }
        }
        Value::Array(items) => {
            for (i, child) in items.iter().enumerate() {
                for new in omissions(child) {
                    let mut v = value.clone();
                    v[i] = new;
                    variants.push(v);
                }
            }
        }
        _ => {}
    }
    variants
}
#[test]
fn every_stored_field_including_nullable_fields_is_required_and_unknowns_fail() {
    let mut empty = message();
    empty.descriptor.identifiers = Default::default();
    empty.descriptor.ordering = Default::default();
    empty.descriptor.format = None;
    let values = [
        got(message()),
        got(empty),
        result(
            MqMqiCall::Get,
            MqMqiOutput::Got {
                disposition: MqGetDisposition::NoMessage,
                message: None,
                cursor: None,
            },
            false,
        ),
        MqMqiResult {
            call: MqMqiCall::CallbackFunction,
            outcome: MqMqiOutcome::CallbackReturned {
                context: MqMqiOptions::PendingStructure {
                    requested_version: None,
                },
            },
        },
    ];
    for value in values {
        let original = as_value(&value);
        for missing in omissions(&original) {
            reject(&missing);
        }
        let mut v = original.clone();
        v["extra"] = json!(null);
        reject(&v);
        let mut v = original.clone();
        v["outcome"]["extra"] = json!(null);
        reject(&v);
    }
    let mut v = as_value(&got(message()));
    v["outcome"]["output"]["message"]["projection"]["properties"][0]["extra"] = json!(0);
    reject(&v);
    let mut v = as_value(&MqMqiResult {
        call: MqMqiCall::Stat,
        outcome: MqMqiOutcome::UnknownOutcome,
    });
    v["outcome"]["extra"] = json!(0);
    reject(&v);
}

#[test]
fn duplicate_fields_escaped_duplicates_tags_trailing_and_unknown_schema_fail() {
    let value = got(message());
    let bytes = stored(&value);
    let text = String::from_utf8(bytes.clone()).unwrap();
    for name in [
        "schema_version",
        "call",
        "kind",
        "message_id",
        "cursor",
        "body",
        "persistence",
        "value",
    ] {
        let token = format!("\"{name}\":");
        let duplicated = text.replacen(&token, &format!("\"{name}\":null,{token}"), 1);
        assert_ne!(duplicated, text);
        assert!(restore(duplicated.as_bytes()).is_err());
    }
    let escaped = text.replacen("\"call\":", "\"\\u0063all\":\"MQGET\",\"call\":", 1);
    assert!(restore(escaped.as_bytes()).is_err());
    for suffix in ["{}", "true", "x"] {
        let mut tail = bytes.clone();
        tail.extend_from_slice(suffix.as_bytes());
        assert!(restore(&tail).is_err());
    }
    for field in ["schema_version", "call"] {
        let mut v = as_value(&value);
        v[field] = json!("future");
        reject(&v);
    }
    let mut v = as_value(&value);
    v["outcome"]["kind"] = json!("future");
    reject(&v);
    let mut v = as_value(&value);
    v["outcome"]["output"]["kind"] = json!("future");
    reject(&v);
    assert!(restore(b"\xff").is_err());
}

#[test]
fn coherent_invalid_results_fail_frozen_validation_not_only_digest_check() {
    let mut bad = got(message());
    bad.call = MqMqiCall::Put;
    let mut v = as_value(&got(message()));
    v["call"] = json!("MQPUT");
    coherent(&mut v, &bad);
    reject(&v);
    let mut bad = got(message());
    if let MqMqiOutcome::Completed {
        output: MqMqiOutput::Got { disposition, .. },
        ..
    } = &mut bad.outcome
    {
        *disposition = MqGetDisposition::Message(MqTruncationDisposition::Complete { length: 4 });
    }
    let mut v = as_value(&got(message()));
    v["outcome"]["output"]["disposition"]["length"] = json!(4);
    coherent(&mut v, &bad);
    reject(&v);
    let original = result(
        MqMqiCall::HandleToBuffer,
        MqMqiOutput::Buffer {
            descriptor: message().descriptor,
            bytes: vec![0, 255],
            data_length: 2,
        },
        true,
    );
    let mut bad = original.clone();
    if let MqMqiOutcome::Completed {
        output: MqMqiOutput::Buffer { data_length, .. },
        ..
    } = &mut bad.outcome
    {
        *data_length = 3;
    }
    let mut v = as_value(&original);
    v["outcome"]["output"]["data_length"] = json!(3);
    coherent(&mut v, &bad);
    reject(&v);
    let original = result(
        MqMqiCall::InquireProperty,
        MqMqiOutput::Property(properties()[0].clone()),
        true,
    );
    let mut bad = original.clone();
    if let MqMqiOutcome::Completed {
        output: MqMqiOutput::Property(p),
        ..
    } = &mut bad.outcome
    {
        p.value.pop();
    }
    let mut v = as_value(&original);
    v["outcome"]["output"]["property"]["value"] = json!([1, 0, 0]);
    coherent(&mut v, &bad);
    reject(&v);
    for value in [
        bad,
        MqMqiResult {
            call: MqMqiCall::Stat,
            outcome: MqMqiOutcome::Completed {
                status: MqMqiStatus::OkNone,
                output: MqMqiOutput::NoOutput,
            },
        },
    ] {
        assert!(encode(&value, HostLimits::default(), MqMqiLimits::default(), BYTES).is_err());
    }
}

#[test]
fn policy_projection_conflicts_and_descriptor_payload_smuggling_are_rejected() {
    let mut v = as_value(&got(message()));
    v["outcome"]["output"]["message"]["projection"]["expiry_ticks"] = json!(13);
    reject(&v);
    let value = result(
        MqMqiCall::Put,
        MqMqiOutput::Put {
            descriptor: message().descriptor,
            outcome: MqDeliveryOutcome::Accepted,
        },
        true,
    );
    for field in ["body", "properties"] {
        let mut v = as_value(&value);
        v["outcome"]["output"]["descriptor"]["projection"][field] = if field == "body" {
            json!([1])
        } else {
            json!([{"name":"extra","kind":"byte-string","value":[1]}])
        };
        reject(&v);
    }
}

#[test]
fn full_host_digest_binds_outcome_contents_and_exact_mqi_profile() {
    let value = got(message());
    let original = as_value(&value);
    let mut v = original.clone();
    v["outcome"]["output"]["message"]["projection"]["body"][0] = json!(1);
    assert_eq!(
        restore(&serde_json::to_vec(&v).unwrap()),
        Err(ReplayError::DigestMismatch)
    );
    let mut v = original.clone();
    v["outcome"]["kind"] = json!("StatusPending");
    v["outcome"].as_object_mut().unwrap().remove("status");
    assert_eq!(
        restore(&serde_json::to_vec(&v).unwrap()),
        Err(ReplayError::DigestMismatch)
    );
    let mut v = original.clone();
    v["host_result_digest"] = json!(mq_mqi_result_digest(&value, MqMqiLimits::default()).unwrap());
    assert_eq!(
        restore(&serde_json::to_vec(&v).unwrap()),
        Err(ReplayError::DigestMismatch)
    );
    let mqi = MqMqiLimits {
        selectors: 255,
        ..Default::default()
    };
    assert_eq!(
        decode(&stored(&value), HostLimits::default(), mqi, BYTES),
        Err(ReplayError::DigestMismatch)
    );
}

#[test]
fn historical_special_connections_are_explicitly_refused() {
    for connection in [MqHconn::Default, MqHconn::Unassociated] {
        for completed in [false, true] {
            assert_eq!(
                encode(
                    &result(
                        MqMqiCall::Connect,
                        MqMqiOutput::Connected(connection),
                        completed
                    ),
                    HostLimits::default(),
                    MqMqiLimits::default(),
                    BYTES
                ),
                Err(ReplayError::Unsupported(
                    ReplayPending::HistoricalSpecialConnection
                ))
            );
        }
    }
}

#[test]
fn finite_byte_profiles_sql_identities_and_collection_preflight_fail_closed() {
    let value = got(message());
    let bytes = stored(&value);
    assert_eq!(
        encode(
            &value,
            HostLimits::default(),
            MqMqiLimits::default(),
            bytes.len()
        )
        .unwrap(),
        bytes
    );
    assert!(
        encode(
            &value,
            HostLimits::default(),
            MqMqiLimits::default(),
            bytes.len() - 1
        )
        .is_err()
    );
    assert!(
        decode(
            &bytes,
            HostLimits::default(),
            MqMqiLimits::default(),
            bytes.len() - 1
        )
        .is_err()
    );
    for cap in [0, usize::MAX] {
        assert!(encode(&value, HostLimits::default(), MqMqiLimits::default(), cap).is_err());
        assert!(decode(&bytes, HostLimits::default(), MqMqiLimits::default(), cap).is_err());
    }
    for mqi in [
        MqMqiLimits {
            canonical_bytes: 1,
            ..Default::default()
        },
        MqMqiLimits {
            selectors: usize::MAX,
            ..Default::default()
        },
    ] {
        assert!(encode(&value, HostLimits::default(), mqi, BYTES).is_err());
        assert!(decode(&bytes, HostLimits::default(), mqi, BYTES).is_err());
    }
    for host in [
        HostLimits {
            max_fields: 1,
            ..Default::default()
        },
        HostLimits {
            max_record_bytes: 2,
            ..Default::default()
        },
        HostLimits {
            max_name_bytes: 1,
            ..Default::default()
        },
        HostLimits {
            max_state_bytes: 1,
            ..Default::default()
        },
        HostLimits {
            max_records: usize::MAX,
            ..Default::default()
        },
    ] {
        assert!(encode(&value, host, MqMqiLimits::default(), BYTES).is_err());
        assert!(decode(&bytes, host, MqMqiLimits::default(), BYTES).is_err());
    }
    for id in [0, (i64::MAX as u64) + 1, u64::MAX] {
        let value = result(
            MqMqiCall::Commit,
            MqMqiOutput::UnitOfWork { unit: id },
            true,
        );
        assert!(encode(&value, HostLimits::default(), MqMqiLimits::default(), BYTES).is_err());
        let mut v = as_value(&result(
            MqMqiCall::Commit,
            MqMqiOutput::UnitOfWork { unit: 1 },
            true,
        ));
        v["outcome"]["output"]["unit"] = json!(id);
        coherent(&mut v, &value);
        reject(&v);
        let mut v = as_value(&got(message()));
        v["outcome"]["output"]["cursor"] = json!(id);
        reject(&v);
    }
    let mut v = as_value(&got(message()));
    v["outcome"]["output"]["message"]["projection"]["properties"] =
        json!(vec![json!({"name":"x","kind":"null","value":[]}); 129]);
    assert!(
        budget::preflight(
            &serde_json::to_vec(&v).unwrap(),
            HostLimits::default(),
            MqMqiLimits::default()
        )
        .is_err()
    );
    let mut v = as_value(&result(
        MqMqiCall::Inquire,
        MqMqiOutput::Attributes {
            integers: vec![],
            characters: vec![],
        },
        true,
    ));
    v["outcome"]["output"]["integers"] = json!(vec![0; 257]);
    assert!(
        budget::preflight(
            &serde_json::to_vec(&v).unwrap(),
            HostLimits::default(),
            MqMqiLimits::default()
        )
        .is_err()
    );
    let nested = format!("{}0{}", "[".repeat(25), "]".repeat(25));
    assert!(restore(nested.as_bytes()).is_err());
}

#[test]
fn prior_cold_live_golden_bytes_and_default_policies_are_unchanged() {
    use crate::{
        MqDeliveryKernel, MqDeliveryLimits, MqLocalQueueUsage, MqObjectCatalog, MqObjectDefinition,
        MqObjectName, MqQueueManagerDefinition,
    };
    let n = |s: &str| MqObjectName::new(s).unwrap();
    let catalog = MqObjectCatalog::new(
        MqQueueManagerDefinition {
            name: n("QM"),
            default_transmission_queue: None,
        },
        vec![MqObjectDefinition::LocalQueue {
            name: n("A"),
            usage: MqLocalQueueUsage::Normal,
            trigger_process: None,
        }],
        Default::default(),
    )
    .unwrap();
    let k = MqDeliveryKernel::new(
        &catalog,
        MqDeliveryLimits::default(),
        MqMessageLimits::default(),
        MqPersistence::Persistent,
    )
    .unwrap();
    let cold = br#"{"schema_version":"mainframe-env.mq-delivery@1","manager":"QM","tick":0,"next_id":1,"next_cursor":1,"queues":[{"name":"A","messages":[]}],"finalized":[]}"#;
    let live = br#"{"schema_version":"mainframe-env.mq-delivery-live@1","manager":"QM","default_persistent":true,"tick":0,"next_id":1,"next_cursor":1,"queues":[{"name":"A","messages":[]}],"pending":[],"finalized":[],"cursors":[]}"#;
    assert_eq!(k.encode().unwrap(), cold);
    assert_eq!(k.encode_live_checkpoint().unwrap(), live);
    assert_eq!(
        MqDeliveryKernel::decode(
            cold,
            &catalog,
            MqDeliveryLimits::default(),
            MqMessageLimits::default(),
            MqPersistence::Persistent
        )
        .unwrap()
        .encode()
        .unwrap(),
        cold
    );
    let mut m = message();
    m.descriptor.priority = MqPriority::QueueDefault;
    m.descriptor.expiry = MqExpiry::Unlimited;
    m.descriptor.persistence = MqPersistence::Persistent;
    m.descriptor.identifiers.group_id = None;
    m.descriptor.ordering = Default::default();
    let original = m.clone();
    let mut k = k;
    k.put_one(&catalog, &n("A"), m.clone(), None).unwrap();
    let cold = k.encode().unwrap();
    let live = k.encode_live_checkpoint().unwrap();
    let recovered = MqDeliveryKernel::decode(
        &cold,
        &catalog,
        MqDeliveryLimits::default(),
        MqMessageLimits::default(),
        MqPersistence::Persistent,
    )
    .unwrap();
    assert_eq!(recovered.encode().unwrap(), cold);
    let resumed = MqDeliveryKernel::decode_live_checkpoint(
        &live,
        &catalog,
        MqDeliveryLimits::default(),
        MqMessageLimits::default(),
        MqPersistence::Persistent,
    )
    .unwrap();
    assert_eq!(resumed.encode_live_checkpoint().unwrap(), live);
    assert_eq!(
        ReplayMessage::from_message(&m)
            .unwrap()
            .into_message()
            .unwrap(),
        original
    );
    m.descriptor.persistence = MqPersistence::NonPersistent;
    k.put_one(&catalog, &n("A"), m, None).unwrap();
    let cold = MqDeliveryKernel::decode(
        &k.encode().unwrap(),
        &catalog,
        MqDeliveryLimits::default(),
        MqMessageLimits::default(),
        MqPersistence::Persistent,
    )
    .unwrap();
    let live = MqDeliveryKernel::decode_live_checkpoint(
        &k.encode_live_checkpoint().unwrap(),
        &catalog,
        MqDeliveryLimits::default(),
        MqMessageLimits::default(),
        MqPersistence::Persistent,
    )
    .unwrap();
    assert_eq!(cold.depth(&n("A")), Some(1));
    assert_eq!(live.depth(&n("A")), Some(2));
}
