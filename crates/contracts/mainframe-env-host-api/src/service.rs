use crate::{AuditEvent, EffectRequest, EffectResult, HostLimits, HostProblem, RegistrySnapshot};
use mainframe_env_execution_api::Invocation;
use std::collections::BTreeMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuditedEffectResult {
    pub effect: EffectResult,
    pub audit: AuditEvent,
}

pub struct ScopedHostService {
    registry: Arc<RegistrySnapshot>,
    limits: HostLimits,
}

impl ScopedHostService {
    #[must_use]
    pub fn new(registry: Arc<RegistrySnapshot>, limits: HostLimits) -> Self {
        Self { registry, limits }
    }

    pub fn invoke(
        &self,
        invocation: &Invocation,
        now_tick: u64,
        cancellation_requested: bool,
        request: EffectRequest,
    ) -> AuditedEffectResult {
        let capability = request
            .request
            .required_capability(mainframe_env_execution_api::InvocationLimits::default());
        let sequence = request.sequence;
        let result = if request.run_unit != invocation.run_unit_id {
            Err(HostProblem::Malformed)
        } else if cancellation_requested || invocation.cancellation.is_some() {
            Err(HostProblem::Cancelled)
        } else if now_tick >= invocation.deadline_tick || now_tick >= request.deadline_tick {
            Err(HostProblem::TimedOut)
        } else if !invocation.principal.has_grant(&capability) {
            Err(HostProblem::Unauthorized)
        } else if let Err(problem) = request.validate(self.limits) {
            Err(problem)
        } else {
            match self.registry.select(&capability) {
                Ok(provider)
                    if invocation
                        .provider_generations
                        .get(&capability)
                        .is_some_and(|required| required != &provider.descriptor().generation) =>
                {
                    Err(HostProblem::ProviderFailure)
                }
                Ok(provider)
                    if format!("{:?}", request.request).len()
                        > provider.descriptor().max_request_bytes =>
                {
                    Err(HostProblem::ResourceExhausted)
                }
                Ok(provider) => {
                    match catch_unwind(AssertUnwindSafe(|| provider.invoke(invocation, request))) {
                        Ok(effect) => effect.validate(sequence, self.limits).and_then(|()| {
                            if format!("{:?}", effect.outcome).len()
                                > provider.descriptor().max_result_bytes
                            {
                                Err(HostProblem::ResourceExhausted)
                            } else {
                                effect.outcome
                            }
                        }),
                        Err(_) => Err(HostProblem::InfrastructureFailure),
                    }
                }
                Err(problem) => Err(problem),
            }
        };
        let decision = match &result {
            Ok(_) => "success",
            Err(HostProblem::Unauthorized) => "deny",
            Err(HostProblem::Cancelled) => "cancelled",
            Err(HostProblem::TimedOut) => "timed-out",
            Err(_) => "failure",
        };
        AuditedEffectResult {
            effect: EffectResult {
                sequence,
                outcome: result,
            },
            audit: AuditEvent {
                action: capability.as_str().to_string(),
                resource_hash: "redacted-at-contract-boundary".to_string(),
                decision: decision.to_string(),
                fields: BTreeMap::from([
                    (
                        "execution".to_string(),
                        invocation.execution_id.as_str().to_string(),
                    ),
                    (
                        "audit_correlation".to_string(),
                        invocation.audit_correlation.clone(),
                    ),
                    (
                        "run_unit".to_string(),
                        invocation.run_unit_id.as_str().to_string(),
                    ),
                    ("sequence".to_string(), sequence.to_string()),
                ]),
            },
        }
    }

    #[must_use]
    pub fn capability_ready(&self, capability: &str) -> bool {
        mainframe_env_execution_api::CapabilityId::new(
            capability,
            mainframe_env_execution_api::InvocationLimits::default(),
        )
        .is_ok_and(|capability| self.registry.select(&capability).is_ok())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CapabilityDescriptor, HostProvider, HostRequest, RegistrySnapshot, StateRequest};
    use mainframe_env_execution_api::{
        ArtifactRef, CapabilityId, ExecutionId, IdempotencyKey, InvocationLimits, Principal,
        PrincipalId, RequestId, ResourceLimits, RunUnitId, Selector, ServiceClass, TraceId,
    };
    use std::collections::{BTreeMap, BTreeSet};

    struct PanicProvider {
        descriptor: CapabilityDescriptor,
    }
    impl HostProvider for PanicProvider {
        fn descriptor(&self) -> &CapabilityDescriptor {
            &self.descriptor
        }
        fn invoke(&self, _: &Invocation, _: EffectRequest) -> EffectResult {
            panic!("provider panic")
        }
    }
    fn invocation(granted: bool) -> Invocation {
        let l = InvocationLimits::default();
        let capability = CapabilityId::new("host.state.read", l).unwrap();
        Invocation::new(
            RequestId::new("request", l).unwrap(),
            ExecutionId::new("execution", l).unwrap(),
            RunUnitId::new("run", l).unwrap(),
            None,
            Selector::new("test", l).unwrap(),
            ArtifactRef::new("artifact", l).unwrap(),
            Principal::new(
                PrincipalId::new("IBMUSER", l).unwrap(),
                if granted {
                    BTreeSet::from([capability])
                } else {
                    BTreeSet::new()
                },
                l,
            )
            .unwrap(),
            ServiceClass::System,
            0,
            100,
            TraceId::new("trace", l).unwrap(),
            IdempotencyKey::new("idem", l).unwrap(),
            1,
            ResourceLimits::default(),
            BTreeMap::new(),
            l,
        )
        .unwrap()
    }
    fn request(run: &RunUnitId) -> EffectRequest {
        EffectRequest {
            run_unit: run.clone(),
            sequence: 1,
            deadline_tick: 100,
            idempotency_key: None,
            request: HostRequest::State(StateRequest::Get { key: "one".into() }),
        }
    }
    fn service() -> ScopedHostService {
        let l = InvocationLimits::default();
        let descriptor = CapabilityDescriptor {
            capability: CapabilityId::new("host.state.read", l).unwrap(),
            provider_id: "panic".into(),
            generation: "1".into(),
            request_schema: "request@1".into(),
            result_schema: "result@1".into(),
            max_request_bytes: 1024,
            max_result_bytes: 1024,
            ready: true,
        };
        let provider: Arc<dyn HostProvider> = Arc::new(PanicProvider { descriptor });
        ScopedHostService::new(
            Arc::new(RegistrySnapshot::new(1, vec![provider], l).unwrap()),
            HostLimits::default(),
        )
    }
    #[test]
    fn denial_never_invokes_provider() {
        let invocation = invocation(false);
        let result = service().invoke(&invocation, 1, false, request(&invocation.run_unit_id));
        assert_eq!(result.effect.outcome, Err(HostProblem::Unauthorized));
        assert_eq!(result.audit.decision, "deny");
    }
    #[test]
    fn panic_is_contained_as_infrastructure_failure() {
        let invocation = invocation(true);
        let result = service().invoke(&invocation, 1, false, request(&invocation.run_unit_id));
        assert_eq!(
            result.effect.outcome,
            Err(HostProblem::InfrastructureFailure)
        );
    }
    #[test]
    fn identity_cancellation_and_deadline_fail_before_provider() {
        let invocation = invocation(true);
        let cancelled = service().invoke(&invocation, 1, true, request(&invocation.run_unit_id));
        assert_eq!(cancelled.effect.outcome, Err(HostProblem::Cancelled));
        let timed_out = service().invoke(
            &invocation,
            invocation.deadline_tick,
            false,
            request(&invocation.run_unit_id),
        );
        assert_eq!(timed_out.effect.outcome, Err(HostProblem::TimedOut));
        let other_run = RunUnitId::new("other", InvocationLimits::default()).unwrap();
        let malformed = service().invoke(&invocation, 1, false, request(&other_run));
        assert_eq!(malformed.effect.outcome, Err(HostProblem::Malformed));
    }
    #[test]
    fn missing_provider_fails_closed() {
        let invocation = invocation(true);
        let empty = ScopedHostService::new(
            Arc::new(RegistrySnapshot::new(1, Vec::new(), InvocationLimits::default()).unwrap()),
            HostLimits::default(),
        );
        assert_eq!(
            empty
                .invoke(&invocation, 1, false, request(&invocation.run_unit_id))
                .effect
                .outcome,
            Err(HostProblem::Unsupported)
        );
    }

    #[test]
    fn provider_generation_and_durable_cancellation_are_rechecked() {
        let limits = InvocationLimits::default();
        let capability = CapabilityId::new("host.state.read", limits).unwrap();
        let generation_mismatch = invocation(true)
            .with_provider_generations(BTreeMap::from([(capability, "stale".into())]), limits)
            .unwrap();
        assert_eq!(
            service()
                .invoke(
                    &generation_mismatch,
                    1,
                    false,
                    request(&generation_mismatch.run_unit_id)
                )
                .effect
                .outcome,
            Err(HostProblem::ProviderFailure)
        );

        let cancelled = invocation(true).with_cancellation(
            mainframe_env_execution_api::Cancellation::new(
                mainframe_env_execution_api::CancellationId::new("cancel", limits).unwrap(),
                "operator request",
                1,
                limits,
            )
            .unwrap(),
        );
        assert_eq!(
            service()
                .invoke(&cancelled, 1, false, request(&cancelled.run_unit_id))
                .effect
                .outcome,
            Err(HostProblem::Cancelled)
        );
    }
}
