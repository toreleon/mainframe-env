//! Full installed local catalog projection; no CD010 or application tokens.
//! Missing macro/unnamed-field/CSD DDNAME semantics remain explicit below.
use super::*;
use mainframe_env_cics::{
    CicsFileDefinition, CicsLimits, CicsTransientDataQueueDefinition, CicsTransientDataQueueKind,
    CicsTransientDataQueueOpen,
};
use mainframe_env_store::LocalArtifactStore;
use mainframe_env_store_api::{
    ArtifactRecord, ArtifactStore, ExecutableArtifactMetadata, ProviderStateRecord,
    ProviderStateStore,
};
use std::io::{Read, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::sync::Mutex;

const ORACLE: &str = include_str!("full_catalog_expected.json");
const ORACLE_SHA: &str = "19d891927d573a6be9926c22cb114e612da6f0a6a923524cfe6d11b236f5b49f";
const CATALOGS: &[(&str, usize)] = &[
    ("cics-map", 21),
    ("cics-file-alias", 8),
    ("cics-program", 27),
    ("online-program", 27),
    ("online-transaction", 25),
    ("cics-tdq-definition", 1),
];
const SEED_NAMESPACES: &[(&str, usize)] = &[
    ("dataset", 12),
    ("dataset-aix", 3),
    ("dataset-seed-generation", 1),
    ("dataset-seed-selection", 1),
];

fn problem(message: impl Into<String>) -> CorpusProblem {
    CorpusProblem::new("carddemo.resources.installed_projection", message)
}
fn refuse(failed: bool, message: &str) -> Result<(), CorpusProblem> {
    if failed {
        Err(problem(message))
    } else {
        Ok(())
    }
}
#[derive(Deserialize)]
struct Bytes {
    key: String,
    payload_base64: String,
}
#[derive(Deserialize)]
struct File {
    key: String,
    dataset: String,
    payload_base64: String,
}
#[derive(Deserialize)]
struct Oracle {
    schema_version: String,
    corpus_commit: String,
    source_pins: BTreeMap<String, String>,
    source_programs: BTreeMap<String, String>,
    csd_programs: Vec<String>,
    compiled_programs: Vec<String>,
    transactions: BTreeMap<String, String>,
    maps: Vec<Bytes>,
    files: Vec<File>,
    tdqueues: Vec<Bytes>,
    local_source_pins: BTreeMap<String, String>,
    correction_source: String,
    parsed_source_fields: usize,
    represented_named_fields: usize,
    qualification: Vec<String>,
}
fn oracle() -> Result<Oracle, CorpusProblem> {
    refuse(
        format!("{:x}", Sha256::digest(ORACLE.as_bytes())) != ORACLE_SHA,
        "frozen full catalog projection differs",
    )?;
    let oracle: Oracle = serde_json::from_str(ORACLE).map_err(package_problem)?;
    refuse(
        oracle.schema_version != "private.carddemo.full-installed-catalog-projection@1"
            || oracle.corpus_commit != "59cc6c2fd7ebd7ef7925cad552a01a4b8b6e4d5e"
            || oracle.source_pins.len() != 133
            || oracle.csd_programs.len() != 26
            || oracle.compiled_programs.len() != 27
            || oracle.source_programs.len() != 26
            || oracle.transactions.len() != 25
            || oracle.maps.len() != 21
            || oracle.files.len() != 8
            || oracle.tdqueues.len() != 1
            || oracle.local_source_pins.len() != 2
            || oracle.parsed_source_fields != 1166
            || oracle.represented_named_fields != 585,
        "frozen full source/projection shape differs",
    )?;
    Ok(oracle)
}
fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}
fn pinned(corpus: &Path, oracle: &Oracle) -> Result<(), CorpusProblem> {
    for (path, digest) in &oracle.source_pins {
        validate_relative_path(path, "full resource input")?;
        let raw = read_corpus_file(corpus, &corpus.join(path))?;
        refuse(
            format!("{:x}", Sha256::digest(raw)) != *digest,
            "full catalog first-party input differs",
        )?;
    }
    for (path, digest) in &oracle.local_source_pins {
        validate_relative_path(path, "approved local correction input")?;
        let raw = fs::read(repository_root().join(path)).map_err(package_problem)?;
        refuse(
            format!("{:x}", Sha256::digest(raw)) != *digest,
            "approved owned correction source/contract differs",
        )?;
    }
    Ok(())
}
fn decoded(raw: &str) -> Result<Vec<u8>, CorpusProblem> {
    base64::engine::general_purpose::STANDARD
        .decode(raw)
        .map_err(package_problem)
}
fn record(namespace: &str, key: &str, payload: Vec<u8>) -> ProviderStateRecord {
    ProviderStateRecord {
        namespace: namespace.into(),
        key: key.into(),
        version: 1,
        payload,
    }
}
fn put_expected(
    rows: &mut BTreeMap<String, ProviderStateRecord>,
    row: ProviderStateRecord,
) -> Result<(), CorpusProblem> {
    refuse(
        rows.insert(row.key.clone(), row).is_some(),
        "duplicate offered catalog key",
    )
}
#[derive(Clone, Debug, Eq, PartialEq)]
struct CatalogSnapshot {
    states: BTreeMap<String, BTreeMap<String, ProviderStateRecord>>,
    artifacts: BTreeMap<String, ArtifactRecord>,
}
fn expected(
    oracle: &Oracle,
    definition: &OnlineApplicationDefinition,
) -> Result<CatalogSnapshot, CorpusProblem> {
    let mut states = CATALOGS
        .iter()
        .map(|(name, _)| ((*name).into(), BTreeMap::new()))
        .collect::<BTreeMap<String, _>>();
    let mut artifacts = BTreeMap::new();
    for program in &definition.programs {
        let artifact = source_artifact_binding::publisher_record(program)?;
        put_expected(
            states.get_mut("online-program").unwrap(),
            record(
                "online-program",
                &program.name,
                program.artifact.as_str().as_bytes().to_vec(),
            ),
        )?;
        put_expected(
            states.get_mut("cics-program").unwrap(),
            record("cics-program", &program.name, Vec::new()),
        )?;
        refuse(
            artifacts.insert(program.name.clone(), artifact).is_some(),
            "duplicate full source publisher name",
        )?;
    }
    for (transaction, program) in &oracle.transactions {
        put_expected(
            states.get_mut("online-transaction").unwrap(),
            record(
                "online-transaction",
                transaction,
                program.as_bytes().to_vec(),
            ),
        )?;
    }
    for map in &oracle.maps {
        put_expected(
            states.get_mut("cics-map").unwrap(),
            record("cics-map", &map.key, decoded(&map.payload_base64)?),
        )?;
    }
    for file in &oracle.files {
        put_expected(
            states.get_mut("cics-file-alias").unwrap(),
            record("cics-file-alias", &file.key, decoded(&file.payload_base64)?),
        )?;
    }
    for queue in &oracle.tdqueues {
        put_expected(
            states.get_mut("cics-tdq-definition").unwrap(),
            record(
                "cics-tdq-definition",
                &queue.key,
                decoded(&queue.payload_base64)?,
            ),
        )?;
    }
    for (name, count) in CATALOGS {
        refuse(
            states[*name].len() != *count,
            "offered complete catalog key set differs",
        )?;
    }
    refuse(
        definition.transactions != oracle.transactions
            || artifacts.keys().cloned().collect::<Vec<_>>() != oracle.compiled_programs,
        "offered full compiled name/route mapping differs",
    )?;
    Ok(CatalogSnapshot { states, artifacts })
}
fn read_rows(
    store: &dyn ProviderStateStore,
    namespace: &str,
    count: usize,
) -> Result<BTreeMap<String, ProviderStateRecord>, CorpusProblem> {
    let rows = store
        .list_provider_state(namespace, count + 1)
        .map_err(package_problem)?;
    refuse(
        rows.len() != count,
        "complete actual catalog inventory is missing or has extra rows",
    )?;
    let mut complete = BTreeMap::new();
    for row in rows {
        refuse(
            row.namespace != namespace || row.version != 1,
            "actual namespace/version differs",
        )?;
        put_expected(&mut complete, row)?;
    }
    Ok(complete)
}
fn compare(expected: &CatalogSnapshot, actual: &CatalogSnapshot) -> Result<(), CorpusProblem> {
    refuse(
        expected != actual,
        "complete installed catalog rows, references, artifacts or metadata differ",
    )
}
fn capture(
    owner: &Owner,
    store: &dyn ProviderStateStore,
    artifact_root: &Path,
    expected: &CatalogSnapshot,
) -> Result<CatalogSnapshot, CorpusProblem> {
    owner.verify()?;
    let states = CATALOGS
        .iter()
        .map(|(namespace, count)| Ok(((*namespace).into(), read_rows(store, namespace, *count)?)))
        .collect::<Result<BTreeMap<_, _>, CorpusProblem>>()?;
    let artifact_store =
        LocalArtifactStore::open(artifact_root, 64 * 1024 * 1024).map_err(package_problem)?;
    let mut artifacts = BTreeMap::new();
    for (name, offered) in &expected.artifacts {
        let actual = artifact_store
            .get_artifact(&offered.artifact)
            .map_err(package_problem)?
            .ok_or_else(|| problem("actual installed full artifact is absent"))?;
        artifacts.insert(name.clone(), actual);
    }
    owner.verify()?;
    let actual = CatalogSnapshot { states, artifacts };
    compare(expected, &actual)?;
    Ok(actual)
}
fn seed_state(
    store: &dyn ProviderStateStore,
) -> Result<BTreeMap<String, Vec<ProviderStateRecord>>, CorpusProblem> {
    SEED_NAMESPACES
        .iter()
        .map(|(namespace, count)| {
            let rows = store
                .list_provider_state(namespace, count + 1)
                .map_err(package_problem)?;
            refuse(
                rows.len() != *count,
                "actual seed namespace is incomplete or has extra rows",
            )?;
            Ok(((*namespace).into(), rows))
        })
        .collect()
}
fn retain_executable_metadata(metadata: &ExecutableArtifactMetadata) -> serde_json::Value {
    // Exhaustive destructuring makes any future storage field a compile-time capture obligation.
    let ExecutableArtifactMetadata {
        artifact_contract,
        compatibility_profile,
        compiler_generation,
        target,
        options,
        host_interfaces,
        ir_contract,
        dialect_contracts,
        semantic_identity,
        manifest_payload_digest,
    } = metadata;
    serde_json::json!({
        "artifact_contract": artifact_contract,
        "compatibility_profile": compatibility_profile,
        "compiler_generation": compiler_generation,
        "target": target,
        "options": options,
        "host_interfaces": host_interfaces,
        "ir_contract": ir_contract,
        "dialect_contracts": dialect_contracts,
        "semantic_identity": semantic_identity,
        "manifest_payload_digest": manifest_payload_digest,
    })
}
fn retain_snapshot(actual: &CatalogSnapshot) -> serde_json::Value {
    // Catalog rows contain map labels and binding names; no raw customer records.
    serde_json::json!({"catalogs":actual.states.iter().map(|(namespace,rows)|serde_json::json!({"namespace":namespace,"rows":rows.values().map(|row|serde_json::json!({"key":row.key,"version":row.version,"payload_base64":base64::engine::general_purpose::STANDARD.encode(&row.payload)})).collect::<Vec<_>>()})).collect::<Vec<_>>(),
        "artifacts":actual.artifacts.iter().map(|(name,row)|serde_json::json!({"name":name,"reference":row.artifact.as_str(),"bytes":row.payload.len(),"sha256":format!("{:x}",Sha256::digest(&row.payload)),"executable_metadata":row.executable.as_ref().map(retain_executable_metadata)})).collect::<Vec<_>>()})
}
fn compile(
    name: &str,
    primary: &str,
    bundle: &SourceBundle,
) -> Result<(OnlineProgramDefinition, SemanticModel), CorpusProblem> {
    let compiler = CobolCompiler::default();
    let analysis = compiler.analyze(bundle);
    let semantic = analysis
        .semantic
        .ok_or_else(|| problem(format!("{primary}: semantic model absent")))?;
    refuse(
        semantic.program_id != name,
        "full source compiler PROGRAM-ID differs",
    )?;
    let result = compiler
        .compile(CompilerRequest {
            source: bundle.clone(),
            mode: CompilationMode::Executable,
            target: CompileTarget::new("reference").map_err(package_problem)?,
            options: CompileOptions::new(BTreeMap::new()).map_err(package_problem)?,
        })
        .map_err(package_problem)?;
    let CompilerResult::Published { artifact, .. } = result else {
        return Err(problem(format!(
            "{primary}: actual executable publication failed"
        )));
    };
    Ok((OnlineProgramDefinition::current(name, &artifact), semantic))
}
fn definition(
    corpus: &Path,
    oracle: &Oracle,
) -> Result<OnlineApplicationDefinition, CorpusProblem> {
    let bundles = explicit_carddemo_bundles(corpus)?
        .into_iter()
        .collect::<BTreeMap<_, _>>();
    let mut programs = Vec::new();
    let mut models = Vec::new();
    for (name, primary) in &oracle.source_programs {
        let bundle = bundles
            .get(primary)
            .ok_or_else(|| problem("declared first-party compiler input absent"))?;
        let (program, semantic) = compile(name, primary, bundle)?;
        programs.push(program);
        models.push(semantic);
    }
    let raw =
        fs::read(repository_root().join(&oracle.correction_source)).map_err(package_problem)?;
    let limits = SourceLimits::default();
    let logical = LogicalPath::new(&oracle.correction_source, limits.max_path_bytes)
        .map_err(package_problem)?;
    let file = SourceFile::input(
        &oracle.correction_source,
        raw,
        SourceFormat::Free,
        SourceEncoding::Utf8,
        limits,
    )
    .map_err(package_problem)?;
    let bundle = SourceBundle::new(&logical, vec![file], BTreeMap::new(), Vec::new(), limits)
        .map_err(package_problem)?;
    let (program, semantic) = compile("COCRDSEC", &oracle.correction_source, &bundle)?;
    programs.push(program);
    models.push(semantic);
    programs.sort_by(|a, b| a.name.cmp(&b.name));
    let maps = carddemo_maps(
        corpus,
        &[
            "app/bms",
            "app/app-transaction-type-db2/bms",
            "app/app-authorization-ims-db2-mq/bms",
        ],
        &models,
    )?;
    refuse(maps.len() != 21, "full represented map inventory differs")?;
    Ok(OnlineApplicationDefinition {
        programs,
        transactions: oracle.transactions.clone(),
        maps,
    })
}
fn install_resources(
    owner: &Owner,
    server: &ProductServer,
    corpus: &Path,
    oracle: &Oracle,
) -> Result<(), CorpusProblem> {
    owner.verify()?;
    server
        .dataset_service()
        .install_seed_generation(
            "CARDDEMO",
            "catalog-g1",
            carddemo_base_seed_objects(corpus)?,
        )
        .map_err(terminal_problem)?;
    for (sequence, (base, index, offset, length)) in [
        ("CARDDATA", "CARDDATA", 16, 11),
        ("CARDXREF", "CARDXREF", 25, 11),
        ("TRANSACT", "TRANSACT", 304, 26),
    ]
    .into_iter()
    .enumerate()
    {
        owner.verify()?;
        server
            .dataset_service()
            .invoke(DatasetRequest::DefineAlternateIndex {
                base: DatasetName::new(format!("AWS.M2.CARDDEMO.{base}.VSAM.KSDS"), 128)
                    .map_err(package_problem)?,
                index: DatasetName::new(format!("AWS.M2.CARDDEMO.{index}.VSAM.AIX.PATH"), 128)
                    .map_err(package_problem)?,
                key_offset: offset,
                key_length: length,
                allow_duplicates: true,
                upgrade: true,
                mutation: Mutation {
                    sequence: sequence as u64 + 1,
                    idempotency_key: IdempotencyKey::new(
                        format!("full-catalog-aix-{sequence}"),
                        InvocationLimits::default(),
                    )
                    .map_err(package_problem)?,
                    transaction: Some("CATALOG-INSTALL".into()),
                },
            })
            .map_err(terminal_problem)?;
    }
    let files = oracle
        .files
        .iter()
        .map(|file| {
            Ok((
                file.key.clone(),
                CicsFileDefinition {
                    dataset: DatasetName::new(&file.dataset, 128).map_err(package_problem)?,
                    ccsid: Some(37),
                },
            ))
        })
        .collect::<Result<BTreeMap<_, _>, CorpusProblem>>()?;
    owner.verify()?;
    server
        .cics_service()
        .register_file_definitions(&files)
        .map_err(terminal_problem)?;
    let limits = CicsLimits::default();
    refuse(
        limits.max_queue_records != 65536 || limits.max_queue_bytes != 64 * 1024 * 1024,
        "existing local TDQUEUE defaults differ",
    )?;
    owner.verify()?;
    server
        .cics_service()
        .register_transient_data_queues(&[CicsTransientDataQueueDefinition {
            name: "JOBS".into(),
            kind: CicsTransientDataQueueKind::Extrapartition,
            enabled: true,
            open: Some(CicsTransientDataQueueOpen::Output),
            record_size: Some(80),
            max_records: limits.max_queue_records,
            max_bytes: limits.max_queue_bytes,
        }])
        .map_err(terminal_problem)
}
async fn run(
    corpus: &Path,
    inventory: &Path,
    evidence: PathBuf,
) -> Result<serde_json::Value, CorpusProblem> {
    verify_carddemo_corpus(corpus, inventory)?;
    let oracle = oracle()?;
    pinned(corpus, &oracle)?;
    let definition = definition(corpus, &oracle)?;
    pinned(corpus, &oracle)?;
    let expected = expected(&oracle, &definition)?;
    let owner = Owner::claim(evidence)?;
    let artifact_root = owner.owned.join("artifacts");
    let url = format!(
        "sqlite:{}?mode=rwc",
        owner.owned.join("state.sqlite").display()
    );
    let config = ServerConfig {
        store_profile: StoreProfile::Sqlite,
        sqlite_url: url.clone(),
        artifact_root: artifact_root.clone(),
        tls: TlsConfig {
            enabled: false,
            certificate_path: None,
            private_key_reference: None,
        },
        ..ServerConfig::default()
    };
    let mut report = serde_json::json!({"schema":"private.carddemo.full-installed-catalog-projection@1","status":"running","snapshots":[],"shutdowns":[],"cleanup":"pending","issue_observations":[],"journey_observations":[],"application_credit":0,"qualification":oracle.qualification,"oracle_sha256":ORACLE_SHA});
    owner.save(&report)?;
    owner.verify()?;
    let store =
        Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262144).map_err(package_problem)?);
    let server = ProductServer::open(
        config.clone(),
        store.clone(),
        Arc::new(MemorySecretResolver::default()),
        default_program_router(),
    )
    .map_err(terminal_problem)?;
    let first = (|| {
        install_resources(&owner, &server, corpus, &oracle)?;
        owner.verify()?;
        let installed = server
            .install_online_application(definition.clone())
            .map_err(terminal_problem)?;
        refuse(
            installed.replayed
                || installed.programs != 27
                || installed.transactions != 25
                || installed.maps != 21,
            "full actual fresh install differs",
        )?;
        let initial = capture(&owner, store.as_ref(), &artifact_root, &expected)?;
        let _complete_seed = seed_observations::capture(&server, corpus)?;
        let seed = seed_state(store.as_ref())?;
        report["snapshots"]
            .as_array_mut()
            .unwrap()
            .push(retain_snapshot(&initial));
        owner.save(&report)?;
        owner.verify()?;
        let reinstalled = server
            .install_online_application(definition)
            .map_err(terminal_problem)?;
        refuse(
            !reinstalled.replayed || installed.identity != reinstalled.identity,
            "full actual reinstall failed identity/replay equality",
        )?;
        let after = capture(&owner, store.as_ref(), &artifact_root, &expected)?;
        refuse(
            initial != after || seed != seed_state(store.as_ref())?,
            "complete reinstall mutated durable catalogs/artifacts/seeds",
        )?;
        report["snapshots"]
            .as_array_mut()
            .unwrap()
            .push(retain_snapshot(&after));
        owner.save(&report)?;
        Ok::<_, CorpusProblem>((initial, seed))
    })();
    let stopped = server.graceful_shutdown().await;
    report["shutdowns"] = serde_json::json!([stopped]);
    owner.save(&report)?;
    drop(server);
    drop(store);
    let result = match first {
        Ok((initial, seed)) if stopped => {
            owner.verify()?;
            pinned(corpus, &oracle)?;
            let store = Arc::new(
                SqliteStateStore::open(&url, 64 * 1024 * 1024, 262144).map_err(package_problem)?,
            );
            let server = ProductServer::open(
                config,
                store.clone(),
                Arc::new(MemorySecretResolver::default()),
                default_program_router(),
            )
            .map_err(terminal_problem)?;
            let result = (|| {
                let reopened = capture(&owner, store.as_ref(), &artifact_root, &expected)?;
                let _complete_seed = seed_observations::capture(&server, corpus)?;
                refuse(
                    initial != reopened || seed != seed_state(store.as_ref())?,
                    "fresh SQLite/provider catalog or full seed state differs",
                )?;
                report["snapshots"]
                    .as_array_mut()
                    .unwrap()
                    .push(retain_snapshot(&reopened));
                Ok::<_, CorpusProblem>(())
            })();
            let stopped = server.graceful_shutdown().await;
            report["shutdowns"]
                .as_array_mut()
                .unwrap()
                .push(serde_json::json!(stopped));
            owner.save(&report)?;
            drop(server);
            drop(store);
            result.and_then(|_| refuse(!stopped, "fresh reopened server drain uncertain"))
        }
        Err(error) => Err(error),
        Ok(_) => Err(problem("initial server drain uncertain")),
    };
    if let Err(error) = result {
        report["status"] = serde_json::json!("fail-owned-state-retained");
        report["failure"] = serde_json::json!(error.to_string());
        owner.save(&report)?;
        return Err(error);
    }
    verify_carddemo_corpus(corpus, inventory)?;
    pinned(corpus, &oracle)?;
    refuse(
        report["snapshots"].as_array().map(Vec::len) != Some(3)
            || report["shutdowns"] != serde_json::json!([true, true]),
        "full actual comparisons/drains incomplete",
    )?;
    owner.cleanup(&mut report)?;
    // Return a private diagnostic comparison only; all CD010 requirements remain
    // pending until omitted source/installation semantics are independently proved.
    Ok(report)
}

// Guarded receipt/owned-root helpers below are retained as a separate private
// component, with the same non-atomic filesystem observation qualification.

#[derive(Clone)]
struct RetainedReceipt {
    identity: (u64, u64),
    bytes: usize,
    sha256: String,
}
struct Owner {
    evidence: PathBuf,
    evidence_id: (u64, u64),
    owned: PathBuf,
    owned_id: (u64, u64),
    receipt: Mutex<Option<RetainedReceipt>>,
}
fn receipt_destination(
    path: &Path,
    expected: Option<&RetainedReceipt>,
) -> Result<(), CorpusProblem> {
    let before = fs::symlink_metadata(path);
    let Some(expected) = expected else {
        return match before {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            _ => Err(problem(
                "first retained seed receipt destination is unknown or unobservable",
            )),
        };
    };
    let before = before.map_err(package_problem)?;
    refuse(
        !before.is_file()
            || (before.dev(), before.ino()) != expected.identity
            || before.len() != expected.bytes as u64,
        "prior retained seed receipt identity differs",
    )?;
    let file = fs::File::open(path).map_err(package_problem)?;
    let opened = file.metadata().map_err(package_problem)?;
    refuse(
        !opened.is_file()
            || (opened.dev(), opened.ino()) != expected.identity
            || opened.len() != expected.bytes as u64,
        "opened prior seed receipt identity differs",
    )?;
    let mut raw = Vec::new();
    file.take(expected.bytes as u64 + 1)
        .read_to_end(&mut raw)
        .map_err(package_problem)?;
    let after = fs::symlink_metadata(path).map_err(package_problem)?;
    refuse(
        !after.is_file()
            || (after.dev(), after.ino()) != expected.identity
            || after.len() != expected.bytes as u64
            || raw.len() != expected.bytes
            || format!("{:x}", Sha256::digest(raw)) != expected.sha256,
        "prior retained seed receipt bytes or identity differ",
    )
}
fn directory_id(path: &Path) -> Result<(u64, u64), CorpusProblem> {
    let metadata = fs::symlink_metadata(path).map_err(package_problem)?;
    refuse(
        !metadata.is_dir(),
        "owned seed namespace is not a plain directory",
    )?;
    Ok((metadata.dev(), metadata.ino()))
}
fn absence(observed: std::io::Result<fs::Metadata>) -> Result<(), CorpusProblem> {
    match observed {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(problem("owned seed state absence observation failed")),
        Ok(_) => Err(problem("owned seed state remains after disposal")),
    }
}
impl Owner {
    fn claim(evidence: PathBuf) -> Result<Self, CorpusProblem> {
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&evidence)
            .map_err(package_problem)?;
        let evidence_id = directory_id(&evidence)?;
        let owned = evidence.join("owned-state");
        refuse(
            directory_id(&evidence)? != evidence_id,
            "owned seed evidence identity changed",
        )?;
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&owned)
            .map_err(package_problem)?;
        let owned_id = directory_id(&owned)?;
        Ok(Self {
            evidence,
            evidence_id,
            owned,
            owned_id,
            receipt: Mutex::new(None),
        })
    }
    fn verify(&self) -> Result<(), CorpusProblem> {
        refuse(
            directory_id(&self.evidence)? != self.evidence_id
                || directory_id(&self.owned)? != self.owned_id,
            "owned seed namespace identity changed",
        )
    }
    fn save(&self, report: &serde_json::Value) -> Result<(), CorpusProblem> {
        self.save_observed(report, || {}, || {})
    }
    fn save_observed(
        &self,
        report: &serde_json::Value,
        before_publication: impl FnOnce(),
        after_publication: impl FnOnce(),
    ) -> Result<(), CorpusProblem> {
        let mut retained = self.receipt.lock().map_err(package_problem)?;
        refuse(
            directory_id(&self.evidence)? != self.evidence_id,
            "owned seed evidence identity changed",
        )?;
        let destination = self.evidence.join("full-catalog.json");
        receipt_destination(&destination, retained.as_ref())?;
        let raw = serde_json::to_vec_pretty(report).map_err(package_problem)?;
        refuse(
            raw.len() > 64 * 1024 * 1024,
            "retained seed evidence exceeds original output ceiling",
        )?;
        let temporary = self.evidence.join("full-catalog.json.next");
        let mut stream = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)
            .map_err(package_problem)?;
        let opened = stream.metadata().map_err(package_problem)?;
        refuse(
            !opened.is_file(),
            "opened retained seed staging file is not plain",
        )?;
        let identity = (opened.dev(), opened.ino());
        stream.write_all(&raw).map_err(package_problem)?;
        stream.sync_all().map_err(package_problem)?;
        before_publication();
        refuse(
            directory_id(&self.evidence)? != self.evidence_id,
            "owned seed evidence identity changed before receipt publication",
        )?;
        let written = stream.metadata().map_err(package_problem)?;
        let staged = fs::symlink_metadata(&temporary).map_err(package_problem)?;
        refuse(
            !written.is_file()
                || !staged.is_file()
                || (written.dev(), written.ino()) != identity
                || (staged.dev(), staged.ino()) != identity
                || written.len() != raw.len() as u64
                || staged.len() != raw.len() as u64,
            "retained seed staging identity differs after sync",
        )?;
        let published = RetainedReceipt {
            identity,
            bytes: raw.len(),
            sha256: format!("{:x}", Sha256::digest(&raw)),
        };
        receipt_destination(&temporary, Some(&published))?;
        receipt_destination(&destination, retained.as_ref())?;
        refuse(
            directory_id(&self.evidence)? != self.evidence_id,
            "owned seed evidence identity changed at receipt publication",
        )?;
        fs::rename(&temporary, &destination).map_err(package_problem)?;
        after_publication();
        refuse(
            directory_id(&self.evidence)? != self.evidence_id,
            "owned seed evidence identity changed after receipt publication",
        )?;
        receipt_destination(&destination, Some(&published))?;
        refuse(
            directory_id(&self.evidence)? != self.evidence_id,
            "owned seed evidence identity changed after final receipt observation",
        )?;
        *retained = Some(published);
        Ok(())
    }
    fn cleanup(&self, report: &mut serde_json::Value) -> Result<(), CorpusProblem> {
        self.verify()?;
        refuse(
            report["snapshots"].as_array().map(Vec::len) != Some(3)
                || report["shutdowns"] != serde_json::json!([true, true]),
            "actual full catalog comparisons or server closure incomplete",
        )?;
        report["cleanup"] = serde_json::json!("verified-owned-removal-pending");
        self.save(report)?;
        self.verify()?;
        fs::remove_dir_all(&self.owned).map_err(package_problem)?;
        absence(fs::symlink_metadata(&self.owned))?;
        report["cleanup"] = serde_json::json!("removed-and-absence-verified");
        report["status"] = serde_json::json!("pass");
        self.save(report)
    }
}

#[cfg(test)]
#[path = "full_catalog_observations_tests.rs"]
mod tests;
