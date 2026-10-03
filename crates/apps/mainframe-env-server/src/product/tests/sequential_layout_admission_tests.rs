//! Independent signed sequential-layout admission fail-first witnesses.
use super::*;
use mainframe_env_host_api::{
    IMS_METADATA_SCHEMA_V1, ImsDatabaseMetadata, ImsDatabaseOrganization, ImsDatabasePcbMetadata,
    ImsDbLevel, ImsFieldMetadata, ImsMetadataCatalog, ImsOperation, ImsPcbMetadata, ImsPsbMetadata,
    ImsQualifier, ImsRequest, ImsSegmentMetadata, ImsSensitiveSegmentMetadata, Mutation,
};
use mainframe_env_store_api::ProviderStateRecord;

const SEQ_LAYOUT_APP: &str = "SIGNED-SEQUENTIAL-LAYOUT";

fn seq_layout_catalog(case: &str) -> ImsMetadataCatalog {
    let mut value = seq_layout_base_catalog();
    let org = if case.starts_with("hsam") {
        ImsDatabaseOrganization::Hsam
    } else if case.starts_with("shsam") {
        ImsDatabaseOrganization::Shsam
    } else if case.starts_with("hisam") {
        ImsDatabaseOrganization::Hisam
    } else if case.starts_with("hidam") {
        ImsDatabaseOrganization::Hidam
    } else {
        ImsDatabaseOrganization::Shisam
    };
    value.databases[0].organization = org;
    if org == ImsDatabaseOrganization::Hsam || case.ends_with("multiple") {
        let mut child = value.databases[0].segments[0].clone();
        child.name = "CHILD".into();
        child.parent = Some("ROOT".into());
        value.databases[0].segments.push(child);
        let ImsPcbMetadata::Database(pcb) = &mut value.psbs[0].pcbs[0] else {
            unreachable!()
        };
        pcb.sensitive_segments.push(ImsSensitiveSegmentMetadata {
            name: "CHILD".into(),
            parent: Some("ROOT".into()),
            processing_options: None,
        });
    }
    if case.contains("variable")
        || matches!(
            org,
            ImsDatabaseOrganization::Hisam | ImsDatabaseOrganization::Hidam
        )
    {
        let index = usize::from(case == "hsam-dependent-variable");
        value.databases[0].segments[index].max_length = 4;
    }
    value
}

fn seq_layout_base_catalog() -> ImsMetadataCatalog {
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

fn seq_layout_package(case: &str) -> ApplicationPackageV2 {
    let trust = test_package_trust();
    let mut package = signed_controller_package(&trust);
    package.base.manifest.name = SEQ_LAYOUT_APP.into();
    let metadata = seq_layout_catalog(case);
    package.sections.ims_metadata = Some(metadata);
    resign_package(&mut package, &trust);
    package
}

fn seq_layout_invocation() -> Invocation {
    let limits = InvocationLimits::default();
    Invocation::new(
        RequestId::new("seq-layout-seq_layout_request", limits).unwrap(),
        ExecutionId::new("seq-layout-execution", limits).unwrap(),
        RunUnitId::new("signed-seq-layout-run", limits).unwrap(),
        None,
        Selector::new("ims:seq-layout", limits).unwrap(),
        ArtifactRef::new("ims:seq-layout", limits).unwrap(),
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
        TraceId::new("seq-layout-trace", limits).unwrap(),
        IdempotencyKey::new("signed-seq-layout-seq_layout_invocation", limits).unwrap(),
        1,
        ResourceLimits::default(),
        BTreeMap::new(),
        limits,
    )
    .unwrap()
}

fn seq_layout_request(
    operation: ImsOperation,
    sequence: u64,
    data: &[u8],
    key: Option<&[u8]>,
) -> ImsRequest {
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
                format!("signed-seq-layout-{sequence}"),
                InvocationLimits::default(),
            )
            .unwrap(),
            transaction: None,
        }),
        system: None,
        q_class: None,
    }
}

fn seq_layout_rows(store: &dyn PlatformStore) -> Vec<ProviderStateRecord> {
    // Fresh stores contain only owner-created ASCII namespaces. The API requires
    // a nonempty prefix; scan every non-NUL ASCII first byte, not selected owners.
    let mut rows = Vec::new();
    for first in 1u8..=127 {
        let prefix = char::from(first).to_string();
        let part = store.list_provider_state_prefix(&prefix, 4096).unwrap();
        assert!(part.len() < 4096, "bounded provider scan must not truncate");
        rows.extend(part);
    }
    rows.sort_by(|a, b| (&a.namespace, &a.key).cmp(&(&b.namespace, &b.key)));
    rows
}

fn seq_layout_open(store: Arc<dyn PlatformStore>, cfg: ServerConfig) -> Arc<ProductServer> {
    if cfg.store_profile == crate::StoreProfile::Postgres {
        let artifacts = mainframe_env_store::PostgresArtifactStore::open(
            &std::env::var("SEQ_LAYOUT_POSTGRES_URL").unwrap(),
            64 * 1024 * 1024,
            262_144,
        )
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

fn seq_layout_sqlite(name: &str) -> (Arc<dyn PlatformStore>, ServerConfig, std::path::PathBuf) {
    let dir = std::env::var_os("SEQ_LAYOUT_RECEIPT_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let path = dir.join(format!(
        "signed-seq-layout-{name}-{}.sqlite",
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

fn seq_layout_load(case: &str, sequence: u64) -> ImsRequest {
    use mainframe_env_ims::{ImsGenericLoadImage, ImsGenericLoadRecord};
    let catalog = seq_layout_catalog(case);
    let two = catalog.databases[0].segments.len() == 2;
    let mut records = vec![ImsGenericLoadRecord {
        segment: "ROOT".into(),
        parent: None,
        data: if case == "hsam-root-variable" {
            b"A1XY".to_vec()
        } else {
            b"A1X".to_vec()
        },
    }];
    if two {
        records.push(ImsGenericLoadRecord {
            segment: "CHILD".into(),
            parent: Some(0),
            data: if case == "hsam-dependent-variable" {
                b"C1XY".to_vec()
            } else {
                b"C1Y".to_vec()
            },
        });
    }
    records.push(ImsGenericLoadRecord {
        segment: "ROOT".into(),
        parent: None,
        data: if case == "hsam-root-variable" {
            b"B2YZ".to_vec()
        } else {
            b"B2Y".to_vec()
        },
    });
    let image = ImsGenericLoadImage {
        database: "FIXDB".into(),
        records,
    };
    seq_layout_request(
        ImsOperation::Load,
        sequence,
        &serde_json::to_vec(&image).unwrap(),
        None,
    )
}

fn seq_layout_log_rows(label: &str, store: &dyn PlatformStore) {
    eprintln!(
        "SEQ_LAYOUT_SIGNED_{label}_ROWS={}",
        serde_json::to_string(
            &seq_layout_rows(store)
                .iter()
                .map(|r| (&r.namespace, &r.key, r.version, &r.payload))
                .collect::<Vec<_>>()
        )
        .unwrap()
    );
}
fn seq_layout_authorize(server: &ProductServer) {
    server
        .bootstrap_administrator("IBMUSER", b"TESTPASS")
        .unwrap();
    for (class, name) in [("IMSPSB", "FIXPSB"), ("IMSDB", "FIXDB")] {
        server
            .racf
            .define_profile(class, name, "IBMUSER", Some(AccessIntent::Control))
            .unwrap();
    }
}
fn seq_layout_reject(store: Arc<dyn PlatformStore>, cfg: ServerConfig, case: &str) {
    let server = seq_layout_open(store.clone(), cfg);
    let before = seq_layout_rows(&*store);
    let selected_before = server
        .ims_service()
        .selected_metadata_generation(SEQ_LAYOUT_APP)
        .unwrap();
    let package = seq_layout_package(case);
    eprintln!(
        "SEQ_LAYOUT_SIGNED_METADATA={}",
        serde_json::to_string(&package.sections.ims_metadata).unwrap()
    );
    let actual = server.install_application_package_v2(&package);
    let after_stage = seq_layout_rows(&*store);
    eprintln!(
        "SEQ_LAYOUT_SIGNED_CASE={case}; ACTUAL_STAGE={actual:?}; BEFORE={}; AFTER_STAGE={}",
        before.len(),
        after_stage.len()
    );
    seq_layout_log_rows("AFTER_STAGE", &*store);
    if let Ok(staged) = &actual {
        let publication = server.publish_application_generation(staged);
        eprintln!("SEQ_LAYOUT_SIGNED_CASE={case}; ACTUAL_PUBLICATION={publication:?}");
        seq_layout_log_rows("AFTER_PUBLICATION", &*store);
        eprintln!(
            "SEQ_LAYOUT_SIGNED_SELECTION={:?}",
            server
                .ims_service()
                .selected_metadata_generation(SEQ_LAYOUT_APP)
                .unwrap()
        );
        if publication.is_ok() {
            seq_layout_authorize(&server);
            let inv = seq_layout_invocation();
            for request in [
                seq_layout_request(ImsOperation::Schedule, 1, &[], None),
                seq_layout_load(case, 2),
                seq_layout_request(ImsOperation::Commit, 3, &[], None),
            ] {
                let receipt = server.ims_execute_selected(SEQ_LAYOUT_APP, &inv, &request);
                eprintln!("SEQ_LAYOUT_SIGNED_ACCEPTED_{case}_CALL={receipt:?}");
                match receipt {
                    Ok(receipt) => assert_eq!(receipt.status, "  "),
                    Err(problem) => {
                        assert_eq!(problem, HostProblem::Malformed);
                        break;
                    }
                }
            }
            seq_layout_log_rows("ACCEPTED_RETAINED", &*store);
        }
    }
    assert_eq!(
        actual,
        Err(if matches!(case, "shsam-multiple" | "shisam-multiple") {
            // Manager-approved existing LimitExceeded transport; no mapper change.
            HostProblem::ResourceExhausted
        } else {
            HostProblem::Malformed
        }),
        "normative malformed signed STAGE: {case}"
    );
    assert_eq!(after_stage, before);
    assert_eq!(
        seq_layout_rows(&*store),
        before,
        "rejected stage must preserve all provider payload/version rows"
    );
    assert_eq!(
        server
            .ims_service()
            .selected_metadata_generation(SEQ_LAYOUT_APP)
            .unwrap(),
        selected_before
    );
}

#[test]
#[ignore = "requires external old/new retained registry and selected-state artifact"]
fn seq_layout_reader_signed_pure() {
    let path = std::path::PathBuf::from(std::env::var_os("SEQ_LAYOUT_READER_DB").unwrap());
    let bytes = std::fs::read(&path).unwrap();
    let store: Arc<dyn PlatformStore> = Arc::new(
        SqliteStateStore::open(
            &format!("sqlite:{}?mode=rw", path.display()),
            64 * 1024 * 1024,
            262_144,
        )
        .unwrap(),
    );
    let before = seq_layout_rows(&*store);
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
    let expected = std::env::var("SEQ_LAYOUT_READER_EXPECT").unwrap();
    match expected.as_str() {
        "good" | "old-invalid" => {
            result.unwrap();
        }
        "multiple" => assert!(matches!(result, Err(HostProblem::ResourceExhausted))),
        "variable" => assert!(matches!(result, Err(HostProblem::Malformed))),
        other => panic!("Unknown pure registry expectation: {other}"),
    }
    if std::env::var("SEQ_LAYOUT_READER_SELECTED").unwrap() == "yes" {
        // Actual selected-only publication artifacts have no activated database.
        // This opens the real existing owner without filtering/replacing any row.
        let ims = mainframe_env_ims::ImsService::open(store.clone(), Default::default()).unwrap();
        let selected = ims.selected_metadata_generation(SEQ_LAYOUT_APP);
        if expected == "good" || expected == "old-invalid" {
            let selected = selected.unwrap().unwrap();
            assert_eq!(selected.generation, 1);
            assert_eq!(
                selected.catalog,
                seq_layout_catalog(&std::env::var("SEQ_LAYOUT_READER_CASE").unwrap())
            );
        } else {
            assert_eq!(selected, Err(HostProblem::InfrastructureFailure));
        }
        eprintln!("SEQ_LAYOUT_SELECTED_PURE_READER_PASS");
    }
    assert_eq!(seq_layout_rows(&*store), before);
    drop(store);
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    eprintln!("SEQ_LAYOUT_SIGNED_PURE_READER_PASS");
}

#[test]
#[ignore = "requires external actual unselected invalid registry artifact"]
fn seq_layout_reader_unselected_server_availability() {
    let path = std::path::PathBuf::from(std::env::var_os("SEQ_LAYOUT_READER_DB").unwrap());
    let bytes = std::fs::read(&path).unwrap();
    let url = format!("sqlite:{}?mode=rw", path.display());
    let store: Arc<dyn PlatformStore> =
        Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap());
    let before = seq_layout_rows(&*store);
    let mut cfg = config();
    cfg.store_profile = crate::StoreProfile::Sqlite;
    cfg.sqlite_url = url;
    let result = ProductServer::open_with_package_trust(
        cfg,
        store.clone(),
        Arc::new(MemorySecretResolver::default()),
        default_program_router(),
        Arc::new(test_package_trust()),
    );
    let error = if std::env::var("SEQ_LAYOUT_READER_EXPECT").unwrap() == "multiple" {
        HostProblem::ResourceExhausted
    } else {
        HostProblem::Malformed
    };
    assert!(matches!(result, Err(problem) if problem == error));
    assert_eq!(seq_layout_rows(&*store), before);
    drop(store);
    // Availability is separate from the pure readers: default startup can
    // observe its shared duration clock, but no provider row may change.
    eprintln!(
        "SEQ_LAYOUT_AVAILABILITY_PHYSICAL_BYTES_EQUAL={}",
        std::fs::read(&path).unwrap() == bytes
    );
    eprintln!("SEQ_LAYOUT_UNSELECTED_REGISTRY_AVAILABILITY_PROVIDER_ROWS_UNCHANGED_PASS");
}

#[test]
#[ignore = "capture genuine old publication and unselected registry state before predicates"]
fn seq_layout_capture_signed_admission_state() {
    for case in [
        "shisam-multiple",
        "shsam-multiple",
        "hsam-root-variable",
        "hsam-dependent-variable",
        "shsam-variable",
    ] {
        let (store, cfg, path) = seq_layout_sqlite(&format!("selected-only-{case}"));
        let server = seq_layout_open(store.clone(), cfg);
        let staged = server
            .install_application_package_v2(&seq_layout_package(case))
            .unwrap();
        server.publish_application_generation(&staged).unwrap();
        assert!(
            server
                .ims_service()
                .selected_metadata_generation(SEQ_LAYOUT_APP)
                .unwrap()
                .is_some()
        );
        eprintln!("SEQ_LAYOUT_OLD_SELECTED_ONLY_ARTIFACT={}", path.display());
        let (store, cfg, path) = seq_layout_sqlite(&format!("unselected-{case}"));
        let server = seq_layout_open(store, cfg);
        let staged = server
            .install_application_package_v2(&seq_layout_package(case))
            .unwrap();
        assert_eq!(staged.generation, 1);
        assert!(
            server
                .ims_service()
                .selected_metadata_generation(SEQ_LAYOUT_APP)
                .unwrap()
                .is_none()
        );
        eprintln!("SEQ_LAYOUT_OLD_UNSELECTED_ARTIFACT={}", path.display());
    }
    eprintln!("SEQ_LAYOUT_OLD_REAL_PUBLICATION_AND_UNSELECTED_CAPTURE_PASS");
}
fn seq_layout_assert_replay_rows(before: &[ProviderStateRecord], after: &[ProviderStateRecord]) {
    let is_audit = |row: &&ProviderStateRecord| {
        matches!(
            row.namespace.as_str(),
            "durable-audit-v1" | "racf-database-v2"
        )
    };
    assert_eq!(
        before
            .iter()
            .filter(|row| !is_audit(row))
            .collect::<Vec<_>>(),
        after
            .iter()
            .filter(|row| !is_audit(row))
            .collect::<Vec<_>>(),
        "replay preserves all non-audit provider payload/version rows"
    );
    eprintln!(
        "SEQ_LAYOUT_REPLAY_AUDIT_BEFORE_AFTER={:?}/{:?}",
        before
            .iter()
            .filter(is_audit)
            .map(|r| (&r.namespace, &r.key, r.version))
            .collect::<Vec<_>>(),
        after
            .iter()
            .filter(is_audit)
            .map(|r| (&r.namespace, &r.key, r.version))
            .collect::<Vec<_>>()
    );
}

fn seq_layout_programming(store: Arc<dyn PlatformStore>, cfg: ServerConfig, case: &str) {
    let server = seq_layout_open(store.clone(), cfg.clone());
    let package = seq_layout_package(case);
    let staged = server.install_application_package_v2(&package).unwrap();
    let publication = server.publish_application_generation(&staged).unwrap();
    assert!(publication.ims_metadata && !publication.replayed);
    let before = seq_layout_rows(&*store);
    let selected = server
        .ims_service()
        .selected_metadata_generation(SEQ_LAYOUT_APP)
        .unwrap();
    assert!(
        server
            .publish_application_generation(&staged)
            .unwrap()
            .replayed
    );
    assert_eq!(seq_layout_rows(&*store), before);
    assert_eq!(
        server
            .ims_service()
            .selected_metadata_generation(SEQ_LAYOUT_APP)
            .unwrap(),
        selected
    );
    seq_layout_authorize(&server);
    let inv = seq_layout_invocation();
    let execute = |r: &ImsRequest| {
        server
            .ims_execute_selected(SEQ_LAYOUT_APP, &inv, r)
            .unwrap()
    };
    assert_eq!(
        execute(&seq_layout_request(ImsOperation::Schedule, 1, &[], None)).status,
        "  "
    );
    let sequential = case.starts_with("hsam") || case.starts_with("shsam");
    let first = if sequential {
        seq_layout_load(case, 2)
    } else {
        seq_layout_request(ImsOperation::Insert, 2, b"A1X", None)
    };
    let receipt = execute(&first);
    assert_eq!(receipt.status, "  ");
    assert_eq!(
        receipt.affected_segments,
        if case == "hsam-fixed" {
            3
        } else if sequential {
            2
        } else {
            1
        }
    );
    let ranged = case == "hisam-ranged" || case == "hidam-ranged";
    if !sequential {
        let second = execute(&seq_layout_request(
            ImsOperation::Insert,
            3,
            if ranged { b"B2YZ" } else { b"B2Y" },
            None,
        ));
        assert_eq!(second.status, "  ");
        assert_eq!(second.affected_segments, 1);
    }
    assert_eq!(
        execute(&seq_layout_request(ImsOperation::Commit, 4, &[], None)).status,
        "  "
    );
    for (sequence, operation) in [
        (5, ImsOperation::GetUnique),
        (6, ImsOperation::GetHoldUnique),
    ] {
        let got = execute(&seq_layout_request(operation, sequence, &[], Some(b"A1")));
        assert_eq!(got.status, "  ");
        assert_eq!(got.segments[0].data, b"A1X");
    }
    let before = seq_layout_rows(&*store);
    assert_eq!(execute(&first), receipt);
    seq_layout_assert_replay_rows(&before, &seq_layout_rows(&*store));
    let expected = if case == "hsam-fixed" {
        b"C1Y".as_slice()
    } else if ranged {
        b"B2YZ".as_slice()
    } else {
        b"B2Y".as_slice()
    };
    assert_eq!(
        execute(&seq_layout_request(ImsOperation::GetNext, 7, &[], None)).segments[0].data,
        expected
    );
    let before = seq_layout_rows(&*store);
    let selected = server
        .ims_service()
        .selected_metadata_generation(SEQ_LAYOUT_APP)
        .unwrap();
    drop(server);
    let reopened_store: Arc<dyn PlatformStore> = if cfg.store_profile == crate::StoreProfile::Sqlite
    {
        Arc::new(SqliteStateStore::open(&cfg.sqlite_url, 64 * 1024 * 1024, 262_144).unwrap())
    } else if cfg.store_profile == crate::StoreProfile::Postgres {
        Arc::new(
            mainframe_env_store::PostgresStateStore::open(
                &std::env::var("SEQ_LAYOUT_POSTGRES_URL").unwrap(),
                64 * 1024 * 1024,
                262_144,
            )
            .unwrap(),
        )
    } else {
        store.clone()
    };
    let reopened = seq_layout_open(reopened_store.clone(), cfg);
    assert_eq!(seq_layout_rows(&*reopened_store), before);
    assert_eq!(
        reopened
            .ims_service()
            .selected_metadata_generation(SEQ_LAYOUT_APP)
            .unwrap(),
        selected
    );
    assert_eq!(
        reopened
            .ims_execute_selected(SEQ_LAYOUT_APP, &inv, &first)
            .unwrap(),
        receipt
    );
    seq_layout_assert_replay_rows(&before, &seq_layout_rows(&*reopened_store));
    assert_eq!(
        reopened
            .ims_execute_selected(
                SEQ_LAYOUT_APP,
                &inv,
                &seq_layout_request(ImsOperation::GetUnique, 8, &[], Some(b"A1"))
            )
            .unwrap()
            .segments[0]
            .data,
        b"A1X"
    );
    assert_eq!(
        reopened
            .ims_execute_selected(
                SEQ_LAYOUT_APP,
                &inv,
                &seq_layout_request(ImsOperation::GetNext, 9, &[], None)
            )
            .unwrap()
            .segments[0]
            .data,
        expected
    );
    eprintln!("SEQ_LAYOUT_SIGNED_CONTROL_{case}_PASS");
}

#[test]
#[ignore = "requires isolated configured PostgreSQL task database and explicit case"]
fn seq_layout_postgres_signed_case() {
    let case = std::env::var("SEQ_LAYOUT_POSTGRES_CASE").unwrap();
    assert!(
        [
            "shisam-multiple",
            "shsam-multiple",
            "hsam-root-variable",
            "hsam-dependent-variable",
            "shsam-variable",
            "shisam-variable",
            "hsam-fixed",
            "shsam-fixed",
            "shisam-fixed",
            "hisam-ranged",
            "hidam-ranged"
        ]
        .contains(&case.as_str())
    );
    let url = std::env::var("SEQ_LAYOUT_POSTGRES_URL").unwrap();
    let store = Arc::new(
        mainframe_env_store::PostgresStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap(),
    );
    let mut cfg = config();
    cfg.store_profile = crate::StoreProfile::Postgres;
    cfg.artifact_profile = ArtifactProfile::Shared;
    cfg.postgres_url_reference = Some("env-base64:MAINFRAME_ENV_SECRET_SEQ_LAYOUT_POSTGRES".into());
    if case.ends_with("fixed") || case.ends_with("ranged") {
        seq_layout_programming(store, cfg, &case);
    } else {
        seq_layout_reject(store, cfg, &case);
    }
    eprintln!("SEQ_LAYOUT_POSTGRES_SIGNED_{case}_PASS");
}

#[test]
fn seq_layout_signed_shisam_multiple_memory() {
    seq_layout_reject(
        Arc::new(MemoryStore::new(Default::default())),
        config(),
        "shisam-multiple",
    );
}

#[test]
fn seq_layout_signed_shisam_multiple_sqlite() {
    let (store, cfg, path) = seq_layout_sqlite("shisam-multiple");
    eprintln!("SEQ_LAYOUT_SIGNED_ARTIFACT={}", path.display());
    seq_layout_reject(store, cfg, "shisam-multiple");
}

#[test]
fn seq_layout_signed_shsam_multiple_memory() {
    seq_layout_reject(
        Arc::new(MemoryStore::new(Default::default())),
        config(),
        "shsam-multiple",
    );
}

#[test]
fn seq_layout_signed_shsam_multiple_sqlite() {
    let (store, cfg, path) = seq_layout_sqlite("shsam-multiple");
    eprintln!("SEQ_LAYOUT_SIGNED_ARTIFACT={}", path.display());
    seq_layout_reject(store, cfg, "shsam-multiple");
}

#[test]
fn seq_layout_signed_hsam_root_variable_memory() {
    seq_layout_reject(
        Arc::new(MemoryStore::new(Default::default())),
        config(),
        "hsam-root-variable",
    );
}

#[test]
fn seq_layout_signed_hsam_root_variable_sqlite() {
    let (store, cfg, path) = seq_layout_sqlite("hsam-root-variable");
    eprintln!("SEQ_LAYOUT_SIGNED_ARTIFACT={}", path.display());
    seq_layout_reject(store, cfg, "hsam-root-variable");
}

#[test]
fn seq_layout_signed_hsam_dependent_variable_memory() {
    seq_layout_reject(
        Arc::new(MemoryStore::new(Default::default())),
        config(),
        "hsam-dependent-variable",
    );
}

#[test]
fn seq_layout_signed_hsam_dependent_variable_sqlite() {
    let (store, cfg, path) = seq_layout_sqlite("hsam-dependent-variable");
    eprintln!("SEQ_LAYOUT_SIGNED_ARTIFACT={}", path.display());
    seq_layout_reject(store, cfg, "hsam-dependent-variable");
}

#[test]
fn seq_layout_signed_shsam_variable_memory() {
    seq_layout_reject(
        Arc::new(MemoryStore::new(Default::default())),
        config(),
        "shsam-variable",
    );
}

#[test]
fn seq_layout_signed_shsam_variable_sqlite() {
    let (store, cfg, path) = seq_layout_sqlite("shsam-variable");
    eprintln!("SEQ_LAYOUT_SIGNED_ARTIFACT={}", path.display());
    seq_layout_reject(store, cfg, "shsam-variable");
}

#[test]
fn seq_layout_signed_shisam_variable_control_memory() {
    seq_layout_reject(
        Arc::new(MemoryStore::new(Default::default())),
        config(),
        "shisam-variable",
    );
}

#[test]
fn seq_layout_signed_shisam_variable_control_sqlite() {
    let (store, cfg, path) = seq_layout_sqlite("shisam-variable");
    eprintln!("SEQ_LAYOUT_SIGNED_ARTIFACT={}", path.display());
    seq_layout_reject(store, cfg, "shisam-variable");
}

#[test]
fn seq_layout_signed_hsam_fixed_memory() {
    seq_layout_programming(
        Arc::new(MemoryStore::new(Default::default())),
        config(),
        "hsam-fixed",
    );
}

#[test]
fn seq_layout_signed_hsam_fixed_sqlite() {
    let (store, cfg, path) = seq_layout_sqlite("hsam-fixed");
    eprintln!("SEQ_LAYOUT_SIGNED_CONTROL_ARTIFACT={}", path.display());
    seq_layout_programming(store, cfg, "hsam-fixed");
}

#[test]
fn seq_layout_signed_shsam_fixed_memory() {
    seq_layout_programming(
        Arc::new(MemoryStore::new(Default::default())),
        config(),
        "shsam-fixed",
    );
}

#[test]
fn seq_layout_signed_shsam_fixed_sqlite() {
    let (store, cfg, path) = seq_layout_sqlite("shsam-fixed");
    eprintln!("SEQ_LAYOUT_SIGNED_CONTROL_ARTIFACT={}", path.display());
    seq_layout_programming(store, cfg, "shsam-fixed");
}

#[test]
fn seq_layout_signed_shisam_fixed_memory() {
    seq_layout_programming(
        Arc::new(MemoryStore::new(Default::default())),
        config(),
        "shisam-fixed",
    );
}

#[test]
fn seq_layout_signed_shisam_fixed_sqlite() {
    let (store, cfg, path) = seq_layout_sqlite("shisam-fixed");
    eprintln!("SEQ_LAYOUT_SIGNED_CONTROL_ARTIFACT={}", path.display());
    seq_layout_programming(store, cfg, "shisam-fixed");
}

#[test]
fn seq_layout_signed_hisam_ranged_memory() {
    seq_layout_programming(
        Arc::new(MemoryStore::new(Default::default())),
        config(),
        "hisam-ranged",
    );
}

#[test]
fn seq_layout_signed_hisam_ranged_sqlite() {
    let (store, cfg, path) = seq_layout_sqlite("hisam-ranged");
    eprintln!("SEQ_LAYOUT_SIGNED_CONTROL_ARTIFACT={}", path.display());
    seq_layout_programming(store, cfg, "hisam-ranged");
}

#[test]
fn seq_layout_signed_hidam_ranged_memory() {
    seq_layout_programming(
        Arc::new(MemoryStore::new(Default::default())),
        config(),
        "hidam-ranged",
    );
}

#[test]
fn seq_layout_signed_hidam_ranged_sqlite() {
    let (store, cfg, path) = seq_layout_sqlite("hidam-ranged");
    eprintln!("SEQ_LAYOUT_SIGNED_CONTROL_ARTIFACT={}", path.display());
    seq_layout_programming(store, cfg, "hidam-ranged");
}
