use super::*;
use mainframe_env_execution_api::{
    ArtifactRef, BoundedPayload, CancellationProbe, ExecutionId, InvocationLimits, Principal,
    RequestId, ResourceLimits, Selector, ServiceClass, TraceId,
};
use mainframe_env_host_api::{HostRequest, MqHandleSharing, MqHconn, MqOperation, MqRequest};
use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};

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

fn effect(value: &Invocation, mutation: &Mutation) -> EffectRequest {
    // Only the existing occurrence metadata is projected here. This legacy
    // payload is never dispatched or accepted as a typed MQI payload.
    EffectRequest {
        run_unit: value.run_unit_id.clone(),
        sequence: mutation.sequence,
        deadline_tick: 90,
        idempotency_key: Some(mutation.idempotency_key.clone()),
        request: HostRequest::Mq(MqRequest {
            operation: MqOperation::Open,
            queue: None,
            handle: None,
            message: Vec::new(),
            message_id: None,
            correlation_id: None,
            options: 0,
            wait_ticks: 0,
            max_message_bytes: 1024,
            mutation: Some(mutation.clone()),
        }),
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
fn forged_owner_and_occurrence_never_reach_state_or_provider() {
    let inv = invocation();
    let env = envelope();
    let m = mutation();
    let e = effect(&inv, &m);
    let p = provider();
    let scope = MqMqiServiceScope::for_host_dispatch(
        &inv,
        owner(),
        &env,
        &m,
        MqMqiEffectOccurrence::from_effect(&e),
        &p,
    );
    let calls = Cell::new(0);
    for case in 0..9 {
        let mut candidate_inv = inv.clone();
        let mut candidate_env = env.clone();
        let mut candidate_m = m.clone();
        let mut candidate_e = e.clone();
        match case {
            0 => candidate_env.context.owner.task_id += 1,
            1 => candidate_env.context.syncpoint_owner = MqSyncpointOwner::HostCoordinator,
            2 => candidate_m.sequence += 1,
            3 => {
                candidate_m.idempotency_key =
                    IdempotencyKey::new("forged-key", InvocationLimits::default()).unwrap()
            }
            4 => {
                candidate_inv.run_unit_id =
                    RunUnitId::new("forged-run", InvocationLimits::default()).unwrap()
            }
            5 => {
                candidate_inv.execution_id =
                    ExecutionId::new("forged-execution", InvocationLimits::default()).unwrap()
            }
            6 => candidate_e.sequence += 1,
            7 => candidate_e.idempotency_key = None,
            8 => {
                candidate_e.run_unit =
                    RunUnitId::new("forged-run", InvocationLimits::default()).unwrap()
            }
            _ => unreachable!(),
        }
        let result = admit_mqi(
            &scope,
            &candidate_inv,
            &candidate_env,
            &candidate_m,
            MqMqiEffectOccurrence::from_effect(&candidate_e),
            1,
        );
        if matches!(result, Ok(MqMqiAdmission::ServiceValidation(_))) {
            calls.set(calls.get() + 1);
        }
        assert!(result.is_err(), "forgery {case}");
    }
    assert_eq!(calls.get(), 0);
}

#[test]
fn live_controls_and_revoked_grants_fail_before_dispatch() {
    let inv = invocation().with_cancellation_probe(CancellationProbe::new());
    let env = envelope();
    let m = mutation();
    let e = effect(&inv, &m);
    let p = provider();
    let scope = MqMqiServiceScope::for_host_dispatch(
        &inv,
        owner(),
        &env,
        &m,
        MqMqiEffectOccurrence::from_effect(&e),
        &p,
    );
    let mut revoked = inv.clone();
    revoked.principal = Principal::new(
        inv.principal.id().clone(),
        BTreeSet::new(),
        InvocationLimits::default(),
    )
    .unwrap();
    assert_eq!(
        admit_mqi(
            &scope,
            &revoked,
            &env,
            &m,
            MqMqiEffectOccurrence::from_effect(&e),
            1
        )
        .unwrap_err(),
        HostProblem::Unauthorized
    );
    for deadline in [0, 1, u64::MAX] {
        let mut candidate = e.clone();
        candidate.deadline_tick = deadline;
        assert!(
            admit_mqi(
                &scope,
                &inv,
                &env,
                &m,
                MqMqiEffectOccurrence::from_effect(&candidate),
                1
            )
            .is_err()
        );
    }
    assert!(
        admit_mqi(
            &scope,
            &inv,
            &env,
            &m,
            MqMqiEffectOccurrence::from_effect(&e),
            90
        )
        .is_err()
    );
    inv.cancellation_probe.as_ref().unwrap().request();
    assert_eq!(
        admit_mqi(
            &scope,
            &inv,
            &env,
            &m,
            MqMqiEffectOccurrence::from_effect(&e),
            1
        )
        .unwrap_err(),
        HostProblem::Cancelled
    );
}

mod contexts;
mod controls_and_bounds;
