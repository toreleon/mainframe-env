//! Exact retained/published V3 package state; no whole CD-009/application token.
//! Original cohort/controller markers and local erased SQL types stay qualified.
use super::*;
use mainframe_env_application::{
    ApplicationGenerationRecord, ApplicationInstallerV2, PackageLimits,
};
use mainframe_env_application::{
    ApplicationPublicationState, PublicationAction, PublicationSectionState,
};
use mainframe_env_batch::{
    BatchControllerDefinition, BatchControllerGeneration, BatchControllerPlan,
    BatchControllerProgram, BatchControllerSelector,
};
use mainframe_env_store::LocalArtifactStore;
use mainframe_env_store_api::{ArtifactRecord, ArtifactStore, PlatformStore};

#[path = "v3_retained_state.rs"]
mod v3_retained_state;
use v3_retained_state::{
    capture, controller_generation, controllers_match, db2_matches, db2_rows, failure_state,
    publication_matches, raw_rows, registry, reinstalled, same_providers, snapshot_json,
};

const INPUTS: &str = include_str!("v3_retained_expected.json");
const APP: &str = "AWS-CARDDEMO";
const PRIMARY: &str = "app/app-transaction-type-db2/cbl/COBTUPDT.cbl";
const MAIN_PHASES: &[&str] = &[
    "g1",
    "reinstall",
    "staged-g2",
    "g2",
    "rollback",
    "unselected-ready-refused",
    "admission-refusals",
    "reopen",
];
const FAILURE_PHASES: &[&str] = &["g1", "staged-malformed", "publication-refused", "reopen"];
const SCOPES: &[&str] = &[
    "memory-main",
    "memory-malformed",
    "sqlite-main",
    "sqlite-malformed",
];
const ROW_BOUNDS: &[(&str, usize)] = &[
    ("application-package-v2", 1),
    ("application-publication-v2", 1),
    ("batch-controller-state", 1),
    ("batch-program", 1),
    ("db2-state", 1),
    ("db2-v1-table", 3),
    ("db2-v1-schema", 3),
    ("db2-v1-installation", 1),
    ("db2-v1-catalog-generation", 2),
    ("db2-v1-table-provenance", 3),
    ("db2-v1-legacy-snapshot", 0),
    ("db2-v1-unit-of-work", 0),
    ("db2-v1-cursor", 0),
    ("db2-v1-cursor-declaration", 0),
    ("db2-v1-replay", 0),
    ("ims-v1-metadata-selection", 1),
    ("ims-v1-metadata-generation:AWS-CARDDEMO", 0),
    ("ims-tm-v1-catalog", 0),
    ("ims-tm-v1-package-definitions", 0),
];
#[derive(Clone, Deserialize)]
struct FileExpected {
    bytes: usize,
    sha256: String,
    #[serde(default)]
    format: String,
    #[serde(default)]
    encoding: String,
}
#[derive(Deserialize)]
struct LibraryExpected {
    name: String,
    members: Vec<String>,
}
#[derive(Deserialize)]
struct BundleExpected {
    primary: String,
    files: BTreeMap<String, FileExpected>,
    libraries: Vec<LibraryExpected>,
    options: BTreeMap<String, String>,
    provenance: Vec<serde_json::Value>,
}
#[derive(Deserialize)]
struct Marker {
    path: String,
    kind: EntryKind,
    bytes: usize,
    sha256: String,
}
#[derive(Deserialize)]
struct ProgramExpected {
    path: String,
    marker_sha256: Option<String>,
    marker_bytes: Option<usize>,
}
#[derive(Deserialize)]
struct Controlled {
    payload: String,
    bytes: usize,
    sha256: String,
}
#[derive(Deserialize)]
struct ControllerTemplate {
    name: String,
    program_path: String,
    selector: BatchControllerSelector,
    plan: BatchControllerPlan,
}
#[derive(Deserialize)]
struct Inputs {
    schema: String,
    corpus_commit: String,
    source_pins: BTreeMap<String, FileExpected>,
    bundle: BundleExpected,
    package_name: String,
    target_product: String,
    g1_version: String,
    g2_version: String,
    markers: Vec<Marker>,
    programs: Vec<ProgramExpected>,
    controllers: Vec<BatchController>,
    controller_templates: Vec<ControllerTemplate>,
    local_tables: Vec<Db2TableDefinition>,
    schema_tables: Vec<SqlTable>,
    catalog_blob: Marker,
    g2_profile: Controlled,
    malformed_catalog: Controlled,
}
fn inputs() -> Result<Inputs, CorpusProblem> {
    refuse(
        format!("{:x}", Sha256::digest(INPUTS.as_bytes()))
            != "50e4c69c37a4ab7363dfa9fc8b0e7efc6ba21c516eac1aac2405e141fc3bfafd",
        "retained V3 complete frozen input bytes differ",
    )?;
    let value: Inputs = serde_json::from_str(INPUTS).map_err(package_problem)?;
    refuse(
        value.schema != "private.carddemo.v3-retained-inputs@1"
            || value.corpus_commit != "59cc6c2fd7ebd7ef7925cad552a01a4b8b6e4d5e"
            || value.source_pins.len() != 204
            || value.package_name != APP
            || value.target_product != "0.2.0"
            || value.g1_version != "0.2.0"
            || value.g2_version != "0.2.1"
            || value.markers.len() != 4
            || value.programs.len() != 5
            || value.controllers.len() != 5
            || value.controller_templates.len() != 5
            || value.local_tables.len() != 3,
        "retained V3 frozen input envelope differs",
    )?;
    Ok(value)
}
fn source_inputs(corpus: &Path, expected: &Inputs) -> Result<(), CorpusProblem> {
    for (path, pin) in &expected.source_pins {
        let bytes = read_corpus_file(corpus, &corpus.join(path))?;
        refuse(
            bytes.len() != pin.bytes || format!("{:x}", Sha256::digest(bytes)) != pin.sha256,
            "retained V3 exact first-party source differs",
        )?;
    }
    Ok(())
}
fn bundle_matches(bundle: &SourceBundle, expected: &BundleExpected) -> bool {
    if expected.primary != PRIMARY
        || bundle
            .file(bundle.primary())
            .map(|file| file.path().as_str())
            != Some(PRIMARY)
        || bundle.options() != &expected.options
        || !expected.provenance.is_empty()
        || !bundle.provenance().is_empty()
        || bundle.files().len() != 74
        || bundle.libraries().len() != 10
        || expected.files.len() != 74
        || expected.libraries.len() != 10
    {
        return false;
    }
    bundle
        .files()
        .iter()
        .map(|file| file.path().as_str())
        .eq(expected.files.keys().map(String::as_str))
        && bundle.files().iter().all(|file| {
            expected.files.get(file.path().as_str()).is_some_and(|pin| {
                let format = match file.format() {
                    SourceFormat::Fixed => "fixed",
                    SourceFormat::Free => "free",
                    SourceFormat::Variable => "variable",
                };
                pin.format == format
                    && pin.encoding == "utf8"
                    && file.encoding() == SourceEncoding::Utf8
                    && pin.bytes == file.bytes().len()
                    && pin.sha256 == format!("{:x}", Sha256::digest(file.bytes()))
            })
        })
        && bundle
            .libraries()
            .iter()
            .zip(&expected.libraries)
            .all(|(actual, pin)| {
                actual.name() == pin.name
                    && actual
                        .members()
                        .iter()
                        .map(LogicalPath::as_str)
                        .eq(pin.members.iter().map(String::as_str))
            })
}
fn compiled(corpus: &Path, expected: &Inputs) -> Result<BatchProgramDefinition, CorpusProblem> {
    let bundles = explicit_carddemo_bundles(corpus)?;
    let (path, bundle) = bundles
        .into_iter()
        .find(|(path, _)| path == PRIMARY)
        .ok_or_else(|| drift("retained V3 actual primary bundle missing"))?;
    refuse(
        !bundle_matches(&bundle, &expected.bundle),
        "retained V3 full compiler input differs",
    )?;
    let selected = BTreeMap::from([("COBTUPDT".into(), (path, bundle))]);
    let mut rows =
        compile_carddemo_batch_definitions(&selected, &BTreeSet::from(["COBTUPDT".into()]))?;
    refuse(
        rows.len() != 1,
        "retained V3 actual compiler result count differs",
    )?;
    let value = rows
        .pop()
        .ok_or_else(|| drift("retained V3 compiled artifact missing"))?;
    refuse(
        value.name != "COBTUPDT",
        "retained V3 compiled program identity differs",
    )?;
    Ok(value)
}
fn publisher(definition: &BatchProgramDefinition) -> Result<ArtifactRecord, CorpusProblem> {
    source_artifact_binding::publisher_record(&OnlineProgramDefinition {
        name: definition.name.clone(),
        artifact: definition.artifact.clone(),
        payload: definition.payload.clone(),
        manifest: definition.manifest.clone(),
        semantic_identity: definition.semantic_identity.clone(),
    })
}
fn blob<'a>(package: &'a ApplicationPackageV2, path: &str) -> Result<&'a [u8], CorpusProblem> {
    let entry = package
        .base
        .manifest
        .entries
        .iter()
        .find(|entry| entry.path == path)
        .ok_or_else(|| drift("retained V3 exact entry missing"))?;
    package
        .base
        .blobs
        .get(&entry.sha256)
        .map(Vec::as_slice)
        .ok_or_else(|| drift("retained V3 entry blob missing"))
}
fn declaration_matches(
    package: &ApplicationPackageV2,
    expected: &Inputs,
    definition: &BatchProgramDefinition,
) -> bool {
    let paths = [
        "source/manifest",
        "resource/manifest",
        "data/db2/catalog",
        "profile/manifest",
        "migration/manifest",
        "program/COBTUPDT",
        "program/PAUDBLOD",
        "program/PAUDBUNL",
        "program/CBPAUP0C",
        "program/DFSURGU0",
    ];
    if package.generation != 1
        || package.base.manifest.name != APP
        || package.base.manifest.version != expected.g1_version
        || package.base.manifest.target_product != expected.target_product
        || package
            .base
            .manifest
            .entries
            .iter()
            .map(|entry| entry.path.as_str())
            .ne(paths)
        || package.base.blobs.len() != 10
        || package.signature.algorithm
            != mainframe_env_application::PACKAGE_AUTHENTICATION_ALGORITHM
        || package.signature.key_id != "carddemo-conformance-key"
        || package.sections.schema_version != APPLICATION_PACKAGE_V3_CONTRACT
        || package.sections.sql_tables != expected.schema_tables
        || package.sections.batch_controllers != expected.controllers
        || !package.sections.host_abi_libraries.is_empty()
        || !package.sections.sql_rows.is_empty()
        || !package.sections.ims_definitions.is_empty()
        || !package.sections.ims_rows.is_empty()
        || package.sections.ims_metadata.is_some()
        || package.sections.ims_tm.is_some()
        || !package.sections.mq_resources.is_empty()
        || !package.sections.security_resources.is_empty()
    {
        return false;
    }
    let entry_matches = |path: &str, kind: EntryKind, digest: &str, length: usize| {
        package
            .base
            .manifest
            .entries
            .iter()
            .find(|entry| entry.path == path)
            .is_some_and(|entry| {
                entry.kind == kind
                    && entry.sha256 == digest
                    && entry.bytes == length
                    && entry.depends_on
                        == if kind == EntryKind::Source {
                            Vec::new()
                        } else {
                            vec!["source/manifest".to_string()]
                        }
                    && package.base.blobs.get(digest).is_some_and(|bytes| {
                        bytes.len() == length
                            && format!("sha256:{:x}", Sha256::digest(bytes)) == digest
                    })
            })
    };
    expected
        .markers
        .iter()
        .all(|entry| entry_matches(&entry.path, entry.kind, &entry.sha256, entry.bytes))
        && entry_matches(
            &expected.catalog_blob.path,
            EntryKind::Data,
            &expected.catalog_blob.sha256,
            expected.catalog_blob.bytes,
        )
        && blob(package, "data/db2/catalog")
            .ok()
            .and_then(|raw| serde_json::from_slice::<Vec<Db2TableDefinition>>(raw).ok())
            .as_ref()
            == Some(&expected.local_tables)
        && expected.programs.iter().all(|entry| {
            if entry.path == "program/COBTUPDT" {
                entry_matches(
                    &entry.path,
                    EntryKind::Program,
                    &format!("sha256:{:x}", Sha256::digest(&definition.payload)),
                    definition.payload.len(),
                ) && blob(package, &entry.path).ok() == Some(definition.payload.as_slice())
            } else {
                entry
                    .marker_sha256
                    .as_ref()
                    .zip(entry.marker_bytes)
                    .is_some_and(|(digest, length)| {
                        entry_matches(&entry.path, EntryKind::Program, digest, length)
                    })
            }
        })
}
fn replace_blob(
    package: &mut ApplicationPackageV2,
    path: &str,
    bytes: Vec<u8>,
) -> Result<(), CorpusProblem> {
    let entry = package
        .base
        .manifest
        .entries
        .iter_mut()
        .find(|row| row.path == path)
        .ok_or_else(|| drift("controlled V3 entry missing"))?;
    let old = entry.sha256.clone();
    let digest = format!("sha256:{:x}", Sha256::digest(&bytes));
    entry.sha256 = digest.clone();
    entry.bytes = bytes.len();
    if !package
        .base
        .manifest
        .entries
        .iter()
        .any(|row| row.sha256 == old)
    {
        package.base.blobs.remove(&old);
    }
    package.base.blobs.insert(digest, bytes);
    Ok(())
}
fn resign(package: &mut ApplicationPackageV2) -> Result<(), CorpusProblem> {
    package.signature = ims_packages::sign_carddemo_package_identity(
        &package_generation_identity(package).map_err(package_problem)?,
    )?;
    Ok(())
}
fn controlled(
    base: &ApplicationPackageV2,
    expected: &Inputs,
    malformed: bool,
) -> Result<ApplicationPackageV2, CorpusProblem> {
    let mut package = base.clone();
    package.generation = 2;
    package.base.manifest.version = expected.g2_version.clone();
    let input = if malformed {
        &expected.malformed_catalog
    } else {
        &expected.g2_profile
    };
    refuse(
        input.payload.len() != input.bytes
            || format!("sha256:{:x}", Sha256::digest(input.payload.as_bytes())) != input.sha256,
        "independent controlled package bytes differ",
    )?;
    replace_blob(
        &mut package,
        if malformed {
            "data/db2/catalog"
        } else {
            "profile/manifest"
        },
        input.payload.as_bytes().to_vec(),
    )?;
    resign(&mut package)?;
    Ok(package)
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct ControllerState {
    schema_version: String,
    applications: Vec<ControllerApplication>,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct ControllerApplication {
    application: String,
    selected: u64,
    generations: Vec<BatchControllerGeneration>,
}
#[derive(Clone, Debug, Eq, PartialEq)]
struct Snapshot {
    rows: BTreeMap<String, Vec<ProviderStateRecord>>,
    selected: ApplicationGenerationRecord,
    packages: BTreeMap<u64, ApplicationPackageV2>,
    staged: BTreeSet<u64>,
    tables: Vec<(Db2TableDefinition, Vec<Vec<Vec<u8>>>)>,
    artifact: ArtifactRecord,
}
struct Backend {
    name: String,
    path: PathBuf,
    identity: (u64, u64),
    config: ServerConfig,
    store: Arc<dyn PlatformStore>,
}
impl Backend {
    fn create(owner: &Owner, name: &str, sqlite: bool) -> Result<Self, CorpusProblem> {
        owner.verify()?;
        refuse(!SCOPES.contains(&name), "V3 physical scope name differs")?;
        let path = owner.owned.join(name);
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&path)
            .map_err(package_problem)?;
        let identity = directory_id(&path)?;
        owner.verify()?;
        let url = format!("sqlite:{}?mode=rwc", path.join("state.sqlite").display());
        let config = ServerConfig {
            store_profile: if sqlite {
                StoreProfile::Sqlite
            } else {
                StoreProfile::Memory
            },
            sqlite_url: url.clone(),
            artifact_root: path.join("artifacts"),
            tls: TlsConfig {
                enabled: false,
                certificate_path: None,
                private_key_reference: None,
            },
            ..ServerConfig::default()
        };
        owner.verify()?;
        refuse(
            directory_id(&path)? != identity,
            "V3 backend physical namespace changed before store open",
        )?;
        let store: Arc<dyn PlatformStore> = if sqlite {
            Arc::new(
                SqliteStateStore::open(&url, 64 * 1024 * 1024, 262144).map_err(package_problem)?,
            )
        } else {
            Arc::new(MemoryStore::new(Default::default()))
        };
        let value = Self {
            name: name.into(),
            path,
            identity,
            config,
            store,
        };
        value.verify(owner)?;
        Ok(value)
    }
    fn verify(&self, owner: &Owner) -> Result<(), CorpusProblem> {
        owner.verify()?;
        refuse(
            directory_id(&self.path)? != self.identity,
            "V3 backend physical namespace identity changed",
        )
    }
    fn open(&self, owner: &Owner) -> Result<Arc<ProductServer>, CorpusProblem> {
        self.verify(owner)?;
        ProductServer::open_with_package_trust(
            self.config.clone(),
            self.store.clone(),
            Arc::new(MemorySecretResolver::default()),
            default_program_router(),
            carddemo_package_trust()?,
        )
        .map_err(terminal_problem)
    }
    fn reopen_store(&mut self, owner: &Owner, sqlite: bool) -> Result<(), CorpusProblem> {
        self.verify(owner)?;
        if sqlite {
            self.store = Arc::new(
                SqliteStateStore::open(&self.config.sqlite_url, 64 * 1024 * 1024, 262144)
                    .map_err(package_problem)?,
            );
        }
        Ok(())
    }
}
fn publication_receipt(
    receipt: &mainframe_env_server::ApplicationPublicationReceipt,
    record: &ApplicationGenerationRecord,
    replayed: bool,
) -> bool {
    receipt.package == APP
        && receipt.generation == record.generation
        && receipt.identity == record.identity
        && receipt.controllers == 5
        && receipt.db2_catalog
        && !receipt.ims_metadata
        && receipt.replayed == replayed
}
fn phase(
    owner: &Owner,
    report: &mut serde_json::Value,
    scope: &str,
    name: &str,
    snapshot: &Snapshot,
) -> Result<(), CorpusProblem> {
    let expected = if scope.ends_with("-main") {
        MAIN_PHASES
    } else {
        FAILURE_PHASES
    };
    let phases = report["scopes"][scope]["phases"]
        .as_array_mut()
        .ok_or_else(|| drift("V3 owned phase ledger invalid"))?;
    refuse(
        expected.get(phases.len()) != Some(&name),
        "actual V3 lifecycle phases omitted or reordered",
    )?;
    phases.push(serde_json::json!({"phase":name,"actual_snapshot":snapshot_json(snapshot)}));
    owner.save(report)
}
async fn drain(
    owner: &Owner,
    report: &mut serde_json::Value,
    backend: &Backend,
    server: Arc<ProductServer>,
    stage: &str,
) -> Result<(), CorpusProblem> {
    let stopped = server.graceful_shutdown().await;
    drop(server);
    report["drains"]
        .as_array_mut()
        .ok_or_else(|| drift("V3 drain ledger invalid"))?
        .push(serde_json::json!({"scope":backend.name,"stage":stage,"stopped":stopped}));
    owner.save(report)?;
    refuse(!stopped, "actual V3 server drain/closure uncertain")
}
fn admission_candidates(
    base: &ApplicationPackageV2,
) -> Result<Vec<(&'static str, InstallProblem, ApplicationPackageV2)>, CorpusProblem> {
    let mut output = Vec::new();
    for variant in 0..7 {
        let mut candidate = base.clone();
        candidate.generation = 3;
        candidate.base.manifest.version = "0.2.2".into();
        let (name, expected_problem) = match variant {
            0 => {
                let removed = candidate
                    .base
                    .manifest
                    .entries
                    .iter()
                    .find(|row| row.kind == EntryKind::Migration)
                    .ok_or_else(|| drift("negative migration missing"))?
                    .sha256
                    .clone();
                candidate
                    .base
                    .manifest
                    .entries
                    .retain(|row| row.kind != EntryKind::Migration);
                candidate.base.blobs.remove(&removed);
                ("missing-kind", InstallProblem::MissingKind)
            }
            1 => {
                candidate.base.manifest.entries[1].path =
                    candidate.base.manifest.entries[0].path.clone();
                ("same-count-duplicate", InstallProblem::DuplicateEntry)
            }
            2 => {
                candidate.base.manifest.entries[1].depends_on =
                    vec!["missing-owned-dependency".into()];
                ("orphan", InstallProblem::OrphanDependency)
            }
            3 => {
                let digest = candidate.base.manifest.entries[2].sha256.clone();
                candidate
                    .base
                    .blobs
                    .get_mut(&digest)
                    .ok_or_else(|| drift("negative blob missing"))?[0] ^= 1;
                ("same-length-corrupt", InstallProblem::ContentMismatch)
            }
            4 => {
                candidate.base.manifest.target_product = "0.3.0".into();
                ("incompatible", InstallProblem::IncompatibleProduct)
            }
            5 => ("signature", InstallProblem::InvalidSignature),
            _ => {
                candidate.generation = 1;
                replace_blob(
                    &mut candidate,
                    "profile/manifest",
                    b"changed retained generation".to_vec(),
                )?;
                (
                    "retained-identity-conflict",
                    InstallProblem::IdentityConflict,
                )
            }
        };
        resign(&mut candidate)?;
        if variant == 5 {
            candidate.signature.value = "invalid-signature".into();
        }
        output.push((name, expected_problem, candidate));
    }
    Ok(output)
}
fn stage_negatives(
    owner: &Owner,
    backend: &Backend,
    server: &ProductServer,
    base: &ApplicationPackageV2,
    before: &Snapshot,
) -> Result<serde_json::Value, CorpusProblem> {
    let mut results = Vec::new();
    for (name, expected_problem, candidate) in admission_candidates(base)? {
        let core = registry(backend.store.as_ref())?;
        refuse(
            core.stage(&candidate) != Err(expected_problem.clone()),
            "source-selected generic refusal differs",
        )?;
        backend.verify(owner)?;
        let actual = server.install_application_package_v2(&candidate);
        let expected = if expected_problem == InstallProblem::IdentityConflict {
            HostProblem::IdempotencyConflict
        } else {
            HostProblem::Malformed
        };
        refuse(
            actual != Err(expected),
            "actual source-selected product admission refusal differs",
        )?;
        refuse(
            raw_rows(backend.store.as_ref())? != before.rows,
            "actual admission refusal changed complete provider bytes/revisions",
        )?;
        results.push(serde_json::json!({"control":name,"actual_result":format!("{actual:?}"),"source_selected_problem":format!("{expected_problem:?}")}));
    }
    Ok(serde_json::json!(results))
}
async fn main_scope(
    owner: &Owner,
    report: &mut serde_json::Value,
    backend: &mut Backend,
    sqlite: bool,
    expected: &Inputs,
    definition: &BatchProgramDefinition,
) -> Result<(), CorpusProblem> {
    let server = backend.open(owner)?;
    let result = (|| {
        backend.verify(owner)?;
        server
            .install_batch_programs(vec![definition.clone()])
            .map_err(terminal_problem)?;
        backend.verify(owner)?;
        install_carddemo_db2_package(&server, std::slice::from_ref(definition))?;
        let installer = registry(backend.store.as_ref())?;
        let selected = installer
            .selected_generation(APP)
            .map_err(package_problem)?
            .ok_or_else(|| drift("actual initial package missing"))?;
        let base = selected.package().clone();
        refuse(
            !declaration_matches(&base, expected, definition),
            "independent full declared package metadata differs",
        )?;
        let mut packages = BTreeMap::from([(1, base.clone())]);
        let staged = BTreeSet::new();
        let initial = capture(
            owner,
            backend,
            &server,
            expected,
            definition,
            &packages,
            &staged,
            (1, PublicationAction::Install),
        )?;
        phase(owner, report, &backend.name, "g1", &initial)?;
        backend.verify(owner)?;
        let replay = server
            .install_application_package_v2(&base)
            .map_err(terminal_problem)?;
        refuse(
            replay != initial.selected,
            "actual reinstall Ready record differs",
        )?;
        backend.verify(owner)?;
        let published = server
            .publish_application_generation(&replay)
            .map_err(terminal_problem)?;
        refuse(
            !publication_receipt(&published, &replay, true),
            "actual replay publication receipt differs",
        )?;
        let reinstalled = capture(
            owner,
            backend,
            &server,
            expected,
            definition,
            &packages,
            &staged,
            (1, PublicationAction::Install),
        )?;
        refuse(
            !self::reinstalled(&initial, &reinstalled),
            "actual reinstall changed complete state or violated source-required registry revision advancement",
        )?;
        phase(owner, report, &backend.name, "reinstall", &reinstalled)?;
        let upgrade = controlled(&base, expected, false)?;
        backend.verify(owner)?;
        let g2 = server
            .install_application_package_v2(&upgrade)
            .map_err(terminal_problem)?;
        refuse(
            g2.generation != 2
                || g2.state != InstallState::Staged
                || g2.identity != package_generation_identity(&upgrade).map_err(package_problem)?,
            "actual g2 was not retained Staged",
        )?;
        packages.insert(2, upgrade);
        let pending = BTreeSet::from([2]);
        let staged_snapshot = capture(
            owner,
            backend,
            &server,
            expected,
            definition,
            &packages,
            &pending,
            (1, PublicationAction::Install),
        )?;
        refuse(
            !same_providers(&reinstalled, &staged_snapshot),
            "actual staging changed selected/published/provider state",
        )?;
        phase(owner, report, &backend.name, "staged-g2", &staged_snapshot)?;
        backend.verify(owner)?;
        let published = server
            .publish_application_generation(&g2)
            .map_err(terminal_problem)?;
        refuse(
            !publication_receipt(&published, &g2, false),
            "actual g2 publication receipt differs",
        )?;
        let upgraded = capture(
            owner,
            backend,
            &server,
            expected,
            definition,
            &packages,
            &staged,
            (2, PublicationAction::Install),
        )?;
        phase(owner, report, &backend.name, "g2", &upgraded)?;
        backend.verify(owner)?;
        let rolled = server
            .rollback_application_generation(&initial.selected)
            .map_err(terminal_problem)?;
        refuse(
            !publication_receipt(&rolled, &initial.selected, false),
            "actual retained rollback receipt differs",
        )?;
        let rollback = capture(
            owner,
            backend,
            &server,
            expected,
            definition,
            &packages,
            &staged,
            (1, PublicationAction::Rollback),
        )?;
        phase(owner, report, &backend.name, "rollback", &rollback)?;
        backend.verify(owner)?;
        let unselected = server.publish_application_generation(&upgraded.selected);
        refuse(
            unselected != Err(HostProblem::IdempotencyConflict),
            "unselected Ready publish retry was not refused",
        )?;
        let after = capture(
            owner,
            backend,
            &server,
            expected,
            definition,
            &packages,
            &staged,
            (1, PublicationAction::Rollback),
        )?;
        refuse(
            after != rollback,
            "unselected Ready refusal changed complete retained/provider state",
        )?;
        phase(
            owner,
            report,
            &backend.name,
            "unselected-ready-refused",
            &after,
        )?;
        report["scopes"][&backend.name]["admission_refusals"] =
            stage_negatives(owner, backend, &server, &base, &after)?;
        let refused = capture(
            owner,
            backend,
            &server,
            expected,
            definition,
            &packages,
            &staged,
            (1, PublicationAction::Rollback),
        )?;
        refuse(
            refused != rollback,
            "admission negative campaign changed complete retained state",
        )?;
        phase(owner, report, &backend.name, "admission-refusals", &refused)?;
        Ok::<_, CorpusProblem>((refused, packages))
    })();
    let retained = if result.is_err() {
        failure_state(owner, report, backend)
    } else {
        Ok(())
    };
    drain(owner, report, backend, server, "initial").await?;
    retained?;
    let (before, packages) = result?;
    backend.reopen_store(owner, sqlite)?;
    let server = backend.open(owner)?;
    let result = (|| {
        let after = capture(
            owner,
            backend,
            &server,
            expected,
            definition,
            &packages,
            &BTreeSet::new(),
            (1, PublicationAction::Rollback),
        )?;
        refuse(
            after != before,
            "fresh trusted backend/provider reopen changed exact selected/retained/state/artifact bytes",
        )?;
        phase(owner, report, &backend.name, "reopen", &after)
    })();
    let retained = if result.is_err() {
        failure_state(owner, report, backend)
    } else {
        Ok(())
    };
    drain(owner, report, backend, server, "reopened").await?;
    retained?;
    result
}
async fn malformed_scope(
    owner: &Owner,
    report: &mut serde_json::Value,
    backend: &mut Backend,
    sqlite: bool,
    expected: &Inputs,
    definition: &BatchProgramDefinition,
) -> Result<(), CorpusProblem> {
    let server = backend.open(owner)?;
    let result = (|| {
        backend.verify(owner)?;
        server
            .install_batch_programs(vec![definition.clone()])
            .map_err(terminal_problem)?;
        backend.verify(owner)?;
        install_carddemo_db2_package(&server, std::slice::from_ref(definition))?;
        let installer = registry(backend.store.as_ref())?;
        let selected = installer
            .selected_generation(APP)
            .map_err(package_problem)?
            .ok_or_else(|| drift("separate failure baseline package missing"))?;
        let base = selected.package().clone();
        refuse(
            !declaration_matches(&base, expected, definition),
            "separate failure baseline independent package differs",
        )?;
        let mut packages = BTreeMap::from([(1, base.clone())]);
        let before = capture(
            owner,
            backend,
            &server,
            expected,
            definition,
            &packages,
            &BTreeSet::new(),
            (1, PublicationAction::Install),
        )?;
        phase(owner, report, &backend.name, "g1", &before)?;
        let bad = controlled(&base, expected, true)?;
        backend.verify(owner)?;
        let accepted = server
            .install_application_package_v2(&bad)
            .map_err(terminal_problem)?;
        refuse(
            accepted.state != InstallState::Staged || accepted.generation != 2,
            "malformed-catalog candidate did not reach actual accepted Staged owner",
        )?;
        packages.insert(2, bad);
        let staged = BTreeSet::from([2]);
        let pending = capture(
            owner,
            backend,
            &server,
            expected,
            definition,
            &packages,
            &staged,
            (1, PublicationAction::Install),
        )?;
        refuse(
            !same_providers(&before, &pending),
            "accepted staged malformed catalog changed prior selected/providers",
        )?;
        phase(owner, report, &backend.name, "staged-malformed", &pending)?;
        backend.verify(owner)?;
        let actual = server.publish_application_generation(&accepted);
        refuse(
            actual != Err(HostProblem::Malformed),
            "actual malformed-catalog publication was not refused at owning prevalidation",
        )?;
        report["scopes"][&backend.name]["publication_refusal"] =
            serde_json::json!({"accepted_stage":accepted,"actual_result":format!("{actual:?}")});
        let after = capture(
            owner,
            backend,
            &server,
            expected,
            definition,
            &packages,
            &staged,
            (1, PublicationAction::Install),
        )?;
        refuse(
            after != pending,
            "failed publication changed complete staged/selected/provider state",
        )?;
        phase(owner, report, &backend.name, "publication-refused", &after)?;
        Ok::<_, CorpusProblem>((after, packages, staged))
    })();
    let retained = if result.is_err() {
        failure_state(owner, report, backend)
    } else {
        Ok(())
    };
    drain(owner, report, backend, server, "initial").await?;
    retained?;
    let (before, packages, staged) = result?;
    backend.reopen_store(owner, sqlite)?;
    let server = backend.open(owner)?;
    let result = (|| {
        let after = capture(
            owner,
            backend,
            &server,
            expected,
            definition,
            &packages,
            &staged,
            (1, PublicationAction::Install),
        )?;
        refuse(
            after != before,
            "fresh failure-owner reopen lost failed Staged or prior selected/providers",
        )?;
        phase(owner, report, &backend.name, "reopen", &after)
    })();
    let retained = if result.is_err() {
        failure_state(owner, report, backend)
    } else {
        Ok(())
    };
    drain(owner, report, backend, server, "reopened").await?;
    retained?;
    result
}
fn finished_v3(report: &serde_json::Value) -> bool {
    let Some(scopes) = report["scopes"].as_object() else {
        return false;
    };
    if !scopes.keys().map(String::as_str).eq(SCOPES.iter().copied()) {
        return false;
    }
    if !SCOPES.iter().all(|scope| {
        let expected = if scope.ends_with("-main") {
            MAIN_PHASES
        } else {
            FAILURE_PHASES
        };
        scopes[*scope]["phases"].as_array().is_some_and(|rows| {
            rows.len() == expected.len()
                && rows
                    .iter()
                    .zip(expected)
                    .enumerate()
                    .all(|(index, (row, name))| {
                        let snapshot = &row["actual_snapshot"];
                        let generation = if *name == "g2" { 2 } else { 1 };
                        let retained = if index < if scope.ends_with("-main") { 2 } else { 1 } {
                            vec!["1"]
                        } else {
                            vec!["1", "2"]
                        };
                        row["phase"] == *name
                            && snapshot["selected"]["state"] == "Ready"
                            && snapshot["selected"]["generation"] == generation
                            && snapshot["retained_packages"]
                                .as_object()
                                .is_some_and(|packages| {
                                    packages.keys().map(String::as_str).eq(retained)
                                })
                            && snapshot["staged_generations"]
                                == serde_json::json!(if *name == "staged-g2"
                                    || (*scope).ends_with("-malformed") && index > 0
                                {
                                    vec![2]
                                } else {
                                    Vec::<u64>::new()
                                })
                    })
        })
    }) {
        return false;
    }
    report["drains"].as_array().is_some_and(|rows| {
        rows.len() == 8
            && rows
                .iter()
                .zip(
                    SCOPES
                        .iter()
                        .flat_map(|scope| [(scope, "initial"), (scope, "reopened")]),
                )
                .all(|(row, (scope, stage))| {
                    row["scope"] == *scope && row["stage"] == stage && row["stopped"] == true
                })
    }) && report["issue_observations"] == serde_json::json!([])
}
fn scope_claims(owner: &Owner, claims: &[(PathBuf, (u64, u64))]) -> Result<(), CorpusProblem> {
    owner.verify()?;
    refuse(claims.len() != 4, "actual V3 scope claim set incomplete")?;
    let names = fs::read_dir(&owner.owned)
        .map_err(package_problem)?
        .map(|entry| {
            entry
                .map_err(package_problem)?
                .file_name()
                .into_string()
                .map_err(|_| drift("V3 scope membership is not observable"))
        })
        .collect::<Result<BTreeSet<_>, _>>()?;
    refuse(
        names.iter().map(String::as_str).ne(SCOPES.iter().copied()),
        "V3 physical scope membership differs",
    )?;
    for ((path, identity), name) in claims.iter().zip(SCOPES) {
        refuse(
            *path != owner.owned.join(name) || directory_id(path)? != *identity,
            "V3 backend namespace replaced or claim order differs",
        )?;
    }
    Ok(())
}
fn cleanup_v3(
    owner: &Owner,
    report: &mut serde_json::Value,
    claims: &[(PathBuf, (u64, u64))],
) -> Result<(), CorpusProblem> {
    owner.verify()?;
    refuse(
        !finished_v3(report) || claims.len() != 4,
        "actual V3 full scoped owner did not complete",
    )?;
    scope_claims(owner, claims)?;
    report["cleanup"] = serde_json::json!("verified-owned-removal-pending");
    owner.save(report)?;
    owner.verify()?;
    scope_claims(owner, claims)?;
    fs::remove_dir_all(&owner.owned).map_err(package_problem)?;
    absence(fs::symlink_metadata(&owner.owned))?;
    report["cleanup"] = serde_json::json!("removed-and-absence-verified");
    report["status"] = serde_json::json!("pass-qualified-state-proof-only");
    owner.save(report)
}
async fn run(corpus: &Path, inventory: &Path, evidence: PathBuf) -> Result<(), CorpusProblem> {
    verify_carddemo_corpus(corpus, inventory)?;
    let expected = inputs()?;
    source_inputs(corpus, &expected)?;
    let owner = Owner::claim(evidence)?;
    let scopes = SCOPES
        .iter()
        .map(|scope| ((*scope).to_string(), serde_json::json!({"phases":[]})))
        .collect::<serde_json::Map<_, _>>();
    let mut report = serde_json::json!({"schema":"private.carddemo.v3-retained-owner@1","status":"running","scopes":scopes,"drains":[],"cleanup":"pending","issue_observations":[],"oracle_sha256":format!("{:x}",Sha256::digest(INPUTS.as_bytes())),"scope":"declared existing V3 package, Memory/provider reopen and fresh SQLite store/provider reopen; zero whole CD009/compiled application execution credit"});
    owner.save(&report)?;
    let mut claims = Vec::new();
    let result=async {
        let definition=compiled(corpus,&expected)?;report["actual_publisher"]=serde_json::json!({"primary":PRIMARY,"artifact":definition.artifact.as_str(),"semantic_identity":definition.semantic_identity,"payload_bytes":definition.payload.len(),"payload_sha256":format!("{:x}",Sha256::digest(&definition.payload)),"manifest":source_artifact_binding::retain_manifest(&definition.manifest)});owner.save(&report)?;
        for sqlite in [false,true] {
            let prefix=if sqlite{"sqlite"}else{"memory"};let mut backend=Backend::create(&owner,&format!("{prefix}-main"),sqlite)?;claims.push((backend.path.clone(),backend.identity));main_scope(&owner,&mut report,&mut backend,sqlite,&expected,&definition).await?;drop(backend);
            let mut backend=Backend::create(&owner,&format!("{prefix}-malformed"),sqlite)?;claims.push((backend.path.clone(),backend.identity));malformed_scope(&owner,&mut report,&mut backend,sqlite,&expected,&definition).await?;drop(backend);
        }
        source_inputs(corpus,&expected)?;Ok::<_,CorpusProblem>(())
    }.await;
    if let Err(error) = result {
        report["status"] = serde_json::json!("fail-owned-state-retained");
        report["failure"] = serde_json::json!(error.to_string());
        owner.save(&report)?;
        return Err(error);
    }
    cleanup_v3(&owner, &mut report, &claims)
}
#[cfg(test)]
#[path = "v3_retained_lifecycle_tests.rs"]
mod tests;
