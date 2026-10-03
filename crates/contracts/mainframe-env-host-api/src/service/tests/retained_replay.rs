use super::*;
use crate::mq_mqi::*;
fn inquiry(i: &Invocation, pending: bool) -> EffectRequest {
    let owner = crate::MqHandleOwner {
        environment: crate::MqHostEnvironment::ZosBatch,
        host_id: 1,
        process_id: 1,
        thread_id: 1,
        task_id: 1,
        syncpoint_epoch: 1,
    };
    let mut registry = crate::MqHandleRegistry::new(1, 4).unwrap();
    let h = registry
        .connect(owner, crate::MqHandleSharing::NonShared)
        .unwrap();
    let o = registry.create_object(owner, h).unwrap();
    let l = MqMqiLimits::default();
    let mut q = MqMqiLocalTypeInquiry::new(h, o, &[20], 1, 0, l)
        .unwrap()
        .into_inquiry();
    if pending {
        q.selectors[0] = MqMqiSelector::PendingInteger(20);
    }
    let k = IdempotencyKey::new("effect", InvocationLimits::default()).unwrap();
    EffectRequest {
        run_unit: i.run_unit_id.clone(),
        sequence: 1,
        deadline_tick: 100,
        idempotency_key: Some(k.clone()),
        request: HostRequest::MqMqi(crate::MqMqiHostRequest {
            envelope: MqMqiRequestEnvelope {
                context: MqMqiContext {
                    owner,
                    syncpoint_owner: crate::MqSyncpointOwner::QueueManager,
                },
                limits: l,
                request: MqMqiRequest::Inquire(q),
            },
            mutation: crate::Mutation {
                sequence: 1,
                idempotency_key: k,
                transaction: None,
            },
        }),
    }
}
fn active() -> Invocation {
    let mut i = invocation(true);
    let l = InvocationLimits::default();
    i.principal = Principal::new(
        i.principal.id().clone(),
        BTreeSet::from([CapabilityId::new("host.mq.write", l).unwrap()]),
        l,
    )
    .unwrap();
    i
}
#[test]
fn retained_replay_default_refuses_without_ordinary_panic_dispatch_and_never_promotes_pending20() {
    let i = active();
    let l = InvocationLimits::default();
    let p = Arc::new(PanicProvider {
        descriptor: CapabilityDescriptor {
            capability: CapabilityId::new("host.mq.write", l).unwrap(),
            provider_id: "default".into(),
            generation: "1".into(),
            request_schema: "fixture@1".into(),
            result_schema: "fixture@1".into(),
            max_request_bytes: 8 * 1024 * 1024,
            max_result_bytes: 8 * 1024 * 1024,
            ready: true,
        },
    });
    let host = ScopedHostService::new(
        Arc::new(RegistrySnapshot::new(1, vec![p], l).unwrap()),
        HostLimits::default(),
    );
    for pending in [false, true] {
        let (r, a) = host
            .replay_retained(&i, 1, false, inquiry(&i, pending), [0; 32], &())
            .into_transaction_parts();
        assert_eq!(r.outcome, Err(HostProblem::Unsupported));
        assert_eq!(a.decision, AuditDecision::Rejected);
    }
    let (r, a) = host
        .replay_retained(&i, 1, true, inquiry(&i, false), [0; 32], &())
        .into_transaction_parts();
    assert_eq!(r.outcome, Err(HostProblem::Cancelled));
    assert_eq!(a.decision, AuditDecision::Cancelled);
}

#[test]
fn retained_replay_wrong_occurrence_denial_is_unknown_before_actual_audit() {
    struct WrongOccurrence(CapabilityDescriptor);
    impl HostProvider for WrongOccurrence {
        fn descriptor(&self) -> &CapabilityDescriptor {
            &self.0
        }
        fn invoke(&self, _: &Invocation, _: EffectRequest) -> EffectResult {
            panic!("ordinary invoke fallback")
        }
        fn replay_retained(
            &self,
            _: &Invocation,
            r: EffectRequest,
            _: [u8; 32],
            _: u64,
            _: &(dyn std::any::Any + Send + Sync),
        ) -> EffectResult {
            EffectResult {
                sequence: r.sequence + 1,
                outcome: Err(HostProblem::Unauthorized),
            }
        }
    }
    let i = active();
    let l = InvocationLimits::default();
    let p = Arc::new(WrongOccurrence(CapabilityDescriptor {
        capability: CapabilityId::new("host.mq.write", l).unwrap(),
        provider_id: "fixture".into(),
        generation: "1".into(),
        request_schema: "fixture@1".into(),
        result_schema: "fixture@1".into(),
        max_request_bytes: 8 * 1024 * 1024,
        max_result_bytes: 8 * 1024 * 1024,
        ready: true,
    }));
    let host = ScopedHostService::new(
        Arc::new(RegistrySnapshot::new(1, vec![p], l).unwrap()),
        HostLimits::default(),
    );
    let (reply, audit) = host
        .replay_retained(&i, 1, false, inquiry(&i, false), [0; 32], &())
        .into_transaction_parts();
    assert_eq!(reply.sequence, 1);
    assert_eq!(reply.outcome, Err(HostProblem::UnknownOutcome));
    assert_eq!(audit.decision, AuditDecision::UnknownOutcome);
}
