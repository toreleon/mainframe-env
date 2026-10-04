use super::*;

fn connect(call: MqMqiCall) -> MqMqiRequestEnvelope {
    let input = MqMqiConnect {
        manager: None,
        sharing: MqHandleSharing::NonShared,
        options: MqMqiOptions::ContractDefault,
    };
    envelope(match call {
        MqMqiCall::Connect => MqMqiRequest::Connect(input),
        MqMqiCall::ConnectExtended => MqMqiRequest::ConnectExtended(input),
        _ => panic!("connection fixture"),
    })
}

#[test]
fn both_connection_calls_retain_exact_warning_and_actual_issued_output() {
    let f = Fixture::new(7);
    for call in [MqMqiCall::Connect, MqMqiCall::ConnectExtended] {
        let status = MqReviewedStatus::from_wire_pair(call, 1, 2002).unwrap();
        let value = MqMqiResult::reviewed_output(
            status,
            MqMqiOutput::Connected(f.connection),
            &connect(call),
        )
        .unwrap();
        assert_eq!(value.validate(MqMqiLimits::default()), Ok(()));
        assert_eq!(
            value.validate_reviewed_output_for(&connect(call).request),
            Ok(())
        );
        assert_eq!(
            value.outcome,
            MqMqiOutcome::ReviewedOutput {
                status,
                output: MqMqiOutput::Connected(f.connection),
            }
        );
        assert_eq!(status.wire_pair(), (1, 2002));
        let bytes = mq_mqi_result_bytes(&value, MqMqiLimits::default()).unwrap();
        check_canonical(&bytes, MQ_MQI_RESULT_DOMAIN);
        let success = MqMqiResult {
            call,
            outcome: MqMqiOutcome::Completed {
                status: MqMqiStatus::OkNone,
                output: MqMqiOutput::Connected(f.connection),
            },
        };
        assert_ne!(
            bytes,
            mq_mqi_result_bytes(&success, MqMqiLimits::default()).unwrap()
        );
    }
}

#[test]
fn warning_requires_exact_call_reason_and_connection_output_shape() {
    let f = Fixture::new(7);
    for call in [MqMqiCall::Connect, MqMqiCall::ConnectExtended] {
        let status = MqReviewedStatus::from_wire_pair(call, 1, 2002).unwrap();
        for output in [
            MqMqiOutput::NoOutput,
            MqMqiOutput::Connected(MqHconn::Unassociated),
            MqMqiOutput::UnitOfWork { unit: 1 },
            MqMqiOutput::Opened {
                object: f.object,
                dynamic: None,
            },
        ] {
            assert!(MqMqiResult::reviewed_output(status, output, &connect(call)).is_err());
        }
        for (completion, reason) in [(2, 2035), (1, 2391), (1, 2267)] {
            let status = MqReviewedStatus::from_wire_pair(call, completion, reason).unwrap();
            assert!(
                MqMqiResult::reviewed_output(
                    status,
                    MqMqiOutput::Connected(f.connection),
                    &connect(call),
                )
                .is_err()
            );
        }
        let wrong_call = match call {
            MqMqiCall::Connect => MqMqiCall::ConnectExtended,
            _ => MqMqiCall::Connect,
        };
        assert!(
            MqMqiResult::reviewed_output(
                status,
                MqMqiOutput::Connected(f.connection),
                &connect(wrong_call),
            )
            .is_err()
        );
    }
}

#[test]
fn reviewed_warning_is_observation_not_a_registry_or_owner_permit() {
    let f = Fixture::new(7);
    let historical = MqHandleObservation::capture_connection(f.connection)
        .unwrap()
        .historical_connection()
        .unwrap();
    let call = MqMqiCall::Connect;
    let value = MqMqiResult::reviewed_output(
        MqReviewedStatus::from_wire_pair(call, 1, 2002).unwrap(),
        MqMqiOutput::Connected(historical),
        &connect(call),
    )
    .unwrap();
    assert!(mq_mqi_result_bytes(&value, MqMqiLimits::default()).is_ok());
    assert_eq!(
        f.registry.validate_connection(owner(), historical),
        Err(MqHandleProblem::Historical)
    );
    let mut wrong = owner();
    wrong.task_id += 1;
    assert!(f.registry.validate_connection(wrong, f.connection).is_err());
    assert_eq!(f.registry.active_handles(), 4);
}
