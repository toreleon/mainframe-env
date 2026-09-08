use mainframe_env_execution_api::{
    ArtifactRef, AuditDecision, AuditRecord, AuditResourceDigest, AuditResourceDigestFormat,
    CapabilityId, ExecutionId, IdempotencyKey, InvocationLimits, LifecycleEvent,
    LifecycleEventKind, PrincipalId, RunUnitId, Selector,
};
use mainframe_env_store::{MemoryStore, PostgresStateStore, SqliteStateStore, StoreLimits};
use mainframe_env_store_api::{
    AuditSink, CheckpointRecord, CheckpointStore, EffectDigestFormat, EffectIntentMetadata,
    EffectRecord, EffectState, EventStore, ExecutionRecord, ExecutionState, OutboxRecord,
    PlatformStore, ProviderRetentionDependency, ProviderRetentionObservationDeletion,
    ProviderRetentionObservationSource, ProviderRetentionRow, ProviderStateArchiveDeletion,
    ProviderStateArchiveReplacement, ProviderStateRecord, ProviderStateStore, ProviderStateWrite,
    RetentionAgeReconciliation, RetentionArchive, RetentionArchivePruneOutcome,
    RetentionArchivePruneRequest, RetentionObservation, RetentionObservationProof, RetentionPolicy,
    RetentionReconciliationReceipt, RetentionRequest, RetentionStore, RetentionTarget,
    SaturationLevel, StoreError, WorkRecord, WorkState,
};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::sync::{Arc, Barrier};

fn policy() -> RetentionPolicy {
    RetentionPolicy {
        lifecycle_ticks: 10,
        idempotency_ticks: 10,
        audit_ticks: 20,
        archive_ticks: 50,
        low_watermark_percent: 70,
        high_watermark_percent: 85,
        max_batch: 64,
    }
}

fn dependency_sensitive(target: RetentionTarget) -> bool {
    matches!(
        target,
        RetentionTarget::ResolvedEffects
            | RetentionTarget::TerminalWork
            | RetentionTarget::LifecycleEvents
            | RetentionTarget::TerminalExecutions
    )
}

fn archive(
    store: &dyn PlatformStore,
    policy: RetentionPolicy,
    request: RetentionRequest,
) -> Result<mainframe_env_store_api::RetentionReceipt, StoreError> {
    if dependency_sensitive(request.target) {
        store.archive_and_prune_with_dependencies(
            policy,
            request,
            &mainframe_env_store_api::CoreRetentionDependencySnapshot {
                expected_epoch: store.provider_state_retention_epoch()?,
                blocked_executions: Vec::new(),
                blocked_effect_keys: Vec::new(),
                unowned: false,
            },
        )
    } else {
        store.archive_and_prune(policy, request)
    }
}

fn retention_forecast(
    store: &dyn PlatformStore,
    target: RetentionTarget,
    policy: RetentionPolicy,
    now_tick: u64,
    growth: u64,
) -> Result<mainframe_env_store_api::RetentionForecast, StoreError> {
    if dependency_sensitive(target) {
        store.retention_forecast_with_dependencies(
            target,
            policy,
            now_tick,
            growth,
            &mainframe_env_store_api::CoreRetentionDependencySnapshot {
                expected_epoch: store.provider_state_retention_epoch()?,
                blocked_executions: Vec::new(),
                blocked_effect_keys: Vec::new(),
                unowned: false,
            },
        )
    } else {
        store.retention_forecast(target, policy, now_tick, growth)
    }
}

struct Ids {
    execution: ExecutionId,
    run: RunUnitId,
    artifact: ArtifactRef,
    principal: PrincipalId,
    selector: Selector,
}

fn ids(label: &str) -> Ids {
    let limits = InvocationLimits::default();
    Ids {
        execution: ExecutionId::new(format!("retention-{label}"), limits).unwrap(),
        run: RunUnitId::new(format!("retention-run-{label}"), limits).unwrap(),
        artifact: ArtifactRef::new("sha256:retention-artifact", limits).unwrap(),
        principal: PrincipalId::new("IBMUSER", limits).unwrap(),
        selector: Selector::new("program:RETENTION", limits).unwrap(),
    }
}

fn execution(ids: &Ids) -> ExecutionRecord {
    ExecutionRecord {
        execution_id: ids.execution.clone(),
        run_unit_id: ids.run.clone(),
        selector: ids.selector.clone(),
        artifact: ids.artifact.clone(),
        principal: ids.principal.clone(),
        state: ExecutionState::Admitted,
        attempt: 1,
        version: 1,
        owner_lease: None,
        lease_expiry_tick: None,
        terminal_tick: None,
    }
}

fn event(ids: &Ids, sequence: u64, tick: u64) -> LifecycleEvent {
    LifecycleEvent {
        execution_id: ids.execution.clone(),
        run_unit_id: ids.run.clone(),
        sequence,
        attempt: 1,
        tick,
        kind: LifecycleEventKind::Completed { return_code: 0 },
    }
}

fn outbox(ids: &Ids, sequence: u64) -> OutboxRecord {
    OutboxRecord {
        notification_id: format!("{}:{sequence:020}", ids.execution),
        execution_id: ids.execution.clone(),
        sequence,
        topic: "execution.lifecycle".into(),
        payload: b"terminal".to_vec(),
        attempt: 0,
        delivered: false,
        delivered_tick: None,
        version: 1,
    }
}

fn finish_execution(store: &dyn PlatformStore, ids: &Ids) {
    store.create_execution(execution(ids)).unwrap();
    let queued = store
        .transition_execution(&ids.execution, 1, ExecutionState::Queued, 1)
        .unwrap();
    let running = store
        .transition_execution(&ids.execution, queued.version, ExecutionState::Running, 2)
        .unwrap();
    let completing = store
        .transition_execution(
            &ids.execution,
            running.version,
            ExecutionState::Completing,
            3,
        )
        .unwrap();
    store
        .transition_execution(
            &ids.execution,
            completing.version,
            ExecutionState::Completed,
            4,
        )
        .unwrap();
}

fn finish_execution_at_zero(store: &dyn PlatformStore, ids: &Ids) {
    store.create_execution(execution(ids)).unwrap();
    let queued = store
        .transition_execution(&ids.execution, 1, ExecutionState::Queued, 0)
        .unwrap();
    let running = store
        .transition_execution(&ids.execution, queued.version, ExecutionState::Running, 0)
        .unwrap();
    let completing = store
        .transition_execution(
            &ids.execution,
            running.version,
            ExecutionState::Completing,
            0,
        )
        .unwrap();
    let completed = store
        .transition_execution(
            &ids.execution,
            completing.version,
            ExecutionState::Completed,
            0,
        )
        .unwrap();
    assert_eq!(completed.terminal_tick, None);
}

fn checkpoint(ids: &Ids) -> CheckpointRecord {
    let payload = b"live checkpoint".to_vec();
    CheckpointRecord {
        execution_id: ids.execution.clone(),
        run_unit_id: ids.run.clone(),
        session_id: None,
        schema_version: 1,
        machine_schema_version: 1,
        artifact: ids.artifact.clone(),
        provider_generation: "retention@1".into(),
        required_host_interfaces: BTreeMap::new(),
        effect_sequence: 0,
        transaction: None,
        principal: ids.principal.clone(),
        security_classification: "internal".into(),
        encryption_key_reference: None,
        payload_size: payload.len() as u64,
        payload_digest: Sha256::digest(&payload).into(),
        payload,
    }
}

fn effect(ids: &Ids, key: &str, state: EffectState) -> EffectRecord {
    EffectRecord {
        execution_id: ids.execution.clone(),
        run_unit_id: ids.run.clone(),
        sequence: 1,
        key: IdempotencyKey::new(key, InvocationLimits::default()).unwrap(),
        digest_format: EffectDigestFormat::CanonicalHostV1,
        request_digest: [1; 32],
        intent: EffectIntentMetadata {
            owner: ids.execution.clone(),
            attempt: 1,
            capability: Some(
                CapabilityId::new("host.retention.test", InvocationLimits::default()).unwrap(),
            ),
            audit_resource: None,
            audit_invocation_key: None,
            created_tick: 10,
            recovery_after_tick: 20,
            epoch: 1,
            recovery_lease: None,
        },
        state,
        result_digest: (state != EffectState::Intent).then_some([2; 32]),
        resolved_tick: matches!(state, EffectState::Completed | EffectState::Failed).then_some(10),
    }
}

fn work(ids: &Ids, key: &str) -> WorkRecord {
    WorkRecord {
        work_id: key.into(),
        execution_id: ids.execution.clone(),
        required_selector: ids.selector.clone(),
        required_generation: "retention@1".into(),
        artifact: ids.artifact.clone(),
        state: WorkState::Queued,
        priority: 0,
        attempt: 0,
        max_attempts: 3,
        available_tick: 1,
        deadline_tick: 1_000,
        cancellation_requested: false,
        worker_id: None,
        lease_id: None,
        lease_epoch: 0,
        lease_expiry_tick: None,
        heartbeat_tick: None,
        terminal_tick: None,
        checkpoint_id: None,
        effect_sequence: 0,
        payload: b"retained work".to_vec(),
    }
}

fn audit(ids: &Ids, key: &str, sequence: u64, tick: u64) -> AuditRecord {
    AuditRecord {
        execution_id: ids.execution.clone(),
        run_unit_id: ids.run.clone(),
        attempt: 1,
        effect_sequence: sequence,
        observed_tick: tick,
        principal: ids.principal.clone(),
        invocation_key: IdempotencyKey::new(key, InvocationLimits::default()).unwrap(),
        capability: CapabilityId::new("host.retention.test", InvocationLimits::default()).unwrap(),
        resource: AuditResourceDigest {
            format: AuditResourceDigestFormat::CanonicalHostResourceV1,
            value: [7; 32],
        },
        decision: AuditDecision::Success,
    }
}

fn replay(
    namespace: &str,
    schema: &str,
    key: &str,
    deadline: u64,
    owner: &ExecutionId,
) -> ProviderStateRecord {
    let value = match namespace {
        "db2-v1-replay" => serde_json::json!({
            "request_digest_format":"mainframe-env.provider-replay-canonical@1",
            "request_sha256":vec![1_u8; 32],
            "recorded_deadline_tick":deadline,
            "owner_execution":owner.as_str(),
            "sqlcode":0,
            "sqlstate":"00000",
            "message":"OK",
            "rows":[],
            "affected_rows":0,
        }),
        "ims-v1-replay" => serde_json::json!({
            "request_digest_format":"mainframe-env.provider-replay-canonical@1",
            "request_sha256":vec![1_u8; 32],
            "recorded_deadline_tick":deadline,
            "owner_execution":owner.as_str(),
            "status":"OK",
            "segments":[],
            "checkpoint_id":null,
            "affected_segments":0,
        }),
        "mq-v1-replay" => serde_json::json!({
            "request_digest_format":"mainframe-env.provider-replay-canonical@1",
            "request_sha256":vec![1_u8; 32],
            "recorded_deadline_tick":deadline,
            "owner_execution":owner.as_str(),
            "completion_code":0,
            "reason_code":0,
            "handle":null,
            "message":[],
            "message_id":null,
            "correlation_id":null,
            "trigger_program":null,
        }),
        other => panic!("unsupported replay fixture namespace {other}"),
    };
    ProviderStateRecord {
        namespace: namespace.into(),
        key: key.into(),
        version: 1,
        payload: serde_json::to_vec(&serde_json::json!({
            "schema_version":schema,
            "object_key":key,
            "value":value
        }))
        .unwrap(),
    }
}

fn cics_replay(key: &str, deadline: u64, owner: &ExecutionId, legacy: bool) -> ProviderStateRecord {
    fn field(output: &mut Vec<u8>, value: &[u8]) {
        output.extend_from_slice(&u32::try_from(value.len()).unwrap().to_be_bytes());
        output.extend_from_slice(value);
    }
    let mut payload = if legacy {
        b"MECER001".to_vec()
    } else {
        let mut value = b"MECER002".to_vec();
        field(&mut value, owner.as_str().as_bytes());
        value.extend_from_slice(&deadline.to_be_bytes());
        value
    };
    payload.extend_from_slice(&[3; 32]);
    payload.push(1);
    field(&mut payload, b"NORMAL");
    payload.extend_from_slice(&0_i32.to_be_bytes());
    payload.extend_from_slice(&0_i32.to_be_bytes());
    field(&mut payload, b"CICSPRD");
    field(&mut payload, b"SYS1");
    field(&mut payload, b"TX01");
    payload.push(0x7d);
    payload.push(0);
    payload.push(0);
    field(&mut payload, b"mainframe-env.cics-response@1");
    field(&mut payload, b"");
    payload.extend_from_slice(&0_u32.to_be_bytes());
    payload.push(1);
    ProviderStateRecord {
        namespace: "cics-effect-replay-v1".into(),
        key: key.into(),
        version: 1,
        payload,
    }
}

fn run_contract(store: &dyn PlatformStore) {
    let prunable = ids("prunable");
    let checkpointed = ids("checkpointed");
    let event_prunable = ids("event-only-prunable");
    let audit_prunable = ids("audit-prunable");
    let audit_nonterminal = ids("audit-nonterminal");
    let audit_direct = ids("audit-direct-without-execution");
    for ids in [&prunable, &checkpointed] {
        finish_execution(store, ids);
        store.append_event(event(ids, 1, 10)).unwrap();
        store.append_event(event(ids, 2, 20)).unwrap();
        for sequence in 1..=2 {
            let row = outbox(ids, sequence);
            store.append_notification(row.clone()).unwrap();
            store
                .mark_notification_delivered(&row.notification_id, 1, 30 + sequence)
                .unwrap();
        }
    }
    finish_execution(store, &event_prunable);
    store.append_event(event(&event_prunable, 1, 10)).unwrap();
    store.append_event(event(&event_prunable, 2, 20)).unwrap();
    store.put_checkpoint(checkpoint(&checkpointed)).unwrap();
    finish_execution(store, &audit_prunable);
    store
        .create_execution(execution(&audit_nonterminal))
        .unwrap();

    let complete = effect(&prunable, "retention-complete", EffectState::Completed);
    let intent = effect(&prunable, "retention-intent", EffectState::Intent);
    store
        .record_intent(effect(
            &prunable,
            complete.key.as_str(),
            EffectState::Intent,
        ))
        .unwrap();
    store
        .record_result(&complete.key, complete.clone())
        .unwrap();
    store.record_intent(intent.clone()).unwrap();
    let protected_effect = effect(
        &checkpointed,
        "retention-checkpoint-effect",
        EffectState::Completed,
    );
    store
        .record_intent(effect(
            &checkpointed,
            protected_effect.key.as_str(),
            EffectState::Intent,
        ))
        .unwrap();
    store
        .record_result(&protected_effect.key, protected_effect.clone())
        .unwrap();

    for record in [
        audit(&audit_prunable, "audit-prunable", 1, 10),
        audit(&checkpointed, "audit-checkpoint", 1, 10),
        audit(&prunable, "audit-unresolved-effect", 1, 10),
        audit(&audit_nonterminal, "audit-nonterminal", 1, 10),
        audit(&audit_direct, "audit-direct", 1, 10),
    ] {
        store.record_audit(record).unwrap();
    }
    let audit_forecast = store
        .retention_forecast(RetentionTarget::Audit, policy(), 100, 1)
        .unwrap();
    assert_eq!(
        (
            audit_forecast.active_records,
            audit_forecast.eligible_records
        ),
        (5, 2)
    );
    let audit_receipt = store
        .archive_and_prune(
            policy(),
            RetentionRequest {
                target: RetentionTarget::Audit,
                now_tick: 40,
                max_records: 8,
            },
        )
        .unwrap();
    assert_eq!((audit_receipt.pruned, audit_receipt.protected), (2, 3));
    assert!(
        store
            .audit_records(&audit_prunable.execution, 1, 8)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        store
            .audit_records(&audit_nonterminal.execution, 1, 8)
            .unwrap()
            .len(),
        1
    );
    assert!(
        store
            .audit_records(&audit_direct.execution, 1, 8)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        store
            .audit_records(&checkpointed.execution, 1, 8)
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        store
            .audit_records(&prunable.execution, 1, 8)
            .unwrap()
            .len(),
        1
    );

    for (target, expected) in [
        (RetentionTarget::LifecycleEvents, 2),
        (RetentionTarget::DeliveredOutbox, 2),
        (RetentionTarget::ResolvedEffects, 1),
    ] {
        let receipt = archive(
            store,
            policy(),
            RetentionRequest {
                target,
                now_tick: 100,
                max_records: 8,
            },
        )
        .unwrap();
        assert_eq!((receipt.archived, receipt.pruned), (expected, expected));
        assert!(receipt.protected > 0);
        assert_eq!(store.retention_archives(target, 8).unwrap().len(), 1);
    }

    assert_eq!(store.events(&prunable.execution, 1, 8).unwrap().len(), 2);
    assert!(
        store
            .events(&event_prunable.execution, 1, 8)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        store.events(&checkpointed.execution, 1, 8).unwrap().len(),
        2
    );
    assert!(store.effect(&complete.key).unwrap().is_none());
    assert_eq!(store.effect(&intent.key).unwrap(), Some(intent));
    assert_eq!(
        store.effect(&protected_effect.key).unwrap(),
        Some(protected_effect)
    );
    assert!(
        store
            .get_checkpoint(&checkpointed.execution)
            .unwrap()
            .is_some()
    );
    assert_eq!(store.pending_notifications(8).unwrap().len(), 0);
    assert_eq!(
        store.prune_retention_archives(policy(), 200, 64).unwrap(),
        7
    );
}

fn complete_work(store: &dyn PlatformStore, ids: &Ids, key: &str) {
    store.enqueue(work(ids, key)).unwrap();
    let claimed = store
        .claim("retention-worker", Some("retention@1"), 1, 10)
        .unwrap()
        .unwrap();
    assert_eq!(claimed.work_id, key);
    store
        .complete(
            key,
            claimed.lease_id.as_deref().unwrap(),
            claimed.lease_epoch,
            2,
        )
        .unwrap();
}

fn run_terminal_lifecycle_contract(store: &dyn PlatformStore) {
    // Contract helpers may share one durable backend during the real-PostgreSQL gate. Drain
    // independently eligible rows from an earlier helper so assertions below are scoped to the
    // three executions created here rather than global leftovers.
    for target in [
        RetentionTarget::TerminalWork,
        RetentionTarget::TerminalExecutions,
    ] {
        loop {
            let receipt = archive(
                store,
                policy(),
                RetentionRequest {
                    target,
                    now_tick: 100,
                    max_records: 8,
                },
            )
            .unwrap();
            if receipt.pruned == 0 {
                break;
            }
        }
    }
    let prunable = ids("terminal-prunable");
    let checkpointed = ids("terminal-checkpointed");
    let unresolved = ids("terminal-unresolved-effect");
    finish_execution(store, &prunable);
    finish_execution(store, &checkpointed);
    finish_execution(store, &unresolved);
    complete_work(store, &prunable, "a-terminal-work");
    complete_work(store, &checkpointed, "b-terminal-work");
    complete_work(store, &unresolved, "c-terminal-work");
    store.put_checkpoint(checkpoint(&checkpointed)).unwrap();
    store
        .record_intent(effect(
            &unresolved,
            "terminal-unresolved-intent",
            EffectState::Intent,
        ))
        .unwrap();

    let executions_before = archive(
        store,
        policy(),
        RetentionRequest {
            target: RetentionTarget::TerminalExecutions,
            now_tick: 100,
            max_records: 8,
        },
    )
    .unwrap();
    assert_eq!(executions_before.pruned, 0);
    let work = archive(
        store,
        policy(),
        RetentionRequest {
            target: RetentionTarget::TerminalWork,
            now_tick: 100,
            max_records: 8,
        },
    )
    .unwrap();
    assert_eq!((work.pruned, work.protected), (1, 2));
    assert!(store.get_work("a-terminal-work").unwrap().is_none());
    assert!(store.get_work("b-terminal-work").unwrap().is_some());
    assert!(store.get_work("c-terminal-work").unwrap().is_some());

    let executions = archive(
        store,
        policy(),
        RetentionRequest {
            target: RetentionTarget::TerminalExecutions,
            now_tick: 100,
            max_records: 8,
        },
    )
    .unwrap();
    assert_eq!(executions.pruned, 1);
    assert!(executions.protected >= 2);
    assert!(store.get_execution(&prunable.execution).unwrap().is_none());
    assert!(
        store
            .get_execution(&checkpointed.execution)
            .unwrap()
            .is_some()
    );
}

fn run_event_dependency_ordering_contract(store: &dyn PlatformStore) {
    let owner = ids("event-dependency-ordering");
    finish_execution(store, &owner);
    store.append_event(event(&owner, 1, 1)).unwrap();
    let notification = outbox(&owner, 1);
    store.append_notification(notification.clone()).unwrap();
    store
        .mark_notification_delivered(&notification.notification_id, 1, 1)
        .unwrap();
    let intent = effect(&owner, "event-order-effect", EffectState::Intent);
    store.record_intent(intent.clone()).unwrap();
    store
        .record_result(
            &intent.key.clone(),
            EffectRecord {
                state: EffectState::Completed,
                result_digest: Some([4; 32]),
                resolved_tick: Some(10),
                ..intent
            },
        )
        .unwrap();
    store
        .record_audit(audit(&owner, "event-order-audit", 1, 1))
        .unwrap();

    assert_eq!(
        retention_forecast(store, RetentionTarget::LifecycleEvents, policy(), 100, 0)
            .unwrap()
            .eligible_records,
        0
    );
    for target in [
        RetentionTarget::Audit,
        RetentionTarget::ResolvedEffects,
        RetentionTarget::DeliveredOutbox,
    ] {
        assert_eq!(
            archive(
                store,
                policy(),
                RetentionRequest {
                    target,
                    now_tick: 100,
                    max_records: 8,
                },
            )
            .unwrap()
            .pruned,
            1
        );
    }
    assert_eq!(
        archive(
            store,
            policy(),
            RetentionRequest {
                target: RetentionTarget::LifecycleEvents,
                now_tick: 100,
                max_records: 8,
            },
        )
        .unwrap()
        .pruned,
        1
    );
}

#[test]
fn memory_events_wait_for_all_recovery_dependencies() {
    run_event_dependency_ordering_contract(&MemoryStore::new(StoreLimits::default()));
}

#[test]
fn sqlite_events_wait_for_all_recovery_dependencies() {
    run_event_dependency_ordering_contract(
        &SqliteStateStore::open("sqlite::memory:", 1024 * 1024, 128).unwrap(),
    );
}

#[test]
fn memory_retention_contract() {
    run_contract(&MemoryStore::new(StoreLimits::default()));
}

#[test]
fn memory_terminal_lifecycle_contract() {
    run_terminal_lifecycle_contract(&MemoryStore::new(StoreLimits::default()));
}

#[test]
fn sqlite_terminal_lifecycle_contract() {
    run_terminal_lifecycle_contract(
        &SqliteStateStore::open("sqlite::memory:", 1024 * 1024, 64).unwrap(),
    );
}

fn run_terminal_execution_capacity_recovery(store: &dyn PlatformStore) {
    let first = ids("execution-capacity-first");
    let second = ids("execution-capacity-second");
    let next = ids("execution-capacity-next");
    finish_execution(store, &first);
    finish_execution(store, &second);
    assert_eq!(
        store.create_execution(execution(&next)),
        Err(StoreError::CapacityExceeded)
    );
    assert_eq!(
        archive(
            store,
            policy(),
            RetentionRequest {
                target: RetentionTarget::TerminalExecutions,
                now_tick: 100,
                max_records: 8,
            },
        )
        .unwrap()
        .pruned,
        2
    );
    store.create_execution(execution(&next)).unwrap();
}

#[test]
fn memory_terminal_execution_retention_recovers_capacity() {
    run_terminal_execution_capacity_recovery(&MemoryStore::new(StoreLimits {
        max_executions: 2,
        ..StoreLimits::default()
    }));
}

#[test]
fn sqlite_terminal_execution_retention_recovers_global_capacity() {
    run_terminal_execution_capacity_recovery(
        &SqliteStateStore::open("sqlite::memory:", 1024 * 1024, 2).unwrap(),
    );
}

fn run_terminal_work_capacity_recovery(store: &dyn PlatformStore) {
    let owner = ids("work-capacity-owner");
    finish_execution(store, &owner);
    complete_work(store, &owner, "a-capacity-work");
    complete_work(store, &owner, "b-capacity-work");
    assert_eq!(
        store.enqueue(work(&owner, "c-capacity-work")),
        Err(StoreError::CapacityExceeded)
    );
    assert_eq!(
        archive(
            store,
            policy(),
            RetentionRequest {
                target: RetentionTarget::TerminalWork,
                now_tick: 100,
                max_records: 8,
            },
        )
        .unwrap()
        .pruned,
        2
    );
    store.enqueue(work(&owner, "c-capacity-work")).unwrap();
}

#[test]
fn memory_terminal_work_retention_recovers_capacity() {
    run_terminal_work_capacity_recovery(&MemoryStore::new(StoreLimits {
        max_work_items: 2,
        ..StoreLimits::default()
    }));
}

#[test]
fn sqlite_terminal_work_retention_recovers_global_capacity() {
    run_terminal_work_capacity_recovery(
        &SqliteStateStore::open("sqlite::memory:", 1024 * 1024, 3).unwrap(),
    );
}

fn run_audit_capacity_recovery(store: &dyn PlatformStore) {
    let terminal = ids("audit-capacity-terminal");
    let nonterminal = ids("audit-capacity-nonterminal");
    finish_execution(store, &terminal);
    store.create_execution(execution(&nonterminal)).unwrap();
    store
        .record_audit(audit(&terminal, "audit-capacity-a", 1, 1))
        .unwrap();
    store
        .record_audit(audit(&terminal, "audit-capacity-b", 2, 2))
        .unwrap();
    assert_eq!(
        store.record_audit(audit(&nonterminal, "audit-capacity-c", 1, 1)),
        Err(StoreError::CapacityExceeded)
    );
    assert_eq!(
        archive(
            store,
            policy(),
            RetentionRequest {
                target: RetentionTarget::Audit,
                now_tick: 100,
                max_records: 8,
            },
        )
        .unwrap()
        .pruned,
        2
    );
    store
        .record_audit(audit(&nonterminal, "audit-capacity-c", 1, 1))
        .unwrap();
    assert_eq!(
        archive(
            store,
            policy(),
            RetentionRequest {
                target: RetentionTarget::Audit,
                now_tick: 100,
                max_records: 8,
            },
        )
        .unwrap()
        .pruned,
        0
    );
}

#[test]
fn memory_audit_retention_recovers_capacity_and_protects_nonterminal_owner() {
    run_audit_capacity_recovery(&MemoryStore::new(StoreLimits {
        max_audits: 2,
        ..StoreLimits::default()
    }));
}

#[test]
fn sqlite_audit_retention_recovers_global_capacity_and_protects_nonterminal_owner() {
    run_audit_capacity_recovery(
        &SqliteStateStore::open("sqlite::memory:", 1024 * 1024, 4).unwrap(),
    );
}

#[test]
fn forecasting_reports_full_shared_provider_headroom() {
    let store = MemoryStore::new(StoreLimits {
        max_provider_state: 2,
        ..StoreLimits::default()
    });
    store
        .put_provider_state(
            replay(
                "mq-v1-replay",
                "mainframe-env.mq-object-row@1",
                "one",
                95,
                &ids("forecast-owner").execution,
            ),
            None,
        )
        .unwrap();
    store
        .put_provider_state(
            replay(
                "mq-v1-replay",
                "mainframe-env.mq-object-row@1",
                "two",
                95,
                &ids("forecast-owner").execution,
            ),
            None,
        )
        .unwrap();
    let forecast = store
        .provider_validated_retention_forecast(RetentionTarget::MqReplay, policy(), 100, 1, 2, 0, 2)
        .unwrap();
    assert_eq!(forecast.headroom, 0);
    assert_eq!(forecast.ticks_to_capacity, Some(0));
    assert_eq!(forecast.saturation, SaturationLevel::Full);
}

#[test]
fn forecasting_includes_dedicated_archive_headroom() {
    let store = MemoryStore::new(StoreLimits {
        max_retention_archive_rows: 1,
        ..StoreLimits::default()
    });
    let expired = ids("archive-forecast-full");
    finish_execution(&store, &expired);
    store.append_event(event(&expired, 1, 1)).unwrap();
    archive(
        &store,
        policy(),
        RetentionRequest {
            target: RetentionTarget::LifecycleEvents,
            now_tick: 100,
            max_records: 1,
        },
    )
    .unwrap();
    let forecast =
        retention_forecast(&store, RetentionTarget::LifecycleEvents, policy(), 100, 1).unwrap();
    assert_eq!(forecast.headroom, store_limits_default_event_headroom());
    assert_eq!(forecast.archive_headroom, 0);
    assert_eq!(forecast.saturation, SaturationLevel::Full);
    assert_eq!(forecast.ticks_to_capacity, Some(0));
}

fn store_limits_default_event_headroom() -> usize {
    StoreLimits::default().max_events
}

fn run_event_saturation_recovery(store: &dyn PlatformStore) {
    let expired = ids("saturation-expired");
    let next = ids("saturation-next");
    finish_execution(store, &expired);
    store.append_event(event(&expired, 1, 1)).unwrap();
    store.append_event(event(&expired, 2, 2)).unwrap();
    store.create_execution(execution(&next)).unwrap();
    assert_eq!(
        store.append_event(event(&next, 1, 3)),
        Err(StoreError::CapacityExceeded)
    );
    let receipt = archive(
        store,
        policy(),
        RetentionRequest {
            target: RetentionTarget::LifecycleEvents,
            now_tick: 100,
            max_records: 8,
        },
    )
    .unwrap();
    assert_eq!((receipt.archived, receipt.pruned), (2, 2));
    store.append_event(event(&next, 1, 3)).unwrap();
}

#[test]
fn memory_retention_recovers_saturated_event_capacity() {
    run_event_saturation_recovery(&MemoryStore::new(StoreLimits {
        max_events: 2,
        max_events_per_execution: 2,
        ..StoreLimits::default()
    }));
}

#[test]
fn sqlite_retention_recovers_saturated_global_capacity() {
    run_event_saturation_recovery(
        &SqliteStateStore::open("sqlite::memory:", 1024 * 1024, 4).unwrap(),
    );
}

fn run_outbox_saturation_recovery(store: &dyn PlatformStore) {
    let expired = ids("outbox-saturation-expired");
    let next = ids("outbox-saturation-next");
    finish_execution(store, &expired);
    for sequence in 1..=2 {
        let row = outbox(&expired, sequence);
        store.append_notification(row.clone()).unwrap();
        store
            .mark_notification_delivered(&row.notification_id, 1, 10 + sequence)
            .unwrap();
    }
    store.create_execution(execution(&next)).unwrap();
    assert_eq!(
        store.append_notification(outbox(&next, 1)),
        Err(StoreError::CapacityExceeded)
    );
    let receipt = archive(
        store,
        policy(),
        RetentionRequest {
            target: RetentionTarget::DeliveredOutbox,
            now_tick: 100,
            max_records: 8,
        },
    )
    .unwrap();
    assert_eq!((receipt.archived, receipt.pruned), (2, 2));
    store.append_notification(outbox(&next, 1)).unwrap();
}

#[test]
fn memory_retention_recovers_saturated_outbox_capacity() {
    run_outbox_saturation_recovery(&MemoryStore::new(StoreLimits {
        max_outbox: 2,
        ..StoreLimits::default()
    }));
}

#[test]
fn sqlite_retention_recovers_saturated_outbox_capacity() {
    run_outbox_saturation_recovery(
        &SqliteStateStore::open("sqlite::memory:", 1024 * 1024, 4).unwrap(),
    );
}

fn run_tick_zero_event_observation_contract(store: &dyn PlatformStore) {
    let expired = ids("tick-zero");
    finish_execution(store, &expired);
    store.append_event(event(&expired, 1, 0)).unwrap();
    let early = archive(
        store,
        policy(),
        RetentionRequest {
            target: RetentionTarget::LifecycleEvents,
            now_tick: 9,
            max_records: 8,
        },
    )
    .unwrap();
    assert_eq!((early.archived, early.pruned), (0, 0));
    assert_eq!(store.events(&expired.execution, 1, 8).unwrap().len(), 1);
    let legacy = store
        .retention_legacy_rows(RetentionTarget::LifecycleEvents, 8)
        .unwrap();
    assert_eq!(legacy.len(), 1);
    store
        .reconcile_retention_age(
            RetentionAgeReconciliation {
                target: RetentionTarget::LifecycleEvents,
                namespace: legacy[0].namespace.clone(),
                key: legacy[0].key.clone(),
                expected_version: legacy[0].source_version,
                owner_execution: None,
            },
            10,
        )
        .unwrap();
    let before_window = archive(
        store,
        policy(),
        RetentionRequest {
            target: RetentionTarget::LifecycleEvents,
            now_tick: 19,
            max_records: 8,
        },
    )
    .unwrap();
    assert_eq!((before_window.archived, before_window.pruned), (0, 0));
    let on_watermark = archive(
        store,
        policy(),
        RetentionRequest {
            target: RetentionTarget::LifecycleEvents,
            now_tick: 20,
            max_records: 8,
        },
    )
    .unwrap();
    assert_eq!((on_watermark.archived, on_watermark.pruned), (1, 1));
}

#[test]
fn memory_tick_zero_event_requires_observation_and_a_full_window() {
    run_tick_zero_event_observation_contract(&MemoryStore::new(StoreLimits::default()));
}

#[test]
fn sqlite_tick_zero_event_requires_observation_and_a_full_window() {
    run_tick_zero_event_observation_contract(
        &SqliteStateStore::open("sqlite::memory:", 1024 * 1024, 128).unwrap(),
    );
}

#[test]
fn legacy_replay_age_is_protected_and_corruption_fails_closed() {
    let store = MemoryStore::new(StoreLimits::default());
    let legacy = replay(
        "mq-v1-replay",
        "mainframe-env.mq-object-row@1",
        "legacy",
        1,
        &ids("legacy-owner").execution,
    );
    let mut legacy_value: serde_json::Value = serde_json::from_slice(&legacy.payload).unwrap();
    legacy_value["value"]
        .as_object_mut()
        .unwrap()
        .remove("recorded_deadline_tick");
    store
        .put_provider_state(
            ProviderStateRecord {
                payload: serde_json::to_vec(&legacy_value).unwrap(),
                ..legacy
            },
            None,
        )
        .unwrap();
    let mut corrupt = replay(
        "mq-v1-replay",
        "mainframe-env.mq-object-row@1",
        "corrupt",
        1,
        &ids("corrupt-owner").execution,
    );
    let mut corrupt_value: serde_json::Value = serde_json::from_slice(&corrupt.payload).unwrap();
    corrupt_value["unexpected"] = serde_json::json!(true);
    corrupt.payload = serde_json::to_vec(&corrupt_value).unwrap();
    store.put_provider_state(corrupt, None).unwrap();
    let mut corrupt_inner = replay(
        "mq-v1-replay",
        "mainframe-env.mq-object-row@1",
        "corrupt-inner",
        1,
        &ids("corrupt-inner-owner").execution,
    );
    let mut corrupt_inner_value: serde_json::Value =
        serde_json::from_slice(&corrupt_inner.payload).unwrap();
    corrupt_inner_value["value"]["message"] = serde_json::json!("not-provider-bytes");
    corrupt_inner.payload = serde_json::to_vec(&corrupt_inner_value).unwrap();
    store.put_provider_state(corrupt_inner, None).unwrap();

    assert_eq!(
        archive(
            &store,
            policy(),
            RetentionRequest {
                target: RetentionTarget::MqReplay,
                now_tick: 100,
                max_records: 8,
            },
        ),
        Err(StoreError::InvalidTransition)
    );
    assert!(
        store
            .get_provider_state("mq-v1-replay", "legacy")
            .unwrap()
            .is_some()
    );
    assert!(
        store
            .get_provider_state("mq-v1-replay", "corrupt")
            .unwrap()
            .is_some()
    );
    assert!(
        store
            .get_provider_state("mq-v1-replay", "corrupt-inner")
            .unwrap()
            .is_some()
    );
}

fn run_legacy_replay_reconciliation(store: &dyn PlatformStore) {
    let raw = store.retention_legacy_rows(RetentionTarget::MqReplay, 8);
    assert_eq!(raw, Err(StoreError::InvalidTransition));
    if raw.is_err() {
        return;
    }
    let owner = ids("legacy-reconciliation-owner");
    finish_execution(store, &owner);
    let mut legacy = replay(
        "mq-v1-replay",
        "mainframe-env.mq-object-row@1",
        "legacy-reconciled",
        1,
        &owner.execution,
    );
    let mut value: serde_json::Value = serde_json::from_slice(&legacy.payload).unwrap();
    let replay = value["value"].as_object_mut().unwrap();
    replay.remove("recorded_deadline_tick");
    replay.remove("owner_execution");
    legacy.payload = serde_json::to_vec(&value).unwrap();
    store.put_provider_state(legacy, None).unwrap();

    let request = RetentionAgeReconciliation {
        target: RetentionTarget::MqReplay,
        namespace: "mq-v1-replay".into(),
        key: "legacy-reconciled".into(),
        expected_version: 1,
        owner_execution: Some(owner.execution.clone()),
    };
    let receipt = store.reconcile_retention_age(request.clone(), 100).unwrap();
    assert_eq!(
        (receipt.source_version, receipt.observation_version),
        (1, 1)
    );
    assert_eq!(receipt.reconciled_tick, 100);
    let refreshed = store.reconcile_retention_age(request, 101).unwrap();
    assert_eq!(refreshed.observation_version, 1);
    assert_eq!(refreshed.reconciled_tick, 100);
    archive(
        store,
        policy(),
        RetentionRequest {
            target: RetentionTarget::MqReplay,
            now_tick: 109,
            max_records: 8,
        },
    )
    .unwrap();
    assert!(
        store
            .get_provider_state("mq-v1-replay", "legacy-reconciled")
            .unwrap()
            .is_some()
    );
    archive(
        store,
        policy(),
        RetentionRequest {
            target: RetentionTarget::MqReplay,
            now_tick: 110,
            max_records: 8,
        },
    )
    .unwrap();
    assert!(
        store
            .get_provider_state("mq-v1-replay", "legacy-reconciled")
            .unwrap()
            .is_none()
    );
}

fn legacy_mq_replay(key: &str, owner: &ExecutionId) -> ProviderStateRecord {
    let mut row = replay(
        "mq-v1-replay",
        "mainframe-env.mq-object-row@1",
        key,
        1,
        owner,
    );
    let mut value: serde_json::Value = serde_json::from_slice(&row.payload).unwrap();
    value["value"]
        .as_object_mut()
        .unwrap()
        .remove("recorded_deadline_tick");
    value["value"]
        .as_object_mut()
        .unwrap()
        .remove("owner_execution");
    row.payload = serde_json::to_vec(&value).unwrap();
    row
}

fn run_legacy_replay_owner_evidence_contract(store: &dyn PlatformStore) {
    let raw = store.retention_legacy_rows(RetentionTarget::MqReplay, 8);
    assert_eq!(raw, Err(StoreError::InvalidTransition));
    if raw.is_err() {
        return;
    }
    let true_owner = ids("legacy-true-owner");
    let wrong_owner = ids("legacy-wrong-owner");
    finish_execution(store, &true_owner);
    finish_execution(store, &wrong_owner);

    let active_key = "legacy-active-effect";
    store
        .record_intent(effect(&true_owner, active_key, EffectState::Intent))
        .unwrap();
    store
        .put_provider_state(legacy_mq_replay(active_key, &true_owner.execution), None)
        .unwrap();
    assert_eq!(
        store.reconcile_retention_age(
            RetentionAgeReconciliation {
                target: RetentionTarget::MqReplay,
                namespace: "mq-v1-replay".into(),
                key: active_key.into(),
                expected_version: 1,
                owner_execution: Some(true_owner.execution.clone()),
            },
            100,
        ),
        Err(StoreError::InvalidTransition)
    );

    let mismatch_key = "legacy-owner-mismatch";
    let intent = effect(&true_owner, mismatch_key, EffectState::Intent);
    store.record_intent(intent.clone()).unwrap();
    store
        .record_result(
            &intent.key.clone(),
            EffectRecord {
                state: EffectState::Completed,
                result_digest: Some([8; 32]),
                resolved_tick: Some(90),
                ..intent
            },
        )
        .unwrap();
    store
        .put_provider_state(legacy_mq_replay(mismatch_key, &true_owner.execution), None)
        .unwrap();
    assert_eq!(
        store.reconcile_retention_age(
            RetentionAgeReconciliation {
                target: RetentionTarget::MqReplay,
                namespace: "mq-v1-replay".into(),
                key: mismatch_key.into(),
                expected_version: 1,
                owner_execution: Some(wrong_owner.execution),
            },
            100,
        ),
        Err(StoreError::Conflict)
    );
    assert!(
        store
            .get_provider_state("mq-v1-replay", mismatch_key)
            .unwrap()
            .is_some()
    );
}

fn run_legacy_replay_protects_reconciliation_effect(store: &dyn PlatformStore) {
    let raw = store.retention_legacy_rows(RetentionTarget::MqReplay, 8);
    assert_eq!(raw, Err(StoreError::InvalidTransition));
    if raw.is_err() {
        return;
    }
    let owner = ids("legacy-effect-order-owner");
    finish_execution(store, &owner);
    let key = "legacy-effect-order";
    let intent = effect(&owner, key, EffectState::Intent);
    store.record_intent(intent.clone()).unwrap();
    store
        .record_result(
            &intent.key.clone(),
            EffectRecord {
                state: EffectState::Completed,
                result_digest: Some([6; 32]),
                resolved_tick: Some(10),
                ..intent
            },
        )
        .unwrap();
    store
        .put_provider_state(legacy_mq_replay(key, &owner.execution), None)
        .unwrap();
    assert_eq!(
        store
            .archive_and_prune_with_dependencies(
                policy(),
                RetentionRequest {
                    target: RetentionTarget::ResolvedEffects,
                    now_tick: 100,
                    max_records: 8,
                },
                &mainframe_env_store_api::CoreRetentionDependencySnapshot {
                    expected_epoch: store.provider_state_retention_epoch().unwrap(),
                    blocked_executions: Vec::new(),
                    blocked_effect_keys: Vec::new(),
                    unowned: true,
                },
            )
            .unwrap()
            .pruned,
        0
    );
    store
        .reconcile_retention_age(
            RetentionAgeReconciliation {
                target: RetentionTarget::MqReplay,
                namespace: "mq-v1-replay".into(),
                key: key.into(),
                expected_version: 1,
                owner_execution: Some(owner.execution.clone()),
            },
            100,
        )
        .unwrap();
    assert_eq!(
        archive(
            store,
            policy(),
            RetentionRequest {
                target: RetentionTarget::MqReplay,
                now_tick: 110,
                max_records: 8,
            },
        )
        .unwrap()
        .pruned,
        1
    );
    assert_eq!(
        archive(
            store,
            policy(),
            RetentionRequest {
                target: RetentionTarget::ResolvedEffects,
                now_tick: 100,
                max_records: 8,
            },
        )
        .unwrap()
        .pruned,
        1
    );
}

#[test]
fn memory_legacy_replay_reconciliation_requires_terminal_matching_effect_owner() {
    run_legacy_replay_owner_evidence_contract(&MemoryStore::new(StoreLimits::default()));
}

#[test]
fn sqlite_legacy_replay_reconciliation_requires_terminal_matching_effect_owner() {
    run_legacy_replay_owner_evidence_contract(
        &SqliteStateStore::open("sqlite::memory:", 1024 * 1024, 128).unwrap(),
    );
}

#[test]
fn memory_legacy_replay_keeps_effect_until_owner_reconciliation() {
    run_legacy_replay_protects_reconciliation_effect(&MemoryStore::new(StoreLimits::default()));
}

#[test]
fn sqlite_legacy_replay_keeps_effect_until_owner_reconciliation() {
    run_legacy_replay_protects_reconciliation_effect(
        &SqliteStateStore::open("sqlite::memory:", 1024 * 1024, 128).unwrap(),
    );
}

fn run_cics_replay_validation_and_migration(store: &dyn PlatformStore) {
    let raw = store.retention_legacy_rows(RetentionTarget::CicsReplay, 8);
    assert_eq!(raw, Err(StoreError::InvalidTransition));
    if raw.is_err() {
        return;
    }
    let owner = ids("cics-legacy-owner");
    finish_execution(store, &owner);
    let legacy = cics_replay("cics-legacy", 1, &owner.execution, true);
    store.put_provider_state(legacy, None).unwrap();
    let listed = store
        .retention_legacy_rows(RetentionTarget::CicsReplay, 8)
        .unwrap();
    assert_eq!(listed.len(), 1);
    store
        .reconcile_retention_age(
            RetentionAgeReconciliation {
                target: RetentionTarget::CicsReplay,
                namespace: "cics-effect-replay-v1".into(),
                key: "cics-legacy".into(),
                expected_version: listed[0].source_version,
                owner_execution: Some(owner.execution.clone()),
            },
            100,
        )
        .unwrap();
    assert_eq!(
        archive(
            store,
            policy(),
            RetentionRequest {
                target: RetentionTarget::CicsReplay,
                now_tick: 109,
                max_records: 8,
            },
        )
        .unwrap()
        .pruned,
        0
    );
    assert_eq!(
        archive(
            store,
            policy(),
            RetentionRequest {
                target: RetentionTarget::CicsReplay,
                now_tick: 110,
                max_records: 8,
            },
        )
        .unwrap()
        .pruned,
        1
    );

    let mut corrupt = cics_replay("cics-corrupt", 1, &owner.execution, false);
    let disposition = 8 + 4 + owner.execution.as_str().len() + 8 + 32;
    corrupt.payload[disposition] = 0xff;
    store.put_provider_state(corrupt, None).unwrap();
    assert_eq!(
        archive(
            store,
            policy(),
            RetentionRequest {
                target: RetentionTarget::CicsReplay,
                now_tick: 100,
                max_records: 8,
            },
        ),
        Err(StoreError::IncompatibleVersion)
    );
    assert!(
        store
            .get_provider_state("cics-effect-replay-v1", "cics-corrupt")
            .unwrap()
            .is_some()
    );
}

#[test]
fn memory_cics_replay_full_validation_and_legacy_migration() {
    run_cics_replay_validation_and_migration(&MemoryStore::new(StoreLimits::default()));
}

#[test]
fn sqlite_cics_replay_full_validation_and_legacy_migration() {
    run_cics_replay_validation_and_migration(
        &SqliteStateStore::open("sqlite::memory:", 1024 * 1024, 128).unwrap(),
    );
}

#[test]
fn memory_legacy_replay_reconciliation_is_cas_fenced_and_conservative() {
    run_legacy_replay_reconciliation(&MemoryStore::new(StoreLimits::default()));
}

#[test]
fn sqlite_legacy_replay_reconciliation_is_cas_fenced_and_conservative() {
    run_legacy_replay_reconciliation(
        &SqliteStateStore::open("sqlite::memory:", 1024 * 1024, 64).unwrap(),
    );
}

fn reconcile_only_listed_legacy_row(
    store: &dyn PlatformStore,
    target: RetentionTarget,
    key: &str,
    now_tick: u64,
) -> RetentionReconciliationReceipt {
    let candidates = store.retention_legacy_rows(target, 16).unwrap();
    let candidate = candidates
        .into_iter()
        .find(|candidate| candidate.key == key)
        .expect("legacy row must expose its exact CAS token");
    store
        .reconcile_retention_age(
            RetentionAgeReconciliation {
                target,
                namespace: candidate.namespace,
                key: key.into(),
                expected_version: candidate.source_version,
                owner_execution: None,
            },
            now_tick,
        )
        .unwrap()
}

fn run_zero_tick_and_legacy_age_contract(store: &dyn PlatformStore) {
    let terminal = ids("legacy-terminal-execution");
    finish_execution_at_zero(store, &terminal);
    let execution_version = store
        .get_execution(&terminal.execution)
        .unwrap()
        .unwrap()
        .version;
    let receipt = reconcile_only_listed_legacy_row(
        store,
        RetentionTarget::TerminalExecutions,
        terminal.execution.as_str(),
        100,
    );
    assert_eq!(receipt.source_version, execution_version);
    assert_eq!(receipt.observation_version, 1);
    assert_eq!(
        store
            .get_execution(&terminal.execution)
            .unwrap()
            .unwrap()
            .version,
        execution_version
    );

    let work_owner = ids("legacy-terminal-work-owner");
    finish_execution(store, &work_owner);
    let mut legacy_work = work(&work_owner, "legacy-terminal-work");
    legacy_work.available_tick = 0;
    store.enqueue(legacy_work).unwrap();
    let claimed = store
        .claim("legacy-worker", Some("retention@1"), 0, 10)
        .unwrap()
        .unwrap();
    store
        .complete(
            &claimed.work_id,
            claimed.lease_id.as_deref().unwrap(),
            claimed.lease_epoch,
            0,
        )
        .unwrap();
    assert_eq!(
        store
            .get_work("legacy-terminal-work")
            .unwrap()
            .unwrap()
            .terminal_tick,
        None
    );
    reconcile_only_listed_legacy_row(
        store,
        RetentionTarget::TerminalWork,
        "legacy-terminal-work",
        100,
    );

    let effect_owner = ids("legacy-resolved-effect-owner");
    finish_execution(store, &effect_owner);
    let intent = effect(&effect_owner, "legacy-resolved-effect", EffectState::Intent);
    store.record_intent(intent.clone()).unwrap();
    let terminal_effect = EffectRecord {
        state: EffectState::Completed,
        result_digest: Some([9; 32]),
        resolved_tick: None,
        ..intent
    };
    store
        .record_result(&terminal_effect.key.clone(), terminal_effect)
        .unwrap();
    reconcile_only_listed_legacy_row(
        store,
        RetentionTarget::ResolvedEffects,
        "legacy-resolved-effect",
        100,
    );

    let audit_owner = ids("legacy-zero-audit-owner");
    finish_execution(store, &audit_owner);
    store
        .record_audit(audit(&audit_owner, "legacy-zero-audit", 1, 0))
        .unwrap();
    let legacy_audit = store
        .retention_legacy_rows(RetentionTarget::Audit, 16)
        .unwrap()
        .into_iter()
        .find(|candidate| candidate.key.contains("legacy-zero-audit"))
        .expect("zero-tick audit must expose an exact observation token");
    store
        .reconcile_retention_age(
            RetentionAgeReconciliation {
                target: RetentionTarget::Audit,
                namespace: legacy_audit.namespace,
                key: legacy_audit.key,
                expected_version: legacy_audit.source_version,
                owner_execution: None,
            },
            100,
        )
        .unwrap();

    for target in [
        RetentionTarget::TerminalExecutions,
        RetentionTarget::TerminalWork,
        RetentionTarget::ResolvedEffects,
    ] {
        assert_eq!(
            archive(
                store,
                policy(),
                RetentionRequest {
                    target,
                    now_tick: 109,
                    max_records: 8,
                },
            )
            .unwrap()
            .pruned,
            0
        );
        assert_eq!(
            archive(
                store,
                policy(),
                RetentionRequest {
                    target,
                    now_tick: 110,
                    max_records: 8,
                },
            )
            .unwrap()
            .pruned,
            1
        );
    }
    assert_eq!(
        archive(
            store,
            policy(),
            RetentionRequest {
                target: RetentionTarget::Audit,
                now_tick: 119,
                max_records: 8,
            },
        )
        .unwrap()
        .pruned,
        0
    );
    assert_eq!(
        archive(
            store,
            policy(),
            RetentionRequest {
                target: RetentionTarget::Audit,
                now_tick: 120,
                max_records: 8,
            },
        )
        .unwrap()
        .pruned,
        1
    );
}

#[test]
fn memory_zero_tick_terminal_rows_require_explicit_age_reconciliation() {
    run_zero_tick_and_legacy_age_contract(&MemoryStore::new(StoreLimits::default()));
}

#[test]
fn sqlite_zero_tick_terminal_rows_require_explicit_age_reconciliation() {
    run_zero_tick_and_legacy_age_contract(
        &SqliteStateStore::open("sqlite::memory:", 1024 * 1024, 128).unwrap(),
    );
}

#[test]
fn full_memory_archive_bytes_roll_back_source_prune() {
    let store = MemoryStore::new(StoreLimits {
        max_retention_archive_bytes: 1,
        ..StoreLimits::default()
    });
    let expired = ids("archive-too-large");
    finish_execution(&store, &expired);
    store.append_event(event(&expired, 1, 1)).unwrap();
    assert_eq!(
        archive(
            &store,
            policy(),
            RetentionRequest {
                target: RetentionTarget::LifecycleEvents,
                now_tick: 100,
                max_records: 8,
            },
        ),
        Err(StoreError::CapacityExceeded)
    );
    assert_eq!(store.events(&expired.execution, 1, 8).unwrap().len(), 1);
}

#[test]
fn full_sqlite_archive_bytes_roll_back_source_prune() {
    let directory = std::env::temp_dir().join(format!(
        "mainframe-env-retention-rollback-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("retention.db");
    let url = format!("sqlite://{}?mode=rwc", path.display());
    let expired = ids("archive-too-large");
    {
        let store = SqliteStateStore::open(&url, 1024 * 1024, 32).unwrap();
        finish_execution(&store, &expired);
        store.append_event(event(&expired, 1, 1)).unwrap();
    }
    let store = SqliteStateStore::open_with_retention_limits(&url, 1024 * 1024, 32, 32, 1).unwrap();
    assert_eq!(
        archive(
            &store,
            policy(),
            RetentionRequest {
                target: RetentionTarget::LifecycleEvents,
                now_tick: 100,
                max_records: 8,
            },
        ),
        Err(StoreError::CapacityExceeded)
    );
    assert_eq!(store.events(&expired.execution, 1, 8).unwrap().len(), 1);
    drop(store);
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_dir(directory);
}

fn run_archive_row_saturation_rollback(store: &dyn PlatformStore) {
    let first = ids("archive-row-capacity-first");
    finish_execution(store, &first);
    for sequence in 1..=3 {
        store.append_event(event(&first, sequence, 1)).unwrap();
    }
    assert_eq!(
        archive(
            store,
            policy(),
            RetentionRequest {
                target: RetentionTarget::LifecycleEvents,
                now_tick: 100,
                max_records: 3,
            },
        )
        .unwrap()
        .pruned,
        3
    );

    let second = ids("archive-row-capacity-second");
    finish_execution(store, &second);
    store.append_event(event(&second, 1, 1)).unwrap();
    assert_eq!(
        archive(
            store,
            policy(),
            RetentionRequest {
                target: RetentionTarget::LifecycleEvents,
                now_tick: 100,
                max_records: 1,
            },
        )
        .unwrap()
        .pruned,
        1
    );

    let protected = ids("archive-row-capacity-protected");
    finish_execution(store, &protected);
    store.append_event(event(&protected, 1, 1)).unwrap();
    assert_eq!(
        archive(
            store,
            policy(),
            RetentionRequest {
                target: RetentionTarget::LifecycleEvents,
                now_tick: 100,
                max_records: 1,
            },
        ),
        Err(StoreError::CapacityExceeded)
    );
    assert_eq!(store.events(&protected.execution, 1, 8).unwrap().len(), 1);
}

#[test]
fn memory_archive_row_saturation_rolls_back_source_prune() {
    run_archive_row_saturation_rollback(&MemoryStore::new(StoreLimits {
        max_retention_archive_rows: 4,
        ..StoreLimits::default()
    }));
}

#[test]
fn sqlite_archive_row_saturation_rolls_back_source_prune() {
    run_archive_row_saturation_rollback(
        &SqliteStateStore::open("sqlite::memory:", 1024 * 1024, 4).unwrap(),
    );
}

fn run_concurrent_archive(left: Arc<dyn PlatformStore>, right: Arc<dyn PlatformStore>) {
    let expired = ids("concurrent-archive");
    finish_execution(left.as_ref(), &expired);
    left.append_event(event(&expired, 1, 1)).unwrap();
    let barrier = Arc::new(Barrier::new(2));
    let run = |store: Arc<dyn PlatformStore>, barrier: Arc<Barrier>| {
        std::thread::spawn(move || {
            barrier.wait();
            archive(
                store.as_ref(),
                policy(),
                RetentionRequest {
                    target: RetentionTarget::LifecycleEvents,
                    now_tick: 100,
                    max_records: 8,
                },
            )
        })
    };
    let first = run(left.clone(), barrier.clone());
    let second = run(right, barrier);
    let first = first.join().unwrap();
    let second = second.join().unwrap();
    let pruned = [first, second]
        .into_iter()
        .filter_map(Result::ok)
        .map(|receipt| receipt.pruned)
        .sum::<usize>();
    assert_eq!(pruned, 1);
    assert!(left.events(&expired.execution, 1, 8).unwrap().is_empty());
    assert_eq!(
        left.retention_archives(RetentionTarget::LifecycleEvents, 8)
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn memory_concurrent_archive_has_one_atomic_winner() {
    let store: Arc<dyn PlatformStore> = Arc::new(MemoryStore::new(StoreLimits::default()));
    run_concurrent_archive(store.clone(), store);
}

#[test]
fn sqlite_concurrent_archive_has_one_atomic_winner() {
    let directory = std::env::temp_dir().join(format!(
        "mainframe-env-retention-concurrent-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("retention.db");
    let url = format!("sqlite://{}?mode=rwc", path.display());
    let left: Arc<dyn PlatformStore> =
        Arc::new(SqliteStateStore::open(&url, 1024 * 1024, 32).unwrap());
    let right: Arc<dyn PlatformStore> =
        Arc::new(SqliteStateStore::open(&url, 1024 * 1024, 32).unwrap());
    run_concurrent_archive(left, right);
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_dir(directory);
}

#[test]
fn sqlite_retention_contract_survives_restart() {
    let directory = std::env::temp_dir().join(format!(
        "mainframe-env-retention-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("retention.db");
    let url = format!("sqlite://{}?mode=rwc", path.display());
    {
        let store = SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap();
        run_contract(&store);
    }
    let reopened = SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap();
    assert!(
        reopened
            .retention_archives(RetentionTarget::LifecycleEvents, 8)
            .unwrap()
            .is_empty()
    );
    assert!(
        reopened
            .get_checkpoint(&ids("checkpointed").execution)
            .unwrap()
            .is_some()
    );
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_dir(directory);
}

#[test]
fn sqlite_open_migrates_per_execution_audit_namespaces_to_global_enumeration() {
    let directory = std::env::temp_dir().join(format!(
        "mainframe-env-audit-index-migration-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("audit.db");
    let url = format!("sqlite://{}?mode=rwc", path.display());
    let owner = ids("legacy-audit-owner");
    {
        let store = SqliteStateStore::open(&url, 1024 * 1024, 32).unwrap();
        store
            .record_audit(audit(&owner, "legacy-audit", 1, 1))
            .unwrap();
        let row = store
            .list_provider_state("durable-audit-v1", 32)
            .unwrap()
            .pop()
            .unwrap();
        store
            .delete_provider_state(&row.namespace, &row.key, row.version)
            .unwrap();
        store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: format!("durable-audit:{}", owner.execution),
                    key: "direct:00000000000000000001".into(),
                    ..row
                },
                None,
            )
            .unwrap();
    }
    let reopened = SqliteStateStore::open(&url, 1024 * 1024, 32).unwrap();
    assert_eq!(
        reopened
            .audit_records(&owner.execution, 1, 8)
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        reopened
            .retention_forecast(RetentionTarget::Audit, policy(), 100, 0)
            .unwrap()
            .eligible_records,
        1
    );
    drop(reopened);
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_dir(directory);
}

#[test]
fn sqlite_audit_migration_length_frames_colon_bearing_identities() {
    let directory = std::env::temp_dir().join(format!(
        "mainframe-env-audit-collision-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("audit.db");
    let url = format!("sqlite://{}?mode=rwc", path.display());
    let left = ids("a");
    let right = ids("a:b");
    {
        let store = SqliteStateStore::open(&url, 1024 * 1024, 32).unwrap();
        for (owner, legacy_key) in [(&left, "b:c"), (&right, "c")] {
            store
                .record_audit(audit(owner, &format!("audit-{legacy_key}"), 1, 1))
                .unwrap();
            let row = store
                .list_provider_state("durable-audit-v1", 32)
                .unwrap()
                .into_iter()
                .find(|row| row.key.contains(owner.execution.as_str()))
                .unwrap();
            store
                .delete_provider_state(&row.namespace, &row.key, row.version)
                .unwrap();
            store
                .put_provider_state(
                    ProviderStateRecord {
                        namespace: format!("durable-audit:{}", owner.execution),
                        key: legacy_key.into(),
                        ..row
                    },
                    None,
                )
                .unwrap();
        }
    }
    let reopened = SqliteStateStore::open(&url, 1024 * 1024, 32).unwrap();
    assert_eq!(
        reopened.audit_records(&left.execution, 1, 8).unwrap().len(),
        1
    );
    assert_eq!(
        reopened
            .audit_records(&right.execution, 1, 8)
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        reopened
            .list_provider_state("durable-audit-v1", 32)
            .unwrap()
            .len(),
        2
    );
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_dir(directory);
}

fn run_dedicated_clock_contract(store: &dyn PlatformStore) {
    let before = store.provider_state_retention_epoch().unwrap();
    assert_eq!(store.advance_logical_clock(100).unwrap(), 100);
    for _ in 0..32 {
        assert_eq!(store.advance_logical_clock(100).unwrap(), 100);
    }
    assert_eq!(store.advance_logical_clock(99).unwrap(), 100);
    assert_eq!(store.advance_logical_clock(101).unwrap(), 101);
    assert_eq!(store.provider_state_retention_epoch().unwrap(), before);
    assert_eq!(
        store.advance_logical_clock((i64::MAX as u64) + 1),
        Err(StoreError::CapacityExceeded)
    );
}

#[test]
fn memory_clock_advances_at_full_live_quota_without_aging_per_request() {
    let store = MemoryStore::new(StoreLimits {
        max_provider_state: 1,
        ..StoreLimits::default()
    });
    store
        .put_provider_state(
            ProviderStateRecord {
                namespace: "quota".into(),
                key: "full".into(),
                version: 1,
                payload: vec![1],
            },
            None,
        )
        .unwrap();
    run_dedicated_clock_contract(&store);
    assert_eq!(store.list_provider_state("quota", 2).unwrap().len(), 1);
}

#[test]
fn sqlite_clock_is_unmetered_monotonic_and_survives_restart() {
    let directory = std::env::temp_dir().join(format!(
        "mainframe-env-clock-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("clock.db");
    let url = format!("sqlite://{}?mode=rwc", path.display());
    {
        let store = SqliteStateStore::open(&url, 1024, 1).unwrap();
        store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: "quota".into(),
                    key: "full".into(),
                    version: 1,
                    payload: vec![1],
                },
                None,
            )
            .unwrap();
        run_dedicated_clock_contract(&store);
    }
    let reopened = SqliteStateStore::open(&url, 1024, 1).unwrap();
    assert_eq!(reopened.advance_logical_clock(1).unwrap(), 101);
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_dir(directory);
}

#[test]
fn sqlite_open_migrates_legacy_clock_and_recovers_live_quota() {
    let directory = std::env::temp_dir().join(format!(
        "mainframe-env-legacy-clock-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("clock.db");
    let url = format!("sqlite://{}?mode=rwc", path.display());
    {
        let store = SqliteStateStore::open(&url, 1024, 1).unwrap();
        store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: "jes-worker-meta".into(),
                    key: "logical-clock".into(),
                    version: 1,
                    payload: 77_u64.to_be_bytes().to_vec(),
                },
                None,
            )
            .unwrap();
    }
    let reopened = SqliteStateStore::open(&url, 1024, 1).unwrap();
    assert_eq!(reopened.advance_logical_clock(1).unwrap(), 77);
    assert!(
        reopened
            .get_provider_state("jes-worker-meta", "logical-clock")
            .unwrap()
            .is_none()
    );
    reopened
        .put_provider_state(
            ProviderStateRecord {
                namespace: "quota".into(),
                key: "recovered".into(),
                version: 1,
                payload: vec![1],
            },
            None,
        )
        .unwrap();
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_dir(directory);
}

fn run_postgres_legacy_clock_full_quota_contract(url: &str) {
    const SCHEMA: &str = "retention_clock_quota_contract";
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let pool = sqlx::PgPool::connect(url).await.unwrap();
        sqlx::query("DROP SCHEMA IF EXISTS retention_clock_quota_contract CASCADE")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("CREATE SCHEMA retention_clock_quota_contract")
            .execute(&pool)
            .await
            .unwrap();
    });
    let separator = if url.contains('?') { '&' } else { '?' };
    let schema_url = format!("{url}{separator}options=-csearch_path%3D{SCHEMA}");
    {
        let legacy = PostgresStateStore::open(&schema_url, 1024, 1).unwrap();
        legacy
            .put_provider_state(
                ProviderStateRecord {
                    namespace: "jes-worker-meta".into(),
                    key: "logical-clock".into(),
                    version: 1,
                    payload: 77_u64.to_be_bytes().to_vec(),
                },
                None,
            )
            .unwrap();
    }
    {
        let migrated = PostgresStateStore::open(&schema_url, 1024, 1).unwrap();
        assert_eq!(migrated.advance_logical_clock(1).unwrap(), 77);
        assert!(
            migrated
                .get_provider_state("jes-worker-meta", "logical-clock")
                .unwrap()
                .is_none()
        );
        migrated
            .put_provider_state(
                ProviderStateRecord {
                    namespace: "quota".into(),
                    key: "recovered".into(),
                    version: 1,
                    payload: vec![1],
                },
                None,
            )
            .unwrap();
    }
    runtime.block_on(async {
        let pool = sqlx::PgPool::connect(url).await.unwrap();
        sqlx::query("DROP SCHEMA retention_clock_quota_contract CASCADE")
            .execute(&pool)
            .await
            .unwrap();
    });
}

fn run_oversized_archive_authorization_contract(store: &dyn PlatformStore) {
    let archived_ids = ids("oversized-archive-authorization");
    finish_execution(store, &archived_ids);
    for sequence in 1..=3 {
        store
            .append_event(event(&archived_ids, sequence, sequence))
            .unwrap();
    }
    let archived = archive(
        store,
        policy(),
        RetentionRequest {
            target: RetentionTarget::LifecycleEvents,
            now_tick: 100,
            max_records: 3,
        },
    )
    .unwrap();
    let archive_id = archived.archive_id.unwrap();
    let newer = ids("newer-small-archive");
    finish_execution(store, &newer);
    store.append_event(event(&newer, 1, 2)).unwrap();
    let newer_archive_id = archive(
        store,
        policy(),
        RetentionRequest {
            target: RetentionTarget::LifecycleEvents,
            now_tick: 110,
            max_records: 1,
        },
    )
    .unwrap()
    .archive_id
    .unwrap();
    assert_eq!(
        store
            .retention_archives(RetentionTarget::LifecycleEvents, 4)
            .unwrap()
            .into_iter()
            .map(|archive| archive.archive_id)
            .collect::<Vec<_>>(),
        vec![archive_id.clone(), newer_archive_id.clone()]
    );

    assert_eq!(
        store.prune_retention_archives(policy(), 200, 1),
        Err(StoreError::InvalidTransition)
    );
    assert_eq!(
        store
            .retention_archives(RetentionTarget::LifecycleEvents, 4)
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        store
            .prune_retention_archives_authorized(
                policy(),
                RetentionArchivePruneRequest {
                    now_tick: 200,
                    max_records: 1,
                    authorized_oversized_archive_id: None,
                },
            )
            .unwrap(),
        RetentionArchivePruneOutcome::AuthorizationRequired {
            archive_id: archive_id.clone(),
            source_rows: 3,
            requested_max_records: 1,
        }
    );
    assert_eq!(
        store.prune_retention_archives_authorized(
            policy(),
            RetentionArchivePruneRequest {
                now_tick: 200,
                max_records: 1,
                authorized_oversized_archive_id: Some(format!("sha256:{}", "0".repeat(64))),
            },
        ),
        Err(StoreError::Conflict)
    );
    assert_eq!(
        store
            .retention_archives(RetentionTarget::LifecycleEvents, 4)
            .unwrap()
            .len(),
        2
    );

    let outcome = store
        .prune_retention_archives_authorized(
            policy(),
            RetentionArchivePruneRequest {
                now_tick: 200,
                max_records: 1,
                authorized_oversized_archive_id: Some(archive_id.clone()),
            },
        )
        .unwrap();
    let RetentionArchivePruneOutcome::Pruned(receipt) = outcome else {
        panic!("exact authorization must remove the reviewed archive");
    };
    assert_eq!(receipt.pruned_source_rows, 3);
    assert_eq!(receipt.archive_ids, vec![archive_id]);
    assert!(receipt.oversized_authorization_used);
    assert_eq!(
        store
            .retention_archives(RetentionTarget::LifecycleEvents, 4)
            .unwrap()
            .into_iter()
            .map(|archive| archive.archive_id)
            .collect::<Vec<_>>(),
        vec![newer_archive_id]
    );
}

#[test]
fn memory_requires_exact_authorization_for_an_oversized_archive() {
    run_oversized_archive_authorization_contract(&MemoryStore::new(StoreLimits::default()));
}

#[test]
fn sqlite_requires_exact_authorization_for_an_oversized_archive() {
    let directory = std::env::temp_dir().join(format!(
        "mainframe-env-oversized-archive-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("retention.db");
    let url = format!("sqlite://{}?mode=rwc", path.display());
    let store = SqliteStateStore::open(&url, 1024 * 1024, 32).unwrap();
    run_oversized_archive_authorization_contract(&store);
    drop(store);
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_dir(directory);
}

fn run_archive_prefix_bound_contract(store: &dyn PlatformStore) {
    for (batch, archived_tick) in [(3_u64, 20_u64), (4, 30), (1, 40)] {
        let owner = ids(&format!("archive-prefix-{batch}-{archived_tick}"));
        finish_execution(store, &owner);
        for sequence in 1..=batch {
            store.append_event(event(&owner, sequence, 1)).unwrap();
        }
        assert_eq!(
            archive(
                store,
                policy(),
                RetentionRequest {
                    target: RetentionTarget::LifecycleEvents,
                    now_tick: archived_tick,
                    max_records: usize::try_from(batch).unwrap(),
                },
            )
            .unwrap()
            .pruned,
            usize::try_from(batch).unwrap()
        );
    }
    let archives = store
        .retention_archives(RetentionTarget::LifecycleEvents, 5)
        .unwrap();
    assert_eq!(archives.len(), 1);
    assert_eq!(archives[0].rows.len(), 3);
    let outcome = store
        .prune_retention_archives_authorized(
            policy(),
            RetentionArchivePruneRequest {
                now_tick: 100,
                max_records: 5,
                authorized_oversized_archive_id: None,
            },
        )
        .unwrap();
    let RetentionArchivePruneOutcome::Pruned(receipt) = outcome else {
        panic!("the oldest bounded prefix must be pruned");
    };
    assert_eq!(receipt.pruned_source_rows, 3);
    assert_eq!(
        archive(
            store,
            policy(),
            RetentionRequest {
                target: RetentionTarget::TerminalExecutions,
                now_tick: 40,
                max_records: 64,
            },
        )
        .unwrap()
        .pruned,
        3
    );
    let _ = store
        .prune_retention_archives_authorized(
            policy(),
            RetentionArchivePruneRequest {
                now_tick: 100,
                max_records: 64,
                authorized_oversized_archive_id: None,
            },
        )
        .unwrap();
}

#[test]
fn memory_archive_read_and_prune_stop_at_the_first_overflow() {
    run_archive_prefix_bound_contract(&MemoryStore::new(StoreLimits::default()));
}

#[test]
fn sqlite_archive_read_and_prune_stop_at_the_first_overflow() {
    run_archive_prefix_bound_contract(
        &SqliteStateStore::open("sqlite::memory:", 1024 * 1024, 64).unwrap(),
    );
}

fn provider_row(
    row: ProviderStateRecord,
    observation: Option<RetentionObservationProof>,
) -> ProviderRetentionRow {
    ProviderRetentionRow {
        row,
        owner_execution: None,
        owner_run_unit: None,
        retention_tick: 10,
        observation,
        dependency: ProviderRetentionDependency::None,
    }
}

fn run_provider_archive_atomic_contract(store: &dyn PlatformStore) {
    let aggregate = ProviderStateRecord {
        namespace: "racf-database-v2".into(),
        key: "authority".into(),
        version: 1,
        payload: b"before".to_vec(),
    };
    store.put_provider_state(aggregate.clone(), None).unwrap();
    let evidence = ProviderStateRecord {
        namespace: "racf-audit".into(),
        key: "evidence".into(),
        version: 1,
        payload: b"audit".to_vec(),
    };
    let replacement = ProviderStateRecord {
        version: 2,
        payload: b"after".to_vec(),
        ..aggregate.clone()
    };
    let archive = store
        .archive_provider_state_replacement(ProviderStateArchiveReplacement {
            expected_epoch: store.provider_state_retention_epoch().unwrap(),
            target: RetentionTarget::RacfEvidence,
            archived_tick: 20,
            watermark_tick: 10,
            replacement: ProviderStateWrite {
                record: replacement.clone(),
                expected_version: Some(1),
            },
            source: aggregate,
            rows: vec![provider_row(evidence, None)],
        })
        .unwrap();
    assert_eq!(archive.rows.len(), 1);
    assert_eq!(
        store
            .get_provider_state("racf-database-v2", "authority")
            .unwrap(),
        Some(replacement.clone())
    );
    store
        .delete_provider_state("racf-database-v2", "authority", 2)
        .unwrap();
    let _ = store
        .prune_retention_archives_authorized(
            policy(),
            RetentionArchivePruneRequest {
                now_tick: 100,
                max_records: 1,
                authorized_oversized_archive_id: None,
            },
        )
        .unwrap();

    let source = ProviderStateRecord {
        namespace: "jes-spool".into(),
        key: "JOB00001".into(),
        version: 1,
        payload: b"purged".to_vec(),
    };
    store.put_provider_state(source.clone(), None).unwrap();
    let expected_epoch = store.provider_state_retention_epoch().unwrap();
    let mut forged_source = source.clone();
    forged_source.payload = b"forged".to_vec();
    assert_eq!(
        store.archive_provider_state_deletion(ProviderStateArchiveDeletion {
            expected_epoch,
            target: RetentionTarget::SpoolJobs,
            archived_tick: 20,
            watermark_tick: 10,
            rows: vec![provider_row(forged_source, None)],
        }),
        Err(StoreError::Conflict)
    );
    let observation = RetentionObservation {
        target: RetentionTarget::SpoolJobs,
        namespace: source.namespace.clone(),
        key: source.key.clone(),
        source_version: source.version,
        source_digest: Sha256::digest(&source.payload).into(),
        observed_tick: 10,
        owner_execution: None,
    };
    let observation_receipt = store
        .record_provider_retention_observation(source.clone(), expected_epoch, observation.clone())
        .unwrap();
    let expected_epoch = store.provider_state_retention_epoch().unwrap();
    let candidate = provider_row(
        source.clone(),
        Some(RetentionObservationProof {
            version: observation_receipt.observation_version,
            observation,
        }),
    );
    let mut forged = candidate.clone();
    forged.row.payload = b"forged".to_vec();
    assert_eq!(
        store.archive_provider_state_deletion(ProviderStateArchiveDeletion {
            expected_epoch,
            target: RetentionTarget::SpoolJobs,
            archived_tick: 20,
            watermark_tick: 10,
            rows: vec![forged],
        }),
        Err(StoreError::IncompatibleVersion)
    );
    let mut stale_proof = candidate.clone();
    stale_proof.observation.as_mut().unwrap().version += 1;
    assert_eq!(
        store.archive_provider_state_deletion(ProviderStateArchiveDeletion {
            expected_epoch,
            target: RetentionTarget::SpoolJobs,
            archived_tick: 20,
            watermark_tick: 10,
            rows: vec![stale_proof],
        }),
        Err(StoreError::Conflict)
    );
    let archived = store
        .archive_provider_state_deletion(ProviderStateArchiveDeletion {
            expected_epoch,
            target: RetentionTarget::SpoolJobs,
            archived_tick: 20,
            watermark_tick: 10,
            rows: vec![candidate],
        })
        .unwrap();
    assert_eq!(archived.rows.len(), 1);
    assert!(
        store
            .get_provider_state("jes-spool", "JOB00001")
            .unwrap()
            .is_none()
    );
    let _ = store
        .prune_retention_archives_authorized(
            policy(),
            RetentionArchivePruneRequest {
                now_tick: 100,
                max_records: 1,
                authorized_oversized_archive_id: None,
            },
        )
        .unwrap();
}

fn run_provider_dependency_rejects_same_run_unresolved_effect(store: &dyn PlatformStore) {
    let owner = ids("provider-unresolved-owner");
    finish_execution(store, &owner);
    let core_intent = effect(&owner, "provider-terminal-effect", EffectState::Intent);
    store.record_intent(core_intent.clone()).unwrap();
    store
        .record_result(
            &core_intent.key,
            EffectRecord {
                state: EffectState::Completed,
                result_digest: Some([2; 32]),
                resolved_tick: Some(10),
                ..core_intent.clone()
            },
        )
        .unwrap();
    let unresolved = effect(&owner, "provider-blocking-intent", EffectState::Intent);
    store.record_intent(unresolved.clone()).unwrap();
    let source = ProviderStateRecord {
        namespace: "db2-v1-replay".into(),
        key: core_intent.key.as_str().into(),
        version: 1,
        payload: b"provider-validated-replay".to_vec(),
    };
    store.put_provider_state(source.clone(), None).unwrap();
    let candidate = ProviderRetentionRow {
        row: source.clone(),
        owner_execution: Some(owner.execution.clone()),
        owner_run_unit: Some(owner.run.clone()),
        retention_tick: 10,
        observation: None,
        dependency: ProviderRetentionDependency::CoreEffect {
            key: core_intent.key.clone(),
            request_digest: [1; 32],
            result_digest: [2; 32],
        },
    };
    assert_eq!(
        store.archive_provider_state_deletion(ProviderStateArchiveDeletion {
            expected_epoch: store.provider_state_retention_epoch().unwrap(),
            target: RetentionTarget::Db2Replay,
            archived_tick: 20,
            watermark_tick: 10,
            rows: vec![candidate.clone()],
        }),
        Err(StoreError::Conflict)
    );
    let unresolved_key = unresolved.key.clone();
    store
        .record_result(
            &unresolved_key,
            EffectRecord {
                state: EffectState::Completed,
                result_digest: Some([2; 32]),
                resolved_tick: Some(10),
                ..unresolved
            },
        )
        .unwrap();
    let archive = store
        .archive_provider_state_deletion(ProviderStateArchiveDeletion {
            expected_epoch: store.provider_state_retention_epoch().unwrap(),
            target: RetentionTarget::Db2Replay,
            archived_tick: 20,
            watermark_tick: 10,
            rows: vec![candidate],
        })
        .unwrap();
    assert_eq!(archive.rows.len(), 1);
}

struct AuthorityReopenFixture {
    archive: RetentionArchive,
    observation_source: ProviderStateRecord,
    observation: RetentionObservation,
    observation_version: u64,
    archive_bytes: u64,
    observation_bytes: u64,
}

fn seed_nonempty_retention_authorities(store: &dyn PlatformStore) -> AuthorityReopenFixture {
    let archived_source = ProviderStateRecord {
        namespace: "jes-spool".into(),
        key: "REOPEN01".into(),
        version: 1,
        payload: b"verified archived payload".to_vec(),
    };
    store
        .put_provider_state(archived_source.clone(), None)
        .unwrap();
    let archive = store
        .archive_provider_state_deletion(ProviderStateArchiveDeletion {
            expected_epoch: store.provider_state_retention_epoch().unwrap(),
            target: RetentionTarget::SpoolJobs,
            archived_tick: 20,
            watermark_tick: 10,
            rows: vec![ProviderRetentionRow {
                row: archived_source,
                owner_execution: None,
                owner_run_unit: None,
                retention_tick: 10,
                observation: None,
                dependency: ProviderRetentionDependency::None,
            }],
        })
        .unwrap();
    let observation_source = ProviderStateRecord {
        namespace: "console-log".into(),
        key: "0000000000000001".into(),
        version: 1,
        payload: b"MVS\0legacy retained evidence".to_vec(),
    };
    store
        .put_provider_state(observation_source.clone(), None)
        .unwrap();
    let observation = RetentionObservation {
        target: RetentionTarget::ConsoleLog,
        namespace: observation_source.namespace.clone(),
        key: observation_source.key.clone(),
        source_version: observation_source.version,
        source_digest: Sha256::digest(&observation_source.payload).into(),
        observed_tick: 15,
        owner_execution: None,
    };
    let receipt = store
        .record_provider_retention_observation(
            observation_source.clone(),
            store.provider_state_retention_epoch().unwrap(),
            observation.clone(),
        )
        .unwrap();
    let usage = store
        .provider_retention_authority_usage(RetentionTarget::SpoolJobs)
        .unwrap();
    let observation_usage = store
        .provider_retention_authority_usage(RetentionTarget::ConsoleLog)
        .unwrap();
    AuthorityReopenFixture {
        archive,
        observation_source,
        observation,
        observation_version: receipt.observation_version,
        archive_bytes: usage.archive_bytes,
        observation_bytes: observation_usage.observation_bytes,
    }
}

fn verify_nonempty_retention_authorities(
    store: &dyn PlatformStore,
    fixture: &AuthorityReopenFixture,
) {
    assert_eq!(
        store
            .retention_archives(RetentionTarget::SpoolJobs, 1)
            .unwrap(),
        vec![fixture.archive.clone()]
    );
    assert_eq!(
        store
            .provider_retention_observation_page(RetentionTarget::ConsoleLog, None, 8)
            .unwrap(),
        vec![(fixture.observation_version, fixture.observation.clone())]
    );
    let archive_usage = store
        .provider_retention_authority_usage(RetentionTarget::SpoolJobs)
        .unwrap();
    let observation_usage = store
        .provider_retention_authority_usage(RetentionTarget::ConsoleLog)
        .unwrap();
    assert_eq!(
        (archive_usage.archive_rows, archive_usage.archive_bytes),
        (1, fixture.archive_bytes)
    );
    assert_eq!(
        (
            observation_usage.observation_rows,
            observation_usage.observation_bytes,
        ),
        (1, fixture.observation_bytes)
    );
}

fn clear_nonempty_retention_authorities(
    store: &dyn PlatformStore,
    fixture: &AuthorityReopenFixture,
) {
    assert!(matches!(
        store
            .prune_retention_archives_authorized(
                policy(),
                RetentionArchivePruneRequest {
                    now_tick: 100,
                    max_records: 1,
                    authorized_oversized_archive_id: None,
                },
            )
            .unwrap(),
        RetentionArchivePruneOutcome::Pruned(_)
    ));
    store
        .delete_provider_retention_observation(ProviderRetentionObservationDeletion {
            expected_epoch: store.provider_state_retention_epoch().unwrap(),
            source: ProviderRetentionObservationSource::Present(fixture.observation_source.clone()),
            observation: fixture.observation.clone(),
            expected_observation_version: fixture.observation_version,
        })
        .unwrap();
    store
        .delete_provider_state(
            &fixture.observation_source.namespace,
            &fixture.observation_source.key,
            fixture.observation_source.version,
        )
        .unwrap();
}

#[test]
fn memory_provider_archives_are_atomic_at_full_live_capacity() {
    run_provider_archive_atomic_contract(&MemoryStore::new(StoreLimits {
        max_provider_state: 1,
        max_retention_archive_rows: 1,
        ..Default::default()
    }));
}

#[test]
fn sqlite_provider_archives_are_atomic_at_full_live_capacity() {
    run_provider_archive_atomic_contract(
        &SqliteStateStore::open_with_retention_limits("sqlite::memory:", 1024, 1, 1, 4096).unwrap(),
    );
}

#[test]
fn memory_provider_dependency_rejects_same_run_unresolved_effect() {
    run_provider_dependency_rejects_same_run_unresolved_effect(&MemoryStore::new(
        StoreLimits::default(),
    ));
}

#[test]
fn sqlite_provider_dependency_rejects_same_run_unresolved_effect() {
    run_provider_dependency_rejects_same_run_unresolved_effect(
        &SqliteStateStore::open("sqlite::memory:", 1024 * 1024, 64).unwrap(),
    );
}

#[test]
fn sqlite_reopens_nonempty_retention_authorities_before_and_after_prune() {
    let directory = std::env::temp_dir().join(format!(
        "mainframe-env-retention-authority-reopen-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("retention.db");
    let url = format!("sqlite://{}?mode=rwc", path.display());
    let fixture = {
        let store = SqliteStateStore::open(&url, 1024 * 1024, 8).unwrap();
        seed_nonempty_retention_authorities(&store)
    };
    {
        let reopened = SqliteStateStore::open(&url, 1024 * 1024, 8).unwrap();
        verify_nonempty_retention_authorities(&reopened, &fixture);
        clear_nonempty_retention_authorities(&reopened, &fixture);
    }
    let reopened = SqliteStateStore::open(&url, 1024 * 1024, 8).unwrap();
    assert!(
        reopened
            .retention_archives(RetentionTarget::SpoolJobs, 1)
            .unwrap()
            .is_empty()
    );
    assert!(
        reopened
            .provider_retention_observation_page(RetentionTarget::ConsoleLog, None, 8)
            .unwrap()
            .is_empty()
    );
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_dir(directory);
}

fn run_capacity_health_is_non_mutating(store: &dyn PlatformStore) {
    let before_epoch = store.provider_state_retention_epoch().unwrap();
    let first = store.retention_capacity_health(policy()).unwrap();
    assert_eq!(
        store.provider_state_retention_epoch().unwrap(),
        before_epoch
    );
    let second = store.retention_capacity_health(policy()).unwrap();
    assert_eq!(second, first);
    assert_eq!(
        store.provider_state_retention_epoch().unwrap(),
        before_epoch
    );
}

#[test]
fn memory_capacity_health_does_not_mutate_provider_authority() {
    run_capacity_health_is_non_mutating(&MemoryStore::new(StoreLimits::default()));
}

#[test]
fn sqlite_capacity_health_rolls_back_its_writable_probe() {
    run_capacity_health_is_non_mutating(
        &SqliteStateStore::open("sqlite::memory:", 1024 * 1024, 64).unwrap(),
    );
}

#[test]
#[ignore = "requires isolated MAINFRAME_ENV_POSTGRES_TEST_URL pointing at PostgreSQL 18"]
fn postgres_retention_contract() {
    let url = std::env::var("MAINFRAME_ENV_POSTGRES_TEST_URL")
        .expect("explicit PostgreSQL test URL required");
    run_postgres_legacy_clock_full_quota_contract(&url);
    let authority_fixture = {
        let store = PostgresStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap();
        seed_nonempty_retention_authorities(&store)
    };
    {
        let reopened = PostgresStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap();
        verify_nonempty_retention_authorities(&reopened, &authority_fixture);
        clear_nonempty_retention_authorities(&reopened, &authority_fixture);
    }
    let store = PostgresStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap();
    run_capacity_health_is_non_mutating(&store);
    assert!(
        store
            .retention_archives(RetentionTarget::SpoolJobs, 1)
            .unwrap()
            .is_empty()
    );
    assert!(
        store
            .provider_retention_observation_page(RetentionTarget::ConsoleLog, None, 8)
            .unwrap()
            .is_empty()
    );
    run_dedicated_clock_contract(&store);
    run_archive_prefix_bound_contract(&store);
    run_provider_archive_atomic_contract(&store);
    run_contract(&store);
    drop(store);
    let reopened = PostgresStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap();
    assert_eq!(reopened.advance_logical_clock(100).unwrap(), 101);
    assert!(
        reopened
            .retention_archives(RetentionTarget::LifecycleEvents, 8)
            .unwrap()
            .is_empty()
    );
    assert!(
        reopened
            .get_checkpoint(&ids("checkpointed").execution)
            .unwrap()
            .is_some()
    );
    run_terminal_lifecycle_contract(&reopened);
    run_legacy_replay_reconciliation(&reopened);
    let left: Arc<dyn PlatformStore> = Arc::new(reopened);
    let right: Arc<dyn PlatformStore> =
        Arc::new(PostgresStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap());
    run_concurrent_archive(left, right.clone());
    run_provider_dependency_rejects_same_run_unresolved_effect(right.as_ref());
}
