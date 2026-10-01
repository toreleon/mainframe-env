use super::*;
use mainframe_env_store::{MemoryStore, StoreLimits};
use mainframe_env_store_api::{ProviderStateRecord, ProviderStateStore};

#[test]
fn duplicate_then_replace_repairs_corrupt_log_and_publishes_closed_stream() {
    let store = MemoryStore::new(StoreLimits::default());
    let limits = RecoveryLimits::default();
    let mut damaged = LogBlock::seal(2, LogPayload::Data(b"bad".to_vec()));
    damaged.declared_length += 1;
    let input = LogDataset {
        name: "UTILLOG".into(),
        kind: LogDatasetKind::Batch,
        closed: false,
        interim: false,
        blocks: vec![
            LogBlock::seal(1, LogPayload::Data(b"good".to_vec())),
            damaged,
        ],
    };
    let plan = UtilityPlan {
        kind: UtilityKind::LogRecovery,
        database: "UTILLOG".into(),
        expected_input_digest: input.digest(),
        expected_records: 2,
    };
    assert_eq!(
        LogRecoveryEngine::stage_duplicate(
            &store,
            "unsafe-dup",
            plan.clone(),
            input.clone(),
            false,
            None,
            limits
        ),
        Err(RecoveryProblem::Unsupported)
    );
    let receipt =
        LogRecoveryEngine::stage_duplicate(&store, "dup", plan, input, true, None, limits).unwrap();
    assert_eq!(receipt.error_markers, 1);
    LogRecoveryEngine::publish(&store, "dup", "UTILLOG", limits).unwrap();
    let interim = LogRecoveryEngine::active(&store, "UTILLOG", limits).unwrap();
    assert!(interim.interim);
    let replace = UtilityPlan {
        kind: UtilityKind::LogRecovery,
        database: "UTILLOG".into(),
        expected_input_digest: interim.digest(),
        expected_records: 2,
    };
    LogRecoveryEngine::stage_replace(
        &store,
        "rep",
        replace,
        vec![LogReplacement {
            sequence: 2,
            payload: LogPayload::Data(b"fixed".to_vec()),
        }],
        limits,
    )
    .unwrap();
    LogRecoveryEngine::publish(&store, "rep", "UTILLOG", limits).unwrap();
    let recovered = LogRecoveryEngine::active(&store, "UTILLOG", limits).unwrap();
    assert!(recovered.closed);
    assert!(!recovered.interim);
    assert_eq!(recovered.validate_usable(limits), Ok(()));
}

#[test]
fn close_rejects_corruption_and_psb_report_tracks_active_membership() {
    let store = MemoryStore::new(StoreLimits::default());
    let limits = RecoveryLimits::default();
    let input = LogDataset {
        name: "OLDS001".into(),
        kind: LogDatasetKind::Online,
        closed: false,
        interim: false,
        blocks: vec![
            LogBlock::seal(1, LogPayload::PsbStart("PAY1".into())),
            LogBlock::seal(2, LogPayload::PsbEnd("PAY1".into())),
            LogBlock::seal(3, LogPayload::PsbStart("PAY2".into())),
        ],
    };
    let plan = UtilityPlan {
        kind: UtilityKind::LogRecovery,
        database: "OLDS001".into(),
        expected_input_digest: input.digest(),
        expected_records: 3,
    };
    let mut bad = input.clone();
    bad.blocks[1].sequence = 9;
    let bad_plan = UtilityPlan {
        expected_input_digest: bad.digest(),
        ..plan.clone()
    };
    assert_eq!(
        LogRecoveryEngine::stage_close(&store, "bad-close", bad_plan, bad, limits),
        Err(RecoveryProblem::CorruptImage)
    );
    assert!(LogRecoveryEngine::active(&store, "OLDS001", limits).is_err());
    LogRecoveryEngine::stage_close(&store, "close", plan, input, limits).unwrap();
    LogRecoveryEngine::publish(&store, "close", "OLDS001", limits).unwrap();
    assert_eq!(
        LogRecoveryEngine::active_psbs(&store, "OLDS001", limits).unwrap(),
        vec!["PAY2"]
    );
    assert!(
        LogRecoveryEngine::publish(&store, "close", "OLDS001", limits)
            .unwrap()
            .replayed
    );
}

#[test]
fn duplicate_with_explicit_lsn_truncates_and_rejects_tampered_stage() {
    let store = MemoryStore::new(StoreLimits::default());
    let limits = RecoveryLimits::default();
    let input = LogDataset {
        name: "OLDS002".into(),
        kind: LogDatasetKind::Online,
        closed: false,
        interim: false,
        blocks: vec![
            LogBlock::seal(1, LogPayload::Data(b"before".to_vec())),
            LogBlock::seal(2, LogPayload::Data(b"cut".to_vec())),
        ],
    };
    let plan = UtilityPlan {
        kind: UtilityKind::LogRecovery,
        database: "OLDS002".into(),
        expected_input_digest: input.digest(),
        expected_records: 1,
    };
    LogRecoveryEngine::stage_duplicate(&store, "truncate", plan, input, false, Some(2), limits)
        .unwrap();
    let stage = store
        .get_provider_state("ims-recovery-v1-log-stage", "truncate")
        .unwrap()
        .unwrap();
    store
        .put_provider_state(
            ProviderStateRecord {
                version: 2,
                payload: b"damaged".to_vec(),
                ..stage
            },
            Some(1),
        )
        .unwrap();
    assert_eq!(
        LogRecoveryEngine::publish(&store, "truncate", "OLDS002", limits),
        Err(RecoveryProblem::CorruptImage)
    );
    assert_eq!(
        LogRecoveryEngine::active(&store, "OLDS002", limits),
        Err(RecoveryProblem::NotFound)
    );
}

#[test]
fn duplicate_of_healthy_log_needs_no_replacement_plan() {
    let store = MemoryStore::new(StoreLimits::default());
    let limits = RecoveryLimits::default();
    let input = LogDataset {
        name: "BATCH01".into(),
        kind: LogDatasetKind::Batch,
        closed: true,
        interim: false,
        blocks: vec![LogBlock::seal(1, LogPayload::Data(b"valid".to_vec()))],
    };
    let plan = UtilityPlan {
        kind: UtilityKind::LogRecovery,
        database: "BATCH01".into(),
        expected_input_digest: input.digest(),
        expected_records: 1,
    };
    let staged =
        LogRecoveryEngine::stage_duplicate(&store, "dup", plan, input, false, None, limits)
            .unwrap();
    assert_eq!(staged.error_markers, 0);
    LogRecoveryEngine::publish(&store, "dup", "BATCH01", limits).unwrap();
    assert_eq!(
        LogRecoveryEngine::active(&store, "BATCH01", limits)
            .unwrap()
            .blocks
            .len(),
        1
    );
}
