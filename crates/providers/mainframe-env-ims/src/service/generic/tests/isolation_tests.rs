use super::*;
use mainframe_env_execution_api::{AuditRecord, ExecutionId};
use mainframe_env_store_api::AuditSink;
use std::sync::{
    Barrier,
    atomic::{AtomicU8, Ordering as AtomicOrdering},
};

fn backends() -> Vec<Arc<dyn ProviderStateStore>> {
    vec![
        Arc::new(MemoryStore::new(Default::default())),
        Arc::new(SqliteStateStore::open("sqlite::memory:", 64 * 1024 * 1024, 262_144).unwrap()),
    ]
}

fn installed(store: Arc<dyn ProviderStateStore>) -> Arc<ImsService> {
    let service = ImsService::open(store, ImsLimits::default()).unwrap();
    service.install_metadata(catalog()).unwrap();
    for run in ["isolation-a", "isolation-b"] {
        execute(
            &service,
            run,
            &request(run, ImsOperation::Schedule, 1, &[], b""),
        );
    }
    service
}

fn rows(store: &dyn ProviderStateStore) -> Vec<ProviderStateRecord> {
    [
        GENERIC_DATABASE_NAMESPACE,
        GENERIC_PENDING_NAMESPACE,
        REPLAY_NAMESPACE,
        SESSION_NAMESPACE,
    ]
    .into_iter()
    .flat_map(|namespace| store.list_provider_state(namespace, 4096).unwrap())
    .collect()
}

fn two_runs(store: Arc<dyn ProviderStateStore>, two_services: bool) {
    let first = ImsService::open(store.clone(), ImsLimits::default()).unwrap();
    first.install_metadata(catalog()).unwrap();
    let second = ImsService::open(store.clone(), ImsLimits::default()).unwrap();
    let writer = if two_services { &second } else { &first };
    execute(
        &first,
        "isolation-a",
        &request("isolation-a", ImsOperation::Schedule, 1, &[], b""),
    );
    execute(
        writer,
        "isolation-b",
        &request("isolation-b", ImsOperation::Schedule, 1, &[], b""),
    );
    execute(
        &first,
        "isolation-a",
        &request("isolation-a", ImsOperation::Insert, 2, &["ROOT"], b"A1X"),
    );
    let insert_b = request("isolation-b", ImsOperation::Insert, 2, &["ROOT"], b"B2Y");
    // The former implementation accepted B, then erased its committed row on A's backout.
    assert_eq!(
        writer.execute(&invocation("isolation-b"), &insert_b),
        Err(HostProblem::IdempotencyConflict)
    );
    execute(
        &first,
        "isolation-a",
        &request("isolation-a", ImsOperation::Rollback, 3, &[], b""),
    );
    assert_eq!(execute(writer, "isolation-b", &insert_b).status, "  ");
    execute(
        writer,
        "isolation-b",
        &request("isolation-b", ImsOperation::Commit, 3, &[], b""),
    );
    // Replaying A's completed rollback must not restore its old image again.
    execute(
        &first,
        "isolation-a",
        &request("isolation-a", ImsOperation::Rollback, 3, &[], b""),
    );
    let reopened = ImsService::open(store, ImsLimits::default()).unwrap();
    assert_eq!(
        restored(
            &reopened.lock().unwrap().state,
            "GENDB",
            ImsLimits::default()
        )
        .unwrap()
        .export_records()
        .iter()
        .map(|row| row.data.clone())
        .collect::<Vec<_>>(),
        vec![b"B2Y".to_vec()]
    );
}

#[test]
fn local_uow_two_runs_memory() {
    two_runs(Arc::new(MemoryStore::new(Default::default())), false);
}

#[test]
fn local_uow_two_services_memory() {
    two_runs(Arc::new(MemoryStore::new(Default::default())), true);
}

#[test]
fn local_uow_two_runs_sqlite() {
    two_runs(
        Arc::new(SqliteStateStore::open("sqlite::memory:", 64 * 1024 * 1024, 262_144).unwrap()),
        false,
    );
}

#[test]
fn local_uow_two_services_sqlite() {
    two_runs(
        Arc::new(SqliteStateStore::open("sqlite::memory:", 64 * 1024 * 1024, 262_144).unwrap()),
        true,
    );
}

#[test]
fn local_uow_load_replace_delete_batch_and_denial_cannot_bypass_owner() {
    for store in backends() {
        let policy = Arc::new(Policy::default());
        let service =
            ImsService::open_authorized(store.clone(), ImsLimits::default(), policy.clone())
                .unwrap();
        service.install_metadata(catalog()).unwrap();
        for run in ["isolation-a", "isolation-b"] {
            execute(
                &service,
                run,
                &request(run, ImsOperation::Schedule, 1, &[], b""),
            );
        }
        execute(
            &service,
            "isolation-a",
            &request("isolation-a", ImsOperation::Insert, 2, &["ROOT"], b"A1X"),
        );
        let hold = request(
            "isolation-b",
            ImsOperation::GetHoldUnique,
            2,
            &["ROOT"],
            b"",
        );
        execute(&service, "isolation-b", &hold);
        let before = rows(&*store);
        for op in [ImsOperation::Replace, ImsOperation::Delete] {
            let req = request("isolation-b", op, 3, &[], b"A1Z");
            assert_eq!(
                service.execute(&invocation("isolation-b"), &req),
                Err(HostProblem::IdempotencyConflict)
            );
            assert_eq!(rows(&*store), before);
        }
        let load = ImsGenericLoadImage {
            database: "GENDB".into(),
            records: vec![],
        };
        let req = request(
            "isolation-b",
            ImsOperation::Load,
            4,
            &[],
            &serde_json::to_vec(&load).unwrap(),
        );
        assert_eq!(
            service.execute(&invocation("isolation-b"), &req),
            Err(HostProblem::IdempotencyConflict)
        );
        let malformed = request("isolation-b", ImsOperation::Load, 5, &[], b"{");
        assert_eq!(
            service.execute(&invocation("isolation-b"), &malformed),
            Err(HostProblem::Malformed)
        );
        let insert = request("isolation-b", ImsOperation::Insert, 6, &["ROOT"], b"B2Y");
        assert_eq!(
            service.execute(
                &invocation_class("isolation-b", ServiceClass::Batch),
                &insert
            ),
            Err(HostProblem::IdempotencyConflict)
        );
        *policy.deny_update.lock().unwrap() = true;
        assert_eq!(
            service.execute(&invocation("isolation-b"), &insert),
            Err(HostProblem::Unauthorized)
        );
        assert_eq!(rows(&*store), before);
        *policy.deny_update.lock().unwrap() = false;
        execute(
            &service,
            "isolation-a",
            &request("isolation-a", ImsOperation::Commit, 3, &[], b""),
        );
        assert_eq!(execute(&service, "isolation-b", &insert).status, "  ");
        execute(
            &service,
            "isolation-b",
            &request("isolation-b", ImsOperation::Rollback, 7, &[], b""),
        );
        assert_eq!(
            restored(
                &service.lock().unwrap().state,
                "GENDB",
                ImsLimits::default()
            )
            .unwrap()
            .export_records()[0]
                .data,
            b"A1X"
        );
    }
}

#[test]
fn local_uow_retained_legacy_undo_reads_but_unproven_backout_is_unknown() {
    for store in backends() {
        let service = installed(store.clone());
        let before = service.lock().unwrap().state.generic_databases["GENDB"].clone();
        execute(
            &service,
            "isolation-a",
            &request("isolation-a", ImsOperation::Insert, 2, &["ROOT"], b"A1X"),
        );
        let mut row = store
            .get_provider_state(GENERIC_PENDING_NAMESPACE, "isolation-a")
            .unwrap()
            .unwrap();
        let legacy = BTreeMap::from([("GENDB", before)]);
        row.payload = encode_object_row("isolation-a", &legacy).unwrap();
        let version = row.version;
        row.version += 1;
        store.put_provider_state(row, Some(version)).unwrap();
        // Reproduce retained pre-fix state after B committed past A's undo.
        let mut live = store
            .get_provider_state(GENERIC_DATABASE_NAMESPACE, "GENDB")
            .unwrap()
            .unwrap();
        let row: ObjectRow<DatabaseEngineImage> = serde_json::from_slice(&live.payload).unwrap();
        let mut engine =
            DatabaseEngine::restore(row.value, engine_limits(ImsLimits::default())).unwrap();
        engine
            .insert(InsertRequest {
                segment: "ROOT".into(),
                parent: None,
                data: b"B2Y".to_vec(),
            })
            .unwrap();
        live.payload = encode_object_row("GENDB", &engine.image()).unwrap();
        let version = live.version;
        live.version += 1;
        store.put_provider_state(live, Some(version)).unwrap();
        let reopened = ImsService::open(store.clone(), ImsLimits::default()).unwrap();
        let before = rows(&*store);
        for op in [ImsOperation::Rollback, ImsOperation::Commit] {
            assert_eq!(
                reopened.execute(
                    &invocation("isolation-a"),
                    &request("isolation-a", op, 3, &[], b"")
                ),
                Err(HostProblem::UnknownOutcome)
            );
        }
        assert_eq!(rows(&*store), before);
        assert_eq!(
            restored(
                &reopened.lock().unwrap().state,
                "GENDB",
                ImsLimits::default()
            )
            .unwrap()
            .record_count(),
            2
        );
    }
}

#[test]
fn local_uow_witness_mismatch_and_malformed_schema_fail_closed() {
    for store in backends() {
        let service = installed(store.clone());
        execute(
            &service,
            "isolation-a",
            &request("isolation-a", ImsOperation::Insert, 2, &["ROOT"], b"A1X"),
        );
        let original = store
            .get_provider_state(GENERIC_PENDING_NAMESPACE, "isolation-a")
            .unwrap()
            .unwrap();
        let mut row = original.clone();
        let mut json: serde_json::Value = serde_json::from_slice(&row.payload).unwrap();
        json["value"]["post_images"]["GENDB"] = serde_json::to_value([0u8; 32]).unwrap();
        json["value"]["owned_images"]["GENDB"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::to_value([0u8; 32]).unwrap());
        row.payload = serde_json::to_vec(&json).unwrap();
        row.version += 1;
        store
            .put_provider_state(row.clone(), Some(original.version))
            .unwrap();
        let reopened = ImsService::open(store.clone(), ImsLimits::default()).unwrap();
        let before = rows(&*store);
        assert_eq!(
            reopened.execute(
                &invocation("isolation-a"),
                &request("isolation-a", ImsOperation::Rollback, 3, &[], b"")
            ),
            Err(HostProblem::UnknownOutcome)
        );
        assert_eq!(rows(&*store), before);
        for mutation in ["schema", "missing", "extra"] {
            let mut json: serde_json::Value = serde_json::from_slice(&original.payload).unwrap();
            match mutation {
                "schema" => {
                    json["value"]["schema_version"] =
                        serde_json::json!("mainframe-env.ims-local-undo@99")
                }
                "missing" => {
                    json["value"].as_object_mut().unwrap().remove("post_images");
                }
                _ => json["value"]["ignored"] = serde_json::json!(true),
            }
            let version = row.version;
            row.version += 1;
            row.payload = serde_json::to_vec(&json).unwrap();
            store
                .put_provider_state(row.clone(), Some(version))
                .unwrap();
            assert!(matches!(
                ImsService::open(store.clone(), ImsLimits::default()),
                Err(HostProblem::InfrastructureFailure)
            ));
        }
    }
}

struct InterceptStore {
    inner: Arc<dyn ProviderStateStore>,
    mode: AtomicU8,
    entered: Barrier,
    release: Barrier,
}

impl InterceptStore {
    fn new(inner: Arc<dyn ProviderStateStore>) -> Arc<Self> {
        Arc::new(Self {
            inner,
            mode: AtomicU8::new(0),
            entered: Barrier::new(2),
            release: Barrier::new(2),
        })
    }
}

impl AuditSink for InterceptStore {
    fn record_audit(&self, record: AuditRecord) -> Result<(), StoreError> {
        self.inner.record_audit(record)
    }
    fn audit_records(
        &self,
        execution_id: &ExecutionId,
        start: u64,
        max: usize,
    ) -> Result<Vec<AuditRecord>, StoreError> {
        self.inner.audit_records(execution_id, start, max)
    }
}

impl ProviderStateStore for InterceptStore {
    fn get_provider_state(
        &self,
        namespace: &str,
        key: &str,
    ) -> Result<Option<ProviderStateRecord>, StoreError> {
        self.inner.get_provider_state(namespace, key)
    }
    fn list_provider_state(
        &self,
        namespace: &str,
        max: usize,
    ) -> Result<Vec<ProviderStateRecord>, StoreError> {
        self.inner.list_provider_state(namespace, max)
    }
    fn put_provider_state(
        &self,
        record: ProviderStateRecord,
        expected: Option<u64>,
    ) -> Result<(), StoreError> {
        self.inner.put_provider_state(record, expected)
    }
    fn delete_provider_state(
        &self,
        namespace: &str,
        key: &str,
        expected: u64,
    ) -> Result<(), StoreError> {
        self.inner.delete_provider_state(namespace, key, expected)
    }
    fn move_provider_state(
        &self,
        record: ProviderStateRecord,
        key: &str,
        expected: u64,
    ) -> Result<(), StoreError> {
        self.inner.move_provider_state(record, key, expected)
    }
    fn put_provider_states_atomic(
        &self,
        writes: Vec<ProviderStateWrite>,
    ) -> Result<(), StoreError> {
        self.inner.put_provider_states_atomic(writes)
    }
    fn mutate_provider_states_atomic(
        &self,
        mutations: Vec<ProviderStateMutation>,
    ) -> Result<(), StoreError> {
        let mode = if mutations.iter().any(|mutation| matches!(mutation, ProviderStateMutation::Put(write) if write.record.namespace == GENERIC_DATABASE_NAMESPACE)) {
            self.mode.swap(0, AtomicOrdering::SeqCst)
        } else { 0 };
        if mode == 1 {
            self.entered.wait();
            self.release.wait();
        }
        if mode == 2 {
            return Err(StoreError::CapacityExceeded);
        }
        self.inner.mutate_provider_states_atomic(mutations)?;
        if mode == 3 {
            return Err(StoreError::Infrastructure("lost acknowledgement".into()));
        }
        Ok(())
    }
}

#[test]
fn local_uow_concurrent_first_writers_have_one_atomic_cas_winner() {
    for inner in backends() {
        let store = InterceptStore::new(inner);
        let first = installed(store.clone());
        let second = ImsService::open(store.clone(), ImsLimits::default()).unwrap();
        store.mode.store(1, AtomicOrdering::SeqCst);
        let thread = std::thread::spawn(move || {
            first.execute(
                &invocation("isolation-a"),
                &request("isolation-a", ImsOperation::Insert, 2, &["ROOT"], b"A1X"),
            )
        });
        store.entered.wait();
        execute(
            &second,
            "isolation-b",
            &request("isolation-b", ImsOperation::Insert, 2, &["ROOT"], b"B2Y"),
        );
        execute(
            &second,
            "isolation-b",
            &request("isolation-b", ImsOperation::Commit, 3, &[], b""),
        );
        store.release.wait();
        assert_eq!(
            thread.join().unwrap(),
            Err(HostProblem::IdempotencyConflict)
        );
        assert!(
            store
                .get_provider_state(GENERIC_PENDING_NAMESPACE, "isolation-a")
                .unwrap()
                .is_none()
        );
        assert!(
            store
                .get_provider_state(REPLAY_NAMESPACE, "isolation-a-2")
                .unwrap()
                .is_none()
        );
        let reopened = ImsService::open(store, ImsLimits::default()).unwrap();
        execute(
            &reopened,
            "isolation-a",
            &request("isolation-a", ImsOperation::Rollback, 3, &[], b""),
        );
        assert_eq!(
            restored(
                &reopened.lock().unwrap().state,
                "GENDB",
                ImsLimits::default()
            )
            .unwrap()
            .export_records()[0]
                .data,
            b"B2Y"
        );
    }
}

#[test]
fn local_uow_failure_is_atomic_and_lost_ack_replays_without_redispatch() {
    for inner in backends() {
        let store = InterceptStore::new(inner);
        let service = installed(store.clone());
        let req = request("isolation-a", ImsOperation::Insert, 2, &["ROOT"], b"A1X");
        let before = rows(&*store);
        store.mode.store(2, AtomicOrdering::SeqCst);
        assert_eq!(
            service.execute(&invocation("isolation-a"), &req),
            Err(HostProblem::ResourceExhausted)
        );
        assert_eq!(rows(&*store), before);
        store.mode.store(3, AtomicOrdering::SeqCst);
        assert_eq!(
            service.execute(&invocation("isolation-a"), &req),
            Err(HostProblem::UnknownOutcome)
        );
        let reopened = ImsService::open(store.clone(), ImsLimits::default()).unwrap();
        assert_eq!(execute(&reopened, "isolation-a", &req).affected_segments, 1);
        let stable = rows(&*store);
        assert_eq!(execute(&reopened, "isolation-a", &req).affected_segments, 1);
        assert_eq!(rows(&*store), stable);
        let rollback = request("isolation-a", ImsOperation::Rollback, 3, &[], b"");
        store.mode.store(3, AtomicOrdering::SeqCst);
        assert_eq!(
            reopened.execute(&invocation("isolation-a"), &rollback),
            Err(HostProblem::UnknownOutcome)
        );
        let final_service = ImsService::open(store, ImsLimits::default()).unwrap();
        assert_eq!(
            execute(&final_service, "isolation-a", &rollback).status,
            "  "
        );
        assert_eq!(
            restored(
                &final_service.lock().unwrap().state,
                "GENDB",
                ImsLimits::default()
            )
            .unwrap()
            .record_count(),
            0
        );
    }
}

#[test]
fn local_uow_survives_fresh_sqlite_connection_and_releases_on_backout() {
    let file = std::env::temp_dir().join(format!(
        "ims-local-uow-{}-{}.sqlite",
        std::process::id(),
        NEXT_FILE.fetch_add(1, Ordering::Relaxed)
    ));
    let url = format!("sqlite://{}?mode=rwc", file.display());
    {
        let first_store: Arc<dyn ProviderStateStore> =
            Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap());
        let first = installed(first_store);
        execute(
            &first,
            "isolation-a",
            &request("isolation-a", ImsOperation::Insert, 2, &["ROOT"], b"A1X"),
        );
    }
    {
        let store: Arc<dyn ProviderStateStore> =
            Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap());
        let second = ImsService::open(store, ImsLimits::default()).unwrap();
        let req = request("isolation-b", ImsOperation::Insert, 2, &["ROOT"], b"B2Y");
        assert_eq!(
            second.execute(&invocation("isolation-b"), &req),
            Err(HostProblem::IdempotencyConflict)
        );
        execute(
            &second,
            "isolation-a",
            &request("isolation-a", ImsOperation::Rollback, 3, &[], b""),
        );
        execute(&second, "isolation-b", &req);
        execute(
            &second,
            "isolation-b",
            &request("isolation-b", ImsOperation::Commit, 3, &[], b""),
        );
    }
    {
        let store: Arc<dyn ProviderStateStore> =
            Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap());
        let last = ImsService::open(store, ImsLimits::default()).unwrap();
        assert_eq!(
            restored(&last.lock().unwrap().state, "GENDB", ImsLimits::default())
                .unwrap()
                .export_records()[0]
                .data,
            b"B2Y"
        );
    }
    std::fs::remove_file(file).unwrap();
}

#[test]
fn local_uow_stale_rollback_cas_cannot_claim_a_committed_backout() {
    for inner in backends() {
        let store = InterceptStore::new(inner);
        let first = installed(store.clone());
        execute(
            &first,
            "isolation-a",
            &request("isolation-a", ImsOperation::Insert, 2, &["ROOT"], b"A1X"),
        );
        let second = ImsService::open(store.clone(), ImsLimits::default()).unwrap();
        store.mode.store(1, AtomicOrdering::SeqCst);
        let thread = std::thread::spawn(move || {
            first.execute(
                &invocation("isolation-a"),
                &request("isolation-a", ImsOperation::Rollback, 3, &[], b""),
            )
        });
        store.entered.wait();
        execute(
            &second,
            "isolation-a",
            &request("isolation-a", ImsOperation::Commit, 4, &[], b""),
        );
        execute(
            &second,
            "isolation-b",
            &request("isolation-b", ImsOperation::Insert, 2, &["ROOT"], b"B2Y"),
        );
        execute(
            &second,
            "isolation-b",
            &request("isolation-b", ImsOperation::Commit, 3, &[], b""),
        );
        store.release.wait();
        assert_eq!(
            thread.join().unwrap(),
            Err(HostProblem::IdempotencyConflict)
        );
        assert!(
            store
                .get_provider_state(REPLAY_NAMESPACE, "isolation-a-3")
                .unwrap()
                .is_none()
        );
        let reopened = ImsService::open(store, ImsLimits::default()).unwrap();
        assert_eq!(
            restored(
                &reopened.lock().unwrap().state,
                "GENDB",
                ImsLimits::default()
            )
            .unwrap()
            .record_count(),
            2
        );
    }
}

#[test]
fn local_uow_logical_cascade_fences_every_affected_database() {
    for store in backends() {
        let service = ImsService::open(store.clone(), ImsLimits::default()).unwrap();
        closure_tests::setup(&service, true);
        closure_tests::insert_child(&service, 3);
        closure_tests::hold_parent(&service, 4);
        let mut delete = request("parent-run", ImsOperation::Delete, 5, &[], b"");
        delete.pcb = 2;
        let before = rows(&*store);
        assert_eq!(
            service.execute(&invocation("parent-run"), &delete),
            Err(HostProblem::IdempotencyConflict)
        );
        assert_eq!(rows(&*store), before);
        execute(
            &service,
            "child-run",
            &request("child-run", ImsOperation::Commit, 4, &[], b""),
        );
        assert_eq!(
            execute(&service, "parent-run", &delete).affected_segments,
            2
        );
        let stale = ImsService::open(store.clone(), ImsLimits::default()).unwrap();
        let insert = request("child-run", ImsOperation::Insert, 6, &["ROOT"], b"R2Z");
        let before = rows(&*store);
        assert_eq!(
            stale.execute(&invocation("child-run"), &insert),
            Err(HostProblem::IdempotencyConflict)
        );
        assert_eq!(rows(&*store), before);
        execute(
            &service,
            "parent-run",
            &request("parent-run", ImsOperation::Rollback, 6, &[], b""),
        );
        assert_eq!(execute(&stale, "child-run", &insert).affected_segments, 1);
    }
}

#[test]
fn local_uow_checkpoint_state_release_fences_an_unchanged_image() {
    // Exercise the manager-owned CHKP publication boundary without changing
    // its handler here: remove undo, reset position, retain a checkpoint.
    for store in backends() {
        let service = installed(store.clone());
        execute(
            &service,
            "isolation-a",
            &request("isolation-a", ImsOperation::Insert, 2, &["ROOT"], b"A1X"),
        );
        let before = store
            .get_provider_state(GENERIC_DATABASE_NAMESPACE, "GENDB")
            .unwrap()
            .unwrap();
        {
            let mut durable = service.lock().unwrap();
            let mut next = durable.state.scoped_snapshot();
            next.generic_pending_undo.remove("isolation-a");
            Arc::make_mut(next.sessions.get_mut("isolation-a").unwrap()).position =
                PcbPosition::default();
            next.checkpoints.insert(
                "CHK-isolation-a".into(),
                next.sessions["isolation-a"].clone(),
            );
            service.persist(&mut durable, next).unwrap();
        }
        let after = store
            .get_provider_state(GENERIC_DATABASE_NAMESPACE, "GENDB")
            .unwrap()
            .unwrap();
        assert_eq!(after.payload, before.payload);
        assert_eq!(after.version, before.version + 1);
        let second = ImsService::open(store, ImsLimits::default()).unwrap();
        execute(
            &second,
            "isolation-b",
            &request("isolation-b", ImsOperation::Insert, 2, &["ROOT"], b"B2Y"),
        );
        execute(
            &second,
            "isolation-b",
            &request("isolation-b", ImsOperation::Commit, 3, &[], b""),
        );
        execute(
            &service,
            "isolation-a",
            &request("isolation-a", ImsOperation::Rollback, 3, &[], b""),
        );
        assert_eq!(
            restored(
                &service.lock().unwrap().state,
                "GENDB",
                ImsLimits::default()
            )
            .unwrap()
            .record_count(),
            2
        );
    }
}
