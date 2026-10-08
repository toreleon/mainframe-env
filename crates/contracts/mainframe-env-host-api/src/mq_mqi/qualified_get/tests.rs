use super::*;
use crate::mq_md_value::tests::value;
use crate::mq_status::MqReviewedStatus;
use crate::*;

fn request(v2: bool, cp: bool) -> MqMqiRequestEnvelope {
    let object: MqHandleObservation = serde_json::from_str(
        r#"{"role":"Object","registry":77,"slot":2,"generation":3,"epoch":4}"#,
    )
    .unwrap();
    MqMqiRequestEnvelope {
        context: MqMqiContext {
            owner: MqHandleOwner {
                environment: MqHostEnvironment::ZosCics,
                host_id: 1,
                process_id: 2,
                thread_id: 3,
                task_id: 4,
                syncpoint_epoch: 5,
            },
            syncpoint_owner: MqSyncpointOwner::HostCoordinator,
        },
        limits: MqMqiLimits::default(),
        request: MqMqiRequest::QualifiedFullGet(MqMqiFullGet {
            connection: MqHconn::Default,
            object: object.historical_object().unwrap(),
            descriptor: value(v2, cp),
            mode: MqGetMode::Remove,
            wait: MqWait::NoWait,
            truncation: MqTruncation::Reject,
            buffer_capacity: 3,
            message_handle: None,
            options: MqMqiOptions::ContractDefault,
            unit: MqMqiUnitOfWork::NoSyncpoint,
        }),
    }
}
fn output(v2: bool, cp: bool) -> MqMqiQualifiedGot {
    MqMqiQualifiedGot {
        characters: value(v2, cp).characters(),
        disposition: MqGetDisposition::Message(T::Complete { length: 3 }),
        message: Some(MqFullMessage {
            descriptor: value(v2, cp),
            body: vec![0, 255, 10],
            properties: vec![],
        }),
        data_length: Some(3),
        cursor: None,
        resolved_queue: Some([if cp { 0x40 } else { b' ' }; 48]),
    }
}
#[test]
fn qualified_profile_shape_call_binding_and_fields_affect_full_digest() {
    for v2 in [false, true] {
        for cp in [false, true] {
            let request = request(v2, cp);
            let out = output(v2, cp);
            let status =
                MqReviewedStatus::from_symbols(MqMqiCall::Get, "MQCC_OK", "MQRC_NONE").unwrap();
            let result = MqMqiResult::reviewed_output(
                status,
                MqMqiOutput::QualifiedFullGot(out.clone()),
                &request,
            )
            .unwrap();
            let host = |r| {
                Ok(HostResult::MqMqi(Box::new(MqMqiHostResult {
                    result: r,
                    limits: request.limits,
                })))
            };
            let digest = canonical_result_digest(&host(result.clone())).unwrap();
            for i in 0..48 {
                let mut changed = result.clone();
                let MqMqiOutcome::ReviewedOutput {
                    output: MqMqiOutput::QualifiedFullGot(v),
                    ..
                } = &mut changed.outcome
                else {
                    panic!()
                };
                v.resolved_queue.as_mut().unwrap()[i] ^= 1;
                assert_ne!(canonical_result_digest(&host(changed)).unwrap(), digest);
            }
            let MqMqiRequest::QualifiedFullGet(get) = request.request.clone() else {
                panic!()
            };
            assert_eq!(request.request.call(), MqMqiCall::Get);
            assert!(
                result
                    .validate_reviewed_output_for(&MqMqiRequest::FullGet(get))
                    .is_err()
            );
            let mut wrong = out.clone();
            wrong.resolved_queue = None;
            assert!(wrong.validate(request.limits.message).is_err());
            wrong = out.clone();
            wrong.characters = if cp {
                MqMdCharacterEncoding::AsciiCompatible
            } else {
                MqMdCharacterEncoding::OwnedCp037
            };
            assert!(wrong.validate(request.limits.message).is_err());
            let mut limits = request.limits.message;
            limits.destination_bytes = 47;
            assert!(out.validate(limits).is_err());
            wrong = out.clone();
            wrong.cursor = Some(i64::MAX as u64 + 1);
            assert!(wrong.validate(request.limits.message).is_err());
        }
    }
}
#[test]
fn qualified_absence_rejected_truncation_and_original_capacity_fail_closed() {
    let mut request = request(false, false);
    let mut value = output(false, false);
    value.disposition = MqGetDisposition::Message(T::RejectedRetained {
        required: 4,
        copied: 3,
    });
    value.data_length = Some(4);
    value.resolved_queue = None;
    let status =
        MqReviewedStatus::from_symbols(MqMqiCall::Get, "MQCC_WARNING", "MQRC_TRUNCATED_MSG_FAILED")
            .unwrap();
    MqMqiResult::reviewed_output(
        status,
        MqMqiOutput::QualifiedFullGot(value.clone()),
        &request,
    )
    .unwrap();
    value.resolved_queue = Some([b' '; 48]);
    assert!(
        MqMqiResult::reviewed_output(
            status,
            MqMqiOutput::QualifiedFullGot(value.clone()),
            &request
        )
        .is_err()
    );
    value.resolved_queue = None;
    let MqMqiRequest::QualifiedFullGet(get) = &mut request.request else {
        panic!()
    };
    get.buffer_capacity = 2;
    assert!(
        MqMqiResult::reviewed_output(
            status,
            MqMqiOutput::QualifiedFullGot(value.clone()),
            &request
        )
        .is_err()
    );
    value.disposition = MqGetDisposition::NoMessage;
    value.message = None;
    value.data_length = None;
    value.validate(request.limits.message).unwrap();
    value.resolved_queue = Some([0; 48]);
    assert!(value.validate(request.limits.message).is_err());
}
