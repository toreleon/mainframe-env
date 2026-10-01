use super::*;

#[test]
fn checkpoint_contract_rejects_symbolic_without_xrst_and_oversized_user_areas() {
    let limits = RecoveryLimits::default();
    let mut request = CheckpointRequest {
        id: "CHK00001".into(),
        kind: CheckpointKind::Symbolic,
        context: RecoveryContext::Batch,
        prior_xrst: false,
        user_areas: vec![vec![1; 8]],
        positions: vec![],
    };
    assert_eq!(
        request.validate(limits),
        Err(RecoveryProblem::InvalidRequest)
    );
    request.prior_xrst = true;
    request.user_areas = vec![vec![1; limits.max_user_area_bytes + 1]];
    assert_eq!(
        request.validate(limits),
        Err(RecoveryProblem::LimitExceeded)
    );
    request.user_areas = vec![vec![1; 8]];
    assert_eq!(request.validate(limits), Ok(()));
}

#[test]
fn log_contract_rejects_reserved_codes_and_malformed_lengths() {
    let limits = RecoveryLimits::default();
    assert_eq!(
        LogRequest {
            code: 0x9f,
            data: vec![1]
        }
        .validate(limits),
        Err(RecoveryProblem::InvalidRequest)
    );
    assert_eq!(
        LogRequest {
            code: 0xa0,
            data: vec![0; limits.max_log_data_bytes + 1]
        }
        .validate(limits),
        Err(RecoveryProblem::LimitExceeded)
    );
    assert_eq!(
        LogRequest {
            code: 0xa0,
            data: vec![1]
        }
        .validate(limits),
        Ok(())
    );
}

#[test]
fn utility_contract_rejects_non_database_and_unbounded_plans() {
    let limits = RecoveryLimits::default();
    let mut plan = UtilityPlan {
        kind: UtilityKind::InitialLoad,
        database: "".into(),
        expected_input_digest: [0; 32],
        expected_records: 1,
    };
    assert_eq!(plan.validate(limits), Err(RecoveryProblem::InvalidRequest));
    plan.database = "ACCOUNTS".into();
    plan.expected_records = limits.max_utility_records + 1;
    assert_eq!(plan.validate(limits), Err(RecoveryProblem::LimitExceeded));
}

#[test]
fn checkpoint_image_digest_detects_corruption_and_is_stable_across_roundtrip() {
    let limits = RecoveryLimits::default();
    let request = CheckpointRequest {
        id: "CHK00001".into(),
        kind: CheckpointKind::Symbolic,
        context: RecoveryContext::MessageDrivenBatch,
        prior_xrst: true,
        user_areas: vec![vec![1, 2, 3]],
        positions: vec![SavedPcbPosition {
            pcb: "DBPCB".into(),
            database: "ACCOUNTS".into(),
            segment_key: vec![7, 8],
        }],
    };
    let image = CheckpointImage::seal(1, request, [9; 32], limits).unwrap();
    let encoded = serde_json::to_vec(&image).unwrap();
    let mut decoded: CheckpointImage = serde_json::from_slice(&encoded).unwrap();
    assert_eq!(image, decoded);
    assert_eq!(decoded.verify(limits), Ok(()));
    decoded.request.user_areas[0][0] ^= 1;
    assert_eq!(decoded.verify(limits), Err(RecoveryProblem::CorruptImage));
    decoded = image;
    decoded.committed_database_digest[0] ^= 1;
    assert_eq!(decoded.verify(limits), Err(RecoveryProblem::CorruptImage));
    decoded.schema_version = "future-version".into();
    assert_eq!(decoded.verify(limits), Err(RecoveryProblem::CorruptImage));
}

#[test]
fn checkpoint_rejects_duplicate_positions_and_forbidden_context_without_mutation() {
    let limits = RecoveryLimits::default();
    let position = SavedPcbPosition {
        pcb: "DBPCB".into(),
        database: "ACCOUNTS".into(),
        segment_key: vec![1],
    };
    let mut request = CheckpointRequest {
        id: "CHK00002".into(),
        kind: CheckpointKind::Symbolic,
        context: RecoveryContext::MessageProcessing,
        prior_xrst: true,
        user_areas: vec![],
        positions: vec![position.clone()],
    };
    let original = request.clone();
    assert_eq!(request.validate(limits), Err(RecoveryProblem::Unsupported));
    assert_eq!(request, original);
    request.context = RecoveryContext::Batch;
    request.positions.push(position);
    assert_eq!(
        request.validate(limits),
        Err(RecoveryProblem::InvalidRequest)
    );
    request.positions.pop();
    request.kind = CheckpointKind::Basic;
    request.user_areas.push(vec![1]);
    assert_eq!(
        request.validate(limits),
        Err(RecoveryProblem::InvalidRequest)
    );
}

#[test]
fn restart_selection_and_serde_reject_malformed_shapes() {
    let limits = RecoveryLimits::default();
    assert_eq!(
        RestartSelection::Id("".into()).validate(limits),
        Err(RecoveryProblem::InvalidRequest)
    );
    assert_eq!(
        RestartSelection::Id("CHKP0001".into()).validate(limits),
        Ok(())
    );
    assert_eq!(RestartSelection::Last.validate(limits), Ok(()));
    assert!(serde_json::from_str::<LogRequest>(r#"{"code":160,"data":[],"extra":1}"#).is_err());
    assert!(serde_json::from_str::<CheckpointImage>(r#"{"schema_version":"unknown"}"#).is_err());
}
