use super::*;
use crate::database::{
    DatabaseDefinition, DatabaseOrganization, FieldDefinition, SegmentDefinition,
};
use mainframe_env_store::{MemoryStore, SqliteStateStore, StoreLimits};
use mainframe_env_store_api::{ProviderStateRecord, ProviderStateStore};

fn definition() -> DatabaseDefinition {
    DatabaseDefinition {
        name: "UTILDB".into(),
        organization: DatabaseOrganization::Hdam,
        segments: vec![SegmentDefinition {
            name: "ROOT".into(),
            parent: None,
            min_length: 3,
            max_length: 3,
            key_field: Some("KEY".into()),
            fields: vec![FieldDefinition {
                name: "KEY".into(),
                offset: 0,
                length: 2,
            }],
        }],
        secondary_indexes: vec![],
    }
}

fn image() -> UtilityImage {
    UtilityImage {
        definition: definition(),
        records: vec![
            UtilityRecord {
                segment: "ROOT".into(),
                parent: None,
                data: b"B2y".to_vec(),
            },
            UtilityRecord {
                segment: "ROOT".into(),
                parent: None,
                data: b"A1x".to_vec(),
            },
        ],
    }
}

#[test]
fn initial_load_is_staged_then_published_and_extracts_real_records() {
    let store = MemoryStore::new(StoreLimits::default());
    let limits = RecoveryLimits::default();
    let input = image();
    let plan = UtilityPlan {
        kind: UtilityKind::InitialLoad,
        database: "UTILDB".into(),
        expected_input_digest: input.digest(),
        expected_records: 2,
    };
    let staged =
        UtilityEngine::stage_initial_load(&store, "load-1", plan, input.clone(), limits).unwrap();
    assert!(!staged.replayed);
    assert!(UtilityEngine::extract(&store, "UTILDB", limits).is_err());
    let published = UtilityEngine::publish(&store, "load-1", "UTILDB", limits).unwrap();
    assert_eq!(published.generation, 1);
    assert_eq!(
        UtilityEngine::extract(&store, "UTILDB", limits).unwrap(),
        input
    );
}

#[test]
fn reorganization_rebuilds_order_and_indexes_then_publishes_new_generation() {
    let store = MemoryStore::new(StoreLimits::default());
    let limits = RecoveryLimits::default();
    let source = image();
    let load = UtilityPlan {
        kind: UtilityKind::InitialLoad,
        database: "UTILDB".into(),
        expected_input_digest: source.digest(),
        expected_records: 2,
    };
    UtilityEngine::stage_initial_load(&store, "load", load, source.clone(), limits).unwrap();
    UtilityEngine::publish(&store, "load", "UTILDB", limits).unwrap();
    let plan = UtilityPlan {
        kind: UtilityKind::Reorganize,
        database: "UTILDB".into(),
        expected_input_digest: source.digest(),
        expected_records: 2,
    };
    let staged = UtilityEngine::stage_reorganization(&store, "reorg", plan, limits).unwrap();
    assert_ne!(staged.image_digest, source.digest());
    assert_eq!(
        UtilityEngine::extract(&store, "UTILDB", limits).unwrap(),
        source
    );
    let published = UtilityEngine::publish(&store, "reorg", "UTILDB", limits).unwrap();
    assert_eq!(published.generation, 2);
    let organized = UtilityEngine::extract(&store, "UTILDB", limits).unwrap();
    assert_eq!(organized.records[0].data, b"A1x");
    assert_eq!(organized.records[1].data, b"B2y");
    let engine = organized.validate(limits).unwrap();
    assert_eq!(UtilityImage::from_engine(&engine).unwrap(), organized);
    assert!(
        UtilityEngine::publish(&store, "reorg", "UTILDB", limits)
            .unwrap()
            .replayed
    );
}

#[test]
fn database_recovery_replaces_corrupt_active_only_after_verified_logs() {
    let store = MemoryStore::new(StoreLimits::default());
    let limits = RecoveryLimits::default();
    let base = image();
    let load = UtilityPlan {
        kind: UtilityKind::InitialLoad,
        database: "UTILDB".into(),
        expected_input_digest: base.digest(),
        expected_records: 2,
    };
    UtilityEngine::stage_initial_load(&store, "load", load, base.clone(), limits).unwrap();
    UtilityEngine::publish(&store, "load", "UTILDB", limits).unwrap();
    let row = store
        .get_provider_state("ims-recovery-v1-db-active", "UTILDB")
        .unwrap()
        .unwrap();
    store
        .put_provider_state(
            ProviderStateRecord {
                version: row.version + 1,
                payload: b"damaged".to_vec(),
                ..row.clone()
            },
            Some(row.version),
        )
        .unwrap();
    assert!(matches!(
        UtilityEngine::extract(&store, "UTILDB", limits),
        Err(RecoveryProblem::CorruptImage)
    ));
    let delta = UtilityDelta::seal(
        1,
        base.digest(),
        UtilityDeltaChange::Insert(UtilityRecord {
            segment: "ROOT".into(),
            parent: None,
            data: b"C3z".to_vec(),
        }),
    );
    let plan = UtilityPlan {
        kind: UtilityKind::DatabaseRecovery,
        database: "UTILDB".into(),
        expected_input_digest: base.digest(),
        expected_records: 3,
    };
    let mut corrupt = delta.clone();
    corrupt.digest[0] ^= 1;
    assert_eq!(
        UtilityEngine::stage_database_recovery(
            &store,
            "bad",
            plan.clone(),
            base.clone(),
            &[corrupt],
            limits
        ),
        Err(RecoveryProblem::CorruptImage)
    );
    assert!(
        store
            .get_provider_state("ims-recovery-v1-db-stage", "bad")
            .unwrap()
            .is_none()
    );
    UtilityEngine::stage_database_recovery(&store, "recover", plan, base, &[delta], limits)
        .unwrap();
    assert!(matches!(
        UtilityEngine::extract(&store, "UTILDB", limits),
        Err(RecoveryProblem::CorruptImage)
    ));
    let receipt = UtilityEngine::publish(&store, "recover", "UTILDB", limits).unwrap();
    assert_eq!(receipt.generation, 3);
    assert_eq!(
        UtilityEngine::extract(&store, "UTILDB", limits)
            .unwrap()
            .records
            .len(),
        3
    );
}

#[test]
fn staged_reorganization_conflict_preserves_stage_and_active_bytes() {
    let store = MemoryStore::new(StoreLimits::default());
    let limits = RecoveryLimits::default();
    let source = image();
    let load = UtilityPlan {
        kind: UtilityKind::InitialLoad,
        database: "UTILDB".into(),
        expected_input_digest: source.digest(),
        expected_records: 2,
    };
    UtilityEngine::stage_initial_load(&store, "load", load, source.clone(), limits).unwrap();
    UtilityEngine::publish(&store, "load", "UTILDB", limits).unwrap();
    let plan = UtilityPlan {
        kind: UtilityKind::Reorganize,
        database: "UTILDB".into(),
        expected_input_digest: source.digest(),
        expected_records: 2,
    };
    UtilityEngine::stage_reorganization(&store, "reorg", plan, limits).unwrap();
    let active = store
        .get_provider_state("ims-recovery-v1-db-active", "UTILDB")
        .unwrap()
        .unwrap();
    store
        .put_provider_state(
            ProviderStateRecord {
                version: 2,
                payload: b"other-writer".to_vec(),
                ..active.clone()
            },
            Some(1),
        )
        .unwrap();
    assert_eq!(
        UtilityEngine::publish(&store, "reorg", "UTILDB", limits),
        Err(RecoveryProblem::Conflict)
    );
    assert_eq!(
        store
            .get_provider_state("ims-recovery-v1-db-active", "UTILDB")
            .unwrap()
            .unwrap()
            .payload,
        b"other-writer"
    );
    assert!(
        store
            .get_provider_state("ims-recovery-v1-db-stage", "reorg")
            .unwrap()
            .is_some()
    );
}

#[test]
fn sqlite_restart_publishes_previously_staged_initial_load() {
    let suffix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let directory = std::env::temp_dir().join(format!(
        "mainframe-env-ims-utility-{}-{suffix}",
        std::process::id()
    ));
    std::fs::create_dir(&directory).unwrap();
    let path = directory.join("utility.sqlite");
    let url = format!("sqlite://{}?mode=rwc", path.display());
    let limits = RecoveryLimits::default();
    let source = image();
    {
        let store = SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap();
        let plan = UtilityPlan {
            kind: UtilityKind::InitialLoad,
            database: "UTILDB".into(),
            expected_input_digest: source.digest(),
            expected_records: 2,
        };
        UtilityEngine::stage_initial_load(&store, "load", plan, source.clone(), limits).unwrap();
    }
    {
        let store = SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap();
        assert!(matches!(
            UtilityEngine::extract(&store, "UTILDB", limits),
            Err(RecoveryProblem::NotFound)
        ));
        UtilityEngine::publish(&store, "load", "UTILDB", limits).unwrap();
        assert_eq!(
            UtilityEngine::extract(&store, "UTILDB", limits).unwrap(),
            source
        );
    }
    std::fs::remove_dir_all(&directory).unwrap();
}

#[test]
fn corrupt_stage_cannot_publish_or_mutate_active_database() {
    let store = MemoryStore::new(StoreLimits::default());
    let limits = RecoveryLimits::default();
    let source = image();
    let plan = UtilityPlan {
        kind: UtilityKind::InitialLoad,
        database: "UTILDB".into(),
        expected_input_digest: source.digest(),
        expected_records: source.records.len(),
    };
    UtilityEngine::stage_initial_load(&store, "load", plan, source, limits).unwrap();
    let stage = store
        .get_provider_state("ims-recovery-v1-db-stage", "load")
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
        UtilityEngine::publish(&store, "load", "UTILDB", limits),
        Err(RecoveryProblem::CorruptImage)
    );
    assert_eq!(
        UtilityEngine::extract(&store, "UTILDB", limits),
        Err(RecoveryProblem::NotFound)
    );
}

#[test]
fn database_recovery_rejects_sequence_gap_without_stage_or_active_mutation() {
    let store = MemoryStore::new(StoreLimits::default());
    let limits = RecoveryLimits::default();
    let source = image();
    let plan = UtilityPlan {
        kind: UtilityKind::DatabaseRecovery,
        database: "UTILDB".into(),
        expected_input_digest: source.digest(),
        expected_records: source.records.len(),
    };
    let gap = UtilityDelta::seal(
        2,
        source.digest(),
        UtilityDeltaChange::Replace {
            ordinal: 0,
            data: b"B2z".to_vec(),
        },
    );
    assert_eq!(
        UtilityEngine::stage_database_recovery(&store, "gap", plan, source, &[gap], limits),
        Err(RecoveryProblem::CorruptImage)
    );
    assert!(
        store
            .get_provider_state("ims-recovery-v1-db-stage", "gap")
            .unwrap()
            .is_none()
    );
    assert!(
        store
            .get_provider_state("ims-recovery-v1-db-active", "UTILDB")
            .unwrap()
            .is_none()
    );
}
