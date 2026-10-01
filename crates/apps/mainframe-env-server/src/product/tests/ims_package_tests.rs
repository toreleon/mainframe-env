use super::*;
use mainframe_env_host_api::{
    IMS_METADATA_SCHEMA_V1, ImsDatabaseMetadata, ImsDatabaseOrganization, ImsDatabasePcbMetadata,
    ImsDbLevel, ImsFieldMetadata, ImsMetadataCatalog, ImsPcbMetadata, ImsPsbMetadata,
    ImsSegmentMetadata, ImsSensitiveSegmentMetadata,
};

fn metadata(version: u32) -> ImsMetadataCatalog {
    let field = |name: &str, length| ImsFieldMetadata {
        name: Some(name.into()),
        offset: 0,
        length,
        sequence: true,
        unique: true,
    };
    ImsMetadataCatalog {
        schema_version: IMS_METADATA_SCHEMA_V1.into(),
        databases: vec![ImsDatabaseMetadata {
            name: "AUTHDB".into(),
            version,
            organization: ImsDatabaseOrganization::Hidam,
            segments: vec![
                ImsSegmentMetadata {
                    name: "ROOT".into(),
                    parent: None,
                    min_length: 16,
                    max_length: 16,
                    fields: vec![field("ROOTKEY", 8)],
                },
                ImsSegmentMetadata {
                    name: "CHILD".into(),
                    parent: Some("ROOT".into()),
                    min_length: 16,
                    max_length: 16,
                    fields: vec![field("CHILDKEY", 8)],
                },
            ],
            secondary_indexes: Vec::new(),
            logical_relationships: Vec::new(),
        }],
        psbs: vec![ImsPsbMetadata {
            name: "AUTHPSB".into(),
            database_level: ImsDbLevel::Current,
            pcbs: vec![ImsPcbMetadata::Database(ImsDatabasePcbMetadata {
                name: "AUTHPCB".into(),
                database: "AUTHDB".into(),
                database_version: Some(version),
                processing_options: "AP".into(),
                sensitive_segments: vec![
                    ImsSensitiveSegmentMetadata {
                        name: "ROOT".into(),
                        parent: None,
                        processing_options: None,
                    },
                    ImsSensitiveSegmentMetadata {
                        name: "CHILD".into(),
                        parent: Some("ROOT".into()),
                        processing_options: Some("G".into()),
                    },
                ],
            })],
        }],
    }
}

fn signed_ims_package(
    trust: &HmacSha256PackageTrust,
    generation: u64,
    version: u32,
) -> ApplicationPackageV2 {
    let mut package = signed_controller_package(trust);
    package.base.manifest.name = "SIGNED-IMS-APPLICATION".into();
    package.generation = generation;
    package.sections.ims_metadata = Some(metadata(version));
    resign_package(&mut package, trust);
    package
}

#[test]
fn signed_ims_metadata_is_validated_before_provider_mutation() {
    let trust = Arc::new(test_package_trust());
    let server = ProductServer::memory_with_package_trust(config(), trust.clone()).unwrap();
    let mut package = signed_ims_package(&trust, 1, 1);
    package.sections.ims_metadata.as_mut().unwrap().databases[0].segments[1].parent =
        Some("MISSING".into());
    resign_package(&mut package, &trust);

    assert_eq!(
        server.install_application_package_v2(&package),
        Err(HostProblem::Malformed)
    );
    assert_eq!(
        server
            .ims_service()
            .selected_metadata_generation("SIGNED-IMS-APPLICATION")
            .unwrap(),
        None
    );
}

#[test]
fn selected_ims_metadata_publication_is_idempotent() {
    let trust = Arc::new(test_package_trust());
    let server = ProductServer::memory_with_package_trust(config(), trust.clone()).unwrap();
    let package = signed_ims_package(&trust, 1, 1);
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
    assert_eq!(
        server.ims_service().publish_metadata_generation(
            "SIGNED-IMS-APPLICATION",
            1,
            &staged.identity,
            Some(&metadata(2)),
        ),
        Err(HostProblem::IdempotencyConflict)
    );
    let selected = server
        .ims_service()
        .selected_metadata_generation("SIGNED-IMS-APPLICATION")
        .unwrap()
        .unwrap();
    assert_eq!(selected.generation, 1);
    assert_eq!(selected.package_identity, staged.identity);
    assert_eq!(selected.catalog, metadata(1));
}

#[test]
fn applying_ims_metadata_recovers_after_provider_commit() {
    let trust = Arc::new(test_package_trust());
    let store = Arc::new(MemoryStore::new(Default::default()));
    let server = ProductServer::open_with_package_trust(
        config(),
        store.clone(),
        Arc::new(MemorySecretResolver::default()),
        default_program_router(),
        trust.clone(),
    )
    .unwrap();
    let package = signed_ims_package(&trust, 1, 1);
    let staged = server.install_application_package_v2(&package).unwrap();
    let retained = server.application_generation_v2(&staged).unwrap();
    server
        .apply_application_batch_controllers(&retained)
        .unwrap();
    server.apply_application_ims_metadata(&retained).unwrap();
    store
        .put_provider_state(
            ProviderStateRecord {
                namespace: APPLICATION_PUBLICATION_NAMESPACE.into(),
                key: staged.package.to_ascii_uppercase(),
                version: 1,
                payload: serde_json::to_vec(&ApplicationPublicationState {
                    schema_version: APPLICATION_PUBLICATION_CONTRACT.into(),
                    package: staged.package.clone(),
                    generation: staged.generation,
                    identity: staged.identity.clone(),
                    action: PublicationAction::Install,
                    controllers: PublicationSectionState::Applied,
                    db2: PublicationSectionState::NotApplicable,
                    ims: PublicationSectionState::Applying,
                    complete: false,
                })
                .unwrap(),
            },
            None,
        )
        .unwrap();
    drop(server);

    let restarted = ProductServer::open_with_package_trust(
        config(),
        store,
        Arc::new(MemorySecretResolver::default()),
        default_program_router(),
        trust,
    )
    .unwrap();
    assert!(
        restarted
            .publish_application_generation(&staged)
            .unwrap()
            .replayed
    );
    assert_eq!(
        restarted
            .ims_service()
            .selected_metadata_generation("SIGNED-IMS-APPLICATION")
            .unwrap()
            .unwrap()
            .generation,
        1
    );
}

#[test]
fn ims_metadata_rollback_and_selection_survive_restart() {
    let trust = Arc::new(test_package_trust());
    let store = Arc::new(MemoryStore::new(Default::default()));
    let server = ProductServer::open_with_package_trust(
        config(),
        store.clone(),
        Arc::new(MemorySecretResolver::default()),
        default_program_router(),
        trust.clone(),
    )
    .unwrap();
    let first_package = signed_ims_package(&trust, 1, 1);
    let first = server
        .install_application_package_v2(&first_package)
        .unwrap();
    server.publish_application_generation(&first).unwrap();

    let second_package = signed_ims_package(&trust, 2, 2);
    let second = server
        .install_application_package_v2(&second_package)
        .unwrap();
    server.publish_application_generation(&second).unwrap();
    assert_eq!(
        server
            .ims_service()
            .selected_metadata_generation("SIGNED-IMS-APPLICATION")
            .unwrap()
            .unwrap()
            .generation,
        2
    );
    server.rollback_application_generation(&first).unwrap();
    drop(server);

    let restarted = ProductServer::open_with_package_trust(
        config(),
        store,
        Arc::new(MemorySecretResolver::default()),
        default_program_router(),
        trust,
    )
    .unwrap();
    let selected = restarted
        .ims_service()
        .selected_metadata_generation("SIGNED-IMS-APPLICATION")
        .unwrap()
        .unwrap();
    assert_eq!(selected.generation, 1);
    assert_eq!(selected.catalog, metadata(1));
    assert_eq!(
        restarted
            .selected_application_v2(&first)
            .unwrap()
            .record()
            .generation,
        1
    );
}

#[test]
fn sqlite_reopen_clears_absent_metadata_and_restores_retained_generation() {
    let directory = std::env::temp_dir().join(format!(
        "mainframe-env-ims-metadata-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&directory).unwrap();
    let path = directory.join("package.db");
    let url = format!("sqlite://{}?mode=rwc", path.display());
    let mut server_config = config();
    server_config.store_profile = crate::StoreProfile::Sqlite;
    server_config.sqlite_url = url.clone();
    let trust = Arc::new(test_package_trust());
    let first;
    let second;
    {
        let store: Arc<dyn PlatformStore> =
            Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap());
        let server = ProductServer::open_with_package_trust(
            server_config.clone(),
            store,
            Arc::new(MemorySecretResolver::default()),
            default_program_router(),
            trust.clone(),
        )
        .unwrap();
        first = server
            .install_application_package_v2(&signed_ims_package(&trust, 1, 1))
            .unwrap();
        server.publish_application_generation(&first).unwrap();
        let mut without_metadata = signed_controller_package(&trust);
        without_metadata.base.manifest.name = "SIGNED-IMS-APPLICATION".into();
        without_metadata.generation = 2;
        resign_package(&mut without_metadata, &trust);
        second = server
            .install_application_package_v2(&without_metadata)
            .unwrap();
        assert!(
            !server
                .publish_application_generation(&second)
                .unwrap()
                .ims_metadata
        );
        assert_eq!(
            server
                .ims_service()
                .selected_metadata_generation("SIGNED-IMS-APPLICATION")
                .unwrap(),
            None
        );
    }
    {
        let store: Arc<dyn PlatformStore> =
            Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap());
        let server = ProductServer::open_with_package_trust(
            server_config,
            store,
            Arc::new(MemorySecretResolver::default()),
            default_program_router(),
            trust,
        )
        .unwrap();
        assert_eq!(
            server
                .ims_service()
                .selected_metadata_generation("SIGNED-IMS-APPLICATION")
                .unwrap(),
            None
        );
        assert_eq!(
            server
                .selected_application_v2(&second)
                .unwrap()
                .record()
                .generation,
            2
        );
        server.rollback_application_generation(&first).unwrap();
        let selected = server
            .ims_service()
            .selected_metadata_generation("SIGNED-IMS-APPLICATION")
            .unwrap()
            .unwrap();
        assert_eq!(selected.generation, 1);
        assert_eq!(selected.package_identity, first.identity);
        assert_eq!(selected.catalog, metadata(1));
    }
    std::fs::remove_file(path).unwrap();
    std::fs::remove_dir(directory).unwrap();
}
