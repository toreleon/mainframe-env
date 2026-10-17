//! Genuine coordinator/store commits for nonmutating dispatch occurrences.
//! Test-only consumer: keeps the deterministic interpreter dependency direction unchanged.
use mainframe_env_diagnostics::FailureCategory;
use mainframe_env_execution_api::{
    ArtifactRef, AuditDecision, BoundedPayload, CapabilityId, Completion, ExecutionId,
    ExecutionOutcome, IdempotencyKey, Invocation, InvocationLimits, LifecycleEventKind, Machine,
    MachineDrive, MachineResume, Principal, PrincipalId, Quantum, RequestId, ResourceLimits,
    RunUnitId, Selector, ServiceClass, TraceId,
};
use mainframe_env_host_api::{
    CapabilityDescriptor, DatasetName, DatasetRequest, DatasetResult, EffectRequest, EffectResult,
    HostLimits, HostProblem, HostProvider, HostRequest, HostResult, RegistrySnapshot,
    ScopedHostService, canonical_audit_resource_digest, canonical_request_digest,
    canonical_result_digest,
};
use mainframe_env_interpreter::{CoordinatorLimits, ExecutionControl, ExecutionCoordinator};
use mainframe_env_store::{MemoryStore, StoreLimits};
use mainframe_env_store_api::{AuditSink, EffectState, EventStore, IdempotencyStore};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

fn invocation() -> Invocation {
    let limits = InvocationLimits::default();
    Invocation::new(
        RequestId::new("request", limits).unwrap(),
        ExecutionId::new("execution", limits).unwrap(),
        RunUnitId::new("run", limits).unwrap(),
        None,
        Selector::new("program:TWO-CALLS", limits).unwrap(),
        ArtifactRef::new("artifact:two-calls", limits).unwrap(),
        Principal::new(
            PrincipalId::new("USER", limits).unwrap(),
            BTreeSet::from([CapabilityId::new("host.program.invoke", limits).unwrap()]),
            limits,
        )
        .unwrap(),
        ServiceClass::Batch,
        0,
        100,
        TraceId::new("trace", limits).unwrap(),
        IdempotencyKey::new("key", limits).unwrap(),
        1,
        ResourceLimits::default(),
        BTreeMap::new(),
        limits,
    )
    .unwrap()
}

struct OneRead(Option<EffectRequest>);
impl Machine for OneRead {
    type Effect = EffectRequest;
    type EffectResult = EffectResult;
    fn drive(
        &mut self,
        resume: MachineResume<EffectResult>,
        _: Quantum,
    ) -> MachineDrive<EffectRequest> {
        match resume {
            MachineResume::Start => MachineDrive::HostCall(self.0.take().unwrap()),
            MachineResume::HostResult(_) => MachineDrive::Completed(Completion {
                return_code: 0,
                output: BoundedPayload::new("test@1", Vec::new(), InvocationLimits::default())
                    .unwrap(),
            }),
            _ => panic!("unexpected resume"),
        }
    }
}
struct ReadPort {
    descriptor: CapabilityDescriptor,
    store: Arc<MemoryStore>,
    calls: Arc<AtomicUsize>,
    unknown: bool,
}
impl HostProvider for ReadPort {
    fn descriptor(&self) -> &CapabilityDescriptor {
        &self.descriptor
    }
    fn invoke(&self, actor: &Invocation, effect: EffectRequest) -> EffectResult {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let intent = self
            .store
            .effect(effect.idempotency_key.as_ref().unwrap())
            .unwrap()
            .unwrap();
        assert_eq!(intent.state, EffectState::Intent);
        assert_eq!(intent.execution_id, actor.execution_id);
        assert_eq!(intent.run_unit_id, actor.run_unit_id);
        assert_eq!(intent.sequence, effect.sequence);
        assert_eq!(
            intent.request_digest,
            canonical_request_digest(&effect.request).unwrap()
        );
        assert!(!effect.request.is_mutating());
        assert_eq!(effect.request.mutation(), None);
        EffectResult {
            sequence: effect.sequence,
            outcome: if self.unknown {
                Err(HostProblem::UnknownOutcome)
            } else {
                Ok(HostResult::Dataset(DatasetResult::Records {
                    records: vec![b"ONE".to_vec()],
                    identities: vec![b"1".to_vec()],
                    version: 1,
                }))
            },
        }
    }
}
fn offered(actor: &Invocation) -> EffectRequest {
    EffectRequest {
        run_unit: actor.run_unit_id.clone(),
        sequence: 1,
        deadline_tick: actor.deadline_tick,
        idempotency_key: Some(
            IdempotencyKey::new(
                format!("{}:1", actor.idempotency_key.as_str()),
                InvocationLimits::default(),
            )
            .unwrap(),
        ),
        request: HostRequest::Dataset(DatasetRequest::Read {
            dataset: DatasetName::new("TEST.INPUT", 128).unwrap(),
            member: None,
            key: None,
            control: Default::default(),
            max_records: 1,
        }),
    }
}
fn fixture(
    limits: StoreLimits,
    unknown: bool,
) -> (
    ExecutionCoordinator,
    Invocation,
    Arc<MemoryStore>,
    Arc<AtomicUsize>,
) {
    let limits_id = InvocationLimits::default();
    let mut actor = invocation();
    actor.principal = Principal::new(
        PrincipalId::new("USER", limits_id).unwrap(),
        BTreeSet::from([CapabilityId::new("host.dataset.read", limits_id).unwrap()]),
        limits_id,
    )
    .unwrap();
    let store = Arc::new(MemoryStore::new(limits));
    let calls = Arc::new(AtomicUsize::new(0));
    let provider: Arc<dyn HostProvider> = Arc::new(ReadPort {
        descriptor: CapabilityDescriptor {
            capability: CapabilityId::new("host.dataset.read", limits_id).unwrap(),
            provider_id: "readonly-journal".into(),
            generation: "1".into(),
            request_schema: "dataset-request@1".into(),
            result_schema: "dataset-result@1".into(),
            max_request_bytes: 4096,
            max_result_bytes: 4096,
            ready: true,
        },
        store: store.clone(),
        calls: calls.clone(),
        unknown,
    });
    let host = Arc::new(ScopedHostService::new(
        Arc::new(RegistrySnapshot::new(1, vec![provider], limits_id).unwrap()),
        HostLimits::default(),
    ));
    (
        ExecutionCoordinator::durable(host, store.clone(), CoordinatorLimits::default()),
        actor,
        store,
        calls,
    )
}
fn execute(
    coordinator: &ExecutionCoordinator,
    actor: &Invocation,
    effect: EffectRequest,
) -> ExecutionOutcome {
    let mut tick = 0;
    coordinator.execute_with_control(&mut OneRead(Some(effect)), actor, || {
        tick += 1;
        Ok(ExecutionControl {
            now_tick: tick,
            cancellation_requested: false,
        })
    })
}

#[test]
fn genuine_read_intent_result_events_and_success_audit_bind_one_actual_dispatch() {
    let (coordinator, actor, store, calls) = fixture(StoreLimits::default(), false);
    let effect = offered(&actor);
    let key = effect.idempotency_key.clone().unwrap();
    assert!(matches!(
        execute(&coordinator, &actor, effect.clone()),
        ExecutionOutcome::Completed(_)
    ));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let record = store.effect(&key).unwrap().unwrap();
    assert_eq!(record.state, EffectState::Completed);
    assert_eq!(record.intent.owner, actor.execution_id);
    assert_eq!(record.intent.attempt, actor.attempt);
    assert_eq!(
        record.intent.audit_invocation_key,
        Some(actor.idempotency_key.clone())
    );
    assert_eq!(
        record.intent.capability.unwrap().as_str(),
        "host.dataset.read"
    );
    assert_eq!(
        record.intent.audit_resource,
        Some(canonical_audit_resource_digest(&effect.request))
    );
    let expected = Ok(HostResult::Dataset(DatasetResult::Records {
        records: vec![b"ONE".to_vec()],
        identities: vec![b"1".to_vec()],
        version: 1,
    }));
    assert_eq!(
        record.result_digest,
        Some(canonical_result_digest(&expected).unwrap())
    );
    assert!(record.resolved_tick.is_some_and(|tick| tick > 0));
    let audits = store.audit_records(&actor.execution_id, 0, 8).unwrap();
    assert_eq!(audits.len(), 1);
    assert_eq!(audits[0].decision, AuditDecision::Success);
    assert_eq!(audits[0].effect_sequence, 1);
    assert_eq!(
        audits[0].resource,
        canonical_audit_resource_digest(&effect.request)
    );
    let events = store.events(&actor.execution_id, 0, 16).unwrap();
    let paired: Vec<_> = events
        .iter()
        .filter_map(|event| match &event.kind {
            LifecycleEventKind::EffectIntent { sequence } => Some((false, *sequence)),
            LifecycleEventKind::EffectResult { sequence } => Some((true, *sequence)),
            _ => None,
        })
        .collect();
    assert_eq!(paired, [(false, 1), (true, 1)]);
}

#[test]
fn readonly_result_persistence_loss_retains_original_intent_and_unknown_outcome() {
    let (coordinator, actor, store, calls) = fixture(
        StoreLimits {
            max_audits: 0,
            ..StoreLimits::default()
        },
        false,
    );
    let effect = offered(&actor);
    let key = effect.idempotency_key.clone().unwrap();
    assert!(
        matches!(execute(&coordinator, &actor, effect), ExecutionOutcome::ProviderFailure(problem) if problem.has_unknown_outcome())
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let record = store.effect(&key).unwrap().unwrap();
    assert_eq!(record.state, EffectState::Intent);
    assert_eq!(record.result_digest, None);
    assert_eq!(record.resolved_tick, None);
    let original_intent = record;
    let mut tick = 10;
    let resumed = coordinator.execute_resumable_with_control(
        &mut OneRead(Some(offered(&actor))),
        &actor,
        || {
            tick += 1;
            Ok(ExecutionControl {
                now_tick: tick,
                cancellation_requested: false,
            })
        },
    );
    assert!(
        matches!(resumed, ExecutionOutcome::ProviderFailure(problem) if problem.has_unknown_outcome())
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(store.effect(&key).unwrap().unwrap(), original_intent);
}

#[test]
fn unknown_read_reply_records_uncertainty_and_never_completes() {
    let (coordinator, actor, store, calls) = fixture(StoreLimits::default(), true);
    let effect = offered(&actor);
    let key = effect.idempotency_key.clone().unwrap();
    assert!(
        matches!(execute(&coordinator, &actor, effect), ExecutionOutcome::ProviderFailure(problem) if problem.has_unknown_outcome())
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let record = store.effect(&key).unwrap().unwrap();
    assert_eq!(record.state, EffectState::UnknownOutcome);
    assert_eq!(record.resolved_tick, None);
    assert_eq!(
        record.result_digest,
        Some(canonical_result_digest(&Err(HostProblem::UnknownOutcome)).unwrap())
    );
}

#[test]
fn failed_read_intent_persistence_refuses_before_provider_dispatch() {
    let (coordinator, actor, store, calls) = fixture(
        StoreLimits {
            max_effects: 0,
            ..StoreLimits::default()
        },
        false,
    );
    let effect = offered(&actor);
    let key = effect.idempotency_key.clone().unwrap();
    assert!(
        matches!(execute(&coordinator, &actor, effect), ExecutionOutcome::InfrastructureFailure(problem) if problem.category == FailureCategory::InfrastructureFailure)
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(store.effect(&key).unwrap(), None);
}
