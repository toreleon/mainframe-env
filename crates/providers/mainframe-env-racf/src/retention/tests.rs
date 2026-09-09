use super::planner::{
    RACF_AUDIT_NAMESPACE, RACF_RECOVERY_NAMESPACE, RACF_TRANSACTION_NAMESPACE, test_build_request,
    test_validate_request,
};
use super::*;
use crate::database::{
    DATABASE_KEY, DATABASE_NAMESPACE, SecurityDatabase, decode_snapshot, encode_snapshot,
};
use crate::model::{
    RecoveryRecord, RecoveryState, SecurityAuditRecord, SecurityDatabaseLimits,
    SecurityTransaction, TransactionState,
};
use crate::{AuditFieldValue, DecisionOutcome, DecisionReason, SafStatus};
use mainframe_env_execution_api::{ExecutionId, InvocationLimits};
use mainframe_env_host_api::HostProblem;
use mainframe_env_store::{MemoryStore, StoreLimits};
use mainframe_env_store_api::{
    ProviderStateRecord, ProviderStateStore, RetentionObservation, RetentionStore, RetentionTarget,
    StoreError,
};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::sync::Arc;

fn policy(batch: usize) -> RacfRetentionPolicy {
    RacfRetentionPolicy {
        retain_ticks: 1,
        low_watermark_percent: 50,
        high_watermark_percent: 75,
        max_batch_records: batch,
    }
}

fn status() -> SafStatus {
    SafStatus {
        saf_return_code: 8,
        racf_return_code: 8,
        racf_reason_code: 4,
        reason: DecisionReason::DefaultDeny,
    }
}

fn audit(id: &str, tick: u64) -> SecurityAuditRecord {
    SecurityAuditRecord {
        id: id.into(),
        correlation: "RETENTION-TEST".into(),
        actor: "SYSTEM".into(),
        action: "AUTHORIZE".into(),
        class: None,
        resource_digest: None,
        decision: DecisionOutcome::Deny,
        status: status(),
        fields: BTreeMap::from([("SAFE".into(), AuditFieldValue::Text("VALUE".into()))]),
        tick,
        retention_observed_tick: None,
    }
}

fn transaction(id: &str, tick: Option<u64>) -> SecurityTransaction {
    SecurityTransaction {
        id: id.into(),
        idempotency_key: id.into(),
        actor: "SYSTEM".into(),
        operation: "RECOVERY".into(),
        request_digest_format: crate::SecurityRequestDigestFormat::RacfCommandCanonicalV1,
        request_digest: format!("sha256:{}", "a".repeat(64)),
        state: TransactionState::Committed,
        base_generation: 1,
        final_generation: Some(2),
        status: status(),
        terminal_result: None,
        terminal_tick: tick,
    }
}

fn recovery(id: &str, transaction_id: &str, tick: Option<u64>) -> RecoveryRecord {
    RecoveryRecord {
        id: id.into(),
        transaction_id: transaction_id.into(),
        state: RecoveryState::Reconciled,
        attempt: 1,
        last_error: None,
        version: 1,
        terminal_tick: tick,
    }
}

fn source(store: &dyn ProviderStateStore) -> ProviderStateRecord {
    store
        .get_provider_state(DATABASE_NAMESPACE, DATABASE_KEY)
        .unwrap()
        .unwrap()
}

fn observation(descriptor: &RacfRetentionDescriptor, tick: u64) -> RetentionObservation {
    RetentionObservation {
        target: RetentionTarget::RacfEvidence,
        namespace: descriptor.namespace.clone(),
        key: descriptor.key.clone(),
        source_version: descriptor.source_version,
        source_digest: descriptor.source_digest,
        observed_tick: tick,
        owner_execution: None,
    }
}

#[test]
fn dedicated_archive_prunes_while_live_provider_row_capacity_is_full() {
    let store = Arc::new(MemoryStore::new(StoreLimits {
        max_provider_state: 1,
        max_blob_bytes: 32 * 1024 * 1024,
        ..StoreLimits::default()
    }));
    let provider: Arc<dyn ProviderStateStore> = store.clone();
    let database = SecurityDatabase::open(provider, Default::default()).unwrap();
    database
        .mutate(|snapshot| {
            snapshot.retention_tick = 1;
            snapshot.audits.push(audit("AUDIT-FULL", 1));
            Ok(())
        })
        .unwrap();
    assert_eq!(
        store
            .list_provider_state(DATABASE_NAMESPACE, 1)
            .unwrap()
            .len(),
        1
    );

    let receipt = database.archive_and_prune(policy(8), 10).unwrap();
    assert_eq!(receipt.archived_audits, 1);
    assert!(receipt.archive_id.is_some());
    assert_eq!(database.summary().unwrap().audits, 0);
    assert_eq!(
        store
            .list_provider_state(DATABASE_NAMESPACE, 1)
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        store
            .provider_retention_archive_usage(RetentionTarget::RacfEvidence)
            .unwrap()
            .0,
        1
    );
}

#[test]
fn unaged_descriptors_are_stable_and_never_grow_the_live_snapshot() {
    let store = Arc::new(MemoryStore::new(StoreLimits::default()));
    let provider: Arc<dyn ProviderStateStore> = store.clone();
    let database = SecurityDatabase::open(provider, Default::default()).unwrap();
    database
        .mutate(|snapshot| {
            snapshot.audits.push(audit("LEGACY-AUDIT", 0));
            snapshot
                .transactions
                .insert("LEGACY-TX".into(), transaction("LEGACY-TX", None));
            snapshot.recovery.insert(
                "LEGACY-RECOVERY".into(),
                recovery("LEGACY-RECOVERY", "LEGACY-TX", None),
            );
            Ok(())
        })
        .unwrap();
    let raw_before = source(store.as_ref());
    let descriptors_before = database.retention_descriptors().unwrap();
    assert_eq!(descriptors_before.len(), 3);
    assert!(
        descriptors_before
            .iter()
            .all(|row| row.source_version == 1 && row.intrinsic_tick.is_none())
    );
    let forecast = database.retention_forecast(policy(8), 10).unwrap();
    assert_eq!(
        (
            forecast.unaged_audits,
            forecast.unaged_transactions,
            forecast.unaged_recovery_records,
        ),
        (1, 1, 1)
    );
    assert_eq!(
        (
            forecast.eligible_audits,
            forecast.eligible_transactions,
            forecast.eligible_recovery_records,
        ),
        (0, 0, 0)
    );
    assert_eq!(source(store.as_ref()), raw_before);
    assert!(
        database
            .archive_and_prune(policy(8), 10)
            .unwrap()
            .archive_id
            .is_none()
    );
    assert_eq!(source(store.as_ref()), raw_before);

    database
        .mutate(|snapshot| {
            snapshot.retention_tick = 20;
            Ok(())
        })
        .unwrap();
    assert_eq!(
        database.retention_descriptors().unwrap(),
        descriptors_before
    );
    let observations = descriptors_before
        .iter()
        .map(|row| observation(row, 10))
        .collect::<Vec<_>>();
    let aggregate = source(store.as_ref());
    let mut epoch = store.provider_state_retention_epoch().unwrap();
    let mut observation_entries = Vec::new();
    for observation in observations {
        let receipt = store
            .record_provider_retention_observation(aggregate.clone(), epoch, observation.clone())
            .unwrap();
        observation_entries.push((receipt.observation_version, observation));
        epoch = store.provider_state_retention_epoch().unwrap();
    }
    let receipt = database
        .archive_and_prune_with_observations(policy(8), 21, &observation_entries, epoch)
        .unwrap();
    assert_eq!(
        (
            receipt.archived_audits,
            receipt.archived_transactions,
            receipt.archived_recovery_records,
        ),
        (1, 1, 1)
    );
}

#[test]
fn terminal_recovery_is_archived_before_its_transaction() {
    let store = Arc::new(MemoryStore::new(StoreLimits::default()));
    let provider: Arc<dyn ProviderStateStore> = store.clone();
    let database = SecurityDatabase::open(provider, Default::default()).unwrap();
    database
        .mutate(|snapshot| {
            snapshot.retention_tick = 1;
            snapshot
                .transactions
                .insert("TX-ORDER".into(), transaction("TX-ORDER", Some(1)));
            snapshot.recovery.insert(
                "RECOVERY-ORDER".into(),
                recovery("RECOVERY-ORDER", "TX-ORDER", Some(1)),
            );
            Ok(())
        })
        .unwrap();

    let first = database.archive_and_prune(policy(1), 10).unwrap();
    assert_eq!(first.archived_recovery_records, 1);
    assert_eq!(first.archived_transactions, 0);
    let second = database.archive_and_prune(policy(1), 10).unwrap();
    assert_eq!(second.archived_transactions, 1);
    let archives = store
        .retention_archives(RetentionTarget::RacfEvidence, 8)
        .unwrap();
    assert_eq!(archives.len(), 2);
    let mut namespaces = archives
        .iter()
        .map(|archive| archive.rows[0].namespace.as_str())
        .collect::<Vec<_>>();
    namespaces.sort_unstable();
    assert_eq!(
        namespaces,
        vec![RACF_RECOVERY_NAMESPACE, RACF_TRANSACTION_NAMESPACE]
    );
}

#[test]
fn canonical_descriptor_digest_matches_the_exact_archived_payload() {
    let store = Arc::new(MemoryStore::new(StoreLimits::default()));
    let provider: Arc<dyn ProviderStateStore> = store.clone();
    let database = SecurityDatabase::open(provider, Default::default()).unwrap();
    let expected = audit("AUDIT-CANONICAL", 1);
    database
        .mutate(|snapshot| {
            snapshot.retention_tick = 1;
            snapshot.audits.push(expected.clone());
            Ok(())
        })
        .unwrap();
    let descriptor = database.retention_descriptors().unwrap().remove(0);
    assert_eq!(descriptor.namespace, RACF_AUDIT_NAMESPACE);
    database.archive_and_prune(policy(1), 10).unwrap();
    let archive = store
        .retention_archives(RetentionTarget::RacfEvidence, 1)
        .unwrap()
        .remove(0);
    let row = &archive.rows[0];
    assert_eq!(row.key, descriptor.key);
    assert_eq!(row.version, 1);
    assert_eq!(
        Sha256::digest(&row.payload).as_slice(),
        descriptor.source_digest
    );
    assert_eq!(
        serde_json::from_slice::<SecurityAuditRecord>(&row.payload).unwrap(),
        expected
    );
}

#[test]
fn provider_validator_rejects_hostile_payload_and_nonexact_delta() {
    let store = Arc::new(MemoryStore::new(StoreLimits::default()));
    let provider: Arc<dyn ProviderStateStore> = store.clone();
    let database = SecurityDatabase::open(provider, Default::default()).unwrap();
    database
        .mutate(|snapshot| {
            snapshot.retention_tick = 1;
            snapshot.audits.push(audit("AUDIT-A", 1));
            snapshot.audits.push(audit("AUDIT-B", 1));
            Ok(())
        })
        .unwrap();
    let source = source(store.as_ref());
    let epoch = store.provider_state_retention_epoch().unwrap();
    let request = test_build_request(
        epoch,
        source.clone(),
        policy(1),
        10,
        &[],
        Default::default(),
    )
    .unwrap();
    test_validate_request(&request, &[], Default::default()).unwrap();

    let source_snapshot = decode_snapshot(&source, Default::default()).unwrap();
    let replacement = decode_snapshot(&request.replacement.record, Default::default()).unwrap();
    assert_eq!(replacement.generation, source_snapshot.generation + 1);
    assert_eq!(replacement.audits.len(), source_snapshot.audits.len() - 1);
    assert_eq!(replacement.transactions, source_snapshot.transactions);
    assert_eq!(replacement.recovery, source_snapshot.recovery);

    let mut hostile_payload = request.clone();
    hostile_payload.rows[0].row.payload.push(b' ');
    assert_eq!(
        test_validate_request(&hostile_payload, &[], Default::default()),
        Err(HostProblem::InfrastructureFailure)
    );

    let mut hostile_delta = request.clone();
    let mut replacement =
        decode_snapshot(&hostile_delta.replacement.record, Default::default()).unwrap();
    replacement.audits.clear();
    hostile_delta.replacement.record.payload =
        encode_snapshot(&replacement, Default::default()).unwrap();
    assert_eq!(
        test_validate_request(&hostile_delta, &[], Default::default()),
        Err(HostProblem::InfrastructureFailure)
    );

    store
        .put_provider_state(
            ProviderStateRecord {
                namespace: "unrelated".into(),
                key: "epoch".into(),
                version: 1,
                payload: vec![1],
            },
            None,
        )
        .unwrap();
    assert_eq!(
        store.archive_provider_state_replacement(request),
        Err(StoreError::Conflict)
    );
}

#[test]
fn exact_max_byte_aggregate_can_shrink_into_the_dedicated_archive() {
    let store = Arc::new(MemoryStore::new(StoreLimits {
        max_provider_state: 1,
        max_retention_archive_bytes: 64 * 1024 * 1024,
        ..StoreLimits::default()
    }));
    let provider: Arc<dyn ProviderStateStore> = store.clone();
    let database = SecurityDatabase::open(provider.clone(), Default::default()).unwrap();
    database
        .mutate(|snapshot| {
            snapshot.retention_tick = 1;
            snapshot.audits.push(audit("AUDIT-EXACT-MAX", 1));
            Ok(())
        })
        .unwrap();
    let before = source(store.as_ref());
    drop(database);
    let limits = SecurityDatabaseLimits {
        max_database_bytes: before.payload.len(),
        ..SecurityDatabaseLimits::default()
    };
    let database = SecurityDatabase::open(provider, limits).unwrap();
    assert_eq!(
        source(store.as_ref()).payload.len(),
        limits.max_database_bytes
    );
    let receipt = database.archive_and_prune(policy(1), 10).unwrap();
    assert_eq!(receipt.archived_audits, 1);
    assert!(source(store.as_ref()).payload.len() < limits.max_database_bytes);
}

#[test]
fn stale_observation_is_protected_and_owner_is_rejected() {
    let store = Arc::new(MemoryStore::new(StoreLimits::default()));
    let provider: Arc<dyn ProviderStateStore> = store.clone();
    let database = SecurityDatabase::open(provider, Default::default()).unwrap();
    database
        .mutate(|snapshot| {
            snapshot.audits.push(audit("LEGACY-OBS", 0));
            Ok(())
        })
        .unwrap();
    let descriptor = database.retention_descriptors().unwrap().remove(0);
    let mut stale = observation(&descriptor, 1);
    stale.source_digest[0] ^= 1;
    assert_eq!(
        database
            .retention_forecast_with_observations(policy(1), 10, &[stale])
            .unwrap()
            .eligible_audits,
        0
    );

    let mut hostile = observation(&descriptor, 1);
    hostile.owner_execution =
        Some(ExecutionId::new("unexpected-owner", InvocationLimits::default()).unwrap());
    assert_eq!(
        database.retention_forecast_with_observations(policy(1), 10, &[hostile]),
        Err(HostProblem::InfrastructureFailure)
    );
}

#[test]
fn pruned_transaction_key_is_a_new_operation_not_archive_replay() {
    let store = Arc::new(MemoryStore::new(StoreLimits::default()));
    let provider: Arc<dyn ProviderStateStore> = store;
    let database = SecurityDatabase::open(provider, Default::default()).unwrap();
    database
        .mutate(|snapshot| {
            snapshot.retention_tick = 1;
            snapshot
                .transactions
                .insert("REUSED-KEY".into(), transaction("REUSED-KEY", Some(1)));
            Ok(())
        })
        .unwrap();
    database.archive_and_prune(policy(1), 10).unwrap();
    let snapshot = database.read().unwrap();
    assert_eq!(
        database
            .transaction_for_replay(&snapshot, "REUSED-KEY")
            .unwrap(),
        None
    );
    database
        .mutate(|snapshot| {
            snapshot.retention_tick = snapshot.retention_tick.max(11);
            let mut replacement = transaction("REUSED-KEY", Some(11));
            replacement.operation = "NEW-OPERATION".into();
            snapshot
                .transactions
                .insert("REUSED-KEY".into(), replacement);
            Ok(())
        })
        .unwrap();
    assert_eq!(database.summary().unwrap().transactions, 1);
}
