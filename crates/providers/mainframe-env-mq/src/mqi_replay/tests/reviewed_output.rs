use super::*;

mod connection_warning;

#[test]
fn reviewed_get_preserves_status_complete_truncation_failure_and_full_payload_identity() {
    for (completion, reason, disposition, message, cursor) in [
        (
            "MQCC_OK",
            "MQRC_NONE",
            MqGetDisposition::Message(MqTruncationDisposition::Complete { length: 3 }),
            Some(message()),
            Some(19),
        ),
        (
            "MQCC_WARNING",
            "MQRC_TRUNCATED_MSG_ACCEPTED",
            MqGetDisposition::Message(MqTruncationDisposition::AcceptedRemoved {
                required: 7,
                copied: 3,
            }),
            Some(message()),
            None,
        ),
        (
            "MQCC_WARNING",
            "MQRC_TRUNCATED_MSG_ACCEPTED",
            MqGetDisposition::Message(MqTruncationDisposition::AcceptedBrowsed {
                required: 7,
                copied: 3,
            }),
            Some(message()),
            Some(19),
        ),
        (
            "MQCC_WARNING",
            "MQRC_TRUNCATED_MSG_FAILED",
            MqGetDisposition::Message(MqTruncationDisposition::RejectedRetained {
                required: 7,
                copied: 3,
            }),
            Some(message()),
            None,
        ),
        (
            "MQCC_FAILED",
            "MQRC_NO_MSG_AVAILABLE",
            MqGetDisposition::NoMessage,
            None,
            None,
        ),
        (
            "MQCC_FAILED",
            "MQRC_NO_MSG_AVAILABLE",
            MqGetDisposition::WaitExpired,
            None,
            None,
        ),
    ] {
        let value = MqMqiResult {
            call: MqMqiCall::Get,
            outcome: MqMqiOutcome::ReviewedOutput {
                status: MqReviewedStatus::from_symbols(MqMqiCall::Get, completion, reason).unwrap(),
                output: MqMqiOutput::Got {
                    disposition,
                    message,
                    cursor,
                },
            },
        };
        roundtrip(&value);
        let mut raw = as_value(&value);
        for field in ["completion", "reason", "output"] {
            let mut missing = raw.clone();
            missing["outcome"].as_object_mut().unwrap().remove(field);
            reject(&missing);
        }
        raw["outcome"]["extra"] = json!(true);
        reject(&raw);
        let mut raw = as_value(&value);
        raw["host_result_digest"][0] = json!(255 - raw["host_result_digest"][0].as_u64().unwrap());
        reject(&raw);
    }
    let invalid = MqMqiResult {
        call: MqMqiCall::Get,
        outcome: MqMqiOutcome::ReviewedOutput {
            status: MqReviewedStatus::from_symbols(
                MqMqiCall::Get,
                "MQCC_WARNING",
                "MQRC_TRUNCATED_MSG_FAILED",
            )
            .unwrap(),
            output: MqMqiOutput::Got {
                disposition: MqGetDisposition::Message(MqTruncationDisposition::AcceptedRemoved {
                    required: 7,
                    copied: 3,
                }),
                message: Some(message()),
                cursor: None,
            },
        },
    };
    assert!(
        encode(
            &invalid,
            HostLimits::default(),
            MqMqiLimits::default(),
            BYTES
        )
        .is_err()
    );
    let mut raw = as_value(&got(message()));
    raw["outcome"] = json!({"kind":"ReviewedOutput","completion":"MQCC_WARNING","reason":"MQRC_TRUNCATED_MSG_FAILED","output":{"kind":"Got","disposition":{"kind":"AcceptedRemoved","required":7,"copied":3},"message":as_value(&got(message()))["outcome"]["output"]["message"],"cursor":null}});
    coherent(&mut raw, &invalid);
    reject(&raw); // A coherent digest cannot admit mismatched output/status.
}

#[test]
fn reviewed_connection_replay_keeps_historical_authority_and_refuses_special_values() {
    let owner = MqHandleOwner {
        environment: MqHostEnvironment::ZosBatch,
        host_id: 1,
        process_id: 2,
        thread_id: 3,
        task_id: 4,
        syncpoint_epoch: 5,
    };
    let mut registry = MqHandleRegistry::new(7, 4).unwrap();
    let live = registry.connect(owner, MqHandleSharing::NonShared).unwrap();
    let status =
        MqReviewedStatus::from_symbols(MqMqiCall::Connect, "MQCC_OK", "MQRC_NONE").unwrap();
    let value = MqMqiResult {
        call: MqMqiCall::Connect,
        outcome: MqMqiOutcome::ReviewedOutput {
            status,
            output: MqMqiOutput::Connected(live),
        },
    };
    let bytes = stored(&value);
    let restored = restore(&bytes).unwrap();
    let MqMqiOutcome::ReviewedOutput {
        output: MqMqiOutput::Connected(past),
        ..
    } = restored.outcome
    else {
        panic!("exact reviewed output")
    };
    assert!(past.is_historical());
    assert_eq!(
        registry.validate_connection(owner, past),
        Err(MqHandleProblem::Historical)
    );
    assert_eq!(
        canonical_result_digest(&host(&restored, MqMqiLimits::default())).unwrap(),
        canonical_result_digest(&host(&value, MqMqiLimits::default())).unwrap()
    );
    assert_eq!(stored(&restored), bytes);
    for connection in [MqHconn::Default, MqHconn::Unassociated] {
        let special = MqMqiResult {
            call: MqMqiCall::Connect,
            outcome: MqMqiOutcome::ReviewedOutput {
                status,
                output: MqMqiOutput::Connected(connection),
            },
        };
        assert_eq!(
            encode(
                &special,
                HostLimits::default(),
                MqMqiLimits::default(),
                BYTES
            ),
            Err(ReplayError::Unsupported(
                ReplayPending::HistoricalSpecialConnection
            ))
        );
    }
    let overflow = MqMqiResult {
        call: MqMqiCall::Get,
        outcome: MqMqiOutcome::ReviewedOutput {
            status: MqReviewedStatus::from_symbols(MqMqiCall::Get, "MQCC_OK", "MQRC_NONE").unwrap(),
            output: MqMqiOutput::Got {
                disposition: MqGetDisposition::Message(MqTruncationDisposition::Complete {
                    length: 3,
                }),
                message: Some(message()),
                cursor: Some(i64::MAX as u64 + 1),
            },
        },
    };
    assert_eq!(
        encode(
            &overflow,
            HostLimits::default(),
            MqMqiLimits::default(),
            BYTES
        ),
        Err(ReplayError::Bounds)
    );
}
