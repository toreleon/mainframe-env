use mainframe_env_execution_api::{
    ArtifactRef, CapabilityId, Completion, ExecutionOutcome, IdempotencyKey, Invocation,
    InvocationLimits, LifecycleEventKind, Machine, MachineDrive, MachineResume, Principal,
    PrincipalId, Quantum, RequestId, ResourceLimits, RunUnitId, Selector, ServiceClass, TraceId,
};
use mainframe_env_host_api::{
    CapabilityDescriptor, EffectRequest, EffectResult, HostLimits, HostProblem, HostProvider,
    HostRequest, HostResult, RegistrySnapshot, ScopedHostService, StateRequest,
};
use mainframe_env_interpreter::{CoordinatorLimits, ExecutionControl, ExecutionCoordinator};
use mainframe_env_store::MemoryStore;
use mainframe_env_store_api::PlatformStore;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

struct FixedProvider {
    descriptor: CapabilityDescriptor,
    outcome: Result<HostResult, HostProblem>,
}

impl HostProvider for FixedProvider {
    fn descriptor(&self) -> &CapabilityDescriptor {
        &self.descriptor
    }

    fn invoke(&self, _: &Invocation, request: EffectRequest) -> EffectResult {
        EffectResult {
            sequence: request.sequence,
            outcome: self.outcome.clone(),
        }
    }
}

struct OneHostCall(Option<EffectRequest>);

impl Machine for OneHostCall {
    type Effect = EffectRequest;
    type EffectResult = EffectResult;

    fn drive(
        &mut self,
        resume: MachineResume<Self::EffectResult>,
        _: Quantum,
    ) -> MachineDrive<Self::Effect> {
        match resume {
            MachineResume::Start => MachineDrive::HostCall(self.0.take().expect("single call")),
            MachineResume::HostResult(_) => MachineDrive::Completed(Completion {
                return_code: 0,
                output: mainframe_env_execution_api::BoundedPayload::new(
                    "test@1",
                    Vec::new(),
                    InvocationLimits::default(),
                )
                .expect("bounded output"),
            }),
            other => panic!("unexpected resume: {other:?}"),
        }
    }
}

fn invocation() -> Invocation {
    let limits = InvocationLimits::default();
    let capability = CapabilityId::new("host.state.read", limits).expect("capability");
    Invocation::new(
        RequestId::new("request", limits).expect("request"),
        mainframe_env_execution_api::ExecutionId::new("execution", limits).expect("execution"),
        RunUnitId::new("run", limits).expect("run"),
        None,
        Selector::new("test", limits).expect("selector"),
        ArtifactRef::new("artifact", limits).expect("artifact"),
        Principal::new(
            PrincipalId::new("USER", limits).expect("principal"),
            BTreeSet::from([capability]),
            limits,
        )
        .expect("principal grants"),
        ServiceClass::Interactive,
        0,
        100,
        TraceId::new("trace", limits).expect("trace"),
        IdempotencyKey::new("invocation-key", limits).expect("invocation key"),
        1,
        ResourceLimits::default(),
        BTreeMap::new(),
        limits,
    )
    .expect("invocation")
}

fn audited_host(outcome: Result<HostResult, HostProblem>) -> Arc<ScopedHostService> {
    let limits = InvocationLimits::default();
    let capability = CapabilityId::new("host.state.read", limits).expect("capability");
    let provider: Arc<dyn HostProvider> = Arc::new(FixedProvider {
        descriptor: CapabilityDescriptor {
            capability,
            provider_id: "retention-test".into(),
            generation: "retention-test@1".into(),
            request_schema: "state-request@1".into(),
            result_schema: "state-result@1".into(),
            max_request_bytes: 4096,
            max_result_bytes: 4096,
            ready: true,
        },
        outcome,
    });
    Arc::new(ScopedHostService::new(
        Arc::new(RegistrySnapshot::new(1, vec![provider], limits).expect("registry")),
        HostLimits::default(),
    ))
}

#[test]
fn post_dispatch_tick_ages_effect_without_rewriting_dispatch_evidence() {
    let invocation = invocation();
    let key = IdempotencyKey::new("post-dispatch-effect", InvocationLimits::default())
        .expect("effect key");
    let request = EffectRequest {
        run_unit: invocation.run_unit_id.clone(),
        sequence: 1,
        deadline_tick: 100,
        idempotency_key: Some(key.clone()),
        request: HostRequest::State(StateRequest::Get { key: "one".into() }),
    };
    let store: Arc<dyn PlatformStore> = Arc::new(MemoryStore::new(Default::default()));
    let coordinator = ExecutionCoordinator::durable(
        audited_host(Ok(HostResult::State {
            value: Some(vec![1]),
            version: 1,
        })),
        store.clone(),
        CoordinatorLimits::default(),
    );
    let mut observations = 0;
    let outcome =
        coordinator.execute_with_control(&mut OneHostCall(Some(request)), &invocation, || {
            observations += 1;
            Ok(ExecutionControl {
                now_tick: if observations <= 3 { 10 } else { 90 },
                cancellation_requested: false,
            })
        });
    assert!(matches!(outcome, ExecutionOutcome::Completed(_)));
    assert_eq!(
        store
            .effect(&key)
            .expect("effect read")
            .expect("effect")
            .resolved_tick,
        Some(90)
    );
    let result_event = store
        .events(&invocation.execution_id, 1, 16)
        .expect("event read")
        .into_iter()
        .find(|event| matches!(event.kind, LifecycleEventKind::EffectResult { sequence: 1 }))
        .expect("result event");
    assert_eq!(result_event.tick, 10);
    assert_eq!(
        store
            .audit_records(&invocation.execution_id, 1, 8)
            .expect("audit read")[0]
            .observed_tick,
        10
    );
}
