//! Force a real session-version race after refresh, before atomic publication.
use super::*;
use mainframe_env_execution_api::{AuditRecord, ExecutionId};
use mainframe_env_store_api::AuditSink;
use std::sync::atomic::{AtomicBool, Ordering};

pub(super) struct SessionCasStore {
    inner: Arc<dyn ProviderStateStore>,
    run: String,
    armed: AtomicBool,
}

impl SessionCasStore {
    pub(super) fn new(inner: Arc<dyn ProviderStateStore>, run: &str) -> Arc<Self> {
        Arc::new(Self {
            inner,
            run: run.into(),
            armed: AtomicBool::new(false),
        })
    }

    pub(super) fn arm(&self) {
        self.armed.store(true, Ordering::SeqCst);
    }
}

impl AuditSink for SessionCasStore {
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

impl ProviderStateStore for SessionCasStore {
    fn advance_logical_clock(&self, floor: u64) -> Result<u64, StoreError> {
        self.inner.advance_logical_clock(floor)
    }

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
        if mutations.iter().any(|mutation| {
            matches!(mutation, ProviderStateMutation::Put(write)
                if write.record.namespace == SESSION_NAMESPACE && write.record.key == self.run)
        }) && self.armed.swap(false, Ordering::SeqCst)
        {
            let mut row = self
                .inner
                .get_provider_state(SESSION_NAMESPACE, &self.run)?
                .ok_or(StoreError::NotFound)?;
            let prior = row.version;
            row.version += 1;
            self.inner.put_provider_state(row, Some(prior))?;
        }
        self.inner.mutate_provider_states_atomic(mutations)
    }
}
