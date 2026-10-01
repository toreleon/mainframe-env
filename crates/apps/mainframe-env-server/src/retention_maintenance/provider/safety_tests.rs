use super::*;
use mainframe_env_execution_api::{ArtifactRef, CapabilityId, PrincipalId, Selector};
use mainframe_env_store::{MemoryStore, StoreLimits};
use mainframe_env_store_api::{
    EffectIntentMetadata, ExecutionRecord, ExecutionState, ProviderStateRecord,
};

pub(super) fn policy() -> RetentionPolicy {
    RetentionPolicy {
        lifecycle_ticks: 1,
        idempotency_ticks: 1,
        audit_ticks: 1,
        archive_ticks: 1,
        low_watermark_percent: 70,
        high_watermark_percent: 85,
        max_batch: 8,
    }
}

pub(super) fn terminal_execution(
    store: &dyn PlatformStore,
    label: &str,
) -> (ExecutionId, RunUnitId) {
    terminal_execution_with_run(store, label, None)
}

fn terminal_execution_with_run(
    store: &dyn PlatformStore,
    label: &str,
    shared_run: Option<RunUnitId>,
) -> (ExecutionId, RunUnitId) {
    let limits = InvocationLimits::default();
    let execution = ExecutionId::new(format!("safety-{label}"), limits).unwrap();
    let run = shared_run
        .unwrap_or_else(|| RunUnitId::new(format!("safety-run-{label}"), limits).unwrap());
    store
        .create_execution(ExecutionRecord {
            execution_id: execution.clone(),
            run_unit_id: run.clone(),
            selector: Selector::new("program:SAFETY", limits).unwrap(),
            artifact: ArtifactRef::new("sha256:safety", limits).unwrap(),
            principal: PrincipalId::new("IBMUSER", limits).unwrap(),
            state: ExecutionState::Admitted,
            attempt: 1,
            version: 1,
            owner_lease: None,
            lease_expiry_tick: None,
            terminal_tick: None,
        })
        .unwrap();
    let queued = store
        .transition_execution(&execution, 1, ExecutionState::Queued, 1)
        .unwrap();
    let running = store
        .transition_execution(&execution, queued.version, ExecutionState::Running, 2)
        .unwrap();
    let completing = store
        .transition_execution(&execution, running.version, ExecutionState::Completing, 3)
        .unwrap();
    store
        .transition_execution(&execution, completing.version, ExecutionState::Completed, 4)
        .unwrap();
    (execution, run)
}

pub(super) fn unresolved_effect(owner: &ExecutionId, run: &RunUnitId, key: &str) -> EffectRecord {
    let limits = InvocationLimits::default();
    EffectRecord {
        execution_id: owner.clone(),
        run_unit_id: run.clone(),
        sequence: 1,
        key: IdempotencyKey::new(key, limits).unwrap(),
        digest_format: EffectDigestFormat::CanonicalHostV1,
        request_digest: [1; 32],
        intent: EffectIntentMetadata {
            owner: owner.clone(),
            attempt: 1,
            capability: Some(CapabilityId::new("host.safety.test", limits).unwrap()),
            audit_resource: None,
            audit_invocation_key: None,
            created_tick: 1,
            recovery_after_tick: 2,
            epoch: 1,
            recovery_lease: None,
        },
        state: EffectState::Intent,
        result_digest: None,
        resolved_tick: None,
    }
}

fn candidate(
    key: &str,
    tick: u64,
    owner: ExecutionId,
    run: RunUnitId,
    dependency: ProviderRetentionDependency,
) -> ProviderRetentionRow {
    ProviderRetentionRow {
        row: ProviderStateRecord {
            namespace: "db2-v1-replay".into(),
            key: key.into(),
            version: 1,
            payload: b"validated-by-provider".to_vec(),
        },
        owner_execution: Some(owner),
        owner_run_unit: Some(run),
        retention_tick: tick,
        observation: None,
        dependency,
    }
}

#[test]
fn unsafe_oldest_owner_does_not_starve_a_later_safe_candidate() {
    let store: Arc<dyn PlatformStore> = Arc::new(MemoryStore::new(StoreLimits::default()));
    let (blocked_owner, blocked_run) = terminal_execution(store.as_ref(), "blocked");
    let (safe_owner, safe_run) = terminal_execution(store.as_ref(), "safe");
    store
        .record_intent(unresolved_effect(
            &blocked_owner,
            &blocked_run,
            "blocking-intent",
        ))
        .unwrap();
    let planner = RetentionPlanner::from_existing(store, policy(), None).unwrap();
    let rows = planner
        .retain_safe_provider_candidates(vec![
            candidate(
                "older-blocked",
                1,
                blocked_owner,
                blocked_run,
                ProviderRetentionDependency::CoreEffect {
                    key: IdempotencyKey::new("older-blocked", InvocationLimits::default()).unwrap(),
                    request_digest: [2; 32],
                    result_digest: [3; 32],
                },
            ),
            candidate(
                "later-safe",
                2,
                safe_owner,
                safe_run,
                ProviderRetentionDependency::CoreEffect {
                    key: IdempotencyKey::new("later-safe", InvocationLimits::default()).unwrap(),
                    request_digest: [2; 32],
                    result_digest: [3; 32],
                },
            ),
        ])
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].row.key, "later-safe");
}

#[test]
fn provider_graph_requires_every_execution_to_be_recovery_clear() {
    let store: Arc<dyn PlatformStore> = Arc::new(MemoryStore::new(StoreLimits::default()));
    let (owner, run) = terminal_execution(store.as_ref(), "graph-owner");
    let (required, required_run) = terminal_execution(store.as_ref(), "graph-required");
    store
        .record_intent(unresolved_effect(
            &required,
            &required_run,
            "required-intent",
        ))
        .unwrap();
    let planner = RetentionPlanner::from_existing(store, policy(), None).unwrap();
    let rows = planner
        .retain_safe_provider_candidates(vec![candidate(
            "graph-row",
            1,
            owner,
            run,
            ProviderRetentionDependency::ProviderGraph {
                required_rows: Vec::new(),
                required_executions: vec![required],
            },
        )])
        .unwrap();
    assert!(rows.is_empty());
}

#[test]
fn frame_uow_planner_preserves_both_owners_and_root_recovery_fences() {
    use mainframe_env_store_api::CheckpointRecord;

    for case in [
        "missing",
        "foreign",
        "checkpoint",
        "intent",
        "unknown",
        "terminal",
    ] {
        let store: Arc<dyn PlatformStore> = Arc::new(MemoryStore::new(StoreLimits::default()));
        let (child, run) = terminal_execution(store.as_ref(), "frame-child");
        let root = if case == "missing" {
            ExecutionId::new("missing-root", InvocationLimits::default()).unwrap()
        } else {
            terminal_execution_with_run(
                store.as_ref(),
                "frame-root",
                (case != "foreign").then(|| run.clone()),
            )
            .0
        };
        if case == "checkpoint" {
            let owner = store.get_execution(&root).unwrap().unwrap();
            let payload = b"root-checkpoint".to_vec();
            store
                .put_checkpoint(CheckpointRecord {
                    execution_id: root.clone(),
                    run_unit_id: run.clone(),
                    session_id: None,
                    schema_version: 1,
                    machine_schema_version: 1,
                    artifact: owner.artifact,
                    provider_generation: "retention@1".into(),
                    required_host_interfaces: BTreeMap::new(),
                    effect_sequence: 0,
                    transaction: None,
                    principal: owner.principal,
                    security_classification: "internal".into(),
                    encryption_key_reference: None,
                    payload_size: payload.len() as u64,
                    payload_digest: payload_digest(&payload),
                    payload,
                })
                .unwrap();
        }
        if matches!(case, "intent" | "unknown") {
            let effect = unresolved_effect(&root, &run, "root-recovery-effect");
            store.record_intent(effect.clone()).unwrap();
            if case == "unknown" {
                store
                    .record_result(
                        &effect.key,
                        EffectRecord {
                            state: EffectState::UnknownOutcome,
                            result_digest: Some([2; 32]),
                            ..effect.clone()
                        },
                    )
                    .unwrap();
            }
        }
        let mut effect = unresolved_effect(&child, &run, "child-syncpoint");
        effect.intent.capability =
            Some(CapabilityId::new("host.cics.execute", InvocationLimits::default()).unwrap());
        store.record_intent(effect.clone()).unwrap();
        store
            .record_result(
                &effect.key,
                EffectRecord {
                    state: EffectState::Completed,
                    result_digest: Some([2; 32]),
                    resolved_tick: Some(10),
                    ..effect.clone()
                },
            )
            .unwrap();
        let mut payload = b"MECU3c".to_vec();
        for value in ["TX1", effect.key.as_str(), child.as_str(), run.as_str()] {
            payload.extend_from_slice(&(value.len() as u32).to_be_bytes());
            payload.extend_from_slice(value.as_bytes());
        }
        payload.extend_from_slice(&10_u64.to_be_bytes());
        payload.extend_from_slice(&10_u64.to_be_bytes());
        payload.extend_from_slice(&(root.as_str().len() as u32).to_be_bytes());
        payload.extend_from_slice(root.as_str().as_bytes());
        let row = ProviderStateRecord {
            namespace: "cics-uow".into(),
            key: effect.key.as_str().into(),
            version: 2,
            payload,
        };
        let mut pending = row.clone();
        pending.version = 1;
        pending.payload[5] = b'C';
        // Pending rows cannot carry a finalization observation.
        let tick_offset = pending.payload.len() - root.as_str().len() - 4 - 8;
        pending.payload[tick_offset..tick_offset + 8].fill(0);
        store.put_provider_state(pending, None).unwrap();
        store.put_provider_state(row.clone(), Some(1)).unwrap();
        let planner = RetentionPlanner::from_existing(store.clone(), policy(), None).unwrap();
        let snapshot = planner.core_dependencies().unwrap();
        assert!(snapshot.blocked_executions.contains(&root), "{case}");
        assert!(snapshot.blocked_executions.contains(&child), "{case}");
        assert!(snapshot.blocked_effect_keys.contains(&effect.key), "{case}");
        assert!(!snapshot.unowned, "{case}");
        let plan = planner
            .provider_plan(RetentionTarget::CicsUnitOfWork, 100, 0)
            .unwrap();
        assert_eq!(plan.rows.len(), usize::from(case == "terminal"), "{case}");
        if case == "terminal" {
            let ProviderRetentionDependency::ProviderGraph {
                required_executions,
                ..
            } = &plan.rows[0].dependency
            else {
                panic!("missing root proof");
            };
            assert_eq!(required_executions, &[root.clone()]);
        }
        // The nested provider replay shares the same additional-root safety boundary.
        let rows = planner
            .retain_safe_provider_candidates(vec![candidate(
                &format!("cics:{}:1", run.as_str()),
                10,
                child,
                run,
                ProviderRetentionDependency::CicsNested {
                    provenance: row.clone(),
                    absent: Vec::new(),
                    required_executions: vec![root],
                },
            )])
            .unwrap();
        assert_eq!(rows.len(), usize::from(case == "terminal"), "{case}");
        assert_eq!(
            store.get_provider_state("cics-uow", &row.key).unwrap(),
            Some(row)
        );
    }
}
