//! Closed facet fixtures do not attest a real installed producer.
use super::*;

#[test]
fn memory_sqlite_independently_admitted_task_does_not_inherit_prior_connection_or_unit() {
    for sqlite in [false, true] {
        let f = Fixture::new(sqlite);
        let root = f.root();
        let mut parent = root.frame();
        let (first, _) = connected(&f, &mut parent);
        let first_unit = unit(&parent, first);
        let mut other = f.parent.clone();
        other.execution_id = ExecutionId::new("other-task", Default::default()).unwrap();
        other.run_unit_id = RunUnitId::new("other-task-run", Default::default()).unwrap();
        other.idempotency_key =
            IdempotencyKey::new("other-task-invocation", Default::default()).unwrap();
        other.request_id = RequestId::new("other-task-request", Default::default()).unwrap();
        other.cancellation_probe = Some(CancellationProbe::new());
        seed_execution(&*f.store, &other);
        let other_root = f.runtime.admit_root(other).unwrap();
        let mut other_frame = other_root.frame();
        let MqMqiOutput::Connected(second) = output(f.call(&mut other_frame, 1, connect())) else {
            panic!()
        };
        assert_ne!(first, second);
        assert_ne!(first_unit, unit(&other_frame, second));
        assert!(other_frame.current_unit(first).is_err());
        warning(f.call(&mut other_frame, 2, connect()), second);
        warning(f.call(&mut parent, 3, connect()), first);
        assert_eq!(unit(&parent, first), first_unit);
    }
}

fn warning(result: EffectResult, expected: MqHconn) {
    let HostResult::MqMqi(h) = result.outcome.unwrap() else {
        panic!()
    };
    let MqMqiOutcome::ReviewedOutput {
        status,
        output: MqMqiOutput::Connected(c),
    } = h.result.outcome
    else {
        panic!("defined prior handle warning")
    };
    assert_eq!(status.wire_pair(), (1, 2002));
    assert_eq!(c, expected);
}

#[test]
fn memory_sqlite_child_first_parent_and_next_checked_child_warning_keep_original_origin_and_work() {
    for sqlite in [false, true] {
        for commit in [false, true] {
            let f = Fixture::new(sqlite);
            let root = f.root();
            let mut parent = root.frame();
            let mut child = f.child(&parent, "first");
            let original_parent = parent.original().clone();
            let original_child = child.original().clone();
            let (c, o) = connected(&f, &mut child);
            let u = unit(&child, c);
            f.call(
                &mut child,
                3,
                put_request(c, o, MqMqiUnitOfWork::Local { unit: u }),
            );
            let owner = f
                .store
                .get_provider_state("mq-selected-v1-uow-owner", &u.to_string())
                .unwrap()
                .unwrap();
            child.return_normal().unwrap();
            warning(f.call(&mut parent, 4, connect()), c);
            let mut next = f.child(&parent, "next");
            let MqMqiRequest::Connect(connect) = connect() else {
                panic!()
            };
            warning(
                f.call(&mut next, 5, MqMqiRequest::ConnectExtended(connect)),
                c,
            );
            assert_eq!(unit(&parent, c), u);
            assert_eq!(unit(&next, c), u);
            assert_eq!(
                f.store
                    .get_provider_state("mq-selected-v1-uow-owner", &u.to_string())
                    .unwrap()
                    .unwrap()
                    .payload,
                owner.payload
            );
            let origin: serde_json::Value = serde_json::from_slice(&owner.payload).unwrap();
            assert_eq!(
                origin["value"]["execution"],
                original_parent.execution_id.as_str()
            );
            assert_eq!(origin["value"]["connection_key"], "first-1");
            assert_eq!(parent.original(), &original_parent);
            assert_eq!(child.original(), &original_child);
            let audits = f
                .store
                .audit_records(&next.original().execution_id, 0, 128)
                .unwrap();
            assert_eq!(audits.last().unwrap().effect_sequence, 5);
            assert_eq!(
                audits.last().unwrap().execution_id,
                next.original().execution_id
            );
            next.return_normal().unwrap();
            assert_eq!(unit(&parent, c), u);
            f.call(
                &mut parent,
                6,
                if commit {
                    MqMqiRequest::Commit {
                        connection: c,
                        unit: u,
                    }
                } else {
                    MqMqiRequest::Back {
                        connection: c,
                        unit: u,
                    }
                },
            );
            assert_eq!(f.depth(), usize::from(commit));
            f.call(
                &mut parent,
                7,
                put_request(c, o, MqMqiUnitOfWork::NoSyncpoint),
            );
            f.call(&mut parent, 8, MqMqiRequest::Disconnect { connection: c });
            assert!(parent.current_unit(c).is_err());
        }
    }
}

#[test]
fn memory_sqlite_warning_cannot_borrow_parent_core_or_foreign_probe_or_task_topology() {
    for sqlite in [false, true] {
        let f = Fixture::new(sqlite);
        let root = f.root();
        let mut parent = root.frame();
        connected(&f, &mut parent);
        let mut child = f.child(&parent, "child");
        let e = effect(&child, 10, connect());
        seed(&*f.store, parent.original(), &e); // deliberately wrong actor
        let rows = f.rows();
        let saf = f.saf.calls.load(Ordering::SeqCst);
        assert!(dispatch(&mut child, &e).is_err());
        let actual = child_invocation(parent.original(), "separate");
        assert!(
            f.runtime
                .prepare_same_task_child(
                    &parent,
                    actual,
                    MqTrustedBatchRelationship::SeparateSubtask
                )
                .is_err()
        );
        let mut fake = child_invocation(parent.original(), "forged-probe");
        fake.cancellation_probe = Some(CancellationProbe::new());
        assert!(
            f.runtime
                .prepare_same_task_child(&parent, fake, MqTrustedBatchRelationship::SameTaskCall)
                .is_err()
        );
        assert_eq!(f.rows(), rows);
        assert_eq!(f.saf.calls.load(Ordering::SeqCst), saf);
    }
}
