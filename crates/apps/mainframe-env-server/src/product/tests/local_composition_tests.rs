//! Genuine Memory/SQLite participants; no synthetic global complete or IssueObservation.
use super::super::local_composition::{
    LocalApplicationDefinition, LocalApplicationReceipt, LocalSourceCompilation,
};
use super::*;
use mainframe_env_application::{ApplicationManifest, ApplicationPackage, PackageEntry};
use mainframe_env_cics::CicsFileDefinition;
use mainframe_env_dataset::DatasetSeedObject;
use mainframe_env_store::StoreLimits;
use mainframe_env_store_api::{ProviderCompositionCommit, ProviderStateStore};
#[path = "local_composition_fault.rs"]
mod fault;
use fault::{FaultStore, Outcome};

const RAW_ONE: &[u8] = &[0xc1, 0xc1, 0xf1, 0xf1, 0xc2, 0xc2, 0xf2, 0xf2];
const RAW_TWO: &[u8] = &[0xc3, 0xc3, 0xf3, 0xf3, 0xc4, 0xc4, 0xf4, 0xf4];

fn fixture(
    store: Arc<dyn PlatformStore>,
    trust: Arc<HmacSha256PackageTrust>,
) -> Arc<ProductServer> {
    let mut settings = config();
    settings.store_profile = crate::StoreProfile::Postgres;
    settings.artifact_profile = ArtifactProfile::Shared;
    settings.postgres_url_reference = Some("env-base64:MAINFRAME_ENV_SECRET_POSTGRES_URL".into());
    settings.validate().unwrap();
    let artifacts: Arc<dyn ArtifactStore> = store.clone();
    ProductServer::open_with_package_trust_and_artifact_store(
        settings,
        store,
        Arc::new(MemorySecretResolver::default()),
        default_program_router(),
        trust,
        artifacts,
    )
    .unwrap()
}
fn definitions(
    server: &ProductServer,
    trust: &HmacSha256PackageTrust,
    generation: u64,
) -> (ApplicationPackageV2, LocalApplicationDefinition) {
    let source = b"IDENTIFICATION DIVISION.\nPROGRAM-ID. LOCALPGM.\nPROCEDURE DIVISION.\nCONTINUE.\nSTOP RUN.\n";
    let published = published_source_fixture("LOCALPGM", std::str::from_utf8(source).unwrap());
    let program = OnlineProgramDefinition::current("LOCALPGM", &published);
    let map = BmsMapDefinition {
        mapset: "LOCALMAP".into(),
        map: "LOCALMAP".into(),
        line: generation as u16,
        column: 1,
        rows: 24,
        columns: 80,
        fields: Vec::new(),
    };
    let file = CicsFileDefinition {
        dataset: DatasetName::new("LOCAL.SEED", 128).unwrap(),
        ccsid: Some(37),
    };
    let bytes = if generation == 1 { RAW_ONE } else { RAW_TWO };
    let seed = DatasetSeedObject {
        source_id: "data/raw-seed".into(),
        dataset: file.dataset.clone(),
        attributes: DatasetAttributes {
            organization: DatasetOrganization::Sequential,
            record_format: RecordFormat::Fixed,
            logical_record_length: 4,
            key_offset: None,
            key_length: None,
            ccsid: Some(37),
        },
        record_length: 4,
        sha256: format!("sha256:{:x}", Sha256::digest(bytes)),
        bytes: bytes.to_vec(),
    };
    let seed_snapshot = {
        let prepared = server
            .dataset
            .prepare_seed_selection(
                "LOCAL-COMPOSE",
                &generation.to_string(),
                vec![seed.clone()],
                true,
            )
            .unwrap();
        prepared.snapshot_digest().unwrap()
    };
    let inputs = vec![
        (EntryKind::Source, "source/LOCALPGM", source.to_vec()),
        (
            EntryKind::Program,
            "program/LOCALPGM",
            program.payload.clone(),
        ),
        (
            EntryKind::Resource,
            "resource/map",
            server.cics.composition_map_payload(&map).unwrap(),
        ),
        (
            EntryKind::Resource,
            "resource/file",
            server.cics.composition_file_payload(&file).unwrap(),
        ),
        (
            EntryKind::Resource,
            "transaction/LC01",
            b"LOCALPGM".to_vec(),
        ),
        (
            EntryKind::Resource,
            "seed/snapshot",
            seed_snapshot.into_bytes(),
        ),
        (EntryKind::Data, "data/raw-seed", seed.bytes.clone()),
        (
            EntryKind::Profile,
            "profile/local",
            b"explicit-local-finite-profile@1".to_vec(),
        ),
        (
            EntryKind::Migration,
            "migration/content",
            b"retained-content-only-no-external-migration".to_vec(),
        ),
    ];
    let mut entries = Vec::new();
    let mut blobs = BTreeMap::new();
    for (kind, path, bytes) in inputs {
        let sha256 = format!("sha256:{:x}", Sha256::digest(&bytes));
        entries.push(PackageEntry {
            path: path.into(),
            kind,
            sha256: sha256.clone(),
            bytes: bytes.len(),
            depends_on: if kind == EntryKind::Source {
                Vec::new()
            } else {
                vec!["source/LOCALPGM".into()]
            },
        });
        blobs.insert(sha256, bytes);
    }
    let mut package = signed_controller_package(trust);
    package.base = ApplicationPackage {
        manifest: ApplicationManifest {
            name: "LOCAL-COMPOSE".into(),
            version: "0.2.0".into(),
            target_product: "0.2.0".into(),
            entries,
        },
        blobs,
    };
    package.generation = generation;
    package.sections.batch_controllers.clear();
    resign_package(&mut package, trust);
    let definition = LocalApplicationDefinition {
        online: OnlineApplicationDefinition {
            programs: vec![program],
            transactions: BTreeMap::from([("LC01".into(), "LOCALPGM".into())]),
            maps: vec![map],
        },
        compilations: vec![source_compilation("LOCALPGM", source)],
        security_profiles: Vec::new(),
        seeds: vec![seed],
        files: BTreeMap::from([("LOCALFILE".into(), file)]),
        map_entries: BTreeMap::from([(
            ("LOCALMAP".into(), "LOCALMAP".into()),
            "resource/map".into(),
        )]),
        file_entries: BTreeMap::from([("LOCALFILE".into(), "resource/file".into())]),
    };
    source_commitments(&mut package, &definition, trust);
    (package, definition)
}
// Build the distinct signed fixture before either package selects the shared
// dataset. The snapshot comes from the real original seed codec, not a guessed
// digest or a bypass after the foreign selection has become authoritative.
fn foreign_definitions(
    server: &ProductServer,
    trust: &HmacSha256PackageTrust,
) -> (ApplicationPackageV2, LocalApplicationDefinition) {
    let (mut package, mut definition) = definitions(server, trust, 1);
    let source = b"IDENTIFICATION DIVISION.\nPROGRAM-ID. OTHERPGM.\nPROCEDURE DIVISION.\nCONTINUE.\nSTOP RUN.\n";
    let published = published_source_fixture("OTHERPGM", std::str::from_utf8(source).unwrap());
    let program = OnlineProgramDefinition::current("OTHERPGM", &published);
    let map = BmsMapDefinition {
        mapset: "OTHERMAP".into(),
        map: "OTHERMAP".into(),
        line: 1,
        column: 1,
        rows: 24,
        columns: 80,
        fields: Vec::new(),
    };
    let file = definition.files["LOCALFILE"].clone();
    let snapshot = {
        let prepared = server
            .dataset
            .prepare_seed_selection("FOREIGN-COMPOSE", "1", definition.seeds.clone(), true)
            .unwrap();
        prepared.snapshot_digest().unwrap()
    };
    let inputs = vec![
        (EntryKind::Source, "source/OTHERPGM", source.to_vec()),
        (
            EntryKind::Program,
            "program/OTHERPGM",
            program.payload.clone(),
        ),
        (
            EntryKind::Resource,
            "resource/map",
            server.cics.composition_map_payload(&map).unwrap(),
        ),
        (
            EntryKind::Resource,
            "resource/file",
            server.cics.composition_file_payload(&file).unwrap(),
        ),
        (
            EntryKind::Resource,
            "transaction/OC01",
            b"OTHERPGM".to_vec(),
        ),
        (EntryKind::Resource, "seed/snapshot", snapshot.into_bytes()),
        (
            EntryKind::Data,
            "data/raw-seed",
            definition.seeds[0].bytes.clone(),
        ),
        (
            EntryKind::Profile,
            "profile/local",
            b"explicit-local-finite-profile@1".to_vec(),
        ),
        (
            EntryKind::Migration,
            "migration/content",
            b"retained-content-only-no-external-migration".to_vec(),
        ),
    ];
    let mut entries = Vec::new();
    let mut blobs = BTreeMap::new();
    for (kind, path, bytes) in inputs {
        let sha256 = format!("sha256:{:x}", Sha256::digest(&bytes));
        entries.push(PackageEntry {
            path: path.into(),
            kind,
            sha256: sha256.clone(),
            bytes: bytes.len(),
            depends_on: if kind == EntryKind::Source {
                Vec::new()
            } else {
                vec!["source/OTHERPGM".into()]
            },
        });
        blobs.insert(sha256, bytes);
    }
    package.base = ApplicationPackage {
        manifest: ApplicationManifest {
            name: "FOREIGN-COMPOSE".into(),
            version: "0.2.0".into(),
            target_product: "0.2.0".into(),
            entries,
        },
        blobs,
    };
    resign_package(&mut package, trust);
    definition.online = OnlineApplicationDefinition {
        programs: vec![program],
        transactions: BTreeMap::from([("OC01".into(), "OTHERPGM".into())]),
        maps: vec![map],
    };
    definition.files = BTreeMap::from([("OTHERFIL".into(), file)]);
    definition.map_entries = BTreeMap::from([(
        ("OTHERMAP".into(), "OTHERMAP".into()),
        "resource/map".into(),
    )]);
    definition.file_entries = BTreeMap::from([("OTHERFIL".into(), "resource/file".into())]);
    definition.compilations = vec![source_compilation("OTHERPGM", source)];
    source_commitments(&mut package, &definition, trust);
    (package, definition)
}

fn refuse_foreign_selection_after_runtime_truncate(
    server: &ProductServer,
    store: &dyn ProviderStateStore,
    trust: &HmacSha256PackageTrust,
    original: &LocalApplicationReceipt,
    foreign: (ApplicationPackageV2, LocalApplicationDefinition),
) {
    let dataset = DatasetName::new("LOCAL.SEED", 128).unwrap();
    let current = store
        .get_provider_state("dataset", dataset.as_str())
        .unwrap()
        .unwrap();
    let next_version = current.version.checked_add(1).unwrap();
    let mutation = Mutation {
        sequence: 2,
        transaction: None,
        idempotency_key: IdempotencyKey::new("foreign-empty-truncate", InvocationLimits::default())
            .unwrap(),
    };
    assert_eq!(
        server
            .dataset
            .invoke(DatasetRequest::Truncate {
                dataset,
                expected_version: Some(current.version),
                mutation
            })
            .unwrap(),
        DatasetResult::Mutated {
            version: next_version
        }
    );
    assert!(seed_records(server).is_empty());
    let expected = server.install_application_package_v2(&foreign.0).unwrap();
    let before = selected_rows(store);
    let selected_before = server
        .dataset
        .selected_seed_generation("LOCAL-COMPOSE")
        .unwrap();
    assert_eq!(selected_before, Some("1".into()));
    assert_eq!(
        server.publish_local_application(&expected, foreign.1),
        Err(HostProblem::IdempotencyConflict)
    );
    assert_eq!(selected_rows(store), before);
    assert_eq!(
        server.selected_local_application("LOCAL-COMPOSE").unwrap(),
        Some(original.generation.clone())
    );
    assert_eq!(
        server
            .selected_local_application("FOREIGN-COMPOSE")
            .unwrap(),
        None
    );
    assert_eq!(
        server
            .dataset
            .selected_seed_generation("LOCAL-COMPOSE")
            .unwrap(),
        selected_before
    );
    assert_eq!(
        server
            .dataset
            .selected_seed_generation("FOREIGN-COMPOSE")
            .unwrap(),
        None
    );
    assert!(seed_records(server).is_empty());
    // The owner package's genuine retry still preserves the empty runtime edit.
    let (_, definition) = definitions(server, trust, 1);
    let retry = server
        .publish_local_application(&original.generation, definition)
        .unwrap();
    assert!(retry.replayed && retry.seed.replayed);
    assert_eq!(selected_rows(store), before);
    assert!(seed_records(server).is_empty());
}

fn refuse_historical_overlapping_exact_retry() {
    let store = Arc::new(MemoryStore::new(Default::default()));
    let dataset = DatasetService::open(store.clone(), Default::default()).unwrap();
    let name = DatasetName::new("LOCAL.SEED", 128).unwrap();
    let seed = DatasetSeedObject {
        source_id: "data/one".into(),
        dataset: name.clone(),
        attributes: DatasetAttributes {
            organization: DatasetOrganization::Sequential,
            record_format: RecordFormat::Fixed,
            logical_record_length: 4,
            key_offset: None,
            key_length: None,
            ccsid: Some(37),
        },
        record_length: 4,
        sha256: format!("sha256:{:x}", Sha256::digest(&RAW_ONE[..4])),
        bytes: RAW_ONE[..4].to_vec(),
    };
    dataset
        .install_seed_generation("OWN", "1", vec![seed.clone()])
        .unwrap();
    for (version, key) in [(1, "legacy-empty-one"), (3, "legacy-empty-two")] {
        let mutation = Mutation {
            sequence: version,
            transaction: None,
            idempotency_key: IdempotencyKey::new(key, InvocationLimits::default()).unwrap(),
        };
        dataset
            .invoke(DatasetRequest::Truncate {
                dataset: name.clone(),
                expected_version: Some(version),
                mutation,
            })
            .unwrap();
        if version == 1 {
            dataset
                .install_seed_generation("OTHER", "1", vec![seed.clone()])
                .unwrap();
        }
    }
    let before = selected_rows(store.as_ref());
    assert!(matches!(
        dataset.prepare_seed_selection("OWN", "1", vec![seed], true),
        Err(HostProblem::IdempotencyConflict)
    ));
    assert_eq!(selected_rows(store.as_ref()), before);
    assert_eq!(
        dataset.selected_seed_generation("OWN").unwrap(),
        Some("1".into())
    );
    assert_eq!(
        dataset.selected_seed_generation("OTHER").unwrap(),
        Some("1".into())
    );
    let DatasetResult::Records { records, .. } = dataset
        .invoke(DatasetRequest::Read {
            dataset: name,
            member: None,
            key: None,
            control: Default::default(),
            max_records: 8,
        })
        .unwrap()
    else {
        panic!("unexpected read");
    };
    assert!(records.is_empty());
}

fn refuse_stale_or_missing_selected_authority() {
    for mode in 0..3 {
        let trust = Arc::new(test_package_trust());
        let store = Arc::new(MemoryStore::new(Default::default()));
        let server = fixture(store.clone(), trust.clone());
        install(&server, &trust, 1);
        let namespace = if mode == 0 {
            "dataset-seed-generation"
        } else {
            "dataset-seed-selection"
        };
        let key = if mode == 0 {
            "LOCAL-COMPOSE@1"
        } else {
            "LOCAL-COMPOSE"
        };
        let current = store.get_provider_state(namespace, key).unwrap().unwrap();
        if mode == 0 {
            store
                .delete_provider_state(namespace, key, current.version)
                .unwrap();
        } else {
            let mut changed = current.clone();
            changed.version += 1;
            if mode == 2 {
                changed.payload = b"malformed-selection".to_vec();
            }
            store
                .put_provider_state(changed, Some(current.version))
                .unwrap();
        }
        let before = selected_rows(store.as_ref());
        let (_, mut definition) = definitions_for_stale_owner(&trust);
        let objects = std::mem::take(&mut definition.seeds);
        let expected = if mode == 2 {
            HostProblem::InfrastructureFailure
        } else {
            HostProblem::UnknownOutcome
        };
        assert!(
            matches!(server.dataset.prepare_seed_selection("LOCAL-COMPOSE", "1", objects, true), Err(problem) if problem == expected)
        );
        assert_eq!(selected_rows(store.as_ref()), before);
    }
}

// Use a separate genuine empty fixture only to obtain the original typed seed
// input after the tested owner has deliberately lost its durable authority.
fn definitions_for_stale_owner(
    trust: &Arc<HmacSha256PackageTrust>,
) -> (ApplicationPackageV2, LocalApplicationDefinition) {
    let isolated = fixture(
        Arc::new(MemoryStore::new(Default::default())),
        trust.clone(),
    );
    definitions(&isolated, trust, 1)
}

fn source_compilation(program: &str, bytes: &[u8]) -> LocalSourceCompilation {
    let limits = SourceLimits::default();
    let path = LogicalPath::new(format!("{program}.cbl"), limits.max_path_bytes).unwrap();
    let source = SourceBundle::new(
        &path,
        vec![
            SourceFile::input(
                path.as_str(),
                bytes.to_vec(),
                SourceFormat::Free,
                SourceEncoding::Utf8,
                limits,
            )
            .unwrap(),
        ],
        BTreeMap::new(),
        Vec::new(),
        limits,
    )
    .unwrap();
    LocalSourceCompilation {
        program: program.into(),
        request: CompilerRequest {
            source,
            mode: CompilationMode::Executable,
            target: CompileTarget::new("reference").unwrap(),
            options: CompileOptions::new(BTreeMap::new()).unwrap(),
        },
        source_entries: BTreeMap::from([(path.as_str().into(), format!("source/{program}"))]),
    }
}

fn full_copy_source_closure_publishes_and_reopens() {
    use mainframe_env_source::{ProvenanceEdgeInput, ProvenanceKind, SourceLibrary};
    let trust = Arc::new(test_package_trust());
    let store = Arc::new(MemoryStore::new(Default::default()));
    let server = fixture(store.clone(), trust.clone());
    let (mut package, mut definition) = definitions(&server, &trust, 1);
    let limits = SourceLimits::default();
    let primary = LogicalPath::new("LOCALPGM.cbl", limits.max_path_bytes).unwrap();
    let a = LogicalPath::new("DATAA.cpy", limits.max_path_bytes).unwrap();
    let b = LogicalPath::new("DATAB.cpy", limits.max_path_bytes).unwrap();
    let main = b"IDENTIFICATION DIVISION.\nPROGRAM-ID. LOCALPGM.\nDATA DIVISION.\nWORKING-STORAGE SECTION.\nCOPY DATAA.\nCOPY DATAB.\nPROCEDURE DIVISION.\nCONTINUE.\nSTOP RUN.\n";
    let a_bytes = mainframe_env_encoding::CodePage::Cp037
        .encode("01 A-FLAG PIC X VALUE 'A'.\n", 1024)
        .unwrap();
    let b_bytes = b"01 B-FLAG PIC X VALUE 'B'.\n";
    let files = vec![
        SourceFile::input(
            primary.as_str(),
            main.to_vec(),
            SourceFormat::Free,
            SourceEncoding::Utf8,
            limits,
        )
        .unwrap(),
        SourceFile::input(
            a.as_str(),
            a_bytes,
            SourceFormat::Free,
            SourceEncoding::Ebcdic(37),
            limits,
        )
        .unwrap(),
        SourceFile::input(
            b.as_str(),
            b_bytes.to_vec(),
            SourceFormat::Free,
            SourceEncoding::Utf8,
            limits,
        )
        .unwrap(),
    ];
    let libraries = vec![
        SourceLibrary::new("first", vec![a.clone()], limits).unwrap(),
        SourceLibrary::new("second", vec![b.clone()], limits).unwrap(),
    ];
    let source = SourceBundle::with_libraries(
        &primary,
        files,
        libraries,
        BTreeMap::from([("cobol.define.LOCAL-MODE".into(), "true".into())]),
        vec![ProvenanceEdgeInput {
            generated_path: primary.clone(),
            generated_bytes: 0..1,
            origin_path: a.clone(),
            origin_bytes: 0..1,
            kind: ProvenanceKind::Generated,
        }],
        limits,
    )
    .unwrap();
    let expected_id = source.id().to_hex();
    let request = CompilerRequest {
        source,
        mode: CompilationMode::Executable,
        target: CompileTarget::new("reference").unwrap(),
        options: CompileOptions::new(BTreeMap::from([(
            mainframe_env_ir::COBOL_EFFECTIVE_LP_OPTION.into(),
            "32".into(),
        )]))
        .unwrap(),
    };
    let CompilerResult::Published { artifact, .. } =
        CobolCompiler::default().compile(request.clone()).unwrap()
    else {
        panic!("fixture did not publish");
    };
    definition.online.programs[0] = OnlineProgramDefinition::current("LOCALPGM", &artifact);
    let mapping: BTreeMap<String, String> = BTreeMap::from([
        (primary.as_str().into(), "source/LOCALPGM".into()),
        (a.as_str().into(), "source/DATAA".into()),
        (b.as_str().into(), "source/DATAB".into()),
    ]);
    package
        .base
        .manifest
        .entries
        .retain(|entry| !matches!(entry.kind, EntryKind::Source | EntryKind::Program));
    for file in request.source.files() {
        let sha = format!("sha256:{:x}", Sha256::digest(file.bytes()));
        package
            .base
            .blobs
            .insert(sha.clone(), file.bytes().to_vec());
        let path = mapping[file.path().as_str()].clone();
        package.base.manifest.entries.push(PackageEntry {
            depends_on: if path == "source/LOCALPGM" {
                vec!["source/DATAA".into(), "source/DATAB".into()]
            } else {
                Vec::new()
            },
            path,
            kind: EntryKind::Source,
            sha256: sha,
            bytes: file.bytes().len(),
        });
    }
    let sha = format!("sha256:{:x}", Sha256::digest(artifact.payload()));
    package
        .base
        .blobs
        .insert(sha.clone(), artifact.payload().to_vec());
    package.base.manifest.entries.push(PackageEntry {
        path: "program/LOCALPGM".into(),
        kind: EntryKind::Program,
        sha256: sha,
        bytes: artifact.payload().len(),
        depends_on: vec!["source/LOCALPGM".into()],
    });
    // Removed old source/program entries must not leave unsigned orphan blobs.
    let retained = package
        .base
        .manifest
        .entries
        .iter()
        .map(|entry| entry.sha256.clone())
        .collect::<BTreeSet<_>>();
    package.base.blobs.retain(|sha, _| retained.contains(sha));
    definition.compilations = vec![LocalSourceCompilation {
        program: "LOCALPGM".into(),
        request,
        source_entries: mapping,
    }];
    source_commitments(&mut package, &definition, &trust);
    let expected = server.install_application_package_v2(&package).unwrap();
    let actual = server
        .publish_local_application(&expected, definition)
        .unwrap();
    assert_eq!(
        actual.compiled_sources,
        BTreeMap::from([("LOCALPGM".into(), expected_id.clone())])
    );
    assert_eq!(
        seed_records(&server),
        RAW_ONE
            .as_chunks::<4>()
            .0
            .iter()
            .map(|chunk| chunk.to_vec())
            .collect::<Vec<_>>()
    );
    let before = selected_rows(store.as_ref());
    drop(server);
    let reopened = fixture(store.clone(), trust);
    assert_eq!(
        reopened
            .selected_local_application("LOCAL-COMPOSE")
            .unwrap(),
        Some(actual.generation)
    );
    assert_eq!(selected_rows(store.as_ref()), before);
    let retained = store
        .get_provider_state("application-local-selection", "LOCAL-COMPOSE")
        .unwrap()
        .unwrap();
    let payload: serde_json::Value = serde_json::from_slice(&retained.payload).unwrap();
    assert_eq!(
        payload["compiled_sources"],
        serde_json::json!({"LOCALPGM": expected_id})
    );
    assert_eq!(
        seed_records(&reopened),
        RAW_ONE
            .as_chunks::<4>()
            .0
            .iter()
            .map(|chunk| chunk.to_vec())
            .collect::<Vec<_>>()
    );
}

fn wrong_source_closure_or_entire_publisher_never_selects_ready() {
    for mode in 0..7 {
        let trust = Arc::new(test_package_trust());
        let store = Arc::new(MemoryStore::new(Default::default()));
        let server = fixture(store.clone(), trust.clone());
        let (package, mut definition) = definitions(&server, &trust, 1);
        let expected = server.install_application_package_v2(&package).unwrap();
        match mode {
            0 => definition.compilations.clear(),
            1 => {
                definition.compilations[0].source_entries.clear();
            }
            2 => {
                definition.compilations[0].request.mode = CompilationMode::Analyze;
            }
            3 => {
                definition.compilations[0].request.options =
                    CompileOptions::new(BTreeMap::from([(
                        "different".into(),
                        "publisher-option".into(),
                    )]))
                    .unwrap();
            }
            4 => {
                definition.online.programs[0].semantic_identity =
                    format!("semantic-sha256:{}", "0".repeat(64));
            }
            5 => {
                let bytes = b"IDENTIFICATION DIVISION.\nPROGRAM-ID. LOCALPGM.\nPROCEDURE DIVISION.\nCONTINUE.\nSTOP RUN.\n";
                let limits = SourceLimits::default();
                let path = LogicalPath::new("RENAMED.cbl", limits.max_path_bytes).unwrap();
                definition.compilations[0].request.source = SourceBundle::new(
                    &path,
                    vec![
                        SourceFile::input(
                            path.as_str(),
                            bytes.to_vec(),
                            SourceFormat::Free,
                            SourceEncoding::Utf8,
                            limits,
                        )
                        .unwrap(),
                    ],
                    BTreeMap::new(),
                    Vec::new(),
                    limits,
                )
                .unwrap();
                definition.compilations[0].source_entries =
                    BTreeMap::from([(path.as_str().into(), "source/LOCALPGM".into())]);
            }
            _ => {
                let source = b"IDENTIFICATION DIVISION.\nPROGRAM-ID. LOCALPGM.\nPROCEDURE DIVISION.\nDISPLAY 'DIFFERENT'.\nSTOP RUN.\n";
                definition.compilations[0] = source_compilation("LOCALPGM", source);
            }
        }
        let before = selected_rows(store.as_ref());
        assert!(
            server
                .publish_local_application(&expected, definition)
                .is_err()
        );
        assert_eq!(selected_rows(store.as_ref()), before);
        assert_eq!(
            server.selected_local_application("LOCAL-COMPOSE").unwrap(),
            None
        );
    }
}

// This contract signs the request identity, not a forged compiler result. A
// provenance-only change can retain the identical MIR payload; a supported
// effective request option can retain the entire identical published artifact.
fn source_commitments(
    package: &mut ApplicationPackageV2,
    definition: &LocalApplicationDefinition,
    trust: &HmacSha256PackageTrust,
) {
    package
        .base
        .manifest
        .entries
        .retain(|entry| !entry.path.starts_with("source-commitment/"));
    for input in &definition.compilations {
        let path = input.resource_path().unwrap();
        let bytes = input.resource_bytes().unwrap();
        let sha256 = format!("sha256:{:x}", Sha256::digest(&bytes));
        let dependencies = input
            .source_entries
            .values()
            .cloned()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let name = normalize_online_name(&input.program, 128).unwrap();
        let program = package
            .base
            .manifest
            .entries
            .iter_mut()
            .find(|entry| entry.path == format!("program/{name}"))
            .unwrap();
        if !program.depends_on.contains(&path) {
            program.depends_on.push(path.clone());
        }
        package.base.manifest.entries.push(PackageEntry {
            path,
            kind: EntryKind::Resource,
            sha256: sha256.clone(),
            bytes: bytes.len(),
            depends_on: dependencies,
        });
        package.base.blobs.insert(sha256, bytes);
    }
    let retained = package
        .base
        .manifest
        .entries
        .iter()
        .map(|entry| entry.sha256.clone())
        .collect::<BTreeSet<_>>();
    package.base.blobs.retain(|sha, _| retained.contains(sha));
    resign_package(package, trust);
}

fn same_payload_metadata_changes_require_new_signed_commitments() {
    use mainframe_env_source::{ProvenanceEdgeInput, ProvenanceKind};
    for provenance in [false, true] {
        let trust = Arc::new(test_package_trust());
        let store = Arc::new(MemoryStore::new(Default::default()));
        let server = fixture(store.clone(), trust.clone());
        let (package, mut definition) = definitions(&server, &trust, 1);
        let original = definition.compilations[0].clone();
        let original_program = definition.online.programs[0].clone();
        let original_commitment = original.resource_bytes().unwrap();
        if provenance {
            let limits = SourceLimits::default();
            let primary = LogicalPath::new("LOCALPGM.cbl", limits.max_path_bytes).unwrap();
            let source = original.request.source.files()[0].bytes().to_vec();
            definition.compilations[0].request.source = SourceBundle::new(
                &primary,
                vec![
                    SourceFile::input(
                        primary.as_str(),
                        source,
                        SourceFormat::Free,
                        SourceEncoding::Utf8,
                        limits,
                    )
                    .unwrap(),
                ],
                BTreeMap::new(),
                vec![ProvenanceEdgeInput {
                    generated_path: primary.clone(),
                    generated_bytes: 0..1,
                    origin_path: primary.clone(),
                    origin_bytes: 0..1,
                    kind: ProvenanceKind::Generated,
                }],
                limits,
            )
            .unwrap();
            assert_ne!(
                definition.compilations[0].request.source.id(),
                original.request.source.id()
            );
        } else {
            definition.compilations[0].request.options = CompileOptions::new(BTreeMap::from([(
                mainframe_env_ir::COBOL_EFFECTIVE_LP_OPTION.into(),
                "32".into(),
            )]))
            .unwrap();
            assert_eq!(
                definition.compilations[0].request.source.id(),
                original.request.source.id()
            );
        }
        let changed = definition.compilations[0].clone();
        let CompilerResult::Published { artifact, .. } = CobolCompiler::default()
            .compile(changed.request.clone())
            .unwrap()
        else {
            panic!("changed request did not publish");
        };
        let changed_program = OnlineProgramDefinition::current("LOCALPGM", &artifact);
        assert_eq!(changed_program.payload, original_program.payload);
        if !provenance {
            assert_eq!(changed_program, original_program);
        }
        assert_ne!(changed.resource_bytes().unwrap(), original_commitment);
        definition.online.programs[0] = changed_program.clone();
        let expected = server.install_application_package_v2(&package).unwrap();
        let before = selected_rows(store.as_ref());
        assert_eq!(
            server.publish_local_application(&expected, definition),
            Err(HostProblem::IdempotencyConflict)
        );
        assert_eq!(selected_rows(store.as_ref()), before);
        assert_eq!(
            server.selected_local_application("LOCAL-COMPOSE").unwrap(),
            None
        );

        // A separately signed, matching request commitment admits the same MIR
        // through the actual publisher; the prior signed package is immutable.
        let matching_store = Arc::new(MemoryStore::new(Default::default()));
        let matching = fixture(matching_store.clone(), trust.clone());
        let (mut matching_package, mut matching_definition) = definitions(&matching, &trust, 1);
        matching_definition.compilations[0] = changed.clone();
        matching_definition.online.programs[0] = changed_program;
        source_commitments(&mut matching_package, &matching_definition, &trust);
        let expected = matching
            .install_application_package_v2(&matching_package)
            .unwrap();
        let selected = matching
            .publish_local_application(&expected, matching_definition)
            .unwrap();
        assert_eq!(
            selected.compiled_sources,
            BTreeMap::from([("LOCALPGM".into(), changed.request.source.id().to_hex())])
        );
        assert_eq!(
            seed_records(&matching),
            RAW_ONE
                .as_chunks::<4>()
                .0
                .iter()
                .map(|chunk| chunk.to_vec())
                .collect::<Vec<_>>()
        );
        let rows = selected_rows(matching_store.as_ref());
        drop(matching);
        let reopened = fixture(matching_store.clone(), trust.clone());
        assert_eq!(
            reopened
                .selected_local_application("LOCAL-COMPOSE")
                .unwrap(),
            Some(selected.generation)
        );
        assert_eq!(selected_rows(matching_store.as_ref()), rows);
        assert_eq!(
            seed_records(&reopened),
            RAW_ONE
                .as_chunks::<4>()
                .0
                .iter()
                .map(|chunk| chunk.to_vec())
                .collect::<Vec<_>>()
        );
    }
}

fn missing_foreign_or_unlinked_source_commitments_refuse_before_preparation() {
    for mode in 0..4 {
        let trust = Arc::new(test_package_trust());
        let store = Arc::new(MemoryStore::new(Default::default()));
        let server = fixture(store.clone(), trust.clone());
        let (mut package, definition) = definitions(&server, &trust, 1);
        let path = definition.compilations[0].resource_path().unwrap();
        if mode == 0 {
            package
                .base
                .manifest
                .entries
                .retain(|entry| entry.path != path);
            for entry in &mut package.base.manifest.entries {
                entry.depends_on.retain(|dependency| dependency != &path);
            }
        } else if mode == 1 {
            let entry = package
                .base
                .manifest
                .entries
                .iter_mut()
                .find(|entry| entry.path == path)
                .unwrap();
            let raw = br#"{"contract":"foreign","source_id":"0000000000000000000000000000000000000000000000000000000000000000"}"#.to_vec();
            entry.sha256 = format!("sha256:{:x}", Sha256::digest(&raw));
            entry.bytes = raw.len();
            package.base.blobs.insert(entry.sha256.clone(), raw);
        } else if mode == 2 {
            package
                .base
                .manifest
                .entries
                .iter_mut()
                .find(|entry| entry.path == "program/LOCALPGM")
                .unwrap()
                .depends_on
                .retain(|dependency| dependency != &path);
        } else {
            package
                .base
                .manifest
                .entries
                .iter_mut()
                .find(|entry| entry.path == path)
                .unwrap()
                .depends_on
                .clear();
        }
        let retained = package
            .base
            .manifest
            .entries
            .iter()
            .map(|entry| entry.sha256.clone())
            .collect::<BTreeSet<_>>();
        package.base.blobs.retain(|sha, _| retained.contains(sha));
        resign_package(&mut package, &trust);
        let expected = server.install_application_package_v2(&package).unwrap();
        let before = selected_rows(store.as_ref());
        assert!(
            server
                .publish_local_application(&expected, definition)
                .is_err()
        );
        assert_eq!(selected_rows(store.as_ref()), before);
        assert_eq!(
            server.selected_local_application("LOCAL-COMPOSE").unwrap(),
            None
        );
    }
}

fn seed_records(server: &ProductServer) -> Vec<Vec<u8>> {
    let DatasetResult::Records { records, .. } = server
        .dataset
        .invoke(DatasetRequest::Read {
            dataset: DatasetName::new("LOCAL.SEED", 128).unwrap(),
            member: None,
            key: None,
            control: Default::default(),
            max_records: 8,
        })
        .unwrap()
    else {
        panic!("unexpected read");
    };
    records
}
fn selected_rows(store: &dyn ProviderStateStore) -> Vec<ProviderStateRecord> {
    let mut rows = Vec::new();
    for namespace in [
        "dataset",
        "dataset-seed-generation",
        "dataset-seed-selection",
        "cics-program",
        "cics-map",
        "cics-file-alias",
        "online-program",
        "online-transaction",
        "application-package-v2",
        "application-local-generation",
        "application-local-selection",
    ] {
        rows.extend(store.list_provider_state(namespace, 128).unwrap());
    }
    rows.sort_by(|left, right| (&left.namespace, &left.key).cmp(&(&right.namespace, &right.key)));
    rows
}
fn install(
    server: &ProductServer,
    trust: &HmacSha256PackageTrust,
    generation: u64,
) -> LocalApplicationReceipt {
    let (package, definition) = definitions(server, trust, generation);
    let expected = server.install_application_package_v2(&package).unwrap();
    server
        .publish_local_application(&expected, definition)
        .unwrap()
}

#[test]
fn all_six_owned_content_kinds_seed_cics_routes_and_ready_reopen_together() {
    let trust = Arc::new(test_package_trust());
    let store = Arc::new(MemoryStore::new(Default::default()));
    let server = fixture(store.clone(), trust.clone());
    let installed = install(&server, &trust, 1);
    assert_eq!(installed.generation.state, InstallState::Ready);
    assert_eq!(installed.seed.records, 2);
    assert_eq!(installed.seed.bytes, 8);
    assert_eq!(
        seed_records(&server),
        RAW_ONE
            .as_chunks::<4>()
            .0
            .iter()
            .map(|chunk| chunk.to_vec())
            .collect::<Vec<_>>()
    );
    assert_eq!(
        server.selected_local_application("LOCAL-COMPOSE").unwrap(),
        Some(installed.generation.clone())
    );
    assert_eq!(
        server.online_transaction("LC01").unwrap().unwrap().0,
        "LOCALPGM"
    );
    let rows = selected_rows(store.as_ref());
    drop(server);
    let reopened = fixture(store.clone(), trust.clone());
    assert_eq!(
        reopened
            .selected_local_application("LOCAL-COMPOSE")
            .unwrap(),
        Some(installed.generation)
    );
    assert_eq!(selected_rows(store.as_ref()), rows);
    assert_eq!(
        seed_records(&reopened),
        RAW_ONE
            .as_chunks::<4>()
            .0
            .iter()
            .map(|chunk| chunk.to_vec())
            .collect::<Vec<_>>()
    );
    full_copy_source_closure_publishes_and_reopens();
}
#[test]
fn exact_retry_preserves_runtime_edited_seed_and_versions() {
    let trust = Arc::new(test_package_trust());
    let store = Arc::new(MemoryStore::new(Default::default()));
    let server = fixture(store.clone(), trust.clone());
    let installed = install(&server, &trust, 1);
    let mutation = Mutation {
        sequence: 1,
        transaction: None,
        idempotency_key: IdempotencyKey::new("runtime-edit", InvocationLimits::default()).unwrap(),
    };
    server
        .dataset
        .invoke(DatasetRequest::Write {
            dataset: DatasetName::new("LOCAL.SEED", 128).unwrap(),
            member: None,
            records: vec![RAW_TWO[..4].to_vec()],
            expected_version: Some(1),
            mutation,
        })
        .unwrap();
    let before = selected_rows(store.as_ref());
    let (_, definition) = definitions(&server, &trust, 1);
    let retried = server
        .publish_local_application(&installed.generation, definition)
        .unwrap();
    assert!(retried.replayed && retried.seed.replayed);
    assert_eq!(selected_rows(store.as_ref()), before);
    assert_eq!(seed_records(&server), vec![RAW_TWO[..4].to_vec()]);
}
#[test]
fn upgrade_and_retained_rollback_reopen_full_original_rows() {
    let trust = Arc::new(test_package_trust());
    let store = Arc::new(MemoryStore::new(Default::default()));
    let server = fixture(store.clone(), trust.clone());
    let first = install(&server, &trust, 1);
    let second = install(&server, &trust, 2);
    assert_eq!(
        seed_records(&server),
        RAW_TWO
            .as_chunks::<4>()
            .0
            .iter()
            .map(|chunk| chunk.to_vec())
            .collect::<Vec<_>>()
    );
    let (_, definition) = definitions(&server, &trust, 1);
    let rolled = server
        .rollback_local_application(&first.generation, definition)
        .unwrap();
    assert_eq!(rolled.generation, first.generation);
    assert_ne!(rolled.generation, second.generation);
    assert_eq!(
        seed_records(&server),
        RAW_ONE
            .as_chunks::<4>()
            .0
            .iter()
            .map(|chunk| chunk.to_vec())
            .collect::<Vec<_>>()
    );
    drop(server);
    let reopened = fixture(store, trust);
    assert_eq!(
        reopened
            .selected_local_application("LOCAL-COMPOSE")
            .unwrap(),
        Some(first.generation)
    );
    assert_eq!(
        seed_records(&reopened),
        RAW_ONE
            .as_chunks::<4>()
            .0
            .iter()
            .map(|chunk| chunk.to_vec())
            .collect::<Vec<_>>()
    );
}
#[test]
fn missing_duplicate_foreign_and_unowned_resources_never_select_ready() {
    for case in 0..5 {
        let trust = Arc::new(test_package_trust());
        let store = Arc::new(MemoryStore::new(Default::default()));
        let server = fixture(store.clone(), trust.clone());
        let (package, mut definition) = definitions(&server, &trust, 1);
        let expected = server.install_application_package_v2(&package).unwrap();
        match case {
            0 => definition.map_entries.clear(),
            1 => definition
                .online
                .programs
                .push(definition.online.programs[0].clone()),
            2 => {
                definition.files.get_mut("LOCALFILE").unwrap().dataset =
                    DatasetName::new("FOREIGN.SEED", 128).unwrap()
            }
            3 => {
                server
                    .cics
                    .register_map(definition.online.maps[0].clone())
                    .unwrap();
            }
            _ => {
                definition
                    .online
                    .transactions
                    .insert("LC01".into(), "FOREIGN".into());
            }
        }
        let before = selected_rows(store.as_ref());
        assert!(
            server
                .publish_local_application(&expected, definition)
                .is_err()
        );
        assert_eq!(selected_rows(store.as_ref()), before);
        assert_eq!(
            server.selected_local_application("LOCAL-COMPOSE").unwrap(),
            None
        );
    }
    let trust = Arc::new(test_package_trust());
    let store = Arc::new(MemoryStore::new(Default::default()));
    let server = fixture(store.clone(), trust.clone());
    let foreign = foreign_definitions(&server, &trust);
    let original = install(&server, &trust, 1);
    refuse_foreign_selection_after_runtime_truncate(
        &server,
        store.as_ref(),
        &trust,
        &original,
        foreign,
    );
    let before = selected_rows(store.as_ref());
    drop(server);
    let reopened = fixture(store.clone(), trust);
    assert_eq!(
        reopened
            .selected_local_application("LOCAL-COMPOSE")
            .unwrap(),
        Some(original.generation)
    );
    assert_eq!(selected_rows(store.as_ref()), before);
    assert!(seed_records(&reopened).is_empty());
    wrong_source_closure_or_entire_publisher_never_selects_ready();
    same_payload_metadata_changes_require_new_signed_commitments();
    missing_foreign_or_unlinked_source_commitments_refuse_before_preparation();
}
#[test]
fn orphan_typed_entries_and_unprepared_subsystem_sections_are_refused() {
    for subsystem in [false, true] {
        let trust = Arc::new(test_package_trust());
        let store = Arc::new(MemoryStore::new(Default::default()));
        let server = fixture(store.clone(), trust.clone());
        let (mut package, definition) = definitions(&server, &trust, 1);
        if subsystem {
            package.sections.batch_controllers =
                signed_controller_package(&trust).sections.batch_controllers;
            package.sections.batch_controllers[0].program = "program/LOCALPGM".into();
            package.sections.batch_controllers[0]
                .properties
                .insert("selector-program".into(), "LOCALPGM".into());
        } else {
            let bytes = b"ORPHAN".to_vec();
            let sha = format!("sha256:{:x}", Sha256::digest(&bytes));
            package.base.blobs.insert(sha.clone(), bytes);
            package.base.manifest.entries.push(PackageEntry {
                path: "resource/orphan".into(),
                kind: EntryKind::Resource,
                sha256: sha,
                bytes: 6,
                depends_on: vec!["source/LOCALPGM".into()],
            });
        }
        resign_package(&mut package, &trust);
        let expected = server.install_application_package_v2(&package).unwrap();
        let before = selected_rows(store.as_ref());
        assert_eq!(
            server.publish_local_application(&expected, definition),
            Err(HostProblem::Unsupported)
        );
        assert_eq!(selected_rows(store.as_ref()), before);
    }
}
#[test]
fn unknown_before_atomic_commit_fences_live_owners_but_reopen_stays_staged() {
    let trust = Arc::new(test_package_trust());
    let store = Arc::new(FaultStore::new(Arc::new(MemoryStore::new(
        Default::default(),
    ))));
    let server = fixture(store.clone(), trust.clone());
    let (package, definition) = definitions(&server, &trust, 1);
    let expected = server.install_application_package_v2(&package).unwrap();
    let before = selected_rows(store.as_ref());
    store.arm(Outcome::Before);
    assert_eq!(
        server.publish_local_application(&expected, definition),
        Err(HostProblem::UnknownOutcome)
    );
    assert_eq!(selected_rows(store.as_ref()), before);
    assert!(!server.readiness().accepting);
    assert_eq!(
        server.selected_local_application("LOCAL-COMPOSE"),
        Err(HostProblem::UnknownOutcome)
    );
    drop(server);
    let reopened = fixture(store, trust);
    assert_eq!(
        reopened
            .selected_local_application("LOCAL-COMPOSE")
            .unwrap(),
        None
    );
}
#[test]
fn unknown_after_actual_atomic_commit_needs_reopen_for_ready_and_full_seed() {
    let trust = Arc::new(test_package_trust());
    let store = Arc::new(FaultStore::new(Arc::new(MemoryStore::new(
        Default::default(),
    ))));
    let server = fixture(store.clone(), trust.clone());
    let (package, definition) = definitions(&server, &trust, 1);
    let expected = server.install_application_package_v2(&package).unwrap();
    store.arm(Outcome::After);
    assert_eq!(
        server.publish_local_application(&expected, definition),
        Err(HostProblem::UnknownOutcome)
    );
    assert!(!server.readiness().accepting);
    assert_eq!(
        server.dataset.invoke(DatasetRequest::List {
            pattern: "LOCAL.*".into(),
            start: None,
            max_items: 8
        }),
        Err(HostProblem::UnknownOutcome)
    );
    drop(server);
    let reopened = fixture(store, trust);
    assert_eq!(
        reopened
            .selected_local_application("LOCAL-COMPOSE")
            .unwrap()
            .unwrap()
            .generation,
        1
    );
    assert_eq!(
        seed_records(&reopened),
        RAW_ONE
            .as_chunks::<4>()
            .0
            .iter()
            .map(|chunk| chunk.to_vec())
            .collect::<Vec<_>>()
    );
}
#[test]
fn partial_or_corrupt_static_authority_refuses_readiness_and_reopen() {
    for corrupt in [false, true] {
        let trust = Arc::new(test_package_trust());
        let store = Arc::new(MemoryStore::new(Default::default()));
        let server = fixture(store.clone(), trust.clone());
        install(&server, &trust, 1);
        let original = store
            .get_provider_state("online-transaction", "LC01")
            .unwrap()
            .unwrap();
        if corrupt {
            let mut changed = original.clone();
            changed.version += 1;
            changed.payload = b"FOREIGN".to_vec();
            store
                .put_provider_state(changed, Some(original.version))
                .unwrap();
        } else {
            store
                .delete_provider_state(&original.namespace, &original.key, original.version)
                .unwrap();
        }
        assert!(server.selected_local_application("LOCAL-COMPOSE").is_err());
        drop(server);
        let mut settings = config();
        settings.store_profile = crate::StoreProfile::Postgres;
        settings.artifact_profile = ArtifactProfile::Shared;
        settings.postgres_url_reference =
            Some("env-base64:MAINFRAME_ENV_SECRET_POSTGRES_URL".into());
        settings.validate().unwrap();
        let artifacts: Arc<dyn ArtifactStore> = store.clone();
        assert!(
            ProductServer::open_with_package_trust_and_artifact_store(
                settings,
                store,
                Arc::new(MemorySecretResolver::default()),
                default_program_router(),
                trust,
                artifacts
            )
            .is_err()
        );
    }
    refuse_stale_or_missing_selected_authority();
}
#[test]
fn genuine_capacity_failure_and_wrong_store_acknowledgement_never_publish_cache() {
    let store = Arc::new(MemoryStore::new(StoreLimits {
        max_provider_state: 0,
        ..Default::default()
    }));
    let write = ProviderStateWrite {
        record: ProviderStateRecord {
            namespace: "control".into(),
            key: "one".into(),
            version: 1,
            payload: vec![1],
        },
        expected_version: None,
    };
    assert!(matches!(
        ProviderCompositionCommit::publish(store.clone(), vec![write]),
        Err(StoreError::CapacityExceeded)
    ));
    assert_eq!(store.get_provider_state("control", "one").unwrap(), None);
    let actual = Arc::new(MemoryStore::new(Default::default()));
    let other = Arc::new(MemoryStore::new(Default::default()));
    let dataset = DatasetService::open(actual.clone(), Default::default()).unwrap();
    let seed = DatasetSeedObject {
        source_id: "data/one".into(),
        dataset: DatasetName::new("LOCAL.SEED", 128).unwrap(),
        attributes: DatasetAttributes {
            organization: DatasetOrganization::Sequential,
            record_format: RecordFormat::Fixed,
            logical_record_length: 4,
            key_offset: None,
            key_length: None,
            ccsid: Some(37),
        },
        record_length: 4,
        sha256: format!("sha256:{:x}", Sha256::digest(&RAW_ONE[..4])),
        bytes: RAW_ONE[..4].to_vec(),
    };
    let prepared = dataset
        .prepare_seed_selection("LOCAL", "1", vec![seed], true)
        .unwrap();
    let wrong = ProviderCompositionCommit::publish(other, prepared.writes().to_vec()).unwrap();
    assert_eq!(prepared.finish(&wrong), Err(HostProblem::UnknownOutcome));
    assert_eq!(
        actual.get_provider_state("dataset", "LOCAL.SEED").unwrap(),
        None
    );
    refuse_historical_overlapping_exact_retry();
}
#[test]
fn sqlite_selection_upgrade_rollback_and_reopen_use_one_real_store() {
    static SERIAL: AtomicU64 = AtomicU64::new(1);
    let root = std::env::temp_dir().join(format!(
        "local-composition-{}-{}",
        std::process::id(),
        SERIAL.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::create_dir(&root).unwrap();
    let url = format!("sqlite://{}?mode=rwc", root.join("state.sqlite").display());
    let trust = Arc::new(test_package_trust());
    let store = Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap());
    let server = fixture(store.clone(), trust.clone());
    let foreign = foreign_definitions(&server, &trust);
    let first = install(&server, &trust, 1);
    install(&server, &trust, 2);
    let (_, definition) = definitions(&server, &trust, 1);
    server
        .rollback_local_application(&first.generation, definition)
        .unwrap();
    drop((server, store));
    let reopened_store = Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap());
    let reopened = fixture(reopened_store.clone(), trust.clone());
    assert_eq!(
        reopened
            .selected_local_application("LOCAL-COMPOSE")
            .unwrap(),
        Some(first.generation.clone())
    );
    assert_eq!(
        seed_records(&reopened),
        RAW_ONE
            .as_chunks::<4>()
            .0
            .iter()
            .map(|chunk| chunk.to_vec())
            .collect::<Vec<_>>()
    );
    refuse_foreign_selection_after_runtime_truncate(
        &reopened,
        reopened_store.as_ref(),
        &trust,
        &first,
        foreign,
    );
    let before = selected_rows(reopened_store.as_ref());
    drop((reopened, reopened_store));
    let final_store = Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap());
    let final_server = fixture(final_store.clone(), trust);
    assert_eq!(
        final_server
            .selected_local_application("LOCAL-COMPOSE")
            .unwrap(),
        Some(first.generation)
    );
    assert_eq!(selected_rows(final_store.as_ref()), before);
    assert!(seed_records(&final_server).is_empty());
    drop((final_server, final_store));
    std::fs::remove_dir_all(root).unwrap();
}

#[path = "local_racf_composition_tests.rs"]
mod racf;

#[path = "local_db2_composition_tests.rs"]
mod db2;

#[path = "local_ims_composition_tests.rs"]
mod ims;

#[path = "local_mq_composition_tests.rs"]
mod mq;

#[path = "local_batch_composition_tests.rs"]
mod batch;

#[path = "local_complete_publication_tests.rs"]
mod complete_publication;
