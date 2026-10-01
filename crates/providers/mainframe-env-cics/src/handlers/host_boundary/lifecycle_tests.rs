//! Test-only interleavings for the existing CICS task/session cleanup owner.
#![cfg(test)]

use super::*;
use mainframe_env_execution_api::AuditRecord;
use mainframe_env_store::MemoryStore;
use mainframe_env_store_api::{AuditSink, ProviderStateMutation, ProviderStateWrite};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

struct CleaningStore {
    inner: MemoryStore,
    pause: AtomicBool,
    fail: bool,
    entered: Sender<()>,
    release: Mutex<Receiver<()>>,
}

impl AuditSink for CleaningStore {
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

impl ProviderStateStore for CleaningStore {
    fn advance_logical_clock(&self, floor: u64) -> Result<u64, StoreError> {
        self.inner.advance_logical_clock(floor)
    }

    fn get_provider_state(
        &self,
        namespace: &str,
        key: &str,
    ) -> Result<Option<ProviderStateRecord>, StoreError> {
        if namespace == "cics-bts-browse-v1" && self.pause.swap(false, Ordering::SeqCst) {
            self.entered.send(()).unwrap();
            self.release
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(10))
                .unwrap();
            if self.fail {
                return Err(StoreError::Infrastructure(
                    "injected-cleanup-read-failure".into(),
                ));
            }
        }
        self.inner.get_provider_state(namespace, key)
    }

    fn list_provider_state(
        &self,
        namespace: &str,
        max: usize,
    ) -> Result<Vec<ProviderStateRecord>, StoreError> {
        self.inner.list_provider_state(namespace, max)
    }

    fn list_provider_state_prefix(
        &self,
        namespace: &str,
        max: usize,
    ) -> Result<Vec<ProviderStateRecord>, StoreError> {
        self.inner.list_provider_state_prefix(namespace, max)
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
        self.inner.mutate_provider_states_atomic(mutations)
    }
}

#[derive(Clone, Copy, Debug)]
enum Cleanup {
    Complete,
    Abort,
    Suspend,
    Discard,
    Disconnect,
    Expire,
}

fn cleanup_interleaving(operation: Cleanup, fail: bool) {
    let (entered, observed) = channel();
    let (resume, release) = channel();
    let store = Arc::new(CleaningStore {
        inner: MemoryStore::new(Default::default()),
        pause: AtomicBool::new(false),
        fail,
        entered,
        release: Mutex::new(release),
    });
    let service = crate::service::tests::service(store.clone());
    let root = crate::service::tests::invocation();
    let session = SessionId::new("cleanup-session", 64).unwrap();
    service
        .launch_terminal(
            root.clone(),
            &session,
            "MENU",
            24,
            80,
            "cleanup-csrf",
            1,
            10_000,
        )
        .unwrap();
    store.pause.store(true, Ordering::SeqCst);
    let worker_service = service.clone();
    let worker_session = session.clone();
    let principal = root.principal.id().clone();
    let worker = std::thread::spawn(move || match operation {
        Cleanup::Complete => worker_service.complete_terminal_run(&worker_session, &principal, 2),
        Cleanup::Abort => worker_service.abort_terminal_run(&worker_session, &principal, 2),
        Cleanup::Suspend => worker_service.suspend_terminal_run(&worker_session, &principal, 2),
        Cleanup::Discard => worker_service
            .discard_terminal_run_if_present(&worker_session, &principal, 2)
            .map(|_| ()),
        Cleanup::Disconnect => {
            worker_service.disconnect_terminal(&worker_session, &principal, "cleanup-csrf", 2)
        }
        Cleanup::Expire => worker_service
            .terminal_snapshot(&worker_session, &principal, 10_001)
            .map(|_| ()),
    });
    observed.recv_timeout(Duration::from_secs(10)).unwrap();
    // Cleanup is now outside the state lock and inside its first provider read.
    let before = store
        .inner
        .get_provider_state("cics-session", session.as_str())
        .unwrap();
    let admission = service.ensure_run(&root);
    let dispatch_blocked = matches!(
        CommandLease::acquire(&service, &root.run_unit_id),
        Err(HostProblem::IdempotencyConflict)
    );
    let input = service.submit_input(&session, 0xf1, &BTreeMap::new());
    let registration = service.register_run(root.clone(), &session, "MENU", "ME01", "S001");
    let restoration = service.restore_terminal_run(root.clone(), &session, "MENU", Vec::new(), 2);
    let duplicate = service.disconnect_terminal(&session, root.principal.id(), "cleanup-csrf", 2);
    let unchanged = store
        .inner
        .get_provider_state("cics-session", session.as_str())
        .unwrap()
        == before;
    // Always release the worker before assertions, including on the old broken runtime.
    resume.send(()).unwrap();
    let outcome = worker.join().unwrap();
    for result in [admission, input, registration, restoration, duplicate] {
        assert_eq!(
            result,
            Err(HostProblem::IdempotencyConflict),
            "{operation:?}, fail={fail}"
        );
    }
    assert!(dispatch_blocked, "{operation:?}, fail={fail}");
    assert!(
        unchanged,
        "concurrent entry changed the session during {operation:?}"
    );
    let state = service.lock().unwrap();
    assert!(state.task_dispatch.cleaning_sessions.is_empty());
    assert!(state.task_dispatch.claims.is_empty());
    if fail {
        assert_eq!(outcome, Err(HostProblem::InfrastructureFailure));
        assert_eq!(state.runs.len(), 1);
        drop(state);
        service.ensure_run(&root).unwrap();
        assert!(
            service
                .terminal_snapshot(&session, root.principal.id(), 2)
                .is_ok()
        );
    } else {
        assert_eq!(
            outcome,
            if matches!(operation, Cleanup::Expire) {
                Err(HostProblem::TimedOut)
            } else {
                Ok(())
            }
        );
        assert!(state.runs.is_empty());
    }
}

#[test]
fn terminal_cleanup_excludes_commands_across_provider_boundary() {
    for operation in [
        Cleanup::Complete,
        Cleanup::Abort,
        Cleanup::Suspend,
        Cleanup::Discard,
        Cleanup::Disconnect,
        Cleanup::Expire,
    ] {
        cleanup_interleaving(operation, false);
    }
}

#[test]
fn terminal_cleanup_failure_releases_exclusive_session_lease() {
    for operation in [
        Cleanup::Complete,
        Cleanup::Abort,
        Cleanup::Suspend,
        Cleanup::Discard,
        Cleanup::Disconnect,
        Cleanup::Expire,
    ] {
        cleanup_interleaving(operation, true);
    }
}
