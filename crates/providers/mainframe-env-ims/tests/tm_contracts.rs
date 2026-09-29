use mainframe_env_host_api::HostProblem;
use mainframe_env_ims::{
    TmAlternatePcbDefinition, TmCall, TmDefinitionSet, TmDestination, TmExecutionContext,
    TmInputMessage, TmLimits, TmPcb, TmPcbStatus, TmTransactionDefinition,
};

fn transaction(code: &str) -> TmTransactionDefinition {
    TmTransactionDefinition {
        code: code.into(),
        psb: "PAYPSB".into(),
        program_selector: "ims:payment".into(),
        artifact: "artifact:payment".into(),
        required_generation: "ims-tm-1".into(),
        context: TmExecutionContext::MessageProcessing,
        priority: 7,
        timeout_ticks: 50,
        conversational: true,
        spa_size: 64,
        alternate_pcbs: vec![
            TmAlternatePcbDefinition {
                name: "REPLY".into(),
                destination: TmDestination::Fixed("TERM1".into()),
                express: false,
            },
            TmAlternatePcbDefinition {
                name: "ROUTE".into(),
                destination: TmDestination::Modifiable,
                express: true,
            },
        ],
    }
}

#[test]
fn generic_tm_definitions_messages_and_statuses_are_bounded() {
    let limits = TmLimits::default();
    let definitions = TmDefinitionSet {
        transactions: vec![transaction("PAY1"), transaction("PAY2")],
    };
    assert_eq!(definitions.validate(limits), Ok(()));

    let message = TmInputMessage {
        message_id: "message-1".into(),
        transaction: "PAY1".into(),
        source: "LTERM1".into(),
        user_id: Some("ALICE".into()),
        group_name: Some("PAYROLL".into()),
        conversation_id: Some("conversation-1".into()),
        segments: vec![b"first".to_vec(), b"second".to_vec()],
    };
    assert_eq!(message.validate(limits), Ok(()));

    let definition = &definitions.transactions[0];
    assert_eq!(
        TmCall::GetUnique.validate(definition.context, definition, limits),
        Ok(())
    );
    assert_eq!(
        TmCall::Insert {
            pcb: TmPcb::Alternate("ROUTE".into()),
            segment: b"reply".to_vec(),
        }
        .validate(definition.context, definition, limits),
        Ok(())
    );
    assert_eq!(
        TmCall::Change {
            pcb: "ROUTE".into(),
            destination: "PAY2".into(),
        }
        .validate(definition.context, definition, limits),
        Ok(())
    );

    assert_eq!(TmPcbStatus::SUCCESS.as_str(), "  ");
    assert_eq!(TmPcbStatus::NO_MORE_MESSAGES.as_str(), "QC");
    assert_eq!(TmPcbStatus::INVALID_CALL.as_str(), "AD");
    assert_eq!(TmPcbStatus::QUEUE_FULL.as_str(), "QF");
}

#[test]
fn malformed_duplicate_and_over_limit_contracts_fail_closed() {
    let limits = TmLimits {
        max_transactions: 1,
        max_alternate_pcbs: 1,
        max_segments_per_message: 1,
        max_segment_bytes: 4,
        max_spa_bytes: 8,
        ..TmLimits::default()
    };
    assert_eq!(
        TmDefinitionSet {
            transactions: vec![transaction("PAY1"), transaction("PAY2")],
        }
        .validate(limits),
        Err(HostProblem::ResourceExhausted)
    );

    let mut invalid = transaction("TOO-LONG9");
    invalid.alternate_pcbs.truncate(1);
    assert_eq!(invalid.validate(limits), Err(HostProblem::Malformed));

    let mut duplicate = transaction("PAY1");
    duplicate.alternate_pcbs = vec![
        TmAlternatePcbDefinition {
            name: "ROUTE".into(),
            destination: TmDestination::Modifiable,
            express: false,
        },
        TmAlternatePcbDefinition {
            name: "ROUTE".into(),
            destination: TmDestination::Modifiable,
            express: false,
        },
    ];
    assert_eq!(
        duplicate.validate(TmLimits::default()),
        Err(HostProblem::Malformed)
    );

    let oversized = TmInputMessage {
        message_id: "message-1".into(),
        transaction: "PAY1".into(),
        source: "LTERM1".into(),
        user_id: None,
        group_name: None,
        conversation_id: None,
        segments: vec![vec![0; 5]],
    };
    assert_eq!(
        oversized.validate(limits),
        Err(HostProblem::ResourceExhausted)
    );
}

#[test]
fn unsupported_contexts_are_rejected_before_runtime_mutation() {
    let limits = TmLimits::default();
    let mut cpic = transaction("CPIC");
    cpic.context = TmExecutionContext::CpiCommunications;
    cpic.conversational = false;
    cpic.spa_size = 0;

    assert_eq!(
        TmCall::GetUnique.validate(cpic.context, &cpic, limits),
        Err(HostProblem::Unsupported)
    );
    assert_eq!(
        TmCall::Insert {
            pcb: TmPcb::Io,
            segment: b"reply".to_vec(),
        }
        .validate(cpic.context, &cpic, limits),
        Err(HostProblem::Unsupported)
    );
    assert_eq!(
        TmCall::Insert {
            pcb: TmPcb::Alternate("ROUTE".into()),
            segment: b"reply".to_vec(),
        }
        .validate(cpic.context, &cpic, limits),
        Ok(())
    );

    let mut ifp = transaction("FAST");
    ifp.context = TmExecutionContext::FastPath;
    assert_eq!(
        TmCall::Purge { pcb: TmPcb::Io }.validate(ifp.context, &ifp, limits),
        Err(HostProblem::Unsupported)
    );
}

#[test]
fn unknown_pcb_status_is_rejected_instead_of_becoming_invalid_call() {
    assert!(serde_json::from_str::<TmPcbStatus>("[88,89]").is_err());
    for status in [
        TmPcbStatus::SUCCESS,
        TmPcbStatus::NO_MORE_MESSAGES,
        TmPcbStatus::INVALID_CALL,
        TmPcbStatus::QUEUE_FULL,
    ] {
        let encoded = serde_json::to_string(&status).unwrap();
        assert_eq!(
            serde_json::from_str::<TmPcbStatus>(&encoded).unwrap(),
            status
        );
    }
}
