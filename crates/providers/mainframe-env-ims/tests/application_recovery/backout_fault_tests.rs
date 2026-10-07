use super::*;
use std::sync::{Barrier, atomic::AtomicBool};

struct RacingRows {
    inner: Arc<dyn TestStore>,
    enabled: AtomicBool,
    barrier: Barrier,
}

impl AuditSink for RacingRows {
    fn record_audit(
        &self,
        audit: mainframe_env_execution_api::AuditRecord,
    ) -> Result<(), StoreError> {
        self.inner.record_audit(audit)
    }
    fn audit_records(
        &self,
        id: &ExecutionId,
        start: u64,
        max: usize,
    ) -> Result<Vec<mainframe_env_execution_api::AuditRecord>, StoreError> {
        self.inner.audit_records(id, start, max)
    }
}
impl ProviderStateStore for RacingRows {
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
        row: ProviderStateRecord,
        version: Option<u64>,
    ) -> Result<(), StoreError> {
        self.inner.put_provider_state(row, version)
    }
    fn delete_provider_state(&self, ns: &str, key: &str, version: u64) -> Result<(), StoreError> {
        self.inner.delete_provider_state(ns, key, version)
    }
    fn move_provider_state(
        &self,
        row: ProviderStateRecord,
        key: &str,
        version: u64,
    ) -> Result<(), StoreError> {
        self.inner.move_provider_state(row, key, version)
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
        if self.enabled.load(Ordering::SeqCst)
            && mutations.iter().any(|m| {
                matches!(m,
            ProviderStateMutation::Put(w) if w.record.namespace == "ims-recovery-v1-session")
            })
        {
            self.barrier.wait();
        }
        // Neither writer is injected to fail: the real backend arbitrates their CAS.
        self.inner.mutate_provider_states_atomic(mutations)
    }
}

#[test]
fn backout_real_simultaneous_publications_have_one_backend_cas_winner() {
    backends("backout-race", |store| {
        let racing = Arc::new(RacingRows {
            inner: store.clone(),
            enabled: AtomicBool::new(false),
            barrier: Barrier::new(2),
        });
        let first = open(racing.clone());
        let invocation = invocation();
        seed(&first, &invocation);
        invoke(&first, &store, &invocation, 1, point(*b"RACE", b"r"));
        insert(&first, &invocation, 103, b"02B");
        let second =
            ImsService::open_authorized(racing.clone(), ImsLimits::default(), Arc::new(Allow))
                .unwrap();
        let a = call(2, rols(*b"RACE", 1));
        let b = call(3, ImsRecoveryCall::Rolb);
        intent(&*store, &invocation, &a);
        intent(&*store, &invocation, &b);
        racing.enabled.store(true, Ordering::SeqCst);
        let mut threads = vec![];
        for (service, request) in [(first, a.clone()), (second, b.clone())] {
            let store = store.clone();
            let invocation = invocation.clone();
            threads.push(std::thread::spawn(move || {
                dispatch(service, store, &invocation, &request)
            }));
        }
        let results = threads
            .into_iter()
            .map(|t| t.join().unwrap())
            .collect::<Vec<_>>();
        racing.enabled.store(false, Ordering::SeqCst);
        assert_eq!(
            results.iter().filter(|r| r.is_ok()).count(),
            1,
            "{results:?}"
        );
        assert_eq!(
            results
                .iter()
                .filter(|r| **r == Err(HostProblem::IdempotencyConflict))
                .count(),
            1,
            "{results:?}"
        );
        let service =
            ImsService::open_authorized(store.clone(), ImsLimits::default(), Arc::new(Allow))
                .unwrap();
        assert_eq!(data(&service, &invocation), vec![b"01A".to_vec()]);
        for (result, request) in results.iter().zip([a, b]) {
            assert_eq!(
                service
                    .observe_application_recovery(&invocation, &request)
                    .unwrap(),
                result.as_ref().ok().cloned()
            );
        }
    });
}

#[test]
fn backout_deadline_and_cancellation_reject_before_actual_image_publication() {
    struct Clock(std::sync::atomic::AtomicU64);
    impl ImsReplayClock for Clock {
        fn now_tick(&self) -> Result<u64, HostProblem> {
            Ok(self.0.load(Ordering::SeqCst))
        }
    }
    backends("backout-clock", |store| {
        let clock = Arc::new(Clock(std::sync::atomic::AtomicU64::new(2)));
        let service = ImsService::open_authorized_with_replay_clock(
            store.clone(),
            ImsLimits::default(),
            Arc::new(Allow),
            clock.clone(),
        )
        .unwrap();
        service.install_metadata(catalog()).unwrap();
        service
            .publish_metadata_generation("LOGAPP", 1, PACKAGE, Some(&catalog()))
            .unwrap();
        let mut invocation = invocation();
        seed(&service, &invocation);
        invoke(&service, &store, &invocation, 1, point(*b"LIVE", &[]));
        insert(&service, &invocation, 103, b"02B");
        let request = call(2, rols(*b"LIVE", 0));
        intent(&*store, &invocation, &request);
        let before = snapshot(&*store);
        clock.0.store(invocation.deadline_tick, Ordering::SeqCst);
        assert_eq!(
            dispatch(service.clone(), store.clone(), &invocation, &request),
            Err(HostProblem::TimedOut)
        );
        assert_eq!(snapshot(&*store), before);
        clock.0.store(2, Ordering::SeqCst);
        invocation.cancellation_probe = Some(mainframe_env_execution_api::CancellationProbe::new());
        invocation.cancellation_probe.as_ref().unwrap().request();
        assert_eq!(
            dispatch(service, store.clone(), &invocation, &request),
            Err(HostProblem::Cancelled)
        );
        assert_eq!(snapshot(&*store), before);
    });
}

#[test]
fn backout_recovery_rows_survive_replay_retention_and_unwitnessed_legacy_points_fail_closed() {
    use mainframe_env_store_api::{
        PlatformStore, RetentionPolicy, RetentionRequest, RetentionTarget,
    };
    fn exercise<S: PlatformStore + 'static>(store: Arc<S>) {
        let effects: Arc<dyn TestStore> = store.clone();
        let service = open(store.clone());
        let invocation = invocation();
        seed(&service, &invocation);
        invoke(&service, &effects, &invocation, 1, point(*b"KEEP", &[]));
        insert(&service, &invocation, 103, b"02B");
        let before = snapshot(&*store);
        let policy = RetentionPolicy {
            lifecycle_ticks: 1,
            idempotency_ticks: 1,
            audit_ticks: 1,
            archive_ticks: 1,
            low_watermark_percent: 70,
            high_watermark_percent: 85,
            max_batch: 64,
        };
        // Recovery-session replay is a separate protected namespace, not an
        // ordinary ims-v1-replay candidate with an invented terminal age.
        assert!(matches!(
            store.archive_and_prune(
                policy,
                RetentionRequest {
                    target: RetentionTarget::ImsReplay,
                    now_tick: 10_000,
                    max_records: 64,
                },
            ),
            Err(StoreError::InvalidTransition)
        ));
        assert_eq!(snapshot(&*store), before);
        invoke(&service, &effects, &invocation, 2, rols(*b"KEEP", 0));
        assert_eq!(data(&service, &invocation), vec![b"01A".to_vec()]);
        invoke(&service, &effects, &invocation, 3, ImsRecoveryCall::Rolb);

        // The old public resource-capture API produces genuine v1 rows without
        // application epochs. The new reader preserves their exact old digest;
        // their synthetic/unwitnessed points never acquire application authority.
        let key = recovery_rows(&*store)[0].key.clone();
        let recovery = RecoverySession::load(&*store, &key, RecoveryLimits::default()).unwrap();
        let transition = recovery
            .begin_uow(
                &*store,
                "legacy-begin",
                vec![mainframe_env_ims::recovery::TrackedResource {
                    namespace: "ims-v1-generic-database".into(),
                    key: "LOGDB".into(),
                    kind: mainframe_env_ims::recovery::TrackedResourceKind::Database,
                }],
            )
            .unwrap();
        store
            .mutate_provider_states_atomic(transition.mutations())
            .unwrap();
        let recovery = RecoverySession::load(&*store, &key, RecoveryLimits::default()).unwrap();
        let transition = recovery
            .sets(
                &*store,
                "legacy-point",
                mainframe_env_ims::recovery::BackoutPointKind::Sets,
                Some(*b"OLD!"),
                vec![],
                false,
            )
            .unwrap();
        store
            .mutate_provider_states_atomic(transition.mutations())
            .unwrap();
        let legacy = recovery_rows(&*store);
        let parsed: serde_json::Value = serde_json::from_slice(&legacy[0].payload).unwrap();
        assert!(parsed["points"][0].get("application_epoch").is_none());
        RecoverySession::load(&*store, &key, RecoveryLimits::default()).unwrap();
        assert_eq!(recovery_rows(&*store), legacy);
        let request = call(4, rols(*b"OLD!", 0));
        intent(&*store, &invocation, &request);
        let before = snapshot(&*store);
        assert_eq!(
            dispatch(service, effects, &invocation, &request),
            Err(HostProblem::UnknownOutcome)
        );
        assert_eq!(snapshot(&*store), before);
    }
    exercise(Arc::new(MemoryStore::new(Default::default())));
    let path = std::env::temp_dir().join(format!(
        "ims-backout-retention-{}.sqlite",
        std::process::id()
    ));
    exercise(Arc::new(
        SqliteStateStore::open(
            &format!("sqlite:{}?mode=rwc", path.display()),
            64 * 1024 * 1024,
            4096,
        )
        .unwrap(),
    ));
    std::fs::remove_file(path).unwrap();
}
