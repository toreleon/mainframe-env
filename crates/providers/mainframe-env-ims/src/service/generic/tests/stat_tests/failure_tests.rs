use super::*;
use mainframe_env_store::StoreLimits;
use mainframe_env_store_api::ProviderStateRecord;
use std::sync::atomic::AtomicBool;

#[test]
fn stat_authorization_selected_pcb_precedes_observation_and_replay() {
    let store = Arc::new(MemoryStore::new(Default::default()));
    let policy = Arc::new(Policy::default());
    let service =
        ImsService::open_authorized(store.clone(), Default::default(), policy.clone()).unwrap();
    install_v2(&service);
    policy.seen.lock().unwrap().clear();
    *policy.deny_read.lock().unwrap() = true;
    let req = v2(2, 3, ImsStatisticsFamily::Vbas, ImsStatisticsFormat::Full);
    let before = rows(store.as_ref());
    assert_eq!(
        invoke(&service, "stat", &req),
        Err(HostProblem::Unauthorized)
    );
    assert_eq!(rows(store.as_ref()), before);
    assert!(
        policy
            .seen
            .lock()
            .unwrap()
            .iter()
            .any(|r| r.name.as_str() == "OTHERDB" && r.intent == AccessIntent::Read)
    );
    assert!(
        !policy
            .seen
            .lock()
            .unwrap()
            .iter()
            .any(|r| r.name.as_str() == "GENDB")
    );
    *policy.deny_read.lock().unwrap() = false;
    assert_eq!(subpool(&invoke(&service, "stat", &req).unwrap()), "MDATA");
    let before = rows(store.as_ref());
    *policy.deny_read.lock().unwrap() = true;
    assert_eq!(
        invoke(&service, "stat", &req),
        Err(HostProblem::Unauthorized)
    );
    assert_eq!(rows(store.as_ref()), before);
}

#[test]
fn stat_missing_runtime_empty_runtime_legacy_metadata_and_counter_boundaries() {
    let (store, service) = setup();
    let req = v2(2, 1, ImsStatisticsFamily::Dbas, ImsStatisticsFormat::Full);
    let before = rows(store.as_ref());
    assert_eq!(invoke(&service, "stat", &req), Err(HostProblem::NotFound));
    assert_eq!(rows(store.as_ref()), before);
    service.install_system_runtime(Default::default()).unwrap();
    assert_eq!(invoke(&service, "stat", &req).unwrap().status, "GE");
    assert!(matches!(
        invoke(&service, "stat", &req).unwrap().system,
        Some(ImsSystemResult::StatisticsV2 {
            observation: None,
            ..
        })
    ));
    let (store, service) = setup();
    let mut runtime = runtime_v2();
    runtime.vsam_subpools_v2.clear();
    service.install_system_runtime(runtime.clone()).unwrap();
    for p in &runtime.buffer_pools {
        service
            .publish_buffer_statistics(ImsBufferStatistics {
                pool: p.name.clone(),
                kind: p.kind,
                buffer_bytes: p.buffer_bytes,
                buffers: p.buffers,
                reads: 0,
                writes: 0,
            })
            .unwrap();
    }
    let before = rows(store.as_ref());
    assert_eq!(
        invoke(
            &service,
            "stat",
            &v2(2, 1, ImsStatisticsFamily::Vbas, ImsStatisticsFormat::Full)
        ),
        Err(HostProblem::Unsupported)
    );
    assert_eq!(rows(store.as_ref()), before);
    let osam = invoke(&service, "stat", &req).unwrap();
    assert!(matches!(
        osam.system,
        Some(ImsSystemResult::StatisticsV2 {
            observation: Some(ImsStatisticsObservationV2::Totals {
                reads: 0,
                writes: 0,
                ..
            }),
            ..
        })
    ));
    let before = rows(store.as_ref());
    let mut invalid = ImsBufferStatistics {
        pool: "OSAM".into(),
        kind: ImsBufferPoolKind::Osam,
        buffer_bytes: 4096,
        buffers: 8,
        reads: u64::MAX,
        writes: 0,
    };
    invalid.buffers = 9;
    assert_eq!(
        service.publish_buffer_statistics(invalid.clone()),
        Err(HostProblem::Malformed)
    );
    assert_eq!(rows(store.as_ref()), before);
    invalid.pool = "MISSING".into();
    assert_eq!(
        service.publish_buffer_statistics(invalid),
        Err(HostProblem::NotFound)
    );
    assert_eq!(rows(store.as_ref()), before);
}

#[test]
fn stat_runtime_order_metadata_validation_and_total_overflow_are_atomic() {
    for mode in 0..5 {
        let (store, service) = setup();
        let mut runtime = runtime_v2();
        match mode {
            0 => {
                runtime.vsam_subpools_v2.pop();
            }
            1 => {
                runtime.vsam_subpools_v2[0].subpool = "MISSING".into();
            }
            2 => {
                runtime.vsam_subpools_v2[0].definition_order = 1;
            }
            3 => {
                runtime.vsam_subpools_v2[3].lsr_pool = 1;
            }
            _ => {
                runtime.vsam_subpools_v2[1] = runtime.vsam_subpools_v2[2].clone();
            }
        }
        let before = rows(store.as_ref());
        assert_eq!(
            service.install_system_runtime(runtime),
            Err(HostProblem::Malformed)
        );
        assert_eq!(rows(store.as_ref()), before);
    }
    let (store, service) = setup();
    let mut p2 = pool();
    p2.name = "OSAM2".into();
    service
        .install_system_runtime(ImsSystemRuntimeDefinition {
            buffer_pools: vec![pool(), p2],
            ..Default::default()
        })
        .unwrap();
    for name in ["OSAM", "OSAM2"] {
        service
            .publish_buffer_statistics(ImsBufferStatistics {
                pool: name.into(),
                kind: ImsBufferPoolKind::Osam,
                buffer_bytes: 4096,
                buffers: 8,
                reads: u64::MAX,
                writes: 0,
            })
            .unwrap();
    }
    let before = rows(store.as_ref());
    assert_eq!(
        invoke(
            &service,
            "stat",
            &v2(2, 1, ImsStatisticsFamily::Dbas, ImsStatisticsFormat::Full)
        ),
        Err(HostProblem::ResourceExhausted)
    );
    assert_eq!(rows(store.as_ref()), before);
}

#[test]
fn stat_stale_session_cas_and_row_capacity_preserve_cursor_and_receipt() {
    let store = Arc::new(MemoryStore::new(StoreLimits {
        max_provider_state: 64,
        ..Default::default()
    }));
    let service = ImsService::open(store.clone(), Default::default()).unwrap();
    install_v2(&service);
    let stale = ImsService::open(store.clone(), Default::default()).unwrap();
    let req = v2(2, 1, ImsStatisticsFamily::Vbas, ImsStatisticsFormat::Full);
    assert_eq!(subpool(&invoke(&service, "stat", &req).unwrap()), "MDATA");
    assert_eq!(
        subpool(
            &invoke(
                &stale,
                "stat",
                &v2(3, 1, ImsStatisticsFamily::Vbas, ImsStatisticsFormat::Full)
            )
            .unwrap()
        ),
        "ZDATA"
    );
    // A stale adapter must refresh. Force a genuine race after that refresh,
    // not a row advance that happened before the request began.
    let raced_store =
        crate::service::generic::tests::session_cas::SessionCasStore::new(store.clone(), "stat");
    let raced = ImsService::open(raced_store.clone(), Default::default()).unwrap();
    let mut expected = rows(store.as_ref());
    raced_store.arm();
    assert_eq!(
        invoke(
            &raced,
            "stat",
            &v2(4, 1, ImsStatisticsFamily::Vbas, ImsStatisticsFormat::Full)
        ),
        Err(HostProblem::IdempotencyConflict)
    );
    expected
        .iter_mut()
        .find(|row| row.namespace == SESSION_NAMESPACE && row.key == "stat")
        .unwrap()
        .version += 1;
    assert_eq!(rows(store.as_ref()), expected);
    let before = rows(store.as_ref());
    for index in before.len()..64 {
        store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: "stat-test-fill".into(),
                    key: index.to_string(),
                    version: 1,
                    payload: vec![1],
                },
                None,
            )
            .unwrap();
    }
    let before = rows(store.as_ref());
    let failed = v2(5, 1, ImsStatisticsFamily::Vbas, ImsStatisticsFormat::Full);
    assert_eq!(
        invoke(&service, "stat", &failed),
        Err(HostProblem::ResourceExhausted)
    );
    assert_eq!(rows(store.as_ref()), before);
    store
        .delete_provider_state("stat-test-fill", "63", 1)
        .unwrap();
    assert_eq!(
        subpool(&invoke(&service, "stat", &failed).unwrap()),
        "AINDEX"
    );
}

struct Clock(AtomicBool);
impl ImsReplayClock for Clock {
    fn now_tick(&self) -> Result<u64, HostProblem> {
        if self.0.load(Ordering::SeqCst) {
            Err(HostProblem::InfrastructureFailure)
        } else {
            Ok(250)
        }
    }
}

fn exercise_unknown(store: Arc<dyn ProviderStateStore>) {
    let clock = Arc::new(Clock(AtomicBool::new(false)));
    let service =
        ImsService::open_with_replay_clock(store.clone(), Default::default(), clock.clone())
            .unwrap();
    install_v2(&service);
    clock.0.store(true, Ordering::SeqCst);
    let req = v2(2, 1, ImsStatisticsFamily::Vbas, ImsStatisticsFormat::Full);
    assert_eq!(
        invoke(&service, "stat", &req),
        Err(HostProblem::UnknownOutcome)
    );
    assert_eq!(
        invoke(&service, "stat", &req),
        Err(HostProblem::UnknownOutcome)
    );
    drop(service);
    clock.0.store(false, Ordering::SeqCst);
    let service =
        ImsService::open_with_replay_clock(store.clone(), Default::default(), clock).unwrap();
    assert_eq!(subpool(&invoke(&service, "stat", &req).unwrap()), "MDATA");
    let before = rows(store.as_ref());
    assert_eq!(subpool(&invoke(&service, "stat", &req).unwrap()), "MDATA");
    assert_eq!(rows(store.as_ref()), before);
    assert_eq!(
        subpool(
            &invoke(
                &service,
                "stat",
                &v2(3, 1, ImsStatisticsFamily::Vbas, ImsStatisticsFormat::Full)
            )
            .unwrap()
        ),
        "ZDATA"
    );
}

#[test]
fn stat_post_publication_unknown_replays_without_cursor_redispatch() {
    exercise_unknown(Arc::new(MemoryStore::new(Default::default())));
}

#[test]
fn stat_sqlite_post_publication_unknown_and_fresh_adapter_replay() {
    let file = std::env::temp_dir().join(format!(
        "ims-stat-unknown-{}-{}.sqlite",
        std::process::id(),
        NEXT_FILE.fetch_add(1, Ordering::Relaxed)
    ));
    let url = format!("sqlite://{}?mode=rwc", file.display());
    exercise_unknown(Arc::new(
        SqliteStateStore::open(&url, 64 * 1024 * 1024, 262144).unwrap(),
    ));
    let store = Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262144).unwrap());
    let service = ImsService::open(store.clone(), Default::default()).unwrap();
    let req = v2(2, 1, ImsStatisticsFamily::Vbas, ImsStatisticsFormat::Full);
    let before = rows(store.as_ref());
    assert_eq!(subpool(&invoke(&service, "stat", &req).unwrap()), "MDATA");
    assert_eq!(rows(store.as_ref()), before);
    assert_eq!(
        subpool(
            &invoke(
                &service,
                "stat",
                &v2(4, 1, ImsStatisticsFamily::Vbas, ImsStatisticsFormat::Full)
            )
            .unwrap()
        ),
        "AINDEX"
    );
    drop(service);
    drop(store);
    std::fs::remove_file(file).unwrap();
}

struct FailAuthorization(AtomicBool);
impl EnterpriseAuthorizer for FailAuthorization {
    fn authorize(&self, _: &PrincipalId, _: &EnterpriseResource) -> Result<(), HostProblem> {
        if self.0.load(Ordering::SeqCst) {
            Err(HostProblem::InfrastructureFailure)
        } else {
            Ok(())
        }
    }
}

#[test]
fn stat_authorization_failure_and_missing_idempotency_publish_nothing() {
    let store = Arc::new(MemoryStore::new(Default::default()));
    let auth = Arc::new(FailAuthorization(AtomicBool::new(false)));
    let service =
        ImsService::open_authorized(store.clone(), Default::default(), auth.clone()).unwrap();
    install_v2(&service);
    let mut req = v2(2, 1, ImsStatisticsFamily::Vbas, ImsStatisticsFormat::Full);
    let before = rows(store.as_ref());
    auth.0.store(true, Ordering::SeqCst);
    assert_eq!(
        invoke(&service, "stat", &req),
        Err(HostProblem::InfrastructureFailure)
    );
    assert_eq!(rows(store.as_ref()), before);
    auth.0.store(false, Ordering::SeqCst);
    req.mutation = None;
    assert_eq!(
        invoke(&service, "stat", &req),
        Err(HostProblem::MissingIdempotency)
    );
    assert_eq!(rows(store.as_ref()), before);
}

fn rewrite(
    store: &dyn ProviderStateStore,
    namespace: &str,
    key: &str,
    f: impl FnOnce(&mut serde_json::Value),
) {
    let mut row = store.get_provider_state(namespace, key).unwrap().unwrap();
    let mut json: serde_json::Value = serde_json::from_slice(&row.payload).unwrap();
    f(&mut json["value"]);
    let version = row.version;
    row.version += 1;
    row.payload = serde_json::to_vec(&json).unwrap();
    store.put_provider_state(row, Some(version)).unwrap();
}

#[test]
fn stat_historical_rows_read_without_promoting_unknown_counters_and_corruption_rejects() {
    let (store, service) = setup();
    service
        .install_system_runtime(ImsSystemRuntimeDefinition {
            buffer_pools: vec![pool()],
            ..Default::default()
        })
        .unwrap();
    publish(&service);
    drop(service);
    rewrite(store.as_ref(), SYSTEM_NAMESPACE, "runtime", |v| {
        v.as_object_mut().unwrap().remove("published_pools_v2");
    });
    let before = rows(store.as_ref());
    let service = ImsService::open(store.clone(), Default::default()).unwrap();
    assert_eq!(rows(store.as_ref()), before);
    assert_eq!(
        invoke(&service, "stat", &stat(2, ImsStatisticsFamily::Dbas, false)),
        Err(HostProblem::NotFound)
    );
    assert_eq!(rows(store.as_ref()), before);
    drop(service);
    for mode in 0..3 {
        let store = Arc::new(MemoryStore::new(Default::default()));
        let service = ImsService::open(store.clone(), Default::default()).unwrap();
        install_v2(&service);
        invoke(
            &service,
            "stat",
            &v2(2, 1, ImsStatisticsFamily::Vbas, ImsStatisticsFormat::Full),
        )
        .unwrap();
        drop(service);
        match mode {
            0 => rewrite(store.as_ref(), SESSION_NAMESPACE, "stat", |v| {
                v["system"]["stat_cursors_v2"]["1"]["next"] = 5.into();
            }),
            1 => rewrite(store.as_ref(), SYSTEM_NAMESPACE, "runtime", |v| {
                v["published_pools_v2"] = serde_json::json!(["MISSING"]);
            }),
            _ => rewrite(store.as_ref(), SYSTEM_NAMESPACE, "runtime", |v| {
                v["pools"]["OSAM"]["buffers"] = 9.into();
            }),
        }
        let before = rows(store.as_ref());
        assert!(ImsService::open(store.clone(), Default::default()).is_err());
        assert_eq!(rows(store.as_ref()), before);
    }
}
