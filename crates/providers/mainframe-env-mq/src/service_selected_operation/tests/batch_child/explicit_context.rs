//! Actual unbound original fixture Invocations and physical backends. This is
//! private trusted embedding configuration, not real installed host attestation.
use super::*;
use crate::host_context::AttestedHostContext;

#[path = "explicit_context/failures.rs"]
mod failures;

fn context() -> AttestedHostContext {
    AttestedHostContext {
        environment: MqHostEnvironment::ZosBatch,
        owner: MqSyncpointOwner::QueueManager,
    }
}
fn unbound(sqlite: bool) -> Fixture {
    let store: Arc<dyn PlatformStore> = if sqlite {
        Arc::new(SqliteStateStore::open("sqlite::memory:", 64 << 20, 256).unwrap())
    } else {
        Arc::new(MemoryStore::new(mainframe_env_store::StoreLimits {
            max_audits: 256,
            ..Default::default()
        }))
    };
    Fixture::with_context(store, true)
}
fn connect_request() -> MqMqiRequest {
    MqMqiRequest::Connect(MqMqiConnect {
        manager: None,
        sharing: MqHandleSharing::NonShared,
        options: MqMqiOptions::ContractDefault,
    })
}
fn child_connect(f: &Fixture, child: &Child) -> MqHconn {
    let MqMqiOutput::Connected(c) = output(child.call(f, 10, connect_request())) else {
        panic!()
    };
    *f.connection.lock().unwrap() = Some(c);
    c
}
fn parent_call(f: &Fixture) -> (EffectRequest, EffectRecord) {
    let e = EffectRequest {
        run_unit: f.inv.run_unit_id.clone(),
        sequence: 50,
        idempotency_key: Some(IdempotencyKey::new("original-call", Default::default()).unwrap()),
        deadline_tick: 900,
        request: HostRequest::Program(ProgramRequest::Call {
            program: ProgramName::new("CHILD", HostLimits::default().max_name_bytes).unwrap(),
            payload: BoundedPayload::new("fixture-call@1", vec![], Default::default()).unwrap(),
            service: None,
        }),
    };
    let record = EffectRecord {
        execution_id: f.inv.execution_id.clone(),
        run_unit_id: f.inv.run_unit_id.clone(),
        sequence: e.sequence,
        key: e.idempotency_key.clone().unwrap(),
        digest_format: EffectDigestFormat::CanonicalHostV1,
        request_digest: canonical_request_digest(&e.request).unwrap(),
        state: EffectState::Intent,
        result_digest: None,
        resolved_tick: None,
        intent: EffectIntentMetadata {
            owner: f.inv.execution_id.clone(),
            attempt: f.inv.attempt,
            capability: Some(e.request.required_capability(Default::default())),
            audit_resource: Some(canonical_audit_resource_digest(&e.request)),
            audit_invocation_key: Some(f.inv.idempotency_key.clone()),
            created_tick: 8,
            recovery_after_tick: 900,
            epoch: 8,
            recovery_lease: None,
        },
    };
    f.store.record_intent(record.clone()).unwrap();
    (e, record)
}

#[test]
fn memory_sqlite_explicit_unbound_original_call_child_first_connect_and_parent_cmit_back() {
    for sqlite in [false, true] {
        let f = unbound(sqlite);
        assert!(f.inv.bindings.is_empty());
        let parent = f.inv.clone();
        let (call, original_call) = parent_call(&f);
        let call_digest = canonical_request_digest(&call.request).unwrap();
        let child = Child::new(&f);
        let original_child = child.inv.clone();
        assert!(child.inv.bindings.is_empty());
        let c = child_connect(&f, &child);
        let o = match output(
            child.call(
                &f,
                11,
                MqMqiRequest::Open(
                    MqObjectOpenRequest::new(
                        c,
                        lookup(),
                        &[MqRouteOpenAccess::InputShared, MqRouteOpenAccess::Output],
                        Default::default(),
                    )
                    .unwrap(),
                ),
            ),
        ) {
            MqMqiOutput::Opened {
                object,
                dynamic: None,
            } => object,
            _ => panic!(),
        };
        let unit = f.unit();
        let put_effect = child.effect(
            &f,
            12,
            MqMqiRequest::Put {
                connection: c,
                object: o,
                put: put(MqMqiUnitOfWork::Local { unit }),
            },
        );
        let put_digest = canonical_request_digest(&put_effect.request).unwrap();
        child.seed(&f, &put_effect);
        let original_put = f
            .store
            .effect(put_effect.idempotency_key.as_ref().unwrap())
            .unwrap();
        child.execute(&f, &put_effect).unwrap();
        assert_pending(&f, unit);
        assert_eq!(f.depth(), 0);
        f.service
            .return_selected_batch_child(child.binding.frame(), &child.inv)
            .unwrap();
        assert_pending(&f, unit);
        f.call(
            20,
            MqMqiRequest::Commit {
                connection: c,
                unit,
            },
        );
        assert_eq!(f.depth(), 1);
        let next = Child::named(&f, "next");
        let unit = f.unit();
        next.call(
            &f,
            30,
            MqMqiRequest::Put {
                connection: c,
                object: o,
                put: put(MqMqiUnitOfWork::Local { unit }),
            },
        );
        next.call(
            &f,
            31,
            MqMqiRequest::Back {
                connection: c,
                unit,
            },
        );
        assert_eq!(f.depth(), 1);
        f.service
            .return_selected_batch_child(next.binding.frame(), &next.inv)
            .unwrap();
        f.call(21, MqMqiRequest::Disconnect { connection: c });
        assert_eq!(f.inv, parent);
        assert_eq!(child.inv, original_child);
        assert!(f.inv.bindings.is_empty() && child.inv.bindings.is_empty());
        assert_eq!(
            canonical_request_digest(&call.request).unwrap(),
            call_digest
        );
        assert_eq!(
            f.store.effect(&original_call.key).unwrap(),
            Some(original_call)
        );
        assert_eq!(
            canonical_request_digest(&put_effect.request).unwrap(),
            put_digest
        );
        assert_eq!(
            f.store
                .effect(put_effect.idempotency_key.as_ref().unwrap())
                .unwrap(),
            original_put
        );
    }
}

#[test]
fn memory_sqlite_explicit_original_completed_child_connect_replay_cold_incarnation_fence() {
    for sqlite in [false, true] {
        let f = unbound(sqlite);
        let child = Child::new(&f);
        let e = child.effect(&f, 10, connect_request());
        child.seed(&f, &e);
        let reply = child.execute(&f, &e).unwrap();
        let key = e.idempotency_key.as_ref().unwrap();
        let mut core = f.store.effect(key).unwrap().unwrap();
        core.state = EffectState::Completed;
        core.result_digest = Some(canonical_result_digest(&reply.outcome).unwrap());
        core.resolved_tick = Some(20);
        f.store.record_result(key, core.clone()).unwrap();
        assert_eq!(child.execute(&f, &e).unwrap(), reply);
        let cold = MqService::open_selected_mqi(
            f.store.clone(),
            MqLimits::default(),
            3,
            5,
            f.saf.clone(),
            f.clock.clone(),
        )
        .unwrap();
        let process = cold
            .mint_selected_process_explicit(&f.inv, context())
            .unwrap();
        let (frame, owner) = cold.bind_selected_root_explicit(process, &f.inv).unwrap();
        let mut fresh = f.effect(20, connect_request());
        let HostRequest::MqMqi(h) = &mut fresh.request else {
            panic!()
        };
        h.envelope.context.owner = owner;
        f.seed(&fresh);
        cold.execute_selected_mqi(
            frame,
            &f.inv,
            fresh
                .mq_mqi_occurrence(HostLimits::default())
                .unwrap()
                .unwrap(),
            &f.provider,
            HostLimits::default(),
        )
        .unwrap();
        let rows = f.rows();
        let saf = f.saf.calls.load(Ordering::SeqCst);
        assert_eq!(child.execute(&f, &e), Err(HostProblem::UnknownOutcome));
        assert_eq!(f.rows(), rows);
        assert_eq!(f.saf.calls.load(Ordering::SeqCst), saf);
        assert_eq!(f.store.effect(key).unwrap(), Some(core));
    }
}
