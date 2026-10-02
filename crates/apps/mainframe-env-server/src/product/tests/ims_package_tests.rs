use super::*;
#[path = "ims_application_recovery_tests.rs"]
mod application_recovery;
#[path = "ims_package_tests/feedback_tests.rs"]
mod feedback_tests;
#[path = "ims_package_tests/gsam_checkpoint_tests.rs"]
mod gsam_checkpoint_tests;
#[path = "ims_package_tests/gsam_tests.rs"]
mod gsam_tests;
#[path = "ims_package_tests/logical_feedback_tests.rs"]
mod logical_feedback_tests;
#[path = "ims_package_tests/null_ssa_tests.rs"]
mod null_ssa_tests;
#[path = "ims_package_tests/secondary_checkpoint_tests.rs"]
mod secondary_checkpoint_tests;

#[path = "ims_secondary_ssa_tests.rs"]
mod secondary_ssa;
use mainframe_env_host_api::{
    IMS_METADATA_SCHEMA_V1, ImsDatabaseMetadata, ImsDatabaseOrganization, ImsDatabasePcbMetadata,
    ImsDbLevel, ImsFieldMetadata, ImsMetadataCatalog, ImsOperation, ImsPcbMetadata, ImsPsbMetadata,
    ImsQualifier, ImsRequest, ImsSegmentMetadata, ImsSensitiveSegmentMetadata,
    ImsTerminalPcbMetadata, Mutation,
};
use mainframe_env_ims::{
    TmAlternatePcbDefinition, TmConversationAction, TmDefinitionSet, TmDestination,
    TmExecutionContext, TmPcb, TmPcbStatus, TmTransactionDefinition,
};

fn carddemo_metadata() -> ImsMetadataCatalog {
    let mut catalog = metadata(1);
    catalog.databases[0].name = "DBPAUTP0".into();
    catalog.databases[0].segments[0].name = "PAUTSUM0".into();
    catalog.databases[0].segments[0].min_length = 100;
    catalog.databases[0].segments[0].max_length = 100;
    catalog.databases[0].segments[0].fields[0].name = Some("ACCNTID".into());
    catalog.databases[0].segments[0].fields[0].length = 6;
    catalog.databases[0].segments[1].name = "PAUTDTL1".into();
    catalog.databases[0].segments[1].parent = Some("PAUTSUM0".into());
    catalog.databases[0].segments[1].min_length = 200;
    catalog.databases[0].segments[1].max_length = 200;
    catalog.databases[0].segments[1].fields[0].name = Some("PAUT9CTS".into());
    catalog.psbs[0].name = "PSBPAUTB".into();
    let ImsPcbMetadata::Database(pcb) = &mut catalog.psbs[0].pcbs[0] else {
        unreachable!()
    };
    pcb.name = "PAUTPCB".into();
    pcb.database = "DBPAUTP0".into();
    pcb.sensitive_segments[0].name = "PAUTSUM0".into();
    pcb.sensitive_segments[1].name = "PAUTDTL1".into();
    pcb.sensitive_segments[1].parent = Some("PAUTSUM0".into());
    pcb.sensitive_segments[1].processing_options = None;
    catalog.psbs[0]
        .pcbs
        .push(ImsPcbMetadata::AlternateTerminal(ImsTerminalPcbMetadata {
            name: "REPLY".into(),
            destination: None,
            modifiable: true,
            express: false,
            same_terminal: false,
            response_mode: false,
        }));
    catalog
}

fn signed_carddemo_package(
    trust: &HmacSha256PackageTrust,
    generation: u64,
) -> ApplicationPackageV2 {
    let mut package = signed_tm_package(trust, generation, "PAUT");
    package.base.manifest.name = "CARDDEMO-IMS".into();
    package.sections.ims_metadata = Some(carddemo_metadata());
    package.sections.ims_tm.as_mut().unwrap().transactions[0].psb = "PSBPAUTB".into();
    resign_package(&mut package, trust);
    package
}

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
            gsam_format: None,
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
                secondary_index: None,
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

fn carddemo_request(operation: ImsOperation, sequence: u64, data: Vec<u8>) -> ImsRequest {
    let limits = InvocationLimits::default();
    ImsRequest {
        operation,
        psb: (operation == ImsOperation::Schedule).then(|| "PSBPAUTB".into()),
        pcb: 1,
        segments: if operation == ImsOperation::Insert {
            vec!["PAUTSUM0".into()]
        } else {
            Vec::new()
        },
        data,
        qualifiers: Vec::new(),
        checkpoint_id: None,
        max_segments: 16,
        system: None,
        q_class: None,
        mutation: operation.is_mutating().then(|| Mutation {
            sequence,
            idempotency_key: IdempotencyKey::new(format!("carddemo-db-{sequence}"), limits)
                .unwrap(),
            transaction: Some("CARDDEMO-IMS".into()),
        }),
    }
}

fn exercise_signed_carddemo_database_and_tm(store: Arc<dyn PlatformStore>, config: ServerConfig) {
    let trust = Arc::new(test_package_trust());
    let server = ProductServer::open_with_package_trust(
        config.clone(),
        store.clone(),
        Arc::new(MemorySecretResolver::default()),
        default_program_router(),
        trust.clone(),
    )
    .unwrap();
    let mut bad = signed_carddemo_package(&trust, 1);
    bad.signature.value = "invalid".into();
    assert_eq!(
        server.install_application_package_v2(&bad),
        Err(HostProblem::Malformed)
    );
    let first = server
        .install_application_package_v2(&signed_carddemo_package(&trust, 1))
        .unwrap();
    server.publish_application_generation(&first).unwrap();
    server
        .bootstrap_administrator("IBMUSER", b"TESTPASS")
        .unwrap();
    for (class, name) in [
        ("IMSPSB", "PSBPAUTB"),
        ("IMSDB", "DBPAUTP0"),
        ("IMSUOW", "PAUT"),
        ("IMSUOW", "DEST.TERM1"),
    ] {
        server
            .racf
            .define_profile(class, name, "IBMUSER", Some(AccessIntent::Control))
            .unwrap();
    }
    let run = "carddemo-database-run";
    assert_eq!(
        server
            .ims_execute_selected(
                "CARDDEMO-IMS",
                &tm_invocation(run, "schedule"),
                &carddemo_request(ImsOperation::Schedule, 1, vec![])
            )
            .unwrap()
            .status,
        "  "
    );
    let mut root = vec![b' '; 100];
    root[..6].copy_from_slice(b"000001");
    root[6..14].copy_from_slice(b"ROOT-ONE");
    let inserted = server
        .ims_execute_selected(
            "CARDDEMO-IMS",
            &tm_invocation(run, "insert"),
            &carddemo_request(ImsOperation::Insert, 2, root.clone()),
        )
        .unwrap();
    assert_eq!(
        (inserted.status.as_str(), inserted.affected_segments),
        ("  ", 1)
    );
    let mut get = carddemo_request(ImsOperation::GetUnique, 3, vec![]);
    get.segments = vec!["PAUTSUM0".into()];
    get.qualifiers = vec![ImsQualifier {
        segment: "PAUTSUM0".into(),
        field: "ACCNTID".into(),
        value: b"000001".to_vec(),
    }];
    let found = server
        .ims_execute_selected("CARDDEMO-IMS", &tm_invocation(run, "get"), &get)
        .unwrap();
    assert_eq!(found.status, "  ");
    assert_eq!(found.segments[0].data, root);
    let committed = server
        .ims_execute_selected(
            "CARDDEMO-IMS",
            &tm_invocation(run, "commit"),
            &carddemo_request(ImsOperation::Commit, 4, vec![]),
        )
        .unwrap();
    assert_eq!(committed.status, "  ");

    let admitted = server
        .ims_tm_enqueue(
            "CARDDEMO-IMS",
            &tm_invocation("carddemo-admit", "admit"),
            tm_message("carddemo-message", "PAUT", None),
        )
        .unwrap();
    let work = server
        .ims_tm_claim("CARDDEMO-IMS", "PAUT", "worker", 1, 100)
        .unwrap()
        .unwrap();
    assert_eq!(work.work_id, admitted.work_id);
    server
        .ims_tm_start(
            "CARDDEMO-IMS",
            &tm_invocation("carddemo-tm", "start"),
            &work,
        )
        .unwrap();
    let message = server
        .ims_tm_call(
            "CARDDEMO-IMS",
            &tm_invocation("carddemo-tm", "gu"),
            TmCall::GetUnique,
        )
        .unwrap();
    assert_eq!(
        (message.status, message.segment),
        (TmPcbStatus::SUCCESS, Some(b"first".to_vec()))
    );

    let second = server
        .install_application_package_v2(&signed_carddemo_package(&trust, 2))
        .unwrap();
    server.publish_application_generation(&second).unwrap();
    server.rollback_application_generation(&first).unwrap();
    assert_eq!(
        server
            .ims_service()
            .selected_metadata_generation("CARDDEMO-IMS")
            .unwrap()
            .unwrap()
            .generation,
        1
    );
    drop(server);
    let reopened = ProductServer::open_with_package_trust(
        config,
        store,
        Arc::new(MemorySecretResolver::default()),
        default_program_router(),
        trust,
    )
    .unwrap();
    assert_eq!(
        reopened
            .selected_application_v2(&first)
            .unwrap()
            .record()
            .generation,
        1
    );
    assert_eq!(
        reopened
            .ims_service()
            .selected_metadata_generation("CARDDEMO-IMS")
            .unwrap()
            .unwrap()
            .generation,
        1
    );
    assert!(
        reopened
            .ims_service()
            .install_metadata(carddemo_metadata())
            .is_ok()
    );
    let mut get_reopened = get;
    get_reopened.mutation = carddemo_request(ImsOperation::GetUnique, 5, vec![]).mutation;
    let found = reopened
        .ims_execute_selected(
            "CARDDEMO-IMS",
            &tm_invocation(run, "get-reopened"),
            &get_reopened,
        )
        .unwrap();
    assert_eq!(
        (found.status.as_str(), found.segments[0].data.as_slice()),
        ("  ", root.as_slice())
    );
}

#[test]
fn signed_carddemo_package_executes_generic_database_and_tm_on_memory() {
    exercise_signed_carddemo_database_and_tm(
        Arc::new(MemoryStore::new(Default::default())),
        config(),
    );
}

#[test]
fn selected_signed_package_ssa_navigation_uses_public_selection_fences() {
    let trust = Arc::new(test_package_trust());
    let server = ProductServer::memory_with_package_trust(config(), trust.clone()).unwrap();
    let run = "selected-ssa";
    let req = mainframe_env_host_api::ImsNavigationRequest {
        request: carddemo_request(ImsOperation::GetUnique, 3, vec![]),
        context: mainframe_env_host_api::ImsExecutionContext::DbBatch,
        ssas: vec![b"PAUTSUM0*O(00010006GE000001)".to_vec()],
    };
    assert_eq!(
        server.ims_navigation_selected("CARDDEMO-IMS", &tm_invocation(run, "ssa"), &req),
        Err(HostProblem::NotFound)
    );
    let mut package = signed_carddemo_package(&trust, 1);
    let catalog = package.sections.ims_metadata.as_mut().unwrap();
    let mut second_pcb = catalog.psbs[0].pcbs[0].clone();
    if let ImsPcbMetadata::Database(pcb) = &mut second_pcb {
        pcb.name = "SSAOTHER".into();
    }
    catalog.psbs[0].pcbs.push(second_pcb);
    resign_package(&mut package, &trust);
    let staged = server.install_application_package_v2(&package).unwrap();
    server.publish_application_generation(&staged).unwrap();
    server
        .bootstrap_administrator("IBMUSER", b"TESTPASS")
        .unwrap();
    for (class, name) in [("IMSPSB", "PSBPAUTB"), ("IMSDB", "DBPAUTP0")] {
        server
            .racf
            .define_profile(class, name, "IBMUSER", Some(AccessIntent::Control))
            .unwrap();
    }
    server
        .ims_execute_selected(
            "CARDDEMO-IMS",
            &tm_invocation(run, "schedule"),
            &carddemo_request(ImsOperation::Schedule, 1, vec![]),
        )
        .unwrap();
    let mut root = vec![0xff; 100];
    root[..6].copy_from_slice(b"000001");
    server
        .ims_execute_selected(
            "CARDDEMO-IMS",
            &tm_invocation(run, "insert"),
            &carddemo_request(ImsOperation::Insert, 2, root.clone()),
        )
        .unwrap();
    let found = server
        .ims_navigation_selected("CARDDEMO-IMS", &tm_invocation(run, "ssa"), &req)
        .unwrap();
    assert_eq!(found.status, "  ");
    assert_eq!(found.segments[0].data, root);
    assert_eq!(
        server
            .ims_navigation_selected("CARDDEMO-IMS", &tm_invocation(run, "ssa"), &req)
            .unwrap(),
        found
    );
    let mut second = req.clone();
    second.request = carddemo_request(ImsOperation::GetHoldUnique, 5, vec![]);
    second.request.pcb = 3;
    let held = server
        .ims_navigation_selected("CARDDEMO-IMS", &tm_invocation(run, "hold-pcb3"), &second)
        .unwrap();
    assert_eq!(held, found);
    let next = mainframe_env_host_api::ImsNavigationRequest {
        request: carddemo_request(ImsOperation::GetNext, 6, vec![]),
        context: second.context,
        ssas: vec![b"PAUTSUM0 ".to_vec()],
    };
    assert_eq!(
        server
            .ims_navigation_selected("CARDDEMO-IMS", &tm_invocation(run, "next-pcb1"), &next)
            .unwrap()
            .status,
        "GE"
    );
    assert_eq!(
        server
            .ims_navigation_selected("CARDDEMO-IMS", &tm_invocation(run, "hold-pcb3"), &second)
            .unwrap(),
        held
    );
    let mut replacement = root;
    replacement[99] = 0x80;
    let mut replace = carddemo_request(ImsOperation::Replace, 7, replacement.clone());
    replace.pcb = 3;
    assert_eq!(
        server
            .ims_execute_selected(
                "CARDDEMO-IMS",
                &tm_invocation(run, "replace-pcb3"),
                &replace
            )
            .unwrap()
            .status,
        "  "
    );
    let before = server
        .store
        .get_provider_state("ims-v1-generic-database", "DBPAUTP0")
        .unwrap();
    let mut forbidden = req;
    forbidden.request.mutation.as_mut().unwrap().sequence = 8;
    forbidden.request.mutation.as_mut().unwrap().idempotency_key =
        IdempotencyKey::new("selected-ssa-forbidden", InvocationLimits::default()).unwrap();
    forbidden.ssas = vec![b"PAUTSUM0*L ".to_vec()];
    assert_eq!(
        server.ims_navigation_selected(
            "CARDDEMO-IMS",
            &tm_invocation(run, "forbidden"),
            &forbidden
        ),
        Err(HostProblem::Unsupported)
    );
    assert_eq!(
        server
            .store
            .get_provider_state("ims-v1-generic-database", "DBPAUTP0")
            .unwrap(),
        before
    );
}

#[test]
fn signed_carddemo_package_executes_generic_database_and_tm_on_sqlite() {
    let directory = std::env::temp_dir().join(format!(
        "carddemo-ims-package-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&directory).unwrap();
    let file = directory.join("state.db");
    let url = format!("sqlite://{}?mode=rwc", file.display());
    let mut server_config = config();
    server_config.store_profile = crate::StoreProfile::Sqlite;
    server_config.sqlite_url = url.clone();
    let store: Arc<dyn PlatformStore> =
        Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap());
    exercise_signed_carddemo_database_and_tm(store, server_config);
    std::fs::remove_file(file).unwrap();
    std::fs::remove_dir(directory).unwrap();
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
