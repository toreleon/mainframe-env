use mainframe_env_execution_api::{
    ArtifactRef, AuditDecision, BoundedPayload, CapabilityId, Completion, ExecutionId,
    IdempotencyKey, Invocation, InvocationLimits, Machine, MachineDrive, MachineResume, Principal,
    PrincipalId, Quantum, RequestId, ResourceLimits, RunUnitId, Selector, ServiceClass, TraceId,
};
use mainframe_env_host_api::{
    CapabilityDescriptor, EffectRequest, EffectResult, HostLimits, HostProvider, HostRequest,
    HostResult, Mutation, RegistrySnapshot, ScopedHostService, StateRequest,
    canonical_result_digest,
};
use mainframe_env_interpreter::{
    CoordinatorLimits, EffectRecoveryLimits, EffectRecoveryResolution, ExecutionControl,
    ExecutionCoordinator, StaleEffectRecoveryWorker,
};
use mainframe_env_store::{MemoryStore, PostgresStateStore, SqliteStateStore, StoreLimits};
use mainframe_env_store_api::{
    EffectRecord, EffectState, PlatformStore, ProviderStateRecord, StoreError,
};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

static NEXT_RECOVERY_TEST: AtomicU64 = AtomicU64::new(1);

struct OneMutation {
    request: Option<EffectRequest>,
}

impl Machine for OneMutation {
    type Effect = EffectRequest;
    type EffectResult = EffectResult;

    fn drive(
        &mut self,
        resume: MachineResume<Self::EffectResult>,
        _: Quantum,
    ) -> MachineDrive<Self::Effect> {
        match resume {
            MachineResume::Start => {
                MachineDrive::HostCall(self.request.take().expect("one host mutation"))
            }
            MachineResume::HostResult(result) => {
                assert!(result.outcome.is_ok());
                MachineDrive::Completed(Completion {
                    return_code: 0,
                    output: BoundedPayload::new(
                        "recovery-test@1",
                        Vec::new(),
                        InvocationLimits::default(),
                    )
                    .unwrap(),
                })
            }
            other => panic!("unexpected recovery-test resume: {other:?}"),
        }
    }
}

struct DurableSuccessProvider {
    descriptor: CapabilityDescriptor,
    store: Arc<dyn PlatformStore>,
    business_namespace: String,
    mutations: Arc<AtomicUsize>,
}

impl HostProvider for DurableSuccessProvider {
    fn descriptor(&self) -> &CapabilityDescriptor {
        &self.descriptor
    }

    fn invoke(&self, _: &Invocation, request: EffectRequest) -> EffectResult {
        let key = request
            .idempotency_key
            .as_ref()
            .expect("mutating effect identity")
            .as_str()
            .to_string();
        let existing = self
            .store
            .get_provider_state(&self.business_namespace, &key)
            .unwrap();
        match existing {
            None => {
                self.store
                    .put_provider_state(
                        ProviderStateRecord {
                            namespace: self.business_namespace.clone(),
                            key,
                            version: 1,
                            payload: b"committed-state-v1".to_vec(),
                        },
                        None,
                    )
                    .unwrap();
                self.mutations.fetch_add(1, Ordering::SeqCst);
            }
            Some(existing) => assert_eq!(existing.payload, b"committed-state-v1"),
        }
        EffectResult {
            sequence: request.sequence,
            outcome: known_success(),
        }
    }
}

struct Orphan {
    key: IdempotencyKey,
    intent: EffectRecord,
    business_namespace: String,
    mutations: Arc<AtomicUsize>,
}

fn known_success() -> Result<HostResult, mainframe_env_host_api::HostProblem> {
    Ok(HostResult::State {
        value: Some(vec![7]),
        version: 1,
    })
}

fn unique(label: &str) -> String {
    format!(
        "r01-{label}-{}-{}",
        std::process::id(),
        NEXT_RECOVERY_TEST.fetch_add(1, Ordering::Relaxed)
    )
}

fn leave_known_success_orphan(store: Arc<dyn PlatformStore>, label: &str) -> Orphan {
    let limits = InvocationLimits::default();
    let identity = unique(label);
    let key = IdempotencyKey::new(format!("{identity}-effect"), limits).unwrap();
    let capability = CapabilityId::new("host.state.write", limits).unwrap();
    let invocation = Invocation::new(
        RequestId::new(format!("{identity}-request"), limits).unwrap(),
        ExecutionId::new(format!("{identity}-execution"), limits).unwrap(),
        RunUnitId::new(format!("{identity}-run"), limits).unwrap(),
        None,
        Selector::new("recovery:test", limits).unwrap(),
        ArtifactRef::new("recovery-artifact", limits).unwrap(),
        Principal::new(
            PrincipalId::new("RECOVERY-TEST", limits).unwrap(),
            BTreeSet::from([capability.clone()]),
            limits,
        )
        .unwrap(),
        ServiceClass::System,
        0,
        100,
        TraceId::new(format!("{identity}-trace"), limits).unwrap(),
        IdempotencyKey::new(format!("{identity}-invocation"), limits).unwrap(),
        1,
        ResourceLimits::default(),
        BTreeMap::new(),
        limits,
    )
    .unwrap();
    let business_namespace = format!("r01-business:{identity}");
    let mutations = Arc::new(AtomicUsize::new(0));
    let provider: Arc<dyn HostProvider> = Arc::new(DurableSuccessProvider {
        descriptor: CapabilityDescriptor {
            capability,
            provider_id: "recovery-test-provider".into(),
            generation: "recovery-test@1".into(),
            request_schema: "state-request@1".into(),
            result_schema: "state-result@1".into(),
            max_request_bytes: 4096,
            max_result_bytes: 4096,
            ready: true,
        },
        store: store.clone(),
        business_namespace: business_namespace.clone(),
        mutations: mutations.clone(),
    });
    let host = Arc::new(ScopedHostService::new(
        Arc::new(RegistrySnapshot::new(1, vec![provider], limits).unwrap()),
        HostLimits::default(),
    ));
    let request = EffectRequest {
        run_unit: invocation.run_unit_id.clone(),
        sequence: 1,
        deadline_tick: 10,
        idempotency_key: Some(key.clone()),
        request: HostRequest::State(StateRequest::Put {
            key: "external-state".into(),
            value: vec![7],
            expected_version: None,
            mutation: Mutation {
                sequence: 1,
                idempotency_key: key.clone(),
                transaction: None,
            },
        }),
    };
    let result = ExecutionCoordinator::durable(host, store.clone(), CoordinatorLimits::default())
        .execute(
            &mut OneMutation {
                request: Some(request),
            },
            &invocation,
            ExecutionControl {
                now_tick: 5,
                cancellation_requested: false,
            },
        );
    assert!(
        matches!(&result, mainframe_env_execution_api::ExecutionOutcome::ProviderFailure(problem) if problem.has_unknown_outcome()),
        "post-dispatch journal failure must be unknown: {result:?}"
    );
    assert_eq!(mutations.load(Ordering::SeqCst), 1);
    assert_eq!(
        store
            .get_provider_state(&business_namespace, key.as_str())
            .unwrap()
            .unwrap()
            .payload,
        b"committed-state-v1"
    );
    let intent = store.effect(&key).unwrap().expect("orphan intent retained");
    assert_eq!(intent.state, EffectState::Intent);
    assert_eq!(intent.intent.owner, invocation.execution_id);
    assert_eq!(intent.intent.attempt, invocation.attempt);
    assert_eq!(
        intent
            .intent
            .capability
            .as_ref()
            .map(|value| value.as_str()),
        Some("host.state.write")
    );
    assert_eq!(intent.intent.created_tick, 5);
    assert_eq!(intent.intent.recovery_after_tick, 10);
    assert_eq!(intent.intent.epoch, 4);
    assert_eq!(intent.intent.recovery_lease, None);
    assert!(
        store
            .audit_records(&invocation.execution_id, 1, 8)
            .unwrap()
            .is_empty(),
        "the failed terminal transaction must not leave a detached audit"
    );
    Orphan {
        key,
        intent,
        business_namespace,
        mutations,
    }
}

fn reconcile_after_restart(store: Arc<dyn PlatformStore>, orphan: Orphan) {
    let worker = StaleEffectRecoveryWorker::new(
        store.clone(),
        "recovery-worker-a",
        EffectRecoveryLimits {
            minimum_age_ticks: 5,
            lease_ticks: 10,
            max_intents: 8,
        },
    )
    .unwrap();
    assert_eq!(
        worker.run_once(9, |_| unreachable!("intent is not stale yet")),
        Ok(Default::default())
    );
    let report = worker
        .run_once(10, |intent| {
            assert_eq!(intent.key, orphan.key);
            assert_eq!(intent.intent.epoch, orphan.intent.intent.epoch);
            let business = store
                .get_provider_state(&orphan.business_namespace, intent.key.as_str())?
                .ok_or(StoreError::NotFound)?;
            if business.payload != b"committed-state-v1" {
                return Err(StoreError::IncompatibleVersion);
            }
            Ok(EffectRecoveryResolution::Completed(
                canonical_result_digest(&known_success())
                    .map_err(|_| StoreError::IncompatibleVersion)?,
            ))
        })
        .unwrap();
    assert_eq!(report.scanned, 1);
    assert_eq!(report.claimed, 1);
    assert_eq!(report.completed, 1);
    assert_eq!(report.failed, 0);
    assert_eq!(report.pending, 0);
    assert_eq!(report.contended, 0);
    assert_eq!(orphan.mutations.load(Ordering::SeqCst), 1);
    let resolved = store.effect(&orphan.key).unwrap().unwrap();
    assert_eq!(resolved.state, EffectState::Completed);
    assert_eq!(resolved.digest_format, orphan.intent.digest_format);
    assert_eq!(resolved.request_digest, orphan.intent.request_digest);
    assert!(resolved.result_digest.is_some());
    let audits = store
        .audit_records(&resolved.execution_id, resolved.sequence, 8)
        .unwrap();
    assert_eq!(audits.len(), 1);
    assert_eq!(audits[0].decision, AuditDecision::Success);
    assert_eq!(audits[0].resource, resolved.intent.audit_resource.unwrap());
    assert_eq!(audits[0].capability, resolved.intent.capability.unwrap());
    assert_eq!(
        store
            .get_provider_state(&orphan.business_namespace, orphan.key.as_str())
            .unwrap()
            .unwrap()
            .payload,
        b"committed-state-v1",
        "provider success survives restart independently of the effect journal"
    );
    assert_eq!(worker.run_once(100, |_| unreachable!()).unwrap().scanned, 0);
    assert_eq!(orphan.mutations.load(Ordering::SeqCst), 1);
}

#[test]
fn stale_effect_recovery_memory_known_success_is_not_redispatched() {
    let store: Arc<dyn PlatformStore> = Arc::new(MemoryStore::new(StoreLimits {
        max_events: 4,
        max_events_per_execution: 4,
        ..StoreLimits::default()
    }));
    let orphan = leave_known_success_orphan(store.clone(), "memory");
    // A new worker/coordinator process boundary reuses the configured in-memory backend.
    reconcile_after_restart(store, orphan);
}

#[test]
fn stale_effect_recovery_sqlite_known_success_is_not_redispatched() {
    let root = std::env::temp_dir().join(unique("sqlite"));
    std::fs::create_dir(&root).unwrap();
    let url = format!("sqlite://{}?mode=rwc", root.join("state.db").display());
    // Eleven rows exist after provider success. Event + outbox alone would fit at thirteen;
    // the mandatory audit row is the capacity edge that forces the whole result transaction back.
    let store: Arc<dyn PlatformStore> =
        Arc::new(SqliteStateStore::open(&url, 1024 * 1024, 13).unwrap());
    let orphan = leave_known_success_orphan(store, "sqlite");
    let reopened: Arc<dyn PlatformStore> =
        Arc::new(SqliteStateStore::open(&url, 1024 * 1024, 13).unwrap());
    reconcile_after_restart(reopened, orphan);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
#[ignore = "requires isolated MAINFRAME_ENV_POSTGRES_TEST_URL"]
fn postgres_stale_effect_recovery_known_success_is_not_redispatched() {
    let url = std::env::var("MAINFRAME_ENV_POSTGRES_TEST_URL")
        .expect("explicit PostgreSQL test URL required");
    // Match the SQLite capacity edge: audit is the only terminal insert beyond the row budget.
    let store: Arc<dyn PlatformStore> =
        Arc::new(PostgresStateStore::open(&url, 1024 * 1024, 13).unwrap());
    let orphan = leave_known_success_orphan(store, "postgres");
    let reopened: Arc<dyn PlatformStore> =
        Arc::new(PostgresStateStore::open(&url, 1024 * 1024, 13).unwrap());
    reconcile_after_restart(reopened, orphan);
}
