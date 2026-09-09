use super::*;
use mainframe_env_execution_api::{ArtifactRef, CapabilityId, PrincipalId, Selector};
use mainframe_env_store::{MemoryStore, StoreLimits};
use mainframe_env_store_api::{
    EffectIntentMetadata, ExecutionRecord, ExecutionState, ProviderStateRecord,
};

fn policy() -> RetentionPolicy {
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

fn terminal_execution(store: &dyn PlatformStore, label: &str) -> (ExecutionId, RunUnitId) {
    let limits = InvocationLimits::default();
    let execution = ExecutionId::new(format!("safety-{label}"), limits).unwrap();
    let run = RunUnitId::new(format!("safety-run-{label}"), limits).unwrap();
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

fn unresolved_effect(owner: &ExecutionId, run: &RunUnitId, key: &str) -> EffectRecord {
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
