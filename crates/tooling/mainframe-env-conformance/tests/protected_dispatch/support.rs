//! Independent public-API fixtures; these do not attest compiled CardDemo or IBM MQ.
use mainframe_env_execution_api::{
    ArtifactRef, BoundedPayload, CapabilityId, ExecutionId, IdempotencyKey, Invocation,
    InvocationLimits, Principal, PrincipalId, RequestId, ResourceLimits, RunUnitId, Selector,
    ServiceClass, TraceId,
};
use mainframe_env_host_api::{
    CapabilityDescriptor, EffectRequest, EffectResult, HostLimits, HostProblem, HostProvider,
    HostResult, RegistrySnapshot, ScopedHostService,
};
use mainframe_env_interpreter::ReferenceMachine;
use mainframe_env_ir::{
    Attribute, CodecLimits, Effect, IrLimits, ModuleBuilder, OperationIdentity, encode_binary,
};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

type Trace = Arc<Mutex<Vec<String>>>;
pub type Calls = Arc<Mutex<Vec<(Invocation, EffectRequest)>>>;

pub fn invocation() -> Invocation {
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

pub fn machine(invocation: &Invocation) -> ReferenceMachine {
    let mut builder = ModuleBuilder::new(IrLimits::default());
    let region = builder.add_region().unwrap();
    let block = builder.add_block(region).unwrap();
    for program in ["FIRST", "SECOND"] {
        builder
            .add_operation(
                block,
                OperationIdentity::new("mainframe.core.cobol", "call", 1).unwrap(),
                vec![],
                0,
                BTreeMap::from([("arg_0".into(), Attribute::Text(format!("'{program}'")))]),
                vec![Effect::ProgramControl],
                vec![],
                None,
            )
            .unwrap();
    }
    builder
        .add_operation(
            block,
            OperationIdentity::new("mainframe.core.cobol", "halt", 1).unwrap(),
            vec![],
            0,
            BTreeMap::new(),
            vec![],
            vec![],
            None,
        )
        .unwrap();
    let binary = encode_binary(&builder.finish().unwrap(), CodecLimits::default()).unwrap();
    ReferenceMachine::from_binary(&binary, invocation.clone(), CodecLimits::default()).unwrap()
}

struct Port {
    descriptor: CapabilityDescriptor,
    trace: Trace,
    calls: Calls,
    unknown: bool,
}
impl HostProvider for Port {
    fn descriptor(&self) -> &CapabilityDescriptor {
        &self.descriptor
    }
    fn invoke(&self, invocation: &Invocation, request: EffectRequest) -> EffectResult {
        self.trace
            .lock()
            .unwrap()
            .push(format!("dispatch:{}", request.sequence));
        self.calls
            .lock()
            .unwrap()
            .push((invocation.clone(), request.clone()));
        EffectResult {
            sequence: request.sequence,
            outcome: if self.unknown {
                Err(HostProblem::UnknownOutcome)
            } else {
                // Independent literal: call-result@1, zero returned arguments.
                Ok(HostResult::Program(
                    BoundedPayload::new(
                        "mainframe-env.cobol.call-result@1",
                        vec![0, 0, 0, 0],
                        InvocationLimits::default(),
                    )
                    .unwrap(),
                ))
            },
        }
    }
}

pub fn host() -> (Arc<ScopedHostService>, Calls) {
    let trace: Trace = Arc::default();
    let calls: Calls = Arc::default();
    let port: Arc<dyn HostProvider> = Arc::new(Port {
        descriptor: CapabilityDescriptor {
            capability: CapabilityId::new("host.program.invoke", InvocationLimits::default())
                .unwrap(),
            provider_id: "fixture-program".into(),
            generation: "fixture-program@1".into(),
            request_schema: "mainframe-env.cobol.call@1".into(),
            result_schema: "mainframe-env.cobol.call-result@1".into(),
            max_request_bytes: 4096,
            max_result_bytes: 4096,
            ready: true,
        },
        trace: trace.clone(),
        calls: calls.clone(),
        unknown: false,
    });
    let host = Arc::new(ScopedHostService::new(
        Arc::new(RegistrySnapshot::new(1, vec![port], InvocationLimits::default()).unwrap()),
        HostLimits::default(),
    ));
    (host, calls)
}
