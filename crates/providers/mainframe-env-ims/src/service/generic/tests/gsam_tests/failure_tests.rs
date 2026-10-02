use super::*;
use mainframe_env_execution_api::AuditRecord;
use mainframe_env_store_api::AuditSink;
use std::sync::{
    Barrier,
    atomic::{AtomicU8, Ordering as AtomicOrdering},
};

pub(super) fn backends() -> Vec<Arc<dyn ProviderStateStore>> {
    vec![
        Arc::new(MemoryStore::new(Default::default())),
        Arc::new(SqliteStateStore::open("sqlite::memory:", 64 * 1024 * 1024, 262_144).unwrap()),
    ]
}
fn rows(store: &dyn ProviderStateStore) -> Vec<ProviderStateRecord> {
    [
        GENERIC_DATABASE_NAMESPACE,
        GENERIC_PENDING_NAMESPACE,
        REPLAY_NAMESPACE,
        SESSION_NAMESPACE,
    ]
    .into_iter()
    .flat_map(|ns| store.list_provider_state(ns, 4096).unwrap())
    .collect()
}
struct Intercept {
    inner: Arc<dyn ProviderStateStore>,
    mode: AtomicU8,
    entered: Barrier,
    release: Barrier,
}
impl Intercept {
    fn new(inner: Arc<dyn ProviderStateStore>) -> Arc<Self> {
        Arc::new(Self {
            inner,
            mode: AtomicU8::new(0),
            entered: Barrier::new(2),
            release: Barrier::new(2),
        })
    }
}
impl AuditSink for Intercept {
    fn record_audit(&self, record: AuditRecord) -> Result<(), StoreError> {
        self.inner.record_audit(record)
    }
    fn audit_records(
        &self,
        execution: &ExecutionId,
        start: u64,
        max: usize,
    ) -> Result<Vec<AuditRecord>, StoreError> {
        self.inner.audit_records(execution, start, max)
    }
}
impl ProviderStateStore for Intercept {
    fn get_provider_state(
        &self,
        ns: &str,
        key: &str,
    ) -> Result<Option<ProviderStateRecord>, StoreError> {
        self.inner.get_provider_state(ns, key)
    }
    fn list_provider_state(
        &self,
        ns: &str,
        max: usize,
    ) -> Result<Vec<ProviderStateRecord>, StoreError> {
        self.inner.list_provider_state(ns, max)
    }
    fn put_provider_state(
        &self,
        record: ProviderStateRecord,
        expected: Option<u64>,
    ) -> Result<(), StoreError> {
        self.inner.put_provider_state(record, expected)
    }
    fn delete_provider_state(&self, ns: &str, key: &str, expected: u64) -> Result<(), StoreError> {
        self.inner.delete_provider_state(ns, key, expected)
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
        let mode = self.mode.swap(0, AtomicOrdering::SeqCst);
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
fn public_gsam_atomic_failure_and_unknown_outcome_reconcile_by_exact_replay() {
    for inner in backends() {
        let run = "gsam-failure";
        let store = Intercept::new(inner);
        let service = installed(store.clone(), run);
        let insert = gsam(run, 2, ImsOperation::Insert, 2, b"A1X");
        let before = rows(&*store);
        store.mode.store(2, AtomicOrdering::SeqCst);
        assert_eq!(
            public(service.clone(), run, insert.clone()),
            Err(HostProblem::ResourceExhausted)
        );
        assert_eq!(rows(&*store), before);
        store.mode.store(3, AtomicOrdering::SeqCst);
        assert_eq!(
            public(service, run, insert.clone()),
            Err(HostProblem::UnknownOutcome)
        );
        let reopened = ImsService::open(store.clone(), ImsLimits::default()).unwrap();
        let committed = public(reopened.clone(), run, insert.clone()).unwrap();
        assert_eq!(committed.result.affected_segments, 1);
        assert!(committed.address.is_some());
        let before = rows(&*store);
        assert_eq!(public(reopened.clone(), run, insert).unwrap(), committed);
        assert_eq!(rows(&*store), before);
        let next = gsam(run, 3, ImsOperation::GetNext, 1, b"");
        store.mode.store(2, AtomicOrdering::SeqCst);
        assert_eq!(
            public(reopened.clone(), run, next.clone()),
            Err(HostProblem::ResourceExhausted)
        );
        assert_eq!(rows(&*store), before);
        store.mode.store(3, AtomicOrdering::SeqCst);
        assert_eq!(
            public(reopened, run, next.clone()),
            Err(HostProblem::UnknownOutcome)
        );
        let fresh = ImsService::open(store.clone(), ImsLimits::default()).unwrap();
        let result = public(fresh.clone(), run, next.clone()).unwrap();
        assert_eq!(result.address, committed.address);
        let before = rows(&*store);
        assert_eq!(public(fresh, run, next).unwrap(), result);
        assert_eq!(rows(&*store), before);
    }
}

#[test]
fn public_gsam_concurrent_cas_has_one_writer_and_no_loser_address_or_replay() {
    for inner in backends() {
        let store = Intercept::new(inner);
        let first = installed(store.clone(), "gsam-a");
        execute(
            &first,
            "gsam-b",
            &request("gsam-b", ImsOperation::Schedule, 1, &[], b""),
        );
        let second = ImsService::open(store.clone(), ImsLimits::default()).unwrap();
        store.mode.store(1, AtomicOrdering::SeqCst);
        let thread = std::thread::spawn(move || {
            public(
                first,
                "gsam-a",
                gsam("gsam-a", 2, ImsOperation::Insert, 2, b"A1X"),
            )
        });
        store.entered.wait();
        let winner = public(
            second.clone(),
            "gsam-b",
            gsam("gsam-b", 2, ImsOperation::Insert, 2, b"B2Y"),
        )
        .unwrap();
        execute(
            &second,
            "gsam-b",
            &request("gsam-b", ImsOperation::Commit, 3, &[], b""),
        );
        store.release.wait();
        assert_eq!(
            thread.join().unwrap(),
            Err(HostProblem::IdempotencyConflict)
        );
        assert!(
            store
                .get_provider_state(REPLAY_NAMESPACE, "gsam-a-2")
                .unwrap()
                .is_none()
        );
        assert!(
            store
                .get_provider_state(GENERIC_PENDING_NAMESPACE, "gsam-a")
                .unwrap()
                .is_none()
        );
        let fresh = ImsService::open(store, ImsLimits::default()).unwrap();
        assert_eq!(
            public(fresh, "gsam-a", gu("gsam-a", 3, 1, winner.address.unwrap()))
                .unwrap()
                .result
                .segments[0]
                .data,
            b"B2Y"
        );
    }
}

#[test]
fn public_gsam_retained_records_materialize_addresses_and_reject_corrupt_readers() {
    let run = "gsam-legacy";
    let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
    let service = installed(store.clone(), run);
    // An old engine record has no address field; old request/result encoding is retained.
    // Legacy route uses the broader metadata option authority; materialize through load.
    let image = ImsGenericLoadImage {
        database: "GENDB".into(),
        records: vec![ImsGenericLoadRecord {
            segment: "ROOT".into(),
            parent: None,
            data: b"A1X".to_vec(),
        }],
    };
    execute(
        &service,
        run,
        &request(
            run,
            ImsOperation::Load,
            2,
            &[],
            &serde_json::to_vec(&image).unwrap(),
        ),
    );
    execute(
        &service,
        run,
        &request(run, ImsOperation::Commit, 3, &[], b""),
    );
    let old_row = store
        .get_provider_state(GENERIC_DATABASE_NAMESPACE, "GENDB")
        .unwrap()
        .unwrap();
    assert!(!String::from_utf8_lossy(&old_row.payload).contains("gsam_address"));
    drop(service);
    let service = ImsService::open(store.clone(), ImsLimits::default()).unwrap();
    let address = public(
        service.clone(),
        run,
        gsam(run, 4, ImsOperation::GetNext, 1, b""),
    )
    .unwrap()
    .address
    .unwrap();
    assert_eq!(
        public(service.clone(), run, gu(run, 5, 1, address.clone()))
            .unwrap()
            .result
            .segments[0]
            .data,
        b"A1X"
    );
    execute(
        &service,
        run,
        &request(run, ImsOperation::Commit, 6, &[], b""),
    );
    // Reloading identical bytes must invalidate the old identity rather than revive it.
    execute(
        &service,
        run,
        &request(
            run,
            ImsOperation::Load,
            7,
            &[],
            &serde_json::to_vec(&image).unwrap(),
        ),
    );
    assert_eq!(
        public(service.clone(), run, gu(run, 8, 1, address))
            .unwrap()
            .result
            .status,
        "AJ"
    );
    execute(
        &service,
        run,
        &request(run, ImsOperation::Commit, 9, &[], b""),
    );
    let new_address = public(
        service.clone(),
        run,
        gsam(run, 10, ImsOperation::GetNext, 1, b""),
    )
    .unwrap()
    .address
    .unwrap();
    assert_eq!(
        public(service.clone(), run, gu(run, 11, 1, new_address))
            .unwrap()
            .result
            .status,
        "  "
    );
    drop(service);
    let mut row = store
        .get_provider_state(GENERIC_DATABASE_NAMESPACE, "GENDB")
        .unwrap()
        .unwrap();
    let expected = row.version;
    let mut payload: serde_json::Value = serde_json::from_slice(&row.payload).unwrap();
    payload["value"]["records"][0]["gsam_address"] = serde_json::to_value([0; 32]).unwrap();
    row.version += 1;
    row.payload = serde_json::to_vec(&payload).unwrap();
    store.put_provider_state(row, Some(expected)).unwrap();
    assert!(matches!(
        ImsService::open(store, ImsLimits::default()),
        Err(HostProblem::InfrastructureFailure)
    ));
}
