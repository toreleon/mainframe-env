use super::*;
use crate::mq_md_value::tests::value;
use crate::mq_status::MqReviewedStatus;
use crate::*;
mod edges;
mod identity;

fn owner() -> MqHandleOwner {
    MqHandleOwner {
        environment: MqHostEnvironment::ZosBatch,
        host_id: 1,
        process_id: 2,
        thread_id: 3,
        task_id: 4,
        syncpoint_epoch: 5,
    }
}
fn observed(role: &str, slot: u32) -> MqHandleObservation {
    serde_json::from_str(&format!(
        r#"{{"role":"{role}","registry":77,"slot":{slot},"generation":3,"epoch":4}}"#
    ))
    .unwrap()
}
fn connection() -> MqHconn {
    observed("Connection", 1).historical_connection().unwrap()
}
fn object() -> MqHobj {
    observed("Object", 2).historical_object().unwrap()
}
fn envelope(request: MqMqiRequest) -> MqMqiRequestEnvelope {
    MqMqiRequestEnvelope {
        context: MqMqiContext {
            owner: owner(),
            syncpoint_owner: MqSyncpointOwner::QueueManager,
        },
        limits: MqMqiLimits::default(),
        request,
    }
}
fn message(v2: bool) -> MqFullMessage {
    MqFullMessage {
        descriptor: value(v2, false),
        body: vec![0, 255, 10],
        properties: vec![MqMessageProperty {
            name: "bytes".into(),
            kind: MqPropertyType::ByteString,
            value: vec![0, 255],
        }],
    }
}
fn put(v2: bool) -> MqMqiFullPut {
    MqMqiFullPut {
        message: message(v2),
        message_handle: None,
        context: MqMqiMessageContext::Default,
        options: MqMqiOptions::ContractDefault,
        unit: MqMqiUnitOfWork::NoSyncpoint,
    }
}
fn get(v2: bool) -> MqMqiFullGet {
    MqMqiFullGet {
        connection: connection(),
        object: object(),
        descriptor: value(v2, false),
        mode: MqGetMode::Remove,
        wait: MqWait::NoWait,
        truncation: MqTruncation::Reject,
        buffer_capacity: 3,
        message_handle: None,
        options: MqMqiOptions::ContractDefault,
        unit: MqMqiUnitOfWork::NoSyncpoint,
    }
}
fn got(v2: bool, disposition: MqGetDisposition, data_length: Option<i32>) -> MqMqiOutput {
    MqMqiOutput::FullGot {
        disposition,
        message: Some(message(v2)),
        data_length,
        cursor: None,
    }
}
fn result(call: MqMqiCall, output: MqMqiOutput) -> MqMqiResult {
    MqMqiResult {
        call,
        outcome: MqMqiOutcome::StatusPending { output },
    }
}
#[test]
fn full_message_requests_keep_original_call_registry_and_explicit_pending_execution() {
    assert_eq!(MqMqiCall::ALL.len(), 26);
    for v2 in [false, true] {
        for (request, call) in [
            (MqMqiRequest::FullGet(get(v2)), MqMqiCall::Get),
            (
                MqMqiRequest::FullPut {
                    connection: connection(),
                    object: object(),
                    put: put(v2),
                },
                MqMqiCall::Put,
            ),
            (
                MqMqiRequest::FullPutOne {
                    connection: connection(),
                    lookup: MqRouteLookup::Queue {
                        name: MqRouteName::new("QUEUE").unwrap(),
                        manager: None,
                        dynamic_pattern: None,
                    },
                    alternate_user: None,
                    put: put(v2),
                },
                MqMqiCall::PutOne,
            ),
        ] {
            let e = envelope(request);
            assert_eq!(e.request.call(), call);
            assert_eq!(e.request.call().source(), call.source());
            assert_eq!(e.validate(), Ok(()));
            assert_eq!(e.review(), Ok(MqMqiPending::StructureAndWireMapping));
            assert!(
                mq_mqi_request_bytes(&e)
                    .unwrap()
                    .windows(9)
                    .any(|b| b == b"MqMdValue")
            );
        }
        let registry = MqHandleRegistry::new(4, 8).unwrap();
        assert!(registry.validate_connection(owner(), connection()).is_err());
        assert!(
            registry
                .validate(
                    owner(),
                    connection(),
                    MqHandle::Object(object()),
                    MqHandleKind::Object
                )
                .is_err()
        );
        let req = envelope(MqMqiRequest::FullPut {
            connection: connection(),
            object: object(),
            put: put(v2),
        });
        let digest = mq_mqi_request_digest(&req).unwrap();
        let mut changed = req.clone();
        if let MqMqiRequest::FullPut { put, .. } = &mut changed.request {
            put.message.body.reverse();
        }
        assert_ne!(mq_mqi_request_digest(&changed).unwrap(), digest);
    }
}
#[test]
fn full_bounds_property_authority_and_context_unit_are_fail_closed() {
    let mut m = message(true);
    assert_eq!(m.validate(MqMessageLimits::default()), Ok(())); // unknown signed MD observations are not legal-call admission
    m.properties.push(m.properties[0].clone());
    assert!(m.validate(MqMessageLimits::default()).is_err());
    m = message(true);
    m.properties[0].kind = MqPropertyType::Int32;
    assert!(m.validate(MqMessageLimits::default()).is_err());
    m = message(true);
    for field in ["body", "properties", "total", "identifier", "format"] {
        let mut limits = MqMessageLimits::default();
        match field {
            "body" => limits.body_bytes = 2,
            "properties" => {
                m = message(true);
                m.properties.push(MqMessageProperty {
                    name: "x".into(),
                    kind: MqPropertyType::Null,
                    value: vec![],
                });
                limits.properties = 1;
            }
            "total" => limits.property_total_bytes = 1,
            "identifier" => limits.identifier_bytes = 23,
            _ => limits.format_bytes = 7,
        }
        assert!(m.validate(limits).is_err());
        m = message(true);
    }
    let mut request = get(true);
    request.buffer_capacity = usize::MAX;
    assert!(envelope(MqMqiRequest::FullGet(request)).validate().is_err());
    let mut p = put(true);
    p.unit = MqMqiUnitOfWork::Local { unit: u64::MAX };
    assert!(
        envelope(MqMqiRequest::FullPut {
            connection: connection(),
            object: object(),
            put: p
        })
        .validate()
        .is_err()
    );
    let mut p = put(true);
    p.context = MqMqiMessageContext::PassAllPending { source: object() };
    assert_eq!(
        envelope(MqMqiRequest::FullPut {
            connection: connection(),
            object: object(),
            put: p
        })
        .review(),
        Ok(MqMqiPending::TrustedContextAndAuthorization)
    );
    let host = MqMqiHostRequest {
        envelope: envelope(MqMqiRequest::FullGet(get(false))),
        mutation: Mutation {
            sequence: 1,
            idempotency_key: mainframe_env_execution_api::IdempotencyKey::new(
                "full",
                mainframe_env_execution_api::InvocationLimits::default(),
            )
            .unwrap(),
            transaction: None,
        },
    };
    let mut limits = HostLimits::default();
    limits.max_record_bytes = 47;
    assert!(host.validate(limits).is_err());
}
#[test]
fn full_get_lengths_truncation_status_and_original_capacity_are_lossless() {
    for v2 in [false, true] {
        let req = envelope(MqMqiRequest::FullGet(get(v2)));
        let complete = MqGetDisposition::Message(MqTruncationDisposition::Complete { length: 3 });
        let output = got(v2, complete, Some(3));
        let status =
            MqReviewedStatus::from_symbols(MqMqiCall::Get, "MQCC_OK", "MQRC_NONE").unwrap();
        assert!(MqMqiResult::reviewed_output(status, output.clone(), &req).is_ok());
        for n in [None, Some(-1), Some(0), Some(2), Some(4)] {
            assert!(
                result(MqMqiCall::Get, got(v2, complete, n))
                    .validate(req.limits)
                    .is_err()
            );
        }
        let mut r = get(v2);
        r.buffer_capacity = 2;
        assert!(
            result(MqMqiCall::Get, output)
                .validate_reviewed_output_for(&MqMqiRequest::FullGet(r))
                .is_err()
        );
        for (t, reason, accept, browse) in [
            (
                MqTruncationDisposition::RejectedRetained {
                    required: 4,
                    copied: 3,
                },
                "MQRC_TRUNCATED_MSG_FAILED",
                false,
                false,
            ),
            (
                MqTruncationDisposition::AcceptedRemoved {
                    required: 4,
                    copied: 3,
                },
                "MQRC_TRUNCATED_MSG_ACCEPTED",
                true,
                false,
            ),
            (
                MqTruncationDisposition::AcceptedBrowsed {
                    required: 4,
                    copied: 3,
                },
                "MQRC_TRUNCATED_MSG_ACCEPTED",
                true,
                true,
            ),
        ] {
            let mut r = get(v2);
            if accept {
                r.truncation = MqTruncation::Accept;
            }
            if browse {
                r.mode = MqGetMode::BrowseFirst;
            }
            let e = envelope(MqMqiRequest::FullGet(r));
            let out = got(v2, MqGetDisposition::Message(t), Some(4));
            let status =
                MqReviewedStatus::from_symbols(MqMqiCall::Get, "MQCC_WARNING", reason).unwrap();
            assert!(MqMqiResult::reviewed_output(status, out.clone(), &e).is_ok());
            let success =
                MqReviewedStatus::from_symbols(MqMqiCall::Get, "MQCC_OK", "MQRC_NONE").unwrap();
            assert!(MqMqiResult::reviewed_output(success, out, &e).is_err());
        }
        let old = MqMqiOutput::Got {
            disposition: complete,
            message: None,
            cursor: None,
        };
        assert!(
            result(MqMqiCall::Get, old)
                .validate_reviewed_output_for(&req.request)
                .is_err()
        );
        assert!(
            result(MqMqiCall::Put, got(v2, complete, Some(3)))
                .validate(req.limits)
                .is_err()
        );
        let out = MqMqiOutput::FullGot {
            disposition: MqGetDisposition::NoMessage,
            message: None,
            data_length: None,
            cursor: None,
        };
        assert!(result(MqMqiCall::Get, out).validate(req.limits).is_ok());
    }
}
mod golden;
