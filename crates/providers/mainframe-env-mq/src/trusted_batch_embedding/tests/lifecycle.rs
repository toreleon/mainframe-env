use super::*;

#[test]
fn memory_sqlite_abort_and_return_are_once_only_and_do_not_decide_rows() {
    for sqlite in [false, true] {
        let f = Fixture::new(sqlite);
        let root = f.root();
        let mut parent = root.frame();
        let mut child = f.child(&parent, "child");
        let rows = f.rows();
        child.abort_preparation().unwrap();
        assert_eq!(child.abort_preparation(), Err(HostProblem::Unauthorized));
        assert_eq!(child.context(), Err(HostProblem::Unauthorized));
        assert!(
            f.runtime
                .prepare_same_task_child(
                    &child,
                    child_invocation(child.original(), "grandchild"),
                    MqTrustedBatchRelationship::SameTaskCall
                )
                .is_err()
        );
        assert_eq!(f.rows(), rows);
        assert_eq!(parent.return_normal(), Err(HostProblem::Unsupported));
        let mut child = f.child(&parent, "next");
        let c = match output(f.call(&mut child, 1, connect())) {
            MqMqiOutput::Connected(c) => c,
            _ => panic!(),
        };
        assert_eq!(child.abort_preparation(), Err(HostProblem::Unsupported));
        child.return_normal().unwrap();
        assert_eq!(child.return_normal(), Err(HostProblem::Unauthorized));
        assert!(parent.current_unit(c).is_ok());
    }
}

#[test]
fn memory_sqlite_drop_retains_work_and_uncertainty_fences_without_cleanup() {
    for sqlite in [false, true] {
        let f = Fixture::new(sqlite);
        let root = f.root();
        let parent = root.frame();
        let mut child = f.child(&parent, "child");
        let (c, o) = connected(&f, &mut child);
        let u = unit(&child, c);
        f.call(
            &mut child,
            3,
            put_request(c, o, MqMqiUnitOfWork::Local { unit: u }),
        );
        let rows = f.rows();
        let audits = f
            .store
            .audit_records(&child.original().execution_id, 0, 128)
            .unwrap();
        drop(child);
        drop(root);
        assert_eq!(unit(&parent, c), u);
        assert_eq!(f.rows(), rows);
        let mut next = f.child(&parent, "next");
        assert_eq!(next.retain_uncertain(), Err(HostProblem::UnknownOutcome));
        assert_eq!(next.retain_uncertain(), Err(HostProblem::Unauthorized));
        assert_eq!(parent.context(), Err(HostProblem::UnknownOutcome));
        assert_eq!(parent.current_unit(c), Err(HostProblem::UnknownOutcome));
        assert_eq!(f.rows(), rows);
        assert_eq!(f.depth(), 0);
        assert_eq!(
            f.store
                .audit_records(
                    &ExecutionId::new("child", Default::default()).unwrap(),
                    0,
                    128
                )
                .unwrap(),
            audits
        );
    }
}

#[test]
fn memory_sqlite_stale_duplicate_wrapper_and_cancelled_parent_refuse_before_saf() {
    for sqlite in [false, true] {
        let f = Fixture::new(sqlite);
        let root = f.root();
        let parent = root.frame();
        let mut child = f.child(&parent, "child");
        let original = child.original().clone();
        let mut duplicate = f
            .runtime
            .prepare_same_task_child(&parent, original, MqTrustedBatchRelationship::SameTaskCall)
            .unwrap();
        let e = effect(&duplicate, 1, connect());
        seed(&*f.store, duplicate.original(), &e);
        let rows = f.rows();
        child.return_normal().unwrap();
        assert_eq!(duplicate.context(), Err(HostProblem::Unauthorized));
        assert_eq!(dispatch(&mut duplicate, &e), Err(HostProblem::Unauthorized));
        assert_eq!(f.rows(), rows);
        assert_eq!(f.saf.calls.load(Ordering::SeqCst), 0);
        f.parent.cancellation_probe.as_ref().unwrap().request();
        assert_eq!(parent.context(), Err(HostProblem::Cancelled));
        assert!(
            f.runtime
                .prepare_same_task_child(
                    &parent,
                    child_invocation(&f.parent, "next"),
                    MqTrustedBatchRelationship::SameTaskCall
                )
                .is_err()
        );
        assert_eq!(f.rows(), rows);
    }
}
