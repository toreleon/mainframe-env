use super::safety_tests::{policy, terminal_execution, unresolved_effect};
use super::*;
use crate::retention_maintenance::RetentionMaintenance;
use mainframe_env_cics::{CicsLimits, CicsReplayClock, CicsService};
use mainframe_env_execution_api::{
    BoundedPayload, CapabilityId, Invocation, Principal, RequestId, ResourceLimits, ServiceClass,
    TraceId,
};
use mainframe_env_host_api::{
    CapabilityDescriptor, CicsConditionPolicy, CicsOperation, CicsRequest, EffectRequest,
    EffectResult, HostLimits, HostProvider, HostRequest, HostResult, Mutation, RegistrySnapshot,
    ScopedHostService, SecurityDecision, SessionId, canonical_request_digest,
    canonical_result_digest,
};
use mainframe_env_store::{MemoryStore, PostgresStateStore, SqliteStateStore, StoreLimits};
use mainframe_env_store_api::{ProviderStateStore, RetentionStore};

struct Security(CapabilityDescriptor);
impl HostProvider for Security {
    fn descriptor(&self) -> &CapabilityDescriptor {
        &self.0
    }
    fn invoke(&self, _: &Invocation, effect: EffectRequest) -> EffectResult {
        assert!(matches!(effect.request, HostRequest::Security(_)));
        EffectResult {
            sequence: effect.sequence,
            outcome: Ok(HostResult::Security(SecurityDecision::Allow)),
        }
    }
}

struct Clock;
impl CicsReplayClock for Clock {
    fn now_tick(&self) -> Result<u64, HostProblem> {
        Ok(20)
    }
}

// Seed both receipts through the real provider, not a hand-authored outer codec.
fn seed(store: Arc<dyn PlatformStore>, label: &str) -> ProviderStateRecord {
    let limits = InvocationLimits::default();
    let (execution, run) = terminal_execution(store.as_ref(), label);
    let owner = store.get_execution(&execution).unwrap().unwrap();
    let security = Arc::new(Security(CapabilityDescriptor {
        capability: CapabilityId::new("host.security.authorize", limits).unwrap(),
        provider_id: "container-retention-test".into(),
        generation: "1".into(),
        request_schema: "security@1".into(),
        result_schema: "security@1".into(),
        max_request_bytes: 65536,
        max_result_bytes: 65536,
        ready: true,
    }));
    let host = Arc::new(ScopedHostService::new(
        Arc::new(RegistrySnapshot::new(1, vec![security], limits).unwrap()),
        HostLimits::default(),
    ));
    let invocation = Invocation::new(
        RequestId::new(format!("request-{label}"), limits).unwrap(),
        execution.clone(),
        run.clone(),
        None,
        owner.selector,
        owner.artifact,
        Principal::new(
            owner.principal,
            ["host.security.authorize", "host.cics.execute"]
                .into_iter()
                .map(|name| CapabilityId::new(name, limits).unwrap())
                .collect(),
            limits,
        )
        .unwrap(),
        ServiceClass::Interactive,
        0,
        100,
        TraceId::new("trace", limits).unwrap(),
        IdempotencyKey::new(format!("invocation-{label}"), limits).unwrap(),
        1,
        ResourceLimits::default(),
        BTreeMap::new(),
        limits,
    )
    .unwrap();
    let service = CicsService::open_with_replay_clock(
        host,
        store.clone(),
        CicsLimits::default(),
        Arc::new(Clock),
    )
    .unwrap();
    let session = SessionId::new(label, 64).unwrap();
    service.create_session(&session, 24, 80).unwrap();
    service
        .register_run(invocation, &session, "TEST", "MEAPPL", "MESYS")
        .unwrap();
    let key = IdempotencyKey::new(format!("container-{label}"), limits).unwrap();
    let request = CicsRequest {
        operation: CicsOperation::PutContainer,
        arguments: [
            ("CHANNEL", b"WORK".as_slice()),
            ("CONTAINER", b"ITEM".as_slice()),
            ("FROM", b"retained".as_slice()),
        ]
        .into_iter()
        .map(|(name, bytes)| {
            (
                name.into(),
                BoundedPayload::new("mainframe-env.cics.literal@1", bytes.to_vec(), limits)
                    .unwrap(),
            )
        })
        .collect(),
        condition_policy: CicsConditionPolicy::Default,
        mutation: Some(Mutation {
            sequence: 1,
            idempotency_key: key.clone(),
            transaction: Some("TEST".into()),
        }),
    };
    let mut effect = unresolved_effect(&execution, &run, key.as_str());
    effect.intent.capability = Some(CapabilityId::new("host.cics.execute", limits).unwrap());
    effect.request_digest = canonical_request_digest(&HostRequest::Cics(request.clone())).unwrap();
    store.record_intent(effect.clone()).unwrap();
    let response = service
        .invoke(
            &EffectRequest {
                run_unit: run,
                sequence: 1,
                deadline_tick: 100,
                idempotency_key: Some(key.clone()),
                request: HostRequest::Cics(request.clone()),
            },
            request,
        )
        .unwrap();
    assert_eq!(response.response, 0);
    effect.state = EffectState::Completed;
    effect.resolved_tick = Some(20);
    effect.result_digest = Some(canonical_result_digest(&Ok(HostResult::Cics(response))).unwrap());
    store.record_result(&key, effect).unwrap();
    store
        .get_provider_state("cics-container-replay-v1", key.as_str())
        .unwrap()
        .unwrap()
}

fn valid_archive(store: Arc<dyn PlatformStore>, label: &str) {
    let private = seed(store.clone(), label);
    let maintenance = RetentionMaintenance::open(store.clone(), policy()).unwrap();
    let pass = maintenance.begin_pass().unwrap();
    let first = pass
        .archive_and_prune(RetentionTarget::CicsReplay, 8)
        .unwrap();
    assert_eq!(first.pruned, 1);
    assert!(
        store
            .get_provider_state(&private.namespace, &private.key)
            .unwrap()
            .is_none()
    );
    assert!(
        store
            .get_provider_state("cics-effect-replay-v1", &private.key)
            .unwrap()
            .is_some()
    );
    let capacity = store
        .get_provider_state("cics-container-capacity-v1", "global")
        .unwrap()
        .unwrap();
    let payload: serde_json::Value = serde_json::from_slice(&capacity.payload).unwrap();
    assert_eq!(payload["replays"], 0);
    assert_eq!(payload["channels"], 1);
    assert_eq!(payload["containers"], 1);
    assert_eq!(
        pass.archive_and_prune(RetentionTarget::CicsReplay, 8)
            .unwrap()
            .pruned,
        1
    );
    assert!(
        store
            .get_provider_state("cics-effect-replay-v1", &private.key)
            .unwrap()
            .is_none()
    );
}

fn rejected_archive(store: Arc<dyn PlatformStore>, label: &str, field: &str) {
    let mut private = seed(store.clone(), label);
    let mut payload: serde_json::Value = serde_json::from_slice(&private.payload).unwrap();
    match field {
        "schema_version" => payload[field] = 2.into(),
        "owner_execution" | "owner_run_unit" | "owner_principal" => {
            payload[field] = "foreign-owner".into()
        }
        "digest" => payload[field] = "not-a-digest".into(),
        "unknown" => payload["unknown"] = true.into(),
        _ => {}
    }
    private.payload = match field {
        "malformed" => b"{".to_vec(),
        "duplicate" => format!(
            "{{\"schema_version\":1,{}",
            &String::from_utf8(private.payload.clone()).unwrap()[1..]
        )
        .into_bytes(),
        _ => serde_json::to_vec(&payload).unwrap(),
    };
    store
        .delete_provider_state(&private.namespace, &private.key, 1)
        .unwrap();
    if field == "version" {
        store.put_provider_state(private.clone(), None).unwrap();
        private.version = 2;
        store.put_provider_state(private.clone(), Some(1)).unwrap();
    } else {
        store.put_provider_state(private.clone(), None).unwrap();
    }
    let outer = store
        .get_provider_state("cics-effect-replay-v1", &private.key)
        .unwrap();
    let capacity = store
        .get_provider_state("cics-container-capacity-v1", "global")
        .unwrap();
    let archives = store
        .retention_archives(RetentionTarget::CicsReplay, 100)
        .unwrap();
    let epoch = store.provider_state_retention_epoch().unwrap();
    let maintenance = RetentionMaintenance::open(store.clone(), policy()).unwrap();
    let pass = maintenance.begin_pass().unwrap();
    assert_eq!(
        pass.archive_and_prune(RetentionTarget::CicsReplay, 8),
        Err(HostProblem::InfrastructureFailure)
    );
    assert_eq!(
        store
            .get_provider_state(&private.namespace, &private.key)
            .unwrap(),
        Some(private)
    );
    assert_eq!(
        store
            .get_provider_state(
                "cics-effect-replay-v1",
                outer.as_ref().unwrap().key.as_str()
            )
            .unwrap(),
        outer
    );
    assert_eq!(
        store
            .get_provider_state("cics-container-capacity-v1", "global")
            .unwrap(),
        capacity
    );
    assert_eq!(
        store
            .retention_archives(RetentionTarget::CicsReplay, 100)
            .unwrap(),
        archives
    );
    assert_eq!(store.provider_state_retention_epoch().unwrap(), epoch);
}

const INVALID_FIELDS: [&str; 9] = [
    "malformed",
    "schema_version",
    "owner_execution",
    "owner_run_unit",
    "owner_principal",
    "digest",
    "duplicate",
    "unknown",
    "version",
];

#[test]
fn private_container_replay_memory_archive_and_rejection() {
    valid_archive(
        Arc::new(MemoryStore::new(StoreLimits::default())),
        "memory-valid",
    );
    for (index, field) in INVALID_FIELDS.into_iter().enumerate() {
        rejected_archive(
            Arc::new(MemoryStore::new(StoreLimits::default())),
            &format!("memory-{index}"),
            field,
        );
    }
}

#[test]
fn private_container_replay_sqlite_archive_reopens_and_rejects() {
    let path = std::env::temp_dir().join(format!(
        "cics-container-retention-{}-{}.sqlite",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let url = format!("sqlite://{}?mode=rwc", path.display());
    valid_archive(
        Arc::new(SqliteStateStore::open(&url, 1024 * 1024, 4096).unwrap()),
        "sqlite-valid",
    );
    let reopened = SqliteStateStore::open(&url, 1024 * 1024, 4096).unwrap();
    assert!(
        reopened
            .get_provider_state("cics-container-replay-v1", "container-sqlite-valid")
            .unwrap()
            .is_none()
    );
    assert_eq!(
        reopened
            .retention_archives(RetentionTarget::CicsReplay, 100)
            .unwrap()
            .len(),
        2
    );
    for (index, field) in INVALID_FIELDS.into_iter().enumerate() {
        rejected_archive(
            Arc::new(SqliteStateStore::open(&url, 1024 * 1024, 4096).unwrap()),
            &format!("sqlite-{index}"),
            field,
        );
        // Remove only this fixture's corrupt rows so the next case is independent.
        let key = format!("container-sqlite-{index}");
        let row = reopened
            .get_provider_state("cics-container-replay-v1", &key)
            .unwrap()
            .unwrap();
        reopened
            .delete_provider_state(&row.namespace, &key, row.version)
            .unwrap();
        let row = reopened
            .get_provider_state("cics-effect-replay-v1", &key)
            .unwrap()
            .unwrap();
        reopened
            .delete_provider_state(&row.namespace, &key, row.version)
            .unwrap();
    }
    drop(reopened);
    std::fs::remove_file(path).unwrap();
}

#[test]
#[ignore = "requires disposable MAINFRAME_ENV_POSTGRES_TEST_URL"]
fn private_container_replay_postgres_archive_and_rejection() {
    let url = std::env::var("MAINFRAME_ENV_POSTGRES_TEST_URL").expect("disposable PostgreSQL URL");
    let store = Arc::new(PostgresStateStore::open(&url, 1024 * 1024, 4096).unwrap());
    valid_archive(store.clone(), "postgres-valid");
    for (index, field) in INVALID_FIELDS.into_iter().enumerate() {
        let label = format!("postgres-{index}");
        rejected_archive(store.clone(), &label, field);
        let key = format!("container-{label}");
        let row = store
            .get_provider_state("cics-container-replay-v1", &key)
            .unwrap()
            .unwrap();
        store
            .delete_provider_state(&row.namespace, &key, row.version)
            .unwrap();
        let row = store
            .get_provider_state("cics-effect-replay-v1", &key)
            .unwrap()
            .unwrap();
        store
            .delete_provider_state(&row.namespace, &key, row.version)
            .unwrap();
    }
}
