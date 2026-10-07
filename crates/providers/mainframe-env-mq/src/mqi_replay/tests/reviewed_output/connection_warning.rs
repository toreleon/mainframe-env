use super::*;

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

fn warning(call: MqMqiCall, connection: MqHconn) -> MqMqiResult {
    MqMqiResult {
        call,
        outcome: MqMqiOutcome::ReviewedOutput {
            status: MqReviewedStatus::from_wire_pair(call, 1, 2002).unwrap(),
            output: MqMqiOutput::Connected(connection),
        },
    }
}

#[test]
fn already_connected_storage_retains_exact_warning_but_only_historical_handle() {
    let owner = owner();
    for call in [MqMqiCall::Connect, MqMqiCall::ConnectExtended] {
        let mut registry = MqHandleRegistry::new(7, 4).unwrap();
        let live = registry.connect(owner, MqHandleSharing::NonShared).unwrap();
        let value = warning(call, live);
        let bytes = stored(&value);
        let restored = restore(&bytes).unwrap();
        let MqMqiOutcome::ReviewedOutput {
            status,
            output: MqMqiOutput::Connected(past),
        } = restored.outcome
        else {
            panic!("warning output retained")
        };
        assert_eq!(status.wire_pair(), (1, 2002));
        assert!(past.is_historical());
        assert_eq!(
            registry.validate_connection(owner, past),
            Err(MqHandleProblem::Historical)
        );
        assert_eq!(
            mq_mqi_result_bytes(&restored, Default::default()).unwrap(),
            mq_mqi_result_bytes(&value, Default::default()).unwrap()
        );
        assert_eq!(
            canonical_result_digest(&host(&restored, Default::default())).unwrap(),
            canonical_result_digest(&host(&value, Default::default())).unwrap()
        );
        assert_eq!(stored(&restored), bytes);
        assert_ne!(restored, value);
        assert_eq!(registry.active_handles(), 1);
        registry.disconnect(owner, live).unwrap();
        assert!(
            registry
                .resolve_observed_connection(
                    owner,
                    MqHandleObservation::capture_connection(past).unwrap()
                )
                .is_err()
        );
        assert!(registry.validate_connection(owner, live).is_err());
    }
}

#[test]
fn coherent_warning_shape_and_status_mutants_and_special_values_are_refused() {
    let owner = owner();
    let mut registry = MqHandleRegistry::new(7, 4).unwrap();
    let live = registry.connect(owner, MqHandleSharing::NonShared).unwrap();
    for call in [MqMqiCall::Connect, MqMqiCall::ConnectExtended] {
        let value = warning(call, live);
        let original = as_value(&value);
        let mut invalid = value.clone();
        if let MqMqiOutcome::ReviewedOutput { output, .. } = &mut invalid.outcome {
            *output = MqMqiOutput::NoOutput;
        }
        let mut raw = original.clone();
        raw["outcome"]["output"] = json!({"kind":"NoOutput"});
        coherent(&mut raw, &invalid);
        reject(&raw);
        for connection in [MqHconn::Default, MqHconn::Unassociated] {
            assert_eq!(
                encode(
                    &warning(call, connection),
                    Default::default(),
                    Default::default(),
                    BYTES
                ),
                Err(ReplayError::Unsupported(
                    ReplayPending::HistoricalSpecialConnection
                ))
            );
        }
        for name in ["completion", "reason", "output"] {
            let mut raw = original.clone();
            raw["outcome"].as_object_mut().unwrap().remove(name);
            reject(&raw);
        }
        for (completion, reason) in [
            ("MQCC_FAILED", "MQRC_NOT_AUTHORIZED"),
            ("MQCC_WARNING", "MQRC_SSL_ALREADY_INITIALIZED"),
        ] {
            let mut invalid = value.clone();
            let mut raw = original.clone();
            if let MqMqiOutcome::ReviewedOutput { status, .. } = &mut invalid.outcome {
                *status = MqReviewedStatus::from_symbols(call, completion, reason).unwrap();
            }
            raw["outcome"]["completion"] = json!(completion);
            raw["outcome"]["reason"] = json!(reason);
            coherent(&mut raw, &invalid);
            reject(&raw);
        }
    }
}
