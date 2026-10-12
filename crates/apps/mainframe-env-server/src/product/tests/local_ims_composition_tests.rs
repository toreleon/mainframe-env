//! Seven genuine fixed-catalog Memory/SQLite controls, with original runtime codecs and no fabricated Complete.
use super::*;
use mainframe_env_host_api::{
    IMS_METADATA_SCHEMA_V1, ImsDatabaseMetadata, ImsDatabaseOrganization, ImsDatabasePcbMetadata,
    ImsDbLevel, ImsFieldMetadata, ImsMetadataCatalog, ImsOperation, ImsPcbMetadata, ImsPsbMetadata,
    ImsRequest, ImsSegmentMetadata, ImsSensitiveSegmentMetadata,
};
use mainframe_env_ims::{ImsCatalogProjection, ImsLimits, ImsService};

const IMS_NAMESPACES: [&str; 9] = [
    "ims-state",
    "ims-v1-database",
    "ims-v1-session-index",
    "ims-v1-checkpoint",
    "ims-v1-replay",
    "ims-v1-unit-of-work",
    "ims-v1-generic-database",
    "ims-v1-generic-unit-of-work",
    "ims-v1-system",
];
fn metadata() -> ImsMetadataCatalog {
    ImsMetadataCatalog {
        schema_version: IMS_METADATA_SCHEMA_V1.into(),
        databases: vec![ImsDatabaseMetadata {
            gsam_format: None,
            name: "LOCALDB".into(),
            version: 1,
            organization: ImsDatabaseOrganization::Hidam,
            segments: vec![ImsSegmentMetadata {
                name: "ROOT".into(),
                parent: None,
                min_length: 3,
                max_length: 3,
                fields: vec![ImsFieldMetadata {
                    name: Some("ROOTKEY".into()),
                    offset: 0,
                    length: 2,
                    sequence: true,
                    unique: true,
                }],
            }],
            secondary_indexes: Vec::new(),
            logical_relationships: Vec::new(),
        }],
        psbs: vec![ImsPsbMetadata {
            name: "LOCALPSB".into(),
            database_level: ImsDbLevel::Current,
            pcbs: vec![ImsPcbMetadata::Database(ImsDatabasePcbMetadata {
                name: "LOCALPCB".into(),
                database: "LOCALDB".into(),
                database_version: Some(1),
                secondary_index: None,
                processing_options: "AP".into(),
                sensitive_segments: vec![ImsSensitiveSegmentMetadata {
                    name: "ROOT".into(),
                    parent: None,
                    processing_options: None,
                }],
            })],
        }],
    }
}
fn signed_metadata(package: &mut ApplicationPackageV2, trust: &HmacSha256PackageTrust) {
    let catalog = metadata();
    let raw = serde_json::to_vec(&catalog).unwrap();
    let sha = format!("sha256:{:x}", Sha256::digest(&raw));
    package.base.blobs.insert(sha.clone(), raw.clone());
    package.base.manifest.entries.push(PackageEntry {
        path: "resource/ims-metadata".into(),
        kind: EntryKind::Resource,
        sha256: sha,
        bytes: raw.len(),
        depends_on: Vec::new(),
    });
    package.sections.ims_metadata = Some(catalog);
    resign_package(package, trust);
}
fn ims_definitions(
    server: &ProductServer,
    trust: &HmacSha256PackageTrust,
    generation: u64,
) -> (ApplicationPackageV2, LocalApplicationDefinition) {
    let (mut package, definition) = definitions(server, trust, generation);
    signed_metadata(&mut package, trust);
    (package, definition)
}
fn install_ims(
    server: &ProductServer,
    trust: &HmacSha256PackageTrust,
    generation: u64,
) -> LocalApplicationReceipt {
    let (package, definition) = ims_definitions(server, trust, generation);
    let expected = server.install_application_package_v2(&package).unwrap();
    server
        .publish_local_application(&expected, definition)
        .unwrap()
}
fn runtime_rows(store: &dyn ProviderStateStore) -> Vec<ProviderStateRecord> {
    let mut rows = Vec::new();
    for namespace in IMS_NAMESPACES {
        rows.extend(store.list_provider_state(namespace, 1024).unwrap());
    }
    rows.sort_by(|a, b| (&a.namespace, &a.key).cmp(&(&b.namespace, &b.key)));
    rows
}
fn rows(store: &dyn ProviderStateStore) -> Vec<ProviderStateRecord> {
    let mut rows = selected_rows(store);
    rows.extend(runtime_rows(store));
    for namespace in [
        "ims-v1-metadata-selection",
        "ims-v1-metadata-generation:LOCAL-COMPOSE",
        "ims-v1-metadata-generation:FOREIGN",
    ] {
        rows.extend(store.list_provider_state(namespace, 65).unwrap());
    }
    rows.sort_by(|a, b| (&a.namespace, &a.key).cmp(&(&b.namespace, &b.key)));
    rows
}
fn current_projection(store: &dyn ProviderStateStore) -> ImsCatalogProjection {
    let row = store
        .get_provider_state("application-local-selection", "LOCAL-COMPOSE")
        .unwrap()
        .unwrap();
    let value: serde_json::Value = serde_json::from_slice(&row.payload).unwrap();
    serde_json::from_value(value["ims"].clone()).unwrap()
}
fn invocation(run: &str) -> Invocation {
    let limits = InvocationLimits::default();
    Invocation::new(
        RequestId::new(format!("request-{run}"), limits).unwrap(),
        ExecutionId::new(format!("execution-{run}"), limits).unwrap(),
        RunUnitId::new(run, limits).unwrap(),
        None,
        Selector::new("program:IMSTEST", limits).unwrap(),
        ArtifactRef::new("artifact", limits).unwrap(),
        Principal::new(
            PrincipalId::new("IBMUSER", limits).unwrap(),
            BTreeSet::from([
                CapabilityId::new("host.ims.read", limits).unwrap(),
                CapabilityId::new("host.ims.write", limits).unwrap(),
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
fn request(run: &str, operation: ImsOperation, sequence: u64, data: &[u8]) -> ImsRequest {
    ImsRequest {
        operation,
        psb: (operation == ImsOperation::Schedule).then(|| "LOCALPSB".into()),
        pcb: 1,
        segments: if operation == ImsOperation::Insert {
            vec!["ROOT".into()]
        } else {
            Vec::new()
        },
        data: data.to_vec(),
        qualifiers: Vec::new(),
        checkpoint_id: None,
        max_segments: 64,
        system: None,
        q_class: None,
        mutation: operation.is_mutating().then(|| Mutation {
            sequence,
            transaction: Some("IMS-LOCAL".into()),
            idempotency_key: IdempotencyKey::new(
                format!("{run}-{sequence}"),
                InvocationLimits::default(),
            )
            .unwrap(),
        }),
    }
}
fn edit(service: &ImsService, run: &str, settle: bool) {
    let invocation = invocation(run);
    assert_eq!(
        service
            .execute(&invocation, &request(run, ImsOperation::Schedule, 1, b""))
            .unwrap()
            .status,
        "  "
    );
    assert_eq!(
        service
            .execute(&invocation, &request(run, ImsOperation::Insert, 2, b"A1X"))
            .unwrap()
            .affected_segments,
        1
    );
    if settle {
        assert_eq!(
            service
                .execute(&invocation, &request(run, ImsOperation::Commit, 3, b""))
                .unwrap()
                .status,
            "  "
        );
        assert_eq!(
            service
                .execute(&invocation, &request(run, ImsOperation::Terminate, 4, b""))
                .unwrap()
                .status,
            "  "
        );
    }
}
fn exercise_ready<S: PlatformStore + 'static>(store: Arc<S>) {
    let trust = Arc::new(test_package_trust());
    let server = fixture(store.clone(), trust.clone());
    let installed = install_ims(&server, &trust, 1);
    let projection = current_projection(store.as_ref());
    assert_eq!(projection.catalog, metadata());
    assert_eq!(projection.generation, 1);
    let selected = server
        .ims
        .selected_metadata_generation("LOCAL-COMPOSE")
        .unwrap()
        .unwrap();
    assert_eq!(selected.catalog, metadata());
    assert_eq!(selected.package_identity, installed.generation.identity);
    assert_eq!(
        server.ims.selected_catalog_composition_matches(&projection),
        Ok(())
    );
    assert!(
        store
            .get_provider_state("ims-state", "catalog")
            .unwrap()
            .is_some()
    );
    assert!(
        store
            .get_provider_state("ims-v1-generic-database", "LOCALDB")
            .unwrap()
            .is_some()
    );
    let before = rows(store.as_ref());
    drop(server);
    let reopened = fixture(store.clone(), trust);
    assert_eq!(rows(store.as_ref()), before);
    assert_eq!(
        reopened
            .selected_local_application("LOCAL-COMPOSE")
            .unwrap(),
        Some(installed.generation)
    );
}
#[test]
fn ims_full_signed_catalog_and_real_provider_images_reopen_on_both_stores() {
    exercise_ready(Arc::new(MemoryStore::new(Default::default())));
    static SERIAL: AtomicU64 = AtomicU64::new(1);
    let root = std::env::temp_dir().join(format!(
        "ims-ready-{}-{}",
        std::process::id(),
        SERIAL.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::create_dir(&root).unwrap();
    let url = format!("sqlite://{}?mode=rwc", root.join("state.sqlite").display());
    exercise_ready(Arc::new(
        SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap(),
    ));
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn ims_fixed_catalog_retry_generation_and_rollback_preserve_actual_runtime_payloads() {
    let trust = Arc::new(test_package_trust());
    let store = Arc::new(MemoryStore::new(Default::default()));
    let server = fixture(store.clone(), trust.clone());
    let first = install_ims(&server, &trust, 1);
    drop(server);
    let service = ImsService::open(store.clone(), Default::default()).unwrap();
    edit(&service, "runtime", true);
    drop(service);
    let server = fixture(store.clone(), trust.clone());
    let actual_runtime = runtime_rows(store.as_ref());
    let before = rows(store.as_ref());
    let (_, definition) = ims_definitions(&server, &trust, 1);
    assert!(
        server
            .publish_local_application(&first.generation, definition)
            .unwrap()
            .replayed
    );
    assert_eq!(rows(store.as_ref()), before);
    install_ims(&server, &trust, 2);
    assert_eq!(runtime_rows(store.as_ref()), actual_runtime);
    let (_, definition) = ims_definitions(&server, &trust, 1);
    server
        .rollback_local_application(&first.generation, definition)
        .unwrap();
    assert_eq!(runtime_rows(store.as_ref()), actual_runtime);
    assert_eq!(
        server
            .ims
            .selected_metadata_generation("LOCAL-COMPOSE")
            .unwrap()
            .unwrap()
            .generation,
        1
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
fn ims_foreign_current_catalog_and_legacy_adoption_refuse_before_retry() {
    for legacy in [false, true] {
        let trust = Arc::new(test_package_trust());
        let store = Arc::new(MemoryStore::new(Default::default()));
        let server = fixture(store.clone(), trust.clone());
        let installed = install_ims(&server, &trust, 1);
        drop(server);
        let service = ImsService::open(store.clone(), Default::default()).unwrap();
        if !legacy {
            service
                .publish_metadata_generation(
                    "FOREIGN",
                    1,
                    &format!("sha256:{}", "a".repeat(64)),
                    Some(&metadata()),
                )
                .unwrap();
        }
        let before = rows(store.as_ref());
        let prior = current_projection(store.as_ref());
        if legacy {
            assert!(matches!(
                service.prepare_catalog_composition(
                    "LOCAL-COMPOSE",
                    1,
                    &installed.generation.identity,
                    metadata(),
                    None,
                    false
                ),
                Err(HostProblem::IdempotencyConflict)
            ));
        } else {
            assert!(matches!(
                service.prepare_catalog_composition(
                    "LOCAL-COMPOSE",
                    1,
                    &installed.generation.identity,
                    metadata(),
                    Some(&prior),
                    false
                ),
                Err(HostProblem::IdempotencyConflict)
            ));
        }
        assert_eq!(rows(store.as_ref()), before);
    }
}
#[test]
fn ims_exact_signed_metadata_schema_and_missing_resource_refuse_without_mutation() {
    for mode in 0..3 {
        let trust = Arc::new(test_package_trust());
        let store = Arc::new(MemoryStore::new(Default::default()));
        let server = fixture(store.clone(), trust.clone());
        let (mut package, definition) = ims_definitions(&server, &trust, 1);
        match mode {
            0 => package
                .base
                .manifest
                .entries
                .retain(|entry| entry.path != "resource/ims-metadata"),
            1 => {
                package.sections.ims_metadata.as_mut().unwrap().databases[0].segments[0]
                    .max_length = 4
            }
            _ => {
                package
                    .base
                    .manifest
                    .entries
                    .iter_mut()
                    .find(|entry| entry.path == "resource/ims-metadata")
                    .unwrap()
                    .kind = EntryKind::Data;
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
        assert!(
            server
                .publish_local_application(&expected, definition)
                .is_err()
        );
        assert_eq!(rows(store.as_ref()), before);
        assert_eq!(
            server.selected_local_application("LOCAL-COMPOSE").unwrap(),
            None
        );
    }
}
#[test]
fn ims_unknown_shared_commit_and_wrong_store_acknowledgement_quarantine_until_reopen() {
    for outcome in [Outcome::Before, Outcome::After] {
        let trust = Arc::new(test_package_trust());
        let store = Arc::new(FaultStore::new(Arc::new(MemoryStore::new(
            Default::default(),
        ))));
        let server = fixture(store.clone(), trust.clone());
        let (package, definition) = ims_definitions(&server, &trust, 1);
        let expected = server.install_application_package_v2(&package).unwrap();
        let before = rows(store.as_ref());
        store.arm(outcome);
        assert_eq!(
            server.publish_local_application(&expected, definition),
            Err(HostProblem::UnknownOutcome)
        );
        assert_eq!(
            server.ims.checkpoint_count(),
            Err(HostProblem::UnknownOutcome)
        );
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
    let service = ImsService::open(left.clone(), Default::default()).unwrap();
    let prepared = service
        .prepare_catalog_composition(
            "LOCAL-COMPOSE",
            1,
            &format!("sha256:{}", "a".repeat(64)),
            metadata(),
            None,
            false,
        )
        .unwrap();
    let commit = ProviderCompositionCommit::publish(right, prepared.writes().to_vec()).unwrap();
    assert_eq!(prepared.finish(&commit), Err(HostProblem::UnknownOutcome));
    assert!(rows(left.as_ref()).is_empty());
    assert_eq!(service.checkpoint_count(), Err(HostProblem::UnknownOutcome));
}
#[test]
fn ims_sqlite_companion_conflict_preserves_full_rows_and_retained_selection() {
    static SERIAL: AtomicU64 = AtomicU64::new(1);
    let root = std::env::temp_dir().join(format!(
        "ims-cas-{}-{}",
        std::process::id(),
        SERIAL.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::create_dir(&root).unwrap();
    let url = format!("sqlite://{}?mode=rwc", root.join("state.sqlite").display());
    let trust = Arc::new(test_package_trust());
    let store = Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap());
    let server = fixture(store.clone(), trust.clone());
    install_ims(&server, &trust, 1);
    let prior = current_projection(store.as_ref());
    let (package, definition) = ims_definitions(&server, &trust, 2);
    let expected = server.install_application_package_v2(&package).unwrap();
    let companion = ProviderStateRecord {
        namespace: "ims-composition-companion".into(),
        key: "owned".into(),
        version: 1,
        payload: b"UNCHANGED".to_vec(),
    };
    store.put_provider_state(companion.clone(), None).unwrap();
    let before = rows(store.as_ref());
    let prepared = server
        .ims
        .prepare_catalog_composition(
            "LOCAL-COMPOSE",
            2,
            &expected.identity,
            metadata(),
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
    drop(reopened);
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn ims_real_pending_capacity_changed_schema_and_actual_version_refuse_without_writes() {
    let trust = Arc::new(test_package_trust());
    let store = Arc::new(MemoryStore::new(Default::default()));
    let server = fixture(store.clone(), trust.clone());
    let installed = install_ims(&server, &trust, 1);
    let prior = current_projection(store.as_ref());
    drop(server);
    let service = ImsService::open(store.clone(), Default::default()).unwrap();
    edit(&service, "pending", false);
    let before = rows(store.as_ref());
    assert!(matches!(
        service.prepare_catalog_composition(
            "LOCAL-COMPOSE",
            2,
            &format!("sha256:{}", "b".repeat(64)),
            metadata(),
            Some(&prior),
            false
        ),
        Err(HostProblem::IdempotencyConflict)
    ));
    assert_eq!(rows(store.as_ref()), before);
    let mut stale = prior.clone();
    stale.catalog.databases[0].segments[0].max_length = 4;
    stale.metadata_identity =
        mainframe_env_ims::validate_ims_metadata(&stale.catalog, Default::default())
            .unwrap()
            .digest;
    assert_eq!(
        service.selected_catalog_composition_matches(&stale),
        Err(HostProblem::IdempotencyConflict)
    );
    assert_eq!(rows(store.as_ref()), before);
    let limited = ImsService::open(
        Arc::new(MemoryStore::new(Default::default())),
        ImsLimits {
            max_databases: 0,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(matches!(
        limited.prepare_catalog_composition(
            "LOCAL-COMPOSE",
            1,
            &installed.generation.identity,
            metadata(),
            None,
            false
        ),
        Err(HostProblem::ResourceExhausted)
    ));
    let actual = store
        .get_provider_state("ims-v1-generic-database", "LOCALDB")
        .unwrap()
        .unwrap();
    store
        .put_provider_state(
            ProviderStateRecord {
                version: actual.version + 1,
                ..actual.clone()
            },
            Some(actual.version),
        )
        .unwrap();
    let before = rows(store.as_ref());
    assert_eq!(
        service.selected_catalog_composition_matches(&prior),
        Err(HostProblem::IdempotencyConflict)
    );
    assert_eq!(rows(store.as_ref()), before);
}
