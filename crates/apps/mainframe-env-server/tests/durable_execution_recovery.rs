use mainframe_env_execution_api::{
    ArtifactRef, AuditDecision, BoundedPayload, CapabilityId, Completion, ExecutionId,
    ExecutionOutcome, IdempotencyKey, Invocation, InvocationLimits, LifecycleEventKind, Machine,
    MachineDrive, MachineResume, Principal, PrincipalId, Quantum, RequestId, ResourceLimits,
    RunUnitId, Selector, ServiceClass, Suspension, TraceId,
};
use mainframe_env_host_api::{
    CapabilityDescriptor, EffectRequest, EffectResult, HostLimits, HostProblem, HostProvider,
    HostRequest, HostResult, Mutation, RegistrySnapshot, ScopedHostService, StateRequest,
    canonical_result_digest,
};
use mainframe_env_interpreter::{CoordinatorLimits, ExecutionControl, ExecutionCoordinator};
use mainframe_env_store::{MemoryStore, PostgresStateStore, StoreLimits};
use mainframe_env_store_api::{
    EffectDigestFormat, EffectState, ExecutionState, PlatformStore, ProviderStateRecord,
};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

struct RestartableMutation {
    request: EffectRequest,
    sent: bool,
}

impl Machine for RestartableMutation {
    type Effect = EffectRequest;
    type EffectResult = EffectResult;

    fn drive(
        &mut self,
        resume: MachineResume<Self::EffectResult>,
        _: Quantum,
    ) -> MachineDrive<Self::Effect> {
        match resume {
            MachineResume::Start if !self.sent => {
                self.sent = true;
                MachineDrive::HostCall(self.request.clone())
            }
            MachineResume::HostResult(result) => {
                assert_eq!(result.outcome, replay_success());
                MachineDrive::Completed(Completion {
                    return_code: 0,
                    output: BoundedPayload::new(
                        "resume-test@1",
                        Vec::new(),
                        InvocationLimits::default(),
                    )
                    .unwrap(),
                })
            }
            other => panic!("unexpected durable-resume input: {other:?}"),
        }
    }
}

struct ReconciledProvider {
    descriptor: CapabilityDescriptor,
    store: Arc<dyn PlatformStore>,
    report_unknown_once: AtomicBool,
    calls: Arc<AtomicUsize>,
    commits: Arc<AtomicUsize>,
}

impl HostProvider for ReconciledProvider {
    fn descriptor(&self) -> &CapabilityDescriptor {
        &self.descriptor
    }

    fn invoke(&self, _: &Invocation, request: EffectRequest) -> EffectResult {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let key = request.idempotency_key.as_ref().unwrap().as_str();
        if self
            .store
            .get_provider_state("resume-test-commit", key)
            .unwrap()
            .is_none()
        {
            self.store
                .put_provider_state(
                    ProviderStateRecord {
                        namespace: "resume-test-commit".into(),
                        key: key.into(),
                        version: 1,
                        payload: b"committed".to_vec(),
                    },
                    None,
                )
                .unwrap();
            self.commits.fetch_add(1, Ordering::SeqCst);
        }
        EffectResult {
            sequence: request.sequence,
            outcome: if self.report_unknown_once.swap(false, Ordering::SeqCst) {
                Err(HostProblem::UnknownOutcome)
            } else {
                replay_success()
            },
        }
    }
}

fn replay_success() -> Result<HostResult, HostProblem> {
    Ok(HostResult::State {
        value: Some(b"committed".to_vec()),
        version: 1,
    })
}

fn invocation(capability: CapabilityId, identity: &str) -> Invocation {
    let limits = InvocationLimits::default();
    let mut invocation = Invocation::new(
        RequestId::new(format!("request-{identity}"), limits).unwrap(),
        ExecutionId::new(format!("execution-{identity}"), limits).unwrap(),
        RunUnitId::new(format!("run-{identity}"), limits).unwrap(),
        None,
        Selector::new("test", limits).unwrap(),
        ArtifactRef::new("artifact", limits).unwrap(),
        Principal::new(
            PrincipalId::new("USER", limits).unwrap(),
            BTreeSet::from([capability.clone()]),
            limits,
        )
        .unwrap(),
        ServiceClass::Interactive,
        0,
        100,
        TraceId::new("trace", limits).unwrap(),
        IdempotencyKey::new(format!("invocation-key-{identity}"), limits).unwrap(),
        1,
        ResourceLimits::default(),
        BTreeMap::new(),
        limits,
    )
    .unwrap();
    invocation.provider_generations = BTreeMap::from([(capability, "1".into())]);
    invocation
}

fn resumable_fixture(
    store: Arc<dyn PlatformStore>,
    calls: Arc<AtomicUsize>,
    commits: Arc<AtomicUsize>,
    unknown: bool,
    identity: &str,
) -> (Arc<ScopedHostService>, Invocation, EffectRequest) {
    let limits = InvocationLimits::default();
    let capability = CapabilityId::new("host.state.write", limits).unwrap();
    let provider: Arc<dyn HostProvider> = Arc::new(ReconciledProvider {
        descriptor: CapabilityDescriptor {
            capability: capability.clone(),
            provider_id: "durable-resume-test".into(),
            generation: "1".into(),
            request_schema: "state-request@1".into(),
            result_schema: "state-result@1".into(),
            max_request_bytes: 4096,
            max_result_bytes: 4096,
            ready: true,
        },
        store,
        report_unknown_once: AtomicBool::new(unknown),
        calls,
        commits,
    });
    let host = Arc::new(ScopedHostService::new(
        Arc::new(RegistrySnapshot::new(1, vec![provider], limits).unwrap()),
        HostLimits::default(),
    ));
    let invocation = invocation(capability, identity);
    let key = IdempotencyKey::new(format!("{identity}-effect"), limits).unwrap();
    let request = EffectRequest {
        run_unit: invocation.run_unit_id.clone(),
        sequence: 1,
        deadline_tick: invocation.deadline_tick,
        idempotency_key: Some(key.clone()),
        request: HostRequest::State(StateRequest::Put {
            key: "business".into(),
            value: b"committed".to_vec(),
            expected_version: None,
            mutation: Mutation {
                sequence: 1,
                idempotency_key: key,
                transaction: None,
            },
        }),
    };
    (host, invocation, request)
}

fn assert_durable_resume_blocks_until_reconciled(store: Arc<dyn PlatformStore>) {
    let calls = Arc::new(AtomicUsize::new(0));
    let commits = Arc::new(AtomicUsize::new(0));
    let (host, invocation, request) = resumable_fixture(
        store.clone(),
        calls.clone(),
        commits.clone(),
        true,
        "resume",
    );
    let coordinator =
        ExecutionCoordinator::durable(host, store.clone(), CoordinatorLimits::default());
    let first = coordinator.execute_resumable_with_control(
        &mut RestartableMutation {
            request: request.clone(),
            sent: false,
        },
        &invocation,
        || {
            Ok(ExecutionControl {
                now_tick: 1,
                cancellation_requested: false,
            })
        },
    );
    assert!(
        matches!(first, ExecutionOutcome::ProviderFailure(problem) if problem.has_unknown_outcome())
    );
    assert_eq!(
        (calls.load(Ordering::SeqCst), commits.load(Ordering::SeqCst)),
        (1, 1)
    );
    assert_eq!(
        store
            .get_execution(&invocation.execution_id)
            .unwrap()
            .unwrap()
            .state,
        ExecutionState::Running
    );

    let (restarted_host, _, _) = resumable_fixture(
        store.clone(),
        calls.clone(),
        commits.clone(),
        false,
        "resume",
    );
    let restarted =
        ExecutionCoordinator::durable(restarted_host, store.clone(), CoordinatorLimits::default());
    let blocked = restarted.execute_resumable_with_control(
        &mut RestartableMutation {
            request: request.clone(),
            sent: false,
        },
        &invocation,
        || {
            Ok(ExecutionControl {
                now_tick: 2,
                cancellation_requested: false,
            })
        },
    );
    assert!(
        matches!(blocked, ExecutionOutcome::ProviderFailure(problem) if problem.has_unknown_outcome())
    );
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "unresolved work was redispatched"
    );

    store
        .reconcile_unknown_versioned(
            request.idempotency_key.as_ref().unwrap(),
            EffectState::Completed,
            EffectDigestFormat::CanonicalHostV1,
            canonical_result_digest(&replay_success()).unwrap(),
        )
        .unwrap();
    let (reconciled_host, _, _) = resumable_fixture(
        store.clone(),
        calls.clone(),
        commits.clone(),
        false,
        "resume",
    );
    let reconciled =
        ExecutionCoordinator::durable(reconciled_host, store.clone(), CoordinatorLimits::default());
    assert!(matches!(
        reconciled.execute_resumable_with_control(
            &mut RestartableMutation {
                request,
                sent: false,
            },
            &invocation,
            || Ok(ExecutionControl {
                now_tick: 3,
                cancellation_requested: false,
            }),
        ),
        ExecutionOutcome::Completed(_)
    ));
    assert_eq!(
        (calls.load(Ordering::SeqCst), commits.load(Ordering::SeqCst)),
        (2, 1)
    );
    assert_eq!(
        store
            .get_execution(&invocation.execution_id)
            .unwrap()
            .unwrap()
            .state,
        ExecutionState::Completed
    );
    let audits = store.audit_records(&invocation.execution_id, 1, 8).unwrap();
    assert_eq!(audits.len(), 2);
    assert_eq!(audits[0].decision, AuditDecision::UnknownOutcome);
    assert_eq!(audits[1].decision, AuditDecision::Success);
}

#[test]
fn durable_resume_blocks_unknown_then_replays_only_after_reconciliation() {
    assert_durable_resume_blocks_until_reconciled(Arc::new(MemoryStore::new(
        StoreLimits::default(),
    )));
}

struct SuspendingMachine;

impl Machine for SuspendingMachine {
    type Effect = EffectRequest;
    type EffectResult = EffectResult;

    fn drive(
        &mut self,
        _: MachineResume<Self::EffectResult>,
        _: Quantum,
    ) -> MachineDrive<Self::Effect> {
        MachineDrive::Suspended(Suspension {
            kind: "online-terminal".into(),
            resume_token: "resume".into(),
            state_bytes: 1,
        })
    }

    fn checkpoint(&self) -> Option<BoundedPayload> {
        BoundedPayload::new("handoff-test@1", vec![1], InvocationLimits::default()).ok()
    }
}

fn assert_suspended_handoff(store: Arc<dyn PlatformStore>) {
    let calls = Arc::new(AtomicUsize::new(0));
    let commits = Arc::new(AtomicUsize::new(0));
    let (host, invocation, _) = resumable_fixture(store.clone(), calls, commits, false, "handoff");
    let coordinator =
        ExecutionCoordinator::durable(host, store.clone(), CoordinatorLimits::default());
    assert!(matches!(
        coordinator.execute_resumable_with_control(&mut SuspendingMachine, &invocation, || Ok(
            ExecutionControl {
                now_tick: 1,
                cancellation_requested: false,
            }
        )),
        ExecutionOutcome::Suspended(_)
    ));
    assert!(
        store
            .get_checkpoint(&invocation.execution_id)
            .unwrap()
            .is_some()
    );

    coordinator
        .complete_suspended_handoff(&invocation, 2)
        .unwrap();
    let execution = store
        .get_execution(&invocation.execution_id)
        .unwrap()
        .unwrap();
    assert_eq!(execution.state, ExecutionState::Completed);
    assert!(
        store
            .get_checkpoint(&invocation.execution_id)
            .unwrap()
            .is_none()
    );
    assert!(matches!(
        store
            .events(&invocation.execution_id, execution.version, 1)
            .unwrap()
            .as_slice(),
        [event] if matches!(event.kind, LifecycleEventKind::HandoffCompleted)
    ));
}

#[test]
fn suspended_handoff_is_terminal_and_drops_the_interpreter_checkpoint() {
    assert_suspended_handoff(Arc::new(MemoryStore::new(StoreLimits::default())));
}

#[test]
#[ignore = "requires isolated MAINFRAME_ENV_POSTGRES_TEST_URL"]
fn postgres_durable_resume_blocks_until_reconciled() {
    let url = std::env::var("MAINFRAME_ENV_POSTGRES_TEST_URL")
        .expect("explicit PostgreSQL test URL required");
    let store: Arc<dyn PlatformStore> =
        Arc::new(PostgresStateStore::open(&url, 4 * 1024 * 1024, 65_536).unwrap());
    assert_durable_resume_blocks_until_reconciled(store.clone());
    assert_suspended_handoff(store);
}
