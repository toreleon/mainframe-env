use super::*;
use crate::MqObjectName;
use mainframe_env_execution_api::{
    ArtifactRef, ExecutionId, Principal, PrincipalId, RequestId, ResourceLimits, RunUnitId,
    Selector, ServiceClass, TraceId,
};
use mainframe_env_host_api::Mutation;
use mainframe_env_store::{MemoryStore, SqliteStateStore};
use std::path::PathBuf;
use std::sync::atomic::AtomicU64;

static NEXT_DB: AtomicU64 = AtomicU64::new(1);
struct Backend {
    store: Option<Arc<dyn PlatformStore>>,
    directory: Option<PathBuf>,
}
impl Backend {
    fn new(sqlite: bool) -> Self {
        if !sqlite {
            return Self {
                store: Some(Arc::new(MemoryStore::new(Default::default()))),
                directory: None,
            };
        }
        let dir = std::env::temp_dir().join(format!(
            "mq-selected-service-{}-{}",
            std::process::id(),
            NEXT_DB.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&dir).unwrap();
        let dir = dir.canonicalize().unwrap();
        let store = SqliteStateStore::open(
            &format!("sqlite://{}?mode=rwc", dir.join("state.sqlite").display()),
            64 << 20,
            256,
        )
        .unwrap();
        Self {
            store: Some(Arc::new(store)),
            directory: Some(dir),
        }
    }
    fn arc(&self) -> Arc<dyn PlatformStore> {
        self.store.as_ref().unwrap().clone()
    }
    fn provider(&self) -> Arc<dyn ProviderStateStore> {
        self.arc()
    }
    fn capture(&self) -> Vec<ProviderStateRecord> {
        self.arc().list_provider_state_prefix("mq-", 256).unwrap()
    }
    fn reopen(&mut self) {
        if let Some(dir) = &self.directory {
            assert_eq!(Arc::strong_count(self.store.as_ref().unwrap()), 1);
            drop(self.store.take());
            self.store = Some(Arc::new(
                SqliteStateStore::open(
                    &format!("sqlite://{}?mode=rw", dir.join("state.sqlite").display()),
                    64 << 20,
                    256,
                )
                .unwrap(),
            ));
        }
    }
}
impl Drop for Backend {
    fn drop(&mut self) {
        drop(self.store.take());
        if let Some(dir) = &self.directory {
            for name in ["state.sqlite", "state.sqlite-wal", "state.sqlite-shm"] {
                let file = dir.join(name);
                if file.exists() {
                    std::fs::remove_file(file).unwrap();
                }
            }
            std::fs::remove_dir(dir).unwrap();
        }
    }
}

#[derive(Default)]
struct Deny(AtomicU64);
impl EnterpriseAuthorizer for Deny {
    fn authorize(&self, _: &PrincipalId, _: &EnterpriseResource) -> Result<(), HostProblem> {
        self.0.fetch_add(1, Ordering::Relaxed);
        Err(HostProblem::Unauthorized)
    }
}
#[derive(Default)]
struct Clock(AtomicU64);
impl MqReplayClock for Clock {
    fn now_tick(&self) -> Result<u64, HostProblem> {
        self.0.fetch_add(1, Ordering::Relaxed);
        Ok(1)
    }
}
fn selected(b: &Backend) -> Result<Arc<MqService>, HostProblem> {
    MqService::open_selected_mqi(
        b.arc(),
        MqLimits::default(),
        3,
        5,
        Arc::new(Deny::default()),
        Arc::new(Clock::default()),
    )
}
fn invocation() -> Invocation {
    let l = InvocationLimits::default();
    Invocation::new(
        RequestId::new("selected-request", l).unwrap(),
        ExecutionId::new("selected-execution", l).unwrap(),
        RunUnitId::new("selected-run", l).unwrap(),
        None,
        Selector::new("mq:selected-test", l).unwrap(),
        ArtifactRef::new("mq:selected-test", l).unwrap(),
        Principal::new(
            PrincipalId::new("TEST", l).unwrap(),
            BTreeSet::from([CapabilityId::new("host.mq.write", l).unwrap()]),
            l,
        )
        .unwrap(),
        ServiceClass::Interactive,
        0,
        100,
        TraceId::new("selected-trace", l).unwrap(),
        IdempotencyKey::new("selected-invocation", l).unwrap(),
        1,
        ResourceLimits::default(),
        BTreeMap::new(),
        l,
    )
    .unwrap()
}
fn put() -> MqRequest {
    MqRequest {
        operation: MqOperation::PutOne,
        queue: Some("Q".into()),
        handle: None,
        options: 0,
        message: vec![0, 255, 41],
        message_id: Some(vec![1; 24]),
        correlation_id: Some(vec![7; 24]),
        wait_ticks: 0,
        max_message_bytes: 1024,
        mutation: Some(Mutation {
            sequence: 1,
            idempotency_key: IdempotencyKey::new("selected-put", InvocationLimits::default())
                .unwrap(),
            transaction: None,
        }),
    }
}
fn fixture(b: &Backend, rich: bool, populated: bool) {
    let legacy = MqService::open(b.provider(), MqLimits::default()).unwrap();
    legacy
        .install(vec![MqQueueDefinition {
            name: "Q".into(),
            trigger_program: None,
        }])
        .unwrap();
    if populated {
        assert_eq!(
            legacy
                .execute(&invocation(), &put())
                .unwrap()
                .completion_code,
            0
        );
    }
    if rich {
        let plan = legacy
            .plan_legacy_delivery_import(
                3,
                5,
                legacy_delivery_import::LegacyImportLimits::default(),
            )
            .unwrap();
        // Fixture publication only. This does not claim an audited MQI flow.
        b.arc()
            .mutate_provider_states_atomic(plan.into_parts().0)
            .unwrap();
    }
}

#[test]
fn memory_sqlite_selected_v1_v2_use_one_real_service_authority_on_reopen() {
    for sqlite in [false, true] {
        for rich in [false, true] {
            for populated in [false, true] {
                let mut b = Backend::new(sqlite);
                fixture(&b, rich, populated);
                let before = b.capture();
                for reopen in [false, true] {
                    if reopen {
                        b.reopen();
                    }
                    let service = selected(&b).unwrap();
                    assert!(service.authorizer.is_some());
                    assert!(service.replay_clock.is_some());
                    let core = service.selected_store.as_ref().unwrap();
                    let provider: Arc<dyn ProviderStateStore> = core.clone();
                    assert!(Arc::ptr_eq(&provider, &service.store));
                    let state = service.lock_selected().unwrap();
                    match &*state {
                        rich_state::StoredAuthority::Legacy(s) => {
                            assert!(!rich);
                            assert_eq!(s.state.queues["Q"].messages.len(), usize::from(populated));
                            assert_eq!(s.state.replay.len(), usize::from(populated));
                        }
                        rich_state::StoredAuthority::Rich(s) => {
                            assert!(rich);
                            assert_eq!(
                                s.delivery.depth(&MqObjectName::new("Q").unwrap()),
                                Some(usize::from(populated))
                            );
                            assert_eq!(s.replay.len(), usize::from(populated));
                        }
                    }
                    drop(state);
                    drop(service);
                    assert_eq!(b.capture(), before);
                }
                if rich {
                    assert!(MqService::open(b.provider(), MqLimits::default()).is_err());
                }
            }
        }
    }
}

#[test]
fn selected_service_cannot_use_sequential_legacy_routes_or_ready_registration() {
    for sqlite in [false, true] {
        for rich in [false, true] {
            let b = Backend::new(sqlite);
            fixture(&b, rich, true);
            let before = b.capture();
            let auth = Arc::new(Deny::default());
            let clock = Arc::new(Clock::default());
            let service = MqService::open_selected_mqi(
                b.arc(),
                MqLimits::default(),
                3,
                5,
                auth.clone(),
                clock.clone(),
            )
            .unwrap();
            assert!(matches!(service.lock(), Err(HostProblem::Unsupported)));
            assert_eq!(
                service.execute(&invocation(), &put()),
                Err(HostProblem::Unsupported)
            );
            assert_eq!(
                service.install(vec![MqQueueDefinition {
                    name: "Q".into(),
                    trigger_program: None,
                }]),
                Err(HostProblem::Unsupported)
            );
            assert_eq!(service.object_catalog(), Err(HostProblem::Unsupported));
            assert_eq!(service.queue_depth("Q"), Err(HostProblem::Unsupported));
            assert!(
                service
                    .plan_legacy_delivery_import(3, 5, Default::default())
                    .is_err()
            );
            assert!(mq_providers(service.clone(), InvocationLimits::default()).is_empty());
            assert_eq!(auth.0.load(Ordering::Relaxed), 0);
            assert_eq!(clock.0.load(Ordering::Relaxed), 0);
            assert_eq!(b.capture(), before);
        }
    }
}

#[test]
fn legacy_service_retains_only_its_existing_access_and_registration() {
    let b = Backend::new(false);
    fixture(&b, false, true);
    let legacy = MqService::open(b.provider(), MqLimits::default()).unwrap();
    assert!(matches!(
        legacy.lock_selected(),
        Err(HostProblem::Unsupported)
    ));
    assert_eq!(legacy.queue_depth("Q").unwrap(), 1);
    assert_eq!(mq_providers(legacy, InvocationLimits::default()).len(), 2);
}

#[test]
fn strict_selected_open_never_initializes_or_repairs_missing_corrupt_mixed_state() {
    for sqlite in [false, true] {
        let b = Backend::new(sqlite);
        let before = b.capture();
        assert!(matches!(selected(&b), Err(HostProblem::NotFound)));
        assert_eq!(b.capture(), before);
        fixture(&b, true, true);
        let original = b.capture();
        let mut marker = original
            .iter()
            .find(|r| r.namespace == STATE_NAMESPACE)
            .unwrap()
            .clone();
        let old = marker.version;
        marker.version += 1;
        marker.payload = br#"{"schema_version":"unrecognized"}"#.to_vec();
        b.arc().put_provider_state(marker, Some(old)).unwrap();
        let corrupt = b.capture();
        assert!(matches!(selected(&b), Err(HostProblem::Malformed)));
        assert_eq!(b.capture(), corrupt);
    }
    for sqlite in [false, true] {
        let b = Backend::new(sqlite);
        fixture(&b, true, false);
        b.arc()
            .put_provider_state(
                ProviderStateRecord {
                    namespace: QUEUE_NAMESPACE.into(),
                    key: "orphan".into(),
                    version: 1,
                    payload: encode_object_row(
                        "orphan",
                        &serde_json::json!({"trigger_program":null,"messages":[]}),
                    )
                    .unwrap(),
                },
                None,
            )
            .unwrap();
        let before = b.capture();
        assert!(matches!(selected(&b), Err(HostProblem::Malformed)));
        assert_eq!(b.capture(), before);
    }
}

#[test]
fn selected_open_refuses_wrong_trusted_identity_or_narrowed_limits_without_writes() {
    for sqlite in [false, true] {
        let b = Backend::new(sqlite);
        fixture(&b, true, true);
        let before = b.capture();
        for (g, f) in [(0, 5), (3, 0), (4, 5), (3, 6), (i64::MAX as u64 + 1, 5)] {
            assert!(matches!(
                MqService::open_selected_mqi(
                    b.arc(),
                    MqLimits::default(),
                    g,
                    f,
                    Arc::new(Deny::default()),
                    Arc::new(Clock::default())
                ),
                Err(HostProblem::IdempotencyConflict)
            ));
        }
        let limits = MqLimits {
            max_state_bytes: 16,
            ..Default::default()
        };
        assert!(
            MqService::open_selected_mqi(
                b.arc(),
                limits,
                3,
                5,
                Arc::new(Deny::default()),
                Arc::new(Clock::default())
            )
            .is_err()
        );
        assert_eq!(b.capture(), before);
    }
}

#[test]
fn checked_legacy_guard_rejects_rich_union_and_releases_the_lock() {
    let b = Backend::new(false);
    fixture(&b, true, false);
    let service = selected(&b).unwrap();
    assert!(matches!(
        LegacyAccess::new(service.durable.lock().unwrap()),
        Err(HostProblem::Unsupported)
    ));
    assert!(service.durable.try_lock().is_ok());
}
