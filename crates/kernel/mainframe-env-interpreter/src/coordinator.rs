use mainframe_env_diagnostics::{
    DiagnosticCode, DiagnosticLimits, ExecutionProblem, FailureCategory, Phase,
};
use mainframe_env_execution_api::{
    ExecutionOutcome, Invocation, Machine, MachineDrive, MachineResume, Quantum,
};
use mainframe_env_host_api::{EffectRequest, EffectResult, ScopedHostService};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CoordinatorLimits {
    pub quantum: Quantum,
    pub max_quanta: u64,
}

impl Default for CoordinatorLimits {
    fn default() -> Self {
        Self {
            quantum: Quantum::new(10_000, 16 * 1024 * 1024).expect("non-zero quantum"),
            max_quanta: 100_000,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ExecutionControl {
    pub now_tick: u64,
    pub cancellation_requested: bool,
}

/// The one 0.1 machine-driving authority used by CLI, batch, and server paths.
///
/// Persistence hooks are layered onto this coordinator by the durable shell;
/// this type owns drive ordering, cancellation/deadline observation, and typed
/// host-result resumption.
pub struct ExecutionCoordinator {
    host: Option<Arc<ScopedHostService>>,
    limits: CoordinatorLimits,
}

impl ExecutionCoordinator {
    #[must_use]
    pub fn local(limits: CoordinatorLimits) -> Self {
        Self { host: None, limits }
    }

    #[must_use]
    pub fn with_host(host: Arc<ScopedHostService>, limits: CoordinatorLimits) -> Self {
        Self {
            host: Some(host),
            limits,
        }
    }

    pub fn execute<M>(
        &self,
        machine: &mut M,
        invocation: &Invocation,
        control: ExecutionControl,
    ) -> ExecutionOutcome
    where
        M: Machine<Effect = EffectRequest, EffectResult = EffectResult>,
    {
        let mut resume = if control.cancellation_requested {
            MachineResume::Cancelled
        } else if control.now_tick >= invocation.deadline_tick {
            MachineResume::TimedOut
        } else {
            MachineResume::Start
        };
        for _ in 0..self.limits.max_quanta {
            match machine.drive(resume, self.limits.quantum) {
                MachineDrive::Continue => resume = MachineResume::Start,
                MachineDrive::HostCall(effect) => {
                    let Some(host) = &self.host else {
                        return ExecutionOutcome::ProviderFailure(problem(
                            FailureCategory::ProviderFailure,
                            "selected execution profile has no host provider",
                        ));
                    };
                    resume = MachineResume::HostResult(
                        host.invoke(
                            invocation,
                            control.now_tick,
                            control.cancellation_requested,
                            effect,
                        )
                        .effect,
                    );
                }
                MachineDrive::Invoke(child) => return ExecutionOutcome::Invoke(child),
                MachineDrive::Transfer(transfer) => return ExecutionOutcome::Transfer(transfer),
                MachineDrive::Suspended(suspension) => {
                    return ExecutionOutcome::Suspended(suspension);
                }
                MachineDrive::Completed(completion) => {
                    return ExecutionOutcome::Completed(completion);
                }
                MachineDrive::Condition(condition) => {
                    return ExecutionOutcome::Condition(condition);
                }
                MachineDrive::Abend(abend) => return ExecutionOutcome::Abend(abend),
                MachineDrive::Failed(problem) => return failed_outcome(problem),
            }
        }
        ExecutionOutcome::ResourceExhausted(problem(
            FailureCategory::ResourceExhausted,
            "execution quantum limit exhausted",
        ))
    }
}

fn failed_outcome(problem: ExecutionProblem) -> ExecutionOutcome {
    match problem.category {
        FailureCategory::Cancelled => ExecutionOutcome::Cancelled,
        FailureCategory::TimedOut => ExecutionOutcome::TimedOut,
        FailureCategory::ResourceExhausted => ExecutionOutcome::ResourceExhausted(problem),
        FailureCategory::ProviderFailure | FailureCategory::UnknownOutcome => {
            ExecutionOutcome::ProviderFailure(problem)
        }
        FailureCategory::InfrastructureFailure => ExecutionOutcome::InfrastructureFailure(problem),
        _ => ExecutionOutcome::Rejected(problem),
    }
}

fn problem(category: FailureCategory, message: &str) -> ExecutionProblem {
    ExecutionProblem::new(
        DiagnosticCode::new("MECOORD0001").expect("static diagnostic code"),
        category,
        Phase::Execute,
        message,
        false,
        category == FailureCategory::UnknownOutcome,
        DiagnosticLimits::default(),
    )
    .expect("bounded static execution problem")
}

#[cfg(test)]
mod tests {
    use super::*;
    use mainframe_env_execution_api::{
        ArtifactRef, Completion, ExecutionId, IdempotencyKey, InvocationLimits, Principal,
        PrincipalId, RequestId, ResourceLimits, RunUnitId, Selector, ServiceClass, TraceId,
    };
    use std::collections::{BTreeMap, BTreeSet};

    struct CompleteMachine;
    impl Machine for CompleteMachine {
        type Effect = EffectRequest;
        type EffectResult = EffectResult;

        fn drive(
            &mut self,
            resume: MachineResume<Self::EffectResult>,
            _: Quantum,
        ) -> MachineDrive<Self::Effect> {
            match resume {
                MachineResume::Cancelled => MachineDrive::Failed(problem(
                    FailureCategory::Cancelled,
                    "cancelled before dispatch",
                )),
                _ => MachineDrive::Completed(Completion {
                    return_code: 0,
                    output: mainframe_env_execution_api::BoundedPayload::new(
                        "test@1",
                        b"OK".to_vec(),
                        InvocationLimits::default(),
                    )
                    .unwrap(),
                }),
            }
        }
    }

    fn invocation() -> Invocation {
        let limits = InvocationLimits::default();
        Invocation::new(
            RequestId::new("request", limits).unwrap(),
            ExecutionId::new("execution", limits).unwrap(),
            RunUnitId::new("run", limits).unwrap(),
            None,
            Selector::new("test", limits).unwrap(),
            ArtifactRef::new("artifact", limits).unwrap(),
            Principal::new(
                PrincipalId::new("USER", limits).unwrap(),
                BTreeSet::new(),
                limits,
            )
            .unwrap(),
            ServiceClass::Interactive,
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

    #[test]
    fn completion_and_cancellation_use_one_driver() {
        let coordinator = ExecutionCoordinator::local(CoordinatorLimits::default());
        assert!(matches!(
            coordinator.execute(
                &mut CompleteMachine,
                &invocation(),
                ExecutionControl::default()
            ),
            ExecutionOutcome::Completed(_)
        ));
        assert_eq!(
            coordinator.execute(
                &mut CompleteMachine,
                &invocation(),
                ExecutionControl {
                    cancellation_requested: true,
                    ..ExecutionControl::default()
                }
            ),
            ExecutionOutcome::Cancelled
        );
    }
}
