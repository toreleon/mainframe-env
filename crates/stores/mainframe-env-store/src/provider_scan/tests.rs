//! Bounded physical observations only, not JES/Core admission.
use super::*;
use mainframe_env_store_api::{ProviderStateRecord, ProviderStateStore};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

pub(crate) fn record(key: &str, payload: &[u8]) -> ProviderStateRecord {
    ProviderStateRecord {
        namespace: "page".into(),
        key: key.into(),
        version: 1,
        payload: payload.into(),
    }
}

pub(crate) fn pages(store: &dyn ProviderStateStore) {
    for row in [
        record("z", b"last"),
        record("a", b"first"),
        record("é", b"utf8"),
    ] {
        store.put_provider_state(row, None).unwrap();
    }
    let mut foreign = record("a", &[42; 100]);
    foreign.namespace = "page-other".into();
    store.put_provider_state(foreign, None).unwrap();
    let epoch = store.provider_state_retention_epoch().unwrap();
    let clock = store.advance_logical_clock(73).unwrap();
    let all = store.list_provider_state_prefix("", 20);
    assert!(all.is_err()); // existing prefix policy remains exact
    let expected = vec![
        record("a", b"first"),
        record("z", b"last"),
        record("é", b"utf8"),
    ];
    // 4-byte namespaces + keys 1/1/2 + bodies 5/4/4 = 29 bytes.
    assert_eq!(
        store.list_provider_state_bounded("page", 4, 29).unwrap(),
        expected
    );
    assert_eq!(store.list_provider_state("page", 4).unwrap(), expected);
    assert_eq!(
        store.list_provider_state_bounded("page", 2, 19).unwrap(),
        expected[..2]
    );
    assert_eq!(
        store.list_provider_state_bounded("page", 3, 28),
        Err(StoreError::CapacityExceeded)
    );
    assert_eq!(
        store.list_provider_state_bounded("page", 2, 18),
        Err(StoreError::CapacityExceeded)
    );
    assert!(
        store
            .list_provider_state_bounded("absent", 1, 1)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        store.list_provider_state_bounded("page", 1, 10).unwrap(),
        expected[..1]
    );
    for (max, bytes) in [
        (0, 1),
        (MAX_PROVIDER_STATE_SCAN + 1, 1),
        (1, 0),
        (1, MAX_PAGE_BYTES + 1),
        (usize::MAX, usize::MAX),
    ] {
        assert_eq!(
            store.list_provider_state_bounded("page", max, bytes),
            Err(StoreError::CapacityExceeded)
        );
    }
    for namespace in [String::new(), "x".repeat(MAX_PROVIDER_NAMESPACE_BYTES + 1)] {
        assert_eq!(
            store.list_provider_state_bounded(&namespace, 1, 1),
            Err(StoreError::IncompatibleVersion)
        );
    }
    // Valid global maxima are not rejected merely because this namespace is small.
    assert_eq!(
        store
            .list_provider_state_bounded("page", MAX_PROVIDER_STATE_SCAN, MAX_PAGE_BYTES)
            .unwrap(),
        expected
    );
    assert_eq!(store.provider_state_retention_epoch().unwrap(), epoch);
    assert_eq!(store.advance_logical_clock(1).unwrap(), clock);
    assert_eq!(store.list_provider_state("page", 4).unwrap(), expected);
    assert_eq!(store.list_provider_state("page-other", 4).unwrap().len(), 1);
}

pub(crate) struct OwnedDirectory(pub(crate) PathBuf);
impl OwnedDirectory {
    pub(crate) fn create() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "mq-bounded-page-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap(); // exclusive ownership; never reuse
        Self(std::fs::canonicalize(path).unwrap())
    }
    pub(crate) fn url(&self) -> String {
        format!("sqlite://{}?mode=rwc", self.0.join("owned.db").display())
    }
}
impl Drop for OwnedDirectory {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn bounded_charge_overflow_refuses() {
    let mut maximum = usize::MAX;
    assert_eq!(
        charge(&mut maximum, 1, 0, 0, MAX_PAGE_BYTES),
        Err(StoreError::CapacityExceeded)
    );
    assert_eq!(
        charge(&mut 0, usize::MAX, 1, 0, MAX_PAGE_BYTES),
        Err(StoreError::CapacityExceeded)
    );
}

struct Unsupported;
impl mainframe_env_store_api::AuditSink for Unsupported {
    fn record_audit(&self, _: mainframe_env_execution_api::AuditRecord) -> Result<(), StoreError> {
        panic!("must not audit")
    }
    fn audit_records(
        &self,
        _: &mainframe_env_execution_api::ExecutionId,
        _: u64,
        _: usize,
    ) -> Result<Vec<mainframe_env_execution_api::AuditRecord>, StoreError> {
        panic!("must not read audits")
    }
}
impl ProviderStateStore for Unsupported {
    fn get_provider_state(
        &self,
        _: &str,
        _: &str,
    ) -> Result<Option<ProviderStateRecord>, StoreError> {
        panic!("no read fallback")
    }
    fn list_provider_state(
        &self,
        _: &str,
        _: usize,
    ) -> Result<Vec<ProviderStateRecord>, StoreError> {
        panic!("no list fallback")
    }
    fn put_provider_state(&self, _: ProviderStateRecord, _: Option<u64>) -> Result<(), StoreError> {
        panic!("no write")
    }
    fn delete_provider_state(&self, _: &str, _: &str, _: u64) -> Result<(), StoreError> {
        panic!("no delete")
    }
    fn move_provider_state(
        &self,
        _: ProviderStateRecord,
        _: &str,
        _: u64,
    ) -> Result<(), StoreError> {
        panic!("no move")
    }
    fn put_provider_states_atomic(
        &self,
        _: Vec<mainframe_env_store_api::ProviderStateWrite>,
    ) -> Result<(), StoreError> {
        panic!("no batch")
    }
    fn mutate_provider_states_atomic(
        &self,
        _: Vec<mainframe_env_store_api::ProviderStateMutation>,
    ) -> Result<(), StoreError> {
        panic!("no mutation")
    }
}
#[test]
fn bounded_default_refuses_without_unrestricted_read_or_write_fallback() {
    assert_eq!(
        Unsupported.list_provider_state_bounded("page", 1, 100),
        Err(StoreError::InvalidTransition)
    );
}
