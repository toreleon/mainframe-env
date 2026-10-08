use crate::{
    EffectRequest, EffectResult, HostLimits, HostProblem, HostResult, MAX_CANONICAL_EFFECT_BYTES,
    RegistrySnapshot, SecurityDecision, canonical_audit_resource_digest, canonical_request_size,
    canonical_result_size,
};
use mainframe_env_execution_api::CapabilityId;
use mainframe_env_execution_api::{AuditDecision, AuditRecord, Invocation};
use std::collections::BTreeMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;

#[must_use = "host effects must not be consumed without their typed audit record"]
#[derive(Clone, Debug, Eq, PartialEq)]
/// A host effect paired with the mandatory durable security audit decision.
pub struct AuditedEffectResult {
    effect: EffectResult,
    audit: AuditRecord,
    mutation_dispatched: bool,
}

impl AuditedEffectResult {
    /// Persist the typed audit record before exposing the effect to a non-transactional caller.
    /// A sink failure after dispatching a mutation is conservatively reported as unknown.
    pub fn persist_with(
        self,
        persist: impl FnOnce(AuditRecord) -> Result<(), HostProblem>,
    ) -> EffectResult {
        match persist(self.audit) {
            Ok(()) => self.effect,
            Err(_) if self.mutation_dispatched => EffectResult {
                sequence: self.effect.sequence,
                outcome: Err(HostProblem::UnknownOutcome),
            },
            Err(problem) => EffectResult {
                sequence: self.effect.sequence,
                outcome: Err(problem),
            },
        }
    }

    /// Transfer both records to a coordinator that commits them in one transaction.
    #[must_use]
    pub fn into_transaction_parts(self) -> (EffectResult, AuditRecord) {
        (self.effect, self.audit)
    }
}

/// Invocation-scoped dispatch through an immutable provider snapshot.
/// Enforces grants, replay metadata, deadlines, exact generations and canonical byte budgets;
/// returns an effect together with its mandatory audit record.
pub struct ScopedHostService {
    registry: Arc<RegistrySnapshot>,
    limits: HostLimits,
}

impl ScopedHostService {
    #[must_use]
    /// Retain a shared provider snapshot and fixed host validation limits for dispatch.
    pub fn new(registry: Arc<RegistrySnapshot>, limits: HostLimits) -> Self {
        Self { registry, limits }
    }

    /// Observe whether the ready provider selected by this frozen registry is
    /// the same physical `Arc` allocation retained by a trusted embedding.
    /// Equal descriptors, generation strings or stores are not object identity.
    /// Missing/not-ready selection preserves the registry's ordinary refusal.
    /// This read-only check performs no dispatch and grants no invocation,
    /// capability, lifecycle, SAF, effect or transaction authority; `invoke`
    /// still enforces its complete independent admission protocol.
    pub fn selects_same_provider(
        &self,
        capability: &CapabilityId,
        expected: &Arc<dyn crate::HostProvider>,
    ) -> Result<bool, HostProblem> {
        self.registry
            .select(capability)
            .map(|selected| Arc::ptr_eq(&selected, expected))
    }

    /// Preflight exact provider-generation requirements before restoring any
    /// state or dispatching the first effect.
    pub fn validate_provider_generations(
        &self,
        generations: &BTreeMap<CapabilityId, String>,
    ) -> Result<(), HostProblem> {
        for (capability, generation) in generations {
            let provider = self.registry.select(capability)?;
            if provider.descriptor().generation != *generation {
                return Err(HostProblem::ProviderFailure);
            }
        }
        Ok(())
    }

    /// Consume one invocation-bound request at the supplied logical tick.
    /// Reject cancellation, expired deadlines or missing grants before provider dispatch.
    /// Validate reply sequence and budgets, preserving unknown outcomes and unusable successful
    /// mutation replies as uncertainty. The returned audit record must be persisted before
    /// consumption.
    pub fn invoke(
        &self,
        invocation: &Invocation,
        now_tick: u64,
        cancellation_requested: bool,
        request: EffectRequest,
    ) -> AuditedEffectResult {
        self.invoke_inner(
            invocation,
            now_tick,
            cancellation_requested,
            request,
            None,
            None,
        )
    }

    /// Dispatch only an original Program request with borrowed Rust context.
    /// Shares every ordinary scoped admission, result and audit check. `Any`
    /// grants no permission: a future closed receiver must validate its genuine
    /// owner; providers default to Unsupported without ordinary dispatch.
    pub fn invoke_program_context(
        &self,
        invocation: &Invocation,
        now_tick: u64,
        cancellation_requested: bool,
        request: EffectRequest,
        context: &(dyn std::any::Any + Send + Sync),
    ) -> AuditedEffectResult {
        self.invoke_inner(
            invocation,
            now_tick,
            cancellation_requested,
            request,
            Some(context),
            None,
        )
    }

    /// Invoke ONLY replay transport under the ordinary scoped host checks.
    /// The local-type wrapper checks zero/duplicates/capacity/char0 representation,
    /// not object origin, INQUIRE access or permission. Successful output must
    /// match that wrapper and the original full canonical result digest. Any
    /// changed reply becomes actual Unknown before this method creates its audit.
    /// No provider activation or ordinary invoke fallback is performed here.
    #[allow(clippy::too_many_arguments)]
    pub fn replay_retained(
        &self,
        invocation: &Invocation,
        now_tick: u64,
        cancellation_requested: bool,
        request: EffectRequest,
        expected_result_digest: [u8; 32],
        context: &(dyn std::any::Any + Send + Sync),
    ) -> AuditedEffectResult {
        self.invoke_inner(
            invocation,
            now_tick,
            cancellation_requested,
            request,
            None,
            Some((expected_result_digest, context)),
        )
    }

    fn invoke_inner(
        &self,
        invocation: &Invocation,
        now_tick: u64,
        cancellation_requested: bool,
        request: EffectRequest,
        context: Option<&(dyn std::any::Any + Send + Sync)>,
        replay: Option<([u8; 32], &(dyn std::any::Any + Send + Sync))>,
    ) -> AuditedEffectResult {
        let capability = request
            .request
            .required_capability(mainframe_env_execution_api::InvocationLimits::default());
        let resource = canonical_audit_resource_digest(&request.request);
        let sequence = request.sequence;
        let mut mutation_dispatched = false;
        let checked_limits = match &request.request {
            crate::HostRequest::MqMqi(host) if replay.is_some() => Some(host.envelope.limits),
            _ => None,
        };
        let checked_inquiry = replay.and_then(|_| match &request.request {
            crate::HostRequest::MqMqi(host) => match &host.envelope.request {
                crate::mq_mqi::MqMqiRequest::Inquire(inquiry) if inquiry.selectors.len() <= 256 => {
                    crate::mq_mqi::MqMqiLocalTypeInquiry::from_inquiry(
                        inquiry.clone(),
                        host.envelope.limits,
                    )
                    .ok()
                }
                _ => None,
            },
            _ => None,
        });
        let result = if (replay.is_some() && checked_inquiry.is_none())
            || (context.is_some() && !matches!(&request.request, crate::HostRequest::Program(_)))
        {
            Err(HostProblem::Unsupported)
        } else if request.run_unit != invocation.run_unit_id {
            Err(HostProblem::Malformed)
        } else if cancellation_requested || invocation.cancellation_requested() {
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
                    if canonical_request_size(
                        &request.request,
                        provider
                            .descriptor()
                            .max_request_bytes
                            .min(MAX_CANONICAL_EFFECT_BYTES),
                    )
                    .is_err() =>
                {
                    Err(HostProblem::ResourceExhausted)
                }
                Ok(provider) => {
                    let mutating = request.request.is_mutating();
                    mutation_dispatched = mutating && replay.is_none();
                    match catch_unwind(AssertUnwindSafe(|| match (context, replay) {
                        (_, Some((digest, context))) => {
                            provider.replay_retained(invocation, request, digest, now_tick, context)
                        }
                        (Some(context), None) => {
                            provider.invoke_program_context(invocation, request, context)
                        }
                        (None, None) => provider.invoke(invocation, request),
                    })) {
                        // Uncertainty is a control outcome, not an oversized success payload.
                        // Never erase it, even when an untrusted provider also corrupts the envelope.
                        Ok(effect)
                            if matches!(&effect.outcome, Err(HostProblem::UnknownOutcome)) =>
                        {
                            Err(HostProblem::UnknownOutcome)
                        }
                        Ok(effect) => {
                            let validation =
                                effect.validate(sequence, self.limits).and_then(|()| {
                                    canonical_result_size(
                                        &effect.outcome,
                                        provider
                                            .descriptor()
                                            .max_result_bytes
                                            .min(MAX_CANONICAL_EFFECT_BYTES),
                                    )
                                    .map(|_| ())
                                });
                            match validation {
                                Ok(()) if replay.is_some() && effect.outcome.is_ok() => {
                                    let exact = match (&effect.outcome, &checked_inquiry) {
                                        (Ok(crate::HostResult::MqMqi(host)), Some(profile)) => {
                                            Some(host.limits) == checked_limits
                                                && profile
                                                    .validate_result(&host.result, host.limits)
                                                    .is_ok()
                                        }
                                        _ => false,
                                    };
                                    if exact
                                        && crate::canonical_result_digest(&effect.outcome).ok()
                                            == replay.map(|r| r.0)
                                    {
                                        effect.outcome
                                    } else {
                                        Err(HostProblem::UnknownOutcome)
                                    }
                                }
                                Ok(()) => effect.outcome,
                                // An unusable replay envelope cannot attest a
                                // known refusal of this original occurrence.
                                Err(_) if replay.is_some() => Err(HostProblem::UnknownOutcome),
                                // The provider has reported a committed success. Losing its
                                // usable reply is not a known rejection that permits retry.
                                Err(_) if mutating && effect.outcome.is_ok() => {
                                    Err(HostProblem::UnknownOutcome)
                                }
                                Err(problem) => Err(problem),
                            }
                        }
                        Err(_) => Err(HostProblem::InfrastructureFailure),
                    }
                }
                Err(problem) => Err(problem),
            }
        };
        let decision = match &result {
            Ok(HostResult::Security(SecurityDecision::Deny)) => AuditDecision::Deny,
            Ok(_) => AuditDecision::Success,
            Err(HostProblem::Unauthorized) => AuditDecision::Deny,
            Err(HostProblem::Cancelled) => AuditDecision::Cancelled,
            Err(HostProblem::TimedOut) => AuditDecision::TimedOut,
            Err(HostProblem::ProviderFailure) => AuditDecision::ProviderFailure,
            Err(HostProblem::InfrastructureFailure) => AuditDecision::InfrastructureFailure,
            Err(HostProblem::UnknownOutcome) => AuditDecision::UnknownOutcome,
            Err(_) => AuditDecision::Rejected,
        };
        AuditedEffectResult {
            effect: EffectResult {
                sequence,
                outcome: result,
            },
            audit: AuditRecord {
                execution_id: invocation.execution_id.clone(),
                run_unit_id: invocation.run_unit_id.clone(),
                attempt: invocation.attempt,
                effect_sequence: sequence,
                observed_tick: now_tick,
                principal: invocation.principal.id().clone(),
                invocation_key: invocation.idempotency_key.clone(),
                capability,
                resource,
                decision,
            },
            mutation_dispatched,
        }
    }

    #[must_use]
    /// Whether an identity is valid and selects an installed ready provider.
    /// Does not check invocation grants, exact required generations or resource-level authority.
    pub fn capability_ready(&self, capability: &str) -> bool {
        mainframe_env_execution_api::CapabilityId::new(
            capability,
            mainframe_env_execution_api::InvocationLimits::default(),
        )
        .is_ok_and(|capability| self.registry.select(&capability).is_ok())
    }
}

#[cfg(test)]
impl AuditedEffectResult {
    fn effect(&self) -> &EffectResult {
        &self.effect
    }

    fn audit(&self) -> &AuditRecord {
        &self.audit
    }
}

#[cfg(test)]
mod tests {
    mod program_context;
    mod retained_replay;
    use super::*;
    use crate::{CapabilityDescriptor, HostProvider, HostRequest, RegistrySnapshot, StateRequest};
    use mainframe_env_execution_api::{
        ArtifactRef, CancellationProbe, CapabilityId, ExecutionId, IdempotencyKey,
        InvocationLimits, Principal, PrincipalId, RequestId, ResourceLimits, RunUnitId, Selector,
        ServiceClass, TraceId,
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
        assert_eq!(result.effect().outcome, Err(HostProblem::Unauthorized));
        assert_eq!(result.audit().decision, AuditDecision::Deny);
    }
    #[test]
    fn panic_is_contained_as_infrastructure_failure() {
        let invocation = invocation(true);
        let result = service().invoke(&invocation, 1, false, request(&invocation.run_unit_id));
        assert_eq!(
            result.effect().outcome,
            Err(HostProblem::InfrastructureFailure)
        );
        assert_eq!(
            result.audit().decision,
            AuditDecision::InfrastructureFailure
        );
    }
    #[test]
    fn identity_cancellation_and_deadline_fail_before_provider() {
        let active = invocation(true);
        let cancelled = service().invoke(&active, 1, true, request(&active.run_unit_id));
        assert_eq!(cancelled.effect().outcome, Err(HostProblem::Cancelled));
        assert_eq!(cancelled.audit().decision, AuditDecision::Cancelled);
        let probe = CancellationProbe::new();
        let live = invocation(true).with_cancellation_probe(probe.clone());
        probe.request();
        let cancelled = service().invoke(&live, 1, false, request(&live.run_unit_id));
        assert_eq!(cancelled.effect().outcome, Err(HostProblem::Cancelled));
        let timed_out = service().invoke(
            &active,
            active.deadline_tick,
            false,
            request(&active.run_unit_id),
        );
        assert_eq!(timed_out.effect().outcome, Err(HostProblem::TimedOut));
        assert_eq!(timed_out.audit().decision, AuditDecision::TimedOut);
        let other_run = RunUnitId::new("other", InvocationLimits::default()).unwrap();
        let malformed = service().invoke(&active, 1, false, request(&other_run));
        assert_eq!(malformed.effect().outcome, Err(HostProblem::Malformed));
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
                .effect()
                .outcome,
            Err(HostProblem::Unsupported)
        );
    }

    fn identity_provider(ready: bool) -> Arc<dyn HostProvider> {
        Arc::new(PanicProvider {
            descriptor: CapabilityDescriptor {
                capability: CapabilityId::new("host.state.read", InvocationLimits::default())
                    .unwrap(),
                provider_id: "identity".into(),
                generation: "same-generation".into(),
                request_schema: "request@1".into(),
                result_schema: "result@1".into(),
                max_request_bytes: 1024,
                max_result_bytes: 1024,
                ready,
            },
        })
    }
    fn identity_service(provider: Arc<dyn HostProvider>) -> ScopedHostService {
        ScopedHostService::new(
            Arc::new(
                RegistrySnapshot::new(1, vec![provider], InvocationLimits::default()).unwrap(),
            ),
            HostLimits::default(),
        )
    }

    #[test]
    fn physical_provider_observation_accepts_only_the_same_allocation_without_dispatch() {
        let expected = identity_provider(true);
        let other = identity_provider(true);
        assert_eq!(expected.descriptor(), other.descriptor());
        let host = identity_service(expected.clone());
        let capability = expected.descriptor().capability.clone();
        assert_eq!(
            host.selects_same_provider(&capability, &expected.clone()),
            Ok(true)
        );
        assert_eq!(host.selects_same_provider(&capability, &other), Ok(false));
        // PanicProvider::invoke would panic. The identity check above cannot
        // dispatch, and even a true observation cannot bypass principal grants.
        let denied = invocation(false);
        assert_eq!(
            host.invoke(&denied, 1, false, request(&denied.run_unit_id))
                .effect()
                .outcome,
            Err(HostProblem::Unauthorized)
        );
    }

    #[test]
    fn physical_provider_observation_preserves_missing_and_not_ready_refusals() {
        let expected = identity_provider(false);
        let capability = expected.descriptor().capability.clone();
        let host = identity_service(expected.clone());
        assert_eq!(
            host.selects_same_provider(&capability, &expected),
            Err(HostProblem::ProviderFailure)
        );
        let absent = CapabilityId::new("host.mq.write", InvocationLimits::default()).unwrap();
        assert_eq!(
            host.selects_same_provider(&absent, &expected),
            Err(HostProblem::Unsupported)
        );
        let empty = ScopedHostService::new(
            Arc::new(RegistrySnapshot::new(1, vec![], InvocationLimits::default()).unwrap()),
            HostLimits::default(),
        );
        assert_eq!(
            empty.selects_same_provider(&capability, &expected),
            Err(HostProblem::Unsupported)
        );
    }

    #[test]
    fn provider_generation_and_durable_cancellation_are_rechecked() {
        let limits = InvocationLimits::default();
        let capability = CapabilityId::new("host.state.read", limits).unwrap();
        assert_eq!(
            service()
                .validate_provider_generations(&BTreeMap::from([(capability.clone(), "1".into())])),
            Ok(())
        );
        assert_eq!(
            service().validate_provider_generations(&BTreeMap::from([(
                capability.clone(),
                "stale".into()
            )])),
            Err(HostProblem::ProviderFailure)
        );
        let generation_mismatch = invocation(true)
            .with_provider_generations(BTreeMap::from([(capability, "stale".into())]), limits)
            .unwrap();
        let failed = service().invoke(
            &generation_mismatch,
            1,
            false,
            request(&generation_mismatch.run_unit_id),
        );
        assert_eq!(failed.effect().outcome, Err(HostProblem::ProviderFailure));
        assert_eq!(failed.audit().decision, AuditDecision::ProviderFailure);

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
                .effect()
                .outcome,
            Err(HostProblem::Cancelled)
        );
    }
    struct BudgetProvider {
        descriptor: CapabilityDescriptor,
        calls: Arc<std::sync::atomic::AtomicUsize>,
        outcome: Result<crate::HostResult, HostProblem>,
        bad_sequence: bool,
    }
    impl HostProvider for BudgetProvider {
        fn descriptor(&self) -> &CapabilityDescriptor {
            &self.descriptor
        }
        fn invoke(&self, _: &Invocation, request: EffectRequest) -> EffectResult {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            EffectResult {
                sequence: if self.bad_sequence {
                    0
                } else {
                    request.sequence
                },
                outcome: self.outcome.clone(),
            }
        }
    }
    fn budget_service(
        capability: &str,
        request_limit: usize,
        result_limit: usize,
        outcome: Result<crate::HostResult, HostProblem>,
        bad_sequence: bool,
    ) -> (ScopedHostService, Arc<std::sync::atomic::AtomicUsize>) {
        let l = InvocationLimits::default();
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let provider = BudgetProvider {
            descriptor: CapabilityDescriptor {
                capability: CapabilityId::new(capability, l).unwrap(),
                provider_id: "budget".into(),
                generation: "1".into(),
                request_schema: "request@1".into(),
                result_schema: "result@1".into(),
                max_request_bytes: request_limit,
                max_result_bytes: result_limit,
                ready: true,
            },
            calls: calls.clone(),
            outcome,
            bad_sequence,
        };
        let service = ScopedHostService::new(
            Arc::new(RegistrySnapshot::new(1, vec![Arc::new(provider)], l).unwrap()),
            HostLimits::default(),
        );
        (service, calls)
    }
    #[test]
    fn canonical_provider_budgets_accept_exact_bytes_and_reject_before_dispatch() {
        let inv = invocation(true);
        let req = request(&inv.run_unit_id);
        let reply = Ok(crate::HostResult::State {
            value: Some(vec![255; 4096]),
            version: 1,
        });
        let n = canonical_request_size(&req.request, usize::MAX).unwrap();
        let m = canonical_result_size(&reply, usize::MAX).unwrap();
        let (host, calls) = budget_service("host.state.read", n, m, reply.clone(), false);
        assert_eq!(
            host.invoke(&inv, 1, false, req.clone()).effect().outcome,
            reply
        );
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        let (host, calls) = budget_service("host.state.read", n - 1, m, reply.clone(), false);
        assert_eq!(
            host.invoke(&inv, 1, false, req.clone()).effect().outcome,
            Err(HostProblem::ResourceExhausted)
        );
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
        let (host, calls) = budget_service("host.state.read", n, m - 1, reply, false);
        assert_eq!(
            host.invoke(&inv, 1, false, req).effect().outcome,
            Err(HostProblem::ResourceExhausted)
        );
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    }
    #[test]
    fn uncertainty_is_not_erased_by_result_budget_or_bad_envelope() {
        let inv = invocation(true);
        let req = request(&inv.run_unit_id);
        let (host, _) = budget_service(
            "host.state.read",
            1024,
            1,
            Err(HostProblem::UnknownOutcome),
            true,
        );
        assert_eq!(
            host.invoke(&inv, 1, false, req).effect().outcome,
            Err(HostProblem::UnknownOutcome)
        );
    }
    #[test]
    fn unusable_successful_mutation_reply_requires_reconciliation() {
        let mut inv = invocation(true);
        let limits = InvocationLimits::default();
        inv.principal = Principal::new(
            inv.principal.id().clone(),
            BTreeSet::from([CapabilityId::new("host.state.write", limits).unwrap()]),
            limits,
        )
        .unwrap();
        let mut req = request(&inv.run_unit_id);
        let key = IdempotencyKey::new("budget-effect", limits).unwrap();
        req.idempotency_key = Some(key.clone());
        req.request = HostRequest::State(StateRequest::Put {
            key: "x".into(),
            value: vec![1],
            expected_version: None,
            mutation: crate::Mutation {
                sequence: 1,
                idempotency_key: key,
                transaction: None,
            },
        });
        let reply = Ok(crate::HostResult::State {
            value: Some(vec![255; 4096]),
            version: 1,
        });
        let (host, calls) = budget_service("host.state.write", 4096, 1, reply, false);
        assert_eq!(
            host.invoke(&inv, 1, false, req).effect().outcome,
            Err(HostProblem::UnknownOutcome)
        );
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[test]
    fn mandatory_audit_failure_is_fail_closed_and_preserves_mutation_uncertainty() {
        let inv = invocation(true);
        let read = service()
            .invoke(&inv, 1, false, request(&inv.run_unit_id))
            .persist_with(|_| Err(HostProblem::ResourceExhausted));
        assert_eq!(read.outcome, Err(HostProblem::ResourceExhausted));

        let limits = InvocationLimits::default();
        let mut invocation = invocation(true);
        invocation.principal = Principal::new(
            invocation.principal.id().clone(),
            BTreeSet::from([CapabilityId::new("host.state.write", limits).unwrap()]),
            limits,
        )
        .unwrap();
        let key = IdempotencyKey::new("audit-failure-effect", limits).unwrap();
        let request = EffectRequest {
            run_unit: invocation.run_unit_id.clone(),
            sequence: 1,
            deadline_tick: 100,
            idempotency_key: Some(key.clone()),
            request: HostRequest::State(StateRequest::Put {
                key: "x".into(),
                value: vec![1],
                expected_version: None,
                mutation: crate::Mutation {
                    sequence: 1,
                    idempotency_key: key,
                    transaction: None,
                },
            }),
        };
        let reply = Ok(crate::HostResult::State {
            value: Some(vec![1]),
            version: 1,
        });
        let (host, calls) = budget_service("host.state.write", 4096, 4096, reply, false);
        let write = host
            .invoke(&invocation, 1, false, request)
            .persist_with(|_| Err(HostProblem::ResourceExhausted));
        assert_eq!(write.outcome, Err(HostProblem::UnknownOutcome));
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    }
}
