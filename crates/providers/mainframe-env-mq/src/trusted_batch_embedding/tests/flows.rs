use super::*;

#[test]
fn memory_sqlite_unbound_original_child_first_flow_retains_parent_work_and_actor() {
    for sqlite in [false, true] {
        let f = Fixture::new(sqlite);
        let root = f.root();
        let mut parent = root.frame();
        let parent_original = parent.original().clone();
        let call = EffectRequest {
            run_unit: f.parent.run_unit_id.clone(),
            sequence: 50,
            deadline_tick: 900,
            idempotency_key: Some(
                IdempotencyKey::new("original-call", Default::default()).unwrap(),
            ),
            request: HostRequest::Program(ProgramRequest::Call {
                program: ProgramName::new("CHILD", HostLimits::default().max_name_bytes).unwrap(),
                payload: BoundedPayload::new("fixture-call@1", vec![], Default::default()).unwrap(),
                service: None,
            }),
        };
        seed(&*f.store, &f.parent, &call);
        let call_core = f
            .store
            .effect(call.idempotency_key.as_ref().unwrap())
            .unwrap();
        let mut child = f.child(&parent, "child");
        let original = child.original().clone();
        assert!(original.bindings.is_empty());
        let (c, o) = connected(&f, &mut child);
        let u = unit(&child, c);
        let owner = f
            .store
            .get_provider_state("mq-selected-v1-uow-owner", &u.to_string())
            .unwrap()
            .unwrap();
        assert_eq!(owner.payload, br#"{"schema_version":"mainframe-env.mq-object-row@1","object_key":"1","value":{"schema_version":"mainframe-env.mq-selected-uow-owner@1","coordinator":"queue-manager-local","unit":1,"connection_key":"child-1","execution":"parent","run":"run","principal":"TEST","generation":3,"fence":5,"registry_epoch":1,"state":"pending","queues":[]}}"#);
        let e = effect(
            &child,
            3,
            put_request(c, o, MqMqiUnitOfWork::Local { unit: u }),
        );
        seed(&*f.store, child.original(), &e);
        let core = f.store.effect(e.idempotency_key.as_ref().unwrap()).unwrap();
        let digest = canonical_request_digest(&e.request).unwrap();
        dispatch(&mut child, &e).unwrap();
        assert_eq!(f.depth(), 0);
        assert_eq!(unit(&parent, c), u);
        child.return_normal().unwrap();
        assert_eq!(child.context(), Err(HostProblem::Unauthorized));
        assert_eq!(f.depth(), 0);
        assert_eq!(unit(&parent, c), u);
        f.call(
            &mut parent,
            10,
            MqMqiRequest::Commit {
                connection: c,
                unit: u,
            },
        );
        assert_eq!(f.depth(), 1);
        let mut next = f.child(&parent, "next");
        let u = unit(&next, c);
        f.call(
            &mut next,
            1,
            put_request(c, o, MqMqiUnitOfWork::Local { unit: u }),
        );
        f.call(
            &mut next,
            2,
            MqMqiRequest::Back {
                connection: c,
                unit: u,
            },
        );
        assert_eq!(f.depth(), 1);
        next.return_normal().unwrap();
        f.call(&mut parent, 11, MqMqiRequest::Disconnect { connection: c });
        assert_eq!(parent.original(), &parent_original);
        assert_eq!(child.original(), &original);
        assert_eq!(root.original(), &f.parent);
        assert_eq!(canonical_request_digest(&e.request).unwrap(), digest);
        assert_eq!(
            f.store.effect(e.idempotency_key.as_ref().unwrap()).unwrap(),
            core
        );
        assert_eq!(
            f.store
                .effect(call.idempotency_key.as_ref().unwrap())
                .unwrap(),
            call_core
        );
        let audits = f
            .store
            .audit_records(&original.execution_id, 0, 128)
            .unwrap();
        assert!(
            audits
                .iter()
                .all(|a| a.execution_id == original.execution_id
                    && a.invocation_key == original.idempotency_key)
        );
        assert_eq!(core.unwrap().execution_id, original.execution_id);
    }
}
