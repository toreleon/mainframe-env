use mainframe_env_ims::recovery::*;

fn request() -> CheckpointRequest {
    serde_json::from_value(serde_json::json!({
        "id":"SECOND", "kind":"symbolic", "context":"batch", "prior_xrst":true,
        "user_areas":[], "positions":[{
            "pcb":"2", "database":"DB", "segment_key":[], "secondary":{
                "index":"INDEX", "metadata_digest":vec![1;32], "search_key":[0,255],
                "source":{"id":2,"identity":vec![2;32],"version":1},
                "target":{"id":1,"identity":vec![3;32],"version":1},
                "current":{"id":1,"identity":vec![3;32],"version":1},
                "source_path":[["ROOT",[1]],["CHILD",[2]]], "current_path":[["ROOT",[1]]]
            }
        }]
    }))
    .unwrap()
}

#[test]
fn secondary_saved_position_is_discriminated_bounded_and_integrity_checked() {
    let mut r = request();
    let limits = RecoveryLimits::default();
    assert_eq!(r.validate(limits), Ok(()));
    let image = CheckpointImage::seal(1, r.clone(), [4; 32], limits).unwrap();
    assert_eq!(
        serde_json::from_slice::<CheckpointImage>(&serde_json::to_vec(&image).unwrap()).unwrap(),
        image
    );
    let mut corrupt = image.clone();
    corrupt.request.positions[0]
        .secondary
        .as_mut()
        .unwrap()
        .search_key[0] ^= 1;
    assert_eq!(corrupt.verify(limits), Err(RecoveryProblem::CorruptImage));
    r.positions[0].segment_key = vec![1];
    assert_eq!(r.validate(limits), Err(RecoveryProblem::InvalidRequest));
    r = request();
    r.positions[0].gsam = Some(SavedGsamPosition::Beginning);
    assert_eq!(r.validate(limits), Err(RecoveryProblem::InvalidRequest));
    r = request();
    r.positions[0].gsam_format = Some([1; 32]);
    assert_eq!(r.validate(limits), Err(RecoveryProblem::InvalidRequest));
    r = request();
    r.positions[0].secondary.as_mut().unwrap().source.identity = [0; 32];
    assert_eq!(r.validate(limits), Err(RecoveryProblem::InvalidRequest));
    r = request();
    r.positions[0].secondary.as_mut().unwrap().search_key = vec![1; 257];
    assert_eq!(r.validate(limits), Err(RecoveryProblem::LimitExceeded));
    r = request();
    r.kind = CheckpointKind::Basic;
    assert_eq!(r.validate(limits), Err(RecoveryProblem::Unsupported));
}

#[test]
fn historical_primary_and_gsam_position_bytes_remain_exact_and_future_fields_reject() {
    for bytes in [
        r#"{"pcb":"1","database":"DB","segment_key":[1]}"#,
        r#"{"pcb":"1","database":"DB","segment_key":[],"gsam":"beginning"}"#,
    ] {
        let position: SavedPcbPosition = serde_json::from_str(bytes).unwrap();
        assert!(position.secondary.is_none());
        assert_eq!(serde_json::to_string(&position).unwrap(), bytes);
    }
    let mut value = serde_json::to_value(request()).unwrap();
    value["positions"][0]["secondary"]["future"] = serde_json::json!(1);
    assert!(serde_json::from_value::<CheckpointRequest>(value).is_err());
}
