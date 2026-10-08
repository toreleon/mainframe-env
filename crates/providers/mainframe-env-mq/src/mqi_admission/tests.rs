use super::*;
use mainframe_env_execution_api::{
    ArtifactRef, BoundedPayload, CancellationProbe, ExecutionId, IdempotencyKey, InvocationLimits,
    Principal, RequestId, ResourceLimits, RunUnitId, Selector, ServiceClass, TraceId,
};
use mainframe_env_host_api::{HostRequest, MqHandleSharing, MqHconn, MqMqiHostRequest};
use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};
mod full_message;

fn invocation() -> Invocation {
    let l = InvocationLimits::default();
    let mut value = Invocation::new(
        RequestId::new("request", l).unwrap(),
        ExecutionId::new("execution", l).unwrap(),
        RunUnitId::new("run", l).unwrap(),
        None,
        Selector::new("test", l).unwrap(),
        ArtifactRef::new("artifact", l).unwrap(),
        Principal::new(
            PrincipalId::new("IBMUSER", l).unwrap(),
            BTreeSet::from([
                CapabilityId::new("host.mq.write", l).unwrap(),
                CapabilityId::new("host.mq.read", l).unwrap(),
            ]),
            l,
        )
        .unwrap(),
        ServiceClass::System,
        0,
        100,
        TraceId::new("trace", l).unwrap(),
        IdempotencyKey::new("invocation-key", l).unwrap(),
        1,
        ResourceLimits::default(),
        BTreeMap::new(),
        l,
    )
    .unwrap();
    bind(
        &mut value,
        "mq.host-context",
        "mainframe-env.mq.host-context@1",
        b"other-bindings|queue-manager",
    );
    value
}

fn bind(value: &mut Invocation, name: &str, schema: &str, bytes: &[u8]) {
    value.bindings.insert(
        name.into(),
        BoundedPayload::new(schema, bytes.to_vec(), InvocationLimits::default()).unwrap(),
    );
}

fn owner() -> MqHandleOwner {
    MqHandleOwner {
        environment: MqHostEnvironment::OtherBindings,
        host_id: 1,
        process_id: 2,
        thread_id: 3,
        task_id: 4,
        syncpoint_epoch: 5,
    }
}

fn envelope() -> MqMqiRequestEnvelope {
    MqMqiRequestEnvelope {
        context: MqMqiContext {
            owner: owner(),
            syncpoint_owner: MqSyncpointOwner::QueueManager,
        },
        limits: MqMqiLimits::default(),
        request: MqMqiRequest::Connect(MqMqiConnect {
            manager: None,
            sharing: MqHandleSharing::NonShared,
            options: MqMqiOptions::ContractDefault,
        }),
    }
}

fn mutation() -> Mutation {
    Mutation {
        sequence: 7,
        idempotency_key: IdempotencyKey::new("effect-key", InvocationLimits::default()).unwrap(),
        transaction: None,
    }
}

fn effect(
    value: &Invocation,
    mutation: &Mutation,
    envelope: &MqMqiRequestEnvelope,
) -> EffectRequest {
    EffectRequest {
        run_unit: value.run_unit_id.clone(),
        sequence: mutation.sequence,
        deadline_tick: 90,
        idempotency_key: Some(mutation.idempotency_key.clone()),
        request: HostRequest::MqMqi(Box::new(MqMqiHostRequest {
            envelope: envelope.clone(),
            mutation: mutation.clone(),
        })),
    }
}

fn scope<'a>(
    invocation: &'a Invocation,
    owner: MqHandleOwner,
    effect: &'a EffectRequest,
    provider: &'a CapabilityDescriptor,
) -> Result<MqMqiServiceScope<'a>, HostProblem> {
    let original = effect
        .mq_mqi_occurrence(HostLimits::default())?
        .ok_or(HostProblem::Malformed)?;
    Ok(MqMqiServiceScope::for_host_dispatch(
        invocation,
        owner,
        original,
        provider,
        HostLimits::default(),
    ))
}

fn attempt(
    trusted: &Invocation,
    invocation: &Invocation,
    owner: MqHandleOwner,
    original: &EffectRequest,
    provider: &CapabilityDescriptor,
    tick: u64,
) -> Result<(), HostProblem> {
    let scope = scope(trusted, owner, original, provider)?;
    admit_mqi(&scope, invocation, tick).map(|_| ())
}

fn original_mut(effect: &mut EffectRequest) -> &mut MqMqiHostRequest {
    match &mut effect.request {
        HostRequest::MqMqi(value) => value,
        _ => unreachable!(),
    }
}

fn provider() -> CapabilityDescriptor {
    CapabilityDescriptor {
        capability: CapabilityId::new("host.mq.write", InvocationLimits::default()).unwrap(),
        provider_id: "mainframe-env-mq".into(),
        generation: "1".into(),
        request_schema: "mainframe-env.mq-request@1".into(),
        result_schema: "mainframe-env.mq-result@1".into(),
        max_request_bytes: 4 * 1024 * 1024,
        max_result_bytes: 4 * 1024 * 1024,
        ready: true,
    }
}

#[test]
fn malformed_original_owner_context_run_sequence_and_key_never_reach_state() {
    let inv = invocation();
    let m = mutation();
    let p = provider();
    let baseline = effect(&inv, &m, &envelope());
    let protected = Cell::new(0);
    for case in 0..10 {
        let mut original = baseline.clone();
        match case {
            0 => original_mut(&mut original).envelope.context.owner.task_id += 1,
            1 => {
                original_mut(&mut original).envelope.context.syncpoint_owner =
                    MqSyncpointOwner::HostCoordinator
            }
            2 => original_mut(&mut original).mutation.sequence += 1,
            3 => {
                original_mut(&mut original).mutation.idempotency_key =
                    IdempotencyKey::new("forged-key", InvocationLimits::default()).unwrap()
            }
            4 => original.sequence += 1,
            5 => original.idempotency_key = None,
            6 => {
                original.run_unit =
                    RunUnitId::new("foreign-run", InvocationLimits::default()).unwrap()
            }
            7 => original.sequence = 0,
            8 => original_mut(&mut original).mutation.transaction = Some(String::new()),
            9 => original_mut(&mut original).envelope.context.owner.host_id = 0,
            _ => unreachable!(),
        }
        let before = original.clone();
        let result = attempt(&inv, &inv, owner(), &original, &p, 1);
        if result.is_ok() {
            protected.set(protected.get() + 1);
        }
        assert!(result.is_err(), "original case {case}");
        assert_eq!(original, before);
    }
    assert_eq!(protected.get(), 0);
}

#[test]
fn live_controls_and_revoked_grants_fail_before_dispatch() {
    let inv = invocation().with_cancellation_probe(CancellationProbe::new());
    let original = effect(&inv, &mutation(), &envelope());
    let p = provider();
    let mut revoked = inv.clone();
    revoked.principal = Principal::new(
        inv.principal.id().clone(),
        BTreeSet::new(),
        InvocationLimits::default(),
    )
    .unwrap();
    assert_eq!(
        attempt(&inv, &revoked, owner(), &original, &p, 1),
        Err(HostProblem::Unauthorized)
    );
    for deadline in [0, 1, u64::MAX] {
        let mut value = original.clone();
        value.deadline_tick = deadline;
        assert!(attempt(&inv, &inv, owner(), &value, &p, 1).is_err());
    }
    assert_eq!(
        attempt(&inv, &inv, owner(), &original, &p, 90),
        Err(HostProblem::TimedOut)
    );
    inv.cancellation_probe.as_ref().unwrap().request();
    assert_eq!(
        attempt(&inv, &inv, owner(), &original, &p, 1),
        Err(HostProblem::Cancelled)
    );
}

mod contexts;
mod controls_and_bounds;
mod original_binding;
mod results;
