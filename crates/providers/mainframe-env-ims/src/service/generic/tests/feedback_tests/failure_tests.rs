use super::*;
use mainframe_env_execution_api::AuditRecord;
use mainframe_env_store_api::AuditSink;
use std::sync::atomic::{AtomicU8, Ordering as AtomicOrdering};

pub(super) fn backends() -> Vec<Arc<dyn ProviderStateStore>> {
    let directory =
        std::env::temp_dir().join(format!("ims-pcb-feedback-v1-tests-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let file = directory.join(format!(
        "{}.sqlite",
        NEXT_FILE.fetch_add(1, Ordering::Relaxed)
    ));
    let url = format!("sqlite:{}?mode=rwc", file.display());
    vec![
        Arc::new(MemoryStore::new(Default::default())),
        Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262144).unwrap()),
    ]
}

struct Intercept {
    inner: Arc<dyn ProviderStateStore>,
    mode: AtomicU8,
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
        r: ProviderStateRecord,
        expected: Option<u64>,
    ) -> Result<(), StoreError> {
        self.inner.put_provider_state(r, expected)
    }
    fn delete_provider_state(&self, ns: &str, key: &str, expected: u64) -> Result<(), StoreError> {
        self.inner.delete_provider_state(ns, key, expected)
    }
    fn move_provider_state(
        &self,
        r: ProviderStateRecord,
        key: &str,
        expected: u64,
    ) -> Result<(), StoreError> {
        self.inner.move_provider_state(r, key, expected)
    }
    fn put_provider_states_atomic(&self, w: Vec<ProviderStateWrite>) -> Result<(), StoreError> {
        self.inner.put_provider_states_atomic(w)
    }
    fn mutate_provider_states_atomic(
        &self,
        m: Vec<ProviderStateMutation>,
    ) -> Result<(), StoreError> {
        let mode = self.mode.swap(0, AtomicOrdering::SeqCst);
        if mode == 1 {
            return Err(StoreError::CapacityExceeded);
        }
        self.inner.mutate_provider_states_atomic(m)?;
        if mode == 2 {
            return Err(StoreError::Infrastructure("lost acknowledgement".into()));
        }
        Ok(())
    }
}

fn rows(store: &dyn ProviderStateStore) -> Vec<ProviderStateRecord> {
    [
        GENERIC_DATABASE_NAMESPACE,
        GENERIC_PENDING_NAMESPACE,
        SESSION_NAMESPACE,
        REPLAY_NAMESPACE,
    ]
    .into_iter()
    .flat_map(|n| store.list_provider_state(n, 4096).unwrap())
    .collect()
}

#[test]
fn public_feedback_atomic_capacity_lost_ack_and_actual_session_cas() {
    for inner in backends() {
        let run = "feedback-atomic";
        let store = Arc::new(Intercept {
            inner,
            mode: AtomicU8::new(0),
        });
        let service = seed(store.clone(), run, false, false);
        let insert = feedback(run, 2, ImsOperation::Insert, 1, &["ROOT"], b"C3Z");
        let before = rows(&*store);
        store.mode.store(1, AtomicOrdering::SeqCst);
        assert_eq!(
            public(service.clone(), run, insert.clone()),
            Err(HostProblem::ResourceExhausted)
        );
        assert_eq!(rows(&*store), before);
        store.mode.store(2, AtomicOrdering::SeqCst);
        assert_eq!(
            public(service, run, insert.clone()),
            Err(HostProblem::UnknownOutcome)
        );
        let reopened = ImsService::open(store.clone(), Default::default()).unwrap();
        let retained = public(reopened.clone(), run, insert.clone()).unwrap();
        assert_key(&retained, "ROOT", 1, b"C3", 0);
        public(
            reopened.clone(),
            run,
            feedback(run, 3, ImsOperation::GetUnique, 1, &["ROOT"], b""),
        )
        .unwrap();
        assert_eq!(public(reopened, run, insert).unwrap(), retained);

        let cas = super::super::session_cas::SessionCasStore::new(store.clone(), run);
        let stale = ImsService::open(cas.clone(), Default::default()).unwrap();
        let before = store
            .get_provider_state(GENERIC_DATABASE_NAMESPACE, "GENDB")
            .unwrap();
        cas.arm();
        assert_eq!(
            public(
                stale,
                run,
                feedback(run, 4, ImsOperation::GetUnique, 1, &["CHILD"], b"")
            ),
            Err(HostProblem::IdempotencyConflict)
        );
        assert_eq!(
            store
                .get_provider_state(GENERIC_DATABASE_NAMESPACE, "GENDB")
                .unwrap(),
            before
        );
        assert!(
            store
                .get_provider_state(REPLAY_NAMESPACE, "feedback-atomic-4")
                .unwrap()
                .is_none()
        );
    }
}

struct Policy {
    deny: AtomicU8,
}
impl EnterpriseAuthorizer for Policy {
    fn authorize(&self, _: &PrincipalId, resource: &EnterpriseResource) -> Result<(), HostProblem> {
        if resource.class == EnterpriseResourceClass::ImsDatabase {
            match self.deny.load(AtomicOrdering::SeqCst) {
                1 => return Err(HostProblem::Unauthorized),
                2 => return Err(HostProblem::InfrastructureFailure),
                _ => {}
            }
        }
        Ok(())
    }
}
#[test]
fn public_feedback_authorization_hides_live_and_retained_feedback_before_observation() {
    for store in backends() {
        let run = "feedback-saf";
        let seeded = seed(store.clone(), run, false, false);
        drop(seeded);
        let policy = Arc::new(Policy {
            deny: AtomicU8::new(0),
        });
        let service =
            ImsService::open_authorized(store, Default::default(), policy.clone()).unwrap();
        let req = feedback(run, 2, ImsOperation::GetHoldUnique, 2, &["CHILD"], b"");
        let expected = public(service.clone(), run, req.clone()).unwrap();
        assert_key(&expected, "CHILD", 2, b"A1C1", 3);
        let before = snapshot(&service);
        for (mode, error) in [
            (1, HostProblem::Unauthorized),
            (2, HostProblem::InfrastructureFailure),
        ] {
            policy.deny.store(mode, AtomicOrdering::SeqCst);
            assert_eq!(
                public(service.clone(), run, req.clone()),
                Err(error.clone())
            );
            assert_eq!(
                public(
                    service.clone(),
                    run,
                    feedback(run, 3, ImsOperation::Insert, 1, &["ROOT"], b"C3Z")
                ),
                Err(error)
            );
            assert_eq!(snapshot(&service), before);
        }
        policy.deny.store(0, AtomicOrdering::SeqCst);
        assert_eq!(public(service, run, req).unwrap(), expected);
    }
}

#[test]
fn public_feedback_process_child() {
    let Ok(url) = std::env::var("IMS_PCB_FEEDBACK_CHILD_V1") else {
        return;
    };
    let run = "feedback-process";
    let store = Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262144).unwrap());
    let first = feedback(run, 2, ImsOperation::GetUnique, 1, &["CHILD"], b"");
    if std::env::var("IMS_PCB_FEEDBACK_PHASE_V1").unwrap() == "seed" {
        let service = seed(store, run, false, false);
        assert_key(
            &public(service.clone(), run, first).unwrap(),
            "CHILD",
            2,
            b"A1C1",
            3,
        );
        public(
            service.clone(),
            run,
            feedback(run, 3, ImsOperation::GetHoldUnique, 1, &["CHILD"], b""),
        )
        .unwrap();
        public(
            service.clone(),
            run,
            feedback(run, 4, ImsOperation::Replace, 1, &[], b"C1Q"),
        )
        .unwrap();
        execute(
            &service,
            run,
            &request(run, ImsOperation::Commit, 5, &[], b""),
        );
    } else {
        let service = ImsService::open(store, Default::default()).unwrap();
        let old = public(service.clone(), run, first).unwrap();
        assert_key(&old, "CHILD", 2, b"A1C1", 3);
        assert_eq!(old.result.segments[0].data, b"C1Z");
        let current = public(
            service,
            run,
            feedback(run, 6, ImsOperation::GetUnique, 1, &["CHILD"], b""),
        )
        .unwrap();
        assert_key(&current, "CHILD", 2, b"A1C1", 3);
        assert_eq!(current.result.segments[0].data, b"C1Q");
    }
}

#[test]
fn public_feedback_sqlite_separate_process_replays_feedback_after_seed_process_exits() {
    let file = std::env::temp_dir().join(format!(
        "ims-feedback-process-{}-{}.sqlite",
        std::process::id(),
        NEXT_FILE.fetch_add(1, Ordering::Relaxed)
    ));
    let url = format!("sqlite:{}?mode=rwc", file.display());
    for phase in ["seed", "reopen"] {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "service::generic::tests::feedback_tests::failure_tests::public_feedback_process_child", "--nocapture"])
            .env("IMS_PCB_FEEDBACK_CHILD_V1", &url).env("IMS_PCB_FEEDBACK_PHASE_V1", phase).output().unwrap();
        assert!(
            output.status.success(),
            "{phase}: {} {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
    }
    std::fs::remove_file(file).unwrap();
}

#[test]
fn public_feedback_receipt_corruption_and_historical_omission_are_fail_closed() {
    for store in backends() {
        let run = "feedback-corruption";
        let service = seed(store.clone(), run, false, false);
        // Existing receipt bytes retain the absent additive field.
        let old = store
            .get_provider_state(REPLAY_NAMESPACE, "feedback-corruption-1")
            .unwrap()
            .unwrap();
        let old_json: serde_json::Value = serde_json::from_slice(&old.payload).unwrap();
        assert!(old_json["value"].get("pcb_feedback_v1").is_none());
        public(
            service.clone(),
            run,
            feedback(run, 2, ImsOperation::GetUnique, 1, &["CHILD"], b""),
        )
        .unwrap();
        drop(service);
        let mut row = store
            .get_provider_state(REPLAY_NAMESPACE, "feedback-corruption-2")
            .unwrap()
            .unwrap();
        let mut json: serde_json::Value = serde_json::from_slice(&row.payload).unwrap();
        json["value"]["pcb_feedback_v1"]["transferred_data_length"] = serde_json::json!(999);
        row.payload = serde_json::to_vec(&json).unwrap();
        let version = row.version;
        row.version += 1;
        store.put_provider_state(row, Some(version)).unwrap();
        assert!(matches!(
            ImsService::open(store, Default::default()),
            Err(HostProblem::ResourceExhausted)
        ));
    }
}

#[test]
fn public_feedback_receipt_with_two_result_families_is_rejected() {
    for store in backends() {
        let run = "feedback-ambiguous";
        let service = seed(store.clone(), run, false, false);
        public(
            service.clone(),
            run,
            feedback(run, 2, ImsOperation::GetUnique, 1, &["CHILD"], b""),
        )
        .unwrap();
        drop(service);
        let mut row = store
            .get_provider_state(REPLAY_NAMESPACE, "feedback-ambiguous-2")
            .unwrap()
            .unwrap();
        let mut json: serde_json::Value = serde_json::from_slice(&row.payload).unwrap();
        // The feedback digest remains valid. A second result family is nevertheless corrupt.
        json["value"]["gsam"] = serde_json::json!({ "address": null });
        row.payload = serde_json::to_vec(&json).unwrap();
        let version = row.version;
        row.version += 1;
        store.put_provider_state(row, Some(version)).unwrap();
        assert!(matches!(
            ImsService::open(store, Default::default()),
            Err(HostProblem::ResourceExhausted)
        ));
    }
}
