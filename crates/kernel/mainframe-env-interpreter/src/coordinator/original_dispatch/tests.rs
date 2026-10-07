//! Real ReferenceMachine loop checks; fixture ports do not attest installed MQ.
use super::*;
use crate::ReferenceMachine;
use mainframe_env_execution_api::{
    ArtifactRef, AuditDecision, BoundedPayload, CapabilityId, ExecutionId, IdempotencyKey,
    Principal, PrincipalId, RequestId, ResourceLimits, RunUnitId, Selector, ServiceClass, TraceId,
};
use mainframe_env_host_api::{
    CapabilityDescriptor, HostLimits, HostProvider, HostRequest, HostResult, ProgramRequest,
    RegistrySnapshot,
};
use mainframe_env_ir::{
    Attribute, CodecLimits, Effect, IrLimits, ModuleBuilder, OperationIdentity, encode_binary,
};
use std::collections::BTreeSet;
use std::sync::Mutex;

type Trace = Arc<Mutex<Vec<String>>>;
type Calls = Arc<Mutex<Vec<(Invocation, EffectRequest)>>>;

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

fn machine(invocation: &Invocation) -> ReferenceMachine {
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

struct Audits {
    trace: Trace,
    records: Mutex<Vec<AuditRecord>>,
    capacity: usize,
}
impl AuditSink for Audits {
    fn record_audit(&self, record: AuditRecord) -> Result<(), StoreError> {
        self.trace
            .lock()
            .unwrap()
            .push(format!("audit:{}", record.effect_sequence));
        let mut records = self.records.lock().unwrap();
        if records.len() == self.capacity {
            return Err(StoreError::CapacityExceeded);
        }
        records.push(record);
        Ok(())
    }
    fn audit_records(
        &self,
        execution: &ExecutionId,
        start: u64,
        max: usize,
    ) -> Result<Vec<AuditRecord>, StoreError> {
        Ok(self
            .records
            .lock()
            .unwrap()
            .iter()
            .filter(|r| &r.execution_id == execution && r.effect_sequence >= start)
            .take(max)
            .cloned()
            .collect())
    }
}

fn fixture(unknown: bool, capacity: usize) -> (ExecutionCoordinator, Trace, Calls, Arc<Audits>) {
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
        unknown,
    });
    let host = Arc::new(ScopedHostService::new(
        Arc::new(RegistrySnapshot::new(1, vec![port], InvocationLimits::default()).unwrap()),
        HostLimits::default(),
    ));
    let audits = Arc::new(Audits {
        trace: trace.clone(),
        records: Mutex::default(),
        capacity,
    });
    (
        ExecutionCoordinator::with_host(host, audits.clone(), CoordinatorLimits::default()),
        trace,
        calls,
        audits,
    )
}

#[test]
fn reference_machine_keeps_original_occurrences_and_exact_observer_audit_order() {
    let invocation = invocation();
    let (coordinator, trace, calls, audits) = fixture(false, 2);
    let mut observation = 0;
    let outcome = coordinator.execute_with_control(&mut machine(&invocation), &invocation, || {
        observation += 1;
        trace.lock().unwrap().push(format!("observe:{observation}"));
        Ok(ExecutionControl {
            now_tick: observation,
            cancellation_requested: false,
        })
    });
    assert!(matches!(outcome, ExecutionOutcome::Completed(_)));
    assert_eq!(
        *trace.lock().unwrap(),
        [
            "observe:1",
            "observe:2",
            "observe:3",
            "dispatch:1",
            "observe:4",
            "audit:1",
            "observe:5",
            "observe:6",
            "dispatch:2",
            "observe:7",
            "audit:2",
            "observe:8",
        ]
    );
    for ((original, effect), (sequence, program, key)) in calls
        .lock()
        .unwrap()
        .iter()
        .zip([(1, "FIRST", "key:1"), (2, "SECOND", "key:2")])
    {
        assert_eq!(original, &invocation);
        assert_eq!(effect.sequence, sequence);
        assert_eq!(effect.run_unit.as_str(), "run");
        assert_eq!(effect.deadline_tick, 100);
        assert_eq!(effect.idempotency_key.as_ref().unwrap().as_str(), key);
        let HostRequest::Program(ProgramRequest::Call {
            program: actual,
            payload,
            service,
        }) = &effect.request
        else {
            panic!("actual Program call required")
        };
        assert_eq!(actual.as_str(), program);
        assert_eq!(payload.schema(), "mainframe-env.cobol.call@1");
        assert_eq!(payload.bytes(), &[0, 0, 0, 0]);
        assert_eq!(service, &None);
    }
    let records = audits.records.lock().unwrap();
    assert_eq!(records.len(), 2);
    for (record, (sequence, tick)) in records.iter().zip([(1, 3), (2, 6)]) {
        assert_eq!(record.execution_id.as_str(), "execution");
        assert_eq!(record.run_unit_id.as_str(), "run");
        assert_eq!(record.principal.as_str(), "USER");
        assert_eq!(record.invocation_key.as_str(), "key");
        assert_eq!(record.effect_sequence, sequence);
        assert_eq!(record.observed_tick, tick);
        assert_eq!(record.decision, AuditDecision::Success);
    }
}

#[test]
fn reference_machine_stops_before_dispatch_when_pre_dispatch_control_cancels() {
    let invocation = invocation();
    let (coordinator, _, calls, audits) = fixture(false, 2);
    let mut observations = 0;
    let outcome = coordinator.execute_with_control(&mut machine(&invocation), &invocation, || {
        observations += 1;
        Ok(ExecutionControl {
            now_tick: observations,
            cancellation_requested: observations == 3,
        })
    });
    assert_eq!(outcome, ExecutionOutcome::Cancelled);
    assert_eq!(observations, 3);
    assert!(calls.lock().unwrap().is_empty());
    assert!(audits.records.lock().unwrap().is_empty());
}

#[test]
fn reference_machine_retains_first_audit_and_never_dispatches_second_after_late_stop() {
    for mode in ["cancel", "timeout", "unavailable", "regressed"] {
        let invocation = invocation();
        let (coordinator, _, calls, audits) = fixture(false, 2);
        let mut observations = 0;
        let outcome =
            coordinator.execute_with_control(&mut machine(&invocation), &invocation, || {
                observations += 1;
                if observations == 4 && mode == "unavailable" {
                    return Err(ExecutionControlError::Unavailable);
                }
                Ok(ExecutionControl {
                    now_tick: if observations == 4 {
                        match mode {
                            "timeout" => 100,
                            "regressed" => 1,
                            _ => observations,
                        }
                    } else {
                        observations
                    },
                    cancellation_requested: observations == 4 && mode == "cancel",
                })
            });
        match mode {
            "cancel" => assert_eq!(outcome, ExecutionOutcome::Cancelled),
            "timeout" => assert_eq!(outcome, ExecutionOutcome::TimedOut),
            _ => assert!(matches!(
                outcome,
                ExecutionOutcome::InfrastructureFailure(_)
            )),
        }
        assert_eq!(observations, 4);
        assert_eq!(calls.lock().unwrap().len(), 1);
        assert_eq!(audits.records.lock().unwrap().len(), 1);
    }
}

#[test]
fn reference_machine_preserves_unknown_ahead_of_late_controls() {
    for unavailable in [false, true] {
        let invocation = invocation();
        let (coordinator, _, calls, audits) = fixture(true, 2);
        let mut observations = 0;
        let outcome =
            coordinator.execute_with_control(&mut machine(&invocation), &invocation, || {
                observations += 1;
                if observations == 4 && unavailable {
                    return Err(ExecutionControlError::Unavailable);
                }
                Ok(ExecutionControl {
                    now_tick: observations,
                    cancellation_requested: observations == 4,
                })
            });
        assert!(matches!(outcome, ExecutionOutcome::ProviderFailure(p) if p.has_unknown_outcome()));
        assert_eq!(observations, 4);
        assert_eq!(calls.lock().unwrap().len(), 1);
        assert_eq!(
            audits.records.lock().unwrap()[0].decision,
            AuditDecision::UnknownOutcome
        );
    }
}

#[test]
fn reference_machine_audit_failure_after_mutating_call_is_unknown_without_redispatch() {
    let invocation = invocation();
    let (coordinator, _, calls, audits) = fixture(false, 0);
    let outcome = coordinator.execute(
        &mut machine(&invocation),
        &invocation,
        ExecutionControl {
            now_tick: 1,
            cancellation_requested: false,
        },
    );
    assert!(matches!(outcome, ExecutionOutcome::ProviderFailure(p) if p.has_unknown_outcome()));
    assert_eq!(calls.lock().unwrap().len(), 1);
    assert!(audits.records.lock().unwrap().is_empty());
}

#[test]
fn reference_machine_local_missing_host_remains_provider_failure() {
    let invocation = invocation();
    let outcome = ExecutionCoordinator::local(CoordinatorLimits::default()).execute(
        &mut machine(&invocation),
        &invocation,
        ExecutionControl {
            now_tick: 1,
            cancellation_requested: false,
        },
    );
    assert!(
        matches!(outcome, ExecutionOutcome::ProviderFailure(p) if p.category == FailureCategory::ProviderFailure)
    );
}
