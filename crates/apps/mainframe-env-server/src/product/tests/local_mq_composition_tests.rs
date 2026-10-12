//! Genuine fixed MQ queue catalog controls; no native MQI or fabricated complete publication.
use super::*;
use mainframe_env_application::{BatchController, BatchControllerKind, MqResource, MqResourceKind};
use mainframe_env_host_api::{MqOperation, MqRequest};
use mainframe_env_mq::{MqCatalogProjection, MqLimits, MqQueueDefinition, MqService};

const MQ_NAMESPACES: [&str; 6] = [
    "mq-state",
    "mq-v1-object-catalog",
    "mq-v1-queue",
    "mq-v1-handle-index",
    "mq-v1-unit-of-work",
    "mq-v1-replay",
];
fn queue_definitions() -> Vec<MqQueueDefinition> {
    vec![MqQueueDefinition {
        name: "LOCAL.QUEUE".into(),
        trigger_program: None,
    }]
}
fn signed_queue(package: &mut ApplicationPackageV2, trust: &HmacSha256PackageTrust) {
    let resource = MqResource {
        name: "LOCAL.QUEUE".into(),
        kind: MqResourceKind::Queue,
        target: None,
        controller: None,
    };
    let raw = serde_json::to_vec(&resource).unwrap();
    let sha = format!("sha256:{:x}", Sha256::digest(&raw));
    package.base.blobs.insert(sha.clone(), raw.clone());
    package.base.manifest.entries.push(PackageEntry {
        path: "resource/mq/LOCAL.QUEUE".into(),
        kind: EntryKind::Resource,
        sha256: sha,
        bytes: raw.len(),
        depends_on: Vec::new(),
    });
    package.sections.mq_resources = vec![resource];
    resign_package(package, trust);
}
fn mq_definitions(
    server: &ProductServer,
    trust: &HmacSha256PackageTrust,
    generation: u64,
) -> (ApplicationPackageV2, LocalApplicationDefinition) {
    let (mut package, definition) = definitions(server, trust, generation);
    signed_queue(&mut package, trust);
    (package, definition)
}
fn install_mq(
    server: &ProductServer,
    trust: &HmacSha256PackageTrust,
    generation: u64,
) -> LocalApplicationReceipt {
    let (package, definition) = mq_definitions(server, trust, generation);
    let expected = server.install_application_package_v2(&package).unwrap();
    server
        .publish_local_application(&expected, definition)
        .unwrap()
}
fn runtime_rows(store: &dyn ProviderStateStore) -> Vec<ProviderStateRecord> {
    let mut rows = Vec::new();
    for namespace in MQ_NAMESPACES {
        rows.extend(store.list_provider_state(namespace, 1024).unwrap());
    }
    rows.sort_by(|a, b| (&a.namespace, &a.key).cmp(&(&b.namespace, &b.key)));
    rows
}
fn rows(store: &dyn ProviderStateStore) -> Vec<ProviderStateRecord> {
    let mut rows = selected_rows(store);
    rows.extend(runtime_rows(store));
    rows.extend(
        store
            .list_provider_state("application-mq-catalog-selection", 2)
            .unwrap(),
    );
    rows.sort_by(|a, b| (&a.namespace, &a.key).cmp(&(&b.namespace, &b.key)));
    rows
}
fn projection(store: &dyn ProviderStateStore) -> MqCatalogProjection {
    let row = store
        .get_provider_state("application-local-selection", "LOCAL-COMPOSE")
        .unwrap()
        .unwrap();
    let value: serde_json::Value = serde_json::from_slice(&row.payload).unwrap();
    serde_json::from_value(value["mq"].clone()).unwrap()
}
fn invocation(run: &str) -> Invocation {
    let limits = InvocationLimits::default();
    Invocation::new(
        RequestId::new(format!("request-{run}"), limits).unwrap(),
        ExecutionId::new(format!("execution-{run}"), limits).unwrap(),
        RunUnitId::new(run, limits).unwrap(),
        None,
        Selector::new("program:MQTEST", limits).unwrap(),
        ArtifactRef::new("artifact", limits).unwrap(),
        Principal::new(
            PrincipalId::new("IBMUSER", limits).unwrap(),
            BTreeSet::from([
                CapabilityId::new("host.mq.read", limits).unwrap(),
                CapabilityId::new("host.mq.write", limits).unwrap(),
            ]),
            limits,
        )
        .unwrap(),
        ServiceClass::Interactive,
        0,
        10_000,
        TraceId::new(format!("trace-{run}"), limits).unwrap(),
        IdempotencyKey::new(format!("invocation-{run}"), limits).unwrap(),
        1,
        ResourceLimits::default(),
        BTreeMap::new(),
        limits,
    )
    .unwrap()
}
fn put(service: &MqService, run: &str, pending: bool) {
    let request = MqRequest {
        operation: MqOperation::PutOne,
        queue: Some("LOCAL.QUEUE".into()),
        handle: None,
        options: if pending { 2 } else { 0 },
        message: b"EXACT-LOCAL-MESSAGE".to_vec(),
        message_id: None,
        correlation_id: None,
        wait_ticks: 0,
        max_message_bytes: 128,
        mutation: Some(Mutation {
            sequence: 1,
            transaction: Some("MQ-LOCAL".into()),
            idempotency_key: IdempotencyKey::new(format!("{run}-put"), InvocationLimits::default())
                .unwrap(),
        }),
    };
    assert_eq!(
        service
            .execute(&invocation(run), &request)
            .unwrap()
            .completion_code,
        0
    );
}
fn exercise_ready<S: PlatformStore + 'static>(store: Arc<S>) {
    let trust = Arc::new(test_package_trust());
    let server = fixture(store.clone(), trust.clone());
    let receipt = install_mq(&server, &trust, 1);
    let selected = projection(store.as_ref());
    assert_eq!(selected.definitions, queue_definitions());
    assert_eq!(server.mq.queue_depth("LOCAL.QUEUE"), Ok(0));
    assert_eq!(
        server.mq.selected_catalog_composition_matches(&selected),
        Ok(())
    );
    let catalog = server.mq.object_catalog().unwrap().unwrap();
    assert_eq!(
        selected.catalog_identity,
        format!("sha256:{:x}", Sha256::digest(catalog.encode().unwrap()))
    );
    let before = rows(store.as_ref());
    drop(server);
    let reopened = fixture(store.clone(), trust);
    assert_eq!(rows(store.as_ref()), before);
    assert_eq!(
        reopened
            .selected_local_application("LOCAL-COMPOSE")
            .unwrap(),
        Some(receipt.generation)
    );
}
#[test]
fn mq_signed_local_catalog_and_original_rows_reopen_on_memory_and_sqlite() {
    exercise_ready(Arc::new(MemoryStore::new(Default::default())));
    static SERIAL: AtomicU64 = AtomicU64::new(1);
    let root = std::env::temp_dir().join(format!(
        "mq-local-ready-{}-{}",
        std::process::id(),
        SERIAL.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::create_dir(&root).unwrap();
    let url = format!("sqlite://{}?mode=rwc", root.join("state.sqlite").display());
    exercise_ready(Arc::new(
        SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap(),
    ));
    let reopened_store = Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap());
    let reopened = fixture(reopened_store.clone(), Arc::new(test_package_trust()));
    assert_eq!(
        reopened
            .selected_local_application("LOCAL-COMPOSE")
            .unwrap()
            .unwrap()
            .generation,
        1
    );
    assert_eq!(reopened.mq.queue_depth("LOCAL.QUEUE"), Ok(0));
    drop((reopened, reopened_store));
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn mq_fixed_catalog_retry_generation_and_rollback_preserve_real_message_and_replay_rows() {
    let trust = Arc::new(test_package_trust());
    let store = Arc::new(MemoryStore::new(Default::default()));
    let server = fixture(store.clone(), trust.clone());
    let first = install_mq(&server, &trust, 1);
    drop(server);
    let service = MqService::open(store.clone(), Default::default()).unwrap();
    put(&service, "message", false);
    drop(service);
    let server = fixture(store.clone(), trust.clone());
    let runtime = runtime_rows(store.as_ref());
    let before = rows(store.as_ref());
    let (_, definition) = mq_definitions(&server, &trust, 1);
    assert!(
        server
            .publish_local_application(&first.generation, definition)
            .unwrap()
            .replayed
    );
    assert_eq!(rows(store.as_ref()), before);
    install_mq(&server, &trust, 2);
    let (_, definition) = mq_definitions(&server, &trust, 1);
    server
        .rollback_local_application(&first.generation, definition)
        .unwrap();
    let actual = runtime_rows(store.as_ref());
    assert_eq!(actual.len(), runtime.len());
    for (old, new) in runtime.iter().zip(&actual) {
        assert_eq!(
            (&old.namespace, &old.key, &old.payload),
            (&new.namespace, &new.key, &new.payload)
        );
        assert_eq!(
            new.version,
            if old.namespace == "mq-state" {
                old.version + 2
            } else {
                old.version
            }
        );
    }
    assert_eq!(
        server.mq.queue_messages("LOCAL.QUEUE").unwrap(),
        vec![b"EXACT-LOCAL-MESSAGE".to_vec()]
    );
    let full = rows(store.as_ref());
    drop(server);
    let reopened = fixture(store.clone(), trust);
    assert_eq!(rows(store.as_ref()), full);
    assert_eq!(
        reopened
            .selected_local_application("LOCAL-COMPOSE")
            .unwrap()
            .unwrap()
            .generation,
        1
    );
}
#[test]
fn mq_foreign_selection_and_preinstalled_legacy_catalog_cannot_be_adopted() {
    // Complete owner namespace admission rejects unexpected keys even before
    // an otherwise empty legacy catalog could be adopted.
    let empty = Arc::new(MemoryStore::new(Default::default()));
    let service = MqService::open(empty.clone(), Default::default()).unwrap();
    let foreign = ProviderStateRecord {
        namespace: "application-mq-catalog-selection".into(),
        key: "foreign".into(),
        version: 1,
        payload: b"UNKNOWN-OWNER".to_vec(),
    };
    empty.put_provider_state(foreign.clone(), None).unwrap();
    let before = rows(empty.as_ref());
    assert!(
        service
            .prepare_catalog_composition(
                "LOCAL-COMPOSE",
                1,
                &format!("sha256:{}", "a".repeat(64)),
                queue_definitions(),
                None,
                false
            )
            .is_err()
    );
    assert_eq!(rows(empty.as_ref()), before);
    assert_eq!(
        empty
            .get_provider_state(&foreign.namespace, &foreign.key)
            .unwrap(),
        Some(foreign)
    );
    for foreign in [false, true] {
        let store = Arc::new(MemoryStore::new(Default::default()));
        let service = MqService::open(store.clone(), Default::default()).unwrap();
        let identity = format!("sha256:{}", "a".repeat(64));
        if foreign {
            let prepared = service
                .prepare_catalog_composition(
                    "FOREIGN",
                    1,
                    &identity,
                    queue_definitions(),
                    None,
                    false,
                )
                .unwrap();
            let commit =
                ProviderCompositionCommit::publish(store.clone(), prepared.writes().to_vec())
                    .unwrap();
            prepared.finish(&commit).unwrap();
        } else {
            service.install(queue_definitions()).unwrap();
        }
        let before = rows(store.as_ref());
        assert!(matches!(
            service.prepare_catalog_composition(
                "LOCAL-COMPOSE",
                1,
                &identity,
                queue_definitions(),
                None,
                false
            ),
            Err(HostProblem::IdempotencyConflict)
        ));
        assert_eq!(rows(store.as_ref()), before);
    }
}
#[test]
fn mq_signed_resource_exact_bytes_kind_and_unsupported_topology_refuse_without_writes() {
    for mode in 0..6 {
        let trust = Arc::new(test_package_trust());
        let store = Arc::new(MemoryStore::new(Default::default()));
        let server = fixture(store.clone(), trust.clone());
        let (mut package, definition) = mq_definitions(&server, &trust, 1);
        match mode {
            0 => package
                .base
                .manifest
                .entries
                .retain(|entry| entry.path != "resource/mq/LOCAL.QUEUE"),
            1 => package.sections.mq_resources[0].name = "OTHER.QUEUE".into(),
            2 => package.sections.mq_resources[0].kind = MqResourceKind::Process,
            3 => package.sections.mq_resources[0].kind = MqResourceKind::Trigger,
            4 => package.sections.mq_resources[0].target = Some("LOCAL.QUEUE".into()),
            _ => {
                package.sections.batch_controllers.push(BatchController {
                    name: "CONTROLLER".into(),
                    program: "program/LOCALPGM".into(),
                    kind: BatchControllerKind::CobolProgram,
                    properties: BTreeMap::from([
                        ("launcher".into(), "tso-run".into()),
                        ("selector-program".into(), "LOCALPGM".into()),
                        ("behavior".into(), "program-call".into()),
                    ]),
                });
                package.sections.mq_resources[0].controller = Some("CONTROLLER".into());
            }
        }
        let used = package
            .base
            .manifest
            .entries
            .iter()
            .map(|entry| entry.sha256.clone())
            .collect::<BTreeSet<_>>();
        package.base.blobs.retain(|sha, _| used.contains(sha));
        resign_package(&mut package, &trust);
        let expected = server.install_application_package_v2(&package).unwrap();
        let before = rows(store.as_ref());
        let publication = server.publish_local_application(&expected, definition);
        assert!(publication.is_err());
        if mode >= 2 {
            assert!(matches!(publication, Err(HostProblem::Unsupported)));
        }
        assert_eq!(rows(store.as_ref()), before);
        assert_eq!(
            server.selected_local_application("LOCAL-COMPOSE").unwrap(),
            None
        );
    }
}
#[test]
fn mq_unknown_shared_commit_and_wrong_store_acknowledgement_fence_runtime_until_reopen() {
    for outcome in [Outcome::Before, Outcome::After] {
        let trust = Arc::new(test_package_trust());
        let store = Arc::new(FaultStore::new(Arc::new(MemoryStore::new(
            Default::default(),
        ))));
        let server = fixture(store.clone(), trust.clone());
        let (package, definition) = mq_definitions(&server, &trust, 1);
        let expected = server.install_application_package_v2(&package).unwrap();
        let before = rows(store.as_ref());
        store.arm(outcome);
        assert_eq!(
            server.publish_local_application(&expected, definition),
            Err(HostProblem::UnknownOutcome)
        );
        assert_eq!(server.mq.object_catalog(), Err(HostProblem::UnknownOutcome));
        drop(server);
        let reopened = fixture(store.clone(), trust);
        if matches!(outcome, Outcome::Before) {
            assert_eq!(rows(store.as_ref()), before);
            assert_eq!(
                reopened
                    .selected_local_application("LOCAL-COMPOSE")
                    .unwrap(),
                None
            );
        } else {
            assert!(
                reopened
                    .selected_local_application("LOCAL-COMPOSE")
                    .unwrap()
                    .is_some()
            );
        }
    }
    let left = Arc::new(MemoryStore::new(Default::default()));
    let right = Arc::new(MemoryStore::new(Default::default()));
    let service = MqService::open(left.clone(), Default::default()).unwrap();
    let prepared = service
        .prepare_catalog_composition(
            "LOCAL-COMPOSE",
            1,
            &format!("sha256:{}", "a".repeat(64)),
            queue_definitions(),
            None,
            false,
        )
        .unwrap();
    let commit = ProviderCompositionCommit::publish(right, prepared.writes().to_vec()).unwrap();
    assert_eq!(prepared.finish(&commit), Err(HostProblem::UnknownOutcome));
    assert!(runtime_rows(left.as_ref()).is_empty());
    assert_eq!(service.object_catalog(), Err(HostProblem::UnknownOutcome));
}
#[test]
fn mq_sqlite_companion_cas_conflict_leaves_all_rows_and_selection_unchanged() {
    static SERIAL: AtomicU64 = AtomicU64::new(1);
    let root = std::env::temp_dir().join(format!(
        "mq-local-cas-{}-{}",
        std::process::id(),
        SERIAL.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::create_dir(&root).unwrap();
    let url = format!("sqlite://{}?mode=rwc", root.join("state.sqlite").display());
    let trust = Arc::new(test_package_trust());
    let store = Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap());
    let server = fixture(store.clone(), trust.clone());
    install_mq(&server, &trust, 1);
    let prior = projection(store.as_ref());
    let (package, definition) = mq_definitions(&server, &trust, 2);
    let expected = server.install_application_package_v2(&package).unwrap();
    let companion = ProviderStateRecord {
        namespace: "mq-composition-companion".into(),
        key: "owned".into(),
        version: 1,
        payload: b"UNCHANGED".to_vec(),
    };
    store.put_provider_state(companion.clone(), None).unwrap();
    let before = rows(store.as_ref());
    let prepared = server
        .mq
        .prepare_catalog_composition(
            "LOCAL-COMPOSE",
            2,
            &expected.identity,
            queue_definitions(),
            Some(&prior),
            false,
        )
        .unwrap();
    let mut writes = prepared.writes().to_vec();
    writes.push(ProviderStateWrite {
        record: companion.clone(),
        expected_version: None,
    });
    assert!(ProviderCompositionCommit::publish(store.clone(), writes).is_err());
    drop(prepared);
    assert_eq!(rows(store.as_ref()), before);
    assert_eq!(
        store
            .get_provider_state(&companion.namespace, &companion.key)
            .unwrap(),
        Some(companion)
    );
    // A separate real runtime service can mutate the same physical store,
    // but it cannot borrow the held cache owner or evade the manifest CAS.
    let other = MqService::open(store.clone(), Default::default()).unwrap();
    let prepared = server
        .mq
        .prepare_catalog_composition(
            "LOCAL-COMPOSE",
            2,
            &expected.identity,
            queue_definitions(),
            Some(&prior),
            false,
        )
        .unwrap();
    put(&other, "concurrent", false);
    let after_runtime = rows(store.as_ref());
    assert!(ProviderCompositionCommit::publish(store.clone(), prepared.writes().to_vec()).is_err());
    drop(prepared);
    assert_eq!(rows(store.as_ref()), after_runtime);
    drop((other, server));
    let server = fixture(store.clone(), trust.clone());
    server
        .publish_local_application(&expected, definition)
        .unwrap();
    let full = rows(store.as_ref());
    drop(server);
    let reopened = fixture(store.clone(), trust);
    assert_eq!(rows(store.as_ref()), full);
    assert_eq!(
        reopened
            .selected_local_application("LOCAL-COMPOSE")
            .unwrap()
            .unwrap()
            .generation,
        2
    );
    drop((reopened, store));
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn mq_real_pending_work_capacity_catalog_drift_and_actual_version_refuse_without_mutation() {
    let trust = Arc::new(test_package_trust());
    let store = Arc::new(MemoryStore::new(Default::default()));
    let server = fixture(store.clone(), trust.clone());
    let first = install_mq(&server, &trust, 1);
    let prior = projection(store.as_ref());
    drop(server);
    let service = MqService::open(store.clone(), Default::default()).unwrap();
    put(&service, "pending", true);
    let before = rows(store.as_ref());
    assert!(matches!(
        service.prepare_catalog_composition(
            "LOCAL-COMPOSE",
            2,
            &format!("sha256:{}", "b".repeat(64)),
            queue_definitions(),
            Some(&prior),
            false
        ),
        Err(HostProblem::IdempotencyConflict)
    ));
    assert_eq!(rows(store.as_ref()), before);
    let mut changed = queue_definitions();
    changed[0].name = "OTHER.QUEUE".into();
    assert!(
        service
            .prepare_catalog_composition(
                "LOCAL-COMPOSE",
                2,
                &first.generation.identity,
                changed,
                Some(&prior),
                false
            )
            .is_err()
    );
    assert_eq!(rows(store.as_ref()), before);
    // Catalog equality retains the actual service limits. A legitimate stricter
    // profile must survive selected admission, next generation and store reopen.
    let owned = Arc::new(MemoryStore::new(Default::default()));
    let limits = MqLimits {
        max_queues: 2,
        max_state_bytes: 1024 * 1024,
        ..Default::default()
    };
    let actual = MqService::open(owned.clone(), limits).unwrap();
    let identity = format!("sha256:{}", "c".repeat(64));
    let prepared = actual
        .prepare_catalog_composition(
            "LOCAL-COMPOSE",
            1,
            &identity,
            queue_definitions(),
            None,
            false,
        )
        .unwrap();
    let selected = prepared.projection().clone();
    let commit =
        ProviderCompositionCommit::publish(owned.clone(), prepared.writes().to_vec()).unwrap();
    prepared.finish(&commit).unwrap();
    let snapshot = rows(owned.as_ref());
    assert_eq!(
        actual.selected_catalog_composition_matches(&selected),
        Ok(())
    );
    assert_eq!(rows(owned.as_ref()), snapshot);
    let prepared = actual
        .prepare_catalog_composition(
            "LOCAL-COMPOSE",
            2,
            &identity,
            queue_definitions(),
            Some(&selected),
            false,
        )
        .unwrap();
    let selected = prepared.projection().clone();
    let commit =
        ProviderCompositionCommit::publish(owned.clone(), prepared.writes().to_vec()).unwrap();
    prepared.finish(&commit).unwrap();
    assert_eq!(
        actual.selected_catalog_composition_matches(&selected),
        Ok(())
    );
    let snapshot = rows(owned.as_ref());
    drop(actual);
    let reopened = MqService::open(owned.clone(), limits).unwrap();
    assert_eq!(
        reopened.selected_catalog_composition_matches(&selected),
        Ok(())
    );
    assert_eq!(rows(owned.as_ref()), snapshot);
    let limited = MqService::open(
        Arc::new(MemoryStore::new(Default::default())),
        MqLimits {
            max_queues: 0,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(matches!(
        limited.prepare_catalog_composition(
            "LOCAL-COMPOSE",
            1,
            &first.generation.identity,
            queue_definitions(),
            None,
            false
        ),
        Err(HostProblem::ResourceExhausted)
    ));
    let row = store
        .get_provider_state("mq-v1-queue", "LOCAL.QUEUE")
        .unwrap()
        .unwrap();
    store
        .put_provider_state(
            ProviderStateRecord {
                version: row.version + 1,
                ..row.clone()
            },
            Some(row.version),
        )
        .unwrap();
    let before = rows(store.as_ref());
    assert_eq!(
        service.selected_catalog_composition_matches(&prior),
        Err(HostProblem::IdempotencyConflict)
    );
    assert_eq!(rows(store.as_ref()), before);
}
