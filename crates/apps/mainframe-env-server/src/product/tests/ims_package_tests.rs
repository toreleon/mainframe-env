use super::*;
use mainframe_env_host_api::{
    IMS_METADATA_SCHEMA_V1, ImsDatabaseMetadata, ImsDatabaseOrganization, ImsDatabasePcbMetadata,
    ImsDbLevel, ImsFieldMetadata, ImsMetadataCatalog, ImsPcbMetadata, ImsPsbMetadata,
    ImsSegmentMetadata, ImsSensitiveSegmentMetadata, ImsTerminalPcbMetadata,
};
use mainframe_env_ims::{
    TmAlternatePcbDefinition, TmConversationAction, TmDefinitionSet, TmDestination,
    TmExecutionContext, TmPcb, TmPcbStatus, TmTransactionDefinition,
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

fn signed_tm_package(
    trust: &HmacSha256PackageTrust,
    generation: u64,
    transaction: &str,
) -> ApplicationPackageV2 {
    let mut package = signed_ims_package(trust, generation, 1);
    package.sections.ims_metadata.as_mut().unwrap().psbs[0]
        .pcbs
        .push(ImsPcbMetadata::AlternateTerminal(ImsTerminalPcbMetadata {
            name: "REPLY".into(),
            destination: None,
            modifiable: true,
            express: false,
            same_terminal: false,
            response_mode: false,
        }));
    let program = package
        .base
        .manifest
        .entries
        .iter()
        .find(|entry| entry.kind == EntryKind::Program)
        .unwrap();
    package.sections.ims_tm = Some(TmDefinitionSet {
        transactions: vec![TmTransactionDefinition {
            code: transaction.into(),
            psb: "AUTHPSB".into(),
            program_selector: program.path.clone(),
            artifact: program.sha256.clone(),
            required_generation: format!("tm-package-{generation}"),
            context: TmExecutionContext::MessageProcessing,
            priority: 7,
            timeout_ticks: 1000,
            conversational: true,
            spa_size: 64,
            alternate_pcbs: vec![TmAlternatePcbDefinition {
                name: "REPLY".into(),
                destination: TmDestination::Modifiable,
                express: false,
            }],
        }],
    });
    resign_package(&mut package, trust);
    package
}

fn tm_invocation(run: &str, key: &str) -> Invocation {
    let limits = InvocationLimits::default();
    Invocation::new(
        RequestId::new(format!("request-{key}"), limits).unwrap(),
        ExecutionId::new(format!("execution-{key}"), limits).unwrap(),
        RunUnitId::new(run, limits).unwrap(),
        None,
        Selector::new("ims:application", limits).unwrap(),
        ArtifactRef::new("artifact:application", limits).unwrap(),
        Principal::new(
            PrincipalId::new("IBMUSER", limits).unwrap(),
            BTreeSet::new(),
            limits,
        )
        .unwrap(),
        ServiceClass::Interactive,
        7,
        u64::MAX,
        TraceId::new(format!("trace-{key}"), limits).unwrap(),
        IdempotencyKey::new(key, limits).unwrap(),
        1,
        ResourceLimits::default(),
        BTreeMap::new(),
        limits,
    )
    .unwrap()
}

fn tm_message(id: &str, transaction: &str, conversation_id: Option<String>) -> TmInputMessage {
    TmInputMessage {
        message_id: id.into(),
        transaction: transaction.into(),
        source: "TERM1".into(),
        user_id: Some("IBMUSER".into()),
        group_name: None,
        conversation_id,
        segments: vec![b"first".to_vec(), b"second".to_vec()],
    }
}

fn permit_tm(server: &ProductServer, transactions: &[&str]) {
    server
        .bootstrap_administrator("IBMUSER", b"TESTPASS")
        .unwrap();
    server
        .racf
        .define_profile("IMSPSB", "AUTHPSB", "IBMUSER", Some(AccessIntent::Control))
        .unwrap();
    for name in transactions
        .iter()
        .copied()
        .chain(["DEST.TERM1", "DEST.TERM2"])
    {
        server
            .racf
            .define_profile("IMSUOW", name, "IBMUSER", Some(AccessIntent::Control))
            .unwrap();
    }
}

#[test]
fn signed_carddemo_shaped_tm_dispatch_is_authorized_bound_and_rollback_safe() {
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
    let mut invalid = signed_tm_package(&trust, 1, "CARD1");
    invalid.sections.ims_tm.as_mut().unwrap().transactions[0].program_selector =
        "program/MISSING".into();
    resign_package(&mut invalid, &trust);
    assert_eq!(
        server.install_application_package_v2(&invalid),
        Err(HostProblem::Malformed)
    );
    let mut invalid_pcb = signed_tm_package(&trust, 1, "CARD1");
    invalid_pcb.sections.ims_tm.as_mut().unwrap().transactions[0].alternate_pcbs[0].name =
        "MISSING".into();
    resign_package(&mut invalid_pcb, &trust);
    assert_eq!(
        server.install_application_package_v2(&invalid_pcb),
        Err(HostProblem::Malformed)
    );
    let first = server
        .install_application_package_v2(&signed_tm_package(&trust, 1, "CARD1"))
        .unwrap();
    server.publish_application_generation(&first).unwrap();
    assert_eq!(
        server.ims_tm_enqueue(
            "SIGNED-IMS-APPLICATION",
            &tm_invocation("admit", "denied"),
            tm_message("card-denied", "CARD1", None)
        ),
        Err(HostProblem::Unauthorized),
    );
    assert_eq!(server.ims_tm.message_state("card-denied").unwrap(), None);
    permit_tm(&server, &["CARD1", "CARD2"]);
    let admitted = server
        .ims_tm_enqueue(
            "SIGNED-IMS-APPLICATION",
            &tm_invocation("admit", "enqueue-1"),
            tm_message("card-1", "CARD1", None),
        )
        .unwrap();
    assert!(
        server
            .ims_tm_enqueue(
                "SIGNED-IMS-APPLICATION",
                &tm_invocation("admit", "enqueue-1"),
                tm_message("card-1", "CARD1", None)
            )
            .unwrap()
            .replayed
    );
    let work = server
        .ims_tm_claim("SIGNED-IMS-APPLICATION", "CARD1", "worker", 1, 100)
        .unwrap()
        .unwrap();
    assert_eq!(work.work_id, admitted.work_id);
    server
        .ims_tm_start(
            "SIGNED-IMS-APPLICATION",
            &tm_invocation("card-run", "start-1"),
            &work,
        )
        .unwrap();
    assert!(
        server
            .ims_tm_start(
                "SIGNED-IMS-APPLICATION",
                &tm_invocation("card-run", "start-1"),
                &work
            )
            .unwrap()
            .replayed
    );
    let gu = server
        .ims_tm_call(
            "SIGNED-IMS-APPLICATION",
            &tm_invocation("card-run", "gu-1"),
            TmCall::GetUnique,
        )
        .unwrap();
    assert_eq!(gu.segment, Some(b"first".to_vec()));
    let gn = server
        .ims_tm_call(
            "SIGNED-IMS-APPLICATION",
            &tm_invocation("card-run", "gn-1"),
            TmCall::GetNext,
        )
        .unwrap();
    assert_eq!(gn.segment, Some(b"second".to_vec()));
    server
        .ims_tm_call(
            "SIGNED-IMS-APPLICATION",
            &tm_invocation("card-run", "change-1"),
            TmCall::Change {
                pcb: "REPLY".into(),
                destination: "TERM2".into(),
            },
        )
        .unwrap();
    server
        .ims_tm_call(
            "SIGNED-IMS-APPLICATION",
            &tm_invocation("card-run", "insert-1"),
            TmCall::Insert {
                pcb: TmPcb::Alternate("REPLY".into()),
                segment: b"reply".to_vec(),
            },
        )
        .unwrap();
    let purged = server
        .ims_tm_call(
            "SIGNED-IMS-APPLICATION",
            &tm_invocation("card-run", "purg-1"),
            TmCall::Purge {
                pcb: TmPcb::Alternate("REPLY".into()),
            },
        )
        .unwrap();
    assert_eq!(purged.status, TmPcbStatus::SUCCESS);
    server
        .ims_tm_call(
            "SIGNED-IMS-APPLICATION",
            &tm_invocation("card-run", "commit-1"),
            TmCall::Commit {
                conversation: Some(TmConversationAction::Continue {
                    spa: b"next".to_vec(),
                }),
            },
        )
        .unwrap();
    assert!(
        server
            .ims_tm_call(
                "SIGNED-IMS-APPLICATION",
                &tm_invocation("card-run", "commit-1"),
                TmCall::Commit {
                    conversation: Some(TmConversationAction::Continue {
                        spa: b"next".to_vec()
                    })
                }
            )
            .unwrap()
            .replayed
    );
    assert_eq!(
        server.ims_tm.outbound("TERM2", 10).unwrap()[0].segments,
        vec![b"reply".to_vec()]
    );

    let second = server
        .install_application_package_v2(&signed_tm_package(&trust, 2, "CARD2"))
        .unwrap();
    server.publish_application_generation(&second).unwrap();
    assert_eq!(
        server.ims_tm_enqueue(
            "SIGNED-IMS-APPLICATION",
            &tm_invocation("admit", "enqueue-1"),
            tm_message("card-1", "CARD1", None)
        ),
        Err(HostProblem::IdempotencyConflict),
    );
    assert_eq!(
        server.ims_tm_enqueue(
            "SIGNED-IMS-APPLICATION",
            &tm_invocation("admit", "wrong-gen"),
            tm_message("wrong-gen", "CARD1", None)
        ),
        Err(HostProblem::NotFound)
    );
    let old = server
        .ims_tm_enqueue(
            "SIGNED-IMS-APPLICATION",
            &tm_invocation("admit", "enqueue-2"),
            tm_message("card-2", "CARD2", None),
        )
        .unwrap();
    server.rollback_application_generation(&first).unwrap();
    assert!(
        server
            .ims_tm_enqueue(
                "SIGNED-IMS-APPLICATION",
                &tm_invocation("admit", "enqueue-3"),
                tm_message("card-3", "CARD1", None)
            )
            .is_ok()
    );
    let old_work = server
        .ims_tm_claim_retained(
            "SIGNED-IMS-APPLICATION",
            2,
            &second.identity,
            "CARD2",
            "old-worker",
            20,
            100,
        )
        .unwrap()
        .unwrap();
    assert_eq!(old_work.work_id, old.work_id);
    server
        .ims_tm_start(
            "SIGNED-IMS-APPLICATION",
            &tm_invocation("old-run", "old-start"),
            &old_work,
        )
        .unwrap();
    server
        .ims_tm_call(
            "SIGNED-IMS-APPLICATION",
            &tm_invocation("old-run", "old-rollback"),
            TmCall::Rollback,
        )
        .unwrap();
    drop(server);
    let reopened = ProductServer::open_with_package_trust(
        config(),
        store,
        Arc::new(MemorySecretResolver::default()),
        default_program_router(),
        trust,
    )
    .unwrap();
    assert!(
        reopened
            .ims_tm
            .selected_package_matches("SIGNED-IMS-APPLICATION", 1, &first.identity)
            .unwrap()
    );
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
