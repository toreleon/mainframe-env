use super::*;

fn completed(call: MqMqiCall, output: MqMqiOutput) -> MqMqiResult {
    MqMqiResult {
        call,
        outcome: MqMqiOutcome::Completed {
            status: MqMqiStatus::OkNone,
            output,
        },
    }
}

#[test]
fn typed_success_outputs_encode_and_wrong_call_or_placeholder_is_rejected() {
    let f = Fixture::new(7);
    let limits = MqMqiLimits::default();
    let values = vec![
        completed(MqMqiCall::Connect, MqMqiOutput::Connected(f.connection)),
        completed(
            MqMqiCall::ConnectExtended,
            MqMqiOutput::Connected(f.connection),
        ),
        completed(
            MqMqiCall::Open,
            MqMqiOutput::Opened {
                object: f.object,
                dynamic: None,
            },
        ),
        completed(
            MqMqiCall::CreateMessageHandle,
            MqMqiOutput::MessageHandle(f.handle),
        ),
        completed(
            MqMqiCall::Subscribe,
            MqMqiOutput::Subscribed {
                object: f.object,
                subscription: f.subscription,
            },
        ),
        completed(
            MqMqiCall::Get,
            MqMqiOutput::Got {
                disposition: MqGetDisposition::Message(MqTruncationDisposition::Complete {
                    length: 3,
                }),
                message: Some(message()),
                cursor: Some(1),
            },
        ),
        completed(
            MqMqiCall::Put,
            MqMqiOutput::Put {
                descriptor: message().descriptor,
                outcome: MqDeliveryOutcome::Accepted,
            },
        ),
        completed(
            MqMqiCall::InquireProperty,
            MqMqiOutput::Property(property()),
        ),
        completed(
            MqMqiCall::HandleToBuffer,
            MqMqiOutput::Buffer {
                descriptor: message().descriptor,
                bytes: vec![0, 255],
                data_length: 2,
            },
        ),
        completed(
            MqMqiCall::BufferToHandle,
            MqMqiOutput::Buffer {
                descriptor: message().descriptor,
                bytes: vec![],
                data_length: 0,
            },
        ),
        completed(
            MqMqiCall::Inquire,
            MqMqiOutput::Attributes {
                integers: vec![1, 2],
                characters: vec![0, 255],
            },
        ),
        completed(MqMqiCall::Begin, MqMqiOutput::UnitOfWork { unit: 1 }),
        completed(
            MqMqiCall::SubscriptionRequest,
            MqMqiOutput::PublicationsRequested { count: 2 },
        ),
        completed(MqMqiCall::SetProperty, MqMqiOutput::NoOutput),
    ];
    for result in values {
        let bytes = mq_mqi_result_bytes(&result, limits).unwrap();
        check_canonical(&bytes, MQ_MQI_RESULT_DOMAIN);
        assert_eq!(
            mq_mqi_result_digest(&result, limits).unwrap(),
            <[u8; 32]>::from(Sha256::digest(&bytes))
        );
        let mut wrong = result;
        wrong.call = MqMqiCall::Stat;
        assert_eq!(
            wrong.validate(limits),
            Err(MqMqiProblem::OutputCallMismatch)
        );
    }
    for call in [
        MqMqiCall::Connect,
        MqMqiCall::Open,
        MqMqiCall::CreateMessageHandle,
        MqMqiCall::Get,
        MqMqiCall::Inquire,
        MqMqiCall::InquireProperty,
        MqMqiCall::Stat,
        MqMqiCall::Subscribe,
        MqMqiCall::CallbackFunction,
    ] {
        assert_eq!(
            completed(call, MqMqiOutput::NoOutput).validate(limits),
            Err(MqMqiProblem::OutputCallMismatch)
        );
    }
}

#[test]
fn only_pinned_status_pairs_and_callback_return_shape_are_representable() {
    let limits = MqMqiLimits::default();
    for call in MqMqiCall::ALL {
        let value = MqMqiResult {
            call,
            outcome: MqMqiOutcome::Completed {
                status: MqMqiStatus::FailedEnvironment,
                output: MqMqiOutput::NoOutput,
            },
        };
        assert_eq!(
            value.validate(limits).is_ok(),
            matches!(call, MqMqiCall::Back | MqMqiCall::Begin | MqMqiCall::Commit)
        );
        let callback = MqMqiResult {
            call,
            outcome: MqMqiOutcome::CallbackReturned {
                context: MqMqiOptions::PendingStructure {
                    requested_version: None,
                },
            },
        };
        assert_eq!(
            callback.validate(limits).is_ok(),
            call == MqMqiCall::CallbackFunction
        );
    }
    let failed = MqMqiResult {
        call: MqMqiCall::Commit,
        outcome: MqMqiOutcome::Completed {
            status: MqMqiStatus::FailedEnvironment,
            output: MqMqiOutput::UnitOfWork { unit: 1 },
        },
    };
    assert_eq!(
        failed.validate(limits),
        Err(MqMqiProblem::StatusCallMismatch)
    );
}

#[test]
fn pending_status_preserves_typed_kernel_observations_without_inventing_success() {
    let limits = MqMqiLimits::default();
    for disposition in [
        MqGetDisposition::NoMessage,
        MqGetDisposition::WaitExpired,
        MqGetDisposition::UnknownOutcome,
    ] {
        let output = MqMqiOutput::Got {
            disposition,
            message: None,
            cursor: None,
        };
        let result = MqMqiResult {
            call: MqMqiCall::Get,
            outcome: MqMqiOutcome::StatusPending {
                output: output.clone(),
            },
        };
        assert!(mq_mqi_result_bytes(&result, limits).is_ok());
        assert!(completed(MqMqiCall::Get, output).validate(limits).is_err());
    }
    let mut truncated = message();
    truncated.body.truncate(2);
    let output = MqMqiOutput::Got {
        disposition: MqGetDisposition::Message(MqTruncationDisposition::AcceptedRemoved {
            required: 3,
            copied: 2,
        }),
        message: Some(truncated),
        cursor: None,
    };
    let result = MqMqiResult {
        call: MqMqiCall::Get,
        outcome: MqMqiOutcome::StatusPending {
            output: output.clone(),
        },
    };
    assert!(mq_mqi_result_bytes(&result, limits).is_ok());
    assert!(completed(MqMqiCall::Get, output).validate(limits).is_err());
    let distribution = MqMqiOutput::Distribution(MqDistributionResult {
        items: vec![
            MqDistributionItemResult {
                destination: "A".into(),
                outcome: MqDeliveryOutcome::Accepted,
            },
            MqDistributionItemResult {
                destination: "B".into(),
                outcome: MqDeliveryOutcome::UnknownOutcome,
            },
        ],
    });
    let result = MqMqiResult {
        call: MqMqiCall::Put,
        outcome: MqMqiOutcome::StatusPending {
            output: distribution.clone(),
        },
    };
    assert!(mq_mqi_result_bytes(&result, limits).is_ok());
    assert!(
        completed(MqMqiCall::Put, distribution)
            .validate(limits)
            .is_err()
    );
}

#[test]
fn malformed_or_oversized_outputs_are_rejected_atomically_and_replay_is_exact() {
    let limits = MqMqiLimits::default();
    let bad = vec![
        MqMqiOutput::Buffer {
            descriptor: message().descriptor,
            bytes: vec![1],
            data_length: 2,
        },
        MqMqiOutput::Buffer {
            descriptor: message().descriptor,
            bytes: vec![0; limits.buffer_bytes + 1],
            data_length: limits.buffer_bytes + 1,
        },
    ];
    for output in bad {
        let result = completed(MqMqiCall::HandleToBuffer, output);
        let before = result.clone();
        assert!(mq_mqi_result_bytes(&result, limits).is_err());
        assert!(mq_mqi_result_digest(&result, limits).is_err());
        assert_eq!(result, before);
    }
    let empty = completed(
        MqMqiCall::Get,
        MqMqiOutput::Got {
            disposition: MqGetDisposition::Message(MqTruncationDisposition::Complete { length: 0 }),
            message: Some(MqMessage {
                body: vec![],
                ..message()
            }),
            cursor: None,
        },
    );
    let bytes = mq_mqi_result_bytes(&empty, limits).unwrap();
    let mut altered = empty.clone();
    if let MqMqiOutcome::Completed {
        output: MqMqiOutput::Got { message, .. },
        ..
    } = &mut altered.outcome
    {
        *message = None;
    }
    assert!(altered.validate(limits).is_err());
    assert_eq!(mq_mqi_result_bytes(&empty, limits).unwrap(), bytes);
    let mut smaller = limits;
    smaller.message.wait_ticks -= 1;
    assert_ne!(
        mq_mqi_result_digest(&empty, limits),
        mq_mqi_result_digest(&empty, smaller)
    );
}
