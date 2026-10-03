use super::*;
use crate::{ProgramName, ProgramRequest};
use mainframe_env_execution_api::BoundedPayload;
use std::sync::atomic::{AtomicUsize, Ordering};

struct ContextProvider {
    descriptor: CapabilityDescriptor,
    calls: AtomicUsize,
    mode: u8,
}
impl HostProvider for ContextProvider {
    fn descriptor(&self) -> &CapabilityDescriptor {
        &self.descriptor
    }
    fn invoke(&self, _: &Invocation, _: EffectRequest) -> EffectResult {
        panic!("context must not delegate to ordinary dispatch")
    }
    fn invoke_program_context(
        &self,
        i: &Invocation,
        r: EffectRequest,
        context: &(dyn std::any::Any + Send + Sync),
    ) -> EffectResult {
        self.calls.fetch_add(1, Ordering::SeqCst);
        assert_eq!(context.downcast_ref::<u32>(), Some(&42));
        assert_eq!(r.run_unit, i.run_unit_id);
        match self.mode {
            1 => panic!("context panic"),
            2 => EffectResult {
                sequence: r.sequence + 1,
                outcome: Err(HostProblem::UnknownOutcome),
            },
            3 => EffectResult {
                sequence: r.sequence + 1,
                outcome: Ok(HostResult::Program(
                    BoundedPayload::new("result@1", vec![7], InvocationLimits::default()).unwrap(),
                )),
            },
            _ => EffectResult {
                sequence: r.sequence,
                outcome: Ok(HostResult::Program(
                    BoundedPayload::new("result@1", vec![7], InvocationLimits::default()).unwrap(),
                )),
            },
        }
    }
}
fn fixture(
    mode: u8,
) -> (
    ScopedHostService,
    Arc<ContextProvider>,
    Invocation,
    EffectRequest,
) {
    let l = InvocationLimits::default();
    let capability = CapabilityId::new("host.program.invoke", l).unwrap();
    let provider = Arc::new(ContextProvider {
        descriptor: CapabilityDescriptor {
            capability: capability.clone(),
            provider_id: "context".into(),
            generation: "1".into(),
            request_schema: "request@1".into(),
            result_schema: "result@1".into(),
            max_request_bytes: 1024,
            max_result_bytes: 1024,
            ready: true,
        },
        calls: AtomicUsize::new(0),
        mode,
    });
    let host = identity_service(provider.clone());
    let mut i = invocation(true);
    i.principal =
        Principal::new(i.principal.id().clone(), BTreeSet::from([capability]), l).unwrap();
    let r = EffectRequest {
        idempotency_key: Some(IdempotencyKey::new("original-program", l).unwrap()),
        request: HostRequest::Program(ProgramRequest::Call {
            program: ProgramName::new("APPLICATION", 128).unwrap(),
            payload: BoundedPayload::new("input@1", vec![1, 2, 3], l).unwrap(),
            service: None,
        }),
        ..request(&i.run_unit_id)
    };
    (host, provider, i, r)
}

#[test]
fn context_uses_frozen_provider_and_original_audit_without_changing_payload() {
    let (host, provider, i, r) = fixture(0);
    let resource = canonical_audit_resource_digest(&r.request);
    let result = host.invoke_program_context(&i, 3, false, r.clone(), &42u32);
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    assert_eq!(result.audit().resource, resource);
    assert_eq!(result.audit().execution_id, i.execution_id);
    assert_eq!(result.audit().run_unit_id, r.run_unit);
    assert_eq!(result.audit().effect_sequence, r.sequence);
    assert_eq!(result.audit().principal, *i.principal.id());
    assert_eq!(result.audit().decision, AuditDecision::Success);
    assert!(result.effect().outcome.is_ok());
}

#[test]
fn non_program_is_refused_before_any_dispatch() {
    let (host, provider, i, _) = fixture(0);
    let result = host.invoke_program_context(&i, 1, false, request(&i.run_unit_id), &42u32);
    assert_eq!(result.effect().outcome, Err(HostProblem::Unsupported));
    assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
    assert_eq!(result.audit().decision, AuditDecision::Rejected);
}

#[test]
fn provider_default_refuses_context_without_ordinary_invoke() {
    let (_, provider, i, r) = fixture(0);
    let host = identity_service(Arc::new(PanicProvider {
        descriptor: provider.descriptor.clone(),
    }));
    assert_eq!(
        host.invoke_program_context(&i, 1, false, r, &42u32)
            .effect()
            .outcome,
        Err(HostProblem::Unsupported)
    );
}

#[test]
fn context_shares_identity_grant_deadline_cancel_generation_and_size_preflight() {
    for case in 0..7 {
        let (host, provider, mut i, mut r) = fixture(0);
        let (now, cancelled, expected) = match case {
            0 => {
                r.run_unit = RunUnitId::new("foreign", InvocationLimits::default()).unwrap();
                (1, false, HostProblem::Malformed)
            }
            1 => (1, true, HostProblem::Cancelled),
            2 => (100, false, HostProblem::TimedOut),
            3 => {
                i.principal = Principal::new(
                    i.principal.id().clone(),
                    BTreeSet::new(),
                    InvocationLimits::default(),
                )
                .unwrap();
                (1, false, HostProblem::Unauthorized)
            }
            4 => {
                i.provider_generations
                    .insert(provider.descriptor.capability.clone(), "foreign".into());
                (1, false, HostProblem::ProviderFailure)
            }
            5 => {
                r.sequence = 0;
                (1, false, HostProblem::Malformed)
            }
            _ => {
                let HostRequest::Program(ProgramRequest::Call { payload, .. }) = &mut r.request
                else {
                    unreachable!()
                };
                *payload =
                    BoundedPayload::new("input@1", vec![0; 2048], InvocationLimits::default())
                        .unwrap();
                (1, false, HostProblem::ResourceExhausted)
            }
        };
        assert_eq!(
            host.invoke_program_context(&i, now, cancelled, r, &42u32)
                .effect()
                .outcome,
            Err(expected),
            "case {case}"
        );
        assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
    }
}

#[test]
fn context_panic_and_reply_uncertainty_use_the_existing_shared_policy() {
    for (mode, expected) in [
        (1, HostProblem::InfrastructureFailure),
        (2, HostProblem::UnknownOutcome),
        (3, HostProblem::UnknownOutcome),
    ] {
        let (host, provider, i, r) = fixture(mode);
        assert_eq!(
            host.invoke_program_context(&i, 1, false, r, &42u32)
                .effect()
                .outcome,
            Err(expected)
        );
        assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    }
}

#[test]
fn context_mutation_audit_failure_cannot_expose_known_success() {
    let (host, provider, i, r) = fixture(0);
    let effect = host
        .invoke_program_context(&i, 1, false, r, &42u32)
        .persist_with(|_| Err(HostProblem::ResourceExhausted));
    assert_eq!(effect.outcome, Err(HostProblem::UnknownOutcome));
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
}
