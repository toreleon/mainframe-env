use super::*;

#[test]
fn memory_sqlite_absent_legacy_and_mismatched_rich_open_never_writes() {
    for sqlite in [false, true] {
        let store = backend(sqlite);
        let saf = Arc::new(Saf::default());
        let clock = Arc::new(Clock(AtomicU64::new(20)));
        let before = store.list_provider_state_prefix("mq-", 4096).unwrap();
        assert!(open(store.clone(), saf.clone(), clock.clone()).is_err());
        assert_eq!(
            store.list_provider_state_prefix("mq-", 4096).unwrap(),
            before
        );
        let legacy = MqService::open(store.clone(), Default::default()).unwrap();
        legacy
            .install(vec![MqQueueDefinition {
                name: "Q".into(),
                trigger_program: None,
            }])
            .unwrap();
        let before = store.list_provider_state_prefix("mq-", 4096).unwrap();
        assert!(open(store.clone(), saf.clone(), clock.clone()).is_err());
        assert_eq!(
            store.list_provider_state_prefix("mq-", 4096).unwrap(),
            before
        );
        let f = Fixture::new(sqlite);
        let before = f.rows();
        assert!(
            MqTrustedBatchRuntime::open(
                f.store.clone(),
                Default::default(),
                4,
                5,
                f.saf.clone(),
                f.clock.clone(),
                descriptor(),
                Default::default(),
                Default::default()
            )
            .is_err()
        );
        assert_eq!(f.rows(), before);
        assert_eq!(saf.calls.load(Ordering::SeqCst), 0);
    }
}

#[test]
fn memory_sqlite_foreign_runtime_and_changed_child_probe_or_topology_refuse() {
    for sqlite in [false, true] {
        let f = Fixture::new(sqlite);
        let root = f.root();
        let parent = root.frame();
        let other = Fixture::new(sqlite);
        let original = child_invocation(&f.parent, "child");
        let before = f.rows();
        assert!(
            other
                .runtime
                .prepare_same_task_child(
                    &parent,
                    original.clone(),
                    MqTrustedBatchRelationship::SameTaskCall
                )
                .is_err()
        );
        assert!(
            f.runtime
                .prepare_same_task_child(
                    &parent,
                    original.clone(),
                    MqTrustedBatchRelationship::SeparateSubtask
                )
                .is_err()
        );
        for case in 0..9 {
            let mut child = original.clone();
            match case {
                0 => child.cancellation_probe = Some(CancellationProbe::new()),
                1 => child.parent_execution_id = None,
                2 => child.execution_id = f.parent.execution_id.clone(),
                3 => child.run_unit_id = RunUnitId::new("foreign", Default::default()).unwrap(),
                4 => child.attempt += 1,
                5 => child.deadline_tick += 1,
                6 => child.limits.max_steps += 1,
                7 => child
                    .provider_generations
                    .insert(descriptor().capability, "other".into())
                    .map(|_| ())
                    .unwrap_or(()),
                _ => {
                    child.principal = Principal::new(
                        PrincipalId::new("FOREIGN", Default::default()).unwrap(),
                        BTreeSet::new(),
                        Default::default(),
                    )
                    .unwrap()
                }
            }
            assert!(
                f.runtime
                    .prepare_same_task_child(
                        &parent,
                        child,
                        MqTrustedBatchRelationship::SameTaskCall
                    )
                    .is_err(),
                "case {case}"
            );
        }
        assert_eq!(f.rows(), before);
        assert_eq!(f.saf.calls.load(Ordering::SeqCst), 0);
        assert_eq!(
            parent.context().unwrap().owner,
            root.frame().context().unwrap().owner
        );
    }
}

#[test]
fn memory_sqlite_present_context_conflicts_root_child_and_parent_some_fail() {
    for sqlite in [false, true] {
        let f = Fixture::new(sqlite);
        let root = f.root();
        let parent = root.frame();
        for payload in [
            b"zos-cics|host-coordinator".as_slice(),
            b"zos-ims|host-coordinator",
            b"mqi-client|queue-manager",
            b"zos-batch|host-coordinator",
            b"malformed",
        ] {
            let mut original = f.parent.clone();
            original.bindings.insert(
                "mq.host-context".into(),
                BoundedPayload::new(
                    "mainframe-env.mq.host-context@1",
                    payload.to_vec(),
                    Default::default(),
                )
                .unwrap(),
            );
            assert!(f.runtime.admit_root(original.clone()).is_err());
            let child = child_invocation(&original, "child");
            assert!(
                f.runtime
                    .prepare_same_task_child(
                        &parent,
                        child,
                        MqTrustedBatchRelationship::SameTaskCall
                    )
                    .is_err()
            );
        }
        let mut cics = f.parent.clone();
        cics.bindings.insert(
            "cics.execution-context".into(),
            BoundedPayload::new(
                "mainframe-env.cics.execution-context@1",
                b"local".to_vec(),
                Default::default(),
            )
            .unwrap(),
        );
        assert!(f.runtime.admit_root(cics).is_err());
        assert!(
            f.runtime
                .admit_root(child_invocation(&f.parent, "child"))
                .is_err()
        );
        assert!(parent.context().is_ok());
    }
}

#[test]
fn memory_sqlite_original_limits_context_and_parent_core_substitution_fail() {
    for sqlite in [false, true] {
        for case in 0..3 {
            let f = Fixture::new(sqlite);
            let root = f.root();
            let parent = root.frame();
            let mut child = f.child(&parent, "child");
            let mut e = effect(&child, 1, connect());
            if let HostRequest::MqMqi(h) = &mut e.request {
                if case == 0 {
                    h.envelope.limits.buffer_bytes -= 1;
                }
                if case == 1 {
                    h.envelope.context.owner.task_id += 1;
                }
            }
            seed(
                &*f.store,
                if case == 2 {
                    &f.parent
                } else {
                    child.original()
                },
                &e,
            );
            let rows = f.rows();
            assert!(dispatch(&mut child, &e).is_err());
            assert_eq!(f.rows(), rows);
            assert_eq!(f.saf.calls.load(Ordering::SeqCst), 0);
        }
    }
}

#[test]
fn memory_sqlite_failed_duplicate_root_setup_reclaims_only_its_new_empty_process() {
    for sqlite in [false, true] {
        let f = Fixture::new(sqlite);
        let root = f.root();
        let mut parent = root.frame();
        let (connection, object) = connected(&f, &mut parent);
        let current = unit(&parent, connection);
        f.call(
            &mut parent,
            3,
            put_request(connection, object, MqMqiUnitOfWork::Local { unit: current }),
        );
        let rows = f.rows();
        let calls = f.saf.calls.load(Ordering::SeqCst);
        let owner = parent.context().unwrap().owner;
        // More than the finite directory process capacity: each failed setup
        // must reclaim only its fresh empty process, without retiring this root.
        for _ in 0..257 {
            assert!(f.runtime.admit_root(f.parent.clone()).is_err());
        }
        let mut other = f.parent.clone();
        other.execution_id = ExecutionId::new("other", Default::default()).unwrap();
        other.run_unit_id = RunUnitId::new("other-run", Default::default()).unwrap();
        let next = f.runtime.admit_root(other).unwrap();
        assert_ne!(
            next.frame().context().unwrap().owner.process_id,
            owner.process_id
        );
        assert_eq!(parent.context().unwrap().owner, owner);
        assert_eq!(unit(&parent, connection), current);
        assert_eq!(f.depth(), 0);
        assert_eq!(f.rows(), rows);
        assert_eq!(f.saf.calls.load(Ordering::SeqCst), calls);
    }
}

#[test]
fn invalid_frozen_provider_and_limit_profiles_are_refused_without_writes() {
    let f = Fixture::new(false);
    let rows = f.rows();
    for case in 0..5 {
        let mut p = descriptor();
        let mut host = HostLimits::default();
        let mut mqi = MqMqiLimits::default();
        match case {
            0 => p.ready = false,
            1 => p.provider_id = "foreign".into(),
            2 => {
                p.capability = CapabilityId::new("host.program.invoke", Default::default()).unwrap()
            }
            3 => host.max_records = usize::MAX,
            _ => mqi.canonical_bytes = 0,
        }
        assert!(
            MqTrustedBatchRuntime::open(
                f.store.clone(),
                Default::default(),
                3,
                5,
                f.saf.clone(),
                f.clock.clone(),
                p,
                host,
                mqi
            )
            .is_err()
        );
        assert_eq!(f.rows(), rows);
    }
}
