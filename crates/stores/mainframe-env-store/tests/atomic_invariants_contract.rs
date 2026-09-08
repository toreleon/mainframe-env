use mainframe_env_execution_api::{
    ArtifactRef, ExecutionId, IdempotencyKey, InvocationLimits, LifecycleEvent, LifecycleEventKind,
    PrincipalId, RunUnitId, Selector,
};
use mainframe_env_store::{MemoryStore, PostgresStateStore, SqliteStateStore, StoreLimits};
use mainframe_env_store_api::{
    ArtifactRecord, CheckpointRecord, EffectDigestFormat, EffectRecord, EffectState,
    ExecutionRecord, ExecutionState, PlatformStore, StoreError,
};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

fn unique(label: &str) -> String {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("{label}-{}-{nonce}", std::process::id())
}

fn execution_record(label: &str) -> ExecutionRecord {
    let limits = InvocationLimits::default();
    ExecutionRecord {
        execution_id: ExecutionId::new(format!("exec-{label}"), limits).unwrap(),
        run_unit_id: RunUnitId::new(format!("run-{label}"), limits).unwrap(),
        selector: Selector::new("program:CONTRACT", limits).unwrap(),
        artifact: ArtifactRef::new(format!("sha256:{}", "a".repeat(64)), limits).unwrap(),
        principal: PrincipalId::new("IBMUSER", limits).unwrap(),
        state: ExecutionState::Admitted,
        attempt: 1,
        version: 1,
        owner_lease: None,
        lease_expiry_tick: None,
    }
}

fn event(execution: &ExecutionRecord, sequence: u64, kind: LifecycleEventKind) -> LifecycleEvent {
    LifecycleEvent {
        execution_id: execution.execution_id.clone(),
        run_unit_id: execution.run_unit_id.clone(),
        sequence,
        attempt: execution.attempt,
        tick: sequence,
        kind,
    }
}

fn notification(
    execution: &ExecutionRecord,
    sequence: u64,
) -> mainframe_env_store_api::OutboxRecord {
    mainframe_env_store_api::OutboxRecord {
        notification_id: format!("{}:{sequence:020}", execution.execution_id),
        execution_id: execution.execution_id.clone(),
        sequence,
        topic: "execution.lifecycle".into(),
        payload: vec![1],
        attempt: 0,
        delivered: false,
        version: 1,
    }
}

fn admit(store: &dyn PlatformStore, execution: &ExecutionRecord) {
    store
        .admit_execution(
            execution.clone(),
            event(execution, 1, LifecycleEventKind::Admitted),
            notification(execution, 1),
        )
        .unwrap();
}

fn checkpoint(execution: &ExecutionRecord) -> CheckpointRecord {
    let payload = format!("checkpoint:{}", execution.execution_id).into_bytes();
    let payload_digest = Sha256::digest(&payload).into();
    CheckpointRecord {
        execution_id: execution.execution_id.clone(),
        run_unit_id: execution.run_unit_id.clone(),
        session_id: None,
        schema_version: 1,
        machine_schema_version: 1,
        artifact: execution.artifact.clone(),
        provider_generation: "contract@1".into(),
        required_host_interfaces: BTreeMap::new(),
        effect_sequence: 1,
        transaction: None,
        principal: execution.principal.clone(),
        security_classification: "application-data".into(),
        encryption_key_reference: None,
        payload_size: payload.len() as u64,
        payload_digest,
        payload,
    }
}

fn intent_record(execution: &ExecutionRecord, key: &str) -> EffectRecord {
    EffectRecord {
        execution_id: execution.execution_id.clone(),
        run_unit_id: execution.run_unit_id.clone(),
        sequence: 1,
        key: IdempotencyKey::new(key, InvocationLimits::default()).unwrap(),
        digest_format: EffectDigestFormat::CanonicalHostV1,
        request_digest: [1; 32],
        state: EffectState::Intent,
        result_digest: None,
    }
}

fn assert_journal_unchanged(
    store: &dyn PlatformStore,
    execution: &ExecutionRecord,
    expected_events: usize,
) {
    let retained = store
        .get_execution(&execution.execution_id)
        .unwrap()
        .unwrap();
    assert_eq!(retained.version, expected_events as u64);
    assert_eq!(
        store.events(&execution.execution_id, 1, 64).unwrap().len(),
        expected_events
    );
    assert_eq!(
        store
            .pending_notifications(65_536)
            .unwrap()
            .into_iter()
            .filter(|record| record.execution_id == execution.execution_id)
            .count(),
        expected_events
    );
}

fn assert_admission_invariants(store: &dyn PlatformStore, prefix: &str) {
    let execution = execution_record(&format!("{prefix}-admission-run"));
    let mut hostile_event = event(&execution, 1, LifecycleEventKind::Admitted);
    hostile_event.run_unit_id = RunUnitId::new("run-hostile", InvocationLimits::default()).unwrap();
    assert_eq!(
        store.admit_execution(
            execution.clone(),
            hostile_event,
            notification(&execution, 1)
        ),
        Err(StoreError::InvalidSequence)
    );
    assert_eq!(store.get_execution(&execution.execution_id).unwrap(), None);

    let execution = execution_record(&format!("{prefix}-admission-outbox"));
    let mut hostile_notification = notification(&execution, 1);
    hostile_notification.topic.clear();
    assert_eq!(
        store.admit_execution(
            execution.clone(),
            event(&execution, 1, LifecycleEventKind::Admitted),
            hostile_notification
        ),
        Err(StoreError::InvalidTransition)
    );
    assert_eq!(store.get_execution(&execution.execution_id).unwrap(), None);
}

fn assert_artifact_invariants(store: &dyn PlatformStore, prefix: &str) {
    let payload = format!("artifact-{prefix}").into_bytes();
    let hash = Sha256::digest(&payload);
    let artifact =
        ArtifactRef::new(format!("sha256:{hash:x}"), InvocationLimits::default()).unwrap();
    let valid = ArtifactRecord {
        artifact: artifact.clone(),
        media_type: "application/octet-stream".into(),
        payload_digest: hash.into(),
        payload,
    };
    let mut hostile = valid.clone();
    hostile.payload_digest = [9; 32];
    assert_eq!(
        store.put_artifact(hostile),
        Err(StoreError::IncompatibleVersion)
    );
    assert_eq!(store.get_artifact(&artifact).unwrap(), None);

    let mut hostile = valid.clone();
    hostile.artifact = ArtifactRef::new(
        format!("sha256:{}", "0".repeat(64)),
        InvocationLimits::default(),
    )
    .unwrap();
    assert_eq!(
        store.put_artifact(hostile),
        Err(StoreError::IncompatibleVersion)
    );
    assert_eq!(store.get_artifact(&artifact).unwrap(), None);
    store.put_artifact(valid.clone()).unwrap();
    assert_eq!(store.get_artifact(&artifact).unwrap(), Some(valid));
}

fn assert_direct_checkpoint_invariants(store: &dyn PlatformStore, prefix: &str) {
    let execution = execution_record(&format!("{prefix}-direct-checkpoint"));
    let valid = checkpoint(&execution);
    let mut hostile_records = Vec::new();

    let mut hostile = valid.clone();
    hostile.schema_version = 2;
    hostile_records.push(hostile);
    let mut hostile = valid.clone();
    hostile.machine_schema_version = 0;
    hostile_records.push(hostile);
    let mut hostile = valid.clone();
    hostile.provider_generation.clear();
    hostile_records.push(hostile);
    let mut hostile = valid.clone();
    hostile.security_classification.clear();
    hostile_records.push(hostile);
    let mut hostile = valid.clone();
    hostile.payload_size += 1;
    hostile_records.push(hostile);
    let mut hostile = valid.clone();
    hostile.payload_digest = [7; 32];
    hostile_records.push(hostile);
    let mut hostile = valid.clone();
    hostile.payload.clear();
    hostile.payload_size = 0;
    hostile.payload_digest = Sha256::digest([]).into();
    hostile_records.push(hostile);

    for hostile in hostile_records {
        assert_eq!(
            store.put_checkpoint(hostile),
            Err(StoreError::IncompatibleVersion)
        );
        assert_eq!(store.get_checkpoint(&execution.execution_id).unwrap(), None);
    }
    store.put_checkpoint(valid.clone()).unwrap();
    assert_eq!(
        store.get_checkpoint(&execution.execution_id).unwrap(),
        Some(valid)
    );
}

fn mutate_checkpoint(case: &str, record: &mut CheckpointRecord) {
    let limits = InvocationLimits::default();
    match case {
        "schema" => record.schema_version = 2,
        "machine-schema" => record.machine_schema_version = 0,
        "generation" => record.provider_generation.clear(),
        "classification" => record.security_classification.clear(),
        "payload-size" => record.payload_size += 1,
        "digest" => record.payload_digest = [8; 32],
        "empty-payload" => {
            record.payload.clear();
            record.payload_size = 0;
            record.payload_digest = Sha256::digest([]).into();
        }
        "execution" => {
            record.execution_id = ExecutionId::new("exec-hostile", limits).unwrap();
        }
        "run-unit" => record.run_unit_id = RunUnitId::new("run-hostile", limits).unwrap(),
        "artifact" => record.artifact = ArtifactRef::new("artifact:hostile", limits).unwrap(),
        "principal" => record.principal = PrincipalId::new("OTHER", limits).unwrap(),
        _ => unreachable!(),
    }
}

fn assert_atomic_checkpoint_invariants(store: &dyn PlatformStore, prefix: &str) {
    for case in [
        "schema",
        "machine-schema",
        "generation",
        "classification",
        "payload-size",
        "digest",
        "empty-payload",
        "execution",
        "run-unit",
        "artifact",
        "principal",
    ] {
        let execution = execution_record(&format!("{prefix}-journal-checkpoint-{case}"));
        admit(store, &execution);
        let mut hostile = checkpoint(&execution);
        mutate_checkpoint(case, &mut hostile);
        assert_eq!(
            store.commit_execution_step(
                &execution.execution_id,
                1,
                None,
                event(&execution, 2, LifecycleEventKind::Suspended),
                None,
                Some(hostile),
                notification(&execution, 2),
            ),
            Err(StoreError::IncompatibleVersion),
            "checkpoint mutation {case}"
        );
        assert_journal_unchanged(store, &execution, 1);
        assert_eq!(store.get_checkpoint(&execution.execution_id).unwrap(), None);
    }

    let execution = execution_record(&format!("{prefix}-journal-checkpoint-valid"));
    admit(store, &execution);
    let valid = checkpoint(&execution);
    let updated = store
        .commit_execution_step(
            &execution.execution_id,
            1,
            None,
            event(&execution, 2, LifecycleEventKind::Suspended),
            None,
            Some(valid.clone()),
            notification(&execution, 2),
        )
        .unwrap();
    assert_eq!(updated.version, 2);
    assert_eq!(
        store.get_checkpoint(&execution.execution_id).unwrap(),
        Some(valid)
    );
}

fn mutate_result(case: &str, record: &mut EffectRecord) -> StoreError {
    let limits = InvocationLimits::default();
    match case {
        "execution" => {
            record.execution_id = ExecutionId::new("exec-hostile", limits).unwrap();
            StoreError::Conflict
        }
        "run-unit" => {
            record.run_unit_id = RunUnitId::new("run-hostile", limits).unwrap();
            StoreError::Conflict
        }
        "sequence" => {
            record.sequence += 1;
            StoreError::Conflict
        }
        "format" => {
            record.digest_format = EffectDigestFormat::LegacyDebug;
            StoreError::Conflict
        }
        "request-digest" => {
            record.request_digest = [2; 32];
            StoreError::Conflict
        }
        "missing-result-digest" => {
            record.result_digest = None;
            StoreError::InvalidTransition
        }
        _ => unreachable!(),
    }
}

fn assert_direct_effect_invariants(store: &dyn PlatformStore, prefix: &str) {
    let execution = execution_record(&format!("{prefix}-direct-effect"));
    let intent = intent_record(&execution, &format!("key-{prefix}-direct-effect"));
    store.record_intent(intent.clone()).unwrap();
    for case in [
        "execution",
        "run-unit",
        "sequence",
        "format",
        "request-digest",
        "missing-result-digest",
    ] {
        let mut hostile = EffectRecord {
            state: EffectState::Completed,
            result_digest: Some([3; 32]),
            ..intent.clone()
        };
        let expected = mutate_result(case, &mut hostile);
        assert_eq!(
            store.record_result(&intent.key, hostile),
            Err(expected),
            "direct result mutation {case}"
        );
        assert_eq!(store.effect(&intent.key).unwrap(), Some(intent.clone()));
    }
    let completed = EffectRecord {
        state: EffectState::Completed,
        result_digest: Some([3; 32]),
        ..intent.clone()
    };
    store.record_result(&intent.key, completed.clone()).unwrap();
    assert_eq!(store.effect(&intent.key).unwrap(), Some(completed));
}

fn assert_atomic_effect_invariants(store: &dyn PlatformStore, prefix: &str) {
    for case in [
        "execution",
        "run-unit",
        "sequence",
        "format",
        "request-digest",
        "missing-result-digest",
    ] {
        let execution = execution_record(&format!("{prefix}-journal-effect-{case}"));
        admit(store, &execution);
        let intent = intent_record(&execution, &format!("key-{prefix}-journal-effect-{case}"));
        store
            .commit_execution_step(
                &execution.execution_id,
                1,
                None,
                event(
                    &execution,
                    2,
                    LifecycleEventKind::EffectIntent { sequence: 1 },
                ),
                Some(intent.clone()),
                None,
                notification(&execution, 2),
            )
            .unwrap();
        let mut hostile = EffectRecord {
            state: EffectState::Completed,
            result_digest: Some([3; 32]),
            ..intent.clone()
        };
        let expected = mutate_result(case, &mut hostile);
        assert_eq!(
            store.commit_execution_step(
                &execution.execution_id,
                2,
                None,
                event(
                    &execution,
                    3,
                    LifecycleEventKind::EffectResult { sequence: 1 },
                ),
                Some(hostile),
                None,
                notification(&execution, 3),
            ),
            Err(expected),
            "atomic result mutation {case}"
        );
        assert_journal_unchanged(store, &execution, 2);
        assert_eq!(store.effect(&intent.key).unwrap(), Some(intent));
    }

    let execution = execution_record(&format!("{prefix}-journal-effect-event-sequence"));
    admit(store, &execution);
    let intent = intent_record(
        &execution,
        &format!("key-{prefix}-journal-effect-event-sequence"),
    );
    assert_eq!(
        store.commit_execution_step(
            &execution.execution_id,
            1,
            None,
            event(
                &execution,
                2,
                LifecycleEventKind::EffectIntent { sequence: 2 },
            ),
            Some(intent.clone()),
            None,
            notification(&execution, 2),
        ),
        Err(StoreError::InvalidSequence)
    );
    assert_journal_unchanged(store, &execution, 1);
    assert_eq!(store.effect(&intent.key).unwrap(), None);

    let execution = execution_record(&format!("{prefix}-journal-effect-valid"));
    admit(store, &execution);
    let intent = intent_record(&execution, &format!("key-{prefix}-journal-effect-valid"));
    store
        .commit_execution_step(
            &execution.execution_id,
            1,
            None,
            event(
                &execution,
                2,
                LifecycleEventKind::EffectIntent { sequence: 1 },
            ),
            Some(intent.clone()),
            None,
            notification(&execution, 2),
        )
        .unwrap();
    let completed = EffectRecord {
        state: EffectState::Completed,
        result_digest: Some([3; 32]),
        ..intent.clone()
    };
    let updated = store
        .commit_execution_step(
            &execution.execution_id,
            2,
            None,
            event(
                &execution,
                3,
                LifecycleEventKind::EffectResult { sequence: 1 },
            ),
            Some(completed.clone()),
            None,
            notification(&execution, 3),
        )
        .unwrap();
    assert_eq!(updated.version, 3);
    assert_eq!(store.effect(&intent.key).unwrap(), Some(completed));
}

fn run_contract(store: &dyn PlatformStore, label: &str) {
    let prefix = unique(label);
    assert_admission_invariants(store, &prefix);
    assert_artifact_invariants(store, &prefix);
    assert_direct_checkpoint_invariants(store, &prefix);
    assert_atomic_checkpoint_invariants(store, &prefix);
    assert_direct_effect_invariants(store, &prefix);
    assert_atomic_effect_invariants(store, &prefix);
}

#[test]
fn memory_atomic_invariants_contract() {
    run_contract(
        &MemoryStore::new(StoreLimits::default()),
        "memory-atomic-invariants",
    );
}

#[test]
fn sqlite_atomic_invariants_contract() {
    run_contract(
        &SqliteStateStore::open("sqlite::memory:", 64 * 1024 * 1024, 262_144).unwrap(),
        "sqlite-atomic-invariants",
    );
}

#[test]
#[ignore = "requires isolated MAINFRAME_ENV_POSTGRES_TEST_URL"]
fn postgres_atomic_invariants_contract() {
    let url = std::env::var("MAINFRAME_ENV_POSTGRES_TEST_URL")
        .expect("explicit PostgreSQL test URL required");
    run_contract(
        &PostgresStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap(),
        "postgres-atomic-invariants",
    );
}
