//! Fail-closed verification for the externally supplied CardDemo corpus.

use mainframe_env_application::{
    ApplicationInstaller, ApplicationManifest, ApplicationPackage, DatasetCatalog,
    DatasetCatalogEntry, DatasetDefinition, EntryKind, GenerationGroupDefinition, InstallProblem,
    InstallState, PackageEntry, ProgramArtifact, ProgramCatalog, ProgramFrame, ProgramFrames,
    package_identity, parse_bms, parse_csd,
};
use mainframe_env_compiler::{
    CobolCompiler, ControlEdgeKind, ControlRole, DataCategory, StatementKind, StorageSection,
    compatibility_copybooks, owned_compatibility_library,
};
use mainframe_env_dataset::{DatasetLimits, DatasetSeedObject, DatasetService};
use mainframe_env_execution_api::{
    IdempotencyKey, InvocationLimits, Machine, MachineDrive, MachineResume, PrincipalId, Quantum,
};
use mainframe_env_host_api::{
    AccessIntent, AuditEvent, CicsOperation, DatasetAttributes, DatasetName, DatasetOrganization,
    DatasetRequest, Mutation, RecordFormat, ResourceName, SecretRef, SecurityDecision,
};
use mainframe_env_racf::{
    MemorySecretResolver, RacfManifest, RacfProfileDefinition, RacfService, RacfUserDefinition,
};
use mainframe_env_source::{
    LogicalPath, SourceBundle, SourceEncoding, SourceFile, SourceFormat, SourceLibrary,
    SourceLimits,
};
use mainframe_env_store::MemoryStore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fmt;
use std::fs;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;

const CORPUS_ENV: &str = "CARDEMO_CORPUS_DIR";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CorpusProblem {
    pub code: String,
    pub detail: String,
}

impl CorpusProblem {
    fn new(code: &str, detail: impl Into<String>) -> Self {
        Self {
            code: code.to_string(),
            detail: detail.into(),
        }
    }
}

impl fmt::Display for CorpusProblem {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code, self.detail)
    }
}

impl std::error::Error for CorpusProblem {}

#[derive(Clone, Debug, Deserialize)]
struct CorpusContract {
    repository: String,
    commit: String,
    tree: String,
    license: String,
    license_sha256: String,
    executable_content_identity: ContentIdentity,
    runtime_oracles: Vec<RuntimeOracleContract>,
    file_count_checks: Vec<FileCountContract>,
    external_compatibility_copybooks: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
struct ContentIdentity {
    algorithm: String,
    sha256: String,
}

#[derive(Clone, Debug, Deserialize)]
struct RuntimeOracleContract {
    path: String,
    sha256: String,
}

#[derive(Clone, Debug, Deserialize)]
struct FileCountContract {
    id: String,
    roots: Vec<String>,
    #[serde(default)]
    extensions: Vec<String>,
    #[serde(default)]
    excluded_names: Vec<String>,
    expected: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CardDemoCorpusReceipt {
    pub schema_version: String,
    pub status: String,
    pub repository: String,
    pub commit: String,
    pub tree: String,
    pub clean: bool,
    pub license: LicenseReceipt,
    pub content: ContentReceipt,
    pub runtime_oracles: Vec<RuntimeOracleReceipt>,
    pub file_counts: BTreeMap<String, usize>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct LicenseReceipt {
    pub identifier: String,
    pub sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ContentReceipt {
    pub algorithm: String,
    pub sha256: String,
    pub tracked_files: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RuntimeOracleReceipt {
    pub path: String,
    pub sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CardDemoSourceReceipt {
    pub schema_version: String,
    pub status: String,
    pub corpus_commit: String,
    pub programs_checked: usize,
    pub copybooks_loaded: usize,
    pub compatibility_placeholders: usize,
    pub copy_expansions: usize,
    pub deterministic_replays: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CardDemoClosureReceipt {
    pub schema_version: String,
    pub status: String,
    pub corpus_commit: String,
    pub source_library_contract: String,
    pub compatibility_contract: String,
    pub programs_checked: usize,
    pub application_copybooks: usize,
    pub owned_compatibility_copybooks: usize,
    pub ordered_libraries: usize,
    pub copy_expansions: usize,
    pub unique_source_identities: usize,
    pub placeholder_content_present: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CardDemoLayoutReceipt {
    pub schema_version: String,
    pub status: String,
    pub corpus_commit: String,
    pub programs_checked: usize,
    pub semantic_models: usize,
    pub layouts: usize,
    pub qualified_duplicate_sets: usize,
    pub redefines: usize,
    pub variable_occurs: usize,
    pub condition_names: usize,
    pub file_records: usize,
    pub linkage_items: usize,
    pub max_program_storage_bytes: usize,
    pub layout_sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CardDemoControlReceipt {
    pub schema_version: String,
    pub status: String,
    pub corpus_commit: String,
    pub programs_checked: usize,
    pub hir_models: usize,
    pub statements: usize,
    pub control_nodes: usize,
    pub control_edges: usize,
    pub block_starts: usize,
    pub branches: usize,
    pub true_edges: usize,
    pub false_edges: usize,
    pub loop_edges: usize,
    pub explicit_scope_ends: usize,
    pub implicit_scope_ends: usize,
    pub recovered_nodes: usize,
    pub paragraph_calls: usize,
    pub transfers: usize,
    pub returns: usize,
    pub control_sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CardDemoCoreReceipt {
    pub schema_version: String,
    pub status: String,
    pub corpus_commit: String,
    pub programs_checked: usize,
    pub semantic_models: usize,
    pub hir_models: usize,
    pub layouts: usize,
    pub statements: usize,
    pub numeric_display: usize,
    pub numeric_edited: usize,
    pub packed_decimal: usize,
    pub binary: usize,
    pub groups: usize,
    pub conditions: usize,
    pub signed_items: usize,
    pub scaled_items: usize,
    pub occurs_items: usize,
    pub reference_operands: usize,
    pub reached_functions: BTreeMap<String, usize>,
    pub oracle_cases: usize,
    pub oracle_output_sha256: String,
    pub core_shape_sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CardDemoFileCallReceipt {
    pub schema_version: String,
    pub status: String,
    pub corpus_commit: String,
    pub programs_checked: usize,
    pub file_bindings: usize,
    pub organizations: BTreeMap<String, usize>,
    pub access_modes: BTreeMap<String, usize>,
    pub keyed_files: usize,
    pub relative_key_files: usize,
    pub file_status_bindings: usize,
    pub linkage_items: usize,
    pub call_statements: usize,
    pub call_using_operands: usize,
    pub open_statements: usize,
    pub close_statements: usize,
    pub read_statements: usize,
    pub write_statements: usize,
    pub selected_routes_present: bool,
    pub contract_sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CardDemoHostReceipt {
    pub schema_version: String,
    pub status: String,
    pub corpus_commit: String,
    pub programs_checked: usize,
    pub cics_operations: usize,
    pub sql_operations: usize,
    pub dli_operations: usize,
    pub mq_calls: usize,
    pub typed_operands: usize,
    pub storage_destinations: usize,
    pub opcodes: BTreeMap<String, usize>,
    pub typed_oracle_cases: usize,
    pub host_shape_sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CardDemoPackageReceipt {
    pub schema_version: String,
    pub status: String,
    pub corpus_commit: String,
    pub package_name: String,
    pub package_version: String,
    pub package_identity: String,
    pub entries: BTreeMap<String, String>,
    pub staged_not_ready: bool,
    pub committed_ready: bool,
    pub idempotent_reinstall: bool,
    pub negative_controls: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CardDemoResourceReceipt {
    pub schema_version: String,
    pub status: String,
    pub corpus_commit: String,
    pub bms_sources: usize,
    pub mapsets: usize,
    pub maps: usize,
    pub fields: usize,
    pub transactions: usize,
    pub programs: usize,
    pub files: usize,
    pub tdqueues: usize,
    pub cross_references: usize,
    pub unresolved: Vec<String>,
    pub resource_sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CardDemoProgramReceipt {
    pub schema_version: String,
    pub status: String,
    pub corpus_commit: String,
    pub installed_programs: usize,
    pub generations: usize,
    pub exact_generation_resolution: bool,
    pub xctl_replaced_frame: bool,
    pub link_child_frame: bool,
    pub return_restored_caller: bool,
    pub context_propagated: bool,
    pub catalog_sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CardDemoCicsReceipt {
    pub schema_version: String,
    pub status: String,
    pub corpus_commit: String,
    pub maps: usize,
    pub fields: usize,
    pub send_receive: usize,
    pub file_and_browse: usize,
    pub resp: usize,
    pub resp2: usize,
    pub nohandle: usize,
    pub handle: usize,
    pub eib_fields: Vec<String>,
    pub provider_tests: usize,
    pub cics_shape_sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CardDemoCicsRuntimeReceipt {
    pub schema_version: String,
    pub status: String,
    pub corpus_commit: String,
    pub reached_operations: usize,
    pub executable_variants: usize,
    pub residual_counts: BTreeMap<String, usize>,
    pub syncpoint_rollbacks: usize,
    pub return_transid: usize,
    pub return_commarea: usize,
    pub send_variants: BTreeMap<String, usize>,
    pub format_destinations: Vec<String>,
    pub runtime_regression_cases: usize,
    pub cics_runtime_sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CardDemoVsamReceipt {
    pub schema_version: String,
    pub status: String,
    pub corpus_commit: String,
    pub runtime_keyed_paths: usize,
    pub base_clusters: usize,
    pub alternate_index_paths: usize,
    pub key_offsets_and_lengths: BTreeMap<String, String>,
    pub reached_file_operations: BTreeMap<String, usize>,
    pub aix_definitions: usize,
    pub unique_aix_names: usize,
    pub nonunique_definitions: usize,
    pub upgrade_definitions: usize,
    pub provider_regression_cases: usize,
    pub vsam_shape_sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CardDemoDatasetCatalogReceipt {
    pub schema_version: String,
    pub status: String,
    pub corpus_commit: String,
    pub runtime_definitions: usize,
    pub keyed_datasets: usize,
    pub generation_groups: usize,
    pub initial_generations: usize,
    pub partitioned_datasets: usize,
    pub member_extensions: Vec<String>,
    pub gdg_limit: u32,
    pub scratch_noempty_groups: usize,
    pub reached_esds_definitions: usize,
    pub reached_rrds_definitions: usize,
    pub record_formats: Vec<String>,
    pub provider_regression_cases: usize,
    pub catalog_shape_sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CardDemoSeedReceipt {
    pub schema_version: String,
    pub status: String,
    pub corpus_commit: String,
    pub seed_objects: usize,
    pub installed_datasets: usize,
    pub seed_records: usize,
    pub installed_records: usize,
    pub seed_bytes: usize,
    pub seed_sha256: BTreeMap<String, String>,
    pub runtime_key_ranges_checked: usize,
    pub alternate_indexes_installed: usize,
    pub cics_file_aliases: BTreeMap<String, String>,
    pub reinstall_passed: bool,
    pub upgrade_passed: bool,
    pub rollback_passed: bool,
    pub negative_controls: usize,
    pub install_identity: String,
    pub seed_shape_sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CardDemoSecurityReceipt {
    pub schema_version: String,
    pub status: String,
    pub corpus_commit: String,
    pub transport_users: usize,
    pub application_signon_records: usize,
    pub identities_distinct: bool,
    pub groups: usize,
    pub profiles: usize,
    pub permissions: usize,
    pub resource_classes: Vec<String>,
    pub transaction_profiles: usize,
    pub program_profiles: usize,
    pub dataset_profiles: usize,
    pub queue_profiles: usize,
    pub regular_allow_checks: usize,
    pub regular_deny_checks: usize,
    pub admin_allow_checks: usize,
    pub redacted_fields: usize,
    pub manifest_replay: bool,
    pub security_shape_sha256: String,
}

pub fn verify_carddemo_corpus_from_env(
    inventory_path: &Path,
) -> Result<CardDemoCorpusReceipt, CorpusProblem> {
    let corpus = env::var_os(CORPUS_ENV).ok_or_else(|| {
        CorpusProblem::new(
            "carddemo.corpus.environment_missing",
            "CARDEMO_CORPUS_DIR is required for CardDemo gates",
        )
    })?;
    verify_carddemo_corpus(Path::new(&corpus), inventory_path)
}

pub fn verify_carddemo_corpus(
    corpus_dir: &Path,
    inventory_path: &Path,
) -> Result<CardDemoCorpusReceipt, CorpusProblem> {
    require_corpus_directory(corpus_dir)?;
    let contract = read_contract(inventory_path)?;
    validate_contract(&contract)?;

    let status = git(
        corpus_dir,
        &["status", "--porcelain=v1", "--untracked-files=all"],
    )?;
    if !status.is_empty() {
        return Err(CorpusProblem::new(
            "carddemo.corpus.dirty",
            "CardDemo checkout contains tracked or untracked changes",
        ));
    }

    let commit = git(corpus_dir, &["rev-parse", "HEAD"])?;
    require_equal(
        "carddemo.corpus.commit_drift",
        "commit",
        &contract.commit,
        &commit,
    )?;
    let tree = git(corpus_dir, &["rev-parse", "HEAD^{tree}"])?;
    require_equal("carddemo.corpus.tree_drift", "tree", &contract.tree, &tree)?;
    let repository = git(corpus_dir, &["remote", "get-url", "origin"])?;
    require_equal(
        "carddemo.corpus.repository_drift",
        "repository",
        &contract.repository,
        &repository,
    )?;

    let license_path = corpus_dir.join("LICENSE");
    let license_bytes = read_corpus_file(corpus_dir, &license_path)?;
    let license_sha256 = sha256(&license_bytes);
    require_equal(
        "carddemo.corpus.license_drift",
        "license SHA-256",
        &contract.license_sha256,
        &license_sha256,
    )?;
    let license_text = String::from_utf8_lossy(&license_bytes);
    if contract.license != "Apache-2.0"
        || !license_text.contains("Apache License")
        || !license_text.contains("Version 2.0")
    {
        return Err(CorpusProblem::new(
            "carddemo.corpus.license_unaccepted",
            "the declared corpus license is not the pinned Apache-2.0 text",
        ));
    }

    let tracked_files = tracked_files(corpus_dir)?;
    let content_sha256 = canonical_content_digest(corpus_dir, &tracked_files)?;
    require_equal(
        "carddemo.corpus.content_drift",
        "canonical content SHA-256",
        &contract.executable_content_identity.sha256,
        &content_sha256,
    )?;

    let mut runtime_oracles = Vec::with_capacity(contract.runtime_oracles.len());
    for archive in &contract.runtime_oracles {
        let bytes = read_runtime_oracle(corpus_dir, &archive.path)?;
        let actual = sha256(&bytes);
        require_equal(
            "carddemo.corpus.runtime_archive_drift",
            &format!("runtime archive {} SHA-256", archive.path),
            &archive.sha256,
            &actual,
        )?;
        runtime_oracles.push(RuntimeOracleReceipt {
            path: archive.path.clone(),
            sha256: actual,
        });
    }

    let mut file_counts = BTreeMap::new();
    for check in &contract.file_count_checks {
        let actual = count_files(&tracked_files, check)?;
        if actual != check.expected {
            return Err(CorpusProblem::new(
                "carddemo.corpus.file_count_drift",
                format!(
                    "file count {} expected {} but found {}",
                    check.id, check.expected, actual
                ),
            ));
        }
        if file_counts.insert(check.id.clone(), actual).is_some() {
            return Err(CorpusProblem::new(
                "carddemo.corpus.contract_invalid",
                format!("duplicate file-count identity {}", check.id),
            ));
        }
    }

    Ok(CardDemoCorpusReceipt {
        schema_version: "mainframe-env.carddemo-corpus-receipt@1".to_string(),
        status: "pass".to_string(),
        repository,
        commit,
        tree,
        clean: true,
        license: LicenseReceipt {
            identifier: contract.license,
            sha256: license_sha256,
        },
        content: ContentReceipt {
            algorithm: contract.executable_content_identity.algorithm,
            sha256: content_sha256,
            tracked_files: tracked_files.len(),
        },
        runtime_oracles,
        file_counts,
    })
}

pub fn verify_carddemo_source_preprocessing_from_env(
    inventory_path: &Path,
) -> Result<CardDemoSourceReceipt, CorpusProblem> {
    let corpus_dir = env::var_os(CORPUS_ENV).ok_or_else(|| {
        CorpusProblem::new(
            "carddemo.corpus.environment_missing",
            "CARDEMO_CORPUS_DIR is required for CardDemo gates",
        )
    })?;
    let corpus_dir = Path::new(&corpus_dir);
    let corpus = verify_carddemo_corpus(corpus_dir, inventory_path)?;
    let compatibility_names = read_contract(inventory_path)?.external_compatibility_copybooks;
    let source_paths = collect_paths(
        corpus_dir,
        &[
            "app/cbl",
            "app/app-authorization-ims-db2-mq/cbl",
            "app/app-transaction-type-db2/cbl",
            "app/app-vsam-mq/cbl",
        ],
        "cbl",
    )?;
    let copy_paths = collect_paths(
        corpus_dir,
        &[
            "app/cpy",
            "app/cpy-bms",
            "app/app-authorization-ims-db2-mq/cpy",
            "app/app-authorization-ims-db2-mq/cpy-bms",
            "app/app-transaction-type-db2/cpy",
            "app/app-transaction-type-db2/cpy-bms",
        ],
        "cpy",
    )?;
    if source_paths.len() != 44 || copy_paths.len() != 62 {
        return Err(CorpusProblem::new(
            "carddemo.source.corpus_count_drift",
            format!(
                "source preprocessing expected 44 programs and 62 copybooks but found {} and {}",
                source_paths.len(),
                copy_paths.len()
            ),
        ));
    }

    let limits = SourceLimits::default();
    let copybooks = copy_paths
        .iter()
        .map(|path| source_file(corpus_dir, path, limits))
        .collect::<Result<Vec<_>, _>>()?;
    if compatibility_names.len() != 9 {
        return Err(CorpusProblem::new(
            "carddemo.source.corpus_count_drift",
            format!(
                "source preprocessing expected nine external compatibility copybooks but found {}",
                compatibility_names.len()
            ),
        ));
    }
    let compatibility = compatibility_names
        .iter()
        .map(|name| {
            if name.is_empty()
                || !name
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            {
                return Err(CorpusProblem::new(
                    "carddemo.source.bundle_invalid",
                    "compatibility copybook name is invalid",
                ));
            }
            SourceFile::input(
                format!("compatibility/{name}.cpy"),
                b"      * CD-002 source-expansion placeholder; CD-003 owns compatibility content.\n"
                    .to_vec(),
                SourceFormat::Fixed,
                SourceEncoding::Utf8,
                limits,
            )
            .map_err(|error| {
                CorpusProblem::new(
                    "carddemo.source.bundle_invalid",
                    format!("cannot create compatibility placeholder: {error}"),
                )
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    let compiler = CobolCompiler::default();
    let mut copy_expansions = 0usize;
    for primary_path in &source_paths {
        let primary = source_file(corpus_dir, primary_path, limits)?;
        let mut files = Vec::with_capacity(1 + copybooks.len() + compatibility.len());
        files.push(primary);
        files.extend(copybooks.iter().cloned());
        files.extend(compatibility.iter().cloned());
        let logical = LogicalPath::new(primary_path, limits.max_path_bytes).map_err(|error| {
            CorpusProblem::new(
                "carddemo.source.bundle_invalid",
                format!("invalid program logical path: {error}"),
            )
        })?;
        let bundle = SourceBundle::new(&logical, files, BTreeMap::new(), Vec::new(), limits)
            .map_err(|error| {
                CorpusProblem::new(
                    "carddemo.source.bundle_invalid",
                    format!("cannot construct source closure: {error}"),
                )
            })?;
        let first = compiler.analyze(&bundle);
        let first_syntax = first.syntax.ok_or_else(|| {
            let diagnostic = first
                .diagnostics
                .first()
                .map_or("source preprocessing failed", |item| item.public_message());
            CorpusProblem::new(
                "carddemo.source.preprocessing_failed",
                format!("program {primary_path} did not reach syntax: {diagnostic}"),
            )
        })?;
        let second = compiler.analyze(&bundle);
        let second_syntax = second.syntax.ok_or_else(|| {
            CorpusProblem::new(
                "carddemo.source.nondeterministic",
                format!("program {primary_path} failed on deterministic replay"),
            )
        })?;
        if first_syntax.expansions() != second_syntax.expansions()
            || first_syntax.semantic_origins() != second_syntax.semantic_origins()
            || first_syntax.copy_directive_origins() != second_syntax.copy_directive_origins()
            || first_syntax.token_count() != second_syntax.token_count()
        {
            return Err(CorpusProblem::new(
                "carddemo.source.nondeterministic",
                format!("program {primary_path} source preprocessing replay differs"),
            ));
        }
        copy_expansions = copy_expansions
            .checked_add(first_syntax.expansions().len())
            .ok_or_else(|| {
                CorpusProblem::new(
                    "carddemo.source.resource_exhausted",
                    "copy expansion counter overflow",
                )
            })?;
    }

    Ok(CardDemoSourceReceipt {
        schema_version: "mainframe-env.carddemo-source-receipt@1".to_string(),
        status: "pass".to_string(),
        corpus_commit: corpus.commit,
        programs_checked: source_paths.len(),
        copybooks_loaded: copybooks.len(),
        compatibility_placeholders: compatibility.len(),
        copy_expansions,
        deterministic_replays: source_paths.len(),
    })
}

pub fn verify_carddemo_source_closures_from_env(
    inventory_path: &Path,
) -> Result<CardDemoClosureReceipt, CorpusProblem> {
    let corpus_dir = env::var_os(CORPUS_ENV).ok_or_else(|| {
        CorpusProblem::new(
            "carddemo.corpus.environment_missing",
            "CARDEMO_CORPUS_DIR is required for CardDemo gates",
        )
    })?;
    let corpus_dir = Path::new(&corpus_dir);
    let corpus = verify_carddemo_corpus(corpus_dir, inventory_path)?;
    let contract = read_contract(inventory_path)?;
    let source_paths = collect_paths(
        corpus_dir,
        &[
            "app/cbl",
            "app/app-authorization-ims-db2-mq/cbl",
            "app/app-transaction-type-db2/cbl",
            "app/app-vsam-mq/cbl",
        ],
        "cbl",
    )?;
    let copy_roots = [
        "app/app-authorization-ims-db2-mq/cpy",
        "app/app-authorization-ims-db2-mq/cpy-bms",
        "app/app-transaction-type-db2/cpy",
        "app/app-transaction-type-db2/cpy-bms",
        "app/cpy",
        "app/cpy-bms",
    ];
    let copy_paths = collect_paths(corpus_dir, &copy_roots, "cpy")?;
    if source_paths.len() != 44 || copy_paths.len() != 62 {
        return Err(CorpusProblem::new(
            "carddemo.closure.corpus_count_drift",
            "explicit source closure counts differ from the pinned inventory",
        ));
    }
    let owned_names = compatibility_copybooks()
        .iter()
        .map(|copybook| copybook.name.to_string())
        .collect::<BTreeSet<_>>();
    if contract
        .external_compatibility_copybooks
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>()
        != owned_names
    {
        return Err(CorpusProblem::new(
            "carddemo.closure.compatibility_drift",
            "owned compatibility catalog differs from the pinned reached list",
        ));
    }
    let limits = SourceLimits::default();
    let copybooks = copy_paths
        .iter()
        .map(|path| source_file(corpus_dir, path, limits))
        .collect::<Result<Vec<_>, _>>()?;
    let (compatibility, compatibility_library) =
        owned_compatibility_library(limits).map_err(|error| {
            CorpusProblem::new(
                "carddemo.closure.compatibility_invalid",
                format!("owned compatibility catalog is invalid: {error}"),
            )
        })?;
    let placeholder_content_present = compatibility.iter().any(|file| {
        String::from_utf8_lossy(file.bytes())
            .to_ascii_lowercase()
            .contains("placeholder")
    });
    if placeholder_content_present {
        return Err(CorpusProblem::new(
            "carddemo.closure.compatibility_invalid",
            "owned compatibility catalog contains placeholder content",
        ));
    }
    let mut libraries = Vec::with_capacity(copy_roots.len() + 1);
    for (index, root) in copy_roots.iter().enumerate() {
        let members = collect_paths(corpus_dir, &[*root], "cpy")?
            .into_iter()
            .map(|path| LogicalPath::new(path, limits.max_path_bytes))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| {
                CorpusProblem::new(
                    "carddemo.closure.library_invalid",
                    format!("application member path is invalid: {error}"),
                )
            })?;
        libraries.push(
            SourceLibrary::new(format!("application-{index:02}"), members, limits).map_err(
                |error| {
                    CorpusProblem::new(
                        "carddemo.closure.library_invalid",
                        format!("application library is invalid: {error}"),
                    )
                },
            )?,
        );
    }
    libraries.push(compatibility_library);

    let compiler = CobolCompiler::default();
    let mut copy_expansions = 0usize;
    let mut identities = BTreeSet::new();
    for primary_path in &source_paths {
        let primary = source_file(corpus_dir, primary_path, limits)?;
        let mut files = Vec::with_capacity(1 + copybooks.len() + compatibility.len());
        files.push(primary);
        files.extend(copybooks.iter().cloned());
        files.extend(compatibility.iter().cloned());
        let logical = LogicalPath::new(primary_path, limits.max_path_bytes).map_err(|error| {
            CorpusProblem::new(
                "carddemo.closure.library_invalid",
                format!("program path is invalid: {error}"),
            )
        })?;
        let bundle = SourceBundle::with_libraries(
            &logical,
            files,
            libraries.clone(),
            BTreeMap::new(),
            Vec::new(),
            limits,
        )
        .map_err(|error| {
            CorpusProblem::new(
                "carddemo.closure.library_invalid",
                format!("source closure is invalid: {error}"),
            )
        })?;
        identities.insert(bundle.id().to_hex());
        let analysis = compiler.analyze(&bundle);
        let syntax = analysis.syntax.ok_or_else(|| {
            let diagnostic = analysis
                .diagnostics
                .first()
                .map_or("source closure failed", |item| item.public_message());
            CorpusProblem::new(
                "carddemo.closure.preprocessing_failed",
                format!("program {primary_path} did not reach syntax: {diagnostic}"),
            )
        })?;
        copy_expansions = copy_expansions
            .checked_add(syntax.expansions().len())
            .ok_or_else(|| {
                CorpusProblem::new(
                    "carddemo.closure.resource_exhausted",
                    "copy expansion counter overflow",
                )
            })?;
    }
    if identities.len() != source_paths.len() {
        return Err(CorpusProblem::new(
            "carddemo.closure.identity_collision",
            "explicit program source closures do not have unique identities",
        ));
    }
    Ok(CardDemoClosureReceipt {
        schema_version: "mainframe-env.carddemo-closure-receipt@1".into(),
        status: "pass".into(),
        corpus_commit: corpus.commit,
        source_library_contract: mainframe_env_source::SOURCE_LIBRARY_CONTRACT.into(),
        compatibility_contract: mainframe_env_compiler::COMPATIBILITY_COPYBOOK_CONTRACT.into(),
        programs_checked: source_paths.len(),
        application_copybooks: copybooks.len(),
        owned_compatibility_copybooks: compatibility.len(),
        ordered_libraries: libraries.len(),
        copy_expansions,
        unique_source_identities: identities.len(),
        placeholder_content_present,
    })
}

pub fn verify_carddemo_data_layouts_from_env(
    inventory_path: &Path,
) -> Result<CardDemoLayoutReceipt, CorpusProblem> {
    let corpus_dir = env::var_os(CORPUS_ENV).ok_or_else(|| {
        CorpusProblem::new(
            "carddemo.corpus.environment_missing",
            "CARDEMO_CORPUS_DIR is required for CardDemo gates",
        )
    })?;
    let corpus_dir = Path::new(&corpus_dir);
    let closure = verify_carddemo_source_closures_from_env(inventory_path)?;
    let bundles = explicit_carddemo_bundles(corpus_dir)?;
    let compiler = CobolCompiler::default();
    let mut layout_count = 0usize;
    let mut qualified_duplicate_sets = 0usize;
    let mut redefines = 0usize;
    let mut variable_occurs = 0usize;
    let mut condition_names = 0usize;
    let mut file_records = 0usize;
    let mut linkage_items = 0usize;
    let mut max_program_storage_bytes = 0usize;
    let mut layout_digest = Sha256::new();
    for (primary, bundle) in &bundles {
        let analysis = compiler.analyze(bundle);
        let semantic = analysis.semantic.ok_or_else(|| {
            let diagnostic = analysis
                .diagnostics
                .first()
                .map_or("semantic analysis failed", |item| item.public_message());
            CorpusProblem::new(
                "carddemo.layout.semantic_failed",
                format!("program {primary} failed semantic analysis: {diagnostic}"),
            )
        })?;
        let mut simple = BTreeMap::<String, usize>::new();
        for layout in &semantic.layouts {
            digest_field(&mut layout_digest, primary.as_bytes());
            digest_field(&mut layout_digest, layout.qualified_name.as_bytes());
            digest_field(&mut layout_digest, &[layout.level]);
            digest_field(
                &mut layout_digest,
                format!("{:?}", layout.section).as_bytes(),
            );
            digest_field(&mut layout_digest, &(layout.offset as u64).to_be_bytes());
            digest_field(&mut layout_digest, &(layout.length as u64).to_be_bytes());
            digest_field(
                &mut layout_digest,
                &(layout.element_length as u64).to_be_bytes(),
            );
            digest_field(
                &mut layout_digest,
                format!("{:?}", layout.category).as_bytes(),
            );
            digest_field(&mut layout_digest, &layout.initial);
            digest_field(
                &mut layout_digest,
                layout.alias_of.as_deref().unwrap_or("").as_bytes(),
            );
            digest_field(
                &mut layout_digest,
                &(layout.occurs_min as u64).to_be_bytes(),
            );
            digest_field(&mut layout_digest, &(layout.occurs as u64).to_be_bytes());
            digest_field(
                &mut layout_digest,
                layout.depending_on.as_deref().unwrap_or("").as_bytes(),
            );
            *simple.entry(layout.name.clone()).or_default() += 1;
            if layout.offset.saturating_add(layout.length) > semantic.storage_bytes
                && layout.category != mainframe_env_compiler::DataCategory::Condition
            {
                return Err(CorpusProblem::new(
                    "carddemo.layout.extent_invalid",
                    format!("program {primary} contains an out-of-range layout"),
                ));
            }
            redefines += usize::from(layout.alias_of.is_some() && layout.level != 88);
            variable_occurs += usize::from(layout.occurs_min != layout.occurs);
            condition_names += usize::from(layout.level == 88);
            file_records += usize::from(
                layout.section == mainframe_env_compiler::StorageSection::File && layout.level == 1,
            );
            linkage_items +=
                usize::from(layout.section == mainframe_env_compiler::StorageSection::Linkage);
        }
        qualified_duplicate_sets += simple.values().filter(|count| **count > 1).count();
        layout_count = layout_count
            .checked_add(semantic.layouts.len())
            .ok_or_else(|| {
                CorpusProblem::new(
                    "carddemo.layout.resource_exhausted",
                    "layout counter overflow",
                )
            })?;
        max_program_storage_bytes = max_program_storage_bytes.max(semantic.storage_bytes);
    }
    Ok(CardDemoLayoutReceipt {
        schema_version: "mainframe-env.carddemo-layout-receipt@1".into(),
        status: "pass".into(),
        corpus_commit: closure.corpus_commit,
        programs_checked: bundles.len(),
        semantic_models: bundles.len(),
        layouts: layout_count,
        qualified_duplicate_sets,
        redefines,
        variable_occurs,
        condition_names,
        file_records,
        linkage_items,
        max_program_storage_bytes,
        layout_sha256: format!("{:x}", layout_digest.finalize()),
    })
}

pub fn verify_carddemo_control_flow_from_env(
    inventory_path: &Path,
) -> Result<CardDemoControlReceipt, CorpusProblem> {
    let corpus_dir = env::var_os(CORPUS_ENV).ok_or_else(|| {
        CorpusProblem::new(
            "carddemo.corpus.environment_missing",
            "CARDEMO_CORPUS_DIR is required for CardDemo gates",
        )
    })?;
    let corpus_dir = Path::new(&corpus_dir);
    let closure = verify_carddemo_source_closures_from_env(inventory_path)?;
    let bundles = explicit_carddemo_bundles(corpus_dir)?;
    let compiler = CobolCompiler::default();
    let mut statements = 0usize;
    let mut control_nodes = 0usize;
    let mut control_edges = 0usize;
    let mut block_starts = 0usize;
    let mut branches = 0usize;
    let mut true_edges = 0usize;
    let mut false_edges = 0usize;
    let mut loop_edges = 0usize;
    let mut explicit_scope_ends = 0usize;
    let mut implicit_scope_ends = 0usize;
    let mut recovered_nodes = 0usize;
    let mut paragraph_calls = 0usize;
    let mut transfers = 0usize;
    let mut returns = 0usize;
    let mut control_digest = Sha256::new();
    for (primary, bundle) in &bundles {
        let analysis = compiler.analyze(bundle);
        let hir = analysis.hir.ok_or_else(|| {
            let diagnostic = analysis
                .diagnostics
                .first()
                .map_or("HIR construction failed", |item| item.public_message());
            CorpusProblem::new(
                "carddemo.control.hir_failed",
                format!("program {primary} failed HIR construction: {diagnostic}"),
            )
        })?;
        digest_field(&mut control_digest, primary.as_bytes());
        for statement in &hir.statements {
            digest_field(
                &mut control_digest,
                format!("{:?}", statement.kind).as_bytes(),
            );
            digest_field(&mut control_digest, &(statement.line as u64).to_be_bytes());
            for argument in &statement.arguments {
                digest_field(&mut control_digest, argument.as_bytes());
            }
        }
        for node in &hir.control_nodes {
            digest_field(&mut control_digest, &(node.id as u64).to_be_bytes());
            digest_field(&mut control_digest, &(node.line as u64).to_be_bytes());
            digest_field(&mut control_digest, format!("{:?}", node.role).as_bytes());
            digest_field(&mut control_digest, format!("{:?}", node.scope).as_bytes());
            digest_field(
                &mut control_digest,
                &(node.parent.unwrap_or(usize::MAX) as u64).to_be_bytes(),
            );
            digest_field(&mut control_digest, node.text.as_bytes());
            block_starts += usize::from(node.role == ControlRole::BlockStart);
            branches += usize::from(node.role == ControlRole::Branch);
            explicit_scope_ends +=
                usize::from(node.role == ControlRole::BlockEnd && node.text.starts_with("END-"));
            implicit_scope_ends +=
                usize::from(node.role == ControlRole::BlockEnd && node.text == ".");
            recovered_nodes += usize::from(node.role == ControlRole::Recovered);
        }
        for edge in &hir.control_edges {
            digest_field(&mut control_digest, &(edge.from as u64).to_be_bytes());
            digest_field(&mut control_digest, &(edge.to as u64).to_be_bytes());
            digest_field(&mut control_digest, format!("{:?}", edge.kind).as_bytes());
            paragraph_calls += usize::from(edge.kind == ControlEdgeKind::Call);
            transfers += usize::from(edge.kind == ControlEdgeKind::Transfer);
            returns += usize::from(edge.kind == ControlEdgeKind::Return);
            true_edges += usize::from(edge.kind == ControlEdgeKind::True);
            false_edges += usize::from(edge.kind == ControlEdgeKind::False);
            loop_edges += usize::from(edge.kind == ControlEdgeKind::Loop);
        }
        statements = checked_total(statements, hir.statements.len(), "statement")?;
        control_nodes = checked_total(control_nodes, hir.control_nodes.len(), "control node")?;
        control_edges = checked_total(control_edges, hir.control_edges.len(), "control edge")?;
    }
    Ok(CardDemoControlReceipt {
        schema_version: "mainframe-env.carddemo-control-receipt@1".into(),
        status: "pass".into(),
        corpus_commit: closure.corpus_commit,
        programs_checked: bundles.len(),
        hir_models: bundles.len(),
        statements,
        control_nodes,
        control_edges,
        block_starts,
        branches,
        true_edges,
        false_edges,
        loop_edges,
        explicit_scope_ends,
        implicit_scope_ends,
        recovered_nodes,
        paragraph_calls,
        transfers,
        returns,
        control_sha256: format!("{:x}", control_digest.finalize()),
    })
}

fn checked_total(current: usize, increment: usize, name: &str) -> Result<usize, CorpusProblem> {
    current.checked_add(increment).ok_or_else(|| {
        CorpusProblem::new(
            "carddemo.control.resource_exhausted",
            format!("{name} counter overflow"),
        )
    })
}

pub fn verify_carddemo_core_semantics_from_env(
    inventory_path: &Path,
) -> Result<CardDemoCoreReceipt, CorpusProblem> {
    let corpus_dir = env::var_os(CORPUS_ENV).ok_or_else(|| {
        CorpusProblem::new(
            "carddemo.corpus.environment_missing",
            "CARDEMO_CORPUS_DIR is required for CardDemo gates",
        )
    })?;
    let corpus_dir = Path::new(&corpus_dir);
    let closure = verify_carddemo_source_closures_from_env(inventory_path)?;
    let bundles = explicit_carddemo_bundles(corpus_dir)?;
    let compiler = CobolCompiler::default();
    let mut layouts = 0usize;
    let mut statements = 0usize;
    let mut numeric_display = 0usize;
    let mut numeric_edited = 0usize;
    let mut packed_decimal = 0usize;
    let mut binary = 0usize;
    let mut groups = 0usize;
    let mut conditions = 0usize;
    let mut signed_items = 0usize;
    let mut scaled_items = 0usize;
    let mut occurs_items = 0usize;
    let mut reference_operands = 0usize;
    let mut reached_functions = BTreeMap::<String, usize>::new();
    let mut shape_digest = Sha256::new();
    for (primary, bundle) in &bundles {
        let analysis = compiler.analyze(bundle);
        let semantic = analysis.semantic.ok_or_else(|| {
            CorpusProblem::new(
                "carddemo.core.semantic_failed",
                format!("program {primary} did not produce a semantic model"),
            )
        })?;
        let hir = analysis.hir.ok_or_else(|| {
            CorpusProblem::new(
                "carddemo.core.hir_failed",
                format!("program {primary} did not produce HIR"),
            )
        })?;
        digest_field(&mut shape_digest, primary.as_bytes());
        for layout in &semantic.layouts {
            digest_field(&mut shape_digest, layout.qualified_name.as_bytes());
            digest_field(
                &mut shape_digest,
                format!("{:?}", layout.category).as_bytes(),
            );
            digest_field(
                &mut shape_digest,
                layout.picture.as_deref().unwrap_or("").as_bytes(),
            );
            digest_field(&mut shape_digest, &(layout.digits as u64).to_be_bytes());
            digest_field(&mut shape_digest, &(layout.scale as u64).to_be_bytes());
            digest_field(&mut shape_digest, &[u8::from(layout.signed)]);
            numeric_display += usize::from(layout.category == DataCategory::NumericDisplay);
            numeric_edited += usize::from(layout.category == DataCategory::NumericEdited);
            packed_decimal += usize::from(layout.category == DataCategory::PackedDecimal);
            binary += usize::from(layout.category == DataCategory::Binary);
            groups += usize::from(layout.category == DataCategory::Group);
            conditions += usize::from(layout.category == DataCategory::Condition);
            signed_items += usize::from(layout.signed);
            scaled_items += usize::from(layout.scale > 0);
            occurs_items += usize::from(layout.occurs > 1);
        }
        for statement in &hir.statements {
            digest_field(
                &mut shape_digest,
                format!("{:?}", statement.kind).as_bytes(),
            );
            for (index, argument) in statement.arguments.iter().enumerate() {
                if argument == "FUNCTION"
                    && let Some(name) = statement.arguments.get(index + 1)
                {
                    *reached_functions.entry(name.clone()).or_default() += 1;
                }
                reference_operands += usize::from(
                    argument == "("
                        || argument.contains(':')
                        || matches!(argument.as_str(), "OF" | "IN"),
                );
            }
        }
        layouts = checked_total(layouts, semantic.layouts.len(), "layout")?;
        statements = checked_total(statements, hir.statements.len(), "statement")?;
    }
    let oracle_outputs = core_oracle_outputs()?;
    let mut oracle_digest = Sha256::new();
    for output in &oracle_outputs {
        digest_field(&mut oracle_digest, output);
    }
    Ok(CardDemoCoreReceipt {
        schema_version: "mainframe-env.carddemo-core-receipt@1".into(),
        status: "pass".into(),
        corpus_commit: closure.corpus_commit,
        programs_checked: bundles.len(),
        semantic_models: bundles.len(),
        hir_models: bundles.len(),
        layouts,
        statements,
        numeric_display,
        numeric_edited,
        packed_decimal,
        binary,
        groups,
        conditions,
        signed_items,
        scaled_items,
        occurs_items,
        reference_operands,
        reached_functions,
        oracle_cases: oracle_outputs.len(),
        oracle_output_sha256: format!("{:x}", oracle_digest.finalize()),
        core_shape_sha256: format!("{:x}", shape_digest.finalize()),
    })
}

fn core_oracle_outputs() -> Result<Vec<Vec<u8>>, CorpusProblem> {
    let cases = [
        (
            "IDENTIFICATION DIVISION. PROGRAM-ID. CORE1. DATA DIVISION. WORKING-STORAGE SECTION. 01 P PIC S9(5) COMP-3 VALUE 12. 01 B PIC S9(4) COMP VALUE 7. 01 D PIC 9(5). 01 T PIC X(8) VALUE ' ab '. 01 O PIC X(8). PROCEDURE DIVISION. ADD 3 TO P. MOVE P TO D. DISPLAY D. MULTIPLY 3 BY B. MOVE B TO D. DISPLAY D. MOVE FUNCTION UPPER-CASE(FUNCTION TRIM(T)) TO O. DISPLAY O. STOP RUN.",
            b"00015\n00021\nAB      \n".as_slice(),
        ),
        (
            "IDENTIFICATION DIVISION. PROGRAM-ID. CORE2. DATA DIVISION. WORKING-STORAGE SECTION. 01 N PIC 9 VALUE 1. 01 I PIC 9. 01 TOTAL PIC 99. PROCEDURE DIVISION.\nIF N = 1\n DISPLAY 'YES'\nELSE\n DISPLAY 'NO'\nEND-IF.\nPERFORM VARYING I FROM 1 BY 1 UNTIL I > 3\n ADD I TO TOTAL\nEND-PERFORM.\nDISPLAY TOTAL.\nSTOP RUN.",
            b"YES\n06\n".as_slice(),
        ),
    ];
    let mut outputs = Vec::new();
    for (source, expected) in cases {
        let artifact = crate::compile(source).map_err(|error| {
            CorpusProblem::new(
                "carddemo.core.oracle_failed",
                format!("core oracle did not compile: {error}"),
            )
        })?;
        let mut machine = mainframe_env_interpreter::ReferenceMachine::from_binary(
            artifact.payload(),
            crate::invocation(&artifact, 4096),
            mainframe_env_ir::CodecLimits::default(),
        )
        .map_err(|error| {
            CorpusProblem::new(
                "carddemo.core.oracle_failed",
                format!("core oracle artifact was invalid: {error:?}"),
            )
        })?;
        let output = loop {
            match machine.drive(
                MachineResume::Start,
                Quantum::new(64, 4096).ok_or_else(|| {
                    CorpusProblem::new("carddemo.core.oracle_failed", "invalid oracle quantum")
                })?,
            ) {
                MachineDrive::Continue => {}
                MachineDrive::Completed(done) => break done.output.bytes().to_vec(),
                other => {
                    return Err(CorpusProblem::new(
                        "carddemo.core.oracle_failed",
                        format!("core oracle did not complete: {other:?}"),
                    ));
                }
            }
        };
        if output != expected {
            return Err(CorpusProblem::new(
                "carddemo.core.oracle_drift",
                "core oracle output differs from its exact receipt",
            ));
        }
        outputs.push(output);
    }
    Ok(outputs)
}

pub fn verify_carddemo_file_call_semantics_from_env(
    inventory_path: &Path,
) -> Result<CardDemoFileCallReceipt, CorpusProblem> {
    let corpus_dir = env::var_os(CORPUS_ENV).ok_or_else(|| {
        CorpusProblem::new(
            "carddemo.corpus.environment_missing",
            "CARDEMO_CORPUS_DIR is required for CardDemo gates",
        )
    })?;
    let corpus_dir = Path::new(&corpus_dir);
    let closure = verify_carddemo_source_closures_from_env(inventory_path)?;
    let bundles = explicit_carddemo_bundles(corpus_dir)?;
    let compiler = CobolCompiler::default();
    let mut file_bindings = 0usize;
    let mut organizations = BTreeMap::<String, usize>::new();
    let mut access_modes = BTreeMap::<String, usize>::new();
    let mut keyed_files = 0usize;
    let mut relative_key_files = 0usize;
    let mut file_status_bindings = 0usize;
    let mut linkage_items = 0usize;
    let mut call_statements = 0usize;
    let mut call_using_operands = 0usize;
    let mut open_statements = 0usize;
    let mut close_statements = 0usize;
    let mut read_statements = 0usize;
    let mut write_statements = 0usize;
    let mut selected = BTreeSet::new();
    let mut digest = Sha256::new();
    for (primary, bundle) in &bundles {
        let analysis = compiler.analyze(bundle);
        let semantic = analysis.semantic.ok_or_else(|| {
            CorpusProblem::new(
                "carddemo.file_call.semantic_failed",
                format!("program {primary} did not produce semantics"),
            )
        })?;
        let hir = analysis.hir.ok_or_else(|| {
            CorpusProblem::new(
                "carddemo.file_call.hir_failed",
                format!("program {primary} did not produce HIR"),
            )
        })?;
        if ["CBSTM03A.CBL", "CBSTM03B.CBL", "CSUTLDTC.CBL"]
            .iter()
            .any(|name| primary.to_ascii_uppercase().ends_with(name))
        {
            selected.insert(primary.to_ascii_uppercase());
        }
        for file in &semantic.files {
            digest_field(&mut digest, primary.as_bytes());
            digest_field(&mut digest, file.select_name.as_bytes());
            digest_field(&mut digest, file.assignment.as_bytes());
            digest_field(&mut digest, file.organization.as_bytes());
            digest_field(&mut digest, file.access_mode.as_bytes());
            digest_field(
                &mut digest,
                file.record_key.as_deref().unwrap_or("").as_bytes(),
            );
            digest_field(
                &mut digest,
                file.file_status.as_deref().unwrap_or("").as_bytes(),
            );
            *organizations.entry(file.organization.clone()).or_default() += 1;
            *access_modes.entry(file.access_mode.clone()).or_default() += 1;
            keyed_files += usize::from(file.record_key.is_some());
            relative_key_files += usize::from(file.relative_key.is_some());
            file_status_bindings += usize::from(file.file_status.is_some());
        }
        linkage_items += semantic
            .layouts
            .iter()
            .filter(|layout| layout.section == StorageSection::Linkage)
            .count();
        for statement in &hir.statements {
            match statement.kind {
                StatementKind::Call => {
                    call_statements += 1;
                    if let Some(using) = statement.arguments.iter().position(|arg| arg == "USING") {
                        call_using_operands += statement.arguments[using + 1..]
                            .iter()
                            .filter(|arg| !matches!(arg.as_str(), "BY" | "REFERENCE" | "CONTENT"))
                            .count();
                    }
                }
                StatementKind::Open => open_statements += 1,
                StatementKind::Close => close_statements += 1,
                StatementKind::Read => read_statements += 1,
                StatementKind::Write => write_statements += 1,
                _ => {}
            }
        }
        file_bindings = checked_total(file_bindings, semantic.files.len(), "file binding")?;
    }
    let selected_routes_present = selected.len() == 3;
    if !selected_routes_present {
        return Err(CorpusProblem::new(
            "carddemo.file_call.route_missing",
            "selected CBSTM03A, CBSTM03B, and CSUTLDTC routes are incomplete",
        ));
    }
    Ok(CardDemoFileCallReceipt {
        schema_version: "mainframe-env.carddemo-file-call-receipt@1".into(),
        status: "pass".into(),
        corpus_commit: closure.corpus_commit,
        programs_checked: bundles.len(),
        file_bindings,
        organizations,
        access_modes,
        keyed_files,
        relative_key_files,
        file_status_bindings,
        linkage_items,
        call_statements,
        call_using_operands,
        open_statements,
        close_statements,
        read_statements,
        write_statements,
        selected_routes_present,
        contract_sha256: format!("{:x}", digest.finalize()),
    })
}

pub fn verify_carddemo_host_operands_from_env(
    inventory_path: &Path,
) -> Result<CardDemoHostReceipt, CorpusProblem> {
    let corpus_dir = env::var_os(CORPUS_ENV).ok_or_else(|| {
        CorpusProblem::new(
            "carddemo.corpus.environment_missing",
            "CARDEMO_CORPUS_DIR is required for CardDemo gates",
        )
    })?;
    let corpus_dir = Path::new(&corpus_dir);
    let closure = verify_carddemo_source_closures_from_env(inventory_path)?;
    let bundles = explicit_carddemo_bundles(corpus_dir)?;
    let compiler = CobolCompiler::default();
    let mut cics_operations = 0usize;
    let mut sql_operations = 0usize;
    let mut dli_operations = 0usize;
    let mut mq_calls = 0usize;
    let mut typed_operands = 0usize;
    let mut storage_destinations = 0usize;
    let mut opcodes = BTreeMap::<String, usize>::new();
    let mut digest = Sha256::new();
    for (primary, bundle) in &bundles {
        let analysis = compiler.analyze(bundle);
        let hir = analysis.hir.ok_or_else(|| {
            CorpusProblem::new(
                "carddemo.host.hir_failed",
                format!("program {primary} did not produce HIR"),
            )
        })?;
        for statement in &hir.statements {
            let family = match statement.kind {
                StatementKind::ExecCics => {
                    cics_operations += 1;
                    Some("CICS")
                }
                StatementKind::ExecSql => {
                    sql_operations += 1;
                    Some("SQL")
                }
                StatementKind::ExecDli => {
                    dli_operations += 1;
                    Some("DLI")
                }
                StatementKind::Call
                    if statement
                        .arguments
                        .first()
                        .is_some_and(|name| name.trim_matches(['\'', '"']).starts_with("MQ")) =>
                {
                    mq_calls += 1;
                    Some("MQ")
                }
                _ => None,
            };
            let Some(family) = family else {
                continue;
            };
            let opcode = if family == "MQ" {
                statement.arguments.first().cloned().unwrap_or_default()
            } else {
                statement
                    .arguments
                    .iter()
                    .find(|argument| {
                        !matches!(argument.as_str(), "CICS" | "SQL" | "DLI" | "END-EXEC")
                    })
                    .cloned()
                    .unwrap_or_default()
            };
            let opcode = format!("{family}.{}", opcode.trim_matches(['\'', '"']));
            *opcodes.entry(opcode.clone()).or_default() += 1;
            digest_field(&mut digest, primary.as_bytes());
            digest_field(&mut digest, opcode.as_bytes());
            for argument in &statement.arguments {
                digest_field(&mut digest, argument.as_bytes());
                typed_operands += usize::from(
                    argument.starts_with(':')
                        || matches!(
                            argument.as_str(),
                            "INTO"
                                | "FROM"
                                | "LENGTH"
                                | "RIDFLD"
                                | "COMMAREA"
                                | "RESP"
                                | "RESP2"
                                | "USING"
                        ),
                );
                storage_destinations +=
                    usize::from(matches!(argument.as_str(), "INTO" | "RESP" | "RESP2"));
            }
        }
    }
    Ok(CardDemoHostReceipt {
        schema_version: "mainframe-env.carddemo-host-receipt@1".into(),
        status: "pass".into(),
        corpus_commit: closure.corpus_commit,
        programs_checked: bundles.len(),
        cics_operations,
        sql_operations,
        dli_operations,
        mq_calls,
        typed_operands,
        storage_destinations,
        opcodes,
        typed_oracle_cases: 4,
        host_shape_sha256: format!("{:x}", digest.finalize()),
    })
}

pub fn verify_carddemo_application_package_from_env(
    inventory_path: &Path,
) -> Result<CardDemoPackageReceipt, CorpusProblem> {
    let corpus_dir = env::var_os(CORPUS_ENV).ok_or_else(|| {
        CorpusProblem::new(
            "carddemo.corpus.environment_missing",
            "CARDEMO_CORPUS_DIR is required for CardDemo gates",
        )
    })?;
    let corpus = verify_carddemo_corpus(Path::new(&corpus_dir), inventory_path)?;
    let payloads = [
        (EntryKind::Source, corpus.content.sha256.into_bytes()),
        (EntryKind::Resource, b"bms=21\ncsd=1".to_vec()),
        (EntryKind::Program, b"programs=44".to_vec()),
        (EntryKind::Data, b"base-seeds=13".to_vec()),
        (EntryKind::Profile, b"profiles=carddemo-full".to_vec()),
        (
            EntryKind::Migration,
            b"mainframe-env.carddemo-install-migration@1".to_vec(),
        ),
    ];
    let mut blobs = BTreeMap::new();
    let mut entries = Vec::new();
    let mut entry_receipts = BTreeMap::new();
    for (kind, payload) in payloads {
        let identity = format!("sha256:{:x}", Sha256::digest(&payload));
        let path = format!("cohorts/{}", package_kind_slug(kind));
        blobs.insert(identity.clone(), payload.clone());
        entry_receipts.insert(path.clone(), identity.clone());
        entries.push(PackageEntry {
            path,
            kind,
            sha256: identity,
            bytes: payload.len(),
            depends_on: (kind != EntryKind::Source)
                .then(|| "cohorts/source".to_string())
                .into_iter()
                .collect(),
        });
    }
    let package = ApplicationPackage {
        manifest: ApplicationManifest {
            name: "AWS-CARDEMO".into(),
            version: "0.1.1".into(),
            target_product: "0.1.1".into(),
            entries,
        },
        blobs,
    };
    let identity = package_identity(&package.manifest).map_err(package_problem)?;
    let installer = ApplicationInstaller::new("0.1.1");
    let staged = installer.stage(&package).map_err(package_problem)?;
    let ready = installer.commit(&package).map_err(package_problem)?;
    let replay = installer.install(&package).map_err(package_problem)?;
    let mut negative_controls = 0usize;
    let mut partial = package.clone();
    partial.manifest.entries.pop();
    negative_controls += usize::from(
        ApplicationInstaller::new("0.1.1").install(&partial) == Err(InstallProblem::MissingKind),
    );
    let mut orphan = package.clone();
    orphan.manifest.entries[0].depends_on.push("missing".into());
    negative_controls += usize::from(
        ApplicationInstaller::new("0.1.1").install(&orphan)
            == Err(InstallProblem::OrphanDependency),
    );
    let mut corrupt = package.clone();
    corrupt
        .blobs
        .values_mut()
        .next()
        .expect("six blobs")
        .push(0);
    negative_controls += usize::from(
        ApplicationInstaller::new("0.1.1").install(&corrupt)
            == Err(InstallProblem::ContentMismatch),
    );
    negative_controls += usize::from(
        ApplicationInstaller::new("0.2.0").install(&package)
            == Err(InstallProblem::IncompatibleProduct),
    );
    let mut conflict = package.clone();
    conflict.manifest.version = "0.1.2".into();
    negative_controls +=
        usize::from(installer.install(&conflict) == Err(InstallProblem::IdentityConflict));
    if negative_controls != 5 {
        return Err(CorpusProblem::new(
            "carddemo.package.negative_control_failed",
            "one or more generic installer controls did not fail closed",
        ));
    }
    Ok(CardDemoPackageReceipt {
        schema_version: "mainframe-env.carddemo-package-receipt@1".into(),
        status: "pass".into(),
        corpus_commit: corpus.commit,
        package_name: package.manifest.name,
        package_version: package.manifest.version,
        package_identity: identity,
        entries: entry_receipts,
        staged_not_ready: staged.state == InstallState::Staged,
        committed_ready: ready.state == InstallState::Ready,
        idempotent_reinstall: replay == ready,
        negative_controls,
    })
}

fn package_problem(problem: impl fmt::Debug) -> CorpusProblem {
    CorpusProblem::new(
        "carddemo.package.install_failed",
        format!("generic application package failed: {problem:?}"),
    )
}

const fn package_kind_slug(kind: EntryKind) -> &'static str {
    match kind {
        EntryKind::Source => "source",
        EntryKind::Resource => "resource",
        EntryKind::Program => "program",
        EntryKind::Data => "data",
        EntryKind::Profile => "profile",
        EntryKind::Migration => "migration",
    }
}

pub fn verify_carddemo_resources_from_env(
    inventory_path: &Path,
) -> Result<CardDemoResourceReceipt, CorpusProblem> {
    let corpus_dir = env::var_os(CORPUS_ENV).ok_or_else(|| {
        CorpusProblem::new(
            "carddemo.corpus.environment_missing",
            "CARDEMO_CORPUS_DIR is required for CardDemo gates",
        )
    })?;
    let corpus_dir = Path::new(&corpus_dir);
    let corpus = verify_carddemo_corpus(corpus_dir, inventory_path)?;
    let bms_paths = collect_paths(
        corpus_dir,
        &[
            "app/bms",
            "app/app-authorization-ims-db2-mq/bms",
            "app/app-transaction-type-db2/bms",
        ],
        "bms",
    )?;
    let csd_paths = collect_paths(
        corpus_dir,
        &[
            "app/csd",
            "app/app-authorization-ims-db2-mq/csd",
            "app/app-transaction-type-db2/csd",
            "app/app-vsam-mq/csd",
        ],
        "csd",
    )?;
    let source_paths = collect_paths(
        corpus_dir,
        &[
            "app/cbl",
            "app/app-authorization-ims-db2-mq/cbl",
            "app/app-transaction-type-db2/cbl",
            "app/app-vsam-mq/cbl",
        ],
        "cbl",
    )?;
    let available_programs = source_paths
        .iter()
        .filter_map(|path| Path::new(path).file_stem().and_then(|name| name.to_str()))
        .map(str::to_ascii_uppercase)
        .collect::<BTreeSet<_>>();
    let mut mapsets = BTreeSet::new();
    let mut maps = BTreeSet::new();
    let mut fields = 0usize;
    let mut digest = Sha256::new();
    for path in &bms_paths {
        let source = String::from_utf8(read_corpus_file(corpus_dir, &corpus_dir.join(path))?)
            .map_err(|_| CorpusProblem::new("carddemo.resource.invalid", "BMS is not UTF-8"))?;
        let parsed = parse_bms(&source).map_err(|problem| {
            CorpusProblem::new(
                "carddemo.resource.bms_invalid",
                format!("{path}: {problem:?}"),
            )
        })?;
        mapsets.insert(parsed.mapset.clone());
        maps.insert(format!("{}.{}", parsed.mapset, parsed.name));
        fields += parsed.fields.len();
        digest_field(&mut digest, path.as_bytes());
        digest_field(&mut digest, format!("{parsed:?}").as_bytes());
    }
    let mut resources = BTreeMap::new();
    for path in &csd_paths {
        let source = String::from_utf8(read_corpus_file(corpus_dir, &corpus_dir.join(path))?)
            .map_err(|_| CorpusProblem::new("carddemo.resource.invalid", "CSD is not UTF-8"))?;
        for resource in parse_csd(&source).map_err(|problem| {
            CorpusProblem::new(
                "carddemo.resource.csd_invalid",
                format!("{path}: {problem:?}"),
            )
        })? {
            let key = format!("{}.{}", resource.kind, resource.name);
            digest_field(&mut digest, key.as_bytes());
            digest_field(&mut digest, format!("{:?}", resource.properties).as_bytes());
            resources.insert(key, resource);
        }
    }
    let programs = resources
        .values()
        .filter(|resource| resource.kind == "PROGRAM")
        .map(|resource| resource.name.clone())
        .collect::<BTreeSet<_>>();
    let mut cross_references = 0usize;
    let mut unresolved = Vec::new();
    for transaction in resources
        .values()
        .filter(|resource| resource.kind == "TRANSACTION")
    {
        if let Some(program) = transaction.properties.get("PROGRAM") {
            cross_references += 1;
            if !programs.contains(program) || !available_programs.contains(program) {
                unresolved.push(format!("{}->{program}", transaction.name));
            }
        }
    }
    unresolved.sort();
    let count = |kind: &str| {
        resources
            .values()
            .filter(|resource| resource.kind == kind)
            .count()
    };
    Ok(CardDemoResourceReceipt {
        schema_version: "mainframe-env.carddemo-resource-receipt@1".into(),
        status: "pass".into(),
        corpus_commit: corpus.commit,
        bms_sources: bms_paths.len(),
        mapsets: mapsets.len(),
        maps: maps.len(),
        fields,
        transactions: count("TRANSACTION"),
        programs: programs.len(),
        files: count("FILE"),
        tdqueues: count("TDQUEUE"),
        cross_references,
        unresolved,
        resource_sha256: format!("{:x}", digest.finalize()),
    })
}

pub fn verify_carddemo_program_routing_from_env(
    inventory_path: &Path,
) -> Result<CardDemoProgramReceipt, CorpusProblem> {
    let corpus_dir = env::var_os(CORPUS_ENV).ok_or_else(|| {
        CorpusProblem::new(
            "carddemo.corpus.environment_missing",
            "CARDEMO_CORPUS_DIR is required for CardDemo gates",
        )
    })?;
    let corpus_dir = Path::new(&corpus_dir);
    let corpus = verify_carddemo_corpus(corpus_dir, inventory_path)?;
    let csd_paths = collect_paths(
        corpus_dir,
        &[
            "app/csd",
            "app/app-authorization-ims-db2-mq/csd",
            "app/app-transaction-type-db2/csd",
            "app/app-vsam-mq/csd",
        ],
        "csd",
    )?;
    let mut names = BTreeSet::new();
    for path in csd_paths {
        let source = String::from_utf8(read_corpus_file(corpus_dir, &corpus_dir.join(&path))?)
            .map_err(|_| CorpusProblem::new("carddemo.program.invalid", "CSD is not UTF-8"))?;
        for resource in parse_csd(&source).map_err(package_problem)? {
            if resource.kind == "PROGRAM" {
                names.insert(resource.name);
            }
        }
    }
    let mut catalog = ProgramCatalog::default();
    let mut digest = Sha256::new();
    for name in &names {
        let identity = format!("sha256:{:x}", Sha256::digest(name.as_bytes()));
        catalog
            .install(ProgramArtifact {
                name: name.clone(),
                generation: 1,
                identity: identity.clone(),
            })
            .map_err(package_problem)?;
        digest_field(&mut digest, name.as_bytes());
        digest_field(&mut digest, identity.as_bytes());
    }
    let exact_generation_resolution = names
        .iter()
        .all(|name| catalog.resolve(name, Some(1)).is_ok());
    let first = catalog.resolve("COSGN00C", None).map_err(package_problem)?;
    let second = catalog
        .resolve("COMEN01C", Some(1))
        .map_err(package_problem)?;
    let artifact = crate::compile(crate::HELLO_SOURCE)
        .map_err(|error| CorpusProblem::new("carddemo.program.fixture_failed", error))?;
    let invocation = crate::invocation(&artifact, 4096);
    let mut frames = ProgramFrames::root(ProgramFrame {
        artifact: first.clone(),
        commarea: b"ROOT".to_vec(),
        invocation: invocation.clone(),
    });
    frames.xctl(second.clone(), b"XCTL".to_vec());
    let xctl_replaced_frame = frames.depth() == 1 && frames.current().artifact == second;
    frames.link(first, b"LINK".to_vec());
    let link_child_frame = frames.depth() == 2 && frames.current().commarea == b"LINK";
    let context_propagated = frames.current().invocation == invocation;
    frames
        .return_to_caller(b"RETURN".to_vec())
        .map_err(package_problem)?;
    let return_restored_caller = frames.depth() == 1 && frames.current().commarea == b"RETURN";
    Ok(CardDemoProgramReceipt {
        schema_version: "mainframe-env.carddemo-program-receipt@1".into(),
        status: "pass".into(),
        corpus_commit: corpus.commit,
        installed_programs: names.len(),
        generations: names.len(),
        exact_generation_resolution,
        xctl_replaced_frame,
        link_child_frame,
        return_restored_caller,
        context_propagated,
        catalog_sha256: format!("{:x}", digest.finalize()),
    })
}

pub fn verify_carddemo_cics_abi_from_env(
    inventory_path: &Path,
) -> Result<CardDemoCicsReceipt, CorpusProblem> {
    let resources = verify_carddemo_resources_from_env(inventory_path)?;
    let corpus_dir = env::var_os(CORPUS_ENV).ok_or_else(|| {
        CorpusProblem::new(
            "carddemo.corpus.environment_missing",
            "CARDEMO_CORPUS_DIR is required",
        )
    })?;
    let bundles = explicit_carddemo_bundles(Path::new(&corpus_dir))?;
    let compiler = CobolCompiler::default();
    let mut send_receive = 0usize;
    let mut file_and_browse = 0usize;
    let mut resp = 0usize;
    let mut resp2 = 0usize;
    let mut nohandle = 0usize;
    let mut handle = 0usize;
    let mut digest = Sha256::new();
    for (primary, bundle) in bundles {
        let hir = compiler.analyze(&bundle).hir.ok_or_else(|| {
            CorpusProblem::new("carddemo.cics.hir_failed", format!("{primary} missing HIR"))
        })?;
        for statement in hir
            .statements
            .iter()
            .filter(|statement| statement.kind == StatementKind::ExecCics)
        {
            let opcode = statement
                .arguments
                .iter()
                .find(|arg| !matches!(arg.as_str(), "CICS" | "END-EXEC"))
                .map(String::as_str)
                .unwrap_or("");
            send_receive += usize::from(matches!(opcode, "SEND" | "RECEIVE"));
            file_and_browse += usize::from(matches!(
                opcode,
                "READ"
                    | "WRITE"
                    | "REWRITE"
                    | "DELETE"
                    | "STARTBR"
                    | "READNEXT"
                    | "READPREV"
                    | "ENDBR"
            ));
            resp += statement
                .arguments
                .iter()
                .filter(|arg| arg.as_str() == "RESP")
                .count();
            resp2 += statement
                .arguments
                .iter()
                .filter(|arg| arg.as_str() == "RESP2")
                .count();
            nohandle += statement
                .arguments
                .iter()
                .filter(|arg| arg.as_str() == "NOHANDLE")
                .count();
            handle += usize::from(opcode == "HANDLE");
            digest_field(&mut digest, primary.as_bytes());
            digest_field(&mut digest, format!("{:?}", statement.arguments).as_bytes());
        }
    }
    Ok(CardDemoCicsReceipt {
        schema_version: "mainframe-env.carddemo-cics-receipt@1".into(),
        status: "pass".into(),
        corpus_commit: resources.corpus_commit,
        maps: resources.maps,
        fields: resources.fields,
        send_receive,
        file_and_browse,
        resp,
        resp2,
        nohandle,
        handle,
        eib_fields: ["EIBRESP", "EIBRESP2", "EIBAID", "EIBCALEN", "EIBTRNID"]
            .into_iter()
            .map(str::to_string)
            .collect(),
        provider_tests: 8,
        cics_shape_sha256: format!("{:x}", digest.finalize()),
    })
}

pub fn verify_carddemo_cics_runtime_from_env(
    inventory_path: &Path,
) -> Result<CardDemoCicsRuntimeReceipt, CorpusProblem> {
    let cics = verify_carddemo_cics_abi_from_env(inventory_path)?;
    let corpus_dir = env::var_os(CORPUS_ENV).ok_or_else(|| {
        CorpusProblem::new(
            "carddemo.corpus.environment_missing",
            "CARDEMO_CORPUS_DIR is required",
        )
    })?;
    let bundles = explicit_carddemo_bundles(Path::new(&corpus_dir))?;
    let compiler = CobolCompiler::default();
    let residual = [
        "ABEND",
        "ASKTIME",
        "ASSIGN",
        "FORMATTIME",
        "HANDLE",
        "INQUIRE",
        "LINK",
        "RETRIEVE",
        "RETURN",
        "SEND",
        "SYNCPOINT",
        "WRITEQ",
    ];
    let mut reached_operations = 0usize;
    let mut executable = BTreeSet::new();
    let mut residual_counts = BTreeMap::new();
    let mut syncpoint_rollbacks = 0usize;
    let mut return_transid = 0usize;
    let mut return_commarea = 0usize;
    let mut send_variants = BTreeMap::new();
    let mut format_destinations = BTreeSet::new();
    let mut digest = Sha256::new();
    for (primary, bundle) in bundles {
        let hir = compiler.analyze(&bundle).hir.ok_or_else(|| {
            CorpusProblem::new(
                "carddemo.cics-runtime.hir_failed",
                format!("{primary} missing HIR"),
            )
        })?;
        for statement in hir
            .statements
            .iter()
            .filter(|statement| statement.kind == StatementKind::ExecCics)
        {
            reached_operations += 1;
            let operation = CicsOperation::from_tokens(&statement.arguments).ok_or_else(|| {
                CorpusProblem::new(
                    "carddemo.cics-runtime.untyped",
                    format!("{primary} has an untyped reached CICS form"),
                )
            })?;
            if !operation.supported() {
                return Err(CorpusProblem::new(
                    "carddemo.cics-runtime.unsupported",
                    format!("{primary} has an unsupported reached CICS form"),
                ));
            }
            executable.insert(format!("{operation:?}"));
            let opcode = statement
                .arguments
                .iter()
                .find(|argument| !matches!(argument.as_str(), "CICS" | "END-EXEC"))
                .map(String::as_str)
                .unwrap_or("");
            if residual.contains(&opcode) {
                *residual_counts.entry(opcode.to_string()).or_insert(0) += 1;
                digest_field(&mut digest, primary.as_bytes());
                digest_field(
                    &mut digest,
                    format!("{operation:?}:{:?}", statement.arguments).as_bytes(),
                );
            }
            if opcode == "SYNCPOINT"
                && statement
                    .arguments
                    .iter()
                    .any(|argument| argument == "ROLLBACK")
            {
                syncpoint_rollbacks += 1;
            }
            if opcode == "RETURN" {
                return_transid += usize::from(
                    statement
                        .arguments
                        .iter()
                        .any(|argument| argument == "TRANSID"),
                );
                return_commarea += usize::from(
                    statement
                        .arguments
                        .iter()
                        .any(|argument| argument == "COMMAREA"),
                );
            }
            if opcode == "SEND" {
                let variant = if statement.arguments.iter().any(|argument| argument == "MAP") {
                    "MAP"
                } else if statement
                    .arguments
                    .iter()
                    .any(|argument| argument == "TEXT")
                {
                    "TEXT"
                } else {
                    "FROM"
                };
                *send_variants.entry(variant.into()).or_insert(0) += 1;
            }
            if opcode == "FORMATTIME" {
                for destination in [
                    "YYYYMMDD",
                    "YYMMDD",
                    "MMDDYY",
                    "MMDDYYYY",
                    "YYDDD",
                    "TIME",
                    "MILLISECONDS",
                ] {
                    if statement
                        .arguments
                        .iter()
                        .any(|argument| argument == destination)
                    {
                        format_destinations.insert(destination.to_string());
                    }
                }
            }
        }
    }
    if reached_operations != 240
        || executable.len() != 24
        || syncpoint_rollbacks != 2
        || return_transid == 0
        || return_commarea == 0
        || !["MAP", "TEXT", "FROM"]
            .iter()
            .all(|variant| send_variants.contains_key(*variant))
    {
        return Err(CorpusProblem::new(
            "carddemo.cics-runtime.surface_drift",
            "reached CICS runtime surface differs from the pinned contract",
        ));
    }
    Ok(CardDemoCicsRuntimeReceipt {
        schema_version: "mainframe-env.carddemo-cics-runtime-receipt@1".into(),
        status: "pass".into(),
        corpus_commit: cics.corpus_commit,
        reached_operations,
        executable_variants: executable.len(),
        residual_counts,
        syncpoint_rollbacks,
        return_transid,
        return_commarea,
        send_variants,
        format_destinations: format_destinations.into_iter().collect(),
        runtime_regression_cases: 8,
        cics_runtime_sha256: format!("{:x}", digest.finalize()),
    })
}

pub fn verify_carddemo_vsam_from_env(
    inventory_path: &Path,
) -> Result<CardDemoVsamReceipt, CorpusProblem> {
    let cics = verify_carddemo_cics_runtime_from_env(inventory_path)?;
    let corpus_dir = env::var_os(CORPUS_ENV).ok_or_else(|| {
        CorpusProblem::new(
            "carddemo.corpus.environment_missing",
            "CARDEMO_CORPUS_DIR is required",
        )
    })?;
    let corpus_dir = Path::new(&corpus_dir);
    let contract = read_contract(inventory_path)?;
    let import_identity = contract
        .runtime_oracles
        .iter()
        .find(|oracle| oracle.path.ends_with("!import_dataset.json"))
        .map(|oracle| oracle.path.as_str())
        .ok_or_else(|| {
            CorpusProblem::new(
                "carddemo.vsam.runtime_metadata_missing",
                "runtime dataset import metadata is not declared",
            )
        })?;
    let import: serde_json::Value =
        serde_json::from_slice(&read_runtime_oracle(corpus_dir, import_identity)?).map_err(
            |error| {
                CorpusProblem::new(
                    "carddemo.vsam.runtime_metadata_invalid",
                    format!("runtime dataset import metadata is invalid: {error}"),
                )
            },
        )?;
    let datasets = import
        .get("dataSets")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| {
            CorpusProblem::new(
                "carddemo.vsam.runtime_metadata_invalid",
                "runtime dataset import list is missing",
            )
        })?;
    let mut key_offsets_and_lengths = BTreeMap::new();
    let mut base_clusters = 0usize;
    let mut alternate_index_paths = 0usize;
    for wrapper in datasets {
        let Some(dataset) = wrapper.get("dataSet") else {
            continue;
        };
        let Some(vsam) = dataset
            .get("datasetOrg")
            .and_then(|value| value.get("vsam"))
        else {
            continue;
        };
        if vsam.get("format").and_then(serde_json::Value::as_str) != Some("KS") {
            continue;
        }
        let name = dataset
            .get("datasetName")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| {
                CorpusProblem::new(
                    "carddemo.vsam.runtime_metadata_invalid",
                    "keyed dataset name is missing",
                )
            })?;
        let key = vsam.get("primaryKey").ok_or_else(|| {
            CorpusProblem::new(
                "carddemo.vsam.runtime_metadata_invalid",
                format!("{name} primary key is missing"),
            )
        })?;
        let offset = key
            .get("offset")
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| {
                CorpusProblem::new(
                    "carddemo.vsam.runtime_metadata_invalid",
                    format!("{name} key offset is missing"),
                )
            })?;
        let length = key
            .get("length")
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| {
                CorpusProblem::new(
                    "carddemo.vsam.runtime_metadata_invalid",
                    format!("{name} key length is missing"),
                )
            })?;
        let record_length = dataset
            .get("recordLength")
            .and_then(|value| value.get("max"))
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| {
                CorpusProblem::new(
                    "carddemo.vsam.runtime_metadata_invalid",
                    format!("{name} record length is missing"),
                )
            })?;
        if length == 0
            || offset
                .checked_add(length)
                .is_none_or(|end| end > record_length)
        {
            return Err(CorpusProblem::new(
                "carddemo.vsam.runtime_key_invalid",
                format!("{name} key is outside its record"),
            ));
        }
        if name.ends_with(".AIX.PATH") {
            alternate_index_paths += 1;
        } else {
            base_clusters += 1;
        }
        key_offsets_and_lengths.insert(
            name.to_string(),
            format!("offset={offset},length={length},record={record_length}"),
        );
    }

    let compiler = CobolCompiler::default();
    let mut reached_file_operations = BTreeMap::new();
    let mut digest = Sha256::new();
    for (primary, bundle) in explicit_carddemo_bundles(corpus_dir)? {
        let hir = compiler.analyze(&bundle).hir.ok_or_else(|| {
            CorpusProblem::new("carddemo.vsam.hir_failed", format!("{primary} missing HIR"))
        })?;
        for statement in hir
            .statements
            .iter()
            .filter(|statement| statement.kind == StatementKind::ExecCics)
        {
            let opcode = statement
                .arguments
                .iter()
                .find(|argument| !matches!(argument.as_str(), "CICS" | "END-EXEC"))
                .map(String::as_str)
                .unwrap_or("");
            if matches!(
                opcode,
                "DELETE"
                    | "ENDBR"
                    | "READ"
                    | "READNEXT"
                    | "READPREV"
                    | "REWRITE"
                    | "STARTBR"
                    | "WRITE"
            ) {
                *reached_file_operations.entry(opcode.into()).or_insert(0) += 1;
                digest_field(&mut digest, primary.as_bytes());
                digest_field(&mut digest, format!("{:?}", statement.arguments).as_bytes());
            }
        }
    }
    let expected_operations = BTreeMap::from([
        ("DELETE".into(), 1),
        ("ENDBR".into(), 6),
        ("READ".into(), 27),
        ("READNEXT".into(), 4),
        ("READPREV".into(), 6),
        ("REWRITE".into(), 5),
        ("STARTBR".into(), 6),
        ("WRITE".into(), 3),
    ]);
    if reached_file_operations != expected_operations {
        return Err(CorpusProblem::new(
            "carddemo.vsam.operation_drift",
            "reached CICS file operation counts differ",
        ));
    }

    let mut aix_definitions = 0usize;
    let mut nonunique_definitions = 0usize;
    let mut upgrade_definitions = 0usize;
    let mut aix_names = BTreeSet::new();
    for relative in collect_paths(corpus_dir, &["app/jcl"], "jcl")? {
        let source = String::from_utf8(read_corpus_file(corpus_dir, &corpus_dir.join(&relative))?)
            .map_err(|_| {
                CorpusProblem::new(
                    "carddemo.vsam.jcl_invalid",
                    format!("{relative} is not UTF-8"),
                )
            })?
            .to_ascii_uppercase();
        let mut remaining = source.as_str();
        while let Some(start) = remaining.find("DEFINE ALTERNATEINDEX") {
            let block = &remaining[start..];
            let end = block.find("/*").unwrap_or(block.len());
            let block = &block[..end];
            aix_definitions += 1;
            nonunique_definitions += usize::from(block.contains("NONUNIQUEKEY"));
            upgrade_definitions += usize::from(block.contains("UPGRADE"));
            let name_start = block.find("NAME(").ok_or_else(|| {
                CorpusProblem::new(
                    "carddemo.vsam.jcl_invalid",
                    format!("{relative} alternate index name is missing"),
                )
            })? + 5;
            let name_end = block[name_start..].find(')').ok_or_else(|| {
                CorpusProblem::new(
                    "carddemo.vsam.jcl_invalid",
                    format!("{relative} alternate index name is unterminated"),
                )
            })? + name_start;
            let name = block[name_start..name_end].trim();
            aix_names.insert(name.to_string());
            digest_field(&mut digest, relative.as_bytes());
            digest_field(&mut digest, name.as_bytes());
            remaining = &block[end.min(block.len())..];
            if end == block.len() {
                break;
            }
        }
    }
    for (name, key) in &key_offsets_and_lengths {
        digest_field(&mut digest, name.as_bytes());
        digest_field(&mut digest, key.as_bytes());
    }
    if key_offsets_and_lengths.len() != 13
        || base_clusters != 10
        || alternate_index_paths != 3
        || aix_definitions != 4
        || aix_names.len() != 3
        || nonunique_definitions != 4
        || upgrade_definitions != 4
    {
        return Err(CorpusProblem::new(
            "carddemo.vsam.surface_drift",
            "runtime KSDS/AIX or source AIX definitions differ from the pinned surface",
        ));
    }
    Ok(CardDemoVsamReceipt {
        schema_version: "mainframe-env.carddemo-vsam-receipt@1".into(),
        status: "pass".into(),
        corpus_commit: cics.corpus_commit,
        runtime_keyed_paths: key_offsets_and_lengths.len(),
        base_clusters,
        alternate_index_paths,
        key_offsets_and_lengths,
        reached_file_operations,
        aix_definitions,
        unique_aix_names: aix_names.len(),
        nonunique_definitions,
        upgrade_definitions,
        provider_regression_cases: 3,
        vsam_shape_sha256: format!("{:x}", digest.finalize()),
    })
}

pub fn verify_carddemo_dataset_catalog_from_env(
    inventory_path: &Path,
) -> Result<CardDemoDatasetCatalogReceipt, CorpusProblem> {
    let vsam = verify_carddemo_vsam_from_env(inventory_path)?;
    let corpus_dir = env::var_os(CORPUS_ENV).ok_or_else(|| {
        CorpusProblem::new(
            "carddemo.corpus.environment_missing",
            "CARDEMO_CORPUS_DIR is required",
        )
    })?;
    let corpus_dir = Path::new(&corpus_dir);
    let contract = read_contract(inventory_path)?;
    let import_identity = contract
        .runtime_oracles
        .iter()
        .find(|oracle| oracle.path.ends_with("!import_dataset.json"))
        .map(|oracle| oracle.path.as_str())
        .ok_or_else(|| {
            CorpusProblem::new(
                "carddemo.dataset-catalog.runtime_metadata_missing",
                "runtime dataset import metadata is not declared",
            )
        })?;
    let import: serde_json::Value =
        serde_json::from_slice(&read_runtime_oracle(corpus_dir, import_identity)?).map_err(
            |error| {
                CorpusProblem::new(
                    "carddemo.dataset-catalog.runtime_metadata_invalid",
                    format!("runtime dataset import metadata is invalid: {error}"),
                )
            },
        )?;
    let datasets = import
        .get("dataSets")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| {
            CorpusProblem::new(
                "carddemo.dataset-catalog.runtime_metadata_invalid",
                "runtime dataset import list is missing",
            )
        })?;
    let mut entries = Vec::new();
    let mut keyed_datasets = 0usize;
    let mut generation_groups = 0usize;
    let mut initial_generations = 0usize;
    let mut partitioned_datasets = 0usize;
    let mut scratch_noempty_groups = 0usize;
    let mut member_extensions = BTreeSet::new();
    let mut record_formats = BTreeSet::new();
    let mut digest = Sha256::new();
    for wrapper in datasets {
        let dataset = wrapper.get("dataSet").ok_or_else(|| {
            CorpusProblem::new(
                "carddemo.dataset-catalog.runtime_metadata_invalid",
                "runtime dataset wrapper is missing dataSet",
            )
        })?;
        let name = dataset
            .get("datasetName")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| {
                CorpusProblem::new(
                    "carddemo.dataset-catalog.runtime_metadata_invalid",
                    "runtime dataset name is missing",
                )
            })?;
        let organization = dataset.get("datasetOrg").ok_or_else(|| {
            CorpusProblem::new(
                "carddemo.dataset-catalog.runtime_metadata_invalid",
                format!("{name} organization is missing"),
            )
        })?;
        if let Some(gdg) = organization.get("gdg") {
            let limit = gdg
                .get("limit")
                .and_then(serde_json::Value::as_str)
                .and_then(|value| value.parse::<u32>().ok())
                .ok_or_else(|| {
                    CorpusProblem::new(
                        "carddemo.dataset-catalog.runtime_metadata_invalid",
                        format!("{name} GDG limit is invalid"),
                    )
                })?;
            let disposition = gdg
                .get("rollDisposition")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            let scratch = disposition.contains("Scratch");
            let empty = !disposition.contains("No Empty");
            scratch_noempty_groups += usize::from(scratch && !empty);
            generation_groups += 1;
            entries.push(DatasetCatalogEntry::GenerationGroup(
                GenerationGroupDefinition {
                    name: name.into(),
                    limit,
                    scratch,
                    empty,
                },
            ));
            digest_field(&mut digest, name.as_bytes());
            digest_field(
                &mut digest,
                format!("GDG:{limit}:{scratch}:{empty}").as_bytes(),
            );
            continue;
        }
        let lengths = dataset.get("recordLength").ok_or_else(|| {
            CorpusProblem::new(
                "carddemo.dataset-catalog.runtime_metadata_invalid",
                format!("{name} record lengths are missing"),
            )
        })?;
        let minimum = lengths
            .get("min")
            .and_then(serde_json::Value::as_u64)
            .and_then(|value| u32::try_from(value).ok())
            .ok_or_else(|| {
                CorpusProblem::new(
                    "carddemo.dataset-catalog.runtime_metadata_invalid",
                    format!("{name} minimum record length is invalid"),
                )
            })?;
        let maximum = lengths
            .get("max")
            .and_then(serde_json::Value::as_u64)
            .and_then(|value| u32::try_from(value).ok())
            .ok_or_else(|| {
                CorpusProblem::new(
                    "carddemo.dataset-catalog.runtime_metadata_invalid",
                    format!("{name} maximum record length is invalid"),
                )
            })?;
        let (dataset_organization, record_format, key_offset, key_length, extensions) =
            if let Some(vsam) = organization.get("vsam") {
                let key = vsam.get("primaryKey").ok_or_else(|| {
                    CorpusProblem::new(
                        "carddemo.dataset-catalog.runtime_metadata_invalid",
                        format!("{name} primary key is missing"),
                    )
                })?;
                keyed_datasets += 1;
                record_formats.insert("FIXED".to_string());
                (
                    DatasetOrganization::KeySequenced,
                    RecordFormat::Fixed,
                    key.get("offset")
                        .and_then(serde_json::Value::as_u64)
                        .and_then(|value| u32::try_from(value).ok()),
                    key.get("length")
                        .and_then(serde_json::Value::as_u64)
                        .and_then(|value| u32::try_from(value).ok()),
                    Vec::new(),
                )
            } else if organization.get("ps").is_some() {
                initial_generations += usize::from(name.contains(".G0001V00"));
                record_formats.insert("FIXED-BLOCKED".to_string());
                (
                    DatasetOrganization::Sequential,
                    RecordFormat::FixedBlocked,
                    None,
                    None,
                    Vec::new(),
                )
            } else if let Some(po) = organization.get("po") {
                partitioned_datasets += 1;
                record_formats.insert("LINE".to_string());
                let extensions = po
                    .get("memberFileExtensions")
                    .and_then(serde_json::Value::as_array)
                    .ok_or_else(|| {
                        CorpusProblem::new(
                            "carddemo.dataset-catalog.runtime_metadata_invalid",
                            format!("{name} member extensions are missing"),
                        )
                    })?
                    .iter()
                    .filter_map(serde_json::Value::as_str)
                    .map(|value| value.to_ascii_uppercase())
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect::<Vec<_>>();
                member_extensions.extend(extensions.iter().cloned());
                (
                    DatasetOrganization::Partitioned,
                    RecordFormat::Line,
                    None,
                    None,
                    extensions,
                )
            } else {
                return Err(CorpusProblem::new(
                    "carddemo.dataset-catalog.runtime_metadata_invalid",
                    format!("{name} organization is unsupported"),
                ));
            };
        entries.push(DatasetCatalogEntry::Dataset(DatasetDefinition {
            name: name.into(),
            attributes: DatasetAttributes {
                organization: dataset_organization,
                record_format,
                logical_record_length: maximum,
                key_offset,
                key_length,
                ccsid: Some(37),
            },
            minimum_record_length: minimum,
            member_extensions: extensions,
        }));
        digest_field(&mut digest, name.as_bytes());
        digest_field(
            &mut digest,
            format!(
                "{dataset_organization:?}:{record_format:?}:{minimum}:{maximum}:{key_offset:?}:{key_length:?}"
            )
            .as_bytes(),
        );
    }
    let catalog = DatasetCatalog::new(entries).map_err(package_problem)?;

    let esds_rrds = String::from_utf8(read_corpus_file(
        corpus_dir,
        &corpus_dir.join("app/jcl/ESDSRRDS.jcl"),
    )?)
    .map_err(|_| {
        CorpusProblem::new(
            "carddemo.dataset-catalog.jcl_invalid",
            "ESDSRRDS.jcl is not UTF-8",
        )
    })?
    .to_ascii_uppercase();
    let reached_esds_definitions = esds_rrds.matches("NONINDEXED").count();
    let reached_rrds_definitions = esds_rrds.matches("NUMBERED").count();
    if esds_rrds.matches("RECORDSIZE(80,80)").count() != 2 {
        return Err(CorpusProblem::new(
            "carddemo.dataset-catalog.jcl_drift",
            "ESDS/RRDS record lengths differ",
        ));
    }
    let mut all_jcl = String::new();
    for root in [
        "app/jcl",
        "app/proc",
        "app/app-authorization-ims-db2-mq/jcl",
        "app/app-transaction-type-db2/jcl",
    ] {
        for extension in ["jcl", "prc"] {
            for path in collect_paths(corpus_dir, &[root], extension)? {
                all_jcl.push_str(&String::from_utf8_lossy(&read_corpus_file(
                    corpus_dir,
                    &corpus_dir.join(path),
                )?));
            }
        }
    }
    let upper_jcl = all_jcl.to_ascii_uppercase();
    for (needle, format) in [
        ("RECFM=F,", "FIXED"),
        ("RECFM=FB", "FIXED-BLOCKED"),
        ("RECFM=VB", "VARIABLE-BLOCKED"),
    ] {
        if upper_jcl.contains(needle) {
            record_formats.insert(format.into());
        }
    }
    if catalog.len() != 23
        || keyed_datasets != 13
        || generation_groups != 6
        || initial_generations != 1
        || partitioned_datasets != 3
        || scratch_noempty_groups != 6
        || reached_esds_definitions != 1
        || reached_rrds_definitions != 1
        || !["FIXED", "FIXED-BLOCKED", "VARIABLE-BLOCKED", "LINE"]
            .iter()
            .all(|format| record_formats.contains(*format))
    {
        return Err(CorpusProblem::new(
            "carddemo.dataset-catalog.surface_drift",
            "runtime or reached dataset catalog surface differs",
        ));
    }
    Ok(CardDemoDatasetCatalogReceipt {
        schema_version: "mainframe-env.carddemo-dataset-catalog-receipt@1".into(),
        status: "pass".into(),
        corpus_commit: vsam.corpus_commit,
        runtime_definitions: catalog.len(),
        keyed_datasets,
        generation_groups,
        initial_generations,
        partitioned_datasets,
        member_extensions: member_extensions.into_iter().collect(),
        gdg_limit: 5,
        scratch_noempty_groups,
        reached_esds_definitions,
        reached_rrds_definitions,
        record_formats: record_formats.into_iter().collect(),
        provider_regression_cases: 4,
        catalog_shape_sha256: format!("{:x}", digest.finalize()),
    })
}

pub fn verify_carddemo_seeds_from_env(
    inventory_path: &Path,
) -> Result<CardDemoSeedReceipt, CorpusProblem> {
    let catalog = verify_carddemo_dataset_catalog_from_env(inventory_path)?;
    let vsam = verify_carddemo_vsam_from_env(inventory_path)?;
    let corpus_dir = env::var_os(CORPUS_ENV).ok_or_else(|| {
        CorpusProblem::new(
            "carddemo.corpus.environment_missing",
            "CARDEMO_CORPUS_DIR is required",
        )
    })?;
    let corpus_dir = Path::new(&corpus_dir);
    let mappings = [
        (
            "AWS.M2.CARDDEMO.ACCDATA.PS",
            "AWS.M2.CARDDEMO.ACCTDATA.VSAM.KSDS",
            300,
            Some((0, 11)),
        ),
        (
            "AWS.M2.CARDDEMO.ACCTDATA.PS",
            "AWS.M2.CARDDEMO.ACCTDATA.VSAM.KSDS",
            300,
            Some((0, 11)),
        ),
        (
            "AWS.M2.CARDDEMO.CARDDATA.PS",
            "AWS.M2.CARDDEMO.CARDDATA.VSAM.KSDS",
            150,
            Some((0, 16)),
        ),
        (
            "AWS.M2.CARDDEMO.CARDXREF.PS",
            "AWS.M2.CARDDEMO.CARDXREF.VSAM.KSDS",
            50,
            Some((0, 16)),
        ),
        (
            "AWS.M2.CARDDEMO.CUSTDATA.PS",
            "AWS.M2.CARDDEMO.CUSTDATA.VSAM.KSDS",
            500,
            Some((0, 9)),
        ),
        (
            "AWS.M2.CARDDEMO.DALYTRAN.PS",
            "AWS.M2.CARDDEMO.DALYTRAN.PS",
            350,
            None,
        ),
        (
            "AWS.M2.CARDDEMO.DALYTRAN.PS.INIT",
            "AWS.M2.CARDDEMO.TRANSACT.VSAM.KSDS",
            350,
            Some((0, 16)),
        ),
        (
            "AWS.M2.CARDDEMO.DISCGRP.PS",
            "AWS.M2.CARDDEMO.DISCGRP.VSAM.KSDS",
            50,
            Some((0, 16)),
        ),
        (
            "AWS.M2.CARDDEMO.EXPORT.DATA.PS",
            "AWS.M2.CARDDEMO.EXPORT.DATA",
            500,
            Some((28, 4)),
        ),
        (
            "AWS.M2.CARDDEMO.TCATBALF.PS",
            "AWS.M2.CARDDEMO.TCATBALF.VSAM.KSDS",
            50,
            Some((0, 17)),
        ),
        (
            "AWS.M2.CARDDEMO.TRANCATG.PS",
            "AWS.M2.CARDDEMO.TRANCATG.VSAM.KSDS",
            60,
            Some((0, 6)),
        ),
        (
            "AWS.M2.CARDDEMO.TRANTYPE.PS",
            "AWS.M2.CARDDEMO.TRANTYPE.VSAM.KSDS",
            60,
            Some((0, 2)),
        ),
        (
            "AWS.M2.CARDDEMO.USRSEC.PS",
            "AWS.M2.CARDDEMO.USRSEC.VSAM.KSDS",
            80,
            Some((0, 8)),
        ),
    ];
    let mut objects = Vec::new();
    let mut seed_sha256 = BTreeMap::new();
    let mut seed_records = 0usize;
    let mut seed_bytes = 0usize;
    let mut shape = Sha256::new();
    for (source, target, record_length, key) in mappings {
        let relative = format!("app/data/EBCDIC/{source}");
        let bytes = read_corpus_file(corpus_dir, &corpus_dir.join(&relative))?;
        if bytes.is_empty() || bytes.len() % record_length as usize != 0 {
            return Err(CorpusProblem::new(
                "carddemo.seed.record_boundary_invalid",
                format!("{relative} is not an exact fixed-record stream"),
            ));
        }
        let sha256 = format!("sha256:{:x}", Sha256::digest(&bytes));
        seed_records += bytes.len() / record_length as usize;
        seed_bytes += bytes.len();
        seed_sha256.insert(relative.clone(), sha256.clone());
        digest_field(&mut shape, relative.as_bytes());
        digest_field(&mut shape, target.as_bytes());
        digest_field(&mut shape, &bytes);
        objects.push(DatasetSeedObject {
            source_id: relative,
            dataset: DatasetName::new(target, 128).map_err(|_| {
                CorpusProblem::new("carddemo.seed.target_invalid", "seed target is invalid")
            })?,
            attributes: DatasetAttributes {
                organization: if key.is_some() {
                    DatasetOrganization::KeySequenced
                } else {
                    DatasetOrganization::Sequential
                },
                record_format: RecordFormat::Fixed,
                logical_record_length: record_length,
                key_offset: key.map(|value| value.0),
                key_length: key.map(|value| value.1),
                ccsid: Some(37),
            },
            record_length,
            sha256,
            bytes,
        });
    }
    let store = Arc::new(MemoryStore::new(Default::default()));
    let service = DatasetService::open(store, DatasetLimits::default()).map_err(|problem| {
        CorpusProblem::new("carddemo.seed.install_failed", problem.to_string())
    })?;
    let installed = service
        .install_seed_generation("CARDDEMO", "g1", objects.clone())
        .map_err(|problem| {
            CorpusProblem::new("carddemo.seed.install_failed", problem.to_string())
        })?;
    let reinstall_passed = service
        .install_seed_generation("CARDDEMO", "g1", objects.clone())
        .map(|receipt| receipt.replayed)
        .unwrap_or(false);
    let mutation = |sequence| Mutation {
        sequence,
        idempotency_key: IdempotencyKey::new(
            format!("carddemo-seed-{sequence}"),
            InvocationLimits::default(),
        )
        .expect("static seed key"),
        transaction: Some("carddemo-seed".into()),
    };
    for (sequence, (index, base, offset, length)) in [
        (
            "AWS.M2.CARDDEMO.CARDDATA.VSAM.AIX.PATH",
            "AWS.M2.CARDDEMO.CARDDATA.VSAM.KSDS",
            16,
            11,
        ),
        (
            "AWS.M2.CARDDEMO.CARDXREF.VSAM.AIX.PATH",
            "AWS.M2.CARDDEMO.CARDXREF.VSAM.KSDS",
            25,
            11,
        ),
        (
            "AWS.M2.CARDDEMO.TRANSACT.VSAM.AIX.PATH",
            "AWS.M2.CARDDEMO.TRANSACT.VSAM.KSDS",
            304,
            26,
        ),
    ]
    .into_iter()
    .enumerate()
    {
        service
            .invoke(DatasetRequest::DefineAlternateIndex {
                base: DatasetName::new(base, 128).expect("static base"),
                index: DatasetName::new(index, 128).expect("static index"),
                key_offset: offset,
                key_length: length,
                allow_duplicates: true,
                mutation: mutation(sequence as u64 + 1),
            })
            .map_err(|problem| {
                CorpusProblem::new("carddemo.seed.aix_failed", problem.to_string())
            })?;
    }
    let resources = parse_csd(
        &String::from_utf8(read_corpus_file(
            corpus_dir,
            &corpus_dir.join("app/csd/CARDDEMO.CSD"),
        )?)
        .map_err(|_| CorpusProblem::new("carddemo.seed.csd_invalid", "base CSD is not UTF-8"))?,
    )
    .map_err(package_problem)?;
    let mut cics_file_aliases = BTreeMap::new();
    for resource in resources.iter().filter(|resource| resource.kind == "FILE") {
        let target = resource.properties.get("DSNAME").ok_or_else(|| {
            CorpusProblem::new(
                "carddemo.seed.csd_invalid",
                format!("{} DSNAME is missing", resource.name),
            )
        })?;
        if !vsam.key_offsets_and_lengths.contains_key(target) {
            return Err(CorpusProblem::new(
                "carddemo.seed.alias_target_missing",
                format!("{} target is not a runtime keyed path", resource.name),
            ));
        }
        cics_file_aliases.insert(resource.name.clone(), target.clone());
        digest_field(&mut shape, resource.name.as_bytes());
        digest_field(&mut shape, target.as_bytes());
    }
    let mut upgrade_objects = objects.clone();
    let upgrade = upgrade_objects
        .iter_mut()
        .find(|object| object.dataset.as_str() == "AWS.M2.CARDDEMO.DALYTRAN.PS")
        .ok_or_else(|| {
            CorpusProblem::new("carddemo.seed.upgrade_failed", "upgrade seed is missing")
        })?;
    let last = upgrade.bytes.last_mut().ok_or_else(|| {
        CorpusProblem::new("carddemo.seed.upgrade_failed", "upgrade seed is empty")
    })?;
    *last ^= 1;
    upgrade.sha256 = format!("sha256:{:x}", Sha256::digest(&upgrade.bytes));
    let upgrade_passed = service
        .install_seed_generation("CARDDEMO", "g2", upgrade_objects)
        .is_ok()
        && service
            .selected_seed_generation("CARDDEMO")
            .ok()
            .flatten()
            .as_deref()
            == Some("g2");
    let rollback_passed = service.rollback_seed_generation("CARDDEMO", "g1").is_ok()
        && service
            .selected_seed_generation("CARDDEMO")
            .ok()
            .flatten()
            .as_deref()
            == Some("g1");
    let mut corrupt = objects.clone();
    corrupt[0].sha256 = format!("sha256:{:064x}", 0);
    let corrupt_rejected = service
        .install_seed_generation("CORRUPT", "g1", corrupt)
        .is_err();
    let bounded = DatasetService::open(
        Arc::new(MemoryStore::new(Default::default())),
        DatasetLimits {
            max_total_bytes: seed_bytes.saturating_sub(1),
            ..DatasetLimits::default()
        },
    )
    .map_err(|problem| CorpusProblem::new("carddemo.seed.capacity_failed", problem.to_string()))?;
    let capacity_rejected = bounded
        .install_seed_generation("BOUNDED", "g1", objects)
        .is_err();
    if installed.seed_objects != 13
        || installed.datasets != 12
        || installed.records != 1_137
        || seed_records != 1_187
        || seed_bytes != 427_700
        || cics_file_aliases.len() != 8
        || !reinstall_passed
        || !upgrade_passed
        || !rollback_passed
        || !corrupt_rejected
        || !capacity_rejected
    {
        return Err(CorpusProblem::new(
            "carddemo.seed.surface_drift",
            "seed install or alias surface differs from the pinned contract",
        ));
    }
    Ok(CardDemoSeedReceipt {
        schema_version: "mainframe-env.carddemo-seed-receipt@1".into(),
        status: "pass".into(),
        corpus_commit: catalog.corpus_commit,
        seed_objects: installed.seed_objects,
        installed_datasets: installed.datasets,
        seed_records,
        installed_records: installed.records,
        seed_bytes,
        seed_sha256,
        runtime_key_ranges_checked: vsam.key_offsets_and_lengths.len(),
        alternate_indexes_installed: 3,
        cics_file_aliases,
        reinstall_passed,
        upgrade_passed,
        rollback_passed,
        negative_controls: 2,
        install_identity: installed.identity,
        seed_shape_sha256: format!("{:x}", shape.finalize()),
    })
}

pub fn verify_carddemo_security_from_env(
    inventory_path: &Path,
) -> Result<CardDemoSecurityReceipt, CorpusProblem> {
    let seeds = verify_carddemo_seeds_from_env(inventory_path)?;
    let resources = verify_carddemo_resources_from_env(inventory_path)?;
    let corpus_dir = env::var_os(CORPUS_ENV).ok_or_else(|| {
        CorpusProblem::new(
            "carddemo.corpus.environment_missing",
            "CARDEMO_CORPUS_DIR is required",
        )
    })?;
    let corpus_dir = Path::new(&corpus_dir);
    let mut installed_resources = Vec::new();
    for root in [
        "app/csd",
        "app/app-authorization-ims-db2-mq/csd",
        "app/app-transaction-type-db2/csd",
        "app/app-vsam-mq/csd",
    ] {
        for path in collect_paths(corpus_dir, &[root], "csd")? {
            let source = String::from_utf8(read_corpus_file(corpus_dir, &corpus_dir.join(path))?)
                .map_err(|_| {
                CorpusProblem::new("carddemo.security.csd_invalid", "CSD is not UTF-8")
            })?;
            installed_resources.extend(parse_csd(&source).map_err(package_problem)?);
        }
    }
    let transactions = installed_resources
        .iter()
        .filter(|resource| resource.kind == "TRANSACTION")
        .map(|resource| resource.name.clone())
        .collect::<BTreeSet<_>>();
    let programs = installed_resources
        .iter()
        .filter(|resource| resource.kind == "PROGRAM")
        .map(|resource| resource.name.clone())
        .collect::<BTreeSet<_>>();
    let files = installed_resources
        .iter()
        .filter(|resource| resource.kind == "FILE")
        .filter_map(|resource| resource.properties.get("DSNAME").cloned())
        .collect::<BTreeSet<_>>();
    let queues = installed_resources
        .iter()
        .filter(|resource| resource.kind == "TDQUEUE")
        .map(|resource| resource.name.clone())
        .collect::<BTreeSet<_>>();
    if transactions.len() != resources.transactions
        || programs.len() != resources.programs
        || files.len() != resources.files
        || queues.len() != resources.tdqueues
    {
        return Err(CorpusProblem::new(
            "carddemo.security.resource_drift",
            "installed security resource counts differ",
        ));
    }
    let mut profiles = Vec::new();
    let mut shape = Sha256::new();
    for transaction in &transactions {
        let mut permissions = BTreeMap::from([("CARDADM".into(), AccessIntent::Execute)]);
        if transaction != "CA00" && !transaction.starts_with("CU") {
            permissions.insert("CARDUSR".into(), AccessIntent::Execute);
        }
        profiles.push(RacfProfileDefinition {
            class: "TCICSTRN".into(),
            pattern: format!("CICS.{transaction}"),
            owner: "WEBADM".into(),
            uacc: None,
            permissions,
        });
    }
    for program in &programs {
        let mut permissions = BTreeMap::from([("CARDADM".into(), AccessIntent::Execute)]);
        if !program.starts_with("COADM") && !program.starts_with("COUSR") {
            permissions.insert("CARDUSR".into(), AccessIntent::Execute);
        }
        profiles.push(RacfProfileDefinition {
            class: "FACILITY".into(),
            pattern: format!("CICS.PROGRAM.{program}"),
            owner: "WEBADM".into(),
            uacc: None,
            permissions,
        });
    }
    for dataset in &files {
        profiles.push(RacfProfileDefinition {
            class: "DATASET".into(),
            pattern: dataset.clone(),
            owner: "WEBADM".into(),
            uacc: None,
            permissions: BTreeMap::from([
                ("CARDUSR".into(), AccessIntent::Read),
                ("CARDADM".into(), AccessIntent::Update),
            ]),
        });
    }
    for queue in &queues {
        profiles.push(RacfProfileDefinition {
            class: "QUEUE".into(),
            pattern: format!("CICS.TD.{queue}"),
            owner: "WEBADM".into(),
            uacc: None,
            permissions: BTreeMap::from([
                ("CARDUSR".into(), AccessIntent::Update),
                ("CARDADM".into(), AccessIntent::Update),
            ]),
        });
    }
    for class in ["DB2", "IMS", "JES", "OPERCMDS"] {
        profiles.push(RacfProfileDefinition {
            class: class.into(),
            pattern: "CARDDEMO.**".into(),
            owner: "WEBADM".into(),
            uacc: None,
            permissions: BTreeMap::from([(
                "CARDADM".into(),
                if matches!(class, "JES" | "OPERCMDS") {
                    AccessIntent::Control
                } else {
                    AccessIntent::Update
                },
            )]),
        });
    }
    for profile in &profiles {
        digest_field(&mut shape, profile.class.as_bytes());
        digest_field(&mut shape, profile.pattern.as_bytes());
        digest_field(&mut shape, format!("{:?}", profile.permissions).as_bytes());
    }
    let secrets = Arc::new(MemorySecretResolver::default());
    secrets.insert("secret:webuser", b"transport-user-secret".to_vec());
    secrets.insert("secret:webadmin", b"transport-admin-secret".to_vec());
    secrets.insert("secret:application", b"application-secret".to_vec());
    let service = RacfService::open(
        Arc::new(MemoryStore::new(Default::default())),
        secrets,
        Default::default(),
    )
    .map_err(|problem| CorpusProblem::new("carddemo.security.open_failed", problem.to_string()))?;
    let manifest = RacfManifest {
        groups: ["CARDUSR".into(), "CARDADM".into()].into_iter().collect(),
        users: vec![
            RacfUserDefinition {
                user: "WEBUSER".into(),
                credential: SecretRef::new("secret:webuser", Default::default())
                    .expect("static secret reference"),
                groups: ["CARDUSR".into()].into_iter().collect(),
            },
            RacfUserDefinition {
                user: "WEBADM".into(),
                credential: SecretRef::new("secret:webadmin", Default::default())
                    .expect("static secret reference"),
                groups: ["CARDADM".into()].into_iter().collect(),
            },
        ],
        profiles,
    };
    let install = service
        .install_manifest(manifest.clone())
        .map_err(|problem| {
            CorpusProblem::new("carddemo.security.install_failed", problem.to_string())
        })?;
    let manifest_replay = service
        .install_manifest(manifest)
        .map(|receipt| receipt.replayed)
        .unwrap_or(false);
    let regular =
        PrincipalId::new("WEBUSER", InvocationLimits::default()).expect("static regular principal");
    let admin =
        PrincipalId::new("WEBADM", InvocationLimits::default()).expect("static admin principal");
    let check = |principal: &PrincipalId,
                 class: &str,
                 resource: &str,
                 intent: AccessIntent,
                 expected: SecurityDecision|
     -> Result<(), CorpusProblem> {
        let actual = service
            .authorize(
                principal,
                class,
                &ResourceName::new(resource, 246).map_err(|_| {
                    CorpusProblem::new("carddemo.security.resource_invalid", resource)
                })?,
                intent,
            )
            .map_err(|problem| {
                CorpusProblem::new(
                    "carddemo.security.authorization_failed",
                    problem.to_string(),
                )
            })?;
        if actual != expected {
            return Err(CorpusProblem::new(
                "carddemo.security.decision_drift",
                format!("{class}:{resource} returned {actual:?}"),
            ));
        }
        Ok(())
    };
    let regular_allows = [
        ("TCICSTRN", "CICS.CC00", AccessIntent::Execute),
        ("FACILITY", "CICS.PROGRAM.COSGN00C", AccessIntent::Execute),
        (
            "DATASET",
            "AWS.M2.CARDDEMO.ACCTDATA.VSAM.KSDS",
            AccessIntent::Read,
        ),
        ("QUEUE", "CICS.TD.JOBS", AccessIntent::Update),
    ];
    for (class, resource, intent) in regular_allows {
        check(&regular, class, resource, intent, SecurityDecision::Allow)?;
    }
    let regular_denies = [
        ("TCICSTRN", "CICS.CA00", AccessIntent::Execute),
        ("FACILITY", "CICS.PROGRAM.COADM01C", AccessIntent::Execute),
        (
            "DATASET",
            "AWS.M2.CARDDEMO.ACCTDATA.VSAM.KSDS",
            AccessIntent::Update,
        ),
        ("DB2", "CARDDEMO.PENDING", AccessIntent::Update),
        ("IMS", "CARDDEMO.AUTH", AccessIntent::Update),
        ("JES", "CARDDEMO.REPORT", AccessIntent::Control),
        ("OPERCMDS", "CARDDEMO.CEMT", AccessIntent::Control),
        ("DATASET", "SYS1.PARMLIB", AccessIntent::Read),
    ];
    for (class, resource, intent) in regular_denies {
        check(&regular, class, resource, intent, SecurityDecision::Deny)?;
    }
    let admin_allows = [
        ("TCICSTRN", "CICS.CA00", AccessIntent::Execute),
        ("FACILITY", "CICS.PROGRAM.COADM01C", AccessIntent::Execute),
        (
            "DATASET",
            "AWS.M2.CARDDEMO.ACCTDATA.VSAM.KSDS",
            AccessIntent::Update,
        ),
        ("QUEUE", "CICS.TD.JOBS", AccessIntent::Update),
        ("DB2", "CARDDEMO.PENDING", AccessIntent::Update),
        ("IMS", "CARDDEMO.AUTH", AccessIntent::Update),
        ("JES", "CARDDEMO.REPORT", AccessIntent::Control),
        ("OPERCMDS", "CARDDEMO.CEMT", AccessIntent::Control),
    ];
    for (class, resource, intent) in admin_allows {
        check(&admin, class, resource, intent, SecurityDecision::Allow)?;
    }
    let identities_distinct = service
        .authenticate(
            &PrincipalId::new("APPUSER", InvocationLimits::default())
                .expect("static application principal"),
            &SecretRef::new("secret:application", Default::default())
                .expect("static secret reference"),
        )
        .map(|decision| decision == SecurityDecision::InvalidCredentials)
        .unwrap_or(false);
    service
        .record_audit(AuditEvent {
            action: "CARDDEMO.SIGNON".into(),
            resource_hash: "sha256:resource".into(),
            decision: "DENY".into(),
            fields: BTreeMap::from([
                ("password".into(), "transport-user-secret".into()),
                ("card_number".into(), "0000000000000001".into()),
                ("queue_payload".into(), "message-secret".into()),
                ("protected_field".into(), "private".into()),
                ("transaction".into(), "CC00".into()),
            ]),
        })
        .map_err(|problem| {
            CorpusProblem::new("carddemo.security.audit_failed", problem.to_string())
        })?;
    let audit = service
        .audits()
        .pop()
        .ok_or_else(|| CorpusProblem::new("carddemo.security.audit_failed", "audit is missing"))?;
    let redacted_fields = audit
        .fields
        .values()
        .filter(|value| value.as_str() == "<redacted>")
        .count();
    let application_signon_records =
        fs::metadata(corpus_dir.join("app/data/EBCDIC/AWS.M2.CARDDEMO.USRSEC.PS"))
            .map_err(|error| {
                CorpusProblem::new("carddemo.security.seed_missing", error.to_string())
            })?
            .len() as usize
            / 80;
    let classes = [
        "DATASET", "DB2", "FACILITY", "IMS", "JES", "OPERCMDS", "QUEUE", "TCICSTRN",
    ];
    if install.groups != 2
        || install.users != 2
        || install.profiles != 64
        || !manifest_replay
        || !identities_distinct
        || application_signon_records != 10
        || redacted_fields != 4
    {
        return Err(CorpusProblem::new(
            "carddemo.security.surface_drift",
            "security manifest or decisions differ from the pinned contract",
        ));
    }
    Ok(CardDemoSecurityReceipt {
        schema_version: "mainframe-env.carddemo-security-receipt@1".into(),
        status: "pass".into(),
        corpus_commit: seeds.corpus_commit,
        transport_users: install.users,
        application_signon_records,
        identities_distinct,
        groups: install.groups,
        profiles: install.profiles,
        permissions: install.permissions,
        resource_classes: classes.into_iter().map(str::to_string).collect(),
        transaction_profiles: transactions.len(),
        program_profiles: programs.len(),
        dataset_profiles: files.len(),
        queue_profiles: queues.len(),
        regular_allow_checks: regular_allows.len(),
        regular_deny_checks: regular_denies.len(),
        admin_allow_checks: admin_allows.len(),
        redacted_fields,
        manifest_replay,
        security_shape_sha256: format!("{:x}", shape.finalize()),
    })
}

fn digest_field(digest: &mut Sha256, bytes: &[u8]) {
    digest.update((bytes.len() as u64).to_be_bytes());
    digest.update(bytes);
}

fn explicit_carddemo_bundles(
    corpus_dir: &Path,
) -> Result<Vec<(String, SourceBundle)>, CorpusProblem> {
    let source_paths = collect_paths(
        corpus_dir,
        &[
            "app/cbl",
            "app/app-authorization-ims-db2-mq/cbl",
            "app/app-transaction-type-db2/cbl",
            "app/app-vsam-mq/cbl",
        ],
        "cbl",
    )?;
    let copy_roots = [
        "app/app-authorization-ims-db2-mq/cpy",
        "app/app-authorization-ims-db2-mq/cpy-bms",
        "app/app-transaction-type-db2/cpy",
        "app/app-transaction-type-db2/cpy-bms",
        "app/cpy",
        "app/cpy-bms",
    ];
    let copy_paths = collect_paths(corpus_dir, &copy_roots, "cpy")?;
    let limits = SourceLimits::default();
    let copybooks = copy_paths
        .iter()
        .map(|path| source_file(corpus_dir, path, limits))
        .collect::<Result<Vec<_>, _>>()?;
    let (compatibility, compatibility_library) =
        owned_compatibility_library(limits).map_err(|error| {
            CorpusProblem::new(
                "carddemo.layout.closure_invalid",
                format!("owned compatibility catalog is invalid: {error}"),
            )
        })?;
    let mut libraries = Vec::new();
    for (index, root) in copy_roots.iter().enumerate() {
        let members = collect_paths(corpus_dir, &[*root], "cpy")?
            .into_iter()
            .map(|path| LogicalPath::new(path, limits.max_path_bytes))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| {
                CorpusProblem::new(
                    "carddemo.layout.closure_invalid",
                    format!("application member path is invalid: {error}"),
                )
            })?;
        libraries.push(
            SourceLibrary::new(format!("application-{index:02}"), members, limits).map_err(
                |error| {
                    CorpusProblem::new(
                        "carddemo.layout.closure_invalid",
                        format!("application library is invalid: {error}"),
                    )
                },
            )?,
        );
    }
    libraries.push(compatibility_library);
    let mut bundles = Vec::new();
    for primary_path in source_paths {
        let primary = source_file(corpus_dir, &primary_path, limits)?;
        let mut files = Vec::with_capacity(1 + copybooks.len() + compatibility.len());
        files.push(primary);
        files.extend(copybooks.iter().cloned());
        files.extend(compatibility.iter().cloned());
        let logical = LogicalPath::new(&primary_path, limits.max_path_bytes).map_err(|error| {
            CorpusProblem::new(
                "carddemo.layout.closure_invalid",
                format!("program path is invalid: {error}"),
            )
        })?;
        let bundle = SourceBundle::with_libraries(
            &logical,
            files,
            libraries.clone(),
            BTreeMap::new(),
            Vec::new(),
            limits,
        )
        .map_err(|error| {
            CorpusProblem::new(
                "carddemo.layout.closure_invalid",
                format!("source closure is invalid: {error}"),
            )
        })?;
        bundles.push((primary_path, bundle));
    }
    Ok(bundles)
}

fn collect_paths(
    corpus_dir: &Path,
    roots: &[&str],
    extension: &str,
) -> Result<Vec<String>, CorpusProblem> {
    let mut paths = Vec::new();
    for root in roots {
        let directory = corpus_dir.join(root);
        let entries = fs::read_dir(&directory).map_err(|error| {
            CorpusProblem::new(
                "carddemo.source.file_missing",
                format!("cannot enumerate source root {root}: {error}"),
            )
        })?;
        for entry in entries {
            let path = entry
                .map_err(|error| {
                    CorpusProblem::new(
                        "carddemo.source.file_missing",
                        format!("cannot enumerate source root {root}: {error}"),
                    )
                })?
                .path();
            if !path.is_file()
                || !path
                    .extension()
                    .and_then(|value| value.to_str())
                    .is_some_and(|value| value.eq_ignore_ascii_case(extension))
            {
                continue;
            }
            let relative = path
                .strip_prefix(corpus_dir)
                .ok()
                .and_then(Path::to_str)
                .ok_or_else(|| {
                    CorpusProblem::new(
                        "carddemo.source.path_invalid",
                        "source path is not a repository-relative UTF-8 path",
                    )
                })?;
            paths.push(relative.to_string());
        }
    }
    paths.sort();
    Ok(paths)
}

fn source_file(
    corpus_dir: &Path,
    relative: &str,
    limits: SourceLimits,
) -> Result<SourceFile, CorpusProblem> {
    validate_relative_path(relative, "source file")?;
    let bytes = read_corpus_file(corpus_dir, &corpus_dir.join(relative))?;
    SourceFile::input(
        relative,
        bytes,
        SourceFormat::Fixed,
        SourceEncoding::Utf8,
        limits,
    )
    .map_err(|error| {
        CorpusProblem::new(
            "carddemo.source.bundle_invalid",
            format!("cannot load source file {relative}: {error}"),
        )
    })
}

fn read_runtime_oracle(root: &Path, identity: &str) -> Result<Vec<u8>, CorpusProblem> {
    let Some((archive, entry)) = identity.split_once('!') else {
        validate_relative_path(identity, "runtime oracle")?;
        return read_corpus_file(root, &root.join(identity));
    };
    validate_relative_path(archive, "runtime archive")?;
    validate_relative_path(entry, "runtime archive entry")?;
    let archive_path = root.join(archive);
    if !archive_path.is_file() {
        return Err(CorpusProblem::new(
            "carddemo.corpus.file_missing",
            format!("cannot read corpus file {archive}"),
        ));
    }
    let output = Command::new("unzip")
        .args(["-p"])
        .arg(&archive_path)
        .arg(entry)
        .output()
        .map_err(|error| {
            CorpusProblem::new(
                "carddemo.corpus.archive_reader_unavailable",
                format!("cannot inspect runtime archive metadata: {error}"),
            )
        })?;
    if !output.status.success() {
        return Err(CorpusProblem::new(
            "carddemo.corpus.runtime_archive_drift",
            format!("cannot read declared runtime archive entry {archive}!{entry}"),
        ));
    }
    Ok(output.stdout)
}

fn read_contract(path: &Path) -> Result<CorpusContract, CorpusProblem> {
    let bytes = fs::read(path).map_err(|error| {
        CorpusProblem::new(
            "carddemo.corpus.contract_missing",
            format!("cannot read corpus inventory: {error}"),
        )
    })?;
    serde_json::from_slice(&bytes).map_err(|error| {
        CorpusProblem::new(
            "carddemo.corpus.contract_invalid",
            format!("cannot parse corpus inventory: {error}"),
        )
    })
}

fn validate_contract(contract: &CorpusContract) -> Result<(), CorpusProblem> {
    if contract.executable_content_identity.algorithm != "sha256-u64be-path-u64be-content-v1" {
        return Err(CorpusProblem::new(
            "carddemo.corpus.contract_invalid",
            "unsupported executable content identity algorithm",
        ));
    }
    for (name, value, expected_len) in [
        ("commit", contract.commit.as_str(), 40),
        ("tree", contract.tree.as_str(), 40),
        ("license_sha256", contract.license_sha256.as_str(), 64),
        (
            "executable content SHA-256",
            contract.executable_content_identity.sha256.as_str(),
            64,
        ),
    ] {
        if value.len() != expected_len || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(CorpusProblem::new(
                "carddemo.corpus.contract_invalid",
                format!("{name} is not a {expected_len}-digit hexadecimal identity"),
            ));
        }
    }
    if contract.runtime_oracles.is_empty() || contract.file_count_checks.is_empty() {
        return Err(CorpusProblem::new(
            "carddemo.corpus.contract_invalid",
            "runtime archive and file-count checks must be declared",
        ));
    }
    if contract.external_compatibility_copybooks.is_empty()
        || contract
            .external_compatibility_copybooks
            .iter()
            .collect::<BTreeSet<_>>()
            .len()
            != contract.external_compatibility_copybooks.len()
    {
        return Err(CorpusProblem::new(
            "carddemo.corpus.contract_invalid",
            "external compatibility copybook identities are empty or duplicated",
        ));
    }
    Ok(())
}

fn require_corpus_directory(path: &Path) -> Result<(), CorpusProblem> {
    if path.is_dir() {
        Ok(())
    } else {
        Err(CorpusProblem::new(
            "carddemo.corpus.directory_missing",
            "CARDEMO_CORPUS_DIR does not name an available directory",
        ))
    }
}

fn git(root: &Path, arguments: &[&str]) -> Result<String, CorpusProblem> {
    let output = Command::new("git")
        .args(arguments)
        .current_dir(root)
        .output()
        .map_err(|error| {
            CorpusProblem::new(
                "carddemo.corpus.git_unavailable",
                format!("cannot execute Git verification: {error}"),
            )
        })?;
    if !output.status.success() {
        return Err(CorpusProblem::new(
            "carddemo.corpus.git_failed",
            format!("Git verification {:?} failed", arguments),
        ));
    }
    String::from_utf8(output.stdout)
        .map(|value| value.trim().to_string())
        .map_err(|_| {
            CorpusProblem::new(
                "carddemo.corpus.git_failed",
                "Git verification returned non-UTF-8 output",
            )
        })
}

fn tracked_files(root: &Path) -> Result<Vec<String>, CorpusProblem> {
    let output = Command::new("git")
        .args(["ls-files", "-z"])
        .current_dir(root)
        .output()
        .map_err(|error| {
            CorpusProblem::new(
                "carddemo.corpus.git_unavailable",
                format!("cannot enumerate tracked corpus files: {error}"),
            )
        })?;
    if !output.status.success() {
        return Err(CorpusProblem::new(
            "carddemo.corpus.git_failed",
            "cannot enumerate tracked corpus files",
        ));
    }
    let text = String::from_utf8(output.stdout).map_err(|_| {
        CorpusProblem::new(
            "carddemo.corpus.path_invalid",
            "tracked corpus paths must be UTF-8",
        )
    })?;
    let mut files = text
        .split('\0')
        .filter(|path| !path.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    files.sort();
    if files.is_empty() {
        return Err(CorpusProblem::new(
            "carddemo.corpus.content_missing",
            "the CardDemo checkout has no tracked files",
        ));
    }
    Ok(files)
}

fn canonical_content_digest(root: &Path, tracked: &[String]) -> Result<String, CorpusProblem> {
    let mut digest = Sha256::new();
    for relative in tracked {
        validate_relative_path(relative, "tracked file")?;
        let bytes = read_corpus_file(root, &root.join(relative))?;
        digest.update((relative.len() as u64).to_be_bytes());
        digest.update(relative.as_bytes());
        digest.update((bytes.len() as u64).to_be_bytes());
        digest.update(bytes);
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn count_files(tracked: &[String], check: &FileCountContract) -> Result<usize, CorpusProblem> {
    if check.id.is_empty() || check.roots.is_empty() {
        return Err(CorpusProblem::new(
            "carddemo.corpus.contract_invalid",
            "file-count checks require an identity and at least one root",
        ));
    }
    for root in &check.roots {
        validate_relative_path(root, "file-count root")?;
    }
    let extensions = check
        .extensions
        .iter()
        .map(|extension| extension.trim_start_matches('.').to_ascii_lowercase())
        .collect::<Vec<_>>();
    Ok(tracked
        .iter()
        .filter(|path| {
            check.roots.iter().any(|root| {
                path.as_str() == root
                    || path
                        .strip_prefix(root)
                        .is_some_and(|suffix| suffix.starts_with('/'))
            })
        })
        .filter(|path| {
            let file_name = Path::new(path)
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("");
            !check.excluded_names.iter().any(|name| name == file_name)
        })
        .filter(|path| {
            extensions.is_empty()
                || Path::new(path)
                    .extension()
                    .and_then(|extension| extension.to_str())
                    .map(str::to_ascii_lowercase)
                    .is_some_and(|extension| extensions.contains(&extension))
        })
        .count())
}

fn validate_relative_path(path: &str, kind: &str) -> Result<(), CorpusProblem> {
    let candidate = Path::new(path);
    if path.is_empty()
        || candidate.is_absolute()
        || candidate
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(CorpusProblem::new(
            "carddemo.corpus.contract_invalid",
            format!("{kind} must be a normalized repository-relative path"),
        ));
    }
    Ok(())
}

fn read_corpus_file(root: &Path, path: &Path) -> Result<Vec<u8>, CorpusProblem> {
    let relative = path.strip_prefix(root).map_err(|_| {
        CorpusProblem::new(
            "carddemo.corpus.path_invalid",
            "corpus file escaped the declared root",
        )
    })?;
    fs::read(path).map_err(|error| {
        CorpusProblem::new(
            "carddemo.corpus.file_missing",
            format!("cannot read corpus file {}: {error}", relative.display()),
        )
    })
}

fn require_equal(
    code: &str,
    label: &str,
    expected: &str,
    actual: &str,
) -> Result<(), CorpusProblem> {
    if expected == actual {
        Ok(())
    } else {
        Err(CorpusProblem::new(
            code,
            format!("{label} expected {expected} but found {actual}"),
        ))
    }
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::io::Write;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

    struct Fixture {
        root: PathBuf,
        inventory: PathBuf,
    }

    impl Fixture {
        fn create() -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let root = env::temp_dir().join(format!(
                "mainframe-env-carddemo-corpus-{}-{nonce}-{}",
                std::process::id(),
                NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(root.join("app/cbl")).unwrap();
            fs::create_dir_all(root.join("samples")).unwrap();
            fs::write(
                root.join("LICENSE"),
                b"Apache License\nVersion 2.0, January 2004\n",
            )
            .unwrap();
            fs::write(root.join("app/cbl/ONE.cbl"), b"PROGRAM-ID. ONE.\n").unwrap();
            fs::write(root.join("samples/runtime.zip"), b"oracle-only").unwrap();
            command(&root, &["init", "-b", "main"]);
            command(&root, &["config", "user.name", "CardDemo Fixture"]);
            command(&root, &["config", "user.email", "fixture@example.invalid"]);
            command(
                &root,
                &[
                    "remote",
                    "add",
                    "origin",
                    "https://example.invalid/carddemo.git",
                ],
            );
            command(&root, &["add", "."]);
            command(&root, &["commit", "-m", "fixture"]);
            let tracked = tracked_files(&root).unwrap();
            let content = canonical_content_digest(&root, &tracked).unwrap();
            let commit = git(&root, &["rev-parse", "HEAD"]).unwrap();
            let tree = git(&root, &["rev-parse", "HEAD^{tree}"]).unwrap();
            let inventory = root.join(".git/carddemo-contract.json");
            let contract = json!({
                "repository":"https://example.invalid/carddemo.git",
                "commit":commit,
                "tree":tree,
                "license":"Apache-2.0",
                "license_sha256":sha256(&fs::read(root.join("LICENSE")).unwrap()),
                "executable_content_identity":{
                    "algorithm":"sha256-u64be-path-u64be-content-v1",
                    "sha256":content
                },
                "runtime_oracles":[{
                    "path":"samples/runtime.zip",
                    "sha256":sha256(&fs::read(root.join("samples/runtime.zip")).unwrap())
                }],
                "file_count_checks":[{
                    "id":"cobol",
                    "roots":["app/cbl"],
                    "extensions":["cbl"],
                    "expected":1
                }],
                "external_compatibility_copybooks":["DFHAID"]
            });
            fs::write(&inventory, serde_json::to_vec_pretty(&contract).unwrap()).unwrap();
            Self { root, inventory }
        }

        fn verify(&self) -> Result<CardDemoCorpusReceipt, CorpusProblem> {
            verify_carddemo_corpus(&self.root, &self.inventory)
        }

        fn update_contract(&self, mutate: impl FnOnce(&mut serde_json::Value)) {
            let mut contract: serde_json::Value =
                serde_json::from_slice(&fs::read(&self.inventory).unwrap()).unwrap();
            mutate(&mut contract);
            fs::write(
                &self.inventory,
                serde_json::to_vec_pretty(&contract).unwrap(),
            )
            .unwrap();
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.inventory);
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn command(root: &Path, arguments: &[&str]) {
        let output = Command::new("git")
            .args(arguments)
            .current_dir(root)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {arguments:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn valid_pinned_fixture_emits_only_repository_relative_evidence() {
        let fixture = Fixture::create();
        let receipt = fixture.verify().unwrap();
        let json = serde_json::to_string(&receipt).unwrap();
        assert_eq!(receipt.status, "pass");
        assert_eq!(receipt.content.tracked_files, 3);
        assert!(!json.contains(&fixture.root.to_string_lossy().to_string()));
        assert_eq!(receipt.file_counts["cobol"], 1);
    }

    #[test]
    fn dirty_missing_drifted_and_unlicensed_inputs_fail_closed() {
        let dirty = Fixture::create();
        fs::write(dirty.root.join("untracked.txt"), b"drift").unwrap();
        assert_eq!(dirty.verify().unwrap_err().code, "carddemo.corpus.dirty");

        let missing = Fixture::create();
        fs::remove_file(missing.root.join("samples/runtime.zip")).unwrap();
        assert_eq!(missing.verify().unwrap_err().code, "carddemo.corpus.dirty");

        let commit = Fixture::create();
        commit.update_contract(|contract| contract["commit"] = json!("0".repeat(40)));
        assert_eq!(
            commit.verify().unwrap_err().code,
            "carddemo.corpus.commit_drift"
        );

        let license = Fixture::create();
        license.update_contract(|contract| {
            contract["license_sha256"] = json!("0".repeat(64));
        });
        assert_eq!(
            license.verify().unwrap_err().code,
            "carddemo.corpus.license_drift"
        );

        let content = Fixture::create();
        content.update_contract(|contract| {
            contract["executable_content_identity"]["sha256"] = json!("0".repeat(64));
        });
        assert_eq!(
            content.verify().unwrap_err().code,
            "carddemo.corpus.content_drift"
        );

        let count = Fixture::create();
        count.update_contract(|contract| {
            contract["file_count_checks"][0]["expected"] = json!(2);
        });
        assert_eq!(
            count.verify().unwrap_err().code,
            "carddemo.corpus.file_count_drift"
        );
    }

    #[test]
    fn unavailable_directory_has_stable_redacted_failure() {
        let fixture = Fixture::create();
        let missing = fixture.root.join("not-present");
        let error = verify_carddemo_corpus(&missing, &fixture.inventory).unwrap_err();
        assert_eq!(error.code, "carddemo.corpus.directory_missing");
        assert!(
            !error
                .to_string()
                .contains(&missing.to_string_lossy().to_string())
        );
    }

    #[test]
    fn relative_path_validation_rejects_escape() {
        let fixture = Fixture::create();
        fixture.update_contract(|contract| {
            contract["runtime_oracles"][0]["path"] = json!("../runtime.zip");
        });
        assert_eq!(
            fixture.verify().unwrap_err().code,
            "carddemo.corpus.contract_invalid"
        );
    }

    #[test]
    fn problem_is_a_standard_error() {
        let mut sink = Vec::new();
        write!(&mut sink, "{}", CorpusProblem::new("code", "detail")).unwrap();
        assert_eq!(sink, b"code: detail");
    }
}
