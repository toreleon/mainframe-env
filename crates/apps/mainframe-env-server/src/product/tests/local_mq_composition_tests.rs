//! Genuine fixed MQ queue catalog controls; no native MQI or fabricated complete publication.
use super::*;
use mainframe_env_application::{
    BatchController, BatchControllerKind, MqResource, MqResourceKind, SecurityResource,
};
use mainframe_env_host_api::{
    AccessIntent, MqOperation, MqRequest, ResourceName, SecurityDecision,
};
use mainframe_env_mq::{MqCatalogProjection, MqLimits, MqQueueDefinition, MqService};
use mainframe_env_racf::{AccessLevel, ClassDescriptor, RacfLimits, RacfProfileComposition};

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

// Opt-in local graph fixtures use existing signed source/artifact/controller mechanisms.
fn graph_definitions(
    server: &ProductServer,
    trust: &HmacSha256PackageTrust,
    generation: u64,
) -> (ApplicationPackageV2, LocalApplicationDefinition) {
    // Same unsupplied RESOURCE schema as RACF's existing installer; grants stay in the signed Profile.
    server
        .racf
        .database()
        .install_profile_schemas(
            Vec::new(),
            vec![ClassDescriptor {
                name: "MQQUEUE".into(),
                supplied: false,
                active: true,
                generic_allowed: true,
                generic_active: true,
                discrete_allowed: true,
                raclist: false,
                default_uacc: AccessLevel::None,
                max_profile_name_bytes: RacfLimits::default().max_name_bytes,
                posit: None,
                member_class: None,
                grouping_class: None,
                profile_template: "RESOURCE".into(),
                version: 1,
            }],
        )
        .unwrap();
    let principal = PrincipalId::new("IBMUSER", InvocationLimits::default()).unwrap();
    if server
        .racf
        .active_principal_epoch(&principal)
        .unwrap()
        .is_none()
    {
        server.bootstrap_identity("IBMUSER", b"TESTPASS").unwrap();
    }
    let (mut package, mut definition) = definitions(server, trust, generation);
    let mut controller = signed_controller_package(trust)
        .sections
        .batch_controllers
        .remove(0);
    controller.name = "LOCAL-CONTROLLER".into();
    controller.program = "program/LOCALPGM".into();
    controller
        .properties
        .insert("selector-program".into(), "LOCALPGM".into());
    package.sections.batch_controllers = vec![controller];
    let controllers = package
        .sections
        .batch_controllers
        .iter()
        .map(|controller| {
            super::super::super::batch_controller::decode_application_batch_controller(
                &package, controller,
            )
            .unwrap()
        })
        .collect::<Vec<_>>();
    let mut resources = vec![
        (
            "resource/batch-controllers".to_owned(),
            serde_json::to_vec(&controllers).unwrap(),
        ),
        (
            "resource/mq/graph-profile".to_owned(),
            b"mainframe-env.local-mq-process-trigger-graph@1".to_vec(),
        ),
    ];
    package.sections.mq_resources = vec![
        MqResource {
            name: "LOCAL.QUEUE".into(),
            kind: MqResourceKind::Queue,
            target: Some("LOCAL.TRIGGER".into()),
            controller: None,
        },
        MqResource {
            name: "LOCAL.TRIGGER".into(),
            kind: MqResourceKind::Trigger,
            target: Some("LOCAL.PROCESS".into()),
            controller: None,
        },
        MqResource {
            name: "LOCAL.PROCESS".into(),
            kind: MqResourceKind::Process,
            target: None,
            controller: Some("LOCAL-CONTROLLER".into()),
        },
    ];
    for resource in &package.sections.mq_resources {
        resources.push((
            format!("resource/mq/{}", resource.name),
            serde_json::to_vec(resource).unwrap(),
        ));
    }
    for (path, raw) in resources {
        let sha = format!("sha256:{:x}", Sha256::digest(&raw));
        package.base.blobs.insert(sha.clone(), raw.clone());
        package.base.manifest.entries.push(PackageEntry {
            path,
            kind: EntryKind::Resource,
            sha256: sha,
            bytes: raw.len(),
            depends_on: vec!["program/LOCALPGM".into()],
        });
    }
    // Replace only the base fixture's exact non-RACF placeholder Profile.
    // Other Profiles and their dependencies remain subject to the strict binding guard.
    let placeholder_bytes = b"explicit-local-finite-profile@1";
    let placeholder_sha = format!("sha256:{:x}", Sha256::digest(placeholder_bytes));
    assert!(package.sections.security_resources.is_empty());
    assert!(definition.security_profiles.is_empty());
    assert_eq!(
        package
            .base
            .manifest
            .entries
            .iter()
            .filter(|entry| entry.path == "profile/local")
            .count(),
        1
    );
    let placeholder_index = package
        .base
        .manifest
        .entries
        .iter()
        .position(|entry| entry.path == "profile/local")
        .unwrap();
    let placeholder = &package.base.manifest.entries[placeholder_index];
    assert_eq!(placeholder.kind, EntryKind::Profile);
    assert_eq!(placeholder.sha256, placeholder_sha);
    assert_eq!(placeholder.bytes, placeholder_bytes.len());
    assert_eq!(placeholder.depends_on, vec!["source/LOCALPGM".to_string()]);
    assert_eq!(
        package.base.blobs.get(&placeholder_sha).unwrap().as_slice(),
        placeholder_bytes
    );
    assert!(package.base.manifest.entries.iter().all(|entry| {
        !entry
            .depends_on
            .iter()
            .any(|dependency| dependency == "profile/local")
    }));
    package.base.manifest.entries.remove(placeholder_index);
    if !package
        .base
        .manifest
        .entries
        .iter()
        .any(|entry| entry.sha256 == placeholder_sha)
    {
        package.base.blobs.remove(&placeholder_sha).unwrap();
    }
    // Bind exact queue authority under the same real signed six-owner composition.
    let profile = RacfProfileComposition {
        class: "MQQUEUE".into(),
        name: "LOCAL.QUEUE".into(),
        owner: "IBMUSER".into(),
        uacc: AccessLevel::None,
        permissions: BTreeMap::from([("IBMUSER".into(), AccessLevel::Update)]),
    };
    package.sections.security_resources.push(SecurityResource {
        class: profile.class.clone(),
        profile: profile.name.clone(),
        owner: profile.owner.clone(),
    });
    let raw = profile.canonical_payload().unwrap();
    let sha = format!("sha256:{:x}", Sha256::digest(&raw));
    package.base.blobs.insert(sha.clone(), raw.clone());
    package.base.manifest.entries.push(PackageEntry {
        path: profile.entry_path().unwrap(),
        kind: EntryKind::Profile,
        sha256: sha,
        bytes: raw.len(),
        depends_on: Vec::new(),
    });
    definition.security_profiles.push(profile);
    resign_package(&mut package, trust);
    (package, definition)
}
fn graph_rows(store: &dyn ProviderStateStore) -> Vec<ProviderStateRecord> {
    let mut full = rows(store);
    for namespace in [
        "batch-controller-state",
        "batch-program",
        "jes-job",
        "jes-meta",
    ] {
        full.extend(store.list_provider_state(namespace, 4096).unwrap());
    }
    full.sort_by(|a, b| (&a.namespace, &a.key).cmp(&(&b.namespace, &b.key)));
    full
}
fn exercise_graph_ready<S: PlatformStore + 'static>(store: Arc<S>) {
    use mainframe_env_mq::{MqObjectKind, MqObjectLookup, MqObjectName};
    let trust = Arc::new(test_package_trust());
    let server = fixture(store.clone(), trust.clone());
    let (package, definition) = graph_definitions(&server, &trust, 1);
    let expected_program = package
        .base
        .manifest
        .entries
        .iter()
        .find(|entry| entry.kind == EntryKind::Program && entry.path == "program/LOCALPGM")
        .unwrap()
        .sha256
        .clone();
    let expected = server.install_application_package_v2(&package).unwrap();
    let installed = server
        .publish_local_application(&expected, definition)
        .unwrap();
    let mq = projection(store.as_ref());
    assert_eq!(
        mq.definitions,
        vec![MqQueueDefinition {
            name: "LOCAL.QUEUE".into(),
            trigger_program: Some("LOCAL.PROCESS".into())
        }]
    );
    let catalog = server.mq.object_catalog().unwrap().unwrap();
    assert_eq!(
        catalog.kind(&MqObjectLookup::Process(
            MqObjectName::new("LOCAL.PROCESS").unwrap()
        )),
        Ok(MqObjectKind::Process)
    );
    let row = store
        .get_provider_state("application-local-selection", "LOCAL-COMPOSE")
        .unwrap()
        .unwrap();
    let selection: serde_json::Value = serde_json::from_slice(&row.payload).unwrap();
    let batch: mainframe_env_batch::BatchControllerProjection =
        serde_json::from_value(selection["batch"].clone()).unwrap();
    assert_eq!(
        server.batch.selected_controller_composition_matches(&batch),
        Ok(())
    );
    assert_eq!(batch.generation.controllers[0].name, "LOCAL-CONTROLLER");
    assert_eq!(
        batch.generation.controllers[0].program.identity,
        expected_program
    );
    assert_eq!(definition_artifact(&server), expected_program);
    let retained = {
        let owner = server.applications_v2.lock().unwrap();
        owner
            .installer
            .generation("LOCAL-COMPOSE", 1, &installed.generation.identity)
            .unwrap()
            .unwrap()
    };
    // Static graph readiness cannot manufacture the original Complete/Applied launch predicate.
    assert_eq!(
        server.require_complete_application_publication(&retained),
        Err(HostProblem::NotFound)
    );
    // A registered second identity proves UACC NONE, not just unknown-user rejection.
    server.bootstrap_identity("MQOTHER", b"TESTPASS").unwrap();
    for (user, queue, intent, expected) in [
        (
            "IBMUSER",
            "LOCAL.QUEUE",
            AccessIntent::Update,
            SecurityDecision::Allow,
        ),
        (
            "MQOTHER",
            "LOCAL.QUEUE",
            AccessIntent::Update,
            SecurityDecision::Deny,
        ),
        (
            "IBMUSER",
            "OTHER.QUEUE",
            AccessIntent::Update,
            SecurityDecision::Deny,
        ),
        (
            "IBMUSER",
            "LOCAL.QUEUE",
            AccessIntent::Control,
            SecurityDecision::Deny,
        ),
    ] {
        assert_eq!(
            server
                .racf
                .authorize(
                    &PrincipalId::new(user, InvocationLimits::default()).unwrap(),
                    "MQQUEUE",
                    &ResourceName::new(queue, 246).unwrap(),
                    intent,
                )
                .unwrap(),
            expected
        );
    }
    put(&server.mq, "graph-put", false);
    let before = graph_rows(store.as_ref());
    drop(server);
    let reopened = fixture(store.clone(), trust);
    assert_eq!(graph_rows(store.as_ref()), before);
    assert_eq!(
        reopened
            .selected_local_application("LOCAL-COMPOSE")
            .unwrap(),
        Some(installed.generation)
    );
    assert_eq!(reopened.mq.queue_depth("LOCAL.QUEUE"), Ok(1));
    assert_eq!(
        reopened.mq.selected_catalog_composition_matches(&mq),
        Ok(())
    );
    assert_eq!(
        reopened
            .batch
            .selected_controller_composition_matches(&batch),
        Ok(())
    );
}
fn definition_artifact(server: &ProductServer) -> String {
    String::from_utf8(
        server
            .store
            .get_provider_state("batch-program", "LOCALPGM")
            .unwrap()
            .unwrap()
            .payload,
    )
    .unwrap()
}
#[test]
fn mq_explicit_process_trigger_graph_owns_signed_catalog_and_controller_on_both_stores() {
    exercise_graph_ready(Arc::new(MemoryStore::new(Default::default())));
    static SERIAL: AtomicU64 = AtomicU64::new(1);
    let root = std::env::temp_dir().join(format!(
        "mq-graph-ready-{}-{}",
        std::process::id(),
        SERIAL.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::create_dir(&root).unwrap();
    let url = format!("sqlite://{}?mode=rwc", root.join("state.sqlite").display());
    exercise_graph_ready(Arc::new(
        SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap(),
    ));
    let store = Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap());
    let server = fixture(store.clone(), Arc::new(test_package_trust()));
    assert_eq!(
        server
            .selected_local_application("LOCAL-COMPOSE")
            .unwrap()
            .unwrap()
            .generation,
        1
    );
    assert_eq!(server.mq.queue_depth("LOCAL.QUEUE"), Ok(1));
    drop((server, store));
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn mq_graph_missing_signed_bytes_kind_cycle_orphan_and_controller_refuse_without_writes() {
    for mode in 0..10 {
        let trust = Arc::new(test_package_trust());
        let store = Arc::new(MemoryStore::new(Default::default()));
        let server = fixture(store.clone(), trust.clone());
        let (mut package, definition) = graph_definitions(&server, &trust, 1);
        match mode {
            0 => package
                .base
                .manifest
                .entries
                .retain(|entry| entry.path != "resource/mq/graph-profile"),
            1 => package.sections.mq_resources[0].target = Some("LOCAL.PROCESS".into()),
            2 => package.sections.mq_resources[1].target = Some("LOCAL.QUEUE".into()),
            3 => package.sections.mq_resources[0].target = None,
            4 => package.sections.mq_resources[0].controller = Some("LOCAL-CONTROLLER".into()),
            5 => package.sections.mq_resources[2].controller = None,
            6 => package.sections.mq_resources[2].target = Some("LOCAL.QUEUE".into()),
            7 => package.sections.mq_resources[2].controller = Some("FOREIGN".into()),
            8 => package.sections.batch_controllers[0]
                .properties
                .insert("selector-program".into(), "OTHERPGM".into())
                .map(|_| ())
                .unwrap(),
            _ => package
                .base
                .manifest
                .entries
                .retain(|entry| entry.path != "resource/mq/LOCAL.PROCESS"),
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
        let before = graph_rows(store.as_ref());
        match server.install_application_package_v2(&package) {
            Ok(expected) => {
                let staged = graph_rows(store.as_ref());
                assert!(
                    server
                        .publish_local_application(&expected, definition)
                        .is_err()
                );
                assert_eq!(graph_rows(store.as_ref()), staged);
            }
            Err(_) => assert_eq!(graph_rows(store.as_ref()), before),
        }
        assert_eq!(
            server.selected_local_application("LOCAL-COMPOSE").unwrap(),
            None
        );
    }
}
#[test]
fn mq_graph_opt_in_keeps_legacy_refusal_and_signed_graph_diagnostics() {
    for mode in 0..3 {
        let trust = Arc::new(test_package_trust());
        let store = Arc::new(MemoryStore::new(Default::default()));
        let server = fixture(store.clone(), trust.clone());
        let (mut package, definition) = graph_definitions(&server, &trust, 1);
        let expected_problem = if mode == 0 {
            package
                .base
                .manifest
                .entries
                .retain(|entry| entry.path != "resource/mq/graph-profile");
            HostProblem::Unsupported
        } else {
            let (path, raw) = if mode == 1 {
                // Keep the signed row consistent with the actual invalid edge,
                // so a blob mismatch cannot stand in for graph validation.
                package.sections.mq_resources[0].target = Some("LOCAL.QUEUE".into());
                (
                    "resource/mq/LOCAL.QUEUE",
                    serde_json::to_vec(&package.sections.mq_resources[0]).unwrap(),
                )
            } else {
                (
                    "resource/mq/graph-profile",
                    b"mainframe-env.local-mq-process-trigger-graph@0".to_vec(),
                )
            };
            let sha = format!("sha256:{:x}", Sha256::digest(&raw));
            let entry = package
                .base
                .manifest
                .entries
                .iter_mut()
                .find(|entry| entry.path == path)
                .unwrap();
            entry.sha256.clone_from(&sha);
            entry.bytes = raw.len();
            package.base.blobs.insert(sha, raw);
            HostProblem::IdempotencyConflict
        };
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
        let before = graph_rows(store.as_ref());
        assert_eq!(
            server
                .publish_local_application(&expected, definition)
                .err(),
            Some(expected_problem),
            "mode {mode}"
        );
        assert_eq!(graph_rows(store.as_ref()), before, "mode {mode}");
        assert_eq!(
            server.selected_local_application("LOCAL-COMPOSE").unwrap(),
            None
        );
    }
}
#[test]
fn mq_graph_retry_upgrade_rollback_keep_real_runtime_and_exact_controller_artifact() {
    let trust = Arc::new(test_package_trust());
    let store = Arc::new(MemoryStore::new(Default::default()));
    let server = fixture(store.clone(), trust.clone());
    let (package, definition) = graph_definitions(&server, &trust, 1);
    let expected = server.install_application_package_v2(&package).unwrap();
    let first = server
        .publish_local_application(&expected, definition)
        .unwrap();
    put(&server.mq, "graph-runtime-edit", false);
    let original = projection(store.as_ref());
    let before = graph_rows(store.as_ref());
    let (_, definition) = graph_definitions(&server, &trust, 1);
    assert!(
        server
            .publish_local_application(&first.generation, definition)
            .unwrap()
            .replayed
    );
    assert_eq!(graph_rows(store.as_ref()), before);
    let (package, definition) = graph_definitions(&server, &trust, 2);
    let expected = server.install_application_package_v2(&package).unwrap();
    server
        .publish_local_application(&expected, definition)
        .unwrap();
    assert_eq!(server.mq.queue_depth("LOCAL.QUEUE"), Ok(1));
    let (_, definition) = graph_definitions(&server, &trust, 1);
    server
        .rollback_local_application(&first.generation, definition)
        .unwrap();
    assert_eq!(projection(store.as_ref()), original);
    assert_eq!(server.mq.queue_depth("LOCAL.QUEUE"), Ok(1));
    assert_eq!(
        definition_artifact(&server),
        server.online_programs.lock().unwrap()["LOCALPGM"].as_str()
    );
    let full = graph_rows(store.as_ref());
    drop(server);
    let reopened = fixture(store.clone(), trust);
    assert_eq!(graph_rows(store.as_ref()), full);
    assert_eq!(
        reopened
            .selected_local_application("LOCAL-COMPOSE")
            .unwrap(),
        Some(first.generation)
    );
}
#[test]
fn mq_graph_unknown_atomic_outcome_and_original_queue_budget_do_not_publish_authority() {
    for outcome in [Outcome::Before, Outcome::After] {
        let trust = Arc::new(test_package_trust());
        let store = Arc::new(FaultStore::new(Arc::new(MemoryStore::new(
            Default::default(),
        ))));
        let server = fixture(store.clone(), trust.clone());
        let (package, definition) = graph_definitions(&server, &trust, 1);
        let expected = server.install_application_package_v2(&package).unwrap();
        let before = graph_rows(store.as_ref());
        store.arm(outcome);
        assert_eq!(
            server.publish_local_application(&expected, definition),
            Err(HostProblem::UnknownOutcome)
        );
        assert_eq!(server.mq.object_catalog(), Err(HostProblem::UnknownOutcome));
        drop(server);
        let reopened = fixture(store.clone(), trust);
        if matches!(outcome, Outcome::Before) {
            assert_eq!(graph_rows(store.as_ref()), before);
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
    let trust = Arc::new(test_package_trust());
    let store = Arc::new(MemoryStore::new(Default::default()));
    let server = fixture(store.clone(), trust.clone());
    let (mut package, _) = graph_definitions(&server, &trust, 1);
    let before_queue_only = graph_rows(store.as_ref());
    assert!(matches!(
        server.mq.prepare_catalog_composition(
            "LOCAL-COMPOSE",
            1,
            &format!("sha256:{}", "a".repeat(64)),
            vec![MqQueueDefinition {
                name: "LOCAL.QUEUE".into(),
                trigger_program: Some("LOCAL.PROCESS".into()),
            }],
            None,
            false
        ),
        Err(HostProblem::Malformed)
    ));
    assert_eq!(graph_rows(store.as_ref()), before_queue_only);
    for index in 0..MqLimits::default().max_queues - 2 {
        package.sections.mq_resources.push(MqResource {
            name: format!("EXTRA.{index}"),
            kind: MqResourceKind::Queue,
            target: None,
            controller: None,
        });
    }
    let before = graph_rows(store.as_ref());
    assert_eq!(
        super::super::super::local_composition::mq::bind_catalog(
            &package,
            None,
            &mut BTreeSet::new()
        ),
        Err(HostProblem::ResourceExhausted)
    );
    assert_eq!(graph_rows(store.as_ref()), before);
}

const LAUNCH_JCL: &[u8] = b"//MQLOCAL JOB CLASS=A\n//RUN EXEC PGM=IKJEFT01\n//SYSTSIN DD *\n  DSN SYSTEM(LOCAL)\n  RUN PROGRAM(LOCALPGM)\n  END\n/*\n";
fn launch_graph_definitions(
    server: &ProductServer,
    trust: &HmacSha256PackageTrust,
) -> (ApplicationPackageV2, LocalApplicationDefinition) {
    let (mut package, definition) = graph_definitions(server, trust, 1);
    for (path, raw) in [
        (
            "resource/mq/controller-launch-profile",
            b"mainframe-env.local-mq-controller-launch@1".as_slice(),
        ),
        ("resource/mq/launch/LOCAL.PROCESS", LAUNCH_JCL),
    ] {
        let sha = format!("sha256:{:x}", Sha256::digest(raw));
        package.base.blobs.insert(sha.clone(), raw.to_vec());
        package.base.manifest.entries.push(PackageEntry {
            path: path.into(),
            kind: EntryKind::Resource,
            sha256: sha,
            bytes: raw.len(),
            depends_on: vec!["program/LOCALPGM".into()],
        });
    }
    resign_package(&mut package, trust);
    (package, definition)
}
fn launch_rows(store: &dyn ProviderStateStore) -> Vec<ProviderStateRecord> {
    let mut full = graph_rows(store);
    for namespace in [
        "application-local-mq-occurrence",
        "batch-local-controller-submission",
    ] {
        full.extend(store.list_provider_state(namespace, 4096).unwrap());
    }
    full.sort_by(|a, b| (&a.namespace, &a.key).cmp(&(&b.namespace, &b.key)));
    full
}
fn exercise_launch_requires_real_publication<S: PlatformStore + 'static>(store: Arc<S>) {
    let trust = Arc::new(test_package_trust());
    let server = fixture(store.clone(), trust.clone());
    let (package, definition) = launch_graph_definitions(&server, &trust);
    let expected = server.install_application_package_v2(&package).unwrap();
    server
        .publish_local_application(&expected, definition)
        .unwrap();
    let before = launch_rows(store.as_ref());
    let caller = invocation("no-complete-authority");
    let request = MqRequest {
        operation: MqOperation::PutOne,
        queue: Some("LOCAL.QUEUE".into()),
        handle: None,
        options: 0,
        message: b"KNOWN-SOURCE-NOT-MINTED".to_vec(),
        message_id: None,
        correlation_id: None,
        wait_ticks: 0,
        max_message_bytes: 128,
        mutation: Some(Mutation {
            sequence: 1,
            transaction: None,
            idempotency_key: IdempotencyKey::new(
                "source-without-complete",
                InvocationLimits::default(),
            )
            .unwrap(),
        }),
    };
    // This is a refusal control. No fabricated Complete row or source Effect is installed.
    assert_eq!(
        server.start_committed_local_mq_controller("LOCAL-COMPOSE", &caller, &request),
        Err(HostProblem::NotFound)
    );
    assert_eq!(launch_rows(store.as_ref()), before);
    assert_eq!(
        store
            .get_provider_state(
                mainframe_env_application::APPLICATION_PUBLICATION_NAMESPACE,
                "LOCAL-COMPOSE"
            )
            .unwrap(),
        None
    );
    assert_eq!(server.mq.queue_depth("LOCAL.QUEUE"), Ok(0));
}
#[test]
fn mq_local_launch_requires_original_complete_publication_without_minting_any_job_on_both_stores() {
    exercise_launch_requires_real_publication(Arc::new(MemoryStore::new(Default::default())));
    static SERIAL: AtomicU64 = AtomicU64::new(1);
    let root = std::env::temp_dir().join(format!(
        "mq-launch-refusal-{}-{}",
        std::process::id(),
        SERIAL.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::create_dir(&root).unwrap();
    exercise_launch_requires_real_publication(Arc::new(
        SqliteStateStore::open(
            &format!("sqlite://{}?mode=rwc", root.join("state.sqlite").display()),
            64 * 1024 * 1024,
            262_144,
        )
        .unwrap(),
    ));
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn mq_local_signed_jcl_exact_selected_program_refuses_foreign_run_extra_steps_and_plan_overflow() {
    let trust = Arc::new(test_package_trust());
    let store = Arc::new(MemoryStore::new(Default::default()));
    let server = fixture(store.clone(), trust.clone());
    let (package, _) = launch_graph_definitions(&server, &trust);
    let controller = super::super::super::batch_controller::decode_application_batch_controller(
        &package,
        &package.sections.batch_controllers[0],
    )
    .unwrap();
    let plan = mainframe_env_batch::parse_jcl(
        &JclBundle {
            primary: std::str::from_utf8(LAUNCH_JCL).unwrap().into(),
            ..Default::default()
        },
        Default::default(),
    )
    .unwrap();
    assert_eq!(plan.name, "MQLOCAL");
    assert_eq!(plan.steps.len(), 1);
    assert_eq!(plan.steps[0].name, "RUN");
    assert_eq!(plan.steps[0].program, "IKJEFT01");
    assert_eq!(plan.steps[0].dds.len(), 1);
    assert_eq!(plan.steps[0].dds[0].name, "SYSTSIN");
    assert_eq!(
        plan.steps[0].dds[0].inline_data,
        b"  DSN SYSTEM(LOCAL)\n  RUN PROGRAM(LOCALPGM)\n  END\n"
    );
    assert_eq!(
        mainframe_env_batch::validate_local_controller_plan(&plan, &controller),
        Ok(())
    );
    let mut wrong = plan.clone();
    wrong.steps[0].dds[0].inline_data = b"RUN PROGRAM(FOREIGN)\n".to_vec();
    assert_eq!(
        mainframe_env_batch::validate_local_controller_plan(&wrong, &controller),
        Err(HostProblem::IdempotencyConflict)
    );
    let mut extra = plan.clone();
    extra.steps.push(plan.steps[0].clone());
    assert_eq!(
        mainframe_env_batch::validate_local_controller_plan(&extra, &controller),
        Err(HostProblem::Unsupported)
    );
    // Authentically re-sign each malformed input so refusal exercises the local
    // launch contract instead of merely failing a stale signature/hash.
    for raw in [LAUNCH_JCL.to_vec(), vec![b' '; 65_537]] {
        let case_store = Arc::new(MemoryStore::new(Default::default()));
        let case_server = fixture(case_store.clone(), trust.clone());
        let (mut package, definition) = launch_graph_definitions(&case_server, &trust);
        let invalid = if raw.len() == LAUNCH_JCL.len() {
            std::str::from_utf8(&raw)
                .unwrap()
                .replace("LOCALPGM", "FOREIGN")
                .into_bytes()
        } else {
            raw
        };
        let sha = format!("sha256:{:x}", Sha256::digest(&invalid));
        let entry = package
            .base
            .manifest
            .entries
            .iter_mut()
            .find(|entry| entry.path == "resource/mq/launch/LOCAL.PROCESS")
            .unwrap();
        entry.bytes = invalid.len();
        entry.sha256 = sha.clone();
        package.base.blobs.insert(sha, invalid);
        resign_package(&mut package, &trust);
        let expected = case_server
            .install_application_package_v2(&package)
            .unwrap();
        let before = launch_rows(case_store.as_ref());
        assert!(
            case_server
                .publish_local_application(&expected, definition)
                .is_err()
        );
        assert_eq!(launch_rows(case_store.as_ref()), before);
    }
}

// Genuine installed compiled legacy MQ source controls; Complete/launch remains a separate owner.
#[path = "local_mq_compiled_source_controls.rs"]
mod compiled_source;
