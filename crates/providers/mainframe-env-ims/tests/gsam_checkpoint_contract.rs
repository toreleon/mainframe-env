use mainframe_env_host_api::ImsGsamAddress;
use mainframe_env_ims::recovery::*;

#[test]
fn gsam_retained_position_preserves_historical_bytes_and_rejects_ambiguous_shapes() {
    let old = r#"{"pcb":"1","database":"DB","segment_key":[1,2]}"#;
    let historical: SavedPcbPosition = serde_json::from_str(old).unwrap();
    assert_eq!(serde_json::to_string(&historical).unwrap(), old);
    for gsam in [
        SavedGsamPosition::Beginning,
        SavedGsamPosition::Eof,
        SavedGsamPosition::Record(ImsGsamAddress {
            database: "DB".into(),
            token: [3; 32],
        }),
        SavedGsamPosition::Output {
            records: 1,
            prefix_digest: [4; 32],
        },
    ] {
        let mut request = CheckpointRequest {
            id: "GSAM".into(),
            kind: CheckpointKind::Symbolic,
            context: RecoveryContext::Batch,
            prior_xrst: true,
            user_areas: vec![],
            positions: vec![SavedPcbPosition {
                gsam_format: None,
                pcb: "1".into(),
                database: "DB".into(),
                segment_key: vec![],
                gsam: Some(gsam),
                secondary: None,
            }],
        };
        let image =
            CheckpointImage::seal(1, request.clone(), [7; 32], RecoveryLimits::default()).unwrap();
        let decoded: CheckpointImage =
            serde_json::from_slice(&serde_json::to_vec(&image).unwrap()).unwrap();
        decoded.verify(RecoveryLimits::default()).unwrap();
        request.positions[0].segment_key.push(1);
        assert_eq!(
            request.validate(RecoveryLimits::default()),
            Err(RecoveryProblem::InvalidRequest)
        );
        request.positions[0].segment_key.clear();
        request.kind = CheckpointKind::Basic;
        assert_eq!(
            request.validate(RecoveryLimits::default()),
            Err(RecoveryProblem::Unsupported)
        );
        let mut corrupt = image;
        corrupt.request.positions[0].gsam = Some(SavedGsamPosition::Eof);
        if decoded.request.positions[0].gsam != corrupt.request.positions[0].gsam {
            assert_eq!(
                corrupt.verify(RecoveryLimits::default()),
                Err(RecoveryProblem::CorruptImage)
            );
        }
    }
    assert!(
        serde_json::from_str::<SavedPcbPosition>(
            r#"{"pcb":"1","database":"DB","segment_key":[],"gsam":{"invented":0}}"#
        )
        .is_err()
    );
}

#[test]
fn gsam_retained_position_bounds_and_database_identity_are_checked_before_sealing() {
    let mut request = CheckpointRequest {
        id: "GSAM".into(),
        kind: CheckpointKind::Symbolic,
        context: RecoveryContext::Batch,
        prior_xrst: true,
        user_areas: vec![],
        positions: vec![SavedPcbPosition {
            gsam_format: None,
            pcb: "1".into(),
            database: "DB".into(),
            segment_key: vec![],
            secondary: None,
            gsam: Some(SavedGsamPosition::Record(ImsGsamAddress {
                database: "OTHER".into(),
                token: [3; 32],
            })),
        }],
    };
    assert_eq!(
        request.validate(RecoveryLimits::default()),
        Err(RecoveryProblem::InvalidRequest)
    );
    request.positions[0].gsam = Some(SavedGsamPosition::Output {
        records: 65_537,
        prefix_digest: [4; 32],
    });
    assert_eq!(
        request.validate(RecoveryLimits::default()),
        Err(RecoveryProblem::LimitExceeded)
    );
    request.positions[0].gsam = None;
    assert_eq!(
        request.validate(RecoveryLimits::default()),
        Err(RecoveryProblem::InvalidRequest)
    );
}
