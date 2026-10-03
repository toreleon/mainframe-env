//! Physical contract proof only; genuine compiled/MQ/SAF proof is a separate suite.
use mainframe_env_execution_api::*;
use mainframe_env_store::{MemoryStore, SqliteStateStore, StoreLimits};
use mainframe_env_store_api::*;
use std::sync::atomic::{AtomicU64, Ordering};

#[path = "root_terminal_contract/writer_guards.rs"]
mod writer_guards;

#[path = "root_terminal_contract/attributed.rs"]
mod attributed;
#[path = "root_terminal_contract/publication_failures.rs"]
mod publication_failures;

fn event(execution: &ExecutionRecord, sequence: u64, kind: LifecycleEventKind) -> LifecycleEvent {
    LifecycleEvent {
        execution_id: execution.execution_id.clone(),
        run_unit_id: execution.run_unit_id.clone(),
        sequence,
        attempt: execution.attempt,
        tick: 10,
        kind,
    }
}
fn outbox(event: &LifecycleEvent) -> OutboxRecord {
    OutboxRecord {
        notification_id: format!("{}:{:020}", event.execution_id, event.sequence),
        execution_id: event.execution_id.clone(),
        sequence: event.sequence,
        topic: "execution.lifecycle.v1".into(),
        payload: lifecycle_notification_payload(&event.kind),
        attempt: 0,
        delivered: false,
        delivered_tick: None,
        version: 1,
    }
}
fn admission() -> RootDriverAdmission {
    let limits = InvocationLimits::default();
    let execution = ExecutionRecord {
        execution_id: ExecutionId::new("native-root", limits).unwrap(),
        run_unit_id: RunUnitId::new("native-run", limits).unwrap(),
        principal: PrincipalId::new("IBMUSER", limits).unwrap(),
        selector: Selector::new("program:ROOT", limits).unwrap(),
        artifact: ArtifactRef::new(format!("sha256:{}", "a".repeat(64)), limits).unwrap(),
        state: ExecutionState::Admitted,
        attempt: 1,
        version: 1,
        owner_lease: None,
        lease_expiry_tick: None,
        terminal_tick: None,
    };
    let event = event(&execution, 1, LifecycleEventKind::Admitted);
    RootDriverAdmission {
        execution,
        event: event.clone(),
        notification: outbox(&event),
        invocation_key: IdempotencyKey::new("original-root", limits).unwrap(),
        configuration_digest: [5; 32],
        deadline_tick: 100,
        provider_namespaces: vec!["native-root-owned-test".into(), "mq-test-terminal".into()],
        provider_rows: Vec::new(),
    }
}
fn running(store: &dyn PlatformStore) -> (RootDriverClaim, ExecutionRecord) {
    let admission = admission();
    let claim = store.admit_root_driver(admission.clone()).unwrap();
    let mut execution = admission.execution;
    for (sequence, state, kind) in [
        (2, ExecutionState::Queued, LifecycleEventKind::Queued),
        (3, ExecutionState::Running, LifecycleEventKind::Started),
    ] {
        let event = event(&execution, sequence, kind);
        execution = store
            .commit_execution_step(
                &execution.execution_id,
                execution.version,
                Some(state),
                event.clone(),
                None,
                None,
                None,
                outbox(&event),
            )
            .unwrap();
    }
    (claim, execution)
}
fn publication(
    store: &dyn PlatformStore,
    claim: &RootDriverClaim,
    execution: &ExecutionRecord,
    normal: bool,
) -> RootTerminalPublication {
    let closure = store.close_root_driver(claim, execution, 10).unwrap();
    let mut steps = Vec::new();
    for (offset, next_state, kind) in if normal {
        vec![
            (
                1,
                ExecutionState::Completing,
                LifecycleEventKind::Completing,
            ),
            (
                2,
                ExecutionState::Completed,
                LifecycleEventKind::Completed { return_code: 4 },
            ),
        ]
    } else {
        vec![(1, ExecutionState::Failed, LifecycleEventKind::Abend)]
    } {
        let event = event(execution, execution.version + offset, kind);
        steps.push(RootTerminalStep {
            event: event.clone(),
            next_state,
            notification: outbox(&event),
        });
    }
    let audits = [
        RootTerminalAuditRole::ProviderSettlement,
        RootTerminalAuditRole::CoreClosure,
    ]
    .into_iter()
    .map(|role| RootTerminalAudit {
        role,
        execution_id: execution.execution_id.clone(),
        run_unit_id: execution.run_unit_id.clone(),
        principal: execution.principal.clone(),
        attempt: execution.attempt,
        invocation_key: claim.admission().invocation_key.clone(),
        capability: CapabilityId::new("host.mq.write", InvocationLimits::default()).unwrap(),
        lifecycle_sequence: execution.version + 1,
        observed_tick: 10,
        resource: RootTerminalResourceDigest { value: [7; 32] },
        decision: AuditDecision::Success,
    })
    .collect();
    RootTerminalPublication {
        closure,
        steps,
        audits,
        observed_tick: 10,
        disposition: if normal {
            RootTerminalDisposition::Normal { return_code: 4 }
        } else {
            RootTerminalDisposition::KnownAbnormal
        },
        dependencies: vec![TerminalRowDependency::Absent {
            namespace: "mq-test-terminal".into(),
            key: "settled".into(),
        }],
        mutations: vec![ProviderStateMutation::Put(ProviderStateWrite {
            record: ProviderStateRecord {
                namespace: "mq-test-terminal".into(),
                key: "settled".into(),
                version: 1,
                payload: b"actual-delta".to_vec(),
            },
            expected_version: None,
        })],
    }
}

fn success(store: &dyn PlatformStore, normal: bool) {
    let (claim, execution) = running(store);
    let request = publication(store, &claim, &execution, normal);
    let expected = request.clone();
    let committed = store.commit_root_terminal_step(request).unwrap();
    assert_eq!(
        committed.execution.state,
        if normal {
            ExecutionState::Completed
        } else {
            ExecutionState::Failed
        }
    );
    assert_eq!(committed.execution.version, if normal { 5 } else { 4 });
    assert_eq!(committed.execution.terminal_tick, Some(10));
    assert_eq!(
        store
            .get_provider_state("mq-test-terminal", "settled")
            .unwrap()
            .unwrap()
            .payload,
        b"actual-delta"
    );
    assert_eq!(
        store.events(&execution.execution_id, 1, 32).unwrap().len(),
        if normal { 5 } else { 4 }
    );
    assert_eq!(
        store.pending_notifications(32).unwrap().len(),
        if normal { 5 } else { 4 }
    );
    let subjects = store
        .audit_subject_records(&execution.execution_id, 32)
        .unwrap();
    assert_eq!(
        subjects,
        expected
            .audits
            .iter()
            .cloned()
            .map(AuditSubjectRecord::RootTerminal)
            .collect::<Vec<_>>()
    );
    assert!(
        store.audit_records(&execution.execution_id, 1, 32).is_err(),
        "old subject readers must refuse new subject explicitly"
    );
    let before = store.provider_state_retention_epoch().unwrap();
    assert!(store.commit_root_terminal_step(expected).is_err());
    assert_eq!(store.provider_state_retention_epoch().unwrap(), before);
}
fn closing_fences(store: &dyn PlatformStore) {
    let (claim, execution) = running(store);
    let request = publication(store, &claim, &execution, true);
    let epoch = store.provider_state_retention_epoch().unwrap();
    let attempted = store.put_provider_state(
        ProviderStateRecord {
            namespace: "native-root-owned-test".into(),
            key: "phantom".into(),
            version: 1,
            payload: vec![1],
        },
        None,
    );
    assert!(attempted.is_err());
    let mut phantom = execution.clone();
    phantom.execution_id = ExecutionId::new("phantom-actor", InvocationLimits::default()).unwrap();
    phantom.state = ExecutionState::Admitted;
    phantom.version = 1;
    assert!(store.create_execution(phantom).is_err());
    let event = event(&execution, 4, LifecycleEventKind::Completing);
    assert!(
        store
            .commit_execution_step(
                &execution.execution_id,
                execution.version,
                Some(ExecutionState::Completing),
                event.clone(),
                None,
                None,
                None,
                outbox(&event)
            )
            .is_err()
    );
    assert_eq!(store.provider_state_retention_epoch().unwrap(), epoch);
    store.commit_root_terminal_step(request).unwrap();
}
fn late_cas_rolls_back(store: &dyn PlatformStore) {
    let (claim, execution) = running(store);
    store
        .put_provider_state(
            ProviderStateRecord {
                namespace: "mq-test-terminal".into(),
                key: "late-cas".into(),
                version: 1,
                payload: vec![9],
            },
            None,
        )
        .unwrap();
    let mut request = publication(store, &claim, &execution, true);
    request.mutations.push(ProviderStateMutation::Delete {
        namespace: "mq-test-terminal".into(),
        key: "late-cas".into(),
        expected_version: 2,
    });
    let before = (
        store.provider_state_retention_epoch().unwrap(),
        store.events(&execution.execution_id, 1, 32).unwrap(),
        store.pending_notifications(32).unwrap(),
    );
    assert!(store.commit_root_terminal_step(request.clone()).is_err());
    assert_eq!(
        store.get_execution(&execution.execution_id).unwrap(),
        Some(execution.clone())
    );
    assert!(
        store
            .get_provider_state("mq-test-terminal", "settled")
            .unwrap()
            .is_none()
    );
    assert!(
        store
            .audit_subject_records(&execution.execution_id, 32)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        (
            store.provider_state_retention_epoch().unwrap(),
            store.events(&execution.execution_id, 1, 32).unwrap(),
            store.pending_notifications(32).unwrap()
        ),
        before
    );
    request.mutations.pop();
    store.commit_root_terminal_step(request).unwrap();
}
fn identity_mutants(store: &dyn PlatformStore) {
    let (claim, execution) = running(store);
    let good = publication(store, &claim, &execution, true);
    let epoch = store.provider_state_retention_epoch().unwrap();
    for mutant in 0..8 {
        let mut bad = good.clone();
        match mutant {
            0 => bad.closure.provider_epoch += 1,
            1 => bad.closure.actors[0].execution.attempt += 1,
            2 => bad.closure.core_records[0].payload.push(0),
            3 => bad.audits[0].resource.value[0] ^= 1,
            4 => {
                bad.audits[0].principal =
                    PrincipalId::new("foreign", InvocationLimits::default()).unwrap()
            }
            5 => bad.steps[0].event.sequence += 1,
            6 => bad.steps[1].notification.payload.clear(),
            7 => {
                bad.mutations = vec![ProviderStateMutation::Delete {
                    namespace: "durable-effect".into(),
                    key: "fake".into(),
                    expected_version: 1,
                }]
            }
            _ => unreachable!(),
        }
        assert!(
            store.commit_root_terminal_step(bad).is_err(),
            "mutant {mutant}"
        );
        assert_eq!(store.provider_state_retention_epoch().unwrap(), epoch);
        assert!(
            store
                .get_provider_state("mq-test-terminal", "settled")
                .unwrap()
                .is_none()
        );
    }
    store.commit_root_terminal_step(good).unwrap();
}

#[test]
fn memory_native_root_normal_commit() {
    success(&MemoryStore::new(StoreLimits::default()), true);
}
#[test]
fn memory_native_root_known_abend() {
    success(&MemoryStore::new(StoreLimits::default()), false);
}
#[test]
fn memory_native_root_closing_fences() {
    closing_fences(&MemoryStore::new(StoreLimits::default()));
}
#[test]
fn memory_native_root_late_cas_rollback() {
    late_cas_rolls_back(&MemoryStore::new(StoreLimits::default()));
}
#[test]
fn memory_native_root_identity_mutants() {
    identity_mutants(&MemoryStore::new(StoreLimits::default()));
}

struct OwnedSqlite {
    path: std::path::PathBuf,
    store: Option<SqliteStateStore>,
}
impl OwnedSqlite {
    fn new() -> Self {
        Self::with_rows(1024)
    }
    fn with_rows(rows: usize) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let path = std::env::temp_dir().join(format!(
            "mq-wave26-terminal-{}-{}.sqlite",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        assert!(!path.exists());
        let store = SqliteStateStore::open(
            &format!("sqlite://{}?mode=rwc", path.display()),
            1 << 20,
            rows,
        )
        .unwrap();
        Self {
            path,
            store: Some(store),
        }
    }
    fn store(&self) -> &SqliteStateStore {
        self.store.as_ref().unwrap()
    }
}
impl Drop for OwnedSqlite {
    fn drop(&mut self) {
        drop(self.store.take());
        for suffix in ["", "-wal", "-shm"] {
            let path = std::path::PathBuf::from(format!("{}{suffix}", self.path.display()));
            if path.exists() {
                std::fs::remove_file(path).unwrap();
            }
        }
    }
}
#[test]
fn sqlite_native_root_normal_commit_and_reopen() {
    let mut fixture = OwnedSqlite::new();
    success(fixture.store(), true);
    drop(fixture.store.take());
    fixture.store = Some(
        SqliteStateStore::open(
            &format!("sqlite://{}?mode=rw", fixture.path.display()),
            1 << 20,
            1024,
        )
        .unwrap(),
    );
    assert_eq!(
        fixture
            .store()
            .get_execution(&admission().execution.execution_id)
            .unwrap()
            .unwrap()
            .state,
        ExecutionState::Completed
    );
    assert_eq!(
        fixture
            .store()
            .audit_subject_records(&admission().execution.execution_id, 32)
            .unwrap()
            .len(),
        2
    );
    assert!(fixture.store().admit_root_driver(admission()).is_err());
}
#[test]
fn sqlite_native_root_known_abend() {
    let fixture = OwnedSqlite::new();
    success(fixture.store(), false);
}
#[test]
fn sqlite_native_root_closing_fences() {
    let fixture = OwnedSqlite::new();
    closing_fences(fixture.store());
}
#[test]
fn sqlite_native_root_late_cas_rollback() {
    let fixture = OwnedSqlite::new();
    late_cas_rolls_back(fixture.store());
}
#[test]
fn sqlite_native_root_identity_mutants() {
    let fixture = OwnedSqlite::new();
    identity_mutants(fixture.store());
}

fn uncertain_retains_without_terminal_decision(store: &dyn PlatformStore, closing: bool) {
    let (claim, execution) = running(store);
    let request = closing.then(|| publication(store, &claim, &execution, true));
    let before = (
        store.events(&execution.execution_id, 1, 32).unwrap(),
        store.pending_notifications(32).unwrap(),
    );
    let fence = store.fence_root_driver(&claim, &execution, 11).unwrap();
    assert_eq!(
        store.fence_root_driver(&claim, &execution, 11).unwrap(),
        fence
    );
    assert!(store.close_root_driver(&claim, &execution, 11).is_err());
    if let Some(request) = request {
        assert!(store.commit_root_terminal_step(request).is_err());
    }
    assert!(
        store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: "mq-test-terminal".into(),
                    key: "after-unknown".into(),
                    version: 1,
                    payload: vec![1]
                },
                None
            )
            .is_err()
    );
    assert_eq!(
        store.get_execution(&execution.execution_id).unwrap(),
        Some(execution.clone())
    );
    assert_eq!(
        (
            store.events(&execution.execution_id, 1, 32).unwrap(),
            store.pending_notifications(32).unwrap()
        ),
        before
    );
    assert!(
        store
            .audit_subject_records(&execution.execution_id, 32)
            .unwrap()
            .is_empty()
    );
    assert!(
        store
            .get_provider_state("mq-test-terminal", "settled")
            .unwrap()
            .is_none()
    );
}
#[test]
fn memory_native_unknown_open_retains() {
    uncertain_retains_without_terminal_decision(&MemoryStore::new(StoreLimits::default()), false);
}
#[test]
fn memory_native_unknown_closing_retains() {
    uncertain_retains_without_terminal_decision(&MemoryStore::new(StoreLimits::default()), true);
}
#[test]
fn sqlite_native_unknown_open_retains() {
    let fixture = OwnedSqlite::new();
    uncertain_retains_without_terminal_decision(fixture.store(), false);
}
#[test]
fn sqlite_native_unknown_closing_retains() {
    let fixture = OwnedSqlite::new();
    uncertain_retains_without_terminal_decision(fixture.store(), true);
}

fn final_controls_and_budget_refuse(store: &dyn PlatformStore) {
    let (claim, execution) = running(store);
    let good = publication(store, &claim, &execution, true);
    let before = (
        store.provider_state_retention_epoch().unwrap(),
        store.events(&execution.execution_id, 1, 32).unwrap(),
        store.pending_notifications(32).unwrap(),
    );
    for mutation in 0..9 {
        let mut bad = good.clone();
        match mutation {
            0 => bad.observed_tick = 0,
            1 => bad.observed_tick = 9,
            2 => bad.observed_tick = claim.admission().deadline_tick,
            3 => bad.observed_tick = 11, // original events/audits still at 10
            4 => bad.audits[0].decision = AuditDecision::Deny,
            5 => bad.dependencies.clear(), // missing insert-only observation
            6 => bad.dependencies = vec![good.dependencies[0].clone(); MAX_ROOT_OPERATIONS + 1],
            7 => {
                if let ProviderStateMutation::Put(write) = &mut bad.mutations[0] {
                    write.record.namespace = "mq-undeclared".into();
                }
            }
            8 => {
                bad.closure.actors[0].last_event.run_unit_id =
                    RunUnitId::new("foreign", InvocationLimits::default()).unwrap()
            }
            _ => unreachable!(),
        }
        assert!(
            store.commit_root_terminal_step(bad).is_err(),
            "mutant {mutation}"
        );
        assert_eq!(
            (
                store.provider_state_retention_epoch().unwrap(),
                store.events(&execution.execution_id, 1, 32).unwrap(),
                store.pending_notifications(32).unwrap()
            ),
            before
        );
        assert_eq!(
            store.get_execution(&execution.execution_id).unwrap(),
            Some(execution.clone())
        );
        assert!(
            store
                .audit_subject_records(&execution.execution_id, 32)
                .unwrap()
                .is_empty()
        );
        assert!(
            store
                .get_provider_state("mq-test-terminal", "settled")
                .unwrap()
                .is_none()
        );
    }
    let mut later = good;
    later.observed_tick = 11;
    for step in &mut later.steps {
        step.event.tick = 11;
    }
    for audit in &mut later.audits {
        audit.observed_tick = 11;
    }
    let commit = store.commit_root_terminal_step(later).unwrap();
    assert_eq!(commit.execution.terminal_tick, Some(11));
}
#[test]
fn memory_native_final_controls_and_max_plus_one() {
    final_controls_and_budget_refuse(&MemoryStore::new(StoreLimits::default()));
}
#[test]
fn sqlite_native_final_controls_and_max_plus_one() {
    let fixture = OwnedSqlite::new();
    final_controls_and_budget_refuse(fixture.store());
}

fn old_clock_refuses_admission(store: &dyn PlatformStore) {
    store.advance_logical_clock(50).unwrap();
    let epoch = store.provider_state_retention_epoch().unwrap();
    assert!(store.admit_root_driver(admission()).is_err());
    assert_eq!(store.provider_state_retention_epoch().unwrap(), epoch);
    assert!(
        store
            .get_execution(&admission().execution.execution_id)
            .unwrap()
            .is_none()
    );
    assert!(
        store
            .get_provider_state(ROOT_DRIVER_NAMESPACE, "native-root")
            .unwrap()
            .is_none()
    );
    assert!(store.pending_notifications(32).unwrap().is_empty());
}
#[test]
fn memory_native_initial_clock_floor() {
    old_clock_refuses_admission(&MemoryStore::new(StoreLimits::default()));
}
#[test]
fn sqlite_native_initial_clock_floor() {
    let fixture = OwnedSqlite::new();
    old_clock_refuses_admission(fixture.store());
}

fn complete_scope_max_plus_one(store: &dyn PlatformStore) {
    let (claim, execution) = running(store);
    for index in 0..=MAX_ROOT_OPERATIONS {
        store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: "native-root-owned-test".into(),
                    key: format!("row-{index:06}"),
                    version: 1,
                    payload: vec![1],
                },
                None,
            )
            .unwrap();
    }
    let original = store
        .get_provider_state(ROOT_DRIVER_NAMESPACE, "native-root")
        .unwrap();
    let epoch = store.provider_state_retention_epoch().unwrap();
    assert!(store.close_root_driver(&claim, &execution, 10).is_err());
    assert_eq!(
        store
            .get_provider_state(ROOT_DRIVER_NAMESPACE, "native-root")
            .unwrap(),
        original
    );
    assert_eq!(store.provider_state_retention_epoch().unwrap(), epoch);
    assert_eq!(
        store
            .list_provider_state("native-root-owned-test", MAX_ROOT_OPERATIONS + 2)
            .unwrap()
            .len(),
        MAX_ROOT_OPERATIONS + 1
    );
}
#[test]
fn memory_native_complete_scope_max_plus_one() {
    complete_scope_max_plus_one(&MemoryStore::new(StoreLimits {
        max_provider_state: 8192,
        ..StoreLimits::default()
    }));
}
#[test]
fn sqlite_native_complete_scope_max_plus_one() {
    let fixture = OwnedSqlite::with_rows(8192);
    complete_scope_max_plus_one(fixture.store());
}

#[test]
fn memory_native_second_audit_quota_rolls_back_all_touched_state() {
    let store = MemoryStore::new(StoreLimits {
        max_audits: 1,
        ..StoreLimits::default()
    });
    let (claim, execution) = running(&store);
    let mut request = publication(&store, &claim, &execution, true);
    request.observed_tick = 20;
    for step in &mut request.steps {
        step.event.tick = 20;
    }
    for audit in &mut request.audits {
        audit.observed_tick = 20;
    }
    let before = (
        store.provider_state_retention_epoch().unwrap(),
        store.events(&execution.execution_id, 1, 32).unwrap(),
        store.pending_notifications(32).unwrap(),
    );
    assert_eq!(
        store.commit_root_terminal_step(request),
        Err(StoreError::CapacityExceeded)
    );
    assert_eq!(
        store.get_execution(&execution.execution_id).unwrap(),
        Some(execution.clone())
    );
    assert_eq!(
        (
            store.provider_state_retention_epoch().unwrap(),
            store.events(&execution.execution_id, 1, 32).unwrap(),
            store.pending_notifications(32).unwrap()
        ),
        before
    );
    assert!(
        store
            .audit_subject_records(&execution.execution_id, 1)
            .unwrap()
            .is_empty()
    );
    assert!(
        store
            .get_provider_state("mq-test-terminal", "settled")
            .unwrap()
            .is_none()
    );
    assert_eq!(
        store.advance_logical_clock(10).unwrap(),
        10,
        "failed final tick must not advance the retained floor"
    );
}

#[test]
fn sqlite_native_final_row_quota_restores_terminal_rows_clock_epoch_and_audits() {
    // This shape reaches 19 rows only after both audit subjects and the winner.
    // SQLite checks the whole quota inside its physical transaction, not a mock.
    let fixture = OwnedSqlite::with_rows(18);
    let store = fixture.store();
    let (claim, execution) = running(store);
    let mut request = publication(store, &claim, &execution, true);
    request.observed_tick = 20;
    for step in &mut request.steps {
        step.event.tick = 20;
    }
    for audit in &mut request.audits {
        audit.observed_tick = 20;
    }
    let before = (
        store.provider_state_retention_epoch().unwrap(),
        store.events(&execution.execution_id, 1, 32).unwrap(),
        store.pending_notifications(32).unwrap(),
        store
            .get_provider_state(ROOT_DRIVER_NAMESPACE, execution.execution_id.as_str())
            .unwrap(),
    );
    assert_eq!(
        store.commit_root_terminal_step(request),
        Err(StoreError::CapacityExceeded)
    );
    assert_eq!(
        store.get_execution(&execution.execution_id).unwrap(),
        Some(execution.clone())
    );
    assert_eq!(
        (
            store.provider_state_retention_epoch().unwrap(),
            store.events(&execution.execution_id, 1, 32).unwrap(),
            store.pending_notifications(32).unwrap(),
            store
                .get_provider_state(ROOT_DRIVER_NAMESPACE, execution.execution_id.as_str())
                .unwrap()
        ),
        before
    );
    assert!(
        store
            .audit_subject_records(&execution.execution_id, 32)
            .unwrap()
            .is_empty()
    );
    assert!(
        store
            .get_provider_state("mq-test-terminal", "settled")
            .unwrap()
            .is_none()
    );
    assert_eq!(store.advance_logical_clock(10).unwrap(), 10);
}

fn orphan_intent() -> EffectRecord {
    let bounds = InvocationLimits::default();
    let root = admission();
    let owner = ExecutionId::new("unadmitted-root-actor", bounds).unwrap();
    // Structural negative fixture only, never a production intent constructor.
    EffectRecord {
        execution_id: owner.clone(),
        run_unit_id: root.execution.run_unit_id,
        sequence: 1,
        key: IdempotencyKey::new("orphan-root-effect", bounds).unwrap(),
        request_digest: [1; 32],
        digest_format: EffectDigestFormat::CanonicalHostV1,
        state: EffectState::Intent,
        result_digest: None,
        resolved_tick: None,
        intent: EffectIntentMetadata {
            owner,
            attempt: 1,
            capability: Some(CapabilityId::new("host.mq.write", bounds).unwrap()),
            audit_resource: Some(AuditResourceDigest {
                format: AuditResourceDigestFormat::CanonicalHostResourceV1,
                value: [2; 32],
            }),
            audit_invocation_key: Some(root.invocation_key),
            created_tick: 10,
            recovery_after_tick: 100,
            epoch: 1,
            recovery_lease: None,
        },
    }
}

fn scoped_effect_phantoms_refuse(store: &dyn PlatformStore) {
    let (claim, execution) = running(store);
    let before = store.provider_state_retention_epoch().unwrap();
    let mut orphan = orphan_intent();
    assert!(store.record_intent(orphan.clone()).is_err());
    assert_eq!(store.provider_state_retention_epoch().unwrap(), before);
    assert!(store.effect(&orphan.key).unwrap().is_none());
    orphan.execution_id = execution.execution_id.clone();
    orphan.intent.owner = execution.execution_id.clone();
    orphan.run_unit_id = RunUnitId::new("wrong-original-run", InvocationLimits::default()).unwrap();
    assert!(store.record_intent(orphan.clone()).is_err());
    assert_eq!(store.provider_state_retention_epoch().unwrap(), before);
    let request = publication(store, &claim, &execution, true);
    let closing_epoch = store.provider_state_retention_epoch().unwrap();
    orphan.run_unit_id = execution.run_unit_id.clone();
    assert!(store.record_intent(orphan).is_err());
    assert_eq!(
        store.provider_state_retention_epoch().unwrap(),
        closing_epoch
    );
    store.commit_root_terminal_step(request).unwrap();
}

fn existing_orphan_refuses_root_admission(store: &dyn PlatformStore) {
    let orphan = orphan_intent();
    store.record_intent(orphan.clone()).unwrap(); // existing legacy low-level contract
    let epoch = store.provider_state_retention_epoch().unwrap();
    assert!(store.admit_root_driver(admission()).is_err());
    assert_eq!(store.provider_state_retention_epoch().unwrap(), epoch);
    assert!(
        store
            .get_execution(&admission().execution.execution_id)
            .unwrap()
            .is_none()
    );
    assert_eq!(store.effect(&orphan.key).unwrap(), Some(orphan));
}

#[test]
fn memory_native_scoped_effect_phantoms_refuse() {
    scoped_effect_phantoms_refuse(&MemoryStore::new(StoreLimits::default()));
}
#[test]
fn sqlite_native_scoped_effect_phantoms_refuse() {
    let owned = OwnedSqlite::new();
    scoped_effect_phantoms_refuse(owned.store());
}
#[test]
fn memory_native_existing_orphan_refuses_root_admission() {
    existing_orphan_refuses_root_admission(&MemoryStore::new(StoreLimits::default()));
}
#[test]
fn sqlite_native_existing_orphan_refuses_root_admission() {
    let owned = OwnedSqlite::new();
    existing_orphan_refuses_root_admission(owned.store());
}
