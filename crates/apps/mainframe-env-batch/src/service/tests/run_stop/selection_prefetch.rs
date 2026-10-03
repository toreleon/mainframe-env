//! Observe the private prepared-selection call; no joined/JES admission.
use super::*;
use mainframe_env_store::StoreLimits;

struct ReadBoundary {
    inner: Arc<dyn ProviderStateStore>,
    refuse: bool,
    calls: Mutex<Vec<(String, usize, usize)>>,
}
impl mainframe_env_store_api::AuditSink for ReadBoundary {
    fn record_audit(&self, row: AuditRecord) -> Result<(), StoreError> {
        self.inner.record_audit(row)
    }
    fn audit_records(
        &self,
        id: &ExecutionId,
        start: u64,
        max: usize,
    ) -> Result<Vec<AuditRecord>, StoreError> {
        self.inner.audit_records(id, start, max)
    }
}
impl ProviderStateStore for ReadBoundary {
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
    fn list_provider_state_prefix(
        &self,
        ns: &str,
        max: usize,
    ) -> Result<Vec<ProviderStateRecord>, StoreError> {
        self.inner.list_provider_state_prefix(ns, max)
    }
    fn list_provider_state_bounded(
        &self,
        ns: &str,
        max: usize,
        bytes: usize,
    ) -> Result<Vec<ProviderStateRecord>, StoreError> {
        self.calls.lock().unwrap().push((ns.into(), max, bytes));
        if self.refuse {
            Err(StoreError::InvalidTransition)
        } else {
            self.inner.list_provider_state_bounded(ns, max, bytes)
        }
    }
    fn put_provider_state(
        &self,
        row: ProviderStateRecord,
        expected: Option<u64>,
    ) -> Result<(), StoreError> {
        self.inner.put_provider_state(row, expected)
    }
    fn delete_provider_state(&self, ns: &str, key: &str, expected: u64) -> Result<(), StoreError> {
        self.inner.delete_provider_state(ns, key, expected)
    }
    fn move_provider_state(
        &self,
        row: ProviderStateRecord,
        old: &str,
        expected: u64,
    ) -> Result<(), StoreError> {
        self.inner.move_provider_state(row, old, expected)
    }
    fn put_provider_states_atomic(&self, rows: Vec<ProviderStateWrite>) -> Result<(), StoreError> {
        self.inner.put_provider_states_atomic(rows)
    }
    fn mutate_provider_states_atomic(
        &self,
        rows: Vec<mainframe_env_store_api::ProviderStateMutation>,
    ) -> Result<(), StoreError> {
        self.inner.mutate_provider_states_atomic(rows)
    }
    fn provider_state_retention_epoch(&self) -> Result<u64, StoreError> {
        self.inner.provider_state_retention_epoch()
    }
}

fn boundary(
    store: Arc<dyn ProviderStateStore>,
    checkpoints: Arc<dyn CheckpointStore>,
    refuse: bool,
) {
    let read = Arc::new(ReadBoundary {
        inner: store,
        refuse,
        calls: Mutex::new(Vec::new()),
    });
    let f = fixture(read.clone(), checkpoints);
    let id = submit(&f, "//TESTJOB JOB CLASS=A\n//STEP1 EXEC PGM=APPMAIN\n");
    let before = physical(&f, &id);
    let trace = f.trace.lock().unwrap().clone();
    let original = invocation();
    let mut observer = control;
    let scope = crate::service::run_stop::RunScope::new(&original, &mut observer);
    let plan = f
        .batch
        .prepare_selection(&scope, &id, "MEMBER1", "INIT0001");
    if refuse {
        assert!(matches!(plan, Err(HostProblem::InfrastructureFailure)));
        assert_eq!(physical(&f, &id), before);
        assert_eq!(*f.trace.lock().unwrap(), trace);
    } else {
        let plan = plan.unwrap();
        assert_eq!(
            plan.current_row(),
            &read
                .inner
                .get_provider_state("jes-job", &id)
                .unwrap()
                .unwrap()
        );
        assert_eq!(physical(&f, &id).rows, before.rows);
        assert_eq!(physical(&f, &id).checkpoint, before.checkpoint);
    }
    assert!(!read.calls.lock().unwrap().is_empty());
    for call in read.calls.lock().unwrap().iter() {
        assert_eq!(
            call,
            &(
                "jes-job".into(),
                f.batch.limits.max_jobs.min(4094) + 1,
                64 * 1024 * 1024
            )
        );
    }
}

#[test]
fn memory_selection_uses_bounded_page_and_refuses_unsupported_without_preflight() {
    for refuse in [false, true] {
        let store = Arc::new(MemoryStore::new(StoreLimits::default()));
        boundary(store.clone(), store, refuse);
    }
}
#[test]
fn sqlite_selection_uses_bounded_page_and_refuses_unsupported_without_preflight() {
    for refuse in [false, true] {
        let directory = OwnedDirectory::create();
        let store = directory.store("rwc");
        boundary(store.clone(), store, refuse);
    }
}
