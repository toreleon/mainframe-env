//! Additional PostgreSQL parity; original Memory/SQLite names and predicates remain intact.
//! Requires the same reset, Root-owned PostgreSQL fixture as the registered profile.
//! No native MQ trigger, root-terminal journal, IBM or CardDemo workflow credit.
use super::super::super::super::fault::{FaultStore, Outcome as CompositionOutcome};
use super::*;
use mainframe_env_application::{SecurityResource, SqlColumn, SqlSeedRow, SqlTable};
use mainframe_env_batch::BatchControllerProjection;
use mainframe_env_db2::{
    Db2CatalogProjection, Db2ColumnDefinition, Db2ResultEncoding, Db2TableDefinition,
};
use mainframe_env_host_api::{
    IMS_METADATA_SCHEMA_V1, ImsDatabaseMetadata, ImsDatabaseOrganization, ImsDatabasePcbMetadata,
    ImsDbLevel, ImsFieldMetadata, ImsMetadataCatalog, ImsPcbMetadata, ImsPsbMetadata,
    ImsSegmentMetadata, ImsSensitiveSegmentMetadata,
};
use mainframe_env_ims::ImsCatalogProjection;
use mainframe_env_mq::MqCatalogProjection;
use mainframe_env_racf::{AccessLevel, RacfProfileComposition};
use mainframe_env_store::{PostgresArtifactStore, PostgresStateStore};
use mainframe_env_store_api::{
    IdempotencyStore, JournalStore, ProviderCompositionCommit, ProviderStateWrite,
};

const PAYLOAD_BYTES: usize = 64 * 1024 * 1024;
const ROWS: usize = 262_144;

fn postgres_url() -> String {
    std::env::var("MAINFRAME_ENV_POSTGRES_TEST_URL")
        .expect("Root-owned reset PostgreSQL URL is required; no implicit service or skip")
}

fn postgres_store(url: &str) -> Arc<PostgresStateStore> {
    Arc::new(PostgresStateStore::open(url, PAYLOAD_BYTES, ROWS).unwrap())
}

fn postgres_server(
    store: Arc<dyn PlatformStore>,
    url: &str,
    trust: Arc<HmacSha256PackageTrust>,
) -> Arc<ProductServer> {
    let mut settings = config();
    settings.store_profile = crate::StoreProfile::Postgres;
    settings.artifact_profile = ArtifactProfile::Shared;
    settings.postgres_url_reference = Some("env-base64:MAINFRAME_ENV_SECRET_POSTGRES_URL".into());
    // Genuine PostgreSQL artifact readiness, rather than the platform trait's default health refusal.
    let artifacts = Arc::new(PostgresArtifactStore::open(url, PAYLOAD_BYTES, ROWS).unwrap());
    assert!(artifacts.is_ready());
    let (program, clock) = launch_fixture_program_and_clock();
    ProductServer::open_configured(
        settings,
        store,
        Arc::new(MemorySecretResolver::default()),
        program,
        trust,
        Arc::new(ProductArtifactStore::Shared(artifacts)),
        Some(clock),
    )
    .unwrap()
}

fn postgres_table() -> Db2TableDefinition {
    Db2TableDefinition {
        name: "LOCAL.CODES".into(),
        columns: vec![
            Db2ColumnDefinition {
                name: "KEY".into(),
                nullable: false,
                max_bytes: 8,
                result_encoding: Db2ResultEncoding::Raw,
                default_value: None,
            },
            Db2ColumnDefinition {
                name: "VALUE".into(),
                nullable: false,
                max_bytes: 64,
                result_encoding: Db2ResultEncoding::Raw,
                default_value: None,
            },
        ],
        primary_key: vec!["KEY".into()],
        foreign_keys: Vec::new(),
        extract: None,
    }
}

fn postgres_metadata() -> ImsMetadataCatalog {
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

fn signed_bytes(
    package: &mut ApplicationPackageV2,
    path: &str,
    kind: EntryKind,
    bytes: Vec<u8>,
    depends_on: Vec<String>,
) {
    let sha = format!("sha256:{:x}", Sha256::digest(&bytes));
    package.base.manifest.entries.push(PackageEntry {
        path: path.into(),
        kind,
        sha256: sha.clone(),
        bytes: bytes.len(),
        depends_on,
    });
    package.base.blobs.insert(sha, bytes);
}

fn postgres_six_definitions(
    server: &ProductServer,
    trust: &HmacSha256PackageTrust,
    generation: u64,
) -> (ApplicationPackageV2, LocalApplicationDefinition) {
    let (mut package, mut definition) = definitions(server, trust, generation);
    let table = postgres_table();
    signed_bytes(
        &mut package,
        "data/db2/catalog",
        EntryKind::Data,
        serde_json::to_vec(&vec![table.clone()]).unwrap(),
        Vec::new(),
    );
    package.sections.sql_tables = vec![SqlTable {
        name: table.name,
        columns: table
            .columns
            .into_iter()
            .map(|column| SqlColumn {
                name: column.name,
                nullable: column.nullable,
            })
            .collect(),
        primary_key: table.primary_key,
    }];
    package.sections.sql_rows = vec![SqlSeedRow {
        table: "LOCAL.CODES".into(),
        values: BTreeMap::from([
            (
                "KEY".into(),
                if generation == 1 { "ONE" } else { "TWO" }.into(),
            ),
            ("VALUE".into(), "SIGNED".into()),
        ]),
    }];
    let metadata = postgres_metadata();
    signed_bytes(
        &mut package,
        "resource/ims-metadata",
        EntryKind::Resource,
        serde_json::to_vec(&metadata).unwrap(),
        Vec::new(),
    );
    package.sections.ims_metadata = Some(metadata);
    // Exactly the original Queue-only port; Process/Trigger is exercised separately by the real owner.
    signed_queue(&mut package, trust);
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
        .map(|item| {
            crate::product::batch_controller::decode_application_batch_controller(&package, item)
                .unwrap()
        })
        .collect::<Vec<_>>();
    signed_bytes(
        &mut package,
        "resource/batch-controllers",
        EntryKind::Resource,
        serde_json::to_vec(&controllers).unwrap(),
        vec!["program/LOCALPGM".into()],
    );
    let profile = RacfProfileComposition {
        class: "FACILITY".into(),
        name: "LOCAL.ACCESS".into(),
        owner: "LOCALOWN".into(),
        uacc: AccessLevel::None,
        permissions: BTreeMap::from([(
            "LOCALOWN".into(),
            if generation == 1 {
                AccessLevel::Read
            } else {
                AccessLevel::Update
            },
        )]),
    };
    package
        .base
        .manifest
        .entries
        .retain(|entry| entry.kind != EntryKind::Profile);
    package.sections.security_resources = vec![SecurityResource {
        class: profile.class.clone(),
        profile: profile.name.clone(),
        owner: profile.owner.clone(),
    }];
    signed_bytes(
        &mut package,
        &profile.entry_path().unwrap(),
        EntryKind::Profile,
        profile.canonical_payload().unwrap(),
        Vec::new(),
    );
    let used = package
        .base
        .manifest
        .entries
        .iter()
        .map(|entry| entry.sha256.clone())
        .collect::<BTreeSet<_>>();
    package.base.blobs.retain(|sha, _| used.contains(sha));
    definition.security_profiles = vec![profile];
    source_commitments(&mut package, &definition, trust);
    (package, definition)
}

fn six_rows(store: &dyn ProviderStateStore) -> Vec<ProviderStateRecord> {
    let mut full = selected_rows(store);
    for namespace in [
        "db2-state",
        "db2-v1-table",
        "db2-v1-schema",
        "db2-v1-installation",
        "db2-v1-catalog-generation",
        "db2-v1-table-provenance",
        "db2-v1-legacy-snapshot",
        "db2-v1-unit-of-work",
        "db2-v1-cursor",
        "db2-v1-cursor-declaration",
        "db2-v1-replay",
        "ims-state",
        "ims-v1-database",
        "ims-v1-session-index",
        "ims-v1-checkpoint",
        "ims-v1-replay",
        "ims-v1-unit-of-work",
        "ims-v1-generic-database",
        "ims-v1-generic-unit-of-work",
        "ims-v1-system",
        "ims-v1-metadata-selection",
        "ims-v1-metadata-generation:LOCAL-COMPOSE",
        "mq-state",
        "mq-v1-object-catalog",
        "mq-v1-queue",
        "mq-v1-handle-index",
        "mq-v1-unit-of-work",
        "mq-v1-replay",
        "application-mq-catalog-selection",
        "batch-controller-state",
        "batch-program",
        "jes-job",
        "jes-meta",
        "jes-scheduler",
        "jes-topology",
        "racf-database-v2",
    ] {
        full.extend(store.list_provider_state(namespace, 4096).unwrap());
    }
    full.sort_by(|a, b| (&a.namespace, &a.key).cmp(&(&b.namespace, &b.key)));
    assert!(
        full.windows(2)
            .all(|pair| (&pair[0].namespace, &pair[0].key) < (&pair[1].namespace, &pair[1].key))
    );
    full
}

fn assert_six(
    server: &ProductServer,
    store: &dyn ProviderStateStore,
    generation: &ApplicationGenerationRecord,
) {
    assert_eq!(
        server.selected_local_application("LOCAL-COMPOSE").unwrap(),
        Some(generation.clone())
    );
    assert_eq!(
        server.online_transaction("LC01").unwrap().unwrap().0,
        "LOCALPGM"
    );
    let seed = if generation.generation == 1 {
        RAW_ONE
    } else {
        RAW_TWO
    };
    assert_eq!(
        seed_records(server),
        seed.as_chunks::<4>()
            .0
            .iter()
            .map(|chunk| chunk.to_vec())
            .collect::<Vec<_>>()
    );
    assert_eq!(
        server.db2.table_definition("LOCAL.CODES").unwrap(),
        postgres_table()
    );
    assert_eq!(
        server.db2.table_rows("LOCAL.CODES").unwrap(),
        if generation.generation == 1 {
            vec![vec![b"ONE".to_vec(), b"SIGNED".to_vec()]]
        } else {
            vec![
                vec![b"ONE".to_vec(), b"SIGNED".to_vec()],
                vec![b"TWO".to_vec(), b"SIGNED".to_vec()],
            ]
        }
    );
    let selected = json(
        &store
            .get_provider_state("application-local-selection", "LOCAL-COMPOSE")
            .unwrap()
            .unwrap(),
    );
    let db2: Db2CatalogProjection = serde_json::from_value(selected["db2"].clone()).unwrap();
    assert_eq!(db2.application, "LOCAL-COMPOSE");
    assert_eq!(db2.generation, generation.generation);
    assert_eq!(db2.identity, generation.identity);
    assert_eq!(
        db2.tables,
        BTreeMap::from([("LOCAL.CODES".into(), postgres_table())])
    );
    assert_eq!(
        server.db2.selected_catalog_composition_matches(&db2),
        Ok(())
    );
    let ims: ImsCatalogProjection = serde_json::from_value(selected["ims"].clone()).unwrap();
    assert_eq!(ims.catalog, postgres_metadata());
    assert_eq!(ims.package_identity, generation.identity);
    assert_eq!(
        server.ims.selected_catalog_composition_matches(&ims),
        Ok(())
    );
    let mq: MqCatalogProjection = serde_json::from_value(selected["mq"].clone()).unwrap();
    assert_eq!(
        mq.definitions,
        vec![MqQueueDefinition {
            name: "LOCAL.QUEUE".into(),
            trigger_program: None
        }]
    );
    assert_eq!(mq.package_identity, generation.identity);
    assert_eq!(server.mq.selected_catalog_composition_matches(&mq), Ok(()));
    let batch: BatchControllerProjection =
        serde_json::from_value(selected["batch"].clone()).unwrap();
    assert_eq!(batch.generation.application, "LOCAL-COMPOSE");
    assert_eq!(batch.generation.identity, generation.identity);
    assert_eq!(batch.generation.controllers.len(), 1);
    assert_eq!(batch.generation.controllers[0].name, "LOCAL-CONTROLLER");
    assert_eq!(
        store
            .get_provider_state("batch-program", "LOCALPGM")
            .unwrap()
            .unwrap()
            .payload,
        batch.generation.controllers[0].program.identity.as_bytes()
    );
    assert_eq!(
        server.batch.selected_controller_composition_matches(&batch),
        Ok(())
    );
    let racf = store
        .get_provider_state("racf-database-v2", "authority")
        .unwrap()
        .unwrap();
    assert!(racf.payload.starts_with(b"MERACF2\0"));
    let authority: serde_json::Value = serde_json::from_slice(&racf.payload[8..]).unwrap();
    let profile = &authority["profiles"]["FACILITY:LOCAL.ACCESS"];
    assert_eq!(profile["owner"], "LOCALOWN");
    assert_eq!(profile["uacc"], "NONE");
    assert_eq!(profile["access_list"][0]["principal"], "LOCALOWN");
    assert_eq!(
        profile["access_list"][0]["access"],
        if generation.generation == 1 {
            "READ"
        } else {
            "UPDATE"
        }
    );
}

#[test]
#[ignore = "requires Root-owned reset PostgreSQL fixture"]
fn postgres_signed_six_participant_cas_reopen_rollback_and_unknown_parity() {
    let url = postgres_url();
    let inner = postgres_store(&url);
    let fault = Arc::new(FaultStore::new(inner.clone()));
    let store: Arc<dyn PlatformStore> = fault.clone();
    let trust = Arc::new(test_package_trust());
    let server = postgres_server(store.clone(), &url, trust.clone());
    server.racf.add_group("LOCALOWN").unwrap();
    let (package, definition) = postgres_six_definitions(&server, &trust, 1);
    let staged = server.install_application_package_v2(&package).unwrap();
    let first = server
        .publish_local_application(&staged, definition)
        .unwrap();
    assert_six(&server, inner.as_ref(), &first.generation);
    let full = six_rows(inner.as_ref());
    drop(server);
    let peer = postgres_store(&url);
    assert_eq!(six_rows(peer.as_ref()), full);
    let server = postgres_server(store.clone(), &url, trust.clone());
    assert_six(&server, peer.as_ref(), &first.generation);
    // A later sorted companion conflicts only after real writes enter the one physical transaction.
    let companion = ProviderStateRecord {
        namespace: "zz-postgres-composition-companion".into(),
        key: "owned".into(),
        version: 1,
        payload: b"ORIGINAL".to_vec(),
    };
    peer.put_provider_state(companion.clone(), None).unwrap();
    let before = six_rows(inner.as_ref());
    let mut writes = before
        .iter()
        .map(|row| ProviderStateWrite {
            record: ProviderStateRecord {
                version: row.version + 1,
                ..row.clone()
            },
            expected_version: Some(row.version),
        })
        .collect::<Vec<_>>();
    writes.push(ProviderStateWrite {
        record: companion.clone(),
        expected_version: None,
    });
    assert!(matches!(
        ProviderCompositionCommit::publish(store.clone(), writes),
        Err(StoreError::Conflict)
    ));
    assert_eq!(six_rows(inner.as_ref()), before);
    assert_eq!(six_rows(peer.as_ref()), before);
    assert_eq!(
        peer.get_provider_state(&companion.namespace, &companion.key)
            .unwrap(),
        Some(companion)
    );
    let (package, definition) = postgres_six_definitions(&server, &trust, 2);
    let staged = server.install_application_package_v2(&package).unwrap();
    let before = six_rows(peer.as_ref());
    fault.arm(CompositionOutcome::Before);
    assert_eq!(
        server.publish_local_application(&staged, definition),
        Err(HostProblem::UnknownOutcome)
    );
    assert!(!server.readiness().accepting);
    assert_eq!(six_rows(peer.as_ref()), before);
    drop(server);
    let server = postgres_server(store.clone(), &url, trust.clone());
    assert_six(&server, peer.as_ref(), &first.generation);
    let (_, definition) = postgres_six_definitions(&server, &trust, 2);
    fault.arm(CompositionOutcome::After);
    assert_eq!(
        server.publish_local_application(&staged, definition),
        Err(HostProblem::UnknownOutcome)
    );
    assert!(!server.readiness().accepting);
    assert_eq!(
        server.selected_local_application("LOCAL-COMPOSE"),
        Err(HostProblem::UnknownOutcome)
    );
    let landed = six_rows(peer.as_ref());
    assert_ne!(landed, before);
    drop(server);
    let server = postgres_server(peer.clone(), &url, trust.clone());
    let ready = ApplicationGenerationRecord {
        state: InstallState::Ready,
        ..staged.clone()
    };
    assert_six(&server, peer.as_ref(), &ready);
    assert_eq!(six_rows(peer.as_ref()), landed);
    let (_, definition) = postgres_six_definitions(&server, &trust, 1);
    let rollback = server
        .rollback_local_application(&first.generation, definition)
        .unwrap();
    assert_eq!(rollback.generation, first.generation);
    assert_six(&server, peer.as_ref(), &first.generation);
    let rolled = six_rows(peer.as_ref());
    drop((server, peer, store, fault, inner));
    let physical = postgres_store(&url);
    let reopened = postgres_server(physical.clone(), &url, trust);
    assert_eq!(six_rows(physical.as_ref()), rolled);
    assert_six(&reopened, physical.as_ref(), &first.generation);
    assert!(!physical.root_custody_supported());
}

fn postgres_complete_fixture(store: Arc<dyn PlatformStore>, url: &str) -> CompleteProducer {
    let trust = Arc::new(test_package_trust());
    let server = postgres_server(store.clone(), url, trust.clone());
    let (mut package, mut definition, artifact) = compiled_definitions(&server, &trust);
    let spool_resources = bind_launch_fixture_jes(&server, &mut package, &mut definition);
    let marker = b"mainframe-env.application-local-complete-initial@1";
    let digest = format!("sha256:{:x}", Sha256::digest(marker));
    package.base.blobs.insert(digest.clone(), marker.to_vec());
    package.base.manifest.entries.push(PackageEntry {
        path: "resource/local-complete-publication".into(),
        kind: EntryKind::Resource,
        sha256: digest,
        bytes: marker.len(),
        depends_on: vec!["program/LOCALPGM".into()],
    });
    resign_package(&mut package, &trust);
    let controller_identity = package
        .base
        .manifest
        .entries
        .iter()
        .find(|entry| entry.path == "program/LOCALPGM" && entry.kind == EntryKind::Program)
        .unwrap()
        .sha256
        .clone();
    let staged = server.install_application_package_v2(&package).unwrap();
    // The actual six-owner initial publication port mints and acknowledges this row.
    // No control writes a Complete/Applied provider row by hand.
    let installed = server
        .publish_complete_local_application(&staged, definition)
        .unwrap();
    let generation = installed.generation;
    let publication = store
        .get_provider_state(APPLICATION_PUBLICATION_NAMESPACE, "LOCAL-COMPOSE")
        .unwrap()
        .unwrap();
    let expected = ApplicationPublicationState {
        schema_version: APPLICATION_PUBLICATION_CONTRACT.into(),
        package: "LOCAL-COMPOSE".into(),
        generation: 1,
        identity: generation.identity.clone(),
        action: PublicationAction::Install,
        controllers: PublicationSectionState::Applied,
        db2: PublicationSectionState::NotApplicable,
        ims: PublicationSectionState::NotApplicable,
        complete: true,
    };
    assert_eq!(
        publication,
        ProviderStateRecord {
            namespace: APPLICATION_PUBLICATION_NAMESPACE.into(),
            key: "LOCAL-COMPOSE".into(),
            version: 1,
            payload: serde_json::to_vec(&expected).unwrap()
        }
    );
    assert_eq!(
        server
            .selected_complete_local_application("LOCAL-COMPOSE")
            .unwrap(),
        Some(generation.clone())
    );
    assert_eq!(
        server.selected_local_application("LOCAL-COMPOSE"),
        Err(HostProblem::IdempotencyConflict)
    );
    assert_launch_fixture_jes(&server, &spool_resources);
    let mut source = invocation("local-compiled-source");
    source.selector = Selector::new("program:LOCALPUT", InvocationLimits::default()).unwrap();
    source.artifact = artifact;
    let installed = server
        .artifacts
        .get_artifact(&source.artifact)
        .unwrap()
        .unwrap();
    let executable = crate::cobol::artifact::admit_executable_artifact(&installed).unwrap();
    assert_eq!(
        executable.content_id().to_reference(),
        source.artifact.as_str()
    );
    assert_eq!(
        server.online_programs.lock().unwrap().get("LOCALPUT"),
        Some(&source.artifact)
    );
    let request = MqRequest {
        operation: MqOperation::PutOne,
        queue: Some("LOCAL.QUEUE".into()),
        handle: None,
        options: 0,
        message: DATA.to_vec(),
        message_id: Some(MESSAGE_ID.to_vec()),
        correlation_id: Some(CORRELATION_ID.to_vec()),
        wait_ticks: 0,
        max_message_bytes: 20,
        mutation: Some(Mutation {
            sequence: 1,
            transaction: None,
            idempotency_key: IdempotencyKey::new(
                "invocation-local-compiled-source:1",
                InvocationLimits::default(),
            )
            .unwrap(),
        }),
    };
    let before_manifest = store
        .get_provider_state("mq-state", "queues")
        .unwrap()
        .unwrap();
    let before_queue = store
        .get_provider_state("mq-v1-queue", "LOCAL.QUEUE")
        .unwrap()
        .unwrap();
    let stable = stable_rows(store.as_ref(), source_key(&request).as_str());
    let producer = InstalledProducer {
        server,
        source,
        request,
        before_manifest,
        before_queue,
        stable,
    };
    // Full independent message/ID, core, lifecycle, audit and replay oracle is the preserved source control.
    producer.known();
    let stable = unchanged_catalog_rows(store.as_ref());
    CompleteProducer {
        producer,
        generation,
        publication,
        controller_identity,
        stable,
    }
}

#[test]
#[ignore = "requires Root-owned reset PostgreSQL fixture"]
fn postgres_real_complete_compiled_mq_to_jes_reopen_replay_and_foreign_refusal() {
    let url = postgres_url();
    let store = postgres_store(&url);
    let fixture = postgres_complete_fixture(store.clone(), &url);
    let p = &fixture.producer;
    let before = launch_rows(store.as_ref());
    let mut foreign = p.source.clone();
    foreign.artifact = ArtifactRef::new("foreign-artifact", InvocationLimits::default()).unwrap();
    assert_eq!(
        p.server
            .start_committed_local_mq_controller("LOCAL-COMPOSE", &foreign, &p.request),
        Err(HostProblem::Unauthorized)
    );
    let mut foreign_request = p.request.clone();
    foreign_request.message[0] ^= 1;
    assert_eq!(
        p.server
            .start_committed_local_mq_controller("LOCAL-COMPOSE", &p.source, &foreign_request),
        Err(HostProblem::IdempotencyConflict)
    );
    assert_eq!(launch_rows(store.as_ref()), before);
    let (job, queued) = start(&fixture);
    let before = launch_rows(store.as_ref());
    assert_eq!(
        p.server
            .start_committed_local_mq_controller("LOCAL-COMPOSE", &p.source, &p.request),
        Err(HostProblem::IdempotencyConflict)
    );
    assert_eq!(launch_rows(store.as_ref()), before);
    assert_eq!(
        p.server
            .retire_committed_local_mq_controller("LOCAL-COMPOSE", source_key(&p.request)),
        Err(HostProblem::UnknownOutcome)
    );
    assert_eq!(
        p.server.run_jes_worker_once(WORKER).unwrap(),
        Some(queued.work_id.clone())
    );
    let (actual, row, checkpoint, work) = assert_real_retirement(&fixture, &job, &queued);
    assert_eq!(
        p.server
            .retire_committed_local_mq_controller("LOCAL-COMPOSE", source_key(&p.request))
            .unwrap(),
        job
    );
    let completed = occurrence(
        &fixture,
        3,
        expected_occurrence(&fixture, "Completed", Some(&job), Some(&row)),
    );
    let full = launch_rows(store.as_ref());
    assert_source_and_catalog(&fixture);
    let source = p.source.clone();
    let request = p.request.clone();
    let generation = fixture.generation.clone();
    let publication = fixture.publication.clone();
    let before_manifest = p.before_manifest.clone();
    let before_queue = p.before_queue.clone();
    let stable = fixture.stable.clone();
    let controller_identity = fixture.controller_identity.clone();
    drop((fixture, store));
    let physical = postgres_store(&url);
    let server = postgres_server(physical.clone(), &url, Arc::new(test_package_trust()));
    let reopened = CompleteProducer {
        producer: InstalledProducer {
            server,
            source,
            request,
            before_manifest,
            before_queue,
            stable: Vec::new(),
        },
        generation,
        publication,
        controller_identity,
        stable,
    };
    assert_eq!(launch_rows(physical.as_ref()), full);
    assert_eq!(
        physical
            .get_provider_state(OCCURRENCES, &completed.key)
            .unwrap(),
        Some(completed)
    );
    assert_eq!(reopened.producer.server.batch.get(&job).unwrap(), actual);
    assert_eq!(
        physical.get_checkpoint(&checkpoint.execution_id).unwrap(),
        Some(checkpoint)
    );
    assert_eq!(physical.get_work(&queued.work_id).unwrap(), Some(work));
    assert_source_and_catalog(&reopened);
    let before = launch_rows(physical.as_ref());
    assert_eq!(
        reopened
            .producer
            .server
            .start_committed_local_mq_controller(
                "LOCAL-COMPOSE",
                &reopened.producer.source,
                &reopened.producer.request
            ),
        Err(HostProblem::IdempotencyConflict)
    );
    assert_eq!(
        reopened
            .producer
            .server
            .retire_committed_local_mq_controller(
                "LOCAL-COMPOSE",
                source_key(&reopened.producer.request)
            ),
        Err(HostProblem::UnknownOutcome)
    );
    assert_eq!(
        reopened
            .producer
            .server
            .run_jes_worker_once(WORKER)
            .unwrap(),
        None
    );
    assert_eq!(launch_rows(physical.as_ref()), before);
    assert_eq!(
        physical.list_provider_state("jes-job", 16).unwrap().len(),
        1
    );
}

#[test]
#[ignore = "requires Root-owned reset PostgreSQL fixture"]
fn postgres_occurrence_before_after_ack_loss_never_redispatches_or_creates_job() {
    let url = postgres_url();
    let inner = postgres_store(&url);
    let fault = Arc::new(OccurrenceFaultStore::new(inner.clone()));
    let store: Arc<dyn PlatformStore> = fault.clone();
    let mut fixture = postgres_complete_fixture(store.clone(), &url);
    let physical = postgres_store(&url);
    for outcome in [OccurrenceOutcome::Before, OccurrenceOutcome::After] {
        let before = launch_rows(physical.as_ref());
        fault.arm(outcome);
        assert_eq!(
            fixture.producer.server.start_committed_local_mq_controller(
                "LOCAL-COMPOSE",
                &fixture.producer.source,
                &fixture.producer.request
            ),
            Err(HostProblem::UnknownOutcome)
        );
        match outcome {
            OccurrenceOutcome::Before => assert_eq!(launch_rows(physical.as_ref()), before),
            OccurrenceOutcome::After => {
                let unknown = occurrence(
                    &fixture,
                    1,
                    expected_occurrence(&fixture, "LaunchUnknown", None, None),
                );
                let binding = physical
                    .get_provider_state(BINDINGS, key(&fixture).as_str())
                    .unwrap()
                    .unwrap();
                assert_eq!(binding.version, 1);
                assert_eq!(json(&binding), expected_binding(&fixture));
                let mut expected = before
                    .into_iter()
                    .map(|row| {
                        if matches!(
                            (row.namespace.as_str(), row.key.as_str()),
                            ("mq-state", "queues")
                                | ("mq-v1-queue", "LOCAL.QUEUE")
                                | ("mq-v1-replay", "invocation-local-compiled-source:1")
                        ) {
                            ProviderStateRecord {
                                version: row.version + 1,
                                ..row
                            }
                        } else {
                            row
                        }
                    })
                    .collect::<Vec<_>>();
                expected.extend([unknown, binding]);
                expected.sort_by(|a, b| (&a.namespace, &a.key).cmp(&(&b.namespace, &b.key)));
                assert_eq!(launch_rows(physical.as_ref()), expected);
            }
        }
        assert!(
            physical
                .list_provider_state("jes-job", 16)
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            fixture.producer.server.run_jes_worker_once(WORKER).unwrap(),
            None
        );
        assert_eq!(
            physical
                .effect(source_key(&fixture.producer.request))
                .unwrap(),
            Some(expected_effect(
                &fixture.producer.source,
                &fixture.producer.request
            ))
        );
        assert_eq!(
            physical
                .get_provider_state(APPLICATION_PUBLICATION_NAMESPACE, "LOCAL-COMPOSE")
                .unwrap(),
            Some(fixture.publication.clone())
        );
        assert_eq!(
            fixture.producer.server.mq.queue_depth("LOCAL.QUEUE"),
            Err(HostProblem::UnknownOutcome)
        );
        let before = launch_rows(physical.as_ref());
        assert_eq!(
            fixture.producer.server.start_committed_local_mq_controller(
                "LOCAL-COMPOSE",
                &fixture.producer.source,
                &fixture.producer.request
            ),
            Err(HostProblem::UnknownOutcome)
        );
        assert_eq!(launch_rows(physical.as_ref()), before);
        assert_eq!(
            fixture
                .producer
                .server
                .retire_committed_local_mq_controller(
                    "LOCAL-COMPOSE",
                    source_key(&fixture.producer.request)
                ),
            Err(match outcome {
                OccurrenceOutcome::Before => HostProblem::NotFound,
                OccurrenceOutcome::After => HostProblem::UnknownOutcome,
            })
        );
        assert_eq!(launch_rows(physical.as_ref()), before);
        // A new server and real separate database connection observe storage, never settle Unknown or launch it.
        fixture.producer.server =
            postgres_server(store.clone(), &url, Arc::new(test_package_trust()));
        assert_eq!(launch_rows(physical.as_ref()), before);
        assert_eq!(
            fixture
                .producer
                .server
                .selected_complete_local_application("LOCAL-COMPOSE")
                .unwrap(),
            Some(fixture.generation.clone())
        );
        assert!(
            physical
                .list_provider_state("jes-job", 16)
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            fixture.producer.server.run_jes_worker_once(WORKER).unwrap(),
            None
        );
    }
    assert!(!physical.root_custody_supported());
}
