use super::*;
use crate::mq_status::{MqCompletion, MqReviewedStatus, MqStatusProblem};

fn observed(
    completion: i64,
    reason: i64,
    disposition: MqGetDisposition,
    copied: usize,
) -> MqMqiResult {
    let mut value = message();
    value.body.truncate(copied);
    value.descriptor.identifiers.message_id = Some(vec![1, 0, 255]);
    value.descriptor.identifiers.correlation_id = Some(vec![2, 0, 255]);
    value.descriptor.expiry = MqExpiry::RelativeHostTicks(7);
    value.properties = vec![property()];
    MqMqiResult {
        call: MqMqiCall::Get,
        outcome: MqMqiOutcome::ReviewedOutput {
            status: MqReviewedStatus::from_wire_pair(MqMqiCall::Get, completion, reason).unwrap(),
            output: MqMqiOutput::Got {
                disposition,
                message: Some(value),
                cursor: None,
            },
        },
    }
}
fn truncation(required: usize, copied: usize, accepted: bool, browsed: bool) -> MqGetDisposition {
    MqGetDisposition::Message(if !accepted {
        MqTruncationDisposition::RejectedRetained { required, copied }
    } else if browsed {
        MqTruncationDisposition::AcceptedBrowsed { required, copied }
    } else {
        MqTruncationDisposition::AcceptedRemoved { required, copied }
    })
}
fn request(capacity: usize, mode: MqGetMode, accept: bool) -> MqMqiRequestEnvelope {
    let f = Fixture::new(7);
    let mut g = get();
    g.buffer_capacity = capacity;
    g.mode = mode;
    g.truncation = if accept {
        MqTruncation::Accept
    } else {
        MqTruncation::Reject
    };
    envelope(MqMqiRequest::Get(MqMqiGet {
        connection: f.connection,
        object: f.object,
        message_handle: None,
        get: g,
        options: MqMqiOptions::ContractDefault,
        unit: MqMqiUnitOfWork::NoSyncpoint,
    }))
}

#[test]
fn reviewed_get_preserves_required_length_descriptor_and_message_for_both_warning_forms() {
    for (reason, mode, accept, browse, capacity) in [
        (2079, MqGetMode::Remove, true, false, 2),
        (2079, MqGetMode::BrowseFirst, true, true, 2),
        (2080, MqGetMode::Remove, false, false, 2),
        (2080, MqGetMode::BrowseFirst, false, false, 2),
        (2079, MqGetMode::Remove, true, false, 0),
    ] {
        let result = observed(1, reason, truncation(7, capacity, accept, browse), capacity);
        let MqMqiOutcome::ReviewedOutput { status, output } = result.outcome.clone() else {
            unreachable!()
        };
        assert_eq!(status.wire_pair(), (1, reason as i32));
        let constructed =
            MqMqiResult::reviewed_output(status, output, &request(capacity, mode, accept)).unwrap();
        assert_eq!(constructed, result);
        let bytes = mq_mqi_result_bytes(&result, MqMqiLimits::default()).unwrap();
        check_canonical(&bytes, MQ_MQI_RESULT_DOMAIN);
        assert_eq!(result.validate(MqMqiLimits::default()), Ok(()));
        let MqMqiOutcome::ReviewedOutput {
            output:
                MqMqiOutput::Got {
                    message: Some(value),
                    ..
                },
            ..
        } = &result.outcome
        else {
            unreachable!()
        };
        assert_eq!(value.body.len(), capacity);
        assert_eq!(
            value.descriptor.identifiers.message_id,
            Some(vec![1, 0, 255])
        );
        assert_eq!(value.descriptor.expiry, MqExpiry::RelativeHostTicks(7));
        assert_eq!(value.properties, vec![property()]);
        let old = MqMqiResult {
            call: result.call,
            outcome: MqMqiOutcome::Completed {
                status: MqMqiStatus::OkNone,
                output: match result.outcome.clone() {
                    MqMqiOutcome::ReviewedOutput { output, .. } => output,
                    _ => unreachable!(),
                },
            },
        };
        assert!(old.validate(MqMqiLimits::default()).is_err());
    }
}

#[test]
fn reviewed_status_output_shape_matrix_is_closed_and_aliases_stay_explicit() {
    let limits = MqMqiLimits::default();
    let full = MqGetDisposition::Message(MqTruncationDisposition::Complete { length: 3 });
    assert!(observed(0, 0, full, 3).validate(limits).is_ok());
    for result in [
        observed(0, 0, truncation(7, 2, true, false), 2),
        observed(1, 2079, truncation(7, 2, false, false), 2),
        observed(1, 2080, truncation(7, 2, true, false), 2),
        observed(1, 2079, full, 3),
        observed(2, 2004, full, 3),
        observed(2, 2033, full, 3),
        observed(1, 2079, MqGetDisposition::UnknownOutcome, 0),
    ] {
        let before = result.clone();
        assert!(mq_mqi_result_bytes(&result, limits).is_err());
        assert_eq!(result, before);
    }
    assert_eq!(
        MqReviewedStatus::from_wire_pair(MqMqiCall::Get, 2, 2080),
        Err(MqStatusProblem::UnknownPair)
    );
    assert_eq!(
        MqReviewedStatus::from_wire_pair(MqMqiCall::Put, 2, 2192),
        Err(MqStatusProblem::AmbiguousReasonIdentity)
    );
    for symbol in ["MQRC_STORAGE_MEDIUM_FULL", "MQRC_PAGESET_FULL"] {
        let status =
            MqReviewedStatus::from_identity(MqMqiCall::Put, "MQCC_FAILED", symbol, 2192, 0x890)
                .unwrap();
        let result = MqMqiResult {
            call: MqMqiCall::Put,
            outcome: MqMqiOutcome::ReviewedOutput {
                status,
                output: MqMqiOutput::Put {
                    descriptor: message().descriptor,
                    outcome: MqDeliveryOutcome::Accepted,
                },
            },
        };
        assert!(result.validate(limits).is_err());
    }
    assert!(MqReviewedStatus::from_wire_pair(MqMqiCall::CallbackFunction, 0, 0).is_err());
    for completion in [-1, 3, i64::MAX] {
        assert!(MqReviewedStatus::from_wire_pair(MqMqiCall::Get, completion, 2079).is_err());
    }
    assert_eq!(MqCompletion::Warning.wire_number(), 1);
}

#[test]
fn reviewed_request_binding_rejects_changed_mode_capacity_options_and_partial_copy() {
    let result = observed(1, 2079, truncation(7, 2, true, false), 2);
    assert_eq!(
        result.validate_reviewed_output_for(&request(2, MqGetMode::Remove, true).request),
        Ok(())
    );
    for env in [
        request(1, MqGetMode::Remove, true),
        request(3, MqGetMode::Remove, true),
        request(2, MqGetMode::BrowseFirst, true),
        request(2, MqGetMode::Remove, false),
    ] {
        assert!(result.validate_reviewed_output_for(&env.request).is_err());
    }
    let mut env = request(2, MqGetMode::Remove, true);
    let MqMqiRequest::Get(value) = &mut env.request else {
        unreachable!()
    };
    value.options = MqMqiOptions::PendingStructure {
        requested_version: Some(4),
    };
    assert!(result.validate_reviewed_output_for(&env.request).is_err());
    let mut result = observed(1, 2080, truncation(7, 2, false, false), 2);
    let MqMqiOutcome::ReviewedOutput {
        output: MqMqiOutput::Got { cursor, .. },
        ..
    } = &mut result.outcome
    else {
        unreachable!()
    };
    *cursor = Some(9);
    assert!(result.validate(env.limits).is_err());
}

#[test]
fn failed_no_message_stays_distinct_from_empty_message_and_unknown() {
    let status = MqReviewedStatus::from_wire_pair(MqMqiCall::Get, 2, 2033).unwrap();
    for disposition in [MqGetDisposition::NoMessage, MqGetDisposition::WaitExpired] {
        let result = MqMqiResult {
            call: MqMqiCall::Get,
            outcome: MqMqiOutcome::ReviewedOutput {
                status,
                output: MqMqiOutput::Got {
                    disposition,
                    message: None,
                    cursor: None,
                },
            },
        };
        assert!(result.validate(MqMqiLimits::default()).is_ok());
        let env = request(0, MqGetMode::Remove, false);
        assert_eq!(
            result.validate_reviewed_output_for(&env.request).is_ok(),
            disposition == MqGetDisposition::NoMessage
        );
    }
    assert!(
        observed(2, 2033, MqGetDisposition::NoMessage, 0)
            .validate(MqMqiLimits::default())
            .is_err()
    );
}

#[test]
fn reviewed_put_success_is_not_pending_or_duplicate_and_handles_still_come_from_registry() {
    let limits = MqMqiLimits::default();
    for call in [MqMqiCall::Put, MqMqiCall::PutOne] {
        let status = MqReviewedStatus::from_wire_pair(call, 0, 0).unwrap();
        for outcome in [
            MqDeliveryOutcome::Accepted,
            MqDeliveryOutcome::Pending,
            MqDeliveryOutcome::Rejected,
            MqDeliveryOutcome::DuplicatePossible,
            MqDeliveryOutcome::UnknownOutcome,
        ] {
            let expected = outcome == MqDeliveryOutcome::Accepted;
            let result = MqMqiResult {
                call,
                outcome: MqMqiOutcome::ReviewedOutput {
                    status,
                    output: MqMqiOutput::Put {
                        descriptor: message().descriptor,
                        outcome,
                    },
                },
            };
            assert_eq!(result.validate(limits).is_ok(), expected);
        }
    }
    let f = Fixture::new(7);
    let result = MqMqiResult {
        call: MqMqiCall::Open,
        outcome: MqMqiOutcome::ReviewedOutput {
            status: MqReviewedStatus::from_wire_pair(MqMqiCall::Open, 0, 0).unwrap(),
            output: MqMqiOutput::Opened {
                object: f.object,
                dynamic: None,
            },
        },
    };
    assert!(result.validate(limits).is_ok());
    assert_eq!(f.registry.active_handles(), 4);
    let mut wrong = result;
    wrong.call = MqMqiCall::Get;
    assert_eq!(
        wrong.validate(limits),
        Err(MqMqiProblem::StatusCallMismatch)
    );
}

#[test]
fn full_host_result_encoding_has_independent_new_variant_framing_and_lossless_fields() {
    use crate::canonical::{Canonical, Encoder, RESULT_DIGEST_DOMAIN, encode};
    // Independent framing expectation; existing field encoders have their own
    // frozen goldens. No production outcome encoder is used by this reference.
    struct Reference<'a> {
        status: MqReviewedStatus,
        output: &'a MqMqiOutput,
        limits: MqMqiLimits,
    }
    impl Canonical for Reference<'_> {
        fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
            out.variant("HostResult", "MqMqi", 1)?;
            out.text("0")?;
            out.object("MqMqiHostResult", 2)?;
            out.text("limits")?;
            self.limits.encode(out)?;
            out.text("result")?;
            out.object("MqMqiResult", 2)?;
            out.text("call")?;
            MqMqiCall::Get.encode(out)?;
            out.text("outcome")?;
            out.variant("MqMqiOutcome", "ReviewedOutput", 2)?;
            out.text("output")?;
            self.output.encode(out)?;
            out.text("status")?;
            self.status.encode(out)
        }
    }
    let result = observed(1, 2080, truncation(7, 2, false, false), 2);
    let limits = MqMqiLimits::default();
    let host = MqMqiHostResult {
        result: result.clone(),
        limits,
    };
    let actual: Result<_, HostProblem> = Ok(HostResult::MqMqi(host.clone()));
    let MqMqiOutcome::ReviewedOutput { status, output } = &result.outcome else {
        unreachable!()
    };
    let expected: Result<_, HostProblem> = Ok(Reference {
        status: *status,
        output,
        limits,
    });
    let mut reference_bytes = Vec::new();
    let expected_size = encode(
        &expected,
        RESULT_DIGEST_DOMAIN,
        MAX_CANONICAL_EFFECT_BYTES,
        &mut |part| reference_bytes.extend_from_slice(part),
    )
    .unwrap();
    assert_eq!(
        canonical_result_size(&actual, MAX_CANONICAL_EFFECT_BYTES),
        Ok(expected_size)
    );
    assert_eq!(
        canonical_result_digest(&actual).unwrap(),
        <[u8; 32]>::from(Sha256::digest(&reference_bytes))
    );
    assert_ne!(
        canonical_result_digest(&actual).unwrap(),
        mq_mqi_result_digest(&result, limits).unwrap()
    );
    assert_eq!(
        canonical_result_size(&actual, expected_size - 1),
        Err(HostProblem::ResourceExhausted)
    );
    assert!(host.validate(HostLimits::default()).is_ok());
    let small = HostLimits {
        max_record_bytes: 6,
        ..HostLimits::default()
    };
    assert_eq!(host.validate(small), Err(HostProblem::ResourceExhausted));
    for field in 0..9 {
        let mut host = host.clone();
        let MqMqiOutcome::ReviewedOutput {
            output:
                MqMqiOutput::Got {
                    message: Some(value),
                    disposition,
                    ..
                },
            ..
        } = &mut host.result.outcome
        else {
            unreachable!()
        };
        match field {
            0 => value.body[0] = 9,
            1 => value.descriptor.identifiers.message_id.as_mut().unwrap()[0] = 9,
            2 => {
                value
                    .descriptor
                    .identifiers
                    .correlation_id
                    .as_mut()
                    .unwrap()[0] = 9
            }
            3 => value.descriptor.expiry = MqExpiry::RelativeHostTicks(8),
            4 => value.descriptor.format = Some("OTHER".into()),
            5 => value.properties[0].value[0] = 9,
            6 => value.properties[0].name = "other".into(),
            7 => value.descriptor.persistence = MqPersistence::NonPersistent,
            8 => *disposition = truncation(8, 2, false, false),
            _ => unreachable!(),
        }
        assert!(host.validate(HostLimits::default()).is_ok());
        assert_ne!(
            canonical_result_digest(&Ok(HostResult::MqMqi(host))).unwrap(),
            canonical_result_digest(&actual).unwrap()
        );
    }
}
