use super::*;

#[test]
fn memory_sqlite_completed_child_receipt_replay_is_fenced_by_new_incarnation() {
    for sqlite in [false, true] {
        let f = Fixture::new(sqlite);
        let root = f.root();
        let parent = root.frame();
        let mut child = f.child(&parent, "child");
        let e = effect(&child, 1, connect());
        seed(&*f.store, child.original(), &e);
        let reply = dispatch(&mut child, &e).unwrap();
        let key = e.idempotency_key.as_ref().unwrap();
        let mut core = f.store.effect(key).unwrap().unwrap();
        core.state = EffectState::Completed;
        core.result_digest = Some(canonical_result_digest(&reply.outcome).unwrap());
        core.resolved_tick = Some(20);
        f.store.record_result(key, core.clone()).unwrap();
        assert_eq!(dispatch(&mut child, &e).unwrap(), reply);
        let cold = open(f.store.clone(), f.saf.clone(), f.clock.clone()).unwrap();
        let cold_root = cold.admit_root(f.parent.clone()).unwrap();
        let mut fresh = cold_root.frame();
        let fresh_effect = effect(&fresh, 20, connect());
        seed(&*f.store, fresh.original(), &fresh_effect);
        dispatch(&mut fresh, &fresh_effect).unwrap();
        let rows = f.rows();
        let saf = f.saf.calls.load(Ordering::SeqCst);
        assert_eq!(dispatch(&mut child, &e), Err(HostProblem::UnknownOutcome));
        assert_eq!(f.rows(), rows);
        assert_eq!(f.saf.calls.load(Ordering::SeqCst), saf);
        assert_eq!(f.store.effect(key).unwrap(), Some(core));
    }
}

#[test]
fn sqlite_physical_reopen_never_reconstructs_opaque_frame_or_pending_unit_access() {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    let path = std::env::temp_dir().join(format!(
        "mq-facet-{}-{}.sqlite",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::SeqCst)
    ));
    assert!(!path.exists());
    let db = || -> Arc<dyn PlatformStore> {
        Arc::new(
            SqliteStateStore::open(
                &format!("sqlite://{}?mode=rwc", path.display()),
                64 << 20,
                256,
            )
            .unwrap(),
        )
    };
    let (parent, rows, child_original, old_connection, core, key, audits) = {
        let f = Fixture::from_store(db());
        let root = f.root();
        let parent = root.frame();
        let mut child = f.child(&parent, "child");
        let (c, o) = connected(&f, &mut child);
        let u = unit(&child, c);
        let e = effect(
            &child,
            3,
            put_request(c, o, MqMqiUnitOfWork::Local { unit: u }),
        );
        seed(&*f.store, child.original(), &e);
        dispatch(&mut child, &e).unwrap();
        let key = e.idempotency_key.clone().unwrap();
        (
            f.parent.clone(),
            f.rows(),
            child.original().clone(),
            c,
            f.store.effect(&key).unwrap(),
            key,
            f.store
                .audit_records(&child.original().execution_id, 0, 128)
                .unwrap(),
        )
    };
    let store = db();
    let saf = Arc::new(Saf::default());
    let runtime = open(
        store.clone(),
        saf.clone(),
        Arc::new(Clock(AtomicU64::new(20))),
    )
    .unwrap();
    let root = runtime.admit_root(parent).unwrap();
    let parent = root.frame();
    let child = runtime
        .prepare_same_task_child(
            &parent,
            child_original.clone(),
            MqTrustedBatchRelationship::SameTaskCall,
        )
        .unwrap();
    assert!(child.current_unit(old_connection).is_err());
    assert_eq!(store.list_provider_state_prefix("mq-", 4096).unwrap(), rows);
    assert_eq!(store.effect(&key).unwrap(), core);
    assert_eq!(
        store
            .audit_records(&child_original.execution_id, 0, 128)
            .unwrap(),
        audits
    );
    assert_eq!(saf.calls.load(Ordering::SeqCst), 0);
    drop(child);
    drop(parent);
    drop(root);
    drop(runtime);
    drop(store);
    for suffix in ["", "-wal", "-shm"] {
        let file = std::path::PathBuf::from(format!("{}{suffix}", path.display()));
        if file.exists() {
            std::fs::remove_file(file).unwrap();
        }
    }
}
