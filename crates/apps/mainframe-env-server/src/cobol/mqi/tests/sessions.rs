//! Raw observation/once containment tests, not MQ lifecycle acceptance.
use super::*;
use mainframe_env_execution_api::{
    Abend, AbendDumpDisposition, ChildInvocation, Completion, Condition, Machine, MachineDrive,
    MachineResume, Quantum, Suspension, Transfer,
};

struct Stop;
impl Machine for Stop {
    type Effect = EffectRequest;
    type EffectResult = EffectResult;
    fn drive(&mut self, _: MachineResume<EffectResult>, _: Quantum) -> MachineDrive<EffectRequest> {
        MachineDrive::Completed(Completion {
            return_code: 0,
            output: payload(),
        })
    }
}
fn payload() -> BoundedPayload {
    BoundedPayload::new("test@1", vec![], InvocationLimits::default()).unwrap()
}
fn outcomes() -> Vec<ExecutionOutcome> {
    let invocation = super::super::super::hardening::parent();
    let ExecutionOutcome::InfrastructureFailure(problem) = ExecutionCoordinator::local(
        Default::default(),
    )
    .execute_with_control(&mut Stop, &invocation, || {
        Err(ExecutionControlError::Unavailable)
    }) else {
        panic!("actual unavailable-control outcome")
    };
    let mut unknown = problem.clone();
    unknown.unknown_outcome = true;
    vec![
        ExecutionOutcome::Completed(Completion {
            return_code: 7,
            output: payload(),
        }),
        ExecutionOutcome::Condition(Condition {
            name: "condition".into(),
            response: 1,
            response2: 2,
            handled: true,
        }),
        ExecutionOutcome::Abend(Abend {
            code: "U0001".into(),
            reason: None,
            dump: AbendDumpDisposition::Unspecified,
        }),
        ExecutionOutcome::Cancelled,
        ExecutionOutcome::TimedOut,
        ExecutionOutcome::ResourceExhausted(problem.clone()),
        ExecutionOutcome::ProviderFailure(problem.clone()),
        ExecutionOutcome::ProviderFailure(unknown),
        ExecutionOutcome::InfrastructureFailure(problem.clone()),
        ExecutionOutcome::Rejected(problem),
        ExecutionOutcome::Suspended(Suspension {
            kind: "held".into(),
            resume_token: "token".into(),
            state_bytes: 2,
        }),
        ExecutionOutcome::Invoke(ChildInvocation {
            selector: invocation.selector.clone(),
            artifact: invocation.artifact.clone(),
            payload: payload(),
        }),
        ExecutionOutcome::Transfer(Transfer {
            selector: invocation.selector.clone(),
            payload: payload(),
            replace_frame: true,
        }),
    ]
}

struct ObserveSession {
    events: Arc<Mutex<Vec<ExecutionOutcome>>>,
    aborts: Arc<Mutex<Vec<HostProblem>>>,
    invocation: Invocation,
    fail: bool,
    panic: bool,
    store: Arc<dyn PlatformStore>,
    control: Arc<dyn ProgramExecutionControl>,
}
impl InstalledMqFrameSession for ObserveSession {
    fn store(&self) -> &Arc<dyn PlatformStore> {
        &self.store
    }
    fn execution_control(&self) -> &Arc<dyn ProgramExecutionControl> {
        &self.control
    }
    fn program_frame(&self) -> Result<Arc<dyn MqMqiProgramFrame>, HostProblem> {
        Ok(Arc::new(Frame(self.invocation.clone())))
    }
    fn abort_preparation(&mut self, problem: &HostProblem) -> Result<(), HostProblem> {
        self.aborts.lock().unwrap().push(problem.clone());
        if self.panic {
            panic!("abort panic");
        }
        if self.fail {
            Err(HostProblem::InfrastructureFailure)
        } else {
            Ok(())
        }
    }
    fn finish(&mut self, raw: &ExecutionOutcome) -> Result<(), HostProblem> {
        self.events.lock().unwrap().push(raw.clone());
        if self.panic {
            panic!("finish panic");
        }
        if self.fail {
            Err(HostProblem::ProviderFailure)
        } else {
            Ok(())
        }
    }
}
fn guard(
    fail: bool,
    panic: bool,
) -> (
    SessionGuard,
    Arc<Mutex<Vec<ExecutionOutcome>>>,
    Arc<Mutex<Vec<HostProblem>>>,
) {
    let events = Arc::new(Mutex::new(vec![]));
    let aborts = Arc::new(Mutex::new(vec![]));
    (
        SessionGuard::new(Box::new(ObserveSession {
            events: events.clone(),
            aborts: aborts.clone(),
            invocation: super::super::super::hardening::parent(),
            fail,
            panic,
            store: Arc::new(MemoryStore::new(Default::default())),
            control: super::setup::control(),
        })),
        events,
        aborts,
    )
}

#[test]
fn every_raw_category_is_preserved_once_before_transport_invalidation() {
    for raw in outcomes() {
        let (mut guard, events, aborts) = guard(false, false);
        let invocation = super::super::super::hardening::parent();
        let frame = guard.frame(&invocation).unwrap();
        assert!(frame.profile(&invocation).is_ok());
        assert_eq!(guard.finish(&raw), Ok(()));
        assert_eq!(events.lock().unwrap().as_slice(), &[raw.clone()]);
        assert_eq!(frame.profile(&invocation), Err(HostProblem::Unauthorized));
        assert_eq!(guard.finish(&raw), Err(HostProblem::UnknownOutcome));
        assert_eq!(
            guard.abort(HostProblem::Malformed),
            HostProblem::UnknownOutcome
        );
        assert_eq!(events.lock().unwrap().len(), 1);
        assert!(aborts.lock().unwrap().is_empty());
    }
}

#[test]
fn abort_finish_failure_and_panic_never_become_known_success_or_retry() {
    for (fail, panic) in [(true, false), (false, true)] {
        for raw in outcomes() {
            let (mut guard, events, aborts) = guard(fail, panic);
            assert_eq!(guard.finish(&raw), Err(HostProblem::UnknownOutcome));
            assert_eq!(guard.finish(&raw), Err(HostProblem::UnknownOutcome));
            assert_eq!(events.lock().unwrap().len(), 1);
            assert!(aborts.lock().unwrap().is_empty());
        }
        let (mut guard, events, aborts) = guard(fail, panic);
        assert_eq!(
            guard.abort(HostProblem::Malformed),
            HostProblem::UnknownOutcome
        );
        assert_eq!(
            guard.abort(HostProblem::Malformed),
            HostProblem::UnknownOutcome
        );
        assert_eq!(aborts.lock().unwrap().as_slice(), &[HostProblem::Malformed]);
        assert!(events.lock().unwrap().is_empty());
    }
}

#[test]
fn no_dispatch_abort_and_drop_only_disable_bounded_transport() {
    for abort in [false, true] {
        let (mut guard, events, aborts) = guard(false, false);
        let invocation = super::super::super::hardening::parent();
        let frame = guard.frame(&invocation).unwrap();
        if abort {
            assert_eq!(
                guard.abort(HostProblem::UnknownOutcome),
                HostProblem::UnknownOutcome
            );
        }
        drop(guard);
        assert_eq!(frame.profile(&invocation), Err(HostProblem::Unauthorized));
        assert!(events.lock().unwrap().is_empty());
        assert_eq!(aborts.lock().unwrap().len(), usize::from(abort));
    }
}
