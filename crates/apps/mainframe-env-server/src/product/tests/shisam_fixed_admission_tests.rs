//! Independent signed-install/publication SHISAM fixed-layout admission proofs.
use super::*;
use mainframe_env_host_api::{
    IMS_METADATA_SCHEMA_V1, ImsDatabaseMetadata, ImsDatabaseOrganization, ImsDatabasePcbMetadata,
    ImsDbLevel, ImsFieldMetadata, ImsMetadataCatalog, ImsOperation, ImsPcbMetadata, ImsPsbMetadata,
    ImsQualifier, ImsRequest, ImsSegmentMetadata, ImsSensitiveSegmentMetadata, Mutation,
};
use mainframe_env_store_api::ProviderStateRecord;

const APP: &str = "SIGNED-SHISAM-FIXED";

fn metadata() -> ImsMetadataCatalog {
    ImsMetadataCatalog {
        schema_version: IMS_METADATA_SCHEMA_V1.into(),
        databases: vec![ImsDatabaseMetadata {
            gsam_format: None,
            name: "FIXDB".into(),
            version: 1,
            organization: ImsDatabaseOrganization::Shisam,
            segments: vec![ImsSegmentMetadata {
                name: "ROOT".into(),
                parent: None,
                min_length: 3,
                max_length: 3,
                fields: vec![ImsFieldMetadata {
                    name: Some("KEY".into()),
                    offset: 0,
                    length: 2,
                    sequence: true,
                    unique: true,
                }],
            }],
            secondary_indexes: vec![],
            logical_relationships: vec![],
        }],
        psbs: vec![ImsPsbMetadata {
            name: "FIXPSB".into(),
            database_level: ImsDbLevel::Current,
            pcbs: vec![ImsPcbMetadata::Database(ImsDatabasePcbMetadata {
                name: "DBPCB".into(),
                database: "FIXDB".into(),
                database_version: Some(1),
                processing_options: "AP".into(),
                secondary_index: None,
                sensitive_segments: vec![ImsSensitiveSegmentMetadata {
                    name: "ROOT".into(),
                    parent: None,
                    processing_options: None,
                }],
            })],
        }],
    }
}

fn package(variable: bool) -> ApplicationPackageV2 {
    let trust = test_package_trust();
    let mut package = signed_controller_package(&trust);
    package.base.manifest.name = APP.into();
    let mut metadata = metadata();
    if variable {
        metadata.databases[0].segments[0].max_length = 4;
    }
    package.sections.ims_metadata = Some(metadata);
    resign_package(&mut package, &trust);
    package
}

fn invocation() -> Invocation {
    let limits = InvocationLimits::default();
    Invocation::new(
        RequestId::new("shisam-request", limits).unwrap(),
        ExecutionId::new("shisam-execution", limits).unwrap(),
        RunUnitId::new("signed-shisam-run", limits).unwrap(),
        None,
        Selector::new("ims:shisam", limits).unwrap(),
        ArtifactRef::new("ims:shisam", limits).unwrap(),
        Principal::new(
            PrincipalId::new("IBMUSER", limits).unwrap(),
            ["host.ims.read", "host.ims.write"]
                .into_iter()
                .map(|s| CapabilityId::new(s, limits).unwrap())
                .collect(),
            limits,
        )
        .unwrap(),
        ServiceClass::Batch,
        0,
        u64::MAX,
        TraceId::new("shisam-trace", limits).unwrap(),
        IdempotencyKey::new("signed-shisam-invocation", limits).unwrap(),
        1,
        ResourceLimits::default(),
        BTreeMap::new(),
        limits,
    )
    .unwrap()
}

fn request(operation: ImsOperation, sequence: u64, data: &[u8], key: Option<&[u8]>) -> ImsRequest {
    ImsRequest {
        operation,
        psb: (operation == ImsOperation::Schedule).then(|| "FIXPSB".into()),
        pcb: 1,
        segments: matches!(
            operation,
            ImsOperation::Insert | ImsOperation::GetUnique | ImsOperation::GetHoldUnique
        )
        .then(|| "ROOT".into())
        .into_iter()
        .collect(),
        qualifiers: key
            .map(|key| ImsQualifier {
                segment: "ROOT".into(),
                field: "KEY".into(),
                value: key.to_vec(),
            })
            .into_iter()
            .collect(),
        data: data.to_vec(),
        checkpoint_id: None,
        max_segments: 16,
        mutation: operation.is_mutating().then(|| Mutation {
            sequence,
            idempotency_key: IdempotencyKey::new(
                format!("signed-shisam-{sequence}"),
                InvocationLimits::default(),
            )
            .unwrap(),
            transaction: None,
        }),
        system: None,
        q_class: None,
    }
}

fn rows(store: &dyn PlatformStore) -> Vec<ProviderStateRecord> {
    let mut rows = store.list_provider_state_prefix("ims", 4096).unwrap();
    rows.extend(
        store
            .list_provider_state_prefix("application", 4096)
            .unwrap(),
    );
    rows
}

fn open(store: Arc<dyn PlatformStore>, cfg: ServerConfig) -> Arc<ProductServer> {
    if cfg.store_profile == crate::StoreProfile::Postgres {
        let url = std::env::var("SHISAM_POSTGRES_URL").unwrap();
        let artifacts =
            mainframe_env_store::PostgresArtifactStore::open(&url, 64 * 1024 * 1024, 262_144)
                .unwrap();
        return ProductServer::open_with_package_trust_and_artifact_store(
            cfg,
            store,
            Arc::new(MemorySecretResolver::default()),
            default_program_router(),
            Arc::new(test_package_trust()),
            Arc::new(artifacts),
        )
        .unwrap();
    }
    ProductServer::open_with_package_trust(
        cfg,
        store,
        Arc::new(MemorySecretResolver::default()),
        default_program_router(),
        Arc::new(test_package_trust()),
    )
    .unwrap()
}

fn sqlite(name: &str) -> (Arc<dyn PlatformStore>, ServerConfig, std::path::PathBuf) {
    let dir = std::env::var_os("SHISAM_RECEIPT_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let path = dir.join(format!(
        "signed-shisam-{name}-{}.sqlite",
        std::process::id()
    ));
    let url = format!("sqlite:{}?mode=rwc", path.display());
    let mut cfg = config();
    cfg.store_profile = crate::StoreProfile::Sqlite;
    cfg.sqlite_url = url.clone();
    (
        Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap()),
        cfg,
        path,
    )
}

fn reject_variable(store: Arc<dyn PlatformStore>, cfg: ServerConfig) {
    let server = open(store.clone(), cfg);
    let before = rows(&*store);
    let actual = server.install_application_package_v2(&package(true));
    let after_stage = rows(&*store);
    eprintln!(
        "SHISAM_SIGNED_VARIABLE_STAGE_ACTUAL={actual:?}; BEFORE_ROWS={}; AFTER_ROWS={}",
        before.len(),
        after_stage.len()
    );
    if let Ok(staged) = &actual {
        let published = server.publish_application_generation(staged);
        eprintln!("SHISAM_SIGNED_VARIABLE_PUBLICATION_ACTUAL={published:?}");
        eprintln!(
            "SHISAM_SIGNED_VARIABLE_SELECTED={:?}",
            server
                .ims_service()
                .selected_metadata_generation(APP)
                .unwrap()
        );
        eprintln!(
            "SHISAM_SIGNED_VARIABLE_ROWS={}",
            serde_json::to_string(
                &rows(&*store)
                    .iter()
                    .map(|row| (&row.namespace, &row.key, row.version, &row.payload))
                    .collect::<Vec<_>>()
            )
            .unwrap()
        );
    }
    assert_eq!(actual, Err(HostProblem::Malformed));
    assert_eq!(
        rows(&*store),
        before,
        "rejected signed metadata must not create a generation or partial rows"
    );
    assert_eq!(after_stage, before);
    assert_eq!(
        server
            .ims_service()
            .selected_metadata_generation(APP)
            .unwrap(),
        None
    );
}

fn fixed_programming(store: Arc<dyn PlatformStore>, cfg: ServerConfig) {
    let server = open(store.clone(), cfg.clone());
    let package = package(false);
    let staged = server.install_application_package_v2(&package).unwrap();
    let published = server.publish_application_generation(&staged).unwrap();
    assert!(published.ims_metadata);
    assert!(!published.replayed);
    assert!(
        server
            .publish_application_generation(&staged)
            .unwrap()
            .replayed
    );
    server
        .bootstrap_administrator("IBMUSER", b"TESTPASS")
        .unwrap();
    for (class, name) in [("IMSPSB", "FIXPSB"), ("IMSDB", "FIXDB")] {
        server
            .racf
            .define_profile(class, name, "IBMUSER", Some(AccessIntent::Control))
            .unwrap();
    }
    let inv = invocation();
    let execute = |r: &ImsRequest| server.ims_execute_selected(APP, &inv, r).unwrap();
    assert_eq!(
        execute(&request(ImsOperation::Schedule, 1, &[], None)).status,
        "  "
    );
    let first = request(ImsOperation::Insert, 2, b"B2Y", None);
    let receipt = execute(&first);
    assert_eq!(receipt.status, "  ");
    assert_eq!(receipt.affected_segments, 1);
    assert_eq!(
        execute(&request(ImsOperation::Insert, 3, b"A1X", None)).affected_segments,
        1
    );
    assert_eq!(
        execute(&request(ImsOperation::Commit, 4, &[], None)).status,
        "  "
    );
    for (sequence, operation) in [
        (5, ImsOperation::GetUnique),
        (6, ImsOperation::GetHoldUnique),
    ] {
        let got = execute(&request(operation, sequence, &[], Some(b"A1")));
        assert_eq!(got.status, "  ");
        assert_eq!(got.segments[0].data, b"A1X");
    }
    let before = rows(&*store);
    assert_eq!(execute(&first), receipt);
    assert_eq!(rows(&*store), before);
    assert_eq!(
        execute(&request(ImsOperation::GetNext, 7, &[], None)).segments[0].data,
        b"B2Y"
    );
    let before = rows(&*store);
    drop(server);
    let reopened = open(store.clone(), cfg);
    assert_eq!(
        rows(&*store),
        before,
        "selected metadata and good images remain byte-exact on reopen"
    );
    let selected = reopened
        .ims_service()
        .selected_metadata_generation(APP)
        .unwrap()
        .unwrap();
    assert_eq!(selected.generation, staged.generation);
    assert_eq!(selected.package_identity, staged.identity);
    assert_eq!(selected.catalog, package.sections.ims_metadata.unwrap());
    assert_eq!(
        reopened.ims_execute_selected(APP, &inv, &first).unwrap(),
        receipt
    );
    assert_eq!(rows(&*store), before);
    assert_eq!(
        reopened
            .ims_execute_selected(
                APP,
                &inv,
                &request(ImsOperation::GetUnique, 8, &[], Some(b"A1"))
            )
            .unwrap()
            .segments[0]
            .data,
        b"A1X"
    );
    assert_eq!(
        reopened
            .ims_execute_selected(APP, &inv, &request(ImsOperation::GetNext, 9, &[], None))
            .unwrap()
            .segments[0]
            .data,
        b"B2Y"
    );
    eprintln!("SHISAM_FIXED_SIGNED_PROGRAMMING_REPLAY_REOPEN_PASS");
}

#[test]
fn shisam_fixed_layout_signed_rejects_variable_memory() {
    reject_variable(Arc::new(MemoryStore::new(Default::default())), config());
}

#[test]
fn shisam_fixed_layout_signed_rejects_variable_sqlite() {
    let (store, cfg, path) = sqlite("variable");
    eprintln!("SHISAM_SIGNED_VARIABLE_SQLITE={}", path.display());
    reject_variable(store, cfg);
}

#[test]
fn shisam_fixed_layout_signed_fixed_programming_memory() {
    fixed_programming(Arc::new(MemoryStore::new(Default::default())), config());
}

#[test]
fn shisam_fixed_layout_signed_fixed_programming_sqlite() {
    let (store, cfg, path) = sqlite("fixed-control");
    fixed_programming(store, cfg);
    if std::env::var_os("SHISAM_CAPTURE_GOOD_DIR").is_some() {
        eprintln!("SHISAM_CAPTURE_GOOD_SIGNED={}", path.display());
    } else {
        std::fs::remove_file(path).unwrap();
    }
}

#[test]
#[ignore = "requires exact externally retained signed SQLite state"]
fn shisam_fixed_layout_signed_historical_reader() {
    let path = std::path::PathBuf::from(std::env::var_os("SHISAM_SIGNED_READER_DB").unwrap());
    let url = format!("sqlite:{}?mode=rw", path.display());
    let store: Arc<dyn PlatformStore> =
        Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap());
    let before = rows(&*store);
    let registry = store
        .get_provider_state(APPLICATION_V2_STATE_NAMESPACE, APPLICATION_V2_STATE_KEY)
        .unwrap()
        .unwrap();
    let result = ApplicationInstallerV2::from_state_payload(
        "0.2.0",
        PackageLimits::default(),
        Arc::new(test_package_trust()),
        &registry.payload,
    )
    .map_err(application_install_problem);
    let ims = mainframe_env_ims::ImsService::open(store.clone(), Default::default()).unwrap();
    let selected = ims.selected_metadata_generation(APP);
    if std::env::var("SHISAM_SIGNED_LAYOUT").unwrap() == "good" {
        result.unwrap();
        let selected = selected.unwrap().unwrap();
        assert_eq!(selected.catalog, metadata());
        assert_eq!(selected.generation, 1);
    } else {
        match std::env::var("SHISAM_READER_EXPECTATION").unwrap().as_str() {
            "old" => {
                result.unwrap();
                assert_eq!(
                    selected.unwrap().unwrap().catalog.databases[0].segments[0].max_length,
                    4
                );
            }
            "new" => {
                assert!(
                    matches!(result, Err(HostProblem::Malformed)),
                    "package registry reopen has its existing Malformed mapping"
                );
                assert_eq!(
                    selected,
                    Err(HostProblem::InfrastructureFailure),
                    "selected-generation decoder has its own InfrastructureFailure mapping"
                );
            }
            other => panic!("Invalid reader expectation: {other}"),
        }
    }
    assert_eq!(
        rows(&*store),
        before,
        "readers must preserve package, metadata and IMS payloads and versions"
    );
    eprintln!("SHISAM_SIGNED_PACKAGE_AND_SELECTION_HISTORICAL_READER_PASS");
}

fn postgres() -> (Arc<dyn PlatformStore>, ServerConfig) {
    let url = std::env::var("SHISAM_POSTGRES_URL").unwrap();
    let store =
        mainframe_env_store::PostgresStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap();
    let mut cfg = config();
    cfg.store_profile = crate::StoreProfile::Postgres;
    cfg.artifact_profile = ArtifactProfile::Shared;
    cfg.postgres_url_reference = Some("env-base64:MAINFRAME_ENV_SECRET_SHISAM_POSTGRES".into());
    (Arc::new(store), cfg)
}

#[test]
#[ignore = "requires isolated PostgreSQL18.6 signed rejection database"]
fn shisam_fixed_layout_signed_rejects_variable_postgres() {
    let (store, cfg) = postgres();
    reject_variable(store, cfg);
    eprintln!("SHISAM_POSTGRES_SIGNED_REJECTION_PASS");
}

#[test]
#[ignore = "requires isolated PostgreSQL18.6 signed programming database"]
fn shisam_fixed_layout_signed_fixed_programming_postgres() {
    let (store, cfg) = postgres();
    fixed_programming(store, cfg);
    eprintln!("SHISAM_POSTGRES_SIGNED_PROGRAMMING_PASS");
}
