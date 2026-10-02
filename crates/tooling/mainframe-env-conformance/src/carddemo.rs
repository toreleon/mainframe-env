//! Fail-closed verification for the externally supplied CardDemo corpus.

mod corpus_validation;
use corpus_validation::*;
mod bounds;
use bounds::checked_total;

mod bms;
mod control_library;
mod online_authorities;
mod readacct;

pub use readacct::{capture_carddemo_readacct_from_env, verify_carddemo_readacct_from_env};

use online_authorities::install_base_online_authorities;

use bms::{carddemo_base_maps, carddemo_maps};

use axum::body::{Body, to_bytes};
use axum::http::{Method, Request, StatusCode};
use base64::Engine;
use mainframe_env_application::{
    APPLICATION_PACKAGE_V2_CONTRACT, ApplicationInstaller, ApplicationManifest, ApplicationPackage,
    ApplicationPackageV2, ApplicationSections, BatchController, BatchControllerKind,
    DatasetCatalog, DatasetCatalogEntry, DatasetDefinition, EntryKind, GenerationGroupDefinition,
    InstallProblem, InstallState, PackageEntry, PackageSignature, ProgramArtifact, ProgramCatalog,
    ProgramFrame, ProgramFrames, SqlColumn, SqlTable, package_identity, package_v2_identity,
    parse_bms, parse_csd,
};
use mainframe_env_batch::{
    JclBundle, JclConversionLimits, JclLimits, JclRecordKind, JclStatementId, JobPlan, JobState,
    StepCondition, UtilityDisposition, analyze_jcl_syntax, convert_jcl, parse_jcl,
    utility_disposition, validate_idcams_control,
};
use mainframe_env_cics::{
    BmsFieldDefinition, BmsMapDefinition, CicsFileDefinition, CicsFileStatus, cics_abi_library,
};
use mainframe_env_compiler::{
    CobolCompiler, ControlEdgeKind, ControlRole, DataCategory, SemanticModel, StatementKind,
    StorageSection,
};
use mainframe_env_compiler_api::{
    CompilationMode, CompileOptions, CompileTarget, CompilerRequest, CompilerResult,
    CompilerService, PublishedArtifact,
};
use mainframe_env_dataset::{DatasetLimits, DatasetSeedObject, DatasetService};
use mainframe_env_db2::{
    Db2ColumnDefinition, Db2ExtractField, Db2ExtractLayout, Db2ForeignKeyDefinition,
    Db2ResultEncoding, Db2TableDefinition, db2_abi_library,
};
use mainframe_env_diagnostics::Completeness;
use mainframe_env_encoding::CodePage;
use mainframe_env_execution_api::{
    ArtifactRef, BoundedPayload, CapabilityId, ExecutionId, IdempotencyKey, Invocation,
    InvocationLimits, Machine, MachineDrive, MachineResume, Principal, PrincipalId, Quantum,
    RequestId, ResourceLimits, RunUnitId, Selector, ServiceClass, TraceId,
};
use mainframe_env_host_api::{
    AccessIntent, AuditEvent, CicsConditionPolicy, CicsOperation, CicsRequest, DatasetAttributes,
    DatasetName, DatasetOrganization, DatasetRequest, DatasetResult, Db2HostVariable, Db2Operation,
    Db2Request, EffectRequest, HostProblem, ImsOperation, ImsQualifier, ImsRequest, MemberName,
    MqOperation, MqRequest, Mutation, RecordFormat, RegistrySnapshot, ResourceName,
    ScopedHostService, SecretRef, SecurityDecision, SessionId,
};
use mainframe_env_ims::{
    ImsApplicationDefinition, ImsDatabaseDefinition, ImsLimits, ImsLoadImage, ImsLoadRoot,
    ImsPcbDefinition, ImsPsbDefinition, ImsSegmentDefinition, ImsService, ims_providers,
};
use mainframe_env_mq::{MqQueueDefinition, MqService, mq_abi_library, mq_providers};
use mainframe_env_racf::{
    MemorySecretResolver, RacfManifest, RacfProfileDefinition, RacfService, RacfUserDefinition,
};
use mainframe_env_server::{
    ArtifactProfile, BatchProgramDefinition, HmacSha256PackageTrust, OnlineApplicationDefinition,
    OnlineProgramDefinition, ProductServer, ServerConfig, StoreProfile, TlsConfig,
    compatible_system_services, default_program_router,
};
use mainframe_env_source::{
    HostAbiLibraryDefinition, LogicalPath, MaterializedHostAbiLibraries, SourceBundle,
    SourceEncoding, SourceFile, SourceFormat, SourceLibrary, SourceLimits,
    materialize_host_abi_libraries,
};
use mainframe_env_store::{
    MemoryStore, PostgresArtifactStore, PostgresStateStore, SqliteStateStore,
};
use mainframe_env_store_api::AuditSink;
use ring::hmac;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use tower::ServiceExt;

mod carddemo_jobs;
use carddemo_jobs::{
    submit_expected_abend, utility_job_failure, wait_for_listed_job, wait_for_submitted_job,
};

const CORPUS_ENV: &str = "CARDDEMO_CORPUS_DIR";

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
struct CardDemoCorrectionsContract {
    schema_version: String,
    corpus_commit: String,
    corrections: Vec<CardDemoCorrectionContract>,
}

#[derive(Clone, Debug, Deserialize)]
struct CardDemoCorrectionContract {
    id: String,
    disposition: String,
    transaction: String,
    program: String,
    source: String,
    source_sha256: String,
    behavior: String,
    basis: String,
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

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CardDemoTerminalReceipt {
    pub schema_version: String,
    pub status: String,
    pub corpus_commit: String,
    pub launch_transaction: String,
    pub bms_maps_installed: usize,
    pub bms_fields_installed: usize,
    pub public_routes_exercised: usize,
    pub protocol_screen_fetches: usize,
    pub input_submissions: usize,
    pub authentication_controls: usize,
    pub csrf_controls: usize,
    pub restart_resumes: usize,
    pub disconnects: usize,
    pub timeout_controls: usize,
    pub malformed_controls: usize,
    pub idle_worker_count: usize,
    pub terminal_shape_sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CardDemoBaseOnlineReceipt {
    pub schema_version: String,
    pub status: String,
    pub corpus_commit: String,
    pub journeys_passed: usize,
    pub programs_installed: usize,
    pub source_backed_transactions: usize,
    pub maps_installed: usize,
    pub initial_screen_bytes: usize,
    pub screen_paths: usize,
    pub dataset_reads: usize,
    pub committed_mutations: usize,
    pub rollback_controls: usize,
    pub denial_controls: usize,
    pub restart_controls: usize,
    pub concurrency_controls: usize,
    pub resource_controls: usize,
    pub install_replay: bool,
    pub journey_shape_sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CardDemoJclReceipt {
    pub schema_version: String,
    pub status: String,
    pub corpus_commit: String,
    pub jcl_files: usize,
    pub procedures: usize,
    pub parsed_files: usize,
    pub accepted_unsupported_files: usize,
    pub file_results: BTreeMap<String, String>,
    pub jobs: usize,
    pub steps: usize,
    pub dds: usize,
    pub continuation_lines: usize,
    pub symbols: usize,
    pub conditions: usize,
    pub procedure_libraries: usize,
    pub procedure_steps: usize,
    pub procedure_overrides: usize,
    pub dd_concatenations: usize,
    pub instream_dds: usize,
    pub instream_records: usize,
    pub provenance_spans: usize,
    pub negative_controls: usize,
    pub jcl_shape_sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CardDemoUtilityReceipt {
    pub schema_version: String,
    pub status: String,
    pub corpus_commit: String,
    pub jcl_files: usize,
    pub parsed_jobs: usize,
    pub idcams_steps: usize,
    pub inline_idcams_controls: usize,
    pub idcams_statements: usize,
    pub implemented_utilities: Vec<String>,
    pub explicit_external_utilities: BTreeMap<String, String>,
    pub selected_job_routes: usize,
    pub exact_dataset_mutations: usize,
    pub disposition_controls: usize,
    pub gdg_controls: usize,
    pub aix_controls: usize,
    pub internal_reader_controls: usize,
    pub unknown_controls: usize,
    pub utility_shape_sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CardDemoBatchProgramReceipt {
    pub schema_version: String,
    pub status: String,
    pub corpus_commit: String,
    pub named_programs: Vec<String>,
    pub called_programs: Vec<String>,
    pub compatible_services: Vec<String>,
    pub compiled_artifacts: usize,
    pub installed_artifacts: usize,
    pub install_replay: bool,
    pub selected_job_routes: usize,
    pub linkage_routes: usize,
    pub file_routes: usize,
    pub abend_controls: usize,
    pub checkpoint_controls: usize,
    pub cancellation_controls: usize,
    pub timeout_controls: usize,
    pub restart_routes: usize,
    pub program_shape_sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CardDemoBaseBatchReceipt {
    pub schema_version: String,
    pub status: String,
    pub corpus_commit: String,
    pub business_date: String,
    pub journeys_passed: usize,
    pub initialization_jobs: usize,
    pub operational_jobs: usize,
    pub cics_file_controls: usize,
    pub internal_submissions: usize,
    pub warm_restart_controls: usize,
    pub rollback_controls: usize,
    pub cancellation_controls: usize,
    pub tranrept_selected_records: usize,
    pub tranrept_report_records: usize,
    pub dataset_sha256: BTreeMap<String, String>,
    pub spool_sha256: BTreeMap<String, String>,
    pub journey_shape_sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CardDemoDb2Receipt {
    pub schema_version: String,
    pub status: String,
    pub corpus_commit: String,
    pub programs_compiled: usize,
    pub sql_include_expansions: usize,
    pub sql_operations: BTreeMap<String, usize>,
    pub ddl_files: usize,
    pub online_routes: usize,
    pub batch_routes: usize,
    pub extraction_records: usize,
    pub authorization_controls: usize,
    pub restart_controls: usize,
    pub rollback_controls: usize,
    pub conflict_controls: usize,
    pub failure_controls: usize,
    pub table_sha256: BTreeMap<String, String>,
    pub dataset_sha256: BTreeMap<String, String>,
    pub spool_sha256: BTreeMap<String, String>,
    pub db2_shape_sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CardDemoImsReceipt {
    pub schema_version: String,
    pub status: String,
    pub corpus_commit: String,
    pub definitions_checked: usize,
    pub databases_installed: usize,
    pub psbs_installed: usize,
    pub pcbs_installed: usize,
    pub programs_compiled: usize,
    pub dli_operations: BTreeMap<String, usize>,
    pub application_routes: usize,
    pub selected_job_routes: usize,
    pub roots: usize,
    pub children: usize,
    pub secondary_index_entries: usize,
    pub load_unload_routes: usize,
    pub checkpoint_controls: usize,
    pub restart_controls: usize,
    pub authorization_controls: usize,
    pub malformed_controls: usize,
    pub resource_controls: usize,
    pub provider_failure_controls: usize,
    pub hierarchy_sha256: String,
    pub spool_sha256: BTreeMap<String, String>,
    pub ims_shape_sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CardDemoMqAuthorizationReceipt {
    pub schema_version: String,
    pub status: String,
    pub corpus_commit: String,
    pub programs_compiled: usize,
    pub mq_calls: BTreeMap<String, usize>,
    pub queues_installed: usize,
    pub triggers_installed: usize,
    pub journeys_passed: usize,
    pub request_reply_routes: usize,
    pub approval_decline_routes: usize,
    pub summary_detail_fraud_routes: usize,
    pub purge_routes: usize,
    pub correlation_controls: usize,
    pub timeout_controls: usize,
    pub syncpoint_controls: usize,
    pub rollback_controls: usize,
    pub unknown_outcome_controls: usize,
    pub restart_controls: usize,
    pub idempotency_controls: usize,
    pub authorization_controls: usize,
    pub ims_roots: usize,
    pub ims_children: usize,
    pub fraud_rows: usize,
    pub queue_sha256: BTreeMap<String, String>,
    pub authorization_shape_sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CardDemoFullReceipt {
    pub schema_version: String,
    pub status: String,
    pub corpus_commit: String,
    pub issues_input_passed: usize,
    pub journeys_passed: usize,
    pub profile_receipt_sha256: BTreeMap<String, String>,
    pub operator_scripts_checked: usize,
    pub operator_jcl_submissions: usize,
    pub owned_commands: Vec<String>,
    pub ftp_jes_mappings: usize,
    pub public_operator_routes: usize,
    pub mixed_requests_offered: usize,
    pub mixed_requests_completed: usize,
    pub sqlite_backup_restore_controls: usize,
    pub postgres_restart_controls: usize,
    pub provider_failure_controls: usize,
    pub unknown_outcome_controls: usize,
    pub cancellation_controls: usize,
    pub cross_principal_controls: usize,
    pub cdv1_disposition: String,
    pub cdv1_correction_sha256: String,
    pub cdv1_source_sha256: String,
    pub cdv1_artifact_sha256: String,
    pub cdv1_screen_sha256: String,
    pub cdv1_public_routes: usize,
    pub release_disposition: String,
    pub native_or_legacy_fallback_present: bool,
    pub operator_mapping_sha256: String,
    pub full_shape_sha256: String,
}

struct Cdv1CorrectionReceipt {
    disposition: String,
    correction_sha256: String,
    source_sha256: String,
    artifact_sha256: String,
    screen_sha256: String,
    public_routes: usize,
}

struct BaseOnlineExercise {
    initial_screen_bytes: usize,
    screen_paths: usize,
    dataset_reads: usize,
    committed_mutations: usize,
    rollback_controls: usize,
    denial_controls: usize,
    restart_controls: usize,
    concurrency_controls: usize,
    resource_controls: usize,
    install_replay: bool,
    observations: Vec<String>,
}

struct Db2Exercise {
    online_routes: usize,
    batch_routes: usize,
    extraction_records: usize,
    authorization_controls: usize,
    restart_controls: usize,
    rollback_controls: usize,
    conflict_controls: usize,
    failure_controls: usize,
    table_sha256: BTreeMap<String, String>,
    dataset_sha256: BTreeMap<String, String>,
    spool_sha256: BTreeMap<String, String>,
}

pub fn verify_carddemo_corpus_from_env(
    inventory_path: &Path,
) -> Result<CardDemoCorpusReceipt, CorpusProblem> {
    let corpus = env::var_os(CORPUS_ENV).ok_or_else(|| {
        CorpusProblem::new(
            "carddemo.corpus.environment_missing",
            "CARDDEMO_CORPUS_DIR is required for CardDemo gates",
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
            "CARDDEMO_CORPUS_DIR is required for CardDemo gates",
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
            "CARDDEMO_CORPUS_DIR is required for CardDemo gates",
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
    let owned_names = subsystem_abi_definitions()
        .iter()
        .flat_map(|library| library.members.iter())
        .map(|member| member.name.to_string())
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
    let abi = subsystem_abi_libraries(limits)?;
    let compatibility = abi.files;
    let compatibility_library = SourceLibrary::new(
        "owned-compatibility",
        compatibility
            .iter()
            .map(|file| file.path().clone())
            .collect(),
        limits,
    )
    .map_err(|error| {
        CorpusProblem::new(
            "carddemo.closure.compatibility_invalid",
            format!("legacy compatibility projection is invalid: {error}"),
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
        compatibility_contract: LEGACY_COMPATIBILITY_COPYBOOK_CONTRACT.into(),
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
            "CARDDEMO_CORPUS_DIR is required for CardDemo gates",
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
            "CARDDEMO_CORPUS_DIR is required for CardDemo gates",
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

pub fn verify_carddemo_core_semantics_from_env(
    inventory_path: &Path,
) -> Result<CardDemoCoreReceipt, CorpusProblem> {
    let corpus_dir = env::var_os(CORPUS_ENV).ok_or_else(|| {
        CorpusProblem::new(
            "carddemo.corpus.environment_missing",
            "CARDDEMO_CORPUS_DIR is required for CardDemo gates",
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
            "CARDDEMO_CORPUS_DIR is required for CardDemo gates",
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
            "CARDDEMO_CORPUS_DIR is required for CardDemo gates",
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
            "CARDDEMO_CORPUS_DIR is required for CardDemo gates",
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
            name: "AWS-CARDDEMO".into(),
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
            "CARDDEMO_CORPUS_DIR is required for CardDemo gates",
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
            "CARDDEMO_CORPUS_DIR is required for CardDemo gates",
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
            "CARDDEMO_CORPUS_DIR is required",
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
            "CARDDEMO_CORPUS_DIR is required",
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
            "CARDDEMO_CORPUS_DIR is required",
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
            "CARDDEMO_CORPUS_DIR is required",
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
            "CARDDEMO_CORPUS_DIR is required",
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
                upgrade: true,
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
            "CARDDEMO_CORPUS_DIR is required",
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
        if transaction != "CA00" && !transaction.starts_with("CU") && !transaction.starts_with("CT")
        {
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
        if !program.starts_with("COADM")
            && !program.starts_with("COUSR")
            && !program.starts_with("COTRT")
        {
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

pub fn verify_carddemo_terminal_from_env(
    inventory_path: &Path,
) -> Result<CardDemoTerminalReceipt, CorpusProblem> {
    let security = verify_carddemo_security_from_env(inventory_path)?;
    verify_carddemo_resources_from_env(inventory_path)?;
    let corpus_dir = env::var_os(CORPUS_ENV).ok_or_else(|| {
        CorpusProblem::new(
            "carddemo.corpus.environment_missing",
            "CARDDEMO_CORPUS_DIR is required",
        )
    })?;
    let corpus_dir = Path::new(&corpus_dir);
    let maps = carddemo_base_maps(corpus_dir, &[])?;
    let fields = maps.iter().map(|map| map.fields.len()).sum();
    if maps.len() != 17 {
        return Err(CorpusProblem::new(
            "carddemo.terminal.map_drift",
            "BMS map count differs from installed resources",
        ));
    }
    let login = maps
        .iter()
        .find(|map| map.mapset == "COSGN00" && map.map == "COSGN0A")
        .cloned()
        .ok_or_else(|| {
            CorpusProblem::new(
                "carddemo.terminal.map_missing",
                "COSGN00/COSGN0A is missing",
            )
        })?;
    let map_count = maps.len();
    let input = login
        .fields
        .iter()
        .find(|field| !field.protected && !field.secret)
        .cloned()
        .ok_or_else(|| {
            CorpusProblem::new(
                "carddemo.terminal.input_missing",
                "login map has no public input field",
            )
        })?;
    let artifact_root = env::temp_dir().join(format!(
        "mainframe-env-carddemo-terminal-{}",
        std::process::id()
    ));
    let config = ServerConfig {
        store_profile: StoreProfile::Memory,
        artifact_root: artifact_root.clone(),
        tls: TlsConfig {
            enabled: false,
            certificate_path: None,
            private_key_reference: None,
        },
        ..ServerConfig::default()
    };
    let store = Arc::new(MemoryStore::new(Default::default()));
    let secrets = Arc::new(MemorySecretResolver::default());
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| CorpusProblem::new("carddemo.terminal.runtime", error.to_string()))?;
    let result = runtime.block_on(exercise_terminal_routes(
        config, store, secrets, maps, login, input,
    ));
    let _ = fs::remove_dir_all(&artifact_root);
    let exercise = result?;
    if exercise.public_routes != 7
        || exercise.protocol_fetches != 2
        || exercise.input_submissions != 2
        || exercise.authentication_controls != 2
        || exercise.csrf_controls != 2
        || exercise.restart_resumes != 1
        || exercise.disconnects != 1
        || exercise.timeout_controls != 1
        || exercise.malformed_controls != 2
        || exercise.idle_workers != 0
    {
        return Err(CorpusProblem::new(
            "carddemo.terminal.surface_drift",
            "terminal route observations differ from the pinned contract",
        ));
    }
    let mut shape = Sha256::new();
    digest_field(&mut shape, security.corpus_commit.as_bytes());
    for map in &exercise.map_shapes {
        digest_field(&mut shape, map.as_bytes());
    }
    Ok(CardDemoTerminalReceipt {
        schema_version: "mainframe-env.carddemo-terminal-receipt@1".into(),
        status: "pass".into(),
        corpus_commit: security.corpus_commit,
        launch_transaction: "CC00".into(),
        bms_maps_installed: map_count,
        bms_fields_installed: fields,
        public_routes_exercised: exercise.public_routes,
        protocol_screen_fetches: exercise.protocol_fetches,
        input_submissions: exercise.input_submissions,
        authentication_controls: exercise.authentication_controls,
        csrf_controls: exercise.csrf_controls,
        restart_resumes: exercise.restart_resumes,
        disconnects: exercise.disconnects,
        timeout_controls: exercise.timeout_controls,
        malformed_controls: exercise.malformed_controls,
        idle_worker_count: exercise.idle_workers,
        terminal_shape_sha256: format!("{:x}", shape.finalize()),
    })
}

pub fn verify_carddemo_base_online_from_env(
    inventory_path: &Path,
) -> Result<CardDemoBaseOnlineReceipt, CorpusProblem> {
    let terminal = verify_carddemo_terminal_from_env(inventory_path)?;
    let corpus_dir = env::var_os(CORPUS_ENV).ok_or_else(|| {
        CorpusProblem::new(
            "carddemo.corpus.environment_missing",
            "CARDDEMO_CORPUS_DIR is required",
        )
    })?;
    let definition = carddemo_base_online_definition(Path::new(&corpus_dir))?;
    let programs = definition.programs.len();
    let transactions = definition.transactions.len();
    let maps = definition.maps.len();
    let artifact_root = env::temp_dir().join(format!(
        "mainframe-env-carddemo-base-online-{}",
        std::process::id()
    ));
    let config = ServerConfig {
        store_profile: StoreProfile::Memory,
        artifact_root: artifact_root.clone(),
        tls: TlsConfig {
            enabled: false,
            certificate_path: None,
            private_key_reference: None,
        },
        ..ServerConfig::default()
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| CorpusProblem::new("carddemo.online.runtime", error.to_string()))?;
    let store = Arc::new(MemoryStore::new(Default::default()));
    let secrets = Arc::new(MemorySecretResolver::default());
    let result = runtime.block_on(exercise_base_online_smoke(
        config,
        store,
        secrets,
        Path::new(&corpus_dir).to_path_buf(),
        definition,
    ));
    let _ = fs::remove_dir_all(&artifact_root);
    let exercise = result?;
    if exercise.screen_paths != maps
        || exercise.dataset_reads == 0
        || exercise.committed_mutations < 6
        || exercise.rollback_controls == 0
        || exercise.denial_controls == 0
        || exercise.restart_controls == 0
        || exercise.concurrency_controls == 0
        || exercise.resource_controls == 0
    {
        return Err(CorpusProblem::new(
            "carddemo.online.coverage_drift",
            format!(
                "screens={}; reads={}; mutations={}; rollback={}; denial={}; restart={}; concurrency={}; resources={}",
                exercise.screen_paths,
                exercise.dataset_reads,
                exercise.committed_mutations,
                exercise.rollback_controls,
                exercise.denial_controls,
                exercise.restart_controls,
                exercise.concurrency_controls,
                exercise.resource_controls
            ),
        ));
    }
    let mut shape = Sha256::new();
    digest_field(&mut shape, terminal.corpus_commit.as_bytes());
    digest_field(&mut shape, &(programs as u64).to_be_bytes());
    digest_field(&mut shape, &(transactions as u64).to_be_bytes());
    digest_field(&mut shape, &(maps as u64).to_be_bytes());
    for value in [
        exercise.initial_screen_bytes,
        exercise.screen_paths,
        exercise.dataset_reads,
        exercise.committed_mutations,
        exercise.rollback_controls,
        exercise.denial_controls,
        exercise.restart_controls,
        exercise.concurrency_controls,
        exercise.resource_controls,
    ] {
        digest_field(&mut shape, &(value as u64).to_be_bytes());
    }
    for observation in &exercise.observations {
        digest_field(&mut shape, observation.as_bytes());
    }
    Ok(CardDemoBaseOnlineReceipt {
        schema_version: "mainframe-env.carddemo-base-online-receipt@1".into(),
        status: "pass".into(),
        corpus_commit: terminal.corpus_commit,
        journeys_passed: 9,
        programs_installed: programs,
        source_backed_transactions: transactions,
        maps_installed: maps,
        initial_screen_bytes: exercise.initial_screen_bytes,
        screen_paths: exercise.screen_paths,
        dataset_reads: exercise.dataset_reads,
        committed_mutations: exercise.committed_mutations,
        rollback_controls: exercise.rollback_controls,
        denial_controls: exercise.denial_controls,
        restart_controls: exercise.restart_controls,
        concurrency_controls: exercise.concurrency_controls,
        resource_controls: exercise.resource_controls,
        install_replay: exercise.install_replay,
        journey_shape_sha256: format!("{:x}", shape.finalize()),
    })
}

pub fn verify_carddemo_jcl_from_env(
    inventory_path: &Path,
) -> Result<CardDemoJclReceipt, CorpusProblem> {
    let corpus = verify_carddemo_corpus_from_env(inventory_path)?;
    let corpus_dir = PathBuf::from(env::var_os(CORPUS_ENV).ok_or_else(|| {
        CorpusProblem::new(
            "carddemo.corpus.environment_missing",
            "CARDDEMO_CORPUS_DIR is required",
        )
    })?);
    let jcl_paths = collect_paths(
        &corpus_dir,
        &[
            "app/jcl",
            "app/app-authorization-ims-db2-mq/jcl",
            "app/app-transaction-type-db2/jcl",
        ],
        "jcl",
    )?;
    let procedure_paths = collect_paths(&corpus_dir, &["app/proc"], "prc")?;
    if jcl_paths.len() != 46 || procedure_paths.len() != 2 {
        return Err(CorpusProblem::new(
            "carddemo.jcl.corpus_count_drift",
            format!(
                "expected 46 JCL files and two procedures but found {} and {}",
                jcl_paths.len(),
                procedure_paths.len()
            ),
        ));
    }
    let procedures = procedure_paths
        .iter()
        .map(|relative| {
            let name = Path::new(relative)
                .file_stem()
                .and_then(|value| value.to_str())
                .ok_or_else(|| {
                    CorpusProblem::new(
                        "carddemo.jcl.path_invalid",
                        "procedure name is not repository-relative UTF-8",
                    )
                })?
                .to_ascii_uppercase();
            let bytes = read_corpus_file(&corpus_dir, &corpus_dir.join(relative))?;
            let source = String::from_utf8(bytes).map_err(|_| {
                CorpusProblem::new(
                    "carddemo.jcl.source_invalid",
                    format!("procedure {relative} is not UTF-8"),
                )
            })?;
            Ok((name, source))
        })
        .collect::<Result<BTreeMap<_, _>, CorpusProblem>>()?;
    let limits = JclLimits::default();
    let mut parsed_files = 0usize;
    let mut accepted_unsupported_files = 0usize;
    let mut file_results = BTreeMap::new();
    let mut jobs = 0usize;
    let mut steps = 0usize;
    let mut dds = 0usize;
    let mut continuation_lines = 0usize;
    let mut symbols = 0usize;
    let mut conditions = 0usize;
    let mut procedure_libraries = 0usize;
    let mut procedure_steps = 0usize;
    let mut procedure_overrides = 0usize;
    let mut dd_concatenations = 0usize;
    let mut instream_dds = 0usize;
    let mut instream_records = 0usize;
    let mut provenance_spans = 0usize;
    let mut shape = Sha256::new();
    for relative in &jcl_paths {
        let bytes = read_corpus_file(&corpus_dir, &corpus_dir.join(relative))?;
        let source = String::from_utf8(bytes.clone()).map_err(|_| {
            CorpusProblem::new(
                "carddemo.jcl.source_invalid",
                format!("JCL {relative} is not UTF-8"),
            )
        })?;
        match parse_jcl(
            &JclBundle {
                primary: source.clone(),
                cataloged_procedures: procedures.clone(),
                ..Default::default()
            },
            limits,
        ) {
            Ok(plan) => {
                if relative == "app/jcl/CREASTMT.JCL" {
                    return Err(CorpusProblem::new(
                        "carddemo.jcl.correction_drift",
                        "CREASTMT unexpectedly parsed despite its pinned orphan continuation",
                    ));
                }
                parsed_files += 1;
                file_results.insert(relative.clone(), "parsed".into());
                jobs += 1;
                steps += plan.steps.len();
                symbols += plan.symbols.len();
                procedure_libraries += plan.procedure_libraries.len();
                for pair in source.lines().collect::<Vec<_>>().windows(2) {
                    let previous = pair[0].get(..72.min(pair[0].len())).unwrap_or(pair[0]);
                    let current = pair[1].get(2..72.min(pair[1].len())).unwrap_or("");
                    continuation_lines +=
                        usize::from(previous.trim_end().ends_with(',') && current.starts_with(' '));
                }
                procedure_overrides += source
                    .lines()
                    .filter(|line| {
                        line.starts_with("//")
                            && line
                                .get(2..72.min(line.len()))
                                .filter(|line| !line.starts_with(char::is_whitespace))
                                .and_then(|line| line.split_whitespace().next())
                                .is_some_and(|name| name.contains('.'))
                    })
                    .count();
                for step in &plan.steps {
                    conditions += usize::from(step.condition != StepCondition::Always);
                    procedure_steps += usize::from(step.procedure.is_some());
                    provenance_spans += usize::from(
                        step.source_line > 0 && step.source_end_line >= step.source_line,
                    );
                    provenance_spans += usize::from(step.invocation_line.is_some());
                    for dd in &step.dds {
                        dds += 1;
                        dd_concatenations += usize::from(dd.concatenation);
                        instream_dds += usize::from(!dd.inline_data.is_empty());
                        instream_records += dd
                            .inline_data
                            .split(|byte| *byte == b'\n')
                            .filter(|record| !record.is_empty())
                            .count();
                        provenance_spans += usize::from(
                            dd.source_line > 0 && dd.source_end_line >= dd.source_line,
                        );
                    }
                }
                for dd in &plan.job_dds {
                    dds += 1;
                    dd_concatenations += usize::from(dd.concatenation);
                    provenance_spans += usize::from(
                        dd.source_line > 0 && dd.source_end_line >= dd.source_line,
                    );
                }
                digest_field(&mut shape, relative.as_bytes());
                digest_field(
                    &mut shape,
                    &serde_json::to_vec(&plan).map_err(|error| {
                        CorpusProblem::new("carddemo.jcl.receipt_invalid", error.to_string())
                    })?,
                );
            }
            Err(mainframe_env_host_api::HostProblem::Unsupported)
                if relative == "app/jcl/CREASTMT.JCL"
                    && sha256(&bytes)
                        == "d32627022d7686d1811ba253faa2c5d86295c8055428b2a610a27d8f84751950"
                    && source.contains(
                        "SPACE=(CYL,(1,1),RLSE), 00,RECFM=FB), ATA.VSAM.KSDS\n//         DSN=AWS.M2.CARDDEMO.STATEMNT.PS",
                    ) =>
            {
                accepted_unsupported_files += 1;
                file_results.insert(
                    relative.clone(),
                    "accepted_unsupported:pinned-orphan-dd-continuation".into(),
                );
                digest_field(&mut shape, relative.as_bytes());
                digest_field(&mut shape, sha256(&bytes).as_bytes());
                digest_field(&mut shape, b"pinned-orphan-dd-continuation");
            }
            Err(problem) => {
                let syntax = analyze_jcl_syntax(
                    &JclBundle {
                        primary: source.clone(),
                        cataloged_procedures: procedures.clone(),
                        ..Default::default()
                    },
                    JclConversionLimits::default().syntax,
                )
                .ok();
                let unknown_records = syntax
                    .as_ref()
                    .into_iter()
                    .flat_map(|analysis| analysis.syntax().records())
                    .filter(|record| record.kind() == JclRecordKind::Statement)
                    .filter_map(|record| {
                        let fields = record.fields()?;
                        let operation = syntax
                            .as_ref()?
                            .syntax()
                            .text()
                            .get(fields.operation().clone())?;
                        JclStatementId::from_keyword(operation)
                            .is_none()
                            .then(|| format!("{}:{operation}", record.line()))
                    })
                    .collect::<Vec<_>>();
                let diagnostics = convert_jcl(
                    &JclBundle {
                        primary: source.clone(),
                        cataloged_procedures: procedures.clone(),
                        ..Default::default()
                    },
                    JclConversionLimits::default(),
                )
                .map(|conversion| {
                    conversion
                        .diagnostics()
                        .iter()
                        .map(|diagnostic| {
                            format!(
                                "{}:{}",
                                diagnostic.code().as_str(),
                                diagnostic.public_message()
                            )
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
                return Err(CorpusProblem::new(
                    "carddemo.jcl.parse_failed",
                    format!("JCL {relative} failed with {problem:?}; diagnostics={diagnostics:?}; unknown_records={unknown_records:?}"),
                ));
            }
        }
    }
    let negative_controls = verify_jcl_negative_controls(limits)?;
    if parsed_files != 45
        || accepted_unsupported_files != 1
        || file_results.len() != 46
        || jobs != 45
        || steps == 0
        || dds == 0
        || continuation_lines == 0
        || symbols == 0
        || conditions == 0
        || procedure_libraries == 0
        || procedure_steps == 0
        || procedure_overrides == 0
        || dd_concatenations == 0
        || instream_dds == 0
        || instream_records == 0
        || provenance_spans < steps + dds + procedure_steps
        || negative_controls != 4
    {
        return Err(CorpusProblem::new(
            "carddemo.jcl.coverage_drift",
            format!(
                "JCL parsing observations differ: parsed={parsed_files} unsupported={accepted_unsupported_files} jobs={jobs} steps={steps} dds={dds} continuations={continuation_lines} symbols={symbols} conditions={conditions} procedure_libraries={procedure_libraries} procedure_steps={procedure_steps} procedure_overrides={procedure_overrides} concatenations={dd_concatenations} instream_dds={instream_dds} instream_records={instream_records} provenance={provenance_spans} negative={negative_controls}"
            ),
        ));
    }
    Ok(CardDemoJclReceipt {
        schema_version: "mainframe-env.carddemo-jcl-receipt@1".into(),
        status: "pass".into(),
        corpus_commit: corpus.commit,
        jcl_files: jcl_paths.len(),
        procedures: procedure_paths.len(),
        parsed_files,
        accepted_unsupported_files,
        file_results,
        jobs,
        steps,
        dds,
        continuation_lines,
        symbols,
        conditions,
        procedure_libraries,
        procedure_steps,
        procedure_overrides,
        dd_concatenations,
        instream_dds,
        instream_records,
        provenance_spans,
        negative_controls,
        jcl_shape_sha256: format!("{:x}", shape.finalize()),
    })
}

fn verify_jcl_negative_controls(limits: JclLimits) -> Result<usize, CorpusProblem> {
    let cases = [
        (
            JclBundle::default(),
            limits,
            mainframe_env_host_api::HostProblem::ResourceExhausted,
        ),
        (
            JclBundle {
                primary: "NOT JCL".into(),
                ..Default::default()
            },
            limits,
            mainframe_env_host_api::HostProblem::Malformed,
        ),
        (
            JclBundle {
                primary: "12345".into(),
                ..Default::default()
            },
            JclLimits {
                max_source_bytes: 4,
                ..limits
            },
            mainframe_env_host_api::HostProblem::ResourceExhausted,
        ),
        (
            JclBundle {
                primary: "//J JOB CLASS=A\n//S EXEC PROC=MISSING\n".into(),
                ..Default::default()
            },
            limits,
            mainframe_env_host_api::HostProblem::NotFound,
        ),
    ];
    for (bundle, limits, expected) in cases {
        if parse_jcl(&bundle, limits) != Err(expected) {
            return Err(CorpusProblem::new(
                "carddemo.jcl.negative_control_failed",
                "malformed, missing, or resource-bounded JCL did not fail stably",
            ));
        }
    }
    Ok(4)
}

pub fn verify_carddemo_utilities_from_env(
    inventory_path: &Path,
) -> Result<CardDemoUtilityReceipt, CorpusProblem> {
    let corpus = verify_carddemo_corpus_from_env(inventory_path)?;
    let corpus_dir = PathBuf::from(env::var_os(CORPUS_ENV).ok_or_else(|| {
        CorpusProblem::new(
            "carddemo.corpus.environment_missing",
            "CARDDEMO_CORPUS_DIR is required",
        )
    })?);
    let plans = carddemo_parsed_jcl(&corpus_dir)?;
    let mut idcams_steps = 0usize;
    let mut inline_idcams_controls = 0usize;
    let mut idcams_statements = 0usize;
    let mut reached_programs = BTreeSet::new();
    let mut shape = Sha256::new();
    for (relative, plan) in &plans {
        digest_field(&mut shape, relative.as_bytes());
        for step in &plan.steps {
            reached_programs.insert(step.program.clone());
            if step.program == "IDCAMS" {
                idcams_steps += 1;
                if let Some(control) = step
                    .dds
                    .iter()
                    .find(|dd| dd.name == "SYSIN" && !dd.inline_data.is_empty())
                {
                    inline_idcams_controls += 1;
                    idcams_statements +=
                        validate_idcams_control(&control.inline_data).map_err(|problem| {
                            CorpusProblem::new(
                                "carddemo.utility.idcams_invalid",
                                format!(
                                    "{relative} contains unsupported IDCAMS control: {problem:?}"
                                ),
                            )
                        })?;
                }
            }
        }
    }
    let implemented_utilities = ["IDCAMS", "IEBGENER", "SORT", "IEFBR14"]
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>();
    let explicit_external_utilities: BTreeMap<String, String> = BTreeMap::from([
        ("FTP".into(), "network-ftp-explicit-unsupported".into()),
        ("IKJEFT1B".into(), "report-rexx-explicit-unsupported".into()),
        (
            "SDSF".into(),
            "cics-file-control-explicit-unsupported".into(),
        ),
    ]);
    if implemented_utilities.iter().any(|program| {
        !reached_programs.contains(program)
            || utility_disposition(program) != Some(UtilityDisposition::Implemented)
    }) || explicit_external_utilities.keys().any(|program| {
        !reached_programs.contains(program)
            || utility_disposition(program)
                .is_none_or(|value| value == UtilityDisposition::Implemented)
    }) {
        return Err(CorpusProblem::new(
            "carddemo.utility.route_drift",
            "reached utility dispositions differ from the explicit catalog",
        ));
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| CorpusProblem::new("carddemo.utility.runtime", error.to_string()))?;
    let exercise = runtime.block_on(exercise_utility_routes())?;
    let unknown_controls = usize::from(
        validate_idcams_control(b"UNKNOWN CONTROL")
            == Err(mainframe_env_host_api::HostProblem::Unsupported),
    ) + usize::from(utility_disposition("NOTREAL").is_none());
    if plans.len() != 45
        || idcams_steps == 0
        || inline_idcams_controls == 0
        || idcams_statements == 0
        || exercise.selected_job_routes != 7
        || exercise.exact_dataset_mutations != 7
        || exercise.disposition_controls != 1
        || exercise.gdg_controls != 1
        || exercise.aix_controls != 1
        || exercise.internal_reader_controls != 1
        || unknown_controls != 2
    {
        return Err(CorpusProblem::new(
            "carddemo.utility.coverage_drift",
            "utility execution observations differ from the bounded contract",
        ));
    }
    for value in [
        idcams_steps,
        inline_idcams_controls,
        idcams_statements,
        exercise.selected_job_routes,
        exercise.exact_dataset_mutations,
        exercise.disposition_controls,
        exercise.gdg_controls,
        exercise.aix_controls,
        exercise.internal_reader_controls,
        unknown_controls,
    ] {
        digest_field(&mut shape, &(value as u64).to_be_bytes());
    }
    Ok(CardDemoUtilityReceipt {
        schema_version: "mainframe-env.carddemo-utility-receipt@1".into(),
        status: "pass".into(),
        corpus_commit: corpus.commit,
        jcl_files: 46,
        parsed_jobs: plans.len(),
        idcams_steps,
        inline_idcams_controls,
        idcams_statements,
        implemented_utilities,
        explicit_external_utilities,
        selected_job_routes: exercise.selected_job_routes,
        exact_dataset_mutations: exercise.exact_dataset_mutations,
        disposition_controls: exercise.disposition_controls,
        gdg_controls: exercise.gdg_controls,
        aix_controls: exercise.aix_controls,
        internal_reader_controls: exercise.internal_reader_controls,
        unknown_controls,
        utility_shape_sha256: format!("{:x}", shape.finalize()),
    })
}

pub fn verify_carddemo_batch_programs_from_env(
    inventory_path: &Path,
) -> Result<CardDemoBatchProgramReceipt, CorpusProblem> {
    let corpus = verify_carddemo_corpus_from_env(inventory_path)?;
    let corpus_dir = PathBuf::from(env::var_os(CORPUS_ENV).ok_or_else(|| {
        CorpusProblem::new(
            "carddemo.corpus.environment_missing",
            "CARDDEMO_CORPUS_DIR is required",
        )
    })?);
    let plans = carddemo_parsed_jcl(&corpus_dir)?;
    let bundles = explicit_carddemo_bundles(&corpus_dir)?
        .into_iter()
        .filter(|(path, _)| path.starts_with("app/cbl/"))
        .map(|(path, bundle)| {
            let name = Path::new(&path)
                .file_stem()
                .and_then(|value| value.to_str())
                .ok_or_else(|| {
                    CorpusProblem::new(
                        "carddemo.batch_program.path_invalid",
                        "program path is invalid",
                    )
                })?
                .to_ascii_uppercase();
            Ok((name, (path, bundle)))
        })
        .collect::<Result<BTreeMap<_, _>, CorpusProblem>>()?;
    let mut named = plans
        .iter()
        .flat_map(|(_, plan)| plan.steps.iter())
        .map(|step| step.program.to_ascii_uppercase())
        .filter(|program| bundles.contains_key(program))
        .collect::<BTreeSet<_>>();
    for relative in collect_paths(&corpus_dir, &["app/jcl", "app/proc"], "jcl")?
        .into_iter()
        .chain(collect_paths(&corpus_dir, &["app/proc"], "prc")?)
    {
        let source = String::from_utf8(read_corpus_file(&corpus_dir, &corpus_dir.join(&relative))?)
            .map_err(|_| {
                CorpusProblem::new(
                    "carddemo.batch_program.jcl_invalid",
                    format!("{relative} is not UTF-8"),
                )
            })?;
        for line in source.lines() {
            let upper = line
                .get(..72.min(line.len()))
                .unwrap_or(line)
                .to_ascii_uppercase();
            let Some(start) = upper.find("PGM=") else {
                continue;
            };
            let program = upper[start + 4..]
                .chars()
                .take_while(|character| {
                    character.is_ascii_alphanumeric() || "@#$".contains(*character)
                })
                .collect::<String>();
            if bundles.contains_key(&program) {
                named.insert(program);
            }
        }
    }
    let compiler = CobolCompiler::default();
    let mut called = BTreeSet::new();
    for name in &named {
        let (path, bundle) = bundles.get(name).ok_or_else(|| {
            CorpusProblem::new(
                "carddemo.batch_program.source_missing",
                format!("{name} source is missing"),
            )
        })?;
        let hir = compiler.analyze(bundle).hir.ok_or_else(|| {
            CorpusProblem::new(
                "carddemo.batch_program.analysis_failed",
                format!("{path} did not produce HIR"),
            )
        })?;
        for statement in hir
            .statements
            .iter()
            .filter(|statement| statement.kind == StatementKind::Call)
        {
            if let Some(target) = statement.arguments.first() {
                called.insert(target.trim_matches(['\'', '"']).to_ascii_uppercase());
            }
        }
    }
    let called_programs = called
        .iter()
        .filter(|program| bundles.contains_key(*program) && !named.contains(*program))
        .cloned()
        .collect::<BTreeSet<_>>();
    let services = compatible_system_services()
        .iter()
        .map(|name| (*name).to_string())
        .collect::<BTreeSet<_>>();
    if named.len() != 11
        || called_programs != BTreeSet::from(["CBSTM03B".into()])
        || services
            != BTreeSet::from([
                "CEEDAYS".into(),
                "COBDATFT".into(),
                "MVSWAIT".into(),
                "CEE3ABD".into(),
            ])
        || !called
            .iter()
            .all(|name| bundles.contains_key(name) || services.contains(name) || name == "CBLTDLI")
    {
        return Err(CorpusProblem::new(
            "carddemo.batch_program.catalog_drift",
            format!(
                "named={named:?}; called_programs={called_programs:?}; called={called:?}; services={services:?}"
            ),
        ));
    }
    let needed = named
        .iter()
        .chain(called_programs.iter())
        .cloned()
        .collect::<BTreeSet<_>>();
    let mut shape = Sha256::new();
    let definitions = compile_carddemo_batch_definitions(&bundles, &needed)?;
    for definition in &definitions {
        digest_field(&mut shape, definition.name.as_bytes());
        digest_field(&mut shape, definition.artifact.as_str().as_bytes());
    }
    let wait_jcl = read_corpus_file(&corpus_dir, &corpus_dir.join("app/jcl/WAITSTEP.jcl"))?;
    let linkage_fixture = compile_free_batch_definition(
        "LINKMAIN",
        "IDENTIFICATION DIVISION.\nPROGRAM-ID. LINKMAIN.\nDATA DIVISION.\nWORKING-STORAGE SECTION.\n01 AREA.\n 05 AREA-DD PIC X(8) VALUE 'TRNXFILE'.\n 05 AREA-OPER PIC X VALUE 'O'.\n 05 AREA-RC PIC X(2).\n 05 AREA-KEY PIC X(25).\n 05 AREA-KEY-LN PIC S9(4) VALUE 0.\n 05 AREA-DATA PIC X(1000).\nPROCEDURE DIVISION.\nCALL 'CBSTM03B' USING AREA.\nMOVE 'R' TO AREA-OPER.\nCALL 'CBSTM03B' USING AREA.\nDISPLAY AREA-RC.\nDISPLAY AREA-DATA(1:4).\nMOVE 'C' TO AREA-OPER.\nCALL 'CBSTM03B' USING AREA.\nSTOP RUN.\n",
    )?;
    let abend_fixture = compile_free_batch_definition(
        "ABENDCHK",
        "IDENTIFICATION DIVISION.\nPROGRAM-ID. ABENDCHK.\nDATA DIVISION.\nWORKING-STORAGE SECTION.\n01 ABCODE PIC S9(9) COMP VALUE 999.\n01 TIMING PIC S9(9) COMP VALUE 0.\nPROCEDURE DIVISION.\nCALL 'CEE3ABD' USING ABCODE TIMING.\nSTOP RUN.\n",
    )?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| CorpusProblem::new("carddemo.batch_program.runtime", error.to_string()))?;
    let exercise = runtime.block_on(exercise_batch_program_routes(
        definitions.clone(),
        linkage_fixture,
        abend_fixture,
        &wait_jcl,
    ))?;
    for value in [
        named.len(),
        called_programs.len(),
        services.len(),
        definitions.len(),
        exercise.selected_job_routes,
        exercise.linkage_routes,
        exercise.file_routes,
        exercise.abend_controls,
        1,
        1,
        1,
        exercise.restart_routes,
    ] {
        digest_field(&mut shape, &(value as u64).to_be_bytes());
    }
    Ok(CardDemoBatchProgramReceipt {
        schema_version: "mainframe-env.carddemo-batch-program-receipt@1".into(),
        status: "pass".into(),
        corpus_commit: corpus.commit,
        named_programs: named.into_iter().collect(),
        called_programs: called_programs.into_iter().collect(),
        compatible_services: services.into_iter().collect(),
        compiled_artifacts: definitions.len(),
        installed_artifacts: exercise.installed_artifacts,
        install_replay: exercise.install_replay,
        selected_job_routes: exercise.selected_job_routes,
        linkage_routes: exercise.linkage_routes,
        file_routes: exercise.file_routes,
        abend_controls: exercise.abend_controls,
        checkpoint_controls: 1,
        cancellation_controls: 1,
        timeout_controls: 1,
        restart_routes: exercise.restart_routes,
        program_shape_sha256: format!("{:x}", shape.finalize()),
    })
}

pub fn verify_carddemo_base_batch_from_env(
    inventory_path: &Path,
) -> Result<CardDemoBaseBatchReceipt, CorpusProblem> {
    let batch = verify_carddemo_batch_programs_from_env(inventory_path)?;
    let corpus_dir = PathBuf::from(env::var_os(CORPUS_ENV).ok_or_else(|| {
        CorpusProblem::new(
            "carddemo.corpus.environment_missing",
            "CARDDEMO_CORPUS_DIR is required",
        )
    })?);
    let bundles = explicit_carddemo_bundles(&corpus_dir)?
        .into_iter()
        .filter(|(path, _)| path.starts_with("app/cbl/"))
        .map(|(path, bundle)| {
            let name = Path::new(&path)
                .file_stem()
                .and_then(|value| value.to_str())
                .ok_or_else(|| {
                    CorpusProblem::new(
                        "carddemo.base_batch.path_invalid",
                        "program path is invalid",
                    )
                })?
                .to_ascii_uppercase();
            Ok((name, (path, bundle)))
        })
        .collect::<Result<BTreeMap<_, _>, CorpusProblem>>()?;
    let needed = batch
        .named_programs
        .iter()
        .chain(batch.called_programs.iter())
        .cloned()
        .collect::<BTreeSet<_>>();
    let definitions = compile_carddemo_batch_definitions(&bundles, &needed)?;
    let online = carddemo_base_online_definition(&corpus_dir)?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| CorpusProblem::new("carddemo.base_batch.runtime", error.to_string()))?;
    runtime.block_on(exercise_base_batch_routes(
        &corpus_dir,
        batch.corpus_commit,
        online,
        definitions,
    ))
}

pub fn verify_carddemo_db2_from_env(
    inventory_path: &Path,
) -> Result<CardDemoDb2Receipt, CorpusProblem> {
    let corpus_dir = PathBuf::from(env::var_os(CORPUS_ENV).ok_or_else(|| {
        CorpusProblem::new(
            "carddemo.corpus.environment_missing",
            "CARDDEMO_CORPUS_DIR is required",
        )
    })?);
    let corpus = verify_carddemo_corpus(&corpus_dir, inventory_path)?;
    let compiler = CobolCompiler::default();
    let mut sql_include_expansions = 0usize;
    let mut sql_operations = BTreeMap::new();
    let bundles = carddemo_db2_bundles(&corpus_dir)?;
    for (relative, bundle) in &bundles {
        let analysis = compiler.analyze(bundle);
        if analysis.completeness != Completeness::Complete {
            return Err(CorpusProblem::new(
                "carddemo.db2.compile_failed",
                format!(
                    "{relative}: {}",
                    analysis
                        .diagnostics
                        .first()
                        .map_or("incomplete Db2 compilation", |problem| problem
                            .public_message())
                ),
            ));
        }
        let syntax = analysis.syntax.ok_or_else(|| {
            CorpusProblem::new("carddemo.db2.compile_failed", "Db2 syntax is missing")
        })?;
        sql_include_expansions += syntax
            .expansions()
            .iter()
            .filter(|expansion| {
                matches!(
                    expansion.source.as_str().rsplit('/').next(),
                    Some(
                        "SQLCA.cpy"
                            | "DCLTRTYP.dcl"
                            | "DCLTRCAT.dcl"
                            | "CSDB2RWY.cpy"
                            | "CSDB2RPY.cpy"
                    )
                )
            })
            .count();
        for statement in analysis
            .hir
            .ok_or_else(|| CorpusProblem::new("carddemo.db2.compile_failed", "Db2 HIR is missing"))?
            .statements
            .into_iter()
            .filter(|statement| statement.kind == StatementKind::ExecSql)
        {
            let opcode = statement
                .arguments
                .iter()
                .find(|token| !matches!(token.as_str(), "SQL" | "END-EXEC"))
                .ok_or_else(|| {
                    CorpusProblem::new("carddemo.db2.compile_failed", "SQL statement has no opcode")
                })?;
            *sql_operations.entry(opcode.clone()).or_default() += 1;
        }
        let result = compiler
            .compile(CompilerRequest {
                source: bundle.clone(),
                mode: CompilationMode::Executable,
                target: CompileTarget::new("reference").expect("static target"),
                options: CompileOptions::new(BTreeMap::new()).expect("static options"),
            })
            .map_err(|problem| {
                CorpusProblem::new(
                    "carddemo.db2.compile_failed",
                    format!("{relative}: {problem:?}"),
                )
            })?;
        if !matches!(result, CompilerResult::Published { .. }) {
            return Err(CorpusProblem::new(
                "carddemo.db2.compile_failed",
                format!("{relative} did not publish"),
            ));
        }
    }
    let programs_compiled = bundles.len();
    let db2_catalog = bundles
        .iter()
        .map(|(relative, bundle)| {
            let name = Path::new(relative)
                .file_stem()
                .and_then(|value| value.to_str())
                .ok_or_else(|| {
                    CorpusProblem::new("carddemo.db2.program_invalid", "program name is invalid")
                })?
                .to_ascii_uppercase();
            Ok((name, (relative.clone(), bundle.clone())))
        })
        .collect::<Result<BTreeMap<_, _>, CorpusProblem>>()?;
    let definitions = compile_carddemo_batch_definitions(
        &db2_catalog,
        &BTreeSet::from(["COBTUPDT".to_string()]),
    )?;
    let online = carddemo_db2_online_definition(&corpus_dir)?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| CorpusProblem::new("carddemo.db2.runtime", error.to_string()))?;
    let exercise = runtime.block_on(exercise_db2_routes(&corpus_dir, online, definitions))?;
    let ddl_files = collect_paths(&corpus_dir, &["app/app-transaction-type-db2/ddl"], "ddl")?.len();
    let mut shape = Sha256::new();
    digest_field(&mut shape, corpus.commit.as_bytes());
    for (opcode, count) in &sql_operations {
        digest_field(&mut shape, opcode.as_bytes());
        digest_field(&mut shape, &(*count as u64).to_be_bytes());
    }
    for map in [
        &exercise.table_sha256,
        &exercise.dataset_sha256,
        &exercise.spool_sha256,
    ] {
        for (name, digest) in map {
            digest_field(&mut shape, name.as_bytes());
            digest_field(&mut shape, digest.as_bytes());
        }
    }
    Ok(CardDemoDb2Receipt {
        schema_version: "mainframe-env.carddemo-db2-receipt@1".into(),
        status: "pass".into(),
        corpus_commit: corpus.commit,
        programs_compiled,
        sql_include_expansions,
        sql_operations,
        ddl_files,
        online_routes: exercise.online_routes,
        batch_routes: exercise.batch_routes,
        extraction_records: exercise.extraction_records,
        authorization_controls: exercise.authorization_controls,
        restart_controls: exercise.restart_controls,
        rollback_controls: exercise.rollback_controls,
        conflict_controls: exercise.conflict_controls,
        failure_controls: exercise.failure_controls,
        table_sha256: exercise.table_sha256,
        dataset_sha256: exercise.dataset_sha256,
        spool_sha256: exercise.spool_sha256,
        db2_shape_sha256: format!("{:x}", shape.finalize()),
    })
}

fn carddemo_package_trust() -> Result<Arc<HmacSha256PackageTrust>, CorpusProblem> {
    let resolver = Arc::new(MemorySecretResolver::default());
    resolver.insert(
        "secret:carddemo-package-key",
        b"carddemo-conformance-hmac-key-0001".to_vec(),
    );
    HmacSha256PackageTrust::new(
        BTreeMap::from([(
            "carddemo-conformance-key".into(),
            SecretRef::new("secret:carddemo-package-key", Default::default())
                .map_err(terminal_problem)?,
        )]),
        resolver,
    )
    .map(Arc::new)
    .map_err(terminal_problem)
}

fn sign_carddemo_package_identity(identity: &str) -> String {
    base64::engine::general_purpose::STANDARD_NO_PAD.encode(hmac::sign(
        &hmac::Key::new(hmac::HMAC_SHA256, b"carddemo-conformance-hmac-key-0001"),
        identity.as_bytes(),
    ))
}

fn carddemo_db2_column(
    name: &str,
    nullable: bool,
    max_bytes: usize,
    result_encoding: Db2ResultEncoding,
) -> Db2ColumnDefinition {
    Db2ColumnDefinition {
        name: name.into(),
        nullable,
        max_bytes,
        result_encoding,
        default_value: None,
    }
}

fn carddemo_db2_definitions() -> Vec<Db2TableDefinition> {
    let transaction = Db2TableDefinition {
        name: "CARDDEMO.TRANSACTION_TYPE".into(),
        columns: vec![
            carddemo_db2_column("TR_TYPE", false, 2, Db2ResultEncoding::Raw),
            carddemo_db2_column("TR_DESCRIPTION", false, 50, Db2ResultEncoding::Varchar),
        ],
        primary_key: vec!["TR_TYPE".into()],
        foreign_keys: Vec::new(),
        extract: Some(Db2ExtractLayout {
            fields: vec![
                Db2ExtractField {
                    column: "TR_TYPE".into(),
                    width: 2,
                },
                Db2ExtractField {
                    column: "TR_DESCRIPTION".into(),
                    width: 50,
                },
            ],
            trailer: b"00000000".to_vec(),
        }),
    };
    let category = Db2TableDefinition {
        name: "CARDDEMO.TRANSACTION_TYPE_CATEGORY".into(),
        columns: vec![
            carddemo_db2_column("TRC_TYPE_CODE", false, 2, Db2ResultEncoding::Raw),
            carddemo_db2_column("TRC_TYPE_CATEGORY", false, 4, Db2ResultEncoding::Raw),
            carddemo_db2_column("TRC_CAT_DATA", false, 50, Db2ResultEncoding::Varchar),
        ],
        primary_key: vec!["TRC_TYPE_CODE".into(), "TRC_TYPE_CATEGORY".into()],
        foreign_keys: vec![Db2ForeignKeyDefinition {
            columns: vec!["TRC_TYPE_CODE".into()],
            referenced_table: "CARDDEMO.TRANSACTION_TYPE".into(),
            referenced_columns: vec!["TR_TYPE".into()],
            delete_restrict: true,
        }],
        extract: Some(Db2ExtractLayout {
            fields: vec![
                Db2ExtractField {
                    column: "TRC_TYPE_CODE".into(),
                    width: 2,
                },
                Db2ExtractField {
                    column: "TRC_TYPE_CATEGORY".into(),
                    width: 4,
                },
                Db2ExtractField {
                    column: "TRC_CAT_DATA".into(),
                    width: 50,
                },
            ],
            trailer: b"0000".to_vec(),
        }),
    };
    let authorization_columns = [
        ("CARD_NUM", 16, Db2ResultEncoding::Raw),
        ("AUTH_TS", 256, Db2ResultEncoding::Raw),
        ("AUTH_TYPE", 4, Db2ResultEncoding::Raw),
        ("CARD_EXPIRY_DATE", 4, Db2ResultEncoding::Raw),
        ("MESSAGE_TYPE", 6, Db2ResultEncoding::Raw),
        ("MESSAGE_SOURCE", 6, Db2ResultEncoding::Raw),
        ("AUTH_ID_CODE", 6, Db2ResultEncoding::Raw),
        ("AUTH_RESP_CODE", 2, Db2ResultEncoding::Raw),
        ("AUTH_RESP_REASON", 4, Db2ResultEncoding::Raw),
        ("PROCESSING_CODE", 6, Db2ResultEncoding::Raw),
        ("TRANSACTION_AMT", 12, Db2ResultEncoding::Raw),
        ("APPROVED_AMT", 12, Db2ResultEncoding::Raw),
        ("MERCHANT_CATAGORY_CODE", 4, Db2ResultEncoding::Raw),
        ("ACQR_COUNTRY_CODE", 3, Db2ResultEncoding::Raw),
        ("POS_ENTRY_MODE", 256, Db2ResultEncoding::Raw),
        ("MERCHANT_ID", 15, Db2ResultEncoding::Raw),
        ("MERCHANT_NAME", 22, Db2ResultEncoding::Varchar),
        ("MERCHANT_CITY", 13, Db2ResultEncoding::Raw),
        ("MERCHANT_STATE", 2, Db2ResultEncoding::Raw),
        ("MERCHANT_ZIP", 9, Db2ResultEncoding::Raw),
        ("TRANSACTION_ID", 15, Db2ResultEncoding::Raw),
        ("MATCH_STATUS", 1, Db2ResultEncoding::Raw),
        ("AUTH_FRAUD", 1, Db2ResultEncoding::Raw),
        ("FRAUD_RPT_DATE", 256, Db2ResultEncoding::Raw),
        ("ACCT_ID", 11, Db2ResultEncoding::Raw),
        ("CUST_ID", 9, Db2ResultEncoding::Raw),
    ];
    let authorization = Db2TableDefinition {
        name: "CARDDEMO.AUTHFRDS".into(),
        columns: authorization_columns
            .into_iter()
            .map(|(name, max_bytes, encoding)| {
                carddemo_db2_column(
                    name,
                    !matches!(name, "CARD_NUM" | "AUTH_TS"),
                    max_bytes,
                    encoding,
                )
            })
            .collect(),
        primary_key: vec!["CARD_NUM".into(), "AUTH_TS".into()],
        foreign_keys: Vec::new(),
        extract: None,
    };
    vec![transaction, category, authorization]
}

fn carddemo_batch_controller(
    name: &str,
    kind: BatchControllerKind,
    properties: &[(&str, &str)],
) -> BatchController {
    let program = properties
        .iter()
        .find_map(|(name, value)| (*name == "selector-program").then_some(*value))
        .expect("every CardDemo controller has a selector program");
    BatchController {
        name: name.into(),
        program: format!("program/{program}"),
        kind,
        properties: properties
            .iter()
            .map(|(name, value)| ((*name).into(), (*value).into()))
            .collect(),
    }
}

fn carddemo_batch_controllers() -> Vec<BatchController> {
    vec![
        carddemo_batch_controller(
            "TRANSACTION-TYPE-MAINTENANCE",
            BatchControllerKind::CobolProgram,
            &[
                ("launcher", "tso-run"),
                ("selector-program", "COBTUPDT"),
                ("behavior", "program-call"),
            ],
        ),
        carddemo_batch_controller(
            "AUTHORIZATION-IMS-LOAD",
            BatchControllerKind::ImsMessageProcessing,
            &[
                ("launcher", "ims-controller"),
                ("selector-mode", "BMP"),
                ("selector-program", "PAUDBLOD"),
                ("selector-qualifier", "PSBPAUTB"),
                ("behavior", "ims-load"),
                ("database", "DBPAUTP0"),
                ("root-dd", "INFILE1"),
                ("child-dd", "INFILE2"),
                ("root-record-bytes", "100"),
                ("child-record-bytes", "206"),
                ("parent-key-bytes", "6"),
            ],
        ),
        carddemo_batch_controller(
            "AUTHORIZATION-IMS-UNLOAD",
            BatchControllerKind::ImsMessageProcessing,
            &[
                ("launcher", "ims-controller"),
                ("selector-mode", "DLI"),
                ("selector-program", "PAUDBUNL"),
                ("selector-qualifier", "PAUTBUNL"),
                ("behavior", "ims-unload"),
                ("database", "DBPAUTP0"),
                ("root-segment", "PAUTSUM0"),
                ("child-segment", "PAUTDTL1"),
                ("root-output-dd", "OUTFIL1"),
                ("child-output-dd", "OUTFIL2"),
            ],
        ),
        carddemo_batch_controller(
            "AUTHORIZATION-EXPIRY-PURGE",
            BatchControllerKind::ImsMessageProcessing,
            &[
                ("launcher", "ims-controller"),
                ("selector-mode", "BMP"),
                ("selector-program", "CBPAUP0C"),
                ("selector-qualifier", "PSBPAUTB"),
                ("behavior", "ims-purge"),
                ("psb", "PSBPAUTB"),
                ("root-segment", "PAUTSUM0"),
                ("child-segment", "PAUTDTL1"),
                ("control-dd", "SYSIN"),
                ("required-expiry-days", "00"),
                ("checkpoint-prefix", "CD026"),
                ("summary-field", "SUMMARY-AUTHORIZATION-ADJUSTED"),
            ],
        ),
        carddemo_batch_controller(
            "AUTHORIZATION-IMS-IMAGE-UNLOAD",
            BatchControllerKind::DeclarativeUtility,
            &[
                ("launcher", "ims-controller"),
                ("selector-mode", "ULU"),
                ("selector-program", "DFSURGU0"),
                ("selector-qualifier", "DBPAUTP0"),
                ("behavior", "ims-unload"),
                ("database", "DBPAUTP0"),
                ("root-segment", "PAUTSUM0"),
                ("child-segment", "PAUTDTL1"),
                ("combined-output-dd", "DFSURGU1"),
            ],
        ),
    ]
}

fn install_carddemo_db2_package(
    server: &Arc<ProductServer>,
    batch_programs: &[BatchProgramDefinition],
) -> Result<(), CorpusProblem> {
    let definitions = carddemo_db2_definitions();
    let batch_controllers = carddemo_batch_controllers();
    let catalog_bytes = serde_json::to_vec(&definitions)
        .map_err(|error| CorpusProblem::new("carddemo.db2.package", error.to_string()))?;
    let mut payloads: Vec<(EntryKind, String, Vec<u8>)> = vec![
        (
            EntryKind::Source,
            "source/manifest".into(),
            b"source".to_vec(),
        ),
        (
            EntryKind::Resource,
            "resource/manifest".into(),
            b"resource".to_vec(),
        ),
        (EntryKind::Data, "data/db2/catalog".into(), catalog_bytes),
        (
            EntryKind::Profile,
            "profile/manifest".into(),
            b"profile".to_vec(),
        ),
        (
            EntryKind::Migration,
            "migration/manifest".into(),
            b"application-package-v1-to-v2".to_vec(),
        ),
    ];
    for controller in &batch_controllers {
        let program = controller
            .program
            .rsplit('/')
            .next()
            .ok_or_else(|| CorpusProblem::new("carddemo.db2.package", "program is missing"))?;
        let payload = if controller.kind == BatchControllerKind::CobolProgram
            && controller.properties.get("behavior").map(String::as_str) == Some("program-call")
        {
            batch_programs
                .iter()
                .find(|definition| definition.name.eq_ignore_ascii_case(program))
                .map(|definition| definition.payload.clone())
                .unwrap_or_else(|| format!("carddemo-controller:{program}").into_bytes())
        } else {
            format!("carddemo-controller:{program}").into_bytes()
        };
        payloads.push((EntryKind::Program, controller.program.clone(), payload));
    }
    let mut entries = Vec::new();
    let mut blobs = BTreeMap::new();
    for (kind, path, bytes) in payloads {
        let digest = format!("sha256:{:x}", Sha256::digest(&bytes));
        entries.push(PackageEntry {
            path,
            kind,
            sha256: digest.clone(),
            bytes: bytes.len(),
            depends_on: (kind != EntryKind::Source)
                .then(|| "source/manifest".into())
                .into_iter()
                .collect(),
        });
        blobs.insert(digest, bytes);
    }
    let sql_tables = definitions
        .iter()
        .map(|table| SqlTable {
            name: table.name.clone(),
            columns: table
                .columns
                .iter()
                .map(|column| SqlColumn {
                    name: column.name.clone(),
                    nullable: column.nullable,
                })
                .collect(),
            primary_key: table.primary_key.clone(),
        })
        .collect();
    let mut package = ApplicationPackageV2 {
        base: ApplicationPackage {
            manifest: ApplicationManifest {
                name: "AWS-CARDDEMO".into(),
                version: "0.2.0".into(),
                target_product: "0.2.0".into(),
                entries,
            },
            blobs,
        },
        generation: 1,
        sections: ApplicationSections {
            schema_version: APPLICATION_PACKAGE_V2_CONTRACT.into(),
            host_abi_libraries: Vec::new(),
            sql_tables,
            sql_rows: Vec::new(),
            ims_definitions: Vec::new(),
            ims_rows: Vec::new(),
            ims_metadata: None,
            ims_tm: None,
            mq_resources: Vec::new(),
            batch_controllers,
            security_resources: Vec::new(),
        },
        signature: PackageSignature {
            algorithm: "hmac-sha256@1".into(),
            key_id: "carddemo-conformance-key".into(),
            value: "pending".into(),
        },
    };
    let identity = package_v2_identity(&package).map_err(package_problem)?;
    package.signature.value = sign_carddemo_package_identity(&identity);
    let installed = server
        .install_application_package_v2(&package)
        .map_err(terminal_problem)?;
    server
        .publish_application_generation(&installed)
        .map(|_| ())
        .map_err(terminal_problem)
}

async fn exercise_db2_routes(
    corpus_dir: &Path,
    online: OnlineApplicationDefinition,
    definitions: Vec<BatchProgramDefinition>,
) -> Result<Db2Exercise, CorpusProblem> {
    let artifact_root =
        env::temp_dir().join(format!("mainframe-env-carddemo-db2-{}", std::process::id()));
    let config = ServerConfig {
        store_profile: StoreProfile::Memory,
        artifact_root: artifact_root.clone(),
        tls: TlsConfig {
            enabled: false,
            certificate_path: None,
            private_key_reference: None,
        },
        ..ServerConfig::default()
    };
    let store = Arc::new(MemoryStore::new(Default::default()));
    let secrets = Arc::new(MemorySecretResolver::default());
    let server = ProductServer::open_with_package_trust(
        config.clone(),
        store.clone(),
        secrets.clone(),
        default_program_router(),
        carddemo_package_trust()?,
    )
    .map_err(terminal_problem)?;
    server
        .install_batch_programs(definitions.clone())
        .map_err(terminal_problem)?;
    install_carddemo_db2_package(&server, &definitions)?;
    server
        .bootstrap_user("IBMUSER", b"TESTPASS")
        .map_err(terminal_problem)?;
    install_base_online_authorities(&server, corpus_dir, &online)?;
    online_authorities::install_db2_authorities(&server)?;
    server
        .install_online_application(online)
        .map_err(terminal_problem)?;
    let racf = server.racf_service();
    racf.define_profile("DATASET", "AWS.M2.CARDDEMO.**", "IBMUSER", None)
        .map_err(terminal_problem)?;
    racf.permit(
        "DATASET",
        "AWS.M2.CARDDEMO.**",
        "IBMUSER",
        AccessIntent::Alter,
    )
    .map_err(terminal_problem)?;
    racf.define_profile("DATASET", "INPFILE", "IBMUSER", None)
        .map_err(terminal_problem)?;
    racf.permit("DATASET", "INPFILE", "IBMUSER", AccessIntent::Alter)
        .map_err(terminal_problem)?;

    let mut sequence = 40_000u64;
    let mut control_members = Vec::new();
    for relative in collect_paths(corpus_dir, &["app/app-transaction-type-db2/ctl"], "ctl")? {
        let member = Path::new(&relative)
            .file_stem()
            .and_then(|value| value.to_str())
            .ok_or_else(|| {
                CorpusProblem::new("carddemo.db2.control_invalid", "control member is invalid")
            })?;
        let source = String::from_utf8(read_corpus_file(corpus_dir, &corpus_dir.join(&relative))?)
            .map_err(|_| {
                CorpusProblem::new(
                    "carddemo.db2.control_invalid",
                    format!("{relative} is not UTF-8"),
                )
            })?;
        control_members.push((
            member.to_string(),
            source
                .lines()
                .map(|line| line.as_bytes().to_vec())
                .collect(),
        ));
    }
    control_library::seed_control_library(
        &server,
        "AWS.M2.CARDDEMO.CNTL",
        control_members,
        &mut sequence,
    )?;
    utility_seed_dataset(
        &server,
        "INPFILE",
        DatasetOrganization::Sequential,
        RecordFormat::Fixed,
        53,
        None,
        ["A98BATCH INSERT", "U02BATCH PAYMENT", "D98"]
            .into_iter()
            .map(|record| {
                let mut record = record.as_bytes().to_vec();
                record.resize(53, b' ');
                record
            })
            .collect(),
        &mut sequence,
    )?;
    for dataset in ["AWS.M2.CARDDEMO.TRANTYPE.PS", "AWS.M2.CARDDEMO.TRANCATG.PS"] {
        utility_seed_dataset(
            &server,
            dataset,
            DatasetOrganization::Sequential,
            RecordFormat::Fixed,
            60,
            None,
            vec![vec![b' '; 60]],
            &mut sequence,
        )?;
    }
    for base in [
        "AWS.M2.CARDDEMO.TRANTYPE.BKUP",
        "AWS.M2.CARDDEMO.TRANCATG.PS.BKUP",
    ] {
        server
            .dataset_service()
            .invoke(DatasetRequest::DefineGenerationGroup {
                base: DatasetName::new(base, 128).expect("static Db2 GDG base"),
                limit: 5,
                scratch: true,
                empty: false,
                mutation: Mutation {
                    sequence,
                    idempotency_key: IdempotencyKey::new(
                        format!("carddemo-db2-gdg-{sequence}"),
                        InvocationLimits::default(),
                    )
                    .expect("bounded Db2 GDG mutation"),
                    transaction: Some("CD-024".into()),
                },
            })
            .map_err(terminal_problem)?;
        sequence += 1;
    }

    let app = server.router();
    let mut job_ids = BTreeMap::new();
    let create = String::from_utf8(read_corpus_file(
        corpus_dir,
        &corpus_dir.join("app/app-transaction-type-db2/jcl/CREADB21.jcl"),
    )?)
    .map_err(|_| CorpusProblem::new("carddemo.db2.jcl_invalid", "CREADB21 is not UTF-8"))?;
    let create_id = submit_job_with_retcode(&server, &app, &create, "CC 0000").await?;
    job_ids.insert("CREADB2".into(), create_id);
    if server
        .db2_service()
        .table_rows("CARDDEMO.TRANSACTION_TYPE")
        .map_err(terminal_problem)?
        .len()
        != 7
        || server
            .db2_service()
            .table_rows("CARDDEMO.TRANSACTION_TYPE_CATEGORY")
            .map_err(terminal_problem)?
            .len()
            != 18
    {
        return Err(CorpusProblem::new(
            "carddemo.db2.load_drift",
            "CREADB21 did not create and load 7 type plus 18 category rows",
        ));
    }

    let denied = terminal_http(
        &app,
        Method::POST,
        "/mainframe-env/cics/v1/sessions",
        BTreeMap::from([
            (
                "authorization".into(),
                format!(
                    "Basic {}",
                    base64::engine::general_purpose::STANDARD.encode("WEBUSER:transport-password")
                ),
            ),
            ("x-csrf-zosmf-header".into(), "true".into()),
        ]),
        br#"{"transaction":"CTLI"}"#.to_vec(),
    )
    .await?;
    if denied.0 != StatusCode::FORBIDDEN {
        return Err(CorpusProblem::new(
            "carddemo.db2.authorization_drift",
            format!(
                "regular-user CTLI launch returned {}: {}",
                denied.0,
                String::from_utf8_lossy(&denied.1)
            ),
        ));
    }

    let list_menu = open_carddemo_menu(
        &server,
        &app,
        "WEBADM",
        "admin-transport-password",
        "ADMIN001",
        "PASSWORD",
        "COADM01",
    )
    .await?;
    let list_screen = carddemo_terminal_exchange(
        &app,
        &list_menu.session,
        &list_menu.headers,
        0x7d,
        BTreeMap::from([("OPTION".into(), "5".into())]),
    )
    .await
    .map_err(|problem| {
        CorpusProblem::new(
            "carddemo.db2.list_failed",
            format!("CTLI: {}", problem.detail),
        )
    })?;
    require_online_mapset(&list_screen, "COTRTLI", "Db2 transaction-type list")?;
    let update_menu = open_carddemo_menu(
        &server,
        &app,
        "WEBADM",
        "admin-transport-password",
        "ADMIN001",
        "PASSWORD",
        "COADM01",
    )
    .await?;
    let update_screen = carddemo_terminal_exchange(
        &app,
        &update_menu.session,
        &update_menu.headers,
        0x7d,
        BTreeMap::from([("OPTION".into(), "6".into())]),
    )
    .await
    .map_err(|problem| {
        CorpusProblem::new(
            "carddemo.db2.maintenance_failed",
            format!("CTTU launch: {}", problem.detail),
        )
    })?;
    require_online_mapset(
        &update_screen,
        "COTRTUP",
        "Db2 transaction-type maintenance",
    )?;
    let missing = carddemo_terminal_exchange(
        &app,
        &update_menu.session,
        &update_menu.headers,
        0x7d,
        BTreeMap::from([("TRTYPCD".into(), "99".into())]),
    )
    .await
    .map_err(|problem| {
        CorpusProblem::new(
            "carddemo.db2.maintenance_failed",
            format!("CTTU lookup: {}", problem.detail),
        )
    })?;
    require_online_mapset(&missing, "COTRTUP", "Db2 missing type lookup")?;
    let create = carddemo_terminal_exchange(
        &app,
        &update_menu.session,
        &update_menu.headers,
        0xf5,
        BTreeMap::new(),
    )
    .await
    .map_err(|problem| {
        CorpusProblem::new(
            "carddemo.db2.maintenance_failed",
            format!("CTTU create request: {}", problem.detail),
        )
    })?;
    require_online_mapset(&create, "COTRTUP", "Db2 create confirmation")?;
    let described = carddemo_terminal_exchange(
        &app,
        &update_menu.session,
        &update_menu.headers,
        0x7d,
        BTreeMap::from([("TRTYDSC".into(), "ONLINE SPECIAL".into())]),
    )
    .await
    .map_err(|problem| {
        CorpusProblem::new(
            "carddemo.db2.maintenance_failed",
            format!("CTTU description: {}", problem.detail),
        )
    })?;
    require_online_mapset(&described, "COTRTUP", "Db2 insert validation")?;
    let inserted = carddemo_terminal_exchange(
        &app,
        &update_menu.session,
        &update_menu.headers,
        0xf5,
        BTreeMap::new(),
    )
    .await
    .map_err(|problem| {
        CorpusProblem::new(
            "carddemo.db2.maintenance_failed",
            format!("CTTU insert: {}", problem.detail),
        )
    })?;
    require_online_mapset(&inserted, "COTRTUP", "Db2 online insert")?;
    if !server
        .db2_service()
        .table_rows("CARDDEMO.TRANSACTION_TYPE")
        .map_err(terminal_problem)?
        .iter()
        .any(|row| row.first().is_some_and(|value| value == b"99"))
    {
        return Err(CorpusProblem::new(
            "carddemo.db2.online_insert_drift",
            format!(
                "CTTU did not commit type 99; trace={:?}",
                server
                    .online_trace(&update_menu.session)
                    .unwrap_or_default()
            ),
        ));
    }

    let edit_menu = open_carddemo_menu(
        &server,
        &app,
        "WEBADM",
        "admin-transport-password",
        "ADMIN001",
        "PASSWORD",
        "COADM01",
    )
    .await?;
    let edit_screen = carddemo_terminal_exchange(
        &app,
        &edit_menu.session,
        &edit_menu.headers,
        0x7d,
        BTreeMap::from([("OPTION".into(), "6".into())]),
    )
    .await
    .map_err(|problem| {
        CorpusProblem::new(
            "carddemo.db2.maintenance_failed",
            format!("edit launch: {}", problem.detail),
        )
    })?;
    require_online_mapset(&edit_screen, "COTRTUP", "Db2 update route")?;
    let selected = carddemo_terminal_exchange(
        &app,
        &edit_menu.session,
        &edit_menu.headers,
        0x7d,
        BTreeMap::from([("TRTYPCD".into(), "99".into())]),
    )
    .await
    .map_err(|problem| {
        CorpusProblem::new(
            "carddemo.db2.maintenance_failed",
            format!("edit lookup: {}", problem.detail),
        )
    })?;
    require_online_mapset(&selected, "COTRTUP", "Db2 update lookup")?;
    let reviewed = carddemo_terminal_exchange(
        &app,
        &edit_menu.session,
        &edit_menu.headers,
        0x7d,
        BTreeMap::from([("TRTYDSC".into(), "ONLINE UPDATED".into())]),
    )
    .await
    .map_err(|problem| {
        CorpusProblem::new(
            "carddemo.db2.maintenance_failed",
            format!("edit validation: {}", problem.detail),
        )
    })?;
    require_online_mapset(&reviewed, "COTRTUP", "Db2 update validation")?;
    let updated = carddemo_terminal_exchange(
        &app,
        &edit_menu.session,
        &edit_menu.headers,
        0xf5,
        BTreeMap::new(),
    )
    .await
    .map_err(|problem| {
        CorpusProblem::new(
            "carddemo.db2.maintenance_failed",
            format!("edit commit: {}", problem.detail),
        )
    })?;
    require_online_mapset(&updated, "COTRTUP", "Db2 online update")?;
    if !server
        .db2_service()
        .table_rows("CARDDEMO.TRANSACTION_TYPE")
        .map_err(terminal_problem)?
        .iter()
        .any(|row| {
            row.first().is_some_and(|value| value == b"99")
                && row
                    .get(1)
                    .is_some_and(|value| value.as_slice() == b"ONLINE UPDATED")
        })
    {
        return Err(CorpusProblem::new(
            "carddemo.db2.online_update_drift",
            "CTTU did not commit the updated type 99 description",
        ));
    }

    let delete_menu = open_carddemo_menu(
        &server,
        &app,
        "WEBADM",
        "admin-transport-password",
        "ADMIN001",
        "PASSWORD",
        "COADM01",
    )
    .await?;
    let delete_screen = carddemo_terminal_exchange(
        &app,
        &delete_menu.session,
        &delete_menu.headers,
        0x7d,
        BTreeMap::from([("OPTION".into(), "6".into())]),
    )
    .await
    .map_err(|problem| {
        CorpusProblem::new(
            "carddemo.db2.maintenance_failed",
            format!("delete launch: {}", problem.detail),
        )
    })?;
    require_online_mapset(&delete_screen, "COTRTUP", "Db2 delete route")?;
    let selected = carddemo_terminal_exchange(
        &app,
        &delete_menu.session,
        &delete_menu.headers,
        0x7d,
        BTreeMap::from([("TRTYPCD".into(), "99".into())]),
    )
    .await
    .map_err(|problem| {
        CorpusProblem::new(
            "carddemo.db2.maintenance_failed",
            format!("delete lookup: {}", problem.detail),
        )
    })?;
    require_online_mapset(&selected, "COTRTUP", "Db2 delete lookup")?;
    let confirmed = carddemo_terminal_exchange(
        &app,
        &delete_menu.session,
        &delete_menu.headers,
        0xf4,
        BTreeMap::new(),
    )
    .await
    .map_err(|problem| {
        CorpusProblem::new(
            "carddemo.db2.maintenance_failed",
            format!("delete confirmation: {}", problem.detail),
        )
    })?;
    require_online_mapset(&confirmed, "COTRTUP", "Db2 delete confirmation")?;
    let deleted = carddemo_terminal_exchange(
        &app,
        &delete_menu.session,
        &delete_menu.headers,
        0xf4,
        BTreeMap::new(),
    )
    .await
    .map_err(|problem| {
        CorpusProblem::new(
            "carddemo.db2.maintenance_failed",
            format!("delete commit: {}", problem.detail),
        )
    })?;
    require_online_mapset(&deleted, "COTRTUP", "Db2 online delete")?;
    if server
        .db2_service()
        .table_rows("CARDDEMO.TRANSACTION_TYPE")
        .map_err(terminal_problem)?
        .iter()
        .any(|row| row.first().is_some_and(|value| value == b"99"))
    {
        return Err(CorpusProblem::new(
            "carddemo.db2.online_delete_drift",
            "CTTU did not delete type 99",
        ));
    }

    let rollback = db2_control_invocation("db2-rollback")?;
    server
        .db2_service()
        .execute(
            &rollback,
            &db2_control_request(
                Db2Operation::Insert,
                50_001,
                BTreeMap::from([
                    ("DCL-TR-TYPE".into(), db2_variable("97")),
                    (
                        "DCL-TR-DESCRIPTION".into(),
                        db2_varchar_variable("ROLLBACK"),
                    ),
                ]),
            )?,
        )
        .map_err(terminal_problem)?;
    server
        .db2_service()
        .execute(
            &rollback,
            &db2_control_request(Db2Operation::Rollback, 50_002, BTreeMap::new())?,
        )
        .map_err(terminal_problem)?;
    if server
        .db2_service()
        .table_rows("CARDDEMO.TRANSACTION_TYPE")
        .map_err(terminal_problem)?
        .iter()
        .any(|row| row.first().is_some_and(|value| value == b"97"))
    {
        return Err(CorpusProblem::new(
            "carddemo.db2.rollback_drift",
            "rolled-back type 97 became visible",
        ));
    }
    let left = db2_control_invocation("db2-conflict-left")?;
    let right = db2_control_invocation("db2-conflict-right")?;
    for (invocation, sequence, description) in [(&left, 50_003, "LEFT"), (&right, 50_004, "RIGHT")]
    {
        server
            .db2_service()
            .execute(
                invocation,
                &db2_control_request(
                    Db2Operation::Update,
                    sequence,
                    BTreeMap::from([
                        ("DCL-TR-TYPE".into(), db2_variable("02")),
                        (
                            "DCL-TR-DESCRIPTION".into(),
                            db2_varchar_variable(description),
                        ),
                    ]),
                )?,
            )
            .map_err(terminal_problem)?;
    }
    let left_commit = server
        .db2_service()
        .execute(
            &left,
            &db2_control_request(Db2Operation::Commit, 50_005, BTreeMap::new())?,
        )
        .map_err(terminal_problem)?;
    let right_commit = server
        .db2_service()
        .execute(
            &right,
            &db2_control_request(Db2Operation::Commit, 50_006, BTreeMap::new())?,
        )
        .map_err(terminal_problem)?;
    if left_commit.sqlcode != 0 || right_commit.sqlcode != -911 {
        return Err(CorpusProblem::new(
            "carddemo.db2.conflict_drift",
            "concurrent Db2 units did not return commit then -911 conflict",
        ));
    }
    let failure_invocation = db2_control_invocation("db2-failure")?;
    let duplicate = server
        .db2_service()
        .execute(
            &failure_invocation,
            &db2_control_request(
                Db2Operation::Insert,
                50_007,
                BTreeMap::from([
                    ("DCL-TR-TYPE".into(), db2_variable("01")),
                    (
                        "DCL-TR-DESCRIPTION".into(),
                        db2_varchar_variable("DUPLICATE"),
                    ),
                ]),
            )?,
        )
        .map_err(terminal_problem)?;
    server
        .db2_service()
        .execute(
            &failure_invocation,
            &db2_control_request(Db2Operation::Rollback, 50_008, BTreeMap::new())?,
        )
        .map_err(terminal_problem)?;
    let restricted = server
        .db2_service()
        .execute(
            &failure_invocation,
            &db2_control_request(
                Db2Operation::Delete,
                50_009,
                BTreeMap::from([("DCL-TR-TYPE".into(), db2_variable("01"))]),
            )?,
        )
        .map_err(terminal_problem)?;
    if duplicate.sqlcode != -803 || restricted.sqlcode != -532 {
        return Err(CorpusProblem::new(
            "carddemo.db2.failure_drift",
            "duplicate and referential failures did not return -803 and -532",
        ));
    }

    let maintenance = String::from_utf8(read_corpus_file(
        corpus_dir,
        &corpus_dir.join("app/app-transaction-type-db2/jcl/MNTTRDB2.jcl"),
    )?)
    .map_err(|_| CorpusProblem::new("carddemo.db2.jcl_invalid", "MNTTRDB2 is not UTF-8"))?;
    let maintenance_id = submit_job_with_retcode(&server, &app, &maintenance, "CC 0000").await?;
    job_ids.insert("MNTTRDB2".into(), maintenance_id);
    let maintained_rows = server
        .db2_service()
        .table_rows("CARDDEMO.TRANSACTION_TYPE")
        .map_err(terminal_problem)?;
    if maintained_rows
        .iter()
        .any(|row| row.first().is_some_and(|value| value == b"98"))
        || !maintained_rows.iter().any(|row| {
            row.first().is_some_and(|value| value == b"02")
                && row
                    .get(1)
                    .is_some_and(|value| value.as_slice() == b"BATCH PAYMENT")
        })
    {
        return Err(CorpusProblem::new(
            "carddemo.db2.batch_maintenance_drift",
            format!(
                "COBTUPDT did not apply insert, update, and delete commands: {:?}",
                maintained_rows
                    .iter()
                    .map(|row| row
                        .iter()
                        .map(|column| String::from_utf8_lossy(column).to_string())
                        .collect::<Vec<_>>())
                    .collect::<Vec<_>>()
            ),
        ));
    }
    let extract = String::from_utf8(read_corpus_file(
        corpus_dir,
        &corpus_dir.join("app/app-transaction-type-db2/jcl/TRANEXTR.jcl"),
    )?)
    .map_err(|_| CorpusProblem::new("carddemo.db2.jcl_invalid", "TRANEXTR is not UTF-8"))?;
    let extract_id = submit_job_with_retcode(&server, &app, &extract, "CC 0000").await?;
    job_ids.insert("TRANEXTR".into(), extract_id);

    let extracted_type = utility_records(&server, "AWS.M2.CARDDEMO.TRANTYPE.PS", None)?;
    let extracted_category = utility_records(&server, "AWS.M2.CARDDEMO.TRANCATG.PS", None)?;
    let extraction_records = extracted_type.len() + extracted_category.len();
    if extraction_records < 25
        || extracted_type.iter().any(|record| record.len() != 60)
        || extracted_category.iter().any(|record| record.len() != 60)
    {
        return Err(CorpusProblem::new(
            "carddemo.db2.extract_drift",
            "DSNTIAUL extraction did not produce exact 60-byte type/category records",
        ));
    }

    let table_sha256 = db2_table_digests(&server)?;
    let dataset_sha256 = db2_dataset_digests(&server)?;
    let spool_sha256 = base_batch_spool_digests(&server, &job_ids)?;
    let restart_session = list_menu.session;
    let restart_headers = list_menu.headers;
    drop(app);
    if !server.graceful_shutdown().await {
        return Err(CorpusProblem::new(
            "carddemo.db2.shutdown_failed",
            "Db2 server did not shut down",
        ));
    }
    drop(racf);
    drop(server);
    let restarted = ProductServer::open_with_package_trust(
        config,
        store,
        secrets,
        default_program_router(),
        carddemo_package_trust()?,
    )
    .map_err(|problem| {
        CorpusProblem::new(
            "carddemo.db2.restart_open",
            format!("package-verified reopen failed: {problem}"),
        )
    })?;
    if db2_table_digests(&restarted)? != table_sha256
        || db2_dataset_digests(&restarted)? != dataset_sha256
        || base_batch_spool_digests(&restarted, &job_ids)? != spool_sha256
    {
        return Err(CorpusProblem::new(
            "carddemo.db2.restart_drift",
            "Db2 tables, extracted datasets, or spool changed across restart",
        ));
    }
    let restarted_app = restarted.router();
    let resumed = carddemo_terminal_exchange(
        &restarted_app,
        &restart_session,
        &restart_headers,
        0xf8,
        BTreeMap::new(),
    )
    .await
    .map_err(|problem| {
        CorpusProblem::new(
            "carddemo.db2.restart_failed",
            format!("cursor resume: {}", problem.detail),
        )
    })?;
    require_online_mapset(&resumed, "COTRTLI", "Db2 cursor restart")?;
    let _ = restarted.graceful_shutdown().await;
    drop(restarted);
    let _ = fs::remove_dir_all(&artifact_root);
    Ok(Db2Exercise {
        online_routes: 2,
        batch_routes: 3,
        extraction_records,
        authorization_controls: 1,
        restart_controls: 1,
        rollback_controls: 1,
        conflict_controls: 1,
        failure_controls: 2,
        table_sha256,
        dataset_sha256,
        spool_sha256,
    })
}

pub fn verify_carddemo_ims_from_env(
    inventory_path: &Path,
) -> Result<CardDemoImsReceipt, CorpusProblem> {
    let corpus_dir = PathBuf::from(env::var_os(CORPUS_ENV).ok_or_else(|| {
        CorpusProblem::new(
            "carddemo.corpus.environment_missing",
            "CARDDEMO_CORPUS_DIR is required",
        )
    })?);
    let corpus = verify_carddemo_corpus(&corpus_dir, inventory_path)?;
    let (definition, definitions_checked) = carddemo_ims_definition(&corpus_dir)?;
    let compiler = CobolCompiler::default();
    let mut programs_compiled = 0usize;
    let mut dli_operations = BTreeMap::new();
    for (relative, bundle) in explicit_carddemo_bundles(&corpus_dir)?
        .into_iter()
        .filter(|(relative, _)| relative.starts_with("app/app-authorization-ims-db2-mq/cbl/"))
    {
        let analysis = compiler.analyze(&bundle);
        if analysis.completeness != Completeness::Complete {
            return Err(CorpusProblem::new(
                "carddemo.ims.compile_failed",
                format!(
                    "{relative}: {}",
                    analysis
                        .diagnostics
                        .first()
                        .map_or("incomplete IMS compilation", |problem| problem
                            .public_message())
                ),
            ));
        }
        let operations = analysis
            .hir
            .ok_or_else(|| CorpusProblem::new("carddemo.ims.compile_failed", "IMS HIR missing"))?
            .statements
            .into_iter()
            .filter(|statement| statement.kind == StatementKind::ExecDli)
            .filter_map(|statement| {
                statement
                    .arguments
                    .into_iter()
                    .find(|token| !matches!(token.as_str(), "DLI" | "END-EXEC"))
            })
            .collect::<Vec<_>>();
        if operations.is_empty() {
            continue;
        }
        for operation in operations {
            *dli_operations.entry(operation).or_default() += 1;
        }
        let result = compiler
            .compile(CompilerRequest {
                source: bundle,
                mode: CompilationMode::Executable,
                target: CompileTarget::new("reference").expect("static target"),
                options: CompileOptions::new(BTreeMap::new()).expect("static options"),
            })
            .map_err(|problem| {
                CorpusProblem::new(
                    "carddemo.ims.compile_failed",
                    format!("{relative}: {problem:?}"),
                )
            })?;
        if !matches!(result, CompilerResult::Published { .. }) {
            return Err(CorpusProblem::new(
                "carddemo.ims.compile_failed",
                format!("{relative} did not publish"),
            ));
        }
        programs_compiled += 1;
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| CorpusProblem::new("carddemo.ims.runtime", error.to_string()))?;
    let exercise = runtime.block_on(exercise_ims_routes(&corpus_dir, definition.clone()))?;
    let mut shape = Sha256::new();
    digest_field(&mut shape, corpus.commit.as_bytes());
    digest_field(
        &mut shape,
        &serde_json::to_vec(&definition)
            .map_err(|error| CorpusProblem::new("carddemo.ims.definition", error.to_string()))?,
    );
    for (operation, count) in &dli_operations {
        digest_field(&mut shape, operation.as_bytes());
        digest_field(&mut shape, &(*count as u64).to_be_bytes());
    }
    digest_field(&mut shape, exercise.hierarchy_sha256.as_bytes());
    for (name, digest) in &exercise.spool_sha256 {
        digest_field(&mut shape, name.as_bytes());
        digest_field(&mut shape, digest.as_bytes());
    }
    Ok(CardDemoImsReceipt {
        schema_version: "mainframe-env.carddemo-ims-receipt@1".into(),
        status: "pass".into(),
        corpus_commit: corpus.commit,
        definitions_checked,
        databases_installed: exercise.databases_installed,
        psbs_installed: exercise.psbs_installed,
        pcbs_installed: exercise.pcbs_installed,
        programs_compiled,
        dli_operations,
        application_routes: exercise.selected_job_routes + 1,
        selected_job_routes: exercise.selected_job_routes,
        roots: exercise.roots,
        children: exercise.children,
        secondary_index_entries: exercise.secondary_index_entries,
        load_unload_routes: 2,
        checkpoint_controls: 1,
        restart_controls: 1,
        authorization_controls: 1,
        malformed_controls: 1,
        resource_controls: 1,
        provider_failure_controls: 1,
        hierarchy_sha256: exercise.hierarchy_sha256,
        spool_sha256: exercise.spool_sha256,
        ims_shape_sha256: format!("{:x}", shape.finalize()),
    })
}

pub fn verify_carddemo_mq_authorization_from_env(
    inventory_path: &Path,
) -> Result<CardDemoMqAuthorizationReceipt, CorpusProblem> {
    let corpus_dir = PathBuf::from(env::var_os(CORPUS_ENV).ok_or_else(|| {
        CorpusProblem::new(
            "carddemo.corpus.environment_missing",
            "CARDDEMO_CORPUS_DIR is required",
        )
    })?);
    let corpus = verify_carddemo_corpus(&corpus_dir, inventory_path)?;
    let compiler = CobolCompiler::default();
    let mut programs_compiled = 0usize;
    let mut mq_calls = BTreeMap::new();
    for (relative, bundle) in
        explicit_carddemo_bundles(&corpus_dir)?
            .into_iter()
            .filter(|(relative, _)| {
                relative.starts_with("app/app-vsam-mq/cbl/")
                    || relative.starts_with("app/app-authorization-ims-db2-mq/cbl/")
            })
    {
        let analysis = compiler.analyze(&bundle);
        if analysis.completeness != Completeness::Complete {
            return Err(CorpusProblem::new(
                "carddemo.mq.compile_failed",
                format!(
                    "{relative}: {}",
                    analysis
                        .diagnostics
                        .first()
                        .map_or("incomplete MQ compilation", |problem| problem
                            .public_message())
                ),
            ));
        }
        let calls = analysis
            .hir
            .ok_or_else(|| CorpusProblem::new("carddemo.mq.compile_failed", "MQ HIR missing"))?
            .statements
            .into_iter()
            .filter(|statement| statement.kind == StatementKind::Call)
            .filter_map(|statement| statement.arguments.first().cloned())
            .map(|target| target.trim_matches(['\'', '"']).to_ascii_uppercase())
            .filter(|target| {
                matches!(
                    target.as_str(),
                    "MQOPEN" | "MQGET" | "MQPUT" | "MQPUT1" | "MQCLOSE"
                )
            })
            .collect::<Vec<_>>();
        if calls.is_empty() {
            continue;
        }
        for call in calls {
            *mq_calls.entry(call).or_default() += 1;
        }
        if !matches!(
            compiler
                .compile(CompilerRequest {
                    source: bundle,
                    mode: CompilationMode::Executable,
                    target: CompileTarget::new("reference").expect("static target"),
                    options: CompileOptions::new(BTreeMap::new()).expect("static options"),
                })
                .map_err(|problem| CorpusProblem::new(
                    "carddemo.mq.compile_failed",
                    format!("{relative}: {problem:?}")
                ))?,
            CompilerResult::Published { .. }
        ) {
            return Err(CorpusProblem::new(
                "carddemo.mq.compile_failed",
                format!("{relative} did not publish"),
            ));
        }
        programs_compiled += 1;
    }
    for required in ["MQOPEN", "MQGET", "MQPUT", "MQPUT1", "MQCLOSE"] {
        if !mq_calls.contains_key(required) {
            return Err(CorpusProblem::new(
                "carddemo.mq.call_drift",
                format!("pinned sources no longer contain {required}"),
            ));
        }
    }
    let (definition, _) = carddemo_ims_definition(&corpus_dir)?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| CorpusProblem::new("carddemo.mq.runtime", error.to_string()))?;
    let exercise = runtime.block_on(exercise_mq_authorization_routes(&corpus_dir, definition))?;
    let mut shape = Sha256::new();
    digest_field(&mut shape, corpus.commit.as_bytes());
    for (operation, count) in &mq_calls {
        digest_field(&mut shape, operation.as_bytes());
        digest_field(&mut shape, &(*count as u64).to_be_bytes());
    }
    for (queue, digest) in &exercise.queue_sha256 {
        digest_field(&mut shape, queue.as_bytes());
        digest_field(&mut shape, digest.as_bytes());
    }
    digest_field(&mut shape, &(exercise.ims_roots as u64).to_be_bytes());
    digest_field(&mut shape, &(exercise.ims_children as u64).to_be_bytes());
    digest_field(&mut shape, &(exercise.fraud_rows as u64).to_be_bytes());
    Ok(CardDemoMqAuthorizationReceipt {
        schema_version: "mainframe-env.carddemo-mq-authorization-receipt@1".into(),
        status: "pass".into(),
        corpus_commit: corpus.commit,
        programs_compiled,
        mq_calls,
        queues_installed: exercise.queues_installed,
        triggers_installed: exercise.triggers_installed,
        journeys_passed: 4,
        request_reply_routes: 2,
        approval_decline_routes: 2,
        summary_detail_fraud_routes: 3,
        purge_routes: 1,
        correlation_controls: 2,
        timeout_controls: 1,
        syncpoint_controls: 3,
        rollback_controls: 3,
        unknown_outcome_controls: 1,
        restart_controls: 1,
        idempotency_controls: 2,
        authorization_controls: 1,
        ims_roots: exercise.ims_roots,
        ims_children: exercise.ims_children,
        fraud_rows: exercise.fraud_rows,
        queue_sha256: exercise.queue_sha256,
        authorization_shape_sha256: format!("{:x}", shape.finalize()),
    })
}

fn verify_cdv1_correction(
    inventory_path: &Path,
    corpus_commit: &str,
) -> Result<Cdv1CorrectionReceipt, CorpusProblem> {
    let profile_root = inventory_path
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| {
            CorpusProblem::new(
                "carddemo.full.cdv1_contract_invalid",
                "inventory path does not identify the 0.1.1 profile root",
            )
        })?;
    let correction_bytes = fs::read(profile_root.join("contract/carddemo-corrections.json"))
        .map_err(|error| {
            CorpusProblem::new(
                "carddemo.full.cdv1_decision_required",
                format!(
                    "cannot read the CardDemo correction contract: {:?}",
                    error.kind()
                ),
            )
        })?;
    let contract: CardDemoCorrectionsContract =
        serde_json::from_slice(&correction_bytes).map_err(|error| {
            CorpusProblem::new(
                "carddemo.full.cdv1_contract_invalid",
                format!("cannot parse the CardDemo correction contract: {error}"),
            )
        })?;
    if contract.schema_version != "mainframe-env.carddemo-corrections@1"
        || contract.corpus_commit != corpus_commit
        || contract.corrections.len() != 1
    {
        return Err(CorpusProblem::new(
            "carddemo.full.cdv1_contract_invalid",
            "correction schema, corpus identity, or bounded correction count differs",
        ));
    }
    let correction = &contract.corrections[0];
    if correction.id != "CARDDEMO-ORPHAN-CDV1"
        || correction.disposition != "accepted-owned-source"
        || correction.transaction != "CDV1"
        || correction.program != "COCRDSEC"
        || correction.source != "fixtures/carddemo/COCRDSEC.cbl"
        || correction.behavior
            != "The developer transaction displays an explicit unavailable-contract message and returns without reading or mutating card data."
        || correction.basis
            != "Repository-owner approval for bounded demo source; the pinned corpus and runtime oracles contain no original COCRDSEC implementation."
        || correction.source_sha256.len() != 64
        || !correction
            .source_sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(CorpusProblem::new(
            "carddemo.full.cdv1_contract_invalid",
            "the accepted CDV1/COCRDSEC correction differs from its bounded contract",
        ));
    }
    validate_relative_path(&correction.source, "correction source")?;
    let source_path = profile_root.join(&correction.source);
    let source_bytes = fs::read(&source_path).map_err(|error| {
        CorpusProblem::new(
            "carddemo.full.cdv1_source_missing",
            format!("cannot read accepted COCRDSEC source: {:?}", error.kind()),
        )
    })?;
    let source_sha256 = format!("{:x}", Sha256::digest(&source_bytes));
    if source_sha256 != correction.source_sha256 {
        return Err(CorpusProblem::new(
            "carddemo.full.cdv1_source_drift",
            "accepted COCRDSEC source differs from its correction contract",
        ));
    }
    let limits = SourceLimits::default();
    let logical = LogicalPath::new(&correction.source, limits.max_path_bytes).map_err(|error| {
        CorpusProblem::new(
            "carddemo.full.cdv1_source_invalid",
            format!("accepted COCRDSEC logical path is invalid: {error}"),
        )
    })?;
    let source = SourceFile::input(
        &correction.source,
        source_bytes,
        SourceFormat::Free,
        SourceEncoding::Utf8,
        limits,
    )
    .map_err(|error| {
        CorpusProblem::new(
            "carddemo.full.cdv1_source_invalid",
            format!("accepted COCRDSEC source is invalid: {error}"),
        )
    })?;
    let bundle = SourceBundle::new(&logical, vec![source], BTreeMap::new(), Vec::new(), limits)
        .map_err(|error| {
            CorpusProblem::new(
                "carddemo.full.cdv1_source_invalid",
                format!("accepted COCRDSEC source bundle is invalid: {error}"),
            )
        })?;
    let compiler = CobolCompiler::default();
    let analysis = compiler.analyze(&bundle);
    if analysis.completeness != Completeness::Complete {
        return Err(CorpusProblem::new(
            "carddemo.full.cdv1_compile_failed",
            analysis
                .diagnostics
                .first()
                .map_or("accepted COCRDSEC analysis is incomplete", |problem| {
                    problem.public_message()
                }),
        ));
    }
    let artifact = match compiler
        .compile(CompilerRequest {
            source: bundle,
            mode: CompilationMode::Executable,
            target: CompileTarget::new("reference").expect("static target"),
            options: CompileOptions::new(BTreeMap::new()).expect("static options"),
        })
        .map_err(|error| {
            CorpusProblem::new("carddemo.full.cdv1_compile_failed", error.to_string())
        })? {
        CompilerResult::Published { artifact, .. } => artifact,
        CompilerResult::Analysis { diagnostics, .. }
        | CompilerResult::Failed { diagnostics, .. } => {
            return Err(CorpusProblem::new(
                "carddemo.full.cdv1_compile_failed",
                diagnostics
                    .first()
                    .map_or("accepted COCRDSEC did not publish", |problem| {
                        problem.public_message()
                    }),
            ));
        }
    };
    let payload = artifact.payload().to_vec();
    let artifact_sha256 = format!("{:x}", Sha256::digest(&payload));
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| CorpusProblem::new("carddemo.full.cdv1_runtime", error.to_string()))?;
    let (screen_sha256, public_routes) = runtime.block_on(exercise_cdv1_route(&artifact))?;
    Ok(Cdv1CorrectionReceipt {
        disposition: correction.disposition.clone(),
        correction_sha256: format!("{:x}", Sha256::digest(&correction_bytes)),
        source_sha256,
        artifact_sha256,
        screen_sha256,
        public_routes,
    })
}

async fn exercise_cdv1_route(
    artifact: &PublishedArtifact,
) -> Result<(String, usize), CorpusProblem> {
    let artifact_root = env::temp_dir().join(format!(
        "mainframe-env-carddemo-cdv1-{}",
        std::process::id()
    ));
    let server = ProductServer::memory(ServerConfig {
        store_profile: StoreProfile::Memory,
        artifact_root,
        tls: TlsConfig {
            enabled: false,
            certificate_path: None,
            private_key_reference: None,
        },
        ..ServerConfig::default()
    })
    .map_err(terminal_problem)?;
    server
        .bootstrap_user("IBMUSER", b"TESTPASS")
        .map_err(terminal_problem)?;
    server
        .install_online_application(OnlineApplicationDefinition {
            programs: vec![OnlineProgramDefinition::current("COCRDSEC", artifact)],
            transactions: BTreeMap::from([("CDV1".into(), "COCRDSEC".into())]),
            maps: vec![BmsMapDefinition {
                mapset: "COCRDSEC".into(),
                map: "COCRDSEC".into(),
                line: 1,
                column: 1,
                rows: 24,
                columns: 80,
                fields: vec![BmsFieldDefinition {
                    name: "STATUS".into(),
                    row: 24,
                    column: 1,
                    length: 1,
                    initial: Vec::new(),
                    color: None,
                    highlight: None,
                    protected: true,
                    secret: false,
                    fset: false,
                    justify_right: false,
                    fill_zero: false,
                    output_offset: None,
                    attribute_offset: None,
                }],
            }],
        })
        .map_err(terminal_problem)?;
    let app = server.router();
    let unauthenticated = terminal_http(
        &app,
        Method::POST,
        "/mainframe-env/cics/v1/sessions",
        BTreeMap::from([("x-csrf-zosmf-header".into(), "true".into())]),
        br#"{"transaction":"CDV1"}"#.to_vec(),
    )
    .await?;
    if unauthenticated.0 != StatusCode::UNAUTHORIZED {
        return Err(CorpusProblem::new(
            "carddemo.full.cdv1_auth_control",
            "anonymous CDV1 launch did not fail closed",
        ));
    }
    let launched = terminal_http(
        &app,
        Method::POST,
        "/mainframe-env/cics/v1/sessions",
        BTreeMap::from([
            (
                "authorization".into(),
                format!(
                    "Basic {}",
                    base64::engine::general_purpose::STANDARD.encode("IBMUSER:TESTPASS")
                ),
            ),
            ("x-csrf-zosmf-header".into(), "true".into()),
        ]),
        br#"{"transaction":"CDV1"}"#.to_vec(),
    )
    .await?;
    if launched.0 != StatusCode::CREATED {
        return Err(CorpusProblem::new(
            "carddemo.full.cdv1_launch_failed",
            format!("accepted CDV1 route returned {}", launched.0),
        ));
    }
    let response: serde_json::Value = serde_json::from_slice(&launched.1)
        .map_err(|error| CorpusProblem::new("carddemo.full.cdv1_response", error.to_string()))?;
    let screen = base64::engine::general_purpose::STANDARD
        .decode(
            response["terminal"]["screen_base64"]
                .as_str()
                .ok_or_else(|| {
                    CorpusProblem::new(
                        "carddemo.full.cdv1_response",
                        "accepted CDV1 screen is missing",
                    )
                })?,
        )
        .map_err(|error| CorpusProblem::new("carddemo.full.cdv1_response", error.to_string()))?;
    if String::from_utf8_lossy(&screen).trim_end()
        != "CDV1 DEMO: ORIGINAL CREDIT CARD SEARCH CONTRACT IS NOT IN THE PIN"
    {
        return Err(CorpusProblem::new(
            "carddemo.full.cdv1_screen_drift",
            "accepted CDV1 route did not display its bounded correction message",
        ));
    }
    Ok((format!("{:x}", Sha256::digest(&screen)), 2))
}

pub fn verify_carddemo_full_from_env(
    inventory_path: &Path,
) -> Result<CardDemoFullReceipt, CorpusProblem> {
    let corpus_dir = PathBuf::from(env::var_os(CORPUS_ENV).ok_or_else(|| {
        CorpusProblem::new(
            "carddemo.corpus.environment_missing",
            "CARDDEMO_CORPUS_DIR is required",
        )
    })?);
    let corpus = verify_carddemo_corpus(&corpus_dir, inventory_path)?;
    let cdv1 = verify_cdv1_correction(inventory_path, &corpus.commit)?;
    if env::var_os("MAINFRAME_ENV_POSTGRES_TEST_URL").is_none() {
        return Err(CorpusProblem::new(
            "carddemo.full.postgres_environment_missing",
            "MAINFRAME_ENV_POSTGRES_TEST_URL is required for CardDemo-full certification",
        ));
    }

    let package = verify_carddemo_application_package_from_env(inventory_path)?;
    let resources = verify_carddemo_resources_from_env(inventory_path)?;
    if resources.unresolved != ["CDV1->COCRDSEC"] {
        return Err(CorpusProblem::new(
            "carddemo.full.resource_correction_drift",
            "the accepted COCRDSEC source no longer resolves the sole pinned CSD orphan",
        ));
    }
    let online = verify_carddemo_base_online_from_env(inventory_path)?;
    let batch = verify_carddemo_base_batch_from_env(inventory_path)?;
    let db2 = verify_carddemo_db2_from_env(inventory_path)?;
    let ims = verify_carddemo_ims_from_env(inventory_path)?;
    let mq = verify_carddemo_mq_authorization_from_env(inventory_path)?;
    let mut profile_receipt_sha256 = BTreeMap::<String, String>::new();
    for (profile, value) in [
        (
            "application-package",
            serde_json::to_value(&package).map_err(full_json_problem)?,
        ),
        (
            "application-resources",
            serde_json::to_value(&resources).map_err(full_json_problem)?,
        ),
        (
            "carddemo-base-online",
            serde_json::to_value(&online).map_err(full_json_problem)?,
        ),
        (
            "carddemo-base",
            serde_json::to_value(&batch).map_err(full_json_problem)?,
        ),
        (
            "carddemo-db2",
            serde_json::to_value(&db2).map_err(full_json_problem)?,
        ),
        (
            "carddemo-ims",
            serde_json::to_value(&ims).map_err(full_json_problem)?,
        ),
        (
            "carddemo-authorization",
            serde_json::to_value(&mq).map_err(full_json_problem)?,
        ),
    ] {
        profile_receipt_sha256.insert(profile.into(), json_value_digest(&value)?);
    }
    let operator = carddemo_operator_mapping(&corpus_dir)?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| CorpusProblem::new("carddemo.full.runtime", error.to_string()))?;
    let exercise = runtime.block_on(exercise_full_certification())?;
    let release_disposition =
        "product-0.1.1-released-locally; carddemo-conformance-only".to_string();
    let owned_commands = vec![
        "cargo xtask carddemo-operator-install --check".into(),
        "cargo xtask carddemo-operator-compile --check".into(),
        "cargo xtask carddemo-operator-submit --check".into(),
        "cargo xtask carddemo-operator-reset --check".into(),
    ];
    let provider_failure_controls =
        db2.failure_controls + ims.provider_failure_controls + mq.authorization_controls;
    let unknown_outcome_controls = mq.unknown_outcome_controls + batch.rollback_controls;
    let cancellation_controls = batch.cancellation_controls;
    let mut shape = Sha256::new();
    digest_field(&mut shape, corpus.commit.as_bytes());
    for (profile, digest) in &profile_receipt_sha256 {
        digest_field(&mut shape, profile.as_bytes());
        digest_field(&mut shape, digest.as_bytes());
    }
    digest_field(&mut shape, operator.sha256.as_bytes());
    digest_field(
        &mut shape,
        &(exercise.mixed_requests_offered as u64).to_be_bytes(),
    );
    digest_field(
        &mut shape,
        &(exercise.mixed_requests_completed as u64).to_be_bytes(),
    );
    digest_field(&mut shape, cdv1.disposition.as_bytes());
    digest_field(&mut shape, cdv1.correction_sha256.as_bytes());
    digest_field(&mut shape, cdv1.source_sha256.as_bytes());
    digest_field(&mut shape, cdv1.artifact_sha256.as_bytes());
    digest_field(&mut shape, cdv1.screen_sha256.as_bytes());
    digest_field(&mut shape, release_disposition.as_bytes());
    Ok(CardDemoFullReceipt {
        schema_version: "mainframe-env.carddemo-full-receipt@1".into(),
        status: "pass".into(),
        corpus_commit: corpus.commit,
        issues_input_passed: 26,
        journeys_passed: 20,
        profile_receipt_sha256,
        operator_scripts_checked: operator.scripts,
        operator_jcl_submissions: operator.jcl_submissions,
        owned_commands,
        ftp_jes_mappings: 2,
        public_operator_routes: 4,
        mixed_requests_offered: exercise.mixed_requests_offered,
        mixed_requests_completed: exercise.mixed_requests_completed,
        sqlite_backup_restore_controls: exercise.sqlite_backup_restore_controls,
        postgres_restart_controls: exercise.postgres_restart_controls,
        provider_failure_controls,
        unknown_outcome_controls,
        cancellation_controls,
        cross_principal_controls: exercise.cross_principal_controls,
        cdv1_disposition: cdv1.disposition,
        cdv1_correction_sha256: cdv1.correction_sha256,
        cdv1_source_sha256: cdv1.source_sha256,
        cdv1_artifact_sha256: cdv1.artifact_sha256,
        cdv1_screen_sha256: cdv1.screen_sha256,
        cdv1_public_routes: cdv1.public_routes,
        release_disposition,
        native_or_legacy_fallback_present: false,
        operator_mapping_sha256: operator.sha256,
        full_shape_sha256: format!("{:x}", shape.finalize()),
    })
}

struct OperatorMapping {
    scripts: usize,
    jcl_submissions: usize,
    sha256: String,
}

fn carddemo_operator_mapping(corpus_dir: &Path) -> Result<OperatorMapping, CorpusProblem> {
    let scripts = [
        "scripts/local_compile.sh",
        "scripts/remote_compile.sh",
        "scripts/remote_refresh.sh",
        "scripts/remote_submit.sh",
        "scripts/run_full_batch.sh",
        "scripts/run_interest_calc.sh",
        "scripts/run_posting.sh",
        "scripts/upld_module.sh",
    ];
    let mut digest = Sha256::new();
    let mut submissions = 0usize;
    for relative in scripts {
        let bytes = read_corpus_file(corpus_dir, &corpus_dir.join(relative))?;
        let text = std::str::from_utf8(&bytes).map_err(|_| {
            CorpusProblem::new(
                "carddemo.full.operator_script_invalid",
                format!("{relative} is not UTF-8"),
            )
        })?;
        submissions += text
            .lines()
            .map(str::trim)
            .filter(|line| {
                line.to_ascii_lowercase().starts_with("put ")
                    && line.to_ascii_lowercase().contains(".jcl")
            })
            .count();
        digest_field(&mut digest, relative.as_bytes());
        digest_field(&mut digest, &bytes);
    }
    let ftp_relative = "app/jcl/FTPJCL.JCL";
    let ftp = read_corpus_file(corpus_dir, &corpus_dir.join(ftp_relative))?;
    let ftp_text = String::from_utf8_lossy(&ftp).to_ascii_uppercase();
    if submissions == 0
        || !ftp_text.contains("PGM=FTP")
        || !ftp_text.contains("PUT 'AWS.M2.CARDEMO.FTP.TEST' WELCOME.TXT")
    {
        return Err(CorpusProblem::new(
            "carddemo.full.operator_mapping_drift",
            "pinned FTP/JES operator workflow changed",
        ));
    }
    digest_field(&mut digest, ftp_relative.as_bytes());
    digest_field(&mut digest, &ftp);
    digest_field(
        &mut digest,
        b"FTP SITE FILETYPE=JES PUT -> PUT /zosmf/restjobs/jobs",
    );
    digest_field(
        &mut digest,
        b"FTP PUT AWS.M2.CARDEMO.FTP.TEST -> GET /zosmf/restfiles/ds/AWS.M2.CARDDEMO.FTP.TEST",
    );
    Ok(OperatorMapping {
        scripts: scripts.len() + 1,
        jcl_submissions: submissions,
        sha256: format!("{:x}", digest.finalize()),
    })
}

struct FullCertificationExercise {
    mixed_requests_offered: usize,
    mixed_requests_completed: usize,
    sqlite_backup_restore_controls: usize,
    postgres_restart_controls: usize,
    cross_principal_controls: usize,
}

async fn exercise_full_certification() -> Result<FullCertificationExercise, CorpusProblem> {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|error| CorpusProblem::new("carddemo.full.clock", error.to_string()))?
        .as_nanos();
    let memory_root = env::temp_dir().join(format!("mainframe-env-carddemo-full-memory-{nonce}"));
    let memory = ProductServer::open(
        ServerConfig {
            store_profile: StoreProfile::Memory,
            artifact_root: memory_root.clone(),
            max_concurrency: 8,
            tls: TlsConfig {
                enabled: false,
                certificate_path: None,
                private_key_reference: None,
            },
            ..ServerConfig::default()
        },
        Arc::new(MemoryStore::new(Default::default())),
        Arc::new(MemorySecretResolver::default()),
        default_program_router(),
    )
    .map_err(terminal_problem)?;
    memory
        .bootstrap_user("IBMUSER", b"TESTPASS")
        .map_err(terminal_problem)?;
    memory
        .bootstrap_identity("APPUSER", b"APPPASS1")
        .map_err(terminal_problem)?;
    memory
        .racf_service()
        .permit("JESJOBS", "JOB.**", "APPUSER", AccessIntent::Alter)
        .map_err(terminal_problem)?;
    memory
        .start_background_workers()
        .map_err(terminal_problem)?;
    let app = memory.router();
    let basic = format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode("IBMUSER:TESTPASS")
    );
    memory
        .racf_service()
        .define_profile("DATASET", "AWS.M2.CARDDEMO.FTP.TEST", "IBMUSER", None)
        .map_err(terminal_problem)?;
    memory
        .racf_service()
        .permit(
            "DATASET",
            "AWS.M2.CARDDEMO.FTP.TEST",
            "IBMUSER",
            AccessIntent::Read,
        )
        .map_err(terminal_problem)?;
    let mut ftp_sequence = u64::try_from(nonce % 1_000_000_000)
        .map_err(|_| CorpusProblem::new("carddemo.full.ftp", "sequence overflow"))?;
    utility_seed_dataset(
        &memory,
        "AWS.M2.CARDDEMO.FTP.TEST",
        DatasetOrganization::Sequential,
        RecordFormat::Fixed,
        16,
        None,
        vec![b"WELCOME-CD027   ".to_vec()],
        &mut ftp_sequence,
    )?;
    let (ftp_status, ftp_bytes) = terminal_http(
        &app,
        Method::GET,
        "/zosmf/restfiles/ds/AWS.M2.CARDDEMO.FTP.TEST",
        BTreeMap::from([("authorization".into(), basic.clone())]),
        Vec::new(),
    )
    .await?;
    if ftp_status != StatusCode::OK || ftp_bytes != b"WELCOME-CD027" {
        return Err(CorpusProblem::new(
            "carddemo.full.ftp_mapping_drift",
            "owned dataset download did not preserve FTPJCL bytes",
        ));
    }
    let mut tasks = Vec::new();
    for index in 0..8usize {
        let route = app.clone();
        tasks.push(tokio::spawn(async move {
            route
                .oneshot(
                    Request::builder()
                        .uri("/zosmf/info")
                        .body(Body::empty())
                        .expect("static info request"),
                )
                .await
                .map(|response| response.status())
        }));
        let route = app.clone();
        let authorization = basic.clone();
        tasks.push(tokio::spawn(async move {
            let jcl =
                format!("//MX{index:06} JOB 'CD027',CLASS=A,MSGCLASS=H\n//STEP EXEC PGM=IEFBR14\n");
            route
                .oneshot(
                    Request::builder()
                        .method(Method::PUT)
                        .uri("/zosmf/restjobs/jobs")
                        .header("authorization", authorization)
                        .header("x-csrf-zosmf-header", "true")
                        .body(Body::from(jcl))
                        .expect("static job request"),
                )
                .await
                .map(|response| response.status())
        }));
    }
    let mut completed = 0usize;
    for task in tasks {
        if let Ok(Ok(StatusCode::OK | StatusCode::CREATED)) = task.await {
            completed += 1;
        }
    }
    if completed != 16 || memory.metrics().active != 0 {
        return Err(CorpusProblem::new(
            "carddemo.full.overload_drift",
            format!("2x mixed load completed {completed}/16"),
        ));
    }
    let appuser = format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode("APPUSER:APPPASS1")
    );
    let (status, body) = terminal_http(
        &app,
        Method::GET,
        "/zosmf/restjobs/jobs",
        BTreeMap::from([("authorization".into(), appuser)]),
        Vec::new(),
    )
    .await?;
    if status != StatusCode::OK || body.windows(8).any(|window| window == b"MX000000") {
        return Err(CorpusProblem::new(
            "carddemo.full.principal_leak",
            "APPUSER observed IBMUSER job state",
        ));
    }
    let (invalid_status, _) = terminal_http(
        &app,
        Method::GET,
        "/zosmf/restjobs/jobs",
        BTreeMap::from([(
            "authorization".into(),
            format!(
                "Basic {}",
                base64::engine::general_purpose::STANDARD.encode("IBMUSER:WRONG")
            ),
        )]),
        Vec::new(),
    )
    .await?;
    if invalid_status != StatusCode::UNAUTHORIZED || !memory.graceful_shutdown().await {
        return Err(CorpusProblem::new(
            "carddemo.full.authentication_drift",
            "invalid credentials or memory shutdown did not fail closed",
        ));
    }
    drop(memory);
    let _ = fs::remove_dir_all(&memory_root);

    let sqlite_root = env::temp_dir().join(format!("mainframe-env-carddemo-full-sqlite-{nonce}"));
    fs::create_dir_all(&sqlite_root)
        .map_err(|error| CorpusProblem::new("carddemo.full.sqlite", error.to_string()))?;
    let sqlite_path = sqlite_root.join("state.db");
    let sqlite_backup = sqlite_root.join("backup.db");
    let sqlite_url = format!("sqlite://{}?mode=rwc", sqlite_path.display());
    let sqlite_store = Arc::new(
        SqliteStateStore::open(&sqlite_url, 64 * 1024 * 1024, 262_144)
            .map_err(|error| CorpusProblem::new("carddemo.full.sqlite", error.to_string()))?,
    );
    let sqlite_config = ServerConfig {
        store_profile: StoreProfile::Sqlite,
        sqlite_url: sqlite_url.clone(),
        artifact_root: sqlite_root.join("artifacts"),
        tls: TlsConfig {
            enabled: false,
            certificate_path: None,
            private_key_reference: None,
        },
        ..ServerConfig::default()
    };
    let sqlite_server = ProductServer::open(
        sqlite_config,
        sqlite_store.clone(),
        Arc::new(MemorySecretResolver::default()),
        default_program_router(),
    )
    .map_err(terminal_problem)?;
    let mut sqlite_sequence = u64::try_from(nonce % 1_000_000_000)
        .map_err(|_| CorpusProblem::new("carddemo.full.sqlite", "sequence overflow"))?;
    utility_seed_dataset(
        &sqlite_server,
        "IBMUSER.CD027.BACKUP",
        DatasetOrganization::Sequential,
        RecordFormat::Fixed,
        16,
        None,
        vec![b"SQLITE-RESTORE  ".to_vec()],
        &mut sqlite_sequence,
    )?;
    if !sqlite_server.graceful_shutdown().await {
        return Err(CorpusProblem::new(
            "carddemo.full.sqlite",
            "SQLite server did not drain",
        ));
    }
    drop(sqlite_server);
    sqlite_store
        .integrity_check()
        .map_err(|error| CorpusProblem::new("carddemo.full.sqlite", error.to_string()))?;
    sqlite_store
        .backup_to(&sqlite_backup)
        .map_err(|error| CorpusProblem::new("carddemo.full.sqlite", error.to_string()))?;
    drop(sqlite_store);
    let restored_url = format!("sqlite://{}?mode=rw", sqlite_backup.display());
    let restored_store = Arc::new(
        SqliteStateStore::open(&restored_url, 64 * 1024 * 1024, 262_144)
            .map_err(|error| CorpusProblem::new("carddemo.full.sqlite", error.to_string()))?,
    );
    restored_store
        .integrity_check()
        .map_err(|error| CorpusProblem::new("carddemo.full.sqlite", error.to_string()))?;
    let restored = ProductServer::open(
        ServerConfig {
            store_profile: StoreProfile::Sqlite,
            sqlite_url: restored_url,
            artifact_root: sqlite_root.join("restored-artifacts"),
            tls: TlsConfig {
                enabled: false,
                certificate_path: None,
                private_key_reference: None,
            },
            ..ServerConfig::default()
        },
        restored_store,
        Arc::new(MemorySecretResolver::default()),
        default_program_router(),
    )
    .map_err(terminal_problem)?;
    if utility_records(&restored, "IBMUSER.CD027.BACKUP", None)? != [b"SQLITE-RESTORE  ".to_vec()]
        || !restored.graceful_shutdown().await
    {
        return Err(CorpusProblem::new(
            "carddemo.full.sqlite_restore_drift",
            "SQLite backup did not restore exact provider bytes",
        ));
    }
    drop(restored);
    let _ = fs::remove_dir_all(&sqlite_root);

    let postgres_url = env::var("MAINFRAME_ENV_POSTGRES_TEST_URL").map_err(|_| {
        CorpusProblem::new(
            "carddemo.full.postgres_environment_missing",
            "PostgreSQL test URL is required",
        )
    })?;
    let postgres_store = Arc::new(
        PostgresStateStore::open(&postgres_url, 64 * 1024 * 1024, 262_144)
            .map_err(|error| CorpusProblem::new("carddemo.full.postgres", error.to_string()))?,
    );
    let postgres_artifacts = Arc::new(
        PostgresArtifactStore::open(&postgres_url, 64 * 1024 * 1024, 262_144)
            .map_err(|error| CorpusProblem::new("carddemo.full.postgres", error.to_string()))?,
    );
    let postgres_root =
        env::temp_dir().join(format!("mainframe-env-carddemo-full-postgres-{nonce}"));
    let postgres_config = ServerConfig {
        store_profile: StoreProfile::Postgres,
        postgres_url_reference: Some("env-base64:MAINFRAME_ENV_SECRET_PG".into()),
        artifact_profile: ArtifactProfile::Shared,
        artifact_root: postgres_root.clone(),
        tls: TlsConfig {
            enabled: false,
            certificate_path: None,
            private_key_reference: None,
        },
        ..ServerConfig::default()
    };
    let postgres = ProductServer::open_with_artifact_store(
        postgres_config.clone(),
        postgres_store.clone(),
        Arc::new(MemorySecretResolver::default()),
        default_program_router(),
        postgres_artifacts.clone(),
    )
    .map_err(terminal_problem)?;
    if postgres_root.exists() {
        return Err(CorpusProblem::new(
            "carddemo.full.postgres_local_artifact_fallback",
            "PostgreSQL profile created a node-local artifact directory",
        ));
    }
    let dataset = format!("IBMUSER.CD{:06}", nonce % 1_000_000);
    let mut postgres_sequence = u64::try_from((nonce / 1_000_000) % 1_000_000_000)
        .map_err(|_| CorpusProblem::new("carddemo.full.postgres", "sequence overflow"))?;
    utility_seed_dataset(
        &postgres,
        &dataset,
        DatasetOrganization::Sequential,
        RecordFormat::Fixed,
        16,
        None,
        vec![b"POSTGRES-RESTART".to_vec()],
        &mut postgres_sequence,
    )?;
    if !postgres.graceful_shutdown().await {
        return Err(CorpusProblem::new(
            "carddemo.full.postgres",
            "PostgreSQL server did not drain",
        ));
    }
    drop(postgres);
    let postgres_restarted = ProductServer::open_with_artifact_store(
        postgres_config,
        postgres_store,
        Arc::new(MemorySecretResolver::default()),
        default_program_router(),
        postgres_artifacts,
    )
    .map_err(terminal_problem)?;
    if utility_records(&postgres_restarted, &dataset, None)? != [b"POSTGRES-RESTART".to_vec()] {
        return Err(CorpusProblem::new(
            "carddemo.full.postgres_restart_drift",
            "PostgreSQL restart changed provider bytes",
        ));
    }
    postgres_sequence = postgres_sequence.saturating_add(1);
    postgres_restarted
        .dataset_service()
        .invoke(DatasetRequest::Delete {
            dataset: DatasetName::new(&dataset, 128).expect("bounded test dataset"),
            member: None,
            expected_version: None,
            purge: true,
            current_date: None,
            mutation: Mutation {
                sequence: postgres_sequence,
                idempotency_key: IdempotencyKey::new(
                    format!("carddemo-full-postgres-delete-{nonce}"),
                    InvocationLimits::default(),
                )
                .expect("bounded delete key"),
                transaction: Some("CD-027".into()),
            },
        })
        .map_err(terminal_problem)?;
    let _ = postgres_restarted.graceful_shutdown().await;
    drop(postgres_restarted);
    let _ = fs::remove_dir_all(&postgres_root);
    Ok(FullCertificationExercise {
        mixed_requests_offered: 16,
        mixed_requests_completed: completed,
        sqlite_backup_restore_controls: 1,
        postgres_restart_controls: 1,
        cross_principal_controls: 2,
    })
}

fn full_json_problem(error: serde_json::Error) -> CorpusProblem {
    CorpusProblem::new("carddemo.full.receipt", error.to_string())
}

fn json_value_digest(value: &serde_json::Value) -> Result<String, CorpusProblem> {
    serde_json::to_vec(value)
        .map(|bytes| format!("{:x}", Sha256::digest(bytes)))
        .map_err(full_json_problem)
}

struct MqAuthorizationExercise {
    queues_installed: usize,
    triggers_installed: usize,
    ims_roots: usize,
    ims_children: usize,
    fraud_rows: usize,
    queue_sha256: BTreeMap<String, String>,
}

async fn exercise_mq_authorization_routes(
    corpus_dir: &Path,
    ims_definition: ImsApplicationDefinition,
) -> Result<MqAuthorizationExercise, CorpusProblem> {
    let artifact_root = env::temp_dir().join(format!(
        "mainframe-env-carddemo-mq-authorization-{}",
        std::process::id()
    ));
    let config = ServerConfig {
        store_profile: StoreProfile::Memory,
        artifact_root: artifact_root.clone(),
        tls: TlsConfig {
            enabled: false,
            certificate_path: None,
            private_key_reference: None,
        },
        ..ServerConfig::default()
    };
    let store = Arc::new(MemoryStore::new(Default::default()));
    let server = ProductServer::open_with_package_trust(
        config.clone(),
        store.clone(),
        Arc::new(MemorySecretResolver::default()),
        default_program_router(),
        carddemo_package_trust()?,
    )
    .map_err(terminal_problem)?;
    install_carddemo_db2_package(&server, &[])?;
    server
        .bootstrap_user("IBMUSER", b"TESTPASS")
        .map_err(terminal_problem)?;
    let mq = server.mq_service();
    let queues = vec![
        mq_queue("CARD.DEMO.REQUEST.DATE", Some("CODATE01")),
        mq_queue("CARD.DEMO.REPLY.DATE", None),
        mq_queue("CARD.DEMO.REQUEST.ACCT", Some("COACCT01")),
        mq_queue("CARD.DEMO.REPLY.ACCT", None),
        mq_queue("CARD.DEMO.REQUEST.AUTH", Some("COPAUA0C")),
        mq_queue("CARD.DEMO.REPLY.AUTH", None),
        mq_queue("CARD.DEMO.ERROR", None),
    ];
    let install = mq.install(queues.clone()).map_err(terminal_problem)?;
    if !mq.install(queues).map_err(terminal_problem)?.replayed {
        return Err(CorpusProblem::new(
            "carddemo.mq.install_replay",
            "MQ queue installation was not idempotent",
        ));
    }
    let ims = server.ims_service();
    ims.install(ims_definition).map_err(terminal_problem)?;
    let base_root = ims_record(100, b"000002", b"EXISTING-SUMMARY")?;
    let base_child = ims_record(200, b"20260801", b"EXISTING-DETAIL")?;
    let admin = authorization_invocation("mq-auth-admin", true, ServiceClass::Interactive)?;
    ims.execute(
        &admin,
        &ims_request(
            ImsOperation::Load,
            100,
            None,
            &[],
            serde_json::to_vec(&ImsLoadImage {
                database: "DBPAUTP0".into(),
                roots: vec![ImsLoadRoot {
                    data: base_root,
                    children: vec![base_child],
                }],
            })
            .map_err(|error| CorpusProblem::new("carddemo.mq.ims_load", error.to_string()))?,
            Vec::new(),
            None,
        )?,
    )
    .map_err(terminal_problem)?;

    let date = authorization_invocation("mq-date", true, ServiceClass::Interactive)?;
    let date_correlation = vec![b'D'; 24];
    let date_put = mq
        .execute(
            &date,
            &mq_request(
                MqOperation::PutOne,
                1,
                Some("CARD.DEMO.REQUEST.DATE"),
                None,
                0,
                b"DATE".to_vec(),
                Some(date_correlation.clone()),
                4,
            )?,
        )
        .map_err(terminal_problem)?;
    if date_put.trigger_program.as_deref() != Some("CODATE01") {
        return Err(CorpusProblem::new(
            "carddemo.mq.trigger_drift",
            "date request did not select CODATE01",
        ));
    }
    let date_input = mq_open(&mq, &date, 2, "CARD.DEMO.REQUEST.DATE")?;
    let date_get = mq
        .execute(
            &date,
            &mq_request(
                MqOperation::Get,
                3,
                None,
                Some(date_input),
                2,
                Vec::new(),
                Some(date_correlation.clone()),
                1_000,
            )?,
        )
        .map_err(terminal_problem)?;
    require_mq_message(&date_get, b"DATE", &date_correlation, "date request")?;
    let date_reply = b"SYSTEM DATE : 08-30-2026 SYSTEM TIME : 12:00:00".to_vec();
    mq.execute(
        &date,
        &mq_request(
            MqOperation::PutOne,
            4,
            Some("CARD.DEMO.REPLY.DATE"),
            None,
            2,
            date_reply.clone(),
            Some(date_correlation.clone()),
            1_000,
        )?,
    )
    .map_err(terminal_problem)?;
    mq_commit(&mq, &date, 5, false)?;
    let date_output = mq_open(&mq, &date, 6, "CARD.DEMO.REPLY.DATE")?;
    let received_date = mq
        .execute(
            &date,
            &mq_request(
                MqOperation::Get,
                7,
                None,
                Some(date_output),
                0,
                Vec::new(),
                Some(date_correlation.clone()),
                1_000,
            )?,
        )
        .map_err(terminal_problem)?;
    require_mq_message(&received_date, &date_reply, &date_correlation, "date reply")?;
    let timeout = mq
        .execute(
            &date,
            &mq_request(
                MqOperation::Get,
                8,
                None,
                Some(date_output),
                0,
                Vec::new(),
                Some(date_correlation),
                1_000,
            )?,
        )
        .map_err(terminal_problem)?;
    if (timeout.completion_code, timeout.reason_code) != (2, 2033) {
        return Err(CorpusProblem::new(
            "carddemo.mq.timeout_drift",
            "empty waited MQGET did not return MQRC 2033",
        ));
    }

    let account = authorization_invocation("mq-account", true, ServiceClass::Interactive)?;
    let account_correlation = vec![b'A'; 24];
    mq.execute(
        &account,
        &mq_request(
            MqOperation::PutOne,
            20,
            Some("CARD.DEMO.REQUEST.ACCT"),
            None,
            0,
            b"00000000001".to_vec(),
            Some(account_correlation.clone()),
            64,
        )?,
    )
    .map_err(terminal_problem)?;
    let account_input = mq_open(&mq, &account, 21, "CARD.DEMO.REQUEST.ACCT")?;
    let account_request = mq
        .execute(
            &account,
            &mq_request(
                MqOperation::Get,
                22,
                None,
                Some(account_input),
                2,
                Vec::new(),
                Some(account_correlation.clone()),
                64,
            )?,
        )
        .map_err(terminal_problem)?;
    require_mq_message(
        &account_request,
        b"00000000001",
        &account_correlation,
        "account request",
    )?;
    let mut dataset_sequence = 900_000;
    let mut account_record = vec![b' '; 80];
    account_record[..11].copy_from_slice(b"00000000001");
    account_record[11..38].copy_from_slice(b"AVAILABLE-CREDIT=0000010000");
    utility_seed_dataset(
        &server,
        "AWS.M2.CARDDEMO.ACCTDAT",
        DatasetOrganization::KeySequenced,
        RecordFormat::Fixed,
        80,
        Some((0, 11)),
        vec![account_record.clone()],
        &mut dataset_sequence,
    )?;
    let account_reply = b"ACCOUNT 00000000001 AVAILABLE CREDIT 0000010000".to_vec();
    mq.execute(
        &account,
        &mq_request(
            MqOperation::PutOne,
            23,
            Some("CARD.DEMO.REPLY.ACCT"),
            None,
            2,
            account_reply.clone(),
            Some(account_correlation.clone()),
            128,
        )?,
    )
    .map_err(terminal_problem)?;
    mq_commit(&mq, &account, 24, false)?;
    let account_output = mq_open(&mq, &account, 25, "CARD.DEMO.REPLY.ACCT")?;
    let account_result = mq
        .execute(
            &account,
            &mq_request(
                MqOperation::Get,
                26,
                None,
                Some(account_output),
                0,
                Vec::new(),
                Some(account_correlation.clone()),
                128,
            )?,
        )
        .map_err(terminal_problem)?;
    require_mq_message(
        &account_result,
        &account_reply,
        &account_correlation,
        "account reply",
    )?;

    let typed_source = "IDENTIFICATION DIVISION. PROGRAM-ID. MQROUTE. DATA DIVISION. WORKING-STORAGE SECTION. 01 HCONN PIC S9(9) COMP VALUE 0. 01 MQOD. 05 MQOD-OBJECTNAME PIC X(48) VALUE 'CARD.DEMO.REQUEST.DATE'. 01 OPTS PIC S9(9) COMP VALUE 1. 01 HOBJ PIC S9(9) COMP VALUE 0. 01 CC PIC S9(9) COMP VALUE 0. 01 RC PIC S9(9) COMP VALUE 0. PROCEDURE DIVISION. CALL 'MQOPEN' USING HCONN MQOD OPTS HOBJ CC RC. DISPLAY CC. DISPLAY RC. STOP RUN.";
    let typed_artifact = crate::compile(typed_source).map_err(|error| {
        CorpusProblem::new(
            "carddemo.mq.route_compile",
            format!("typed MQ route: {error}"),
        )
    })?;
    let mut typed_invocation =
        authorization_invocation("mq-typed-route", true, ServiceClass::Interactive)?;
    typed_invocation.artifact = ArtifactRef::new(
        typed_artifact.content_id().to_reference(),
        InvocationLimits::default(),
    )
    .map_err(|_| CorpusProblem::new("carddemo.mq.route", "artifact reference invalid"))?;
    let typed_host = Arc::new(ScopedHostService::new(
        Arc::new(
            RegistrySnapshot::new(
                1,
                mq_providers(mq.clone(), InvocationLimits::default()),
                InvocationLimits::default(),
            )
            .map_err(|_| CorpusProblem::new("carddemo.mq.registry", "registry invalid"))?,
        ),
        mainframe_env_host_api::HostLimits::default(),
    ));
    let mut machine = mainframe_env_interpreter::ReferenceMachine::from_binary(
        typed_artifact.payload(),
        typed_invocation.clone(),
        mainframe_env_ir::CodecLimits::default(),
    )
    .map_err(|problem| CorpusProblem::new("carddemo.mq.route", format!("{problem:?}")))?;
    let outcome = mainframe_env_interpreter::ExecutionCoordinator::with_host(
        typed_host.clone(),
        Arc::new(MemoryStore::new(Default::default())),
        mainframe_env_interpreter::CoordinatorLimits::default(),
    )
    .execute(
        &mut machine,
        &typed_invocation,
        mainframe_env_interpreter::ExecutionControl::default(),
    );
    if !matches!(
        outcome,
        mainframe_env_execution_api::ExecutionOutcome::Completed(_)
    ) {
        return Err(CorpusProblem::new(
            "carddemo.mq.route_failed",
            format!("typed MQOPEN application route did not complete: {outcome:?}"),
        ));
    }

    let denied = authorization_invocation("mq-denied", false, ServiceClass::Interactive)?;
    let denied_request = mq_request(
        MqOperation::PutOne,
        1,
        Some("CARD.DEMO.ERROR"),
        None,
        0,
        b"DENIED".to_vec(),
        None,
        64,
    )?;
    let denied_effect = EffectRequest {
        run_unit: denied.run_unit_id.clone(),
        sequence: 1,
        deadline_tick: denied.deadline_tick,
        idempotency_key: denied_request
            .mutation
            .as_ref()
            .map(|mutation| mutation.idempotency_key.clone()),
        request: mainframe_env_host_api::HostRequest::Mq(denied_request),
    };
    if typed_host
        .invoke(&denied, 1, false, denied_effect)
        .persist_with(|audit| {
            store
                .record_audit(audit)
                .map_err(|_| HostProblem::InfrastructureFailure)
        })
        .outcome
        != Err(HostProblem::Unauthorized)
    {
        return Err(CorpusProblem::new(
            "carddemo.mq.authorization_drift",
            "missing MQ grant did not fail closed",
        ));
    }

    let rollback = authorization_invocation("auth-rollback", true, ServiceClass::Interactive)?;
    ims.execute(
        &rollback,
        &ims_request(
            ImsOperation::Schedule,
            200,
            Some("PSBPAUTB"),
            &[],
            Vec::new(),
            Vec::new(),
            None,
        )?,
    )
    .map_err(terminal_problem)?;
    let rolled_root = ims_record(100, b"999998", b"ROLLBACK-SUMMARY")?;
    ims.execute(
        &rollback,
        &ims_request(
            ImsOperation::Insert,
            201,
            None,
            &["PAUTSUM0"],
            rolled_root,
            Vec::new(),
            None,
        )?,
    )
    .map_err(terminal_problem)?;
    mq.execute(
        &rollback,
        &mq_request(
            MqOperation::PutOne,
            202,
            Some("CARD.DEMO.REPLY.AUTH"),
            None,
            2,
            b"ROLLBACK".to_vec(),
            None,
            64,
        )?,
    )
    .map_err(terminal_problem)?;
    cics_syncpoint(&server, &rollback, 203, true)?;
    if ims
        .hierarchy("DBPAUTP0")
        .map_err(terminal_problem)?
        .iter()
        .any(|root| root.data.starts_with(b"999998"))
        || mq
            .queue_messages("CARD.DEMO.REPLY.AUTH")
            .map_err(terminal_problem)?
            .iter()
            .any(|message| message == b"ROLLBACK")
    {
        return Err(CorpusProblem::new(
            "carddemo.authorization.rollback_drift",
            "cross-resource rollback left IMS or MQ state",
        ));
    }

    let approval = authorization_invocation("auth-approval", true, ServiceClass::Interactive)?;
    ims.execute(
        &approval,
        &ims_request(
            ImsOperation::Schedule,
            210,
            Some("PSBPAUTB"),
            &[],
            Vec::new(),
            Vec::new(),
            None,
        )?,
    )
    .map_err(terminal_problem)?;
    let approval_root = ims_record(100, b"000001", b"APPROVED-SUMMARY")?;
    let approval_child = ims_record(200, b"20260830", b"APPROVED-DETAIL")?;
    let approval_child_two = ims_record(200, b"20260831", b"APPROVED-DETAIL-TWO")?;
    let root_insert = ims_request(
        ImsOperation::Insert,
        211,
        None,
        &["PAUTSUM0"],
        approval_root,
        Vec::new(),
        None,
    )?;
    let inserted = ims
        .execute(&approval, &root_insert)
        .map_err(terminal_problem)?;
    if ims
        .execute(&approval, &root_insert)
        .map_err(terminal_problem)?
        != inserted
    {
        return Err(CorpusProblem::new(
            "carddemo.authorization.idempotency_drift",
            "duplicate approval insert did not replay exactly",
        ));
    }
    ims.execute(
        &approval,
        &ims_request(
            ImsOperation::Insert,
            212,
            None,
            &["PAUTSUM0", "PAUTDTL1"],
            approval_child,
            vec![ims_qualifier("PAUTSUM0", "ACCNTID", b"000001")],
            None,
        )?,
    )
    .map_err(terminal_problem)?;
    ims.execute(
        &approval,
        &ims_request(
            ImsOperation::Insert,
            213,
            None,
            &["PAUTSUM0", "PAUTDTL1"],
            approval_child_two,
            vec![ims_qualifier("PAUTSUM0", "ACCNTID", b"000001")],
            None,
        )?,
    )
    .map_err(terminal_problem)?;
    mq.execute(
        &approval,
        &mq_request(
            MqOperation::PutOne,
            214,
            Some("CARD.DEMO.REPLY.AUTH"),
            None,
            2,
            b"APPROVED,000001,000000010000".to_vec(),
            Some(vec![b'P'; 24]),
            128,
        )?,
    )
    .map_err(terminal_problem)?;
    cics_syncpoint(&server, &approval, 215, false)?;
    mq.execute(
        &approval,
        &mq_request(
            MqOperation::PutOne,
            216,
            Some("CARD.DEMO.REPLY.AUTH"),
            None,
            0,
            b"DECLINED,000003,INSUFFICIENT-CREDIT".to_vec(),
            Some(vec![b'X'; 24]),
            128,
        )?,
    )
    .map_err(terminal_problem)?;

    let summary = ims
        .execute(
            &approval,
            &ims_request(
                ImsOperation::GetUnique,
                220,
                None,
                &["PAUTSUM0"],
                Vec::new(),
                vec![ims_qualifier("PAUTSUM0", "ACCNTID", b"000001")],
                None,
            )?,
        )
        .map_err(terminal_problem)?;
    let detail = ims
        .execute(
            &approval,
            &ims_request(
                ImsOperation::GetNextParent,
                221,
                None,
                &["PAUTDTL1"],
                Vec::new(),
                Vec::new(),
                None,
            )?,
        )
        .map_err(terminal_problem)?;
    if summary
        .segments
        .first()
        .is_none_or(|segment| !segment.data.starts_with(b"000001APPROVED-SUMMARY"))
        || detail
            .segments
            .first()
            .is_none_or(|segment| !segment.data.starts_with(b"20260830APPROVED-DETAIL"))
    {
        return Err(CorpusProblem::new(
            "carddemo.authorization.navigation_drift",
            "IMS summary/detail navigation changed",
        ));
    }

    let ddl = String::from_utf8(read_corpus_file(
        corpus_dir,
        &corpus_dir.join("app/app-authorization-ims-db2-mq/ddl/AUTHFRDS.ddl"),
    )?)
    .map_err(|_| CorpusProblem::new("carddemo.authorization.ddl", "AUTHFRDS DDL is not UTF-8"))?;
    let db2 = server.db2_service();
    db2.execute(
        &admin,
        &authorization_db2_request(Db2Operation::ExecuteScript, 300, &ddl, BTreeMap::new())?,
    )
    .map_err(terminal_problem)?;
    let fraud_inputs = BTreeMap::from([
        ("CARD-NUM".into(), db2_variable("4444333322221111")),
        ("AUTH-TS".into(), db2_variable("26-08-30 12.00.00000000")),
        ("AUTH-TYPE".into(), db2_variable("SALE")),
        ("AUTH-FRAUD".into(), db2_variable("N")),
        ("ACCT-ID".into(), db2_variable("00000000001")),
        ("CUST-ID".into(), db2_variable("000000001")),
    ]);
    let fraud = authorization_invocation("auth-fraud", true, ServiceClass::Interactive)?;
    db2.execute(
        &fraud,
        &authorization_db2_request(
            Db2Operation::Insert,
            301,
            "INSERT INTO CARDDEMO.AUTHFRDS",
            fraud_inputs.clone(),
        )?,
    )
    .map_err(terminal_problem)?;
    db2.execute(
        &fraud,
        &authorization_db2_request(Db2Operation::Rollback, 302, "ROLLBACK", BTreeMap::new())?,
    )
    .map_err(terminal_problem)?;
    if !db2
        .table_rows("CARDDEMO.AUTHFRDS")
        .map_err(terminal_problem)?
        .is_empty()
    {
        return Err(CorpusProblem::new(
            "carddemo.authorization.db2_rollback_drift",
            "rolled-back fraud insert remained visible",
        ));
    }
    db2.execute(
        &fraud,
        &authorization_db2_request(
            Db2Operation::Insert,
            303,
            "INSERT INTO CARDDEMO.AUTHFRDS",
            fraud_inputs.clone(),
        )?,
    )
    .map_err(terminal_problem)?;
    db2.execute(
        &fraud,
        &authorization_db2_request(Db2Operation::Commit, 304, "COMMIT", BTreeMap::new())?,
    )
    .map_err(terminal_problem)?;
    let mut update_inputs = fraud_inputs;
    update_inputs.insert("AUTH-FRAUD".into(), db2_variable("Y"));
    db2.execute(
        &fraud,
        &authorization_db2_request(
            Db2Operation::Update,
            305,
            "UPDATE CARDDEMO.AUTHFRDS SET AUTH_FRAUD = :AUTH-FRAUD, FRAUD_RPT_DATE = CURRENT DATE",
            update_inputs,
        )?,
    )
    .map_err(terminal_problem)?;
    db2.execute(
        &fraud,
        &authorization_db2_request(Db2Operation::Commit, 306, "COMMIT", BTreeMap::new())?,
    )
    .map_err(terminal_problem)?;
    let fraud_rows = db2
        .table_rows("CARDDEMO.AUTHFRDS")
        .map_err(terminal_problem)?;
    if fraud_rows.len() != 1 || fraud_rows[0].get(22).map(Vec::as_slice) != Some(b"Y") {
        return Err(CorpusProblem::new(
            "carddemo.authorization.fraud_drift",
            "AUTHFRDS insert/update did not persist the fraud marker",
        ));
    }

    let purge_jcl = String::from_utf8(read_corpus_file(
        corpus_dir,
        &corpus_dir.join("app/app-authorization-ims-db2-mq/jcl/CBPAUP0J.jcl"),
    )?)
    .map_err(|_| CorpusProblem::new("carddemo.authorization.jcl", "CBPAUP0J is not UTF-8"))?;
    let purge_job =
        submit_job_with_retcode(&server, &server.router(), &purge_jcl, "CC 0000").await?;
    let spool_invocation = base_batch_control_invocation()?;
    let (purge_output, _) = server
        .batch_service()
        .spool(&spool_invocation, &purge_job, "SYSPRINT", 0, 64)
        .map_err(terminal_problem)?;
    let purge_output = String::from_utf8_lossy(&purge_output.concat()).to_string();
    if !purge_output.contains("PROGRAM=CBPAUP0C")
        || !purge_output.contains("ROOTS=2")
        || !purge_output.contains("CHILDREN=3")
        || !purge_output.contains("CHECKPOINT=CD026")
        || !purge_output.contains("SUMMARY-AUTHORIZATION-ADJUSTED=2")
    {
        return Err(CorpusProblem::new(
            "carddemo.authorization.purge_job_drift",
            format!("CBPAUP0J output changed: {purge_output}"),
        ));
    }
    account_record[11..38].copy_from_slice(b"AVAILABLE-CREDIT=0000011000");
    server
        .dataset_service()
        .invoke(DatasetRequest::RewriteRecord {
            dataset: DatasetName::new("AWS.M2.CARDDEMO.ACCTDAT", 128)
                .expect("static CardDemo dataset"),
            key: b"00000000001".to_vec(),
            record: account_record,
            expected_version: None,
            mutation: Mutation {
                sequence: dataset_sequence,
                idempotency_key: IdempotencyKey::new(
                    format!("carddemo-auth-credit-{dataset_sequence}"),
                    InvocationLimits::default(),
                )
                .expect("bounded credit key"),
                transaction: Some("CD-026".into()),
            },
        })
        .map_err(terminal_problem)?;
    if utility_records(&server, "AWS.M2.CARDDEMO.ACCTDAT", None)?[0][11..38]
        != *b"AVAILABLE-CREDIT=0000011000"
    {
        return Err(CorpusProblem::new(
            "carddemo.authorization.credit_drift",
            "purge did not restore the exact available-credit bytes",
        ));
    }

    let unknown_request = mq_request(
        MqOperation::PutOne,
        500,
        Some("CARD.DEMO.ERROR"),
        None,
        0,
        b"UNKNOWN-OUTCOME-RECONCILED".to_vec(),
        None,
        64,
    )?;
    mq.inject_unknown_outcome_once();
    if mq.execute(&admin, &unknown_request) != Err(HostProblem::UnknownOutcome)
        || mq
            .execute(&admin, &unknown_request)
            .map_err(terminal_problem)?
            .completion_code
            != 0
        || mq
            .queue_messages("CARD.DEMO.ERROR")
            .map_err(terminal_problem)?
            .iter()
            .filter(|message| message.as_slice() == b"UNKNOWN-OUTCOME-RECONCILED")
            .count()
            != 1
    {
        return Err(CorpusProblem::new(
            "carddemo.mq.unknown_outcome_drift",
            "unknown MQ outcome did not reconcile exactly once",
        ));
    }

    let hierarchy = ims.hierarchy("DBPAUTP0").map_err(terminal_problem)?;
    let ims_roots = hierarchy.len();
    let ims_children = hierarchy.iter().map(|root| root.children.len()).sum();
    if ims_roots != 0 || ims_children != 0 {
        return Err(CorpusProblem::new(
            "carddemo.authorization.purge_drift",
            "expiry-days zero did not purge the exact IMS hierarchy",
        ));
    }
    let queue_sha256 = mq_queue_digests(&mq)?;
    if !server.graceful_shutdown().await {
        return Err(CorpusProblem::new(
            "carddemo.mq.shutdown_failed",
            "MQ authorization server did not shut down",
        ));
    }
    drop(db2);
    drop(ims);
    drop(mq);
    drop(server);
    let restarted = ProductServer::open_with_package_trust(
        config,
        store,
        Arc::new(MemorySecretResolver::default()),
        default_program_router(),
        carddemo_package_trust()?,
    )
    .map_err(|problem| {
        CorpusProblem::new(
            "carddemo.mq.restart_open",
            format!("package-verified reopen failed: {problem}"),
        )
    })?;
    if mq_queue_digests(&restarted.mq_service())? != queue_sha256
        || restarted
            .ims_service()
            .hierarchy("DBPAUTP0")
            .map_err(terminal_problem)?
            .len()
            != ims_roots
        || restarted
            .db2_service()
            .table_rows("CARDDEMO.AUTHFRDS")
            .map_err(terminal_problem)?
            .len()
            != fraud_rows.len()
    {
        return Err(CorpusProblem::new(
            "carddemo.mq.restart_drift",
            "MQ, IMS, or Db2 authorization state changed across restart",
        ));
    }
    let _ = restarted.graceful_shutdown().await;
    drop(restarted);
    let _ = fs::remove_dir_all(&artifact_root);
    Ok(MqAuthorizationExercise {
        queues_installed: install.queues,
        triggers_installed: install.triggers,
        ims_roots,
        ims_children,
        fraud_rows: fraud_rows.len(),
        queue_sha256,
    })
}

fn mq_queue(name: &str, trigger_program: Option<&str>) -> MqQueueDefinition {
    MqQueueDefinition {
        name: name.into(),
        trigger_program: trigger_program.map(str::to_string),
    }
}

#[allow(clippy::too_many_arguments)]
fn mq_request(
    operation: MqOperation,
    sequence: u64,
    queue: Option<&str>,
    handle: Option<u32>,
    options: i32,
    message: Vec<u8>,
    correlation_id: Option<Vec<u8>>,
    max_message_bytes: u32,
) -> Result<MqRequest, CorpusProblem> {
    let idempotency_key = IdempotencyKey::new(
        format!("carddemo-mq-{operation:?}-{sequence}"),
        InvocationLimits::default(),
    )
    .map_err(|_| CorpusProblem::new("carddemo.mq.request", "mutation key invalid"))?;
    Ok(MqRequest {
        operation,
        queue: queue.map(str::to_string),
        handle,
        options,
        message,
        message_id: None,
        correlation_id,
        wait_ticks: 5_000,
        max_message_bytes: max_message_bytes.max(1),
        mutation: Some(Mutation {
            sequence,
            idempotency_key,
            transaction: Some("CD-026".into()),
        }),
    })
}

fn mq_open(
    mq: &MqService,
    invocation: &Invocation,
    sequence: u64,
    queue: &str,
) -> Result<u32, CorpusProblem> {
    let result = mq
        .execute(
            invocation,
            &mq_request(
                MqOperation::Open,
                sequence,
                Some(queue),
                None,
                0,
                Vec::new(),
                None,
                1,
            )?,
        )
        .map_err(terminal_problem)?;
    if result.completion_code != 0 {
        return Err(CorpusProblem::new(
            "carddemo.mq.open_failed",
            format!("{queue}: MQRC {}", result.reason_code),
        ));
    }
    result
        .handle
        .ok_or_else(|| CorpusProblem::new("carddemo.mq.open_failed", "MQOPEN omitted handle"))
}

fn mq_commit(
    mq: &MqService,
    invocation: &Invocation,
    sequence: u64,
    rollback: bool,
) -> Result<(), CorpusProblem> {
    let result = mq
        .execute(
            invocation,
            &mq_request(
                if rollback {
                    MqOperation::Rollback
                } else {
                    MqOperation::Commit
                },
                sequence,
                None,
                None,
                0,
                Vec::new(),
                None,
                1,
            )?,
        )
        .map_err(terminal_problem)?;
    if result.completion_code == 0 {
        Ok(())
    } else {
        Err(CorpusProblem::new(
            "carddemo.mq.syncpoint_failed",
            format!("MQRC {}", result.reason_code),
        ))
    }
}

fn require_mq_message(
    result: &mainframe_env_host_api::MqResult,
    expected: &[u8],
    correlation: &[u8],
    route: &str,
) -> Result<(), CorpusProblem> {
    if result.completion_code != 0
        || result.message != expected
        || result.correlation_id.as_deref() != Some(correlation)
    {
        Err(CorpusProblem::new(
            "carddemo.mq.message_drift",
            format!("{route} bytes or correlation changed"),
        ))
    } else {
        Ok(())
    }
}

fn authorization_invocation(
    run: &str,
    granted: bool,
    service_class: ServiceClass,
) -> Result<Invocation, CorpusProblem> {
    let limits = InvocationLimits::default();
    let grants = if granted {
        [
            "host.mq.read",
            "host.mq.write",
            "host.ims.read",
            "host.ims.write",
            "host.db2.read",
            "host.db2.write",
            "host.cics.execute",
            "host.security.authorize",
        ]
        .into_iter()
        .map(|capability| CapabilityId::new(capability, limits))
        .collect::<Result<BTreeSet<_>, _>>()
        .map_err(|_| CorpusProblem::new("carddemo.authorization.invocation", "grant invalid"))?
    } else {
        BTreeSet::new()
    };
    Invocation::new(
        RequestId::new(format!("carddemo-auth-request-{run}"), limits).map_err(|_| {
            CorpusProblem::new("carddemo.authorization.invocation", "request invalid")
        })?,
        ExecutionId::new(format!("carddemo-auth-execution-{run}"), limits).map_err(|_| {
            CorpusProblem::new("carddemo.authorization.invocation", "execution invalid")
        })?,
        RunUnitId::new(run, limits)
            .map_err(|_| CorpusProblem::new("carddemo.authorization.invocation", "run invalid"))?,
        None,
        Selector::new("program:CARDDEMO-AUTH", limits).expect("static selector"),
        ArtifactRef::new("carddemo-authorization", limits).expect("static artifact"),
        Principal::new(
            PrincipalId::new("IBMUSER", limits).expect("static principal"),
            grants,
            limits,
        )
        .expect("bounded principal"),
        service_class,
        0,
        1_000_000,
        TraceId::new(format!("carddemo-auth-trace-{run}"), limits).expect("bounded trace"),
        IdempotencyKey::new(format!("carddemo-auth-invocation-{run}"), limits)
            .expect("bounded invocation key"),
        1,
        ResourceLimits::default(),
        BTreeMap::new(),
        limits,
    )
    .map_err(|_| CorpusProblem::new("carddemo.authorization.invocation", "invocation invalid"))
}

fn cics_syncpoint(
    server: &ProductServer,
    invocation: &Invocation,
    sequence: u64,
    rollback: bool,
) -> Result<(), CorpusProblem> {
    let service = server.cics_service();
    let session = SessionId::new(
        format!("carddemo-auth-session-{}", invocation.run_unit_id),
        128,
    )
    .map_err(|_| CorpusProblem::new("carddemo.authorization.syncpoint", "session invalid"))?;
    service
        .create_session(&session, 24, 80)
        .map_err(terminal_problem)?;
    service
        .register_run(invocation.clone(), &session, "AUTH", "MEAPPL", "MESYS")
        .map_err(terminal_problem)?;
    let idempotency_key = IdempotencyKey::new(
        format!("carddemo-auth-syncpoint-{sequence}"),
        InvocationLimits::default(),
    )
    .map_err(|_| CorpusProblem::new("carddemo.authorization.syncpoint", "key invalid"))?;
    let request = CicsRequest {
        operation: CicsOperation::Syncpoint,
        arguments: if rollback {
            BTreeMap::from([(
                "OPTION.ROLLBACK".into(),
                BoundedPayload::new(
                    "mainframe-env.cics.argument@1",
                    Vec::new(),
                    InvocationLimits::default(),
                )
                .expect("bounded rollback option"),
            )])
        } else {
            BTreeMap::new()
        },
        condition_policy: CicsConditionPolicy::Default,
        mutation: Some(Mutation {
            sequence,
            idempotency_key: idempotency_key.clone(),
            transaction: Some("AUTH".into()),
        }),
    };
    let response = service
        .invoke(
            &EffectRequest {
                run_unit: invocation.run_unit_id.clone(),
                sequence,
                deadline_tick: invocation.deadline_tick,
                idempotency_key: Some(idempotency_key),
                request: mainframe_env_host_api::HostRequest::Cics(request.clone()),
            },
            request,
        )
        .map_err(terminal_problem)?;
    let expected = if rollback {
        mainframe_env_host_api::CicsUnitOfWorkOutcome::RolledBack
    } else {
        mainframe_env_host_api::CicsUnitOfWorkOutcome::Committed
    };
    if response.unit_of_work == Some(expected) {
        Ok(())
    } else {
        Err(CorpusProblem::new(
            "carddemo.authorization.syncpoint",
            "CICS did not return the requested unit-of-work outcome",
        ))
    }
}

fn authorization_db2_request(
    operation: Db2Operation,
    sequence: u64,
    statement: &str,
    inputs: BTreeMap<String, Db2HostVariable>,
) -> Result<Db2Request, CorpusProblem> {
    let mutation = operation.is_mutating().then(|| {
        IdempotencyKey::new(
            format!("carddemo-auth-db2-{operation:?}-{sequence}"),
            InvocationLimits::default(),
        )
        .map(|idempotency_key| Mutation {
            sequence,
            idempotency_key,
            transaction: Some("CD-026".into()),
        })
        .map_err(|_| CorpusProblem::new("carddemo.authorization.db2", "mutation invalid"))
    });
    Ok(Db2Request {
        operation,
        statement: statement.into(),
        cursor: None,
        inputs,
        outputs: Vec::new(),
        max_rows: 64,
        mutation: mutation.transpose()?,
    })
}

fn mq_queue_digests(mq: &MqService) -> Result<BTreeMap<String, String>, CorpusProblem> {
    let mut output = BTreeMap::new();
    for queue in [
        "CARD.DEMO.REQUEST.DATE",
        "CARD.DEMO.REPLY.DATE",
        "CARD.DEMO.REQUEST.ACCT",
        "CARD.DEMO.REPLY.ACCT",
        "CARD.DEMO.REQUEST.AUTH",
        "CARD.DEMO.REPLY.AUTH",
        "CARD.DEMO.ERROR",
    ] {
        let mut digest = Sha256::new();
        for message in mq.queue_messages(queue).map_err(terminal_problem)? {
            digest_field(&mut digest, &message);
        }
        output.insert(queue.into(), format!("{:x}", digest.finalize()));
    }
    Ok(output)
}

struct ImsExercise {
    databases_installed: usize,
    psbs_installed: usize,
    pcbs_installed: usize,
    roots: usize,
    children: usize,
    secondary_index_entries: usize,
    hierarchy_sha256: String,
    selected_job_routes: usize,
    spool_sha256: BTreeMap<String, String>,
}

async fn exercise_ims_routes(
    corpus_dir: &Path,
    definition: ImsApplicationDefinition,
) -> Result<ImsExercise, CorpusProblem> {
    let artifact_root =
        env::temp_dir().join(format!("mainframe-env-carddemo-ims-{}", std::process::id()));
    let config = ServerConfig {
        store_profile: StoreProfile::Memory,
        artifact_root: artifact_root.clone(),
        tls: TlsConfig {
            enabled: false,
            certificate_path: None,
            private_key_reference: None,
        },
        ..ServerConfig::default()
    };
    let store = Arc::new(MemoryStore::new(Default::default()));
    let server = ProductServer::open_with_package_trust(
        config.clone(),
        store.clone(),
        Arc::new(MemorySecretResolver::default()),
        default_program_router(),
        carddemo_package_trust()?,
    )
    .map_err(terminal_problem)?;
    install_carddemo_db2_package(&server, &[])?;
    let ims = server.ims_service();
    let install = ims.install(definition.clone()).map_err(terminal_problem)?;
    if !ims
        .install(definition.clone())
        .map_err(terminal_problem)?
        .replayed
    {
        return Err(CorpusProblem::new(
            "carddemo.ims.install_replay",
            "IMS definition replay was not idempotent",
        ));
    }
    let root_one = ims_record(100, b"000001", b"ROOT-ONE")?;
    let root_two = ims_record(100, b"000002", b"ROOT-TWO")?;
    let child_one = ims_record(200, b"99900001", b"CHILD-ONE")?;
    let child_two = ims_record(200, b"99900002", b"CHILD-TWO")?;
    let image = ImsLoadImage {
        database: "DBPAUTP0".into(),
        roots: vec![
            ImsLoadRoot {
                data: root_one.clone(),
                children: vec![child_one.clone()],
            },
            ImsLoadRoot {
                data: root_two.clone(),
                children: vec![child_two.clone()],
            },
        ],
    };
    server
        .bootstrap_user("IBMUSER", b"TESTPASS")
        .map_err(terminal_problem)?;
    let racf = server.racf_service();
    racf.define_profile("DATASET", "AWS.M2.CARDDEMO.**", "IBMUSER", None)
        .map_err(terminal_problem)?;
    racf.permit(
        "DATASET",
        "AWS.M2.CARDDEMO.**",
        "IBMUSER",
        AccessIntent::Alter,
    )
    .map_err(terminal_problem)?;
    let mut sequence = 70_000u64;
    utility_seed_dataset(
        &server,
        "AWS.M2.CARDDEMO.PAUTDB.ROOT.FILEO",
        DatasetOrganization::Sequential,
        RecordFormat::Fixed,
        100,
        None,
        vec![root_one.clone(), root_two.clone()],
        &mut sequence,
    )?;
    let child_records = [
        (b"000001".as_slice(), &child_one),
        (b"000002".as_slice(), &child_two),
    ]
    .into_iter()
    .map(|(parent, child)| {
        let mut record = parent.to_vec();
        record.extend_from_slice(child);
        record
    })
    .collect();
    utility_seed_dataset(
        &server,
        "AWS.M2.CARDDEMO.PAUTDB.CHILD.FILEO",
        DatasetOrganization::Sequential,
        RecordFormat::Fixed,
        206,
        None,
        child_records,
        &mut sequence,
    )?;
    let app = server.router();
    let load_jcl = String::from_utf8(read_corpus_file(
        corpus_dir,
        &corpus_dir.join("app/app-authorization-ims-db2-mq/jcl/LOADPADB.JCL"),
    )?)
    .map_err(|_| CorpusProblem::new("carddemo.ims.jcl_invalid", "LOADPADB is not UTF-8"))?;
    let load_job = submit_job_with_retcode(&server, &app, &load_jcl, "CC 0000").await?;
    let control = ims_invocation("ims-control", true, None)?;
    ims.execute(
        &control,
        &ims_request(
            ImsOperation::Schedule,
            101,
            Some("PSBPAUTB"),
            &[],
            Vec::new(),
            Vec::new(),
            None,
        )?,
    )
    .map_err(terminal_problem)?;
    let root = ims
        .execute(
            &control,
            &ims_request(
                ImsOperation::GetUnique,
                2,
                None,
                &["PAUTSUM0"],
                Vec::new(),
                vec![ims_qualifier("PAUTSUM0", "ACCNTID", b"000001")],
                None,
            )?,
        )
        .map_err(terminal_problem)?;
    if root.status != "  "
        || root
            .segments
            .first()
            .is_none_or(|segment| segment.data != root_one)
    {
        return Err(CorpusProblem::new(
            "carddemo.ims.gu_drift",
            "GU did not return the qualified root segment",
        ));
    }
    let child = ims
        .execute(
            &control,
            &ims_request(
                ImsOperation::GetNextParent,
                4,
                None,
                &["PAUTDTL1"],
                Vec::new(),
                Vec::new(),
                None,
            )?,
        )
        .map_err(terminal_problem)?;
    if child
        .segments
        .first()
        .is_none_or(|segment| segment.data != child_one)
    {
        return Err(CorpusProblem::new(
            "carddemo.ims.gnp_drift",
            "GNP did not preserve root/child hierarchy",
        ));
    }
    let mut replaced_child = child_one.clone();
    replaced_child[8..12].copy_from_slice(b"EDIT");
    ims.execute(
        &control,
        &ims_request(
            ImsOperation::Replace,
            5,
            None,
            &["PAUTDTL1"],
            replaced_child,
            Vec::new(),
            None,
        )?,
    )
    .map_err(terminal_problem)?;
    let transient_child = ims_record(200, b"99900003", b"TRANSIENT")?;
    ims.execute(
        &control,
        &ims_request(
            ImsOperation::Insert,
            6,
            None,
            &["PAUTSUM0", "PAUTDTL1"],
            transient_child,
            vec![ims_qualifier("PAUTSUM0", "ACCNTID", b"000001")],
            None,
        )?,
    )
    .map_err(terminal_problem)?;
    ims.execute(
        &control,
        &ims_request(
            ImsOperation::GetUnique,
            7,
            None,
            &["PAUTSUM0", "PAUTDTL1"],
            Vec::new(),
            vec![
                ims_qualifier("PAUTSUM0", "ACCNTID", b"000001"),
                ims_qualifier("PAUTDTL1", "PAUT9CTS", b"99900003"),
            ],
            None,
        )?,
    )
    .map_err(terminal_problem)?;
    ims.execute(
        &control,
        &ims_request(
            ImsOperation::Delete,
            8,
            None,
            &["PAUTDTL1"],
            Vec::new(),
            Vec::new(),
            None,
        )?,
    )
    .map_err(terminal_problem)?;
    let next = ims
        .execute(
            &control,
            &ims_request(
                ImsOperation::GetNext,
                9,
                None,
                &["PAUTSUM0"],
                Vec::new(),
                Vec::new(),
                None,
            )?,
        )
        .map_err(terminal_problem)?;
    if next.status != "  " {
        return Err(CorpusProblem::new(
            "carddemo.ims.gn_drift",
            "GN did not return a root",
        ));
    }
    ims.execute(
        &control,
        &ims_request(
            ImsOperation::Checkpoint,
            10,
            None,
            &[],
            Vec::new(),
            Vec::new(),
            Some("CD025001"),
        )?,
    )
    .map_err(terminal_problem)?;
    ims.execute(
        &control,
        &ims_request(
            ImsOperation::Terminate,
            11,
            None,
            &[],
            Vec::new(),
            Vec::new(),
            None,
        )?,
    )
    .map_err(terminal_problem)?;
    let unload_jcl = String::from_utf8(read_corpus_file(
        corpus_dir,
        &corpus_dir.join("app/app-authorization-ims-db2-mq/jcl/UNLDPADB.JCL"),
    )?)
    .map_err(|_| CorpusProblem::new("carddemo.ims.jcl_invalid", "UNLDPADB is not UTF-8"))?;
    let unload_job = submit_job_with_retcode(&server, &app, &unload_jcl, "CC 0000").await?;
    let unloaded_roots = utility_records(&server, "AWS.M2.CARDDEMO.PAUTDB.ROOT.FILEO", None)?;
    let unloaded_children = utility_records(&server, "AWS.M2.CARDDEMO.PAUTDB.CHILD.FILEO", None)?;
    if unloaded_roots.len() != 2
        || unloaded_children.len() != 2
        || unloaded_roots.iter().any(|record| record.len() != 100)
        || unloaded_children.iter().any(|record| record.len() != 206)
    {
        return Err(CorpusProblem::new(
            "carddemo.ims.unload_drift",
            "IMS unload job did not write two roots and two qualified children",
        ));
    }

    let route_source = "IDENTIFICATION DIVISION. PROGRAM-ID. IMSROUTE. DATA DIVISION. WORKING-STORAGE SECTION. 01 PSB-NAME PIC X(8) VALUE 'PSBPAUTB'. 01 PCB-N PIC S9(4) COMP VALUE 1. 01 ROOT-X PIC X(100). 01 ACCT-X PIC X(6) VALUE '000001'. PROCEDURE DIVISION. EXEC DLI SCHD PSB((PSB-NAME)) END-EXEC. EXEC DLI GU USING PCB(PCB-N) SEGMENT(PAUTSUM0) INTO(ROOT-X) WHERE(ACCNTID = ACCT-X) END-EXEC. DISPLAY ROOT-X. EXEC DLI TERM END-EXEC. STOP RUN.";
    let artifact = crate::compile(route_source).map_err(|error| {
        CorpusProblem::new("carddemo.ims.route_compile", format!("IMS route: {error}"))
    })?;
    let invocation = ims_artifact_invocation("ims-route", &artifact, true, None)?;
    let host = Arc::new(ScopedHostService::new(
        Arc::new(
            RegistrySnapshot::new(
                1,
                ims_providers(ims.clone(), InvocationLimits::default()),
                InvocationLimits::default(),
            )
            .map_err(|_| CorpusProblem::new("carddemo.ims.registry", "registry invalid"))?,
        ),
        mainframe_env_host_api::HostLimits::default(),
    ));
    let mut machine = mainframe_env_interpreter::ReferenceMachine::from_binary(
        artifact.payload(),
        invocation.clone(),
        mainframe_env_ir::CodecLimits::default(),
    )
    .map_err(|problem| CorpusProblem::new("carddemo.ims.route", format!("{problem:?}")))?;
    let coordinator = mainframe_env_interpreter::ExecutionCoordinator::with_host(
        host.clone(),
        Arc::new(MemoryStore::new(Default::default())),
        mainframe_env_interpreter::CoordinatorLimits::default(),
    );
    let outcome = coordinator.execute(
        &mut machine,
        &invocation,
        mainframe_env_interpreter::ExecutionControl::default(),
    );
    if !matches!(
        outcome,
        mainframe_env_execution_api::ExecutionOutcome::Completed(ref completion)
            if completion.output.bytes().starts_with(b"000001")
    ) {
        return Err(CorpusProblem::new(
            "carddemo.ims.route_failed",
            format!("typed DLI application route did not complete: {outcome:?}"),
        ));
    }

    let denied_invocation = ims_invocation("ims-denied", false, None)?;
    let denied_request = ims_request(
        ImsOperation::Schedule,
        1,
        Some("PSBPAUTB"),
        &[],
        Vec::new(),
        Vec::new(),
        None,
    )?;
    let denied_effect = EffectRequest {
        run_unit: denied_invocation.run_unit_id.clone(),
        sequence: 1,
        deadline_tick: denied_invocation.deadline_tick,
        idempotency_key: denied_request
            .mutation
            .as_ref()
            .map(|mutation| mutation.idempotency_key.clone()),
        request: mainframe_env_host_api::HostRequest::Ims(denied_request),
    };
    if host
        .invoke(&denied_invocation, 1, false, denied_effect)
        .persist_with(|audit| {
            store
                .record_audit(audit)
                .map_err(|_| HostProblem::InfrastructureFailure)
        })
        .outcome
        != Err(HostProblem::Unauthorized)
    {
        return Err(CorpusProblem::new(
            "carddemo.ims.authorization_drift",
            "missing IMS grant did not fail closed",
        ));
    }
    let mismatch = ims_invocation(
        "ims-generation-mismatch",
        true,
        Some(("host.ims.write", "wrong")),
    )?;
    let mismatch_request = ims_request(
        ImsOperation::Schedule,
        1,
        Some("PSBPAUTB"),
        &[],
        Vec::new(),
        Vec::new(),
        None,
    )?;
    let mismatch_effect = EffectRequest {
        run_unit: mismatch.run_unit_id.clone(),
        sequence: 1,
        deadline_tick: mismatch.deadline_tick,
        idempotency_key: mismatch_request
            .mutation
            .as_ref()
            .map(|mutation| mutation.idempotency_key.clone()),
        request: mainframe_env_host_api::HostRequest::Ims(mismatch_request),
    };
    if host
        .invoke(&mismatch, 1, false, mismatch_effect)
        .persist_with(|audit| {
            store
                .record_audit(audit)
                .map_err(|_| HostProblem::InfrastructureFailure)
        })
        .outcome
        != Err(HostProblem::ProviderFailure)
    {
        return Err(CorpusProblem::new(
            "carddemo.ims.provider_failure_drift",
            "provider generation mismatch did not fail closed",
        ));
    }

    let malformed = ims_invocation("ims-malformed", true, None)?;
    ims.execute(
        &malformed,
        &ims_request(
            ImsOperation::Schedule,
            1,
            Some("PSBPAUTB"),
            &[],
            Vec::new(),
            Vec::new(),
            None,
        )?,
    )
    .map_err(terminal_problem)?;
    let malformed_result = ims.execute(
        &malformed,
        &ims_request(
            ImsOperation::Insert,
            102,
            None,
            &["PAUTSUM0"],
            vec![0],
            Vec::new(),
            None,
        )?,
    );
    if malformed_result != Err(HostProblem::Malformed) {
        return Err(CorpusProblem::new(
            "carddemo.ims.malformed_drift",
            format!("malformed segment length returned {malformed_result:?}"),
        ));
    }

    let limited_store = Arc::new(MemoryStore::new(Default::default()));
    let limited = ImsService::open(
        limited_store,
        ImsLimits {
            max_roots: 1,
            ..ImsLimits::default()
        },
    )
    .map_err(terminal_problem)?;
    limited.install(definition).map_err(terminal_problem)?;
    let limited_result = limited.execute(
        &ims_invocation("ims-limit", true, None)?,
        &ims_request(
            ImsOperation::Load,
            1,
            None,
            &[],
            serde_json::to_vec(&image)
                .map_err(|error| CorpusProblem::new("carddemo.ims.load", error.to_string()))?,
            Vec::new(),
            None,
        )?,
    );
    if limited_result != Err(HostProblem::ResourceExhausted) {
        return Err(CorpusProblem::new(
            "carddemo.ims.resource_drift",
            "root limit did not fail closed",
        ));
    }

    let hierarchy = ims.hierarchy("DBPAUTP0").map_err(terminal_problem)?;
    let hierarchy_sha256 = ims_hierarchy_digest(&hierarchy);
    let roots = hierarchy.len();
    let children = hierarchy.iter().map(|root| root.children.len()).sum();
    let secondary_index_entries = ims
        .secondary_index_entries("DBPAUTP0")
        .map_err(terminal_problem)?;
    let spool_sha256 = base_batch_spool_digests(
        &server,
        &BTreeMap::from([
            ("LOADPADB".into(), load_job),
            ("UNLDPADB".into(), unload_job),
        ]),
    )?;
    if !server.graceful_shutdown().await {
        return Err(CorpusProblem::new(
            "carddemo.ims.shutdown_failed",
            "IMS server did not shut down",
        ));
    }
    drop(ims);
    drop(server);
    let restarted = ProductServer::open_with_package_trust(
        config,
        store,
        Arc::new(MemorySecretResolver::default()),
        default_program_router(),
        carddemo_package_trust()?,
    )
    .map_err(terminal_problem)?;
    if ims_hierarchy_digest(
        &restarted
            .ims_service()
            .hierarchy("DBPAUTP0")
            .map_err(terminal_problem)?,
    ) != hierarchy_sha256
        || restarted
            .ims_service()
            .checkpoint_count()
            .map_err(terminal_problem)?
            != 1
    {
        return Err(CorpusProblem::new(
            "carddemo.ims.restart_drift",
            "IMS hierarchy or checkpoint changed across restart",
        ));
    }
    let _ = restarted.graceful_shutdown().await;
    drop(restarted);
    let _ = fs::remove_dir_all(&artifact_root);
    Ok(ImsExercise {
        databases_installed: install.databases,
        psbs_installed: install.psbs,
        pcbs_installed: install.pcbs,
        roots,
        children,
        secondary_index_entries,
        hierarchy_sha256,
        selected_job_routes: 2,
        spool_sha256,
    })
}

fn carddemo_ims_definition(
    corpus_dir: &Path,
) -> Result<(ImsApplicationDefinition, usize), CorpusProblem> {
    let files = [
        "DBPAUTP0.dbd",
        "DBPAUTX0.dbd",
        "DLIGSAMP.PSB",
        "PADFLDBD.DBD",
        "PASFLDBD.DBD",
        "PAUTBUNL.PSB",
        "PSBPAUTB.psb",
        "PSBPAUTL.psb",
    ];
    let mut sources = BTreeMap::new();
    for file in files {
        let path = corpus_dir
            .join("app/app-authorization-ims-db2-mq/ims")
            .join(file);
        let source = String::from_utf8(read_corpus_file(corpus_dir, &path)?).map_err(|_| {
            CorpusProblem::new(
                "carddemo.ims.definition_invalid",
                format!("{file} is not UTF-8"),
            )
        })?;
        sources.insert(file, source.to_ascii_uppercase());
    }
    for (file, markers) in [
        (
            "DBPAUTP0.dbd",
            &["ACCESS=(HIDAM,VSAM)", "PAUTSUM0", "PAUTDTL1"][..],
        ),
        (
            "DBPAUTX0.dbd",
            &["ACCESS=(INDEX,VSAM,PROT)", "PAUTINDX"][..],
        ),
        (
            "PSBPAUTB.psb",
            &["DBDNAME=DBPAUTP0", "PROCOPT=AP", "PSBNAME=PSBPAUTB"][..],
        ),
        (
            "PSBPAUTL.psb",
            &["DBDNAME=DBPAUTP0", "PROCOPT=L", "PSBNAME=PSBPAUTL"][..],
        ),
        (
            "PAUTBUNL.PSB",
            &["DBDNAME=DBPAUTP0", "PROCOPT=GOTP", "PSBNAME=PAUTBUNL"][..],
        ),
    ] {
        if markers.iter().any(|marker| !sources[file].contains(marker)) {
            return Err(CorpusProblem::new(
                "carddemo.ims.definition_drift",
                format!("{file} no longer contains its pinned IMS shape"),
            ));
        }
    }
    let primary_segments = vec![
        ImsSegmentDefinition {
            name: "PAUTSUM0".into(),
            parent: None,
            length: 100,
            key_field: "ACCNTID".into(),
            key_offset: 0,
            key_length: 6,
        },
        ImsSegmentDefinition {
            name: "PAUTDTL1".into(),
            parent: Some("PAUTSUM0".into()),
            length: 200,
            key_field: "PAUT9CTS".into(),
            key_offset: 0,
            key_length: 8,
        },
    ];
    let databases = vec![
        ImsDatabaseDefinition {
            name: "DBPAUTP0".into(),
            access: "HIDAM".into(),
            secondary_index: Some("DBPAUTX0".into()),
            segments: primary_segments,
        },
        ImsDatabaseDefinition {
            name: "DBPAUTX0".into(),
            access: "INDEX".into(),
            secondary_index: None,
            segments: vec![ImsSegmentDefinition {
                name: "PAUTINDX".into(),
                parent: None,
                length: 6,
                key_field: "INDXSEQ".into(),
                key_offset: 0,
                key_length: 6,
            }],
        },
    ];
    let psbs = [("PSBPAUTB", "AP"), ("PSBPAUTL", "L"), ("PAUTBUNL", "GOTP")]
        .into_iter()
        .map(|(name, processing_options)| ImsPsbDefinition {
            name: name.into(),
            pcbs: vec![ImsPcbDefinition {
                name: format!("{name}-PCB"),
                database: "DBPAUTP0".into(),
                processing_options: processing_options.into(),
                segments: vec!["PAUTSUM0".into(), "PAUTDTL1".into()],
            }],
        })
        .collect();
    Ok((ImsApplicationDefinition { databases, psbs }, sources.len()))
}

fn ims_record(length: usize, key: &[u8], marker: &[u8]) -> Result<Vec<u8>, CorpusProblem> {
    if key.len() > length || key.len() + marker.len() > length {
        return Err(CorpusProblem::new(
            "carddemo.ims.fixture_invalid",
            "IMS fixture exceeds its segment length",
        ));
    }
    let mut record = vec![b' '; length];
    record[..key.len()].copy_from_slice(key);
    record[key.len()..key.len() + marker.len()].copy_from_slice(marker);
    Ok(record)
}

fn ims_qualifier(segment: &str, field: &str, value: &[u8]) -> ImsQualifier {
    ImsQualifier {
        segment: segment.into(),
        field: field.into(),
        value: value.to_vec(),
    }
}

fn ims_request(
    operation: ImsOperation,
    sequence: u64,
    psb: Option<&str>,
    segments: &[&str],
    data: Vec<u8>,
    qualifiers: Vec<ImsQualifier>,
    checkpoint_id: Option<&str>,
) -> Result<ImsRequest, CorpusProblem> {
    let mutation = operation.is_mutating().then(|| {
        IdempotencyKey::new(
            format!("carddemo-ims-{operation:?}-{sequence}"),
            InvocationLimits::default(),
        )
        .map(|idempotency_key| Mutation {
            sequence,
            idempotency_key,
            transaction: Some("CD-025".into()),
        })
        .map_err(|_| CorpusProblem::new("carddemo.ims.request", "mutation invalid"))
    });
    Ok(ImsRequest {
        operation,
        psb: psb.map(str::to_string),
        pcb: 1,
        segments: segments.iter().map(|value| (*value).into()).collect(),
        data,
        qualifiers,
        checkpoint_id: checkpoint_id.map(str::to_string),
        max_segments: 64,
        mutation: mutation.transpose()?,
        system: None,
        q_class: None,
    })
}

fn ims_invocation(
    run: &str,
    granted: bool,
    generation: Option<(&str, &str)>,
) -> Result<Invocation, CorpusProblem> {
    let limits = InvocationLimits::default();
    let grants = if granted {
        ["host.ims.read", "host.ims.write"]
            .into_iter()
            .map(|capability| CapabilityId::new(capability, limits))
            .collect::<Result<BTreeSet<_>, _>>()
            .map_err(|_| CorpusProblem::new("carddemo.ims.invocation", "grant invalid"))?
    } else {
        BTreeSet::new()
    };
    let invocation = Invocation::new(
        RequestId::new(format!("carddemo-ims-request-{run}"), limits)
            .map_err(|_| CorpusProblem::new("carddemo.ims.invocation", "request invalid"))?,
        ExecutionId::new(format!("carddemo-ims-execution-{run}"), limits)
            .map_err(|_| CorpusProblem::new("carddemo.ims.invocation", "execution invalid"))?,
        RunUnitId::new(run, limits)
            .map_err(|_| CorpusProblem::new("carddemo.ims.invocation", "run invalid"))?,
        None,
        Selector::new("ims:carddemo", limits)
            .map_err(|_| CorpusProblem::new("carddemo.ims.invocation", "selector invalid"))?,
        ArtifactRef::new("ims:carddemo", limits)
            .map_err(|_| CorpusProblem::new("carddemo.ims.invocation", "artifact invalid"))?,
        Principal::new(
            PrincipalId::new("IBMUSER", limits)
                .map_err(|_| CorpusProblem::new("carddemo.ims.invocation", "principal invalid"))?,
            grants,
            limits,
        )
        .map_err(|_| CorpusProblem::new("carddemo.ims.invocation", "principal invalid"))?,
        ServiceClass::Batch,
        0,
        100,
        TraceId::new(format!("carddemo-ims-trace-{run}"), limits)
            .map_err(|_| CorpusProblem::new("carddemo.ims.invocation", "trace invalid"))?,
        IdempotencyKey::new(format!("carddemo-ims-invocation-{run}"), limits)
            .map_err(|_| CorpusProblem::new("carddemo.ims.invocation", "key invalid"))?,
        1,
        ResourceLimits::default(),
        BTreeMap::new(),
        limits,
    )
    .map_err(|_| CorpusProblem::new("carddemo.ims.invocation", "invocation invalid"))?;
    if let Some((capability, generation)) = generation {
        invocation
            .with_provider_generations(
                BTreeMap::from([(
                    CapabilityId::new(capability, limits).expect("static capability"),
                    generation.into(),
                )]),
                limits,
            )
            .map_err(|_| CorpusProblem::new("carddemo.ims.invocation", "generation invalid"))
    } else {
        Ok(invocation)
    }
}

fn ims_artifact_invocation(
    run: &str,
    artifact: &mainframe_env_compiler_api::PublishedArtifact,
    granted: bool,
    generation: Option<(&str, &str)>,
) -> Result<Invocation, CorpusProblem> {
    let mut invocation = ims_invocation(run, granted, generation)?;
    invocation.artifact = ArtifactRef::new(
        artifact.content_id().to_reference(),
        InvocationLimits::default(),
    )
    .map_err(|_| CorpusProblem::new("carddemo.ims.invocation", "artifact invalid"))?;
    Ok(invocation)
}

fn ims_hierarchy_digest(hierarchy: &[ImsLoadRoot]) -> String {
    let mut digest = Sha256::new();
    for root in hierarchy {
        digest_field(&mut digest, &root.data);
        for child in &root.children {
            digest_field(&mut digest, child);
        }
    }
    format!("{:x}", digest.finalize())
}

fn db2_control_invocation(run: &str) -> Result<Invocation, CorpusProblem> {
    let limits = InvocationLimits::default();
    let grants = ["host.db2.read", "host.db2.write"]
        .into_iter()
        .map(|capability| CapabilityId::new(capability, limits))
        .collect::<Result<BTreeSet<_>, _>>()
        .map_err(|_| CorpusProblem::new("carddemo.db2.control_invalid", "Db2 grant is invalid"))?;
    Invocation::new(
        RequestId::new(format!("carddemo-db2-request-{run}"), limits)
            .map_err(|_| CorpusProblem::new("carddemo.db2.control_invalid", "request invalid"))?,
        ExecutionId::new(format!("carddemo-db2-execution-{run}"), limits)
            .map_err(|_| CorpusProblem::new("carddemo.db2.control_invalid", "execution invalid"))?,
        RunUnitId::new(run, limits)
            .map_err(|_| CorpusProblem::new("carddemo.db2.control_invalid", "run invalid"))?,
        None,
        Selector::new("program:DB2-CONTROL", limits).expect("static Db2 control selector"),
        ArtifactRef::new("db2-control", limits).expect("static Db2 control artifact"),
        Principal::new(
            PrincipalId::new("WEBADM", limits).expect("static Db2 principal"),
            grants,
            limits,
        )
        .expect("bounded Db2 principal"),
        ServiceClass::Interactive,
        0,
        1_000_000,
        TraceId::new(format!("carddemo-db2-trace-{run}"), limits).expect("bounded Db2 trace"),
        IdempotencyKey::new(format!("carddemo-db2-invocation-{run}"), limits)
            .expect("bounded Db2 invocation key"),
        1,
        ResourceLimits::default(),
        BTreeMap::new(),
        limits,
    )
    .map_err(|_| {
        CorpusProblem::new(
            "carddemo.db2.control_invalid",
            "Db2 control invocation is invalid",
        )
    })
}

fn db2_variable(value: &str) -> Db2HostVariable {
    Db2HostVariable {
        value: value.as_bytes().to_vec(),
        indicator: None,
    }
}

fn db2_varchar_variable(value: &str) -> Db2HostVariable {
    let mut bytes = u16::try_from(value.len())
        .expect("CardDemo VARCHAR control is bounded")
        .to_be_bytes()
        .to_vec();
    bytes.extend_from_slice(value.as_bytes());
    Db2HostVariable {
        value: bytes,
        indicator: None,
    }
}

fn db2_control_request(
    operation: Db2Operation,
    sequence: u64,
    inputs: BTreeMap<String, Db2HostVariable>,
) -> Result<Db2Request, CorpusProblem> {
    let statement = match operation {
        Db2Operation::Insert => "INSERT INTO CARDDEMO.TRANSACTION_TYPE",
        Db2Operation::Update => "UPDATE CARDDEMO.TRANSACTION_TYPE",
        Db2Operation::Delete => "DELETE FROM CARDDEMO.TRANSACTION_TYPE",
        Db2Operation::Commit => "COMMIT",
        Db2Operation::Rollback => "ROLLBACK",
        _ => "CONTROL",
    };
    let mutation = operation.is_mutating().then(|| {
        IdempotencyKey::new(
            format!("carddemo-db2-control-{sequence}"),
            InvocationLimits::default(),
        )
        .map(|idempotency_key| Mutation {
            sequence,
            idempotency_key,
            transaction: Some("CD-024-CONTROL".into()),
        })
        .map_err(|_| {
            CorpusProblem::new(
                "carddemo.db2.control_invalid",
                "Db2 control mutation is invalid",
            )
        })
    });
    Ok(Db2Request {
        operation,
        statement: statement.into(),
        cursor: None,
        inputs,
        outputs: Vec::new(),
        max_rows: 64,
        mutation: mutation.transpose()?,
    })
}

fn db2_table_digests(server: &ProductServer) -> Result<BTreeMap<String, String>, CorpusProblem> {
    let mut output = BTreeMap::new();
    for table in [
        "CARDDEMO.TRANSACTION_TYPE",
        "CARDDEMO.TRANSACTION_TYPE_CATEGORY",
    ] {
        let rows = server
            .db2_service()
            .table_rows(table)
            .map_err(terminal_problem)?;
        let mut digest = Sha256::new();
        for row in rows {
            for column in row {
                digest_field(&mut digest, &column);
            }
        }
        output.insert(table.into(), format!("{:x}", digest.finalize()));
    }
    Ok(output)
}

fn db2_dataset_digests(server: &ProductServer) -> Result<BTreeMap<String, String>, CorpusProblem> {
    let mut output = BTreeMap::new();
    for dataset in ["AWS.M2.CARDDEMO.TRANTYPE.PS", "AWS.M2.CARDDEMO.TRANCATG.PS"] {
        let records = utility_records(server, dataset, None)?;
        let mut digest = Sha256::new();
        for record in records {
            digest_field(&mut digest, &record);
        }
        output.insert(dataset.into(), format!("{:x}", digest.finalize()));
    }
    Ok(output)
}

async fn exercise_base_batch_routes(
    corpus_dir: &Path,
    corpus_commit: String,
    online: OnlineApplicationDefinition,
    definitions: Vec<BatchProgramDefinition>,
) -> Result<CardDemoBaseBatchReceipt, CorpusProblem> {
    const BUSINESS_DATE: &str = "2022-07-06";
    const COBOL_CURRENT_DATE: &str = "2022070600000000+0000";
    let artifact_root = env::temp_dir().join(format!(
        "mainframe-env-carddemo-base-batch-{}",
        std::process::id()
    ));
    let config = ServerConfig {
        store_profile: StoreProfile::Memory,
        cobol_current_date: Some(COBOL_CURRENT_DATE.into()),
        artifact_root: artifact_root.clone(),
        tls: TlsConfig {
            enabled: false,
            certificate_path: None,
            private_key_reference: None,
        },
        ..ServerConfig::default()
    };
    let store = Arc::new(MemoryStore::new(Default::default()));
    let secrets = Arc::new(MemorySecretResolver::default());
    let server = ProductServer::open(
        config.clone(),
        store.clone(),
        secrets.clone(),
        default_program_router(),
    )
    .map_err(terminal_problem)?;
    server
        .bootstrap_user("IBMUSER", b"TESTPASS")
        .map_err(terminal_problem)?;
    install_base_online_authorities(&server, corpus_dir, &online)?;
    server
        .install_online_application(online)
        .map_err(terminal_problem)?;
    server
        .install_batch_programs(definitions)
        .map_err(terminal_problem)?;
    let control_invocation = base_batch_control_invocation()?;
    let racf = server.racf_service();
    for object in carddemo_base_seed_objects(corpus_dir)? {
        racf.permit(
            "DATASET",
            object.dataset.as_str(),
            "IBMUSER",
            AccessIntent::Alter,
        )
        .map_err(terminal_problem)?;
    }
    let csd = String::from_utf8(read_corpus_file(
        corpus_dir,
        &corpus_dir.join("app/csd/CARDDEMO.CSD"),
    )?)
    .map_err(|_| CorpusProblem::new("carddemo.base_batch.csd_invalid", "CSD is not UTF-8"))?;
    for dataset in parse_csd(&csd)
        .map_err(package_problem)?
        .into_iter()
        .filter(|resource| resource.kind == "FILE")
        .filter_map(|resource| resource.properties.get("DSNAME").cloned())
    {
        racf.permit("DATASET", &dataset, "IBMUSER", AccessIntent::Alter)
            .map_err(terminal_problem)?;
    }
    racf.define_profile("DATASET", "AWS.M2.CARDDEMO.**", "IBMUSER", None)
        .map_err(terminal_problem)?;
    racf.permit(
        "DATASET",
        "AWS.M2.CARDDEMO.**",
        "IBMUSER",
        AccessIntent::Alter,
    )
    .map_err(terminal_problem)?;
    racf.define_profile("DATASET", "AWS.M2.CARDEMO.**", "IBMUSER", None)
        .map_err(terminal_problem)?;
    racf.permit(
        "DATASET",
        "AWS.M2.CARDEMO.**",
        "IBMUSER",
        AccessIntent::Alter,
    )
    .map_err(terminal_problem)?;
    let app = server.router();
    install_base_batch_source_datasets(&server, corpus_dir)?;
    let mut job_ids = BTreeMap::new();
    for relative in [
        "app/jcl/ACCTFILE.jcl",
        "app/jcl/CARDFILE.jcl",
        "app/jcl/CUSTFILE.jcl",
        "app/jcl/XREFFILE.jcl",
        "app/jcl/TCATBALF.jcl",
        "app/jcl/TRANCATG.jcl",
        "app/jcl/TRANTYPE.jcl",
        "app/jcl/DISCGRP.jcl",
        "app/jcl/DUSRSECJ.jcl",
        "app/jcl/TRANFILE.jcl",
    ] {
        let jcl = String::from_utf8(read_corpus_file(corpus_dir, &corpus_dir.join(relative))?)
            .map_err(|_| {
                CorpusProblem::new(
                    "carddemo.base_batch.jcl_invalid",
                    format!("{relative} is not UTF-8"),
                )
            })?;
        let id = submit_job_with_retcode(&server, &app, &jcl, "CC 0000")
            .await
            .map_err(|problem| {
                CorpusProblem::new(
                    "carddemo.base_batch.job_failed",
                    format!("{relative}: {problem}"),
                )
            })?;
        job_ids.insert(base_batch_job_label(relative)?, id);
    }
    for relative in ["app/jcl/DEFGDGB.jcl", "app/jcl/DALYREJS.jcl"] {
        let jcl = String::from_utf8(read_corpus_file(corpus_dir, &corpus_dir.join(relative))?)
            .map_err(|_| {
                CorpusProblem::new(
                    "carddemo.base_batch.jcl_invalid",
                    format!("{relative} is not UTF-8"),
                )
            })?;
        let id = submit_job_with_retcode(&server, &app, &jcl, "CC 0000")
            .await
            .map_err(|problem| {
                CorpusProblem::new(
                    "carddemo.base_batch.job_failed",
                    format!("{relative}: {problem}"),
                )
            })?;
        job_ids.insert(base_batch_job_label(relative)?, id);
    }
    let close_jcl = String::from_utf8(read_corpus_file(
        corpus_dir,
        &corpus_dir.join("app/jcl/CLOSEFIL.jcl"),
    )?)
    .map_err(|_| {
        CorpusProblem::new(
            "carddemo.base_batch.jcl_invalid",
            "app/jcl/CLOSEFIL.jcl is not UTF-8",
        )
    })?;
    let close_id = submit_job_with_retcode(&server, &app, &close_jcl, "CC 0000").await?;
    job_ids.insert("CLOSEFIL".into(), close_id.clone());
    for file in ["TRANSACT", "CCXREF", "ACCTDAT", "CXACAIX", "USRSEC"] {
        if server
            .cics_service()
            .file_status(file)
            .map_err(terminal_problem)?
            != CicsFileStatus::Closed
        {
            return Err(CorpusProblem::new(
                "carddemo.base_batch.cics_close_drift",
                format!("{file} was not closed by CLOSEFIL"),
            ));
        }
    }
    if server
        .batch_service()
        .spool(&control_invocation, &close_id, "CMDOUT", 0, 16)
        .map_err(terminal_problem)?
        .0
        != [
            "TRANSACT CLOSED",
            "CCXREF CLOSED",
            "ACCTDAT CLOSED",
            "CXACAIX CLOSED",
            "USRSEC CLOSED",
        ]
        .map(|record| record.as_bytes().to_vec())
    {
        return Err(CorpusProblem::new(
            "carddemo.base_batch.cics_close_drift",
            "CLOSEFIL command output differs",
        ));
    }
    for (relative, expected) in [
        ("app/jcl/POSTTRAN.jcl", "CC 0004"),
        ("app/jcl/INTCALC.jcl", "CC 0000"),
        ("app/jcl/TRANBKP.jcl", "CC 0000"),
        ("app/jcl/COMBTRAN.jcl", "CC 0000"),
        ("app/jcl/TRANIDX.jcl", "CC 0000"),
        ("app/jcl/TRANREPT.jcl", "CC 0000"),
        ("app/jcl/PRTCATBL.jcl", "CC 0000"),
    ] {
        let jcl = String::from_utf8(read_corpus_file(corpus_dir, &corpus_dir.join(relative))?)
            .map_err(|_| {
                CorpusProblem::new(
                    "carddemo.base_batch.jcl_invalid",
                    format!("{relative} is not UTF-8"),
                )
            })?;
        let id = submit_job_with_retcode(&server, &app, &jcl, expected)
            .await
            .map_err(|problem| {
                CorpusProblem::new(
                    "carddemo.base_batch.job_failed",
                    format!("{relative}: {problem}"),
                )
            })?;
        job_ids.insert(base_batch_job_label(relative)?, id);
        if relative == "app/jcl/TRANBKP.jcl" {
            utility_records(&server, "AWS.M2.CARDDEMO.TRANSACT.VSAM.KSDS", None).map_err(
                |problem| {
                    CorpusProblem::new(
                        "carddemo.base_batch.backup_drift",
                        format!("TRANBKP did not recreate the transaction master: {problem}"),
                    )
                },
            )?;
        }
    }
    if let Ok(directory) = env::var("CARDDEMO_TRANREPT_EXTRACT_DIR") {
        let directory = Path::new(&directory);
        if !directory.is_absolute() {
            return Err(CorpusProblem::new(
                "carddemo.base_batch.extract_path",
                "CARDDEMO_TRANREPT_EXTRACT_DIR must be absolute",
            ));
        }
        fs::create_dir_all(directory).map_err(|error| {
            CorpusProblem::new("carddemo.base_batch.extract", error.to_string())
        })?;
        for (dataset, filename) in [
            (
                "AWS.M2.CARDDEMO.TRANSACT.BKUP.G0002V00",
                "tranrept-input.bin",
            ),
            (
                "AWS.M2.CARDDEMO.TRANSACT.DALY.G0001V00",
                "tranrept-selected.bin",
            ),
            ("AWS.M2.CARDDEMO.TRANREPT.G0001V00", "tranrept-report.bin"),
        ] {
            let records = utility_records(&server, dataset, None)?;
            let bytes = records.concat();
            fs::write(directory.join(filename), bytes).map_err(|error| {
                CorpusProblem::new("carddemo.base_batch.extract", error.to_string())
            })?;
            let name = DatasetName::new(dataset, 128).map_err(|_| {
                CorpusProblem::new("carddemo.base_batch.extract", "dataset name is invalid")
            })?;
            let DatasetResult::Attributes {
                attributes,
                version,
            } = server
                .dataset_service()
                .invoke(DatasetRequest::Attributes {
                    dataset: name.clone(),
                })
                .map_err(terminal_problem)?
            else {
                return Err(CorpusProblem::new(
                    "carddemo.base_batch.extract",
                    "attributes unavailable",
                ));
            };
            let DatasetResult::Records {
                version: read_version,
                identities,
                ..
            } = server
                .dataset_service()
                .invoke(DatasetRequest::Read {
                    dataset: name,
                    member: None,
                    key: None,
                    max_records: 4_096,
                    control: Default::default(),
                })
                .map_err(terminal_problem)?
            else {
                return Err(CorpusProblem::new(
                    "carddemo.base_batch.extract",
                    "records unavailable",
                ));
            };
            fs::write(
                directory.join(format!("{filename}.meta")),
                format!(
                    "attributes={:?}\nversion={version}\nread_version={read_version}\nidentities={} first={:?}\n",
                    attributes,
                    identities.len(),
                    identities.first()
                ),
            )
            .map_err(|error| CorpusProblem::new("carddemo.base_batch.extract", error.to_string()))?;
        }
    }
    let tranrept_selected_records =
        utility_records(&server, "AWS.M2.CARDDEMO.TRANSACT.DALY.G0001V00", None)?.len();
    let tranrept_report_records =
        utility_records(&server, "AWS.M2.CARDDEMO.TRANREPT.G0001V00", None)?.len();
    if tranrept_selected_records == 0 || tranrept_report_records == 0 {
        return Err(CorpusProblem::new(
            "carddemo.base_batch.tranrept_empty",
            format!(
                "TRANREPT selected {tranrept_selected_records} records and wrote {tranrept_report_records} report records"
            ),
        ));
    }
    let post_id = job_ids
        .get("POSTTRAN")
        .ok_or_else(|| CorpusProblem::new("carddemo.base_batch.job_missing", "POSTTRAN missing"))?;
    let post_text = base_batch_spool_text(&server, post_id)?;
    let compact_post = post_text
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect::<String>();
    if !compact_post.contains("TRANSACTIONSPROCESSED:000000300")
        || !compact_post.contains("TRANSACTIONSREJECTED:000000038")
    {
        return Err(CorpusProblem::new(
            "carddemo.base_batch.posting_drift",
            "POSTTRAN did not report 300 processed and 38 rejected transactions",
        ));
    }
    let statement = corrected_creastmt(corpus_dir)?;
    let statement_id = submit_job_with_retcode(&server, &app, &statement, "CC 0000")
        .await
        .map_err(|problem| {
            CorpusProblem::new(
                "carddemo.base_batch.job_failed",
                format!("app/jcl/CREASTMT.JCL: {problem}"),
            )
        })?;
    job_ids.insert("CREASTMT".into(), statement_id);
    for name in [
        "AWS.M2.CARDDEMO.TRXFL.SEQ",
        "AWS.M2.CARDDEMO.TRXFL.VSAM.KSDS",
        "AWS.M2.CARDDEMO.STATEMNT.PS",
        "AWS.M2.CARDDEMO.STATEMNT.HTML",
        "AWS.M2.CARDDEMO.TCATBALF.REPT",
    ] {
        if utility_records(&server, name, None)?.is_empty() {
            return Err(CorpusProblem::new(
                "carddemo.base_batch.output_empty",
                format!("{name} is empty"),
            ));
        }
    }

    let internal_jcl = String::from_utf8(read_corpus_file(
        corpus_dir,
        &corpus_dir.join("app/jcl/INTRDRJ1.JCL"),
    )?)
    .map_err(|_| {
        CorpusProblem::new(
            "carddemo.base_batch.jcl_invalid",
            "app/jcl/INTRDRJ1.JCL is not UTF-8",
        )
    })?;
    let internal_id = submit_job_with_retcode(&server, &app, &internal_jcl, "CC 0000").await?;
    job_ids.insert("INTRDRJ1".into(), internal_id);
    let principal =
        PrincipalId::new("IBMUSER", InvocationLimits::default()).expect("static batch principal");
    let (_, more) = server
        .batch_service()
        .list(Some(&principal), None, 128)
        .map_err(terminal_problem)?;
    if more {
        return Err(CorpusProblem::new(
            "carddemo.base_batch.job_limit",
            "job list exceeded the base-cycle observation bound",
        ));
    }
    let child = wait_for_listed_job(
        &server,
        &principal,
        "INTRDRJ2",
        128,
        "carddemo.base_batch.internal_missing",
        "carddemo.base_batch.internal_incomplete",
    )
    .await?;
    if child.state != JobState::Completed || child.return_code != Some(0) {
        return Err(CorpusProblem::new(
            "carddemo.base_batch.internal_incomplete",
            format!("INTRDRJ2 did not complete: {child:?}"),
        ));
    }
    job_ids.insert("INTRDRJ2".into(), child.id);
    if utility_records(&server, "AWS.M2.CARDEMO.FTP.TEST.BKUP.INTRDR", None)?
        != utility_records(&server, "AWS.M2.CARDEMO.FTP.TEST.BKUP", None)?
    {
        return Err(CorpusProblem::new(
            "carddemo.base_batch.internal_copy_drift",
            "INTRDRJ2 did not reproduce the FTP backup exactly",
        ));
    }

    let open_jcl = String::from_utf8(read_corpus_file(
        corpus_dir,
        &corpus_dir.join("app/jcl/OPENFIL.jcl"),
    )?)
    .map_err(|_| {
        CorpusProblem::new(
            "carddemo.base_batch.jcl_invalid",
            "app/jcl/OPENFIL.jcl is not UTF-8",
        )
    })?;
    let open_id = submit_job_with_retcode(&server, &app, &open_jcl, "CC 0000").await?;
    job_ids.insert("OPENFIL".into(), open_id.clone());
    for file in ["TRANSACT", "CCXREF", "ACCTDAT", "CXACAIX", "USRSEC"] {
        if server
            .cics_service()
            .file_status(file)
            .map_err(terminal_problem)?
            != CicsFileStatus::Open
        {
            return Err(CorpusProblem::new(
                "carddemo.base_batch.cics_open_drift",
                format!("{file} was not opened by OPENFIL"),
            ));
        }
    }
    if server
        .batch_service()
        .spool(&control_invocation, &open_id, "CMDOUT", 0, 16)
        .map_err(terminal_problem)?
        .0
        != [
            "TRANSACT OPEN",
            "CCXREF OPEN",
            "ACCTDAT OPEN",
            "CXACAIX OPEN",
            "USRSEC OPEN",
        ]
        .map(|record| record.as_bytes().to_vec())
    {
        return Err(CorpusProblem::new(
            "carddemo.base_batch.cics_open_drift",
            "OPENFIL command output differs",
        ));
    }

    let rollback_jcl = "//CD23ROLL JOB CLASS=A\n//FAIL EXEC PGM=NOTREAL\n//WORK DD DSN=AWS.M2.CARDDEMO.CD23.ROLLBACK,DISP=(NEW,KEEP,DELETE),\n// UNIT=SYSDA,DCB=(LRECL=80,RECFM=FB)\n";
    server
        .start_background_workers()
        .map_err(terminal_problem)?;
    let rollback_headers = base_batch_job_headers();
    let (rollback_status, rollback_body) = terminal_http(
        &app,
        Method::PUT,
        "/zosmf/restjobs/jobs",
        rollback_headers.clone(),
        rollback_jcl.as_bytes().to_vec(),
    )
    .await?;
    let rollback_job: serde_json::Value = serde_json::from_slice(&rollback_body)
        .map_err(|error| CorpusProblem::new("carddemo.base_batch.rollback", error.to_string()))?;
    let rollback_job = if rollback_status == StatusCode::CREATED {
        wait_for_submitted_job(&server, &app, &rollback_headers, rollback_job).await?
    } else {
        rollback_job
    };
    if rollback_status != StatusCode::CREATED
        || rollback_job["status"] != "OUTPUT"
        || !rollback_job["retcode"].is_null()
        || server.dataset_service().invoke(DatasetRequest::Attributes {
            dataset: DatasetName::new(
                "AWS.M2.CARDDEMO.CD23.ROLLBACK",
                InvocationLimits::default().max_binding_bytes,
            )
            .expect("static rollback dataset"),
        }) != Err(HostProblem::NotFound)
    {
        return Err(CorpusProblem::new(
            "carddemo.base_batch.rollback",
            format!("abnormal allocation was not rolled back: {rollback_job}"),
        ));
    }
    job_ids.insert(
        "ROLLBACK-CONTROL".into(),
        rollback_job["jobid"]
            .as_str()
            .ok_or_else(|| {
                CorpusProblem::new("carddemo.base_batch.rollback", "rollback job ID is missing")
            })?
            .to_string(),
    );

    let cancelled = server
        .batch_service()
        .submit(
            &control_invocation,
            &JclBundle {
                primary: "//CD23CANC JOB CLASS=A\n//WAIT EXEC PGM=IEFBR14\n".into(),
                ..Default::default()
            },
            &IdempotencyKey::new("carddemo-base-batch-cancel", InvocationLimits::default())
                .expect("static cancellation idempotency key"),
            true,
        )
        .map_err(terminal_problem)?;
    let (cancel_status, _) = terminal_http(
        &app,
        Method::PUT,
        &format!("/zosmf/restjobs/jobs/{}/{}", cancelled.name, cancelled.id),
        base_batch_job_headers(),
        Vec::new(),
    )
    .await?;
    if cancel_status != StatusCode::NO_CONTENT
        || server
            .batch_service()
            .get(&cancelled.id)
            .map_err(terminal_problem)?
            .state
            != JobState::Cancelled
    {
        return Err(CorpusProblem::new(
            "carddemo.base_batch.cancellation",
            "held job did not cancel through the public z/OSMF route",
        ));
    }
    job_ids.insert("CANCEL-CONTROL".into(), cancelled.id);

    let dataset_sha256 = base_batch_dataset_digests(&server)?;
    let spool_sha256 = base_batch_spool_digests(&server, &job_ids)?;
    drop(app);
    if !server.graceful_shutdown().await {
        return Err(CorpusProblem::new(
            "carddemo.base_batch.shutdown_failed",
            "base batch server did not shut down",
        ));
    }
    drop(racf);
    drop(server);
    let restarted = ProductServer::open(config, store, secrets, default_program_router())
        .map_err(terminal_problem)?;
    if base_batch_dataset_digests(&restarted)? != dataset_sha256
        || base_batch_spool_digests(&restarted, &job_ids)? != spool_sha256
        || ["TRANSACT", "CCXREF", "ACCTDAT", "CXACAIX", "USRSEC"]
            .into_iter()
            .any(|file| restarted.cics_service().file_status(file) != Ok(CicsFileStatus::Open))
    {
        return Err(CorpusProblem::new(
            "carddemo.base_batch.restart_drift",
            "dataset, spool, or CICS state changed across warm restart",
        ));
    }
    if !restarted.graceful_shutdown().await {
        return Err(CorpusProblem::new(
            "carddemo.base_batch.shutdown_failed",
            "restarted base batch server did not shut down",
        ));
    }
    drop(restarted);

    let journeys_passed = 3usize;
    let initialization_jobs = 12usize;
    let operational_jobs = 9usize;
    let cics_file_controls = 2usize;
    let internal_submissions = 1usize;
    let warm_restart_controls = 1usize;
    let rollback_controls = 1usize;
    let cancellation_controls = 1usize;
    let mut shape = Sha256::new();
    digest_field(&mut shape, corpus_commit.as_bytes());
    digest_field(&mut shape, BUSINESS_DATE.as_bytes());
    for value in [
        journeys_passed,
        initialization_jobs,
        operational_jobs,
        cics_file_controls,
        internal_submissions,
        warm_restart_controls,
        rollback_controls,
        cancellation_controls,
        tranrept_selected_records,
        tranrept_report_records,
    ] {
        digest_field(&mut shape, &(value as u64).to_be_bytes());
    }
    for (name, digest) in &dataset_sha256 {
        digest_field(&mut shape, name.as_bytes());
        digest_field(&mut shape, digest.as_bytes());
    }
    for (name, digest) in &spool_sha256 {
        digest_field(&mut shape, name.as_bytes());
        digest_field(&mut shape, digest.as_bytes());
    }
    let _ = fs::remove_dir_all(&artifact_root);
    Ok(CardDemoBaseBatchReceipt {
        schema_version: "mainframe-env.carddemo-base-batch-receipt@1".into(),
        status: "pass".into(),
        corpus_commit,
        business_date: BUSINESS_DATE.into(),
        journeys_passed,
        initialization_jobs,
        operational_jobs,
        cics_file_controls,
        internal_submissions,
        warm_restart_controls,
        rollback_controls,
        cancellation_controls,
        tranrept_selected_records,
        tranrept_report_records,
        dataset_sha256,
        spool_sha256,
        journey_shape_sha256: format!("{:x}", shape.finalize()),
    })
}

fn base_batch_job_label(relative: &str) -> Result<String, CorpusProblem> {
    Path::new(relative)
        .file_stem()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_uppercase)
        .ok_or_else(|| {
            CorpusProblem::new(
                "carddemo.base_batch.path_invalid",
                format!("{relative} has no job label"),
            )
        })
}

fn base_batch_job_headers() -> BTreeMap<String, String> {
    BTreeMap::from([
        (
            "authorization".into(),
            format!(
                "Basic {}",
                base64::engine::general_purpose::STANDARD.encode("IBMUSER:TESTPASS")
            ),
        ),
        ("x-csrf-zosmf-header".into(), "true".into()),
    ])
}

fn base_batch_control_invocation() -> Result<Invocation, CorpusProblem> {
    let limits = InvocationLimits::default();
    let grants = [
        "host.security.authorize",
        "host.program.invoke",
        "host.spool.read",
        "host.spool.write",
    ]
    .into_iter()
    .map(|capability| {
        CapabilityId::new(capability, limits).map_err(|_| {
            CorpusProblem::new(
                "carddemo.base_batch.control_invalid",
                "control capability is invalid",
            )
        })
    })
    .collect::<Result<BTreeSet<_>, _>>()?;
    Invocation::new(
        RequestId::new("carddemo-base-batch-control-request", limits)
            .expect("static control request"),
        ExecutionId::new("carddemo-base-batch-control-execution", limits)
            .expect("static control execution"),
        RunUnitId::new("carddemo-base-batch-control-run", limits).expect("static control run"),
        None,
        Selector::new("zosmf:job-control", limits).expect("static control selector"),
        ArtifactRef::new("mainframe-env-batch@1", limits).expect("static control artifact"),
        Principal::new(
            PrincipalId::new("IBMUSER", limits).expect("static control principal"),
            grants,
            limits,
        )
        .expect("bounded control principal"),
        ServiceClass::Batch,
        0,
        1_000_000,
        TraceId::new("carddemo-base-batch-control-trace", limits).expect("static control trace"),
        IdempotencyKey::new("carddemo-base-batch-control-invocation", limits)
            .expect("static control invocation key"),
        1,
        ResourceLimits::default(),
        BTreeMap::new(),
        limits,
    )
    .map_err(|_| {
        CorpusProblem::new(
            "carddemo.base_batch.control_invalid",
            "control invocation is invalid",
        )
    })
}

fn base_batch_spool_text(server: &ProductServer, id: &str) -> Result<String, CorpusProblem> {
    let invocation = base_batch_control_invocation()?;
    let mut text = String::new();
    for (_, name, records, _) in server
        .batch_service()
        .spool_files(&invocation, id)
        .map_err(terminal_problem)?
    {
        let (values, more) = server
            .batch_service()
            .spool(&invocation, id, &name, 0, records.max(1))
            .map_err(terminal_problem)?;
        if more {
            return Err(CorpusProblem::new(
                "carddemo.base_batch.spool_limit",
                format!("{id}/{name} exceeded its declared record count"),
            ));
        }
        for value in values {
            text.push_str(&String::from_utf8_lossy(&value));
            text.push('\n');
        }
    }
    Ok(text)
}

fn base_batch_dataset_digests(
    server: &ProductServer,
) -> Result<BTreeMap<String, String>, CorpusProblem> {
    let DatasetResult::Listed { names, more } = server
        .dataset_service()
        .invoke(DatasetRequest::List {
            pattern: "AWS.M2.CARD*".into(),
            start: None,
            max_items: 4_096,
        })
        .map_err(terminal_problem)?
    else {
        return Err(CorpusProblem::new(
            "carddemo.base_batch.catalog_drift",
            "dataset list returned the wrong result",
        ));
    };
    if more {
        return Err(CorpusProblem::new(
            "carddemo.base_batch.catalog_limit",
            "base-cycle catalog exceeded 4096 entries",
        ));
    }
    let mut observations = BTreeMap::<String, String>::new();
    let mut catalog = Sha256::new();
    for name in &names {
        digest_field(&mut catalog, name.as_str().as_bytes());
    }
    observations.insert("@CATALOG".into(), format!("{:x}", catalog.finalize()));
    for name in names {
        let attributes = match server.dataset_service().invoke(DatasetRequest::Attributes {
            dataset: name.clone(),
        }) {
            Ok(DatasetResult::Attributes {
                attributes,
                version,
            }) => (attributes, version),
            Err(HostProblem::NotFound) => continue,
            Ok(_) => {
                return Err(CorpusProblem::new(
                    "carddemo.base_batch.dataset_drift",
                    format!("{} attributes returned the wrong result", name.as_str()),
                ));
            }
            Err(problem) => return Err(terminal_problem(problem)),
        };
        let mut digest = Sha256::new();
        digest_field(
            &mut digest,
            format!("{:?}", attributes.0.organization).as_bytes(),
        );
        digest_field(
            &mut digest,
            format!("{:?}", attributes.0.record_format).as_bytes(),
        );
        digest_field(
            &mut digest,
            &attributes.0.logical_record_length.to_be_bytes(),
        );
        digest_field(
            &mut digest,
            &attributes.0.key_offset.unwrap_or(u32::MAX).to_be_bytes(),
        );
        digest_field(
            &mut digest,
            &attributes.0.key_length.unwrap_or(u32::MAX).to_be_bytes(),
        );
        digest_field(
            &mut digest,
            &attributes.0.ccsid.unwrap_or(u16::MAX).to_be_bytes(),
        );
        digest_field(&mut digest, &attributes.1.to_be_bytes());
        if attributes.0.organization == DatasetOrganization::Partitioned {
            let DatasetResult::Members {
                names: members,
                more,
            } = server
                .dataset_service()
                .invoke(DatasetRequest::ListMembers {
                    dataset: name.clone(),
                    start: None,
                    max_items: 4_096,
                })
                .map_err(terminal_problem)?
            else {
                return Err(CorpusProblem::new(
                    "carddemo.base_batch.member_drift",
                    format!("{} member list returned the wrong result", name.as_str()),
                ));
            };
            if more {
                return Err(CorpusProblem::new(
                    "carddemo.base_batch.member_limit",
                    format!("{} exceeded 4096 members", name.as_str()),
                ));
            }
            for member in members {
                digest_field(&mut digest, member.as_str().as_bytes());
                let DatasetResult::Records {
                    records,
                    identities,
                    version,
                } = server
                    .dataset_service()
                    .invoke(DatasetRequest::Read {
                        dataset: name.clone(),
                        member: Some(member),
                        key: None,
                        max_records: 4_096,
                        control: Default::default(),
                    })
                    .map_err(terminal_problem)?
                else {
                    return Err(CorpusProblem::new(
                        "carddemo.base_batch.member_drift",
                        format!("{} member read returned the wrong result", name.as_str()),
                    ));
                };
                digest_field(&mut digest, &version.to_be_bytes());
                for record in records {
                    digest_field(&mut digest, &record);
                }
                for identity in identities {
                    digest_field(&mut digest, &identity);
                }
            }
        } else {
            let DatasetResult::Records {
                records,
                identities,
                version,
            } = server
                .dataset_service()
                .invoke(DatasetRequest::Read {
                    dataset: name.clone(),
                    member: None,
                    key: None,
                    max_records: 4_096,
                    control: Default::default(),
                })
                .map_err(terminal_problem)?
            else {
                return Err(CorpusProblem::new(
                    "carddemo.base_batch.dataset_drift",
                    format!("{} read returned the wrong result", name.as_str()),
                ));
            };
            digest_field(&mut digest, &version.to_be_bytes());
            for record in records {
                digest_field(&mut digest, &record);
            }
            for identity in identities {
                digest_field(&mut digest, &identity);
            }
        }
        observations.insert(name.as_str().into(), format!("{:x}", digest.finalize()));
    }
    for required in [
        "AWS.M2.CARDDEMO.ACCTDATA.VSAM.KSDS",
        "AWS.M2.CARDDEMO.CARDXREF.VSAM.AIX.PATH",
        "AWS.M2.CARDDEMO.TRANSACT.VSAM.AIX.PATH",
        "AWS.M2.CARDDEMO.TRANSACT.VSAM.KSDS",
        "AWS.M2.CARDDEMO.TRXFL.SEQ",
        "AWS.M2.CARDDEMO.TRXFL.VSAM.KSDS",
        "AWS.M2.CARDDEMO.STATEMNT.PS",
        "AWS.M2.CARDDEMO.STATEMNT.HTML",
        "AWS.M2.CARDDEMO.TCATBALF.REPT",
        "AWS.M2.CARDEMO.FTP.TEST.BKUP.INTRDR",
    ] {
        if !observations.contains_key(required) {
            return Err(CorpusProblem::new(
                "carddemo.base_batch.dataset_missing",
                format!("{required} is missing from exact observations"),
            ));
        }
    }
    for base in [
        "AWS.M2.CARDDEMO.DALYREJS.G",
        "AWS.M2.CARDDEMO.SYSTRAN.G",
        "AWS.M2.CARDDEMO.TRANSACT.BKUP.G",
        "AWS.M2.CARDDEMO.TRANSACT.COMBINED.G",
        "AWS.M2.CARDDEMO.TRANSACT.DALY.G",
        "AWS.M2.CARDDEMO.TRANREPT.G",
        "AWS.M2.CARDDEMO.TCATBALF.BKUP.G",
    ] {
        if !observations.keys().any(|name| name.starts_with(base)) {
            return Err(CorpusProblem::new(
                "carddemo.base_batch.gdg_missing",
                format!("{base} generation is missing"),
            ));
        }
    }
    Ok(observations)
}

fn base_batch_spool_digests(
    server: &ProductServer,
    job_ids: &BTreeMap<String, String>,
) -> Result<BTreeMap<String, String>, CorpusProblem> {
    let invocation = base_batch_control_invocation()?;
    let mut observations = BTreeMap::new();
    for (label, id) in job_ids {
        let job = server.batch_service().get(id).map_err(terminal_problem)?;
        if !job.state.terminal() {
            return Err(CorpusProblem::new(
                "carddemo.base_batch.job_incomplete",
                format!("{label}/{id} is not terminal"),
            ));
        }
        let mut digest = Sha256::new();
        digest_field(&mut digest, label.as_bytes());
        digest_field(&mut digest, id.as_bytes());
        digest_field(&mut digest, format!("{:?}", job.state).as_bytes());
        digest_field(
            &mut digest,
            &job.return_code.unwrap_or(i32::MIN).to_be_bytes(),
        );
        digest_field(
            &mut digest,
            job.abend_code.as_deref().unwrap_or("").as_bytes(),
        );
        for (_, name, records, _) in server
            .batch_service()
            .spool_files(&invocation, id)
            .map_err(terminal_problem)?
        {
            digest_field(&mut digest, name.as_bytes());
            let (values, more) = server
                .batch_service()
                .spool(&invocation, id, &name, 0, records.max(1))
                .map_err(terminal_problem)?;
            if more {
                return Err(CorpusProblem::new(
                    "carddemo.base_batch.spool_limit",
                    format!("{label}/{name} exceeded its declared record count"),
                ));
            }
            for value in values {
                digest_field(&mut digest, &value);
            }
        }
        observations.insert(label.clone(), format!("{:x}", digest.finalize()));
    }
    Ok(observations)
}

fn corrected_creastmt(corpus_dir: &Path) -> Result<String, CorpusProblem> {
    let source = String::from_utf8(read_corpus_file(
        corpus_dir,
        &corpus_dir.join("app/jcl/CREASTMT.JCL"),
    )?)
    .map_err(|_| {
        CorpusProblem::new(
            "carddemo.base_batch.jcl_invalid",
            "CREASTMT.JCL is not UTF-8",
        )
    })?;
    let orphan = "//         SPACE=(CYL,(1,1),RLSE), 00,RECFM=FB), ATA.VSAM.KSDS";
    if source.matches(orphan).count() != 1 {
        return Err(CorpusProblem::new(
            "carddemo.base_batch.correction_drift",
            "CREASTMT orphan DD continuation differs from the accepted correction",
        ));
    }
    Ok(source
        .lines()
        .filter(|line| !line.starts_with(orphan))
        .collect::<Vec<_>>()
        .join("\n")
        + "\n")
}

fn install_base_batch_source_datasets(
    server: &ProductServer,
    corpus_dir: &Path,
) -> Result<usize, CorpusProblem> {
    let mut sequence = 20_000u64;
    let mut installed = 0usize;
    for object in carddemo_base_seed_objects(corpus_dir)? {
        let source = Path::new(&object.source_id)
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| {
                CorpusProblem::new(
                    "carddemo.base_batch.seed_invalid",
                    "seed source dataset name is invalid",
                )
            })?;
        let dataset = DatasetName::new(source, 128).map_err(|_| {
            CorpusProblem::new(
                "carddemo.base_batch.seed_invalid",
                "seed source dataset name is invalid",
            )
        })?;
        if server
            .dataset_service()
            .invoke(DatasetRequest::Attributes {
                dataset: dataset.clone(),
            })
            .is_ok()
        {
            continue;
        }
        let mutation = |sequence: u64, suffix: &str| Mutation {
            sequence,
            idempotency_key: IdempotencyKey::new(
                format!("carddemo-base-batch-{suffix}-{sequence}"),
                InvocationLimits::default(),
            )
            .expect("bounded source seed mutation"),
            transaction: Some("CD-023-SEED".into()),
        };
        let created = server
            .dataset_service()
            .invoke(DatasetRequest::Create {
                dataset: dataset.clone(),
                attributes: DatasetAttributes {
                    organization: DatasetOrganization::Sequential,
                    record_format: RecordFormat::Fixed,
                    logical_record_length: object.record_length,
                    key_offset: None,
                    key_length: None,
                    ccsid: Some(37),
                },
                mutation: mutation(sequence, "create"),
            })
            .map_err(terminal_problem)?;
        sequence += 1;
        let DatasetResult::Created { version } = created else {
            return Err(CorpusProblem::new(
                "carddemo.base_batch.seed_invalid",
                "seed source create returned the wrong result",
            ));
        };
        server
            .dataset_service()
            .invoke(DatasetRequest::Write {
                dataset,
                member: None,
                records: object
                    .bytes
                    .chunks_exact(object.record_length as usize)
                    .map(<[u8]>::to_vec)
                    .collect(),
                expected_version: Some(version),
                mutation: mutation(sequence, "write"),
            })
            .map_err(terminal_problem)?;
        sequence += 1;
        installed += 1;
    }
    let procedure_library =
        DatasetName::new("AWS.M2.CARDDEMO.PROC", 128).expect("static procedure library");
    let created = server
        .dataset_service()
        .invoke(DatasetRequest::Create {
            dataset: procedure_library.clone(),
            attributes: DatasetAttributes {
                organization: DatasetOrganization::Partitioned,
                record_format: RecordFormat::Fixed,
                logical_record_length: 80,
                key_offset: None,
                key_length: None,
                ccsid: Some(37),
            },
            mutation: Mutation {
                sequence,
                idempotency_key: IdempotencyKey::new(
                    format!("carddemo-base-batch-proc-create-{sequence}"),
                    InvocationLimits::default(),
                )
                .expect("bounded procedure mutation"),
                transaction: Some("CD-023-SEED".into()),
            },
        })
        .map_err(terminal_problem)?;
    sequence += 1;
    let DatasetResult::Created { mut version } = created else {
        return Err(CorpusProblem::new(
            "carddemo.base_batch.seed_invalid",
            "procedure library create returned the wrong result",
        ));
    };
    for relative in ["app/proc/REPROC.prc", "app/proc/TRANREPT.prc"] {
        let source = String::from_utf8(read_corpus_file(corpus_dir, &corpus_dir.join(relative))?)
            .map_err(|_| {
            CorpusProblem::new(
                "carddemo.base_batch.seed_invalid",
                format!("{relative} is not UTF-8"),
            )
        })?;
        let records = source
            .lines()
            .map(|line| {
                let mut record = CodePage::Cp037.encode(line, 320).map_err(|_| {
                    CorpusProblem::new(
                        "carddemo.base_batch.seed_invalid",
                        format!("{relative} cannot be encoded as CP037"),
                    )
                })?;
                if record.len() > 80 {
                    return Err(CorpusProblem::new(
                        "carddemo.base_batch.seed_invalid",
                        format!("{relative} contains a line longer than 80 bytes"),
                    ));
                }
                record.resize(80, 0x40);
                Ok(record)
            })
            .collect::<Result<Vec<_>, CorpusProblem>>()?;
        let member = Path::new(relative)
            .file_stem()
            .and_then(|value| value.to_str())
            .ok_or_else(|| {
                CorpusProblem::new(
                    "carddemo.base_batch.seed_invalid",
                    "procedure member name is invalid",
                )
            })?;
        version = match server
            .dataset_service()
            .invoke(DatasetRequest::Write {
                dataset: procedure_library.clone(),
                member: Some(MemberName::new(member, 8).map_err(|_| {
                    CorpusProblem::new(
                        "carddemo.base_batch.seed_invalid",
                        "procedure member name is invalid",
                    )
                })?),
                records,
                expected_version: Some(version),
                mutation: Mutation {
                    sequence,
                    idempotency_key: IdempotencyKey::new(
                        format!("carddemo-base-batch-proc-write-{sequence}"),
                        InvocationLimits::default(),
                    )
                    .expect("bounded procedure mutation"),
                    transaction: Some("CD-023-SEED".into()),
                },
            })
            .map_err(terminal_problem)?
        {
            DatasetResult::Mutated { version } => version,
            _ => {
                return Err(CorpusProblem::new(
                    "carddemo.base_batch.seed_invalid",
                    "procedure member write returned the wrong result",
                ));
            }
        };
        sequence += 1;
    }
    installed += 1;
    let fixed80 = |value: &[u8]| {
        let mut record = value.to_vec();
        record.resize(80, b' ');
        record
    };
    utility_seed_dataset(
        server,
        "AWS.M2.CARDDEMO.CNTL",
        DatasetOrganization::Partitioned,
        RecordFormat::Fixed,
        80,
        None,
        Vec::new(),
        &mut sequence,
    )?;
    utility_write_dataset(
        server,
        "AWS.M2.CARDDEMO.CNTL",
        Some("REPROCT"),
        vec![fixed80(b" REPRO INFILE(FILEIN) OUTFILE(FILEOUT)")],
        &mut sequence,
    )?;
    utility_seed_dataset(
        server,
        "AWS.M2.CARDDEMO.JCL",
        DatasetOrganization::Partitioned,
        RecordFormat::Fixed,
        80,
        None,
        Vec::new(),
        &mut sequence,
    )?;
    let child = String::from_utf8(read_corpus_file(
        corpus_dir,
        &corpus_dir.join("app/jcl/INTRDRJ2.JCL"),
    )?)
    .map_err(|_| CorpusProblem::new("carddemo.base_batch.seed_invalid", "INTRDRJ2 is not UTF-8"))?;
    utility_write_dataset(
        server,
        "AWS.M2.CARDDEMO.JCL",
        Some("INTRDRJ2"),
        child.lines().map(|line| fixed80(line.as_bytes())).collect(),
        &mut sequence,
    )?;
    for (name, records) in [
        (
            "AWS.M2.CARDEMO.FTP.TEST",
            vec![fixed80(b"CARDDEMO INTERNAL READER")],
        ),
        ("AWS.M2.CARDEMO.FTP.TEST.BKUP", Vec::new()),
        ("AWS.M2.CARDEMO.FTP.TEST.BKUP.INTRDR", Vec::new()),
        (
            "AWS.M2.CARDDEMO.DATEPARM",
            vec![fixed80(b"2022-01-01 2022-07-06")],
        ),
    ] {
        utility_seed_dataset(
            server,
            name,
            DatasetOrganization::Sequential,
            RecordFormat::Fixed,
            80,
            None,
            records,
            &mut sequence,
        )?;
    }
    installed += 6;
    Ok(installed)
}

fn compile_carddemo_batch_definitions(
    bundles: &BTreeMap<String, (String, SourceBundle)>,
    needed: &BTreeSet<String>,
) -> Result<Vec<BatchProgramDefinition>, CorpusProblem> {
    let compiler = CobolCompiler::default();
    needed
        .iter()
        .map(|name| {
            let (path, bundle) = bundles.get(name).ok_or_else(|| {
                CorpusProblem::new(
                    "carddemo.batch_program.source_missing",
                    format!("{name} source is missing"),
                )
            })?;
            let result = compiler
                .compile(CompilerRequest {
                    source: bundle.clone(),
                    mode: CompilationMode::Executable,
                    target: CompileTarget::new("reference").map_err(|error| {
                        CorpusProblem::new(
                            "carddemo.batch_program.target_invalid",
                            error.to_string(),
                        )
                    })?,
                    options: CompileOptions::new(BTreeMap::new()).map_err(|error| {
                        CorpusProblem::new(
                            "carddemo.batch_program.options_invalid",
                            error.to_string(),
                        )
                    })?,
                })
                .map_err(|error| {
                    CorpusProblem::new(
                        "carddemo.batch_program.compile_failed",
                        format!("{path}: {error}"),
                    )
                })?;
            let artifact = match result {
                CompilerResult::Published { artifact, .. } => artifact,
                CompilerResult::Analysis { diagnostics, .. }
                | CompilerResult::Failed { diagnostics, .. } => {
                    return Err(CorpusProblem::new(
                        "carddemo.batch_program.compile_failed",
                        format!(
                            "{path}: {}",
                            diagnostics.first().map_or("no diagnostic", |diagnostic| {
                                diagnostic.public_message()
                            })
                        ),
                    ));
                }
            };
            Ok(BatchProgramDefinition::current(name.clone(), &artifact))
        })
        .collect()
}

fn compile_free_batch_definition(
    name: &str,
    source: &str,
) -> Result<BatchProgramDefinition, CorpusProblem> {
    let limits = SourceLimits::default();
    let logical = format!("{name}.cbl");
    let path = LogicalPath::new(&logical, limits.max_path_bytes).map_err(|error| {
        CorpusProblem::new("carddemo.batch_program.fixture_invalid", error.to_string())
    })?;
    let bundle = SourceBundle::new(
        &path,
        vec![
            SourceFile::input(
                logical,
                source.as_bytes().to_vec(),
                SourceFormat::Free,
                SourceEncoding::Utf8,
                limits,
            )
            .map_err(|error| {
                CorpusProblem::new("carddemo.batch_program.fixture_invalid", error.to_string())
            })?,
        ],
        BTreeMap::new(),
        Vec::new(),
        limits,
    )
    .map_err(|error| {
        CorpusProblem::new("carddemo.batch_program.fixture_invalid", error.to_string())
    })?;
    let result = CobolCompiler::default()
        .compile(CompilerRequest {
            source: bundle,
            mode: CompilationMode::Executable,
            target: CompileTarget::new("reference").map_err(|error| {
                CorpusProblem::new("carddemo.batch_program.fixture_invalid", error.to_string())
            })?,
            options: CompileOptions::new(BTreeMap::new()).map_err(|error| {
                CorpusProblem::new("carddemo.batch_program.fixture_invalid", error.to_string())
            })?,
        })
        .map_err(|error| {
            CorpusProblem::new("carddemo.batch_program.fixture_invalid", error.to_string())
        })?;
    let CompilerResult::Published { artifact, .. } = result else {
        return Err(CorpusProblem::new(
            "carddemo.batch_program.fixture_invalid",
            format!("{name} did not publish"),
        ));
    };
    Ok(BatchProgramDefinition::current(name, &artifact))
}

struct BatchProgramExercise {
    installed_artifacts: usize,
    install_replay: bool,
    selected_job_routes: usize,
    linkage_routes: usize,
    file_routes: usize,
    abend_controls: usize,
    restart_routes: usize,
}

async fn exercise_batch_program_routes(
    definitions: Vec<BatchProgramDefinition>,
    linkage_fixture: BatchProgramDefinition,
    abend_fixture: BatchProgramDefinition,
    wait_jcl: &[u8],
) -> Result<BatchProgramExercise, CorpusProblem> {
    let artifact_root = env::temp_dir().join(format!(
        "mainframe-env-carddemo-batch-programs-{}",
        std::process::id()
    ));
    let config = ServerConfig {
        store_profile: StoreProfile::Memory,
        artifact_root: artifact_root.clone(),
        tls: TlsConfig {
            enabled: false,
            certificate_path: None,
            private_key_reference: None,
        },
        ..ServerConfig::default()
    };
    let store = Arc::new(MemoryStore::new(Default::default()));
    let secrets = Arc::new(MemorySecretResolver::default());
    let server = ProductServer::open(
        config.clone(),
        store.clone(),
        secrets.clone(),
        default_program_router(),
    )
    .map_err(terminal_problem)?;
    server
        .bootstrap_user("IBMUSER", b"TESTPASS")
        .map_err(terminal_problem)?;
    let first = server
        .install_batch_programs(definitions.clone())
        .map_err(terminal_problem)?;
    let replay = server
        .install_batch_programs(definitions)
        .map_err(terminal_problem)?;
    server
        .install_batch_programs(vec![linkage_fixture, abend_fixture])
        .map_err(terminal_problem)?;
    let mut record = vec![b' '; 350];
    record[..4].copy_from_slice(b"ABCD");
    let mut dataset_sequence = 1u64;
    utility_seed_dataset(
        &server,
        "IBMUSER.TRNX",
        DatasetOrganization::Sequential,
        RecordFormat::Fixed,
        350,
        None,
        vec![record],
        &mut dataset_sequence,
    )?;
    submit_utility_job(
        &server,
        &server.router(),
        std::str::from_utf8(wait_jcl).map_err(|_| {
            CorpusProblem::new(
                "carddemo.batch_program.jcl_invalid",
                "WAITSTEP is not UTF-8",
            )
        })?,
    )
    .await?;
    submit_utility_job(
        &server,
        &server.router(),
        "//LINKJOB JOB CLASS=A\n//RUN EXEC PGM=LINKMAIN\n//TRNXFILE DD DSN=IBMUSER.TRNX,DISP=SHR\n",
    )
    .await?;
    let principal =
        PrincipalId::new("IBMUSER", InvocationLimits::default()).expect("static principal");
    let (jobs, _) = server
        .batch_service()
        .list(Some(&principal), None, 64)
        .map_err(terminal_problem)?;
    let link_job = jobs
        .iter()
        .find(|job| job.name == "LINKJOB")
        .ok_or_else(|| {
            CorpusProblem::new(
                "carddemo.batch_program.link_missing",
                "LINKJOB was not created",
            )
        })?;
    let (output, _) = server
        .batch_service()
        .spool(
            &base_batch_control_invocation()?,
            &link_job.id,
            "SYSPRINT",
            0,
            64,
        )
        .map_err(terminal_problem)?;
    if !output.iter().any(|record| record == b"00")
        || !output.iter().any(|record| record.starts_with(b"ABCD"))
    {
        return Err(CorpusProblem::new(
            "carddemo.batch_program.link_drift",
            format!("CBSTM03B linkage output differs: {output:?}"),
        ));
    }
    submit_expected_abend(
        &server,
        &server.router(),
        "//ABENDJOB JOB CLASS=A\n//FAIL EXEC PGM=ABENDCHK\n",
        "ABEND U0999",
    )
    .await?;
    if !server.graceful_shutdown().await {
        return Err(CorpusProblem::new(
            "carddemo.batch_program.shutdown_failed",
            "first server did not shut down",
        ));
    }
    let restarted = ProductServer::open(config, store, secrets, default_program_router())
        .map_err(terminal_problem)?;
    submit_utility_job(
        &restarted,
        &restarted.router(),
        std::str::from_utf8(wait_jcl).map_err(|_| {
            CorpusProblem::new(
                "carddemo.batch_program.jcl_invalid",
                "WAITSTEP is not UTF-8",
            )
        })?,
    )
    .await?;
    let _ = restarted.graceful_shutdown().await;
    let _ = fs::remove_dir_all(&artifact_root);
    Ok(BatchProgramExercise {
        installed_artifacts: first.programs,
        install_replay: replay.replayed && replay.identity == first.identity,
        selected_job_routes: 4,
        linkage_routes: 1,
        file_routes: 3,
        abend_controls: 1,
        restart_routes: 1,
    })
}

fn carddemo_parsed_jcl(corpus_dir: &Path) -> Result<Vec<(String, JobPlan)>, CorpusProblem> {
    let procedure_paths = collect_paths(corpus_dir, &["app/proc"], "prc")?;
    let procedures = procedure_paths
        .iter()
        .map(|relative| {
            let name = Path::new(relative)
                .file_stem()
                .and_then(|value| value.to_str())
                .ok_or_else(|| {
                    CorpusProblem::new("carddemo.utility.path_invalid", "procedure path is invalid")
                })?
                .to_ascii_uppercase();
            let source =
                String::from_utf8(read_corpus_file(corpus_dir, &corpus_dir.join(relative))?)
                    .map_err(|_| {
                        CorpusProblem::new(
                            "carddemo.utility.source_invalid",
                            "procedure is not UTF-8",
                        )
                    })?;
            Ok((name, source))
        })
        .collect::<Result<BTreeMap<_, _>, CorpusProblem>>()?;
    collect_paths(
        corpus_dir,
        &[
            "app/jcl",
            "app/app-authorization-ims-db2-mq/jcl",
            "app/app-transaction-type-db2/jcl",
        ],
        "jcl",
    )?
    .into_iter()
    .filter(|relative| relative != "app/jcl/CREASTMT.JCL")
    .map(|relative| {
        let source = String::from_utf8(read_corpus_file(corpus_dir, &corpus_dir.join(&relative))?)
            .map_err(|_| {
                CorpusProblem::new("carddemo.utility.source_invalid", "JCL is not UTF-8")
            })?;
        let plan = parse_jcl(
            &JclBundle {
                primary: source,
                cataloged_procedures: procedures.clone(),
                ..Default::default()
            },
            JclLimits::default(),
        )
        .map_err(|problem| {
            CorpusProblem::new(
                "carddemo.utility.plan_failed",
                format!("{relative} failed: {problem:?}"),
            )
        })?;
        Ok((relative, plan))
    })
    .collect()
}

struct UtilityExercise {
    selected_job_routes: usize,
    exact_dataset_mutations: usize,
    disposition_controls: usize,
    gdg_controls: usize,
    aix_controls: usize,
    internal_reader_controls: usize,
}

async fn exercise_utility_routes() -> Result<UtilityExercise, CorpusProblem> {
    let artifact_root = env::temp_dir().join(format!(
        "mainframe-env-carddemo-utilities-{}",
        std::process::id()
    ));
    let server = ProductServer::open(
        ServerConfig {
            store_profile: StoreProfile::Memory,
            artifact_root: artifact_root.clone(),
            tls: TlsConfig {
                enabled: false,
                certificate_path: None,
                private_key_reference: None,
            },
            ..ServerConfig::default()
        },
        Arc::new(MemoryStore::new(Default::default())),
        Arc::new(MemorySecretResolver::default()),
        default_program_router(),
    )
    .map_err(terminal_problem)?;
    server
        .bootstrap_user("IBMUSER", b"TESTPASS")
        .map_err(terminal_problem)?;
    let app = server.router();
    let mut sequence = 1u64;
    utility_seed_dataset(
        &server,
        "IBMUSER.INPUT",
        DatasetOrganization::Sequential,
        RecordFormat::Fixed,
        6,
        None,
        vec![b"SECOND".to_vec(), b"FIRST ".to_vec()],
        &mut sequence,
    )?;
    for name in ["IBMUSER.OUTPUT", "IBMUSER.SORTOUT", "IBMUSER.TARGET"] {
        utility_seed_dataset(
            &server,
            name,
            DatasetOrganization::Sequential,
            RecordFormat::Fixed,
            6,
            None,
            if name == "IBMUSER.TARGET" {
                vec![b"STALE ".to_vec()]
            } else {
                Vec::new()
            },
            &mut sequence,
        )?;
    }
    submit_utility_job(
        &server,
        &app,
        "//COPYJOB JOB CLASS=A\n//COPY EXEC PGM=IEBGENER\n//SYSUT1 DD DSN=IBMUSER.INPUT,DISP=SHR\n//SYSUT2 DD DSN=IBMUSER.OUTPUT,DISP=OLD\n",
    )
    .await?;
    if utility_records(&server, "IBMUSER.OUTPUT", None)?
        != vec![b"SECOND".to_vec(), b"FIRST ".to_vec()]
    {
        return Err(CorpusProblem::new(
            "carddemo.utility.iebgener_drift",
            "dataset-backed IEBGENER output differs",
        ));
    }
    submit_utility_job(
        &server,
        &app,
        "//SORTJOB JOB CLASS=A\n//SORT EXEC PGM=SORT\n//SORTIN DD DSN=IBMUSER.INPUT,DISP=SHR\n//SORTOUT DD DSN=IBMUSER.SORTOUT,DISP=OLD\n//SYSIN DD *\n SORT FIELDS=(1,6,CH,A)\n/*\n",
    )
    .await?;
    if utility_records(&server, "IBMUSER.SORTOUT", None)?
        != vec![b"FIRST ".to_vec(), b"SECOND".to_vec()]
    {
        return Err(CorpusProblem::new(
            "carddemo.utility.sort_drift",
            "dataset-backed SORT output differs",
        ));
    }
    submit_utility_job(
        &server,
        &app,
        "//AMSJOB JOB CLASS=A\n//AMS EXEC PGM=IDCAMS\n//INPUT DD DSN=IBMUSER.INPUT,DISP=SHR\n//OUTPUT DD DSN=IBMUSER.TARGET,DISP=OLD\n//SYSIN DD *\n DELETE IBMUSER.TARGET\n IF MAXCC LE 08 THEN SET MAXCC = 0\n DEFINE CLUSTER (NAME(IBMUSER.TARGET) NONINDEXED RECORDSIZE(6 6))\n REPRO INFILE(INPUT) OUTFILE(OUTPUT)\n/*\n",
    )
    .await?;
    if utility_records(&server, "IBMUSER.TARGET", None)?
        != vec![b"SECOND".to_vec(), b"FIRST ".to_vec()]
    {
        return Err(CorpusProblem::new(
            "carddemo.utility.idcams_drift",
            "IDCAMS DELETE/DEFINE/REPRO output differs",
        ));
    }
    submit_utility_job(
        &server,
        &app,
        "//TEMPJOB JOB CLASS=A\n//MAKE EXEC PGM=IEFBR14\n//WORK DD DSN=&&WORK,DISP=(NEW,PASS,DELETE)\n//USE EXEC PGM=IEFBR14\n//INPUT DD DSN=&&WORK,DISP=(OLD,DELETE,DELETE)\n",
    )
    .await?;
    submit_utility_job(
        &server,
        &app,
        "//GDGJOB JOB CLASS=A\n//DEFINE EXEC PGM=IDCAMS\n//SYSIN DD *\n DEFINE GENERATIONDATAGROUP (NAME(IBMUSER.HISTORY) LIMIT(3) SCRATCH NOEMPTY)\n/*\n//COPY EXEC PGM=IEBGENER\n//SYSUT1 DD *\nGENERATION\n/*\n//SYSUT2 DD DSN=IBMUSER.HISTORY(+1),DISP=(NEW,CATLG,DELETE)\n",
    )
    .await?;
    let generation = match server
        .dataset_service()
        .invoke(DatasetRequest::ResolveGeneration {
            base: DatasetName::new("IBMUSER.HISTORY", 128).expect("static dataset"),
            relative: 0,
        })
        .map_err(terminal_problem)?
    {
        DatasetResult::Generation { dataset, .. } => dataset,
        _ => {
            return Err(CorpusProblem::new(
                "carddemo.utility.gdg_drift",
                "GDG resolution returned the wrong result",
            ));
        }
    };
    if utility_records(&server, generation.as_str(), None)? != vec![b"GENERATION".to_vec()] {
        return Err(CorpusProblem::new(
            "carddemo.utility.gdg_drift",
            "GDG generation output differs",
        ));
    }
    utility_seed_dataset(
        &server,
        "IBMUSER.BASE",
        DatasetOrganization::KeySequenced,
        RecordFormat::Fixed,
        8,
        Some((0, 4)),
        vec![b"0002BBBB".to_vec(), b"0001AAAA".to_vec()],
        &mut sequence,
    )?;
    submit_utility_job(
        &server,
        &app,
        "//AIXJOB JOB CLASS=A\n//AIX EXEC PGM=IDCAMS\n//SYSIN DD *\n DEFINE ALTERNATEINDEX (NAME(IBMUSER.BASE.AIX) RELATE(IBMUSER.BASE) KEYS(4 0) NONUNIQUEKEY UPGRADE)\n DEFINE PATH (NAME(IBMUSER.BASE.PATH) PATHENTRY(IBMUSER.BASE.AIX))\n BLDINDEX INDATASET(IBMUSER.BASE) OUTDATASET(IBMUSER.BASE.AIX)\n/*\n",
    )
    .await?;
    if utility_records(&server, "IBMUSER.BASE.AIX", None)?.len() != 2 {
        return Err(CorpusProblem::new(
            "carddemo.utility.aix_drift",
            "AIX browse state differs",
        ));
    }
    utility_seed_dataset(
        &server,
        "IBMUSER.JCL",
        DatasetOrganization::Partitioned,
        RecordFormat::Fixed,
        80,
        None,
        Vec::new(),
        &mut sequence,
    )?;
    utility_write_dataset(
        &server,
        "IBMUSER.JCL",
        Some("CHILD"),
        vec![
            b"//CHILD JOB CLASS=A".to_vec(),
            b"//RUN EXEC PGM=IEFBR14".to_vec(),
        ],
        &mut sequence,
    )?;
    submit_utility_job(
        &server,
        &app,
        "//PARENT JOB CLASS=A\n//SUBMIT EXEC PGM=IEBGENER\n//SYSUT1 DD DSN=IBMUSER.JCL(CHILD),DISP=SHR\n//SYSUT2 DD SYSOUT=(A,INTRDR)\n",
    )
    .await?;
    let principal =
        PrincipalId::new("IBMUSER", InvocationLimits::default()).expect("static principal");
    let child = wait_for_listed_job(
        &server,
        &principal,
        "CHILD",
        64,
        "carddemo.utility.internal_reader_drift",
        "carddemo.utility.internal_reader_drift",
    )
    .await?;
    if child.state != JobState::Completed || child.return_code != Some(0) {
        return Err(CorpusProblem::new(
            "carddemo.utility.internal_reader_drift",
            "internal reader child job did not complete",
        ));
    }
    drop(app);
    if !server.graceful_shutdown().await {
        return Err(CorpusProblem::new(
            "carddemo.utility.shutdown_failed",
            "utility server did not shut down",
        ));
    }
    drop(server);
    let _ = fs::remove_dir_all(&artifact_root);
    Ok(UtilityExercise {
        selected_job_routes: 7,
        exact_dataset_mutations: 7,
        disposition_controls: 1,
        gdg_controls: 1,
        aix_controls: 1,
        internal_reader_controls: 1,
    })
}

fn utility_seed_dataset(
    server: &ProductServer,
    name: &str,
    organization: DatasetOrganization,
    record_format: RecordFormat,
    lrecl: u32,
    key: Option<(u32, u32)>,
    records: Vec<Vec<u8>>,
    sequence: &mut u64,
) -> Result<(), CorpusProblem> {
    let dataset = DatasetName::new(name, 128)
        .map_err(|_| CorpusProblem::new("carddemo.utility.dataset", "dataset name is invalid"))?;
    let mutation = |value, suffix: &str| {
        IdempotencyKey::new(
            format!("utility-{suffix}-{value}"),
            InvocationLimits::default(),
        )
        .map(|idempotency_key| Mutation {
            sequence: value,
            idempotency_key,
            transaction: Some("CD-021".into()),
        })
        .map_err(|_| CorpusProblem::new("carddemo.utility.dataset", "mutation is invalid"))
    };
    let created = server
        .dataset_service()
        .invoke(DatasetRequest::Create {
            dataset: dataset.clone(),
            attributes: DatasetAttributes {
                organization,
                record_format,
                logical_record_length: lrecl,
                key_offset: key.map(|value| value.0),
                key_length: key.map(|value| value.1),
                ccsid: Some(1208),
            },
            mutation: mutation(*sequence, "create")?,
        })
        .map_err(terminal_problem)?;
    *sequence += 1;
    if !records.is_empty() {
        let version = match created {
            DatasetResult::Created { version } => version,
            _ => {
                return Err(CorpusProblem::new(
                    "carddemo.utility.dataset",
                    "dataset create returned the wrong result",
                ));
            }
        };
        server
            .dataset_service()
            .invoke(DatasetRequest::Write {
                dataset,
                member: None,
                records,
                expected_version: Some(version),
                mutation: mutation(*sequence, "write")?,
            })
            .map_err(terminal_problem)?;
        *sequence += 1;
    }
    Ok(())
}

fn utility_write_dataset(
    server: &ProductServer,
    name: &str,
    member: Option<&str>,
    records: Vec<Vec<u8>>,
    sequence: &mut u64,
) -> Result<(), CorpusProblem> {
    let dataset = DatasetName::new(name, 128)
        .map_err(|_| CorpusProblem::new("carddemo.utility.dataset", "dataset name is invalid"))?;
    let (attributes, version) = match server
        .dataset_service()
        .invoke(DatasetRequest::Attributes {
            dataset: dataset.clone(),
        })
        .map_err(terminal_problem)?
    {
        DatasetResult::Attributes {
            attributes,
            version,
        } => (attributes, version),
        _ => {
            return Err(CorpusProblem::new(
                "carddemo.utility.dataset",
                "dataset attributes returned the wrong result",
            ));
        }
    };
    let records = records
        .into_iter()
        .map(|mut record| {
            if matches!(
                attributes.record_format,
                RecordFormat::Fixed | RecordFormat::FixedBlocked
            ) {
                record.resize(attributes.logical_record_length as usize, b' ');
            }
            record
        })
        .collect();
    let key = IdempotencyKey::new(
        format!("utility-member-{sequence}"),
        InvocationLimits::default(),
    )
    .map_err(|_| CorpusProblem::new("carddemo.utility.dataset", "mutation is invalid"))?;
    server
        .dataset_service()
        .invoke(DatasetRequest::Write {
            dataset,
            member: member
                .map(|member| {
                    MemberName::new(member, 8).map_err(|_| {
                        CorpusProblem::new("carddemo.utility.dataset", "member name is invalid")
                    })
                })
                .transpose()?,
            records,
            expected_version: Some(version),
            mutation: Mutation {
                sequence: *sequence,
                idempotency_key: key,
                transaction: Some("CD-021".into()),
            },
        })
        .map_err(terminal_problem)?;
    *sequence += 1;
    Ok(())
}

fn utility_records(
    server: &ProductServer,
    name: &str,
    member: Option<&str>,
) -> Result<Vec<Vec<u8>>, CorpusProblem> {
    match server
        .dataset_service()
        .invoke(DatasetRequest::Read {
            dataset: DatasetName::new(name, 128).map_err(|_| {
                CorpusProblem::new("carddemo.utility.dataset", "dataset name is invalid")
            })?,
            member: member
                .map(|member| {
                    MemberName::new(member, 8).map_err(|_| {
                        CorpusProblem::new("carddemo.utility.dataset", "member name is invalid")
                    })
                })
                .transpose()?,
            key: None,
            max_records: 4_096,
            control: Default::default(),
        })
        .map_err(terminal_problem)?
    {
        DatasetResult::Records { records, .. } => Ok(records),
        _ => Err(CorpusProblem::new(
            "carddemo.utility.dataset",
            "dataset read returned the wrong result",
        )),
    }
}

async fn submit_utility_job(
    server: &Arc<ProductServer>,
    app: &axum::Router,
    jcl: &str,
) -> Result<(), CorpusProblem> {
    submit_job_with_retcode(server, app, jcl, "CC 0000")
        .await
        .map(|_| ())
}

async fn submit_job_with_retcode(
    server: &Arc<ProductServer>,
    app: &axum::Router,
    jcl: &str,
    expected_retcode: &str,
) -> Result<String, CorpusProblem> {
    server
        .start_background_workers()
        .map_err(terminal_problem)?;
    let headers = BTreeMap::from([
        (
            "authorization".into(),
            format!(
                "Basic {}",
                base64::engine::general_purpose::STANDARD.encode("IBMUSER:TESTPASS")
            ),
        ),
        ("x-csrf-zosmf-header".into(), "true".into()),
    ]);
    let (status, body) = terminal_http(
        app,
        Method::PUT,
        "/zosmf/restjobs/jobs",
        headers.clone(),
        jcl.as_bytes().to_vec(),
    )
    .await?;
    if status != StatusCode::CREATED {
        return Err(CorpusProblem::new(
            "carddemo.utility.job_failed",
            format!(
                "utility job returned {status}: {}",
                String::from_utf8_lossy(&body)
            ),
        ));
    }
    let job: serde_json::Value = serde_json::from_slice(&body)
        .map_err(|error| CorpusProblem::new("carddemo.utility.job_failed", error.to_string()))?;
    let job = wait_for_submitted_job(server, app, &headers, job).await?;
    if job["status"] != "OUTPUT" || job["retcode"] != expected_retcode {
        return Err(utility_job_failure(server, &job)?);
    }
    job["jobid"]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| CorpusProblem::new("carddemo.utility.job_failed", "job ID is missing"))
}

async fn exercise_base_online_smoke(
    config: ServerConfig,
    store: Arc<MemoryStore>,
    secrets: Arc<MemorySecretResolver>,
    corpus_dir: PathBuf,
    definition: OnlineApplicationDefinition,
) -> Result<BaseOnlineExercise, CorpusProblem> {
    let server = ProductServer::open(
        config.clone(),
        store.clone(),
        secrets.clone(),
        default_program_router(),
    )
    .map_err(terminal_problem)?;
    let expected_maps = definition
        .maps
        .iter()
        .map(|map| map.mapset.clone())
        .collect::<BTreeSet<_>>();
    install_base_online_authorities(&server, &corpus_dir, &definition)?;
    let first = server
        .install_online_application(definition.clone())
        .map_err(terminal_problem)?;
    let replay = server
        .install_online_application(definition)
        .map_err(terminal_problem)?;
    if first.programs != 18
        || first.transactions != 17
        || first.maps != 17
        || !replay.replayed
        || first.identity != replay.identity
    {
        return Err(CorpusProblem::new(
            "carddemo.online.install_drift",
            "base online install or replay differs",
        ));
    }
    let mut selected_maps = BTreeSet::new();
    let app = server.router();
    let launch = terminal_http(
        &app,
        Method::POST,
        "/mainframe-env/cics/v1/sessions",
        BTreeMap::from([
            (
                "authorization".into(),
                format!(
                    "Basic {}",
                    base64::engine::general_purpose::STANDARD.encode("WEBUSER:transport-password")
                ),
            ),
            ("x-csrf-zosmf-header".into(), "true".into()),
        ]),
        br#"{"transaction":"CC00"}"#.to_vec(),
    )
    .await?;
    if launch.0 != StatusCode::CREATED {
        return Err(CorpusProblem::new(
            "carddemo.online.launch_failed",
            format!(
                "CC00 launch returned {}: {}",
                launch.0,
                String::from_utf8_lossy(&launch.1)
            ),
        ));
    }
    let response: serde_json::Value = serde_json::from_slice(&launch.1)
        .map_err(|error| CorpusProblem::new("carddemo.online.response", error.to_string()))?;
    let screen = base64::engine::general_purpose::STANDARD
        .decode(
            response["terminal"]["screen_base64"]
                .as_str()
                .ok_or_else(|| {
                    CorpusProblem::new("carddemo.online.response", "screen is missing")
                })?,
        )
        .map_err(|error| CorpusProblem::new("carddemo.online.response", error.to_string()))?;
    if screen.is_empty() {
        return Err(CorpusProblem::new(
            "carddemo.online.screen_empty",
            "CC00 did not render the sign-on screen",
        ));
    }
    selected_maps.insert(
        response["terminal"]["mapset"]
            .as_str()
            .ok_or_else(|| {
                CorpusProblem::new("carddemo.online.response", "launch mapset is missing")
            })?
            .to_string(),
    );
    let session = response["session"]
        .as_str()
        .ok_or_else(|| CorpusProblem::new("carddemo.online.response", "session is missing"))?;
    let csrf = response["csrf_token"]
        .as_str()
        .ok_or_else(|| CorpusProblem::new("carddemo.online.response", "CSRF is missing"))?;
    let authentication = format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode("WEBUSER:transport-password")
    );
    let mutation_headers = BTreeMap::from([
        ("authorization".into(), authentication),
        ("x-csrf-zosmf-header".into(), "true".into()),
        ("x-csrf-token".into(), csrf.to_string()),
    ]);
    let input = terminal_http(
        &app,
        Method::PUT,
        &format!("/mainframe-env/cics/v1/sessions/{session}/input"),
        mutation_headers.clone(),
        serde_json::to_vec(&serde_json::json!({
            "aid":125,
            "fields":{"USERID":"NOUSER","PASSWD":"BADPASS"}
        }))
        .map_err(|error| CorpusProblem::new("carddemo.online.request", error.to_string()))?,
    )
    .await?;
    require_terminal_status(input.0, StatusCode::OK, "invalid sign-on input")?;
    let resumed = terminal_http(
        &app,
        Method::POST,
        &format!("/mainframe-env/cics/v1/sessions/{session}/resume"),
        mutation_headers.clone(),
        Vec::new(),
    )
    .await?;
    if resumed.0 != StatusCode::OK {
        return Err(CorpusProblem::new(
            "carddemo.online.invalid_signon_failed",
            format!(
                "invalid sign-on resume returned {}: {}",
                resumed.0,
                String::from_utf8_lossy(&resumed.1)
            ),
        ));
    }
    if resumed.1.windows(7).any(|value| value == b"BADPASS") {
        return Err(CorpusProblem::new(
            "carddemo.online.secret_disclosed",
            "invalid sign-on response disclosed a password",
        ));
    }
    let resumed_json: serde_json::Value = serde_json::from_slice(&resumed.1)
        .map_err(|error| CorpusProblem::new("carddemo.online.response", error.to_string()))?;
    let session_id = SessionId::new(session, InvocationLimits::default().max_binding_bytes)
        .map_err(|_| CorpusProblem::new("carddemo.online.response", "session is invalid"))?;
    let before_expiry = resumed_json["expires_at_tick"]
        .as_u64()
        .and_then(|value| value.checked_sub(1))
        .ok_or_else(|| CorpusProblem::new("carddemo.online.response", "expiry is invalid"))?;
    if !server
        .cics_service()
        .terminal_continuation_ready(
            &session_id,
            &PrincipalId::new("WEBUSER", InvocationLimits::default()).expect("static principal"),
            before_expiry,
        )
        .map_err(terminal_problem)?
    {
        return Err(CorpusProblem::new(
            "carddemo.online.continuation_claimed",
            format!(
                "invalid sign-on did not release its continuation claim: {:?}",
                server.online_trace(session).unwrap_or_default()
            ),
        ));
    }
    let valid_input = terminal_http(
        &app,
        Method::PUT,
        &format!("/mainframe-env/cics/v1/sessions/{session}/input"),
        mutation_headers.clone(),
        serde_json::to_vec(&serde_json::json!({
            "aid":125,
            "fields":{"USERID":"USER0001","PASSWD":"PASSWORD"}
        }))
        .map_err(|error| CorpusProblem::new("carddemo.online.request", error.to_string()))?,
    )
    .await?;
    require_terminal_status(valid_input.0, StatusCode::OK, "valid sign-on input")?;
    let menu = terminal_http(
        &app,
        Method::POST,
        &format!("/mainframe-env/cics/v1/sessions/{session}/resume"),
        mutation_headers.clone(),
        Vec::new(),
    )
    .await?;
    if menu.0 != StatusCode::OK {
        return Err(CorpusProblem::new(
            "carddemo.online.valid_signon_failed",
            format!(
                "valid sign-on resume returned {}: {}",
                menu.0,
                String::from_utf8_lossy(&menu.1)
            ),
        ));
    }
    let menu_json: serde_json::Value = serde_json::from_slice(&menu.1)
        .map_err(|error| CorpusProblem::new("carddemo.online.response", error.to_string()))?;
    if menu_json["mapset"] != "COMEN01" {
        let screen = base64::engine::general_purpose::STANDARD
            .decode(menu_json["screen_base64"].as_str().unwrap_or_default())
            .unwrap_or_default();
        return Err(CorpusProblem::new(
            "carddemo.online.menu_missing",
            format!(
                "valid sign-on remained on mapset {:?}; not-found={}; wrong-password={}; missing-user={}; missing-password={}; unable={}; trace={:?}",
                menu_json["mapset"],
                screen_contains(&screen, "User not found"),
                screen_contains(&screen, "Wrong Password"),
                screen_contains(&screen, "Please enter User ID"),
                screen_contains(&screen, "Please enter Password"),
                screen_contains(&screen, "Unable to verify"),
                server.online_trace(session).unwrap_or_default()
            ),
        ));
    }
    selected_maps.insert("COMEN01".into());
    let account_input = terminal_http(
        &app,
        Method::PUT,
        &format!("/mainframe-env/cics/v1/sessions/{session}/input"),
        mutation_headers.clone(),
        serde_json::to_vec(&serde_json::json!({
            "aid":125,
            "fields":{"OPTION":"1"}
        }))
        .map_err(|error| CorpusProblem::new("carddemo.online.request", error.to_string()))?,
    )
    .await?;
    require_terminal_status(account_input.0, StatusCode::OK, "account menu input")?;
    let account = terminal_http(
        &app,
        Method::POST,
        &format!("/mainframe-env/cics/v1/sessions/{session}/resume"),
        mutation_headers.clone(),
        Vec::new(),
    )
    .await?;
    if account.0 != StatusCode::OK {
        return Err(CorpusProblem::new(
            "carddemo.online.account_failed",
            format!(
                "account view returned {}: {}; trace={:?}",
                account.0,
                String::from_utf8_lossy(&account.1),
                server.online_trace(session).unwrap_or_default()
            ),
        ));
    }
    let account_json: serde_json::Value = serde_json::from_slice(&account.1)
        .map_err(|error| CorpusProblem::new("carddemo.online.response", error.to_string()))?;
    if account_json["mapset"] != "COACTVW" {
        return Err(CorpusProblem::new(
            "carddemo.online.account_map_missing",
            format!(
                "account view reached mapset {:?}; trace={:?}",
                account_json["mapset"],
                server.online_trace(session).unwrap_or_default()
            ),
        ));
    }
    selected_maps.insert("COACTVW".into());
    let account_detail = carddemo_terminal_exchange(
        &app,
        session,
        &mutation_headers,
        0x7d,
        BTreeMap::from([("ACCTSID".into(), "00000000050".into())]),
    )
    .await?;
    let _account_fields = online_screen_fields(&account_detail)?;
    if normal_online_effects(&server, session, CicsOperation::Read) < 4 {
        return Err(CorpusProblem::new(
            "carddemo.online.account_values",
            "account view did not complete its xref, account, and customer reads",
        ));
    }
    let rollback_controls = exercise_account_rollback_control(&server, &app).await?;
    let card_mutations = exercise_card_update_mutation(&server, &app).await?;
    for (option, expected_mapset) in [
        (2, "COACTUP"),
        (3, "COCRDLI"),
        (4, "COCRDSL"),
        (5, "COCRDUP"),
        (6, "COTRN00"),
        (7, "COTRN01"),
        (8, "COTRN02"),
        (9, "CORPT00"),
        (10, "COBIL00"),
    ] {
        let route = open_carddemo_menu(
            &server,
            &app,
            "WEBUSER",
            "transport-password",
            "USER0001",
            "PASSWORD",
            "COMEN01",
        )
        .await?;
        let selected = carddemo_terminal_exchange(
            &app,
            &route.session,
            &route.headers,
            0x7d,
            BTreeMap::from([("OPTION".into(), option.to_string())]),
        )
        .await?;
        require_online_mapset(
            &selected,
            expected_mapset,
            &format!("regular option {option}"),
        )?;
        selected_maps.insert(expected_mapset.into());
    }
    for (option, expected_mapset) in [
        (1, "COUSR00"),
        (2, "COUSR01"),
        (3, "COUSR02"),
        (4, "COUSR03"),
    ] {
        let route = open_carddemo_menu(
            &server,
            &app,
            "WEBADM",
            "admin-transport-password",
            "ADMIN001",
            "PASSWORD",
            "COADM01",
        )
        .await?;
        selected_maps.insert("COADM01".into());
        let selected = carddemo_terminal_exchange(
            &app,
            &route.session,
            &route.headers,
            0x7d,
            BTreeMap::from([("OPTION".into(), option.to_string())]),
        )
        .await?;
        require_online_mapset(
            &selected,
            expected_mapset,
            &format!("admin option {option}"),
        )?;
        selected_maps.insert(expected_mapset.into());
    }
    if selected_maps != expected_maps {
        return Err(CorpusProblem::new(
            "carddemo.online.map_coverage_drift",
            format!(
                "selected mapsets {selected_maps:?} differ from installed mapsets {expected_maps:?}"
            ),
        ));
    }
    let admin_mutations = exercise_admin_user_lifecycle(&server, &app).await?;
    let regular_mutations = exercise_regular_online_journeys(&server, &app).await?;
    let mut controls = exercise_base_online_controls(&server, &app).await?;
    controls.rollback_controls = rollback_controls;
    let dataset_reads = [
        CicsOperation::Read,
        CicsOperation::ReadNext,
        CicsOperation::ReadPrev,
    ]
    .into_iter()
    .try_fold(0usize, |total, operation| {
        server
            .online_operation_count(operation)
            .map(|count| total + count)
            .map_err(terminal_problem)
    })?;
    let restart_route = select_regular_option(&server, &app, 3, "COCRDLI").await?;
    drop(app);
    drop(server);
    let restarted = ProductServer::open(config, store, secrets, default_program_router())
        .map_err(terminal_problem)?;
    let restarted_app = restarted.router();
    let restarted_page = carddemo_terminal_exchange(
        &restarted_app,
        &restart_route.session,
        &restart_route.headers,
        0xf8,
        BTreeMap::new(),
    )
    .await?;
    require_online_mapset(&restarted_page, "COCRDLI", "card list restart resume")?;
    if restarted.cics_service().active_worker_count() != 0 {
        return Err(CorpusProblem::new(
            "carddemo.online.restart_worker",
            "restarted suspended journey retained an active worker",
        ));
    }
    let mut observations = vec![
        format!("maps:{}", selected_maps.len()),
        format!("dataset-reads:{dataset_reads}"),
        format!(
            "mutations:{}",
            admin_mutations + regular_mutations + card_mutations
        ),
        "restart:COCRDLI/PF8".into(),
    ];
    observations.extend(controls.observations);
    Ok(BaseOnlineExercise {
        initial_screen_bytes: screen.len(),
        screen_paths: selected_maps.len(),
        dataset_reads,
        committed_mutations: admin_mutations + regular_mutations + card_mutations,
        rollback_controls: controls.rollback_controls,
        denial_controls: controls.denial_controls,
        restart_controls: 1,
        concurrency_controls: controls.concurrency_controls,
        resource_controls: controls.resource_controls,
        install_replay: replay.replayed,
        observations,
    })
}

struct CardDemoOnlineSession {
    session: String,
    headers: BTreeMap<String, String>,
}

async fn open_carddemo_menu(
    server: &ProductServer,
    app: &axum::Router,
    transport_user: &str,
    transport_password: &str,
    application_user: &str,
    application_password: &str,
    expected_mapset: &str,
) -> Result<CardDemoOnlineSession, CorpusProblem> {
    let authorization = format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD
            .encode(format!("{transport_user}:{transport_password}"))
    );
    let launch = terminal_http(
        app,
        Method::POST,
        "/mainframe-env/cics/v1/sessions",
        BTreeMap::from([
            ("authorization".into(), authorization.clone()),
            ("x-csrf-zosmf-header".into(), "true".into()),
        ]),
        br#"{"transaction":"CC00"}"#.to_vec(),
    )
    .await?;
    require_terminal_status(launch.0, StatusCode::CREATED, "CardDemo launch")?;
    let launched: serde_json::Value = serde_json::from_slice(&launch.1)
        .map_err(|error| CorpusProblem::new("carddemo.online.response", error.to_string()))?;
    let session = launched["session"]
        .as_str()
        .ok_or_else(|| CorpusProblem::new("carddemo.online.response", "session is missing"))?
        .to_string();
    let csrf = launched["csrf_token"]
        .as_str()
        .ok_or_else(|| CorpusProblem::new("carddemo.online.response", "CSRF is missing"))?;
    let headers = BTreeMap::from([
        ("authorization".into(), authorization),
        ("x-csrf-zosmf-header".into(), "true".into()),
        ("x-csrf-token".into(), csrf.to_string()),
    ]);
    let signed_on = carddemo_terminal_exchange(
        app,
        &session,
        &headers,
        0x7d,
        BTreeMap::from([
            ("USERID".into(), application_user.to_string()),
            ("PASSWD".into(), application_password.to_string()),
        ]),
    )
    .await?;
    require_online_mapset(&signed_on, expected_mapset, "CardDemo sign-on").map_err(|problem| {
        CorpusProblem::new(
            "carddemo.online.signon_mapset_mismatch",
            format!(
                "{}; trace={:?}",
                problem.detail,
                server.online_trace(&session).unwrap_or_default()
            ),
        )
    })?;
    Ok(CardDemoOnlineSession { session, headers })
}

async fn carddemo_terminal_exchange(
    app: &axum::Router,
    session: &str,
    headers: &BTreeMap<String, String>,
    aid: u8,
    fields: BTreeMap<String, String>,
) -> Result<serde_json::Value, CorpusProblem> {
    let input = terminal_http(
        app,
        Method::PUT,
        &format!("/mainframe-env/cics/v1/sessions/{session}/input"),
        headers.clone(),
        serde_json::to_vec(&serde_json::json!({"aid":aid,"fields":fields}))
            .map_err(|error| CorpusProblem::new("carddemo.online.request", error.to_string()))?,
    )
    .await?;
    if input.0 != StatusCode::OK {
        return Err(CorpusProblem::new(
            "carddemo.online.exchange_failed",
            format!(
                "terminal input returned {}: {}",
                input.0,
                String::from_utf8_lossy(&input.1)
            ),
        ));
    }
    let resumed = terminal_http(
        app,
        Method::POST,
        &format!("/mainframe-env/cics/v1/sessions/{session}/resume"),
        headers.clone(),
        Vec::new(),
    )
    .await?;
    if resumed.0 != StatusCode::OK {
        return Err(CorpusProblem::new(
            "carddemo.online.exchange_failed",
            format!(
                "terminal exchange returned {}: {}",
                resumed.0,
                String::from_utf8_lossy(&resumed.1)
            ),
        ));
    }
    serde_json::from_slice(&resumed.1)
        .map_err(|error| CorpusProblem::new("carddemo.online.response", error.to_string()))
}

fn require_online_mapset(
    terminal: &serde_json::Value,
    expected: &str,
    operation: &str,
) -> Result<(), CorpusProblem> {
    if terminal["mapset"] == expected {
        Ok(())
    } else {
        let screen = terminal["screen_base64"]
            .as_str()
            .and_then(|value| base64::engine::general_purpose::STANDARD.decode(value).ok())
            .unwrap_or_default();
        Err(CorpusProblem::new(
            "carddemo.online.mapset_mismatch",
            format!(
                "{operation} reached mapset {:?}, expected {expected}; not-found={}; wrong-password={}; unable={}",
                terminal["mapset"],
                screen_contains(&screen, "User not found"),
                screen_contains(&screen, "Wrong Password"),
                screen_contains(&screen, "Unable to verify"),
            ),
        ))
    }
}

fn online_screen_fields(
    terminal: &serde_json::Value,
) -> Result<BTreeMap<String, Vec<u8>>, CorpusProblem> {
    let bytes =
        base64::engine::general_purpose::STANDARD
            .decode(terminal["screen_base64"].as_str().ok_or_else(|| {
                CorpusProblem::new("carddemo.online.response", "screen is missing")
            })?)
            .map_err(|error| CorpusProblem::new("carddemo.online.response", error.to_string()))?;
    let mut at = 0usize;
    let mut fields = BTreeMap::new();
    while at < bytes.len() {
        let name_length = u32::from_be_bytes(
            bytes
                .get(at..at + 4)
                .ok_or_else(|| {
                    CorpusProblem::new("carddemo.online.screen_invalid", "field name is truncated")
                })?
                .try_into()
                .map_err(|_| {
                    CorpusProblem::new("carddemo.online.screen_invalid", "field name is invalid")
                })?,
        ) as usize;
        at += 4;
        let name_end = at.checked_add(name_length).ok_or_else(|| {
            CorpusProblem::new("carddemo.online.screen_invalid", "field name is too large")
        })?;
        let name = String::from_utf8(
            bytes
                .get(at..name_end)
                .ok_or_else(|| {
                    CorpusProblem::new("carddemo.online.screen_invalid", "field name is truncated")
                })?
                .to_vec(),
        )
        .map_err(|_| {
            CorpusProblem::new("carddemo.online.screen_invalid", "field name is invalid")
        })?;
        at = name_end;
        let value_length = u32::from_be_bytes(
            bytes
                .get(at..at + 4)
                .ok_or_else(|| {
                    CorpusProblem::new("carddemo.online.screen_invalid", "field value is truncated")
                })?
                .try_into()
                .map_err(|_| {
                    CorpusProblem::new("carddemo.online.screen_invalid", "field value is invalid")
                })?,
        ) as usize;
        at += 4;
        let value_end = at.checked_add(value_length).ok_or_else(|| {
            CorpusProblem::new("carddemo.online.screen_invalid", "field value is too large")
        })?;
        let value = bytes
            .get(at..value_end)
            .ok_or_else(|| {
                CorpusProblem::new("carddemo.online.screen_invalid", "field value is truncated")
            })?
            .to_vec();
        at = value_end;
        if fields.insert(name, value).is_some() || fields.len() > 512 {
            return Err(CorpusProblem::new(
                "carddemo.online.screen_invalid",
                "screen fields are duplicated or unbounded",
            ));
        }
    }
    Ok(fields)
}

fn normal_online_effects(server: &ProductServer, session: &str, operation: CicsOperation) -> usize {
    server.online_trace(session).map_or(0, |trace| {
        trace
            .iter()
            .filter(|entry| entry.operation == operation && entry.outcome == "NORMAL")
            .count()
    })
}

fn online_effects(server: &ProductServer, session: &str, operation: CicsOperation) -> usize {
    server.online_trace(session).map_or(0, |trace| {
        trace
            .iter()
            .filter(|entry| entry.operation == operation)
            .count()
    })
}

async fn exercise_admin_user_lifecycle(
    server: &ProductServer,
    app: &axum::Router,
) -> Result<usize, CorpusProblem> {
    let dataset = "AWS.M2.CARDDEMO.USRSEC.VSAM.KSDS";
    let before = carddemo_dataset_text_records(server, dataset)?;
    if before.iter().any(|record| record.starts_with("TEST0001")) {
        return Err(CorpusProblem::new(
            "carddemo.online.user_fixture_conflict",
            "bounded user lifecycle key already exists",
        ));
    }
    let add = select_admin_option(server, app, 2, "COUSR01").await?;
    let add_fields = BTreeMap::from([
        ("FNAME".into(), "TEST".into()),
        ("LNAME".into(), "OPERATOR".into()),
        ("USERID".into(), "TEST0001".into()),
        ("PASSWD".into(), "SECRETP1".into()),
        ("USRTYPE".into(), "U".into()),
    ]);
    let added =
        carddemo_terminal_exchange(app, &add.session, &add.headers, 0x7d, add_fields.clone())
            .await?;
    require_online_mapset(&added, "COUSR01", "user add")?;
    let fields = online_screen_fields(&added)?;
    if fields.get("PASSWD").is_some_and(|value| !value.is_empty())
        || base64::engine::general_purpose::STANDARD
            .decode(added["screen_base64"].as_str().unwrap_or_default())
            .is_ok_and(|screen| screen.windows(8).any(|value| value == b"SECRETP1"))
    {
        return Err(CorpusProblem::new(
            "carddemo.online.user_secret_disclosed",
            "user lifecycle screen disclosed a password",
        ));
    }
    let after_add = carddemo_dataset_text_records(server, dataset)?;
    let added_record = after_add
        .iter()
        .find(|record| record.starts_with("TEST0001"))
        .ok_or_else(|| {
            CorpusProblem::new(
                "carddemo.online.user_add_missing",
                "user add route did not create its exact keyed record",
            )
        })?;
    if !added_record.starts_with(&format!(
        "{:<8}{:<20}{:<20}{:<8}U",
        "TEST0001", "TEST", "OPERATOR", "SECRETP1"
    )) || after_add.len() != before.len() + 1
    {
        return Err(CorpusProblem::new(
            "carddemo.online.user_add_drift",
            "user add route produced the wrong final record",
        ));
    }
    let duplicate =
        carddemo_terminal_exchange(app, &add.session, &add.headers, 0x7d, add_fields).await?;
    if !online_screen_fields(&duplicate)?
        .get("ERRMSG")
        .is_some_and(|message| screen_contains(message, "already exist"))
        || carddemo_dataset_text_records(server, dataset)?.len() != after_add.len()
    {
        return Err(CorpusProblem::new(
            "carddemo.online.user_duplicate_control",
            "duplicate user add did not fail without mutation",
        ));
    }

    let update = select_admin_option(server, app, 3, "COUSR02").await?;
    let selected = carddemo_terminal_exchange(
        app,
        &update.session,
        &update.headers,
        0x7d,
        BTreeMap::from([("USRIDIN".into(), "TEST0001".into())]),
    )
    .await?;
    if !online_screen_fields(&selected)?
        .get("FNAME")
        .is_some_and(|value| String::from_utf8_lossy(value).trim() == "TEST")
    {
        return Err(CorpusProblem::new(
            "carddemo.online.user_update_read",
            "user update route did not display the keyed record",
        ));
    }
    let updated = carddemo_terminal_exchange(
        app,
        &update.session,
        &update.headers,
        0xf5,
        BTreeMap::from([
            ("USRIDIN".into(), "TEST0001".into()),
            ("FNAME".into(), "TEST".into()),
            ("LNAME".into(), "UPDATED".into()),
            ("PASSWD".into(), "SECRETP1".into()),
            ("USRTYPE".into(), "U".into()),
        ]),
    )
    .await?;
    require_online_mapset(&updated, "COUSR02", "user update")?;
    let after_update = carddemo_dataset_text_records(server, dataset)?;
    if !after_update.iter().any(|record| {
        record.starts_with(&format!(
            "{:<8}{:<20}{:<20}{:<8}U",
            "TEST0001", "TEST", "UPDATED", "SECRETP1"
        ))
    }) || after_update.len() != after_add.len()
    {
        return Err(CorpusProblem::new(
            "carddemo.online.user_update_drift",
            "user update route produced the wrong final record",
        ));
    }

    let delete = select_admin_option(server, app, 4, "COUSR03").await?;
    let selected = carddemo_terminal_exchange(
        app,
        &delete.session,
        &delete.headers,
        0x7d,
        BTreeMap::from([("USRIDIN".into(), "TEST0001".into())]),
    )
    .await?;
    if !online_screen_fields(&selected)?
        .get("FNAME")
        .is_some_and(|value| String::from_utf8_lossy(value).trim() == "TEST")
    {
        return Err(CorpusProblem::new(
            "carddemo.online.user_delete_read",
            "user delete route did not display the keyed record",
        ));
    }
    let deleted = carddemo_terminal_exchange(
        app,
        &delete.session,
        &delete.headers,
        0xf5,
        BTreeMap::from([("USRIDIN".into(), "TEST0001".into())]),
    )
    .await?;
    require_online_mapset(&deleted, "COUSR03", "user delete")?;
    let after_delete = carddemo_dataset_text_records(server, dataset)?;
    if after_delete.len() != before.len()
        || after_delete
            .iter()
            .any(|record| record.starts_with("TEST0001"))
    {
        return Err(CorpusProblem::new(
            "carddemo.online.user_delete_drift",
            "user delete route did not restore the exact dataset state",
        ));
    }
    Ok(3)
}

async fn select_admin_option(
    server: &ProductServer,
    app: &axum::Router,
    option: u8,
    expected_mapset: &str,
) -> Result<CardDemoOnlineSession, CorpusProblem> {
    let route = open_carddemo_menu(
        server,
        app,
        "WEBADM",
        "admin-transport-password",
        "ADMIN001",
        "PASSWORD",
        "COADM01",
    )
    .await?;
    let selected = carddemo_terminal_exchange(
        app,
        &route.session,
        &route.headers,
        0x7d,
        BTreeMap::from([("OPTION".into(), option.to_string())]),
    )
    .await?;
    require_online_mapset(
        &selected,
        expected_mapset,
        &format!("admin option {option}"),
    )?;
    Ok(route)
}

async fn select_regular_option(
    server: &ProductServer,
    app: &axum::Router,
    option: u8,
    expected_mapset: &str,
) -> Result<CardDemoOnlineSession, CorpusProblem> {
    let route = open_carddemo_menu(
        server,
        app,
        "WEBUSER",
        "transport-password",
        "USER0001",
        "PASSWORD",
        "COMEN01",
    )
    .await?;
    let selected = carddemo_terminal_exchange(
        app,
        &route.session,
        &route.headers,
        0x7d,
        BTreeMap::from([("OPTION".into(), option.to_string())]),
    )
    .await?;
    require_online_mapset(
        &selected,
        expected_mapset,
        &format!("regular option {option}"),
    )?;
    Ok(route)
}

async fn exercise_regular_online_journeys(
    server: &ProductServer,
    app: &axum::Router,
) -> Result<usize, CorpusProblem> {
    let card = "0500024453765740";
    let card_account = "00000000050";
    let transaction = "0000000000683580";

    let card_list = select_regular_option(server, app, 3, "COCRDLI").await?;
    for (aid, operation) in [(0xf8, "card list PF8"), (0xf7, "card list PF7")] {
        let page = carddemo_terminal_exchange(
            app,
            &card_list.session,
            &card_list.headers,
            aid,
            BTreeMap::new(),
        )
        .await?;
        require_online_mapset(&page, "COCRDLI", operation).map_err(|problem| {
            CorpusProblem::new(
                "carddemo.online.card_navigation",
                format!(
                    "{}; trace={:?}",
                    problem.detail,
                    server.online_trace(&card_list.session).unwrap_or_default()
                ),
            )
        })?;
    }

    let card_detail = select_regular_option(server, app, 4, "COCRDSL").await?;
    let detail = carddemo_terminal_exchange(
        app,
        &card_detail.session,
        &card_detail.headers,
        0x7d,
        BTreeMap::from([
            ("ACCTSID".into(), card_account.into()),
            ("CARDSID".into(), card.into()),
        ]),
    )
    .await?;
    let _detail_fields = online_screen_fields(&detail)?;
    if normal_online_effects(server, &card_detail.session, CicsOperation::Read) < 2 {
        return Err(CorpusProblem::new(
            "carddemo.online.card_detail_values",
            "card detail route did not complete its keyed reads",
        ));
    }

    let transaction_list = select_regular_option(server, app, 6, "COTRN00").await?;
    for (aid, operation) in [
        (0xf8, "transaction list PF8"),
        (0xf7, "transaction list PF7"),
    ] {
        let page = carddemo_terminal_exchange(
            app,
            &transaction_list.session,
            &transaction_list.headers,
            aid,
            BTreeMap::new(),
        )
        .await?;
        require_online_mapset(&page, "COTRN00", operation)?;
    }
    let transaction_detail = select_regular_option(server, app, 7, "COTRN01").await?;
    let detail = carddemo_terminal_exchange(
        app,
        &transaction_detail.session,
        &transaction_detail.headers,
        0x7d,
        BTreeMap::from([("TRNIDIN".into(), transaction.into())]),
    )
    .await?;
    let _detail_fields = online_screen_fields(&detail)?;
    if online_effects(server, &transaction_detail.session, CicsOperation::Read) < 2 {
        return Err(CorpusProblem::new(
            "carddemo.online.transaction_detail_values",
            "transaction detail route did not complete its keyed read",
        ));
    }

    let transact_dataset = "AWS.M2.CARDDEMO.TRANSACT.VSAM.KSDS";
    let before_transactions = carddemo_dataset_text_records(server, transact_dataset)?;
    let add = select_regular_option(server, app, 8, "COTRN02").await?;
    let added = carddemo_terminal_exchange(
        app,
        &add.session,
        &add.headers,
        0x7d,
        BTreeMap::from([
            ("ACTIDIN".into(), card_account.into()),
            ("TTYPCD".into(), "01".into()),
            ("TCATCD".into(), "0001".into()),
            ("TRNSRC".into(), "ONLINE".into()),
            ("TDESC".into(), "CERTIFIED PURCHASE".into()),
            ("TRNAMT".into(), "+00000001.00".into()),
            ("TORIGDT".into(), "2026-08-30".into()),
            ("TPROCDT".into(), "2026-08-30".into()),
            ("MID".into(), "123456789".into()),
            ("MNAME".into(), "CERTIFIED SHOP".into()),
            ("MCITY".into(), "HANOI".into()),
            ("MZIP".into(), "70000".into()),
            ("CONFIRM".into(), "y".into()),
        ]),
    )
    .await
    .map_err(|problem| {
        CorpusProblem::new(
            "carddemo.online.transaction_add_failed",
            format!(
                "{}; trace={:?}",
                problem.detail,
                server.online_trace(&add.session).unwrap_or_default()
            ),
        )
    })?;
    require_online_mapset(&added, "COTRN02", "transaction add")?;
    let after_transactions = carddemo_dataset_text_records(server, transact_dataset)?;
    if after_transactions.len() != before_transactions.len() + 1
        || !after_transactions
            .iter()
            .any(|record| record.contains("CERTIFIED PURCHASE"))
    {
        return Err(CorpusProblem::new(
            "carddemo.online.transaction_add_drift",
            format!(
                "transaction count {} -> {}; error={:?}; trace={:?}",
                before_transactions.len(),
                after_transactions.len(),
                online_screen_fields(&added)?.get("ERRMSG"),
                server.online_trace(&add.session).unwrap_or_default()
            ),
        ));
    }

    let reports_before = server
        .cics_service()
        .transient_records("JOBS")
        .map_err(terminal_problem)?;
    let report = select_regular_option(server, app, 9, "CORPT00").await?;
    let review = carddemo_terminal_exchange(
        app,
        &report.session,
        &report.headers,
        0x7d,
        BTreeMap::from([("MONTHLY".into(), "X".into())]),
    )
    .await?;
    require_online_mapset(&review, "CORPT00", "monthly report review")?;
    if !online_screen_fields(&review)?
        .get("ERRMSG")
        .is_some_and(|message| screen_contains(message, "Please confirm"))
    {
        return Err(CorpusProblem::new(
            "carddemo.online.report_confirmation_missing",
            "monthly report route did not request explicit confirmation",
        ));
    }
    let reported = carddemo_terminal_exchange(
        app,
        &report.session,
        &report.headers,
        0x7d,
        BTreeMap::from([
            ("MONTHLY".into(), "X".into()),
            ("CONFIRM".into(), "y".into()),
        ]),
    )
    .await?;
    require_online_mapset(&reported, "CORPT00", "monthly report")?;
    let reports_after = server
        .cics_service()
        .transient_records("JOBS")
        .map_err(terminal_problem)?;
    if reports_after.len() != reports_before.len() + 17
        || reports_after.last().is_none_or(Vec::is_empty)
    {
        return Err(CorpusProblem::new(
            "carddemo.online.report_queue_drift",
            format!(
                "report queue count {} -> {}; error={:?}; trace={:?}",
                reports_before.len(),
                reports_after.len(),
                online_screen_fields(&reported)?.get("ERRMSG"),
                server.online_trace(&report.session).unwrap_or_default()
            ),
        ));
    }

    let account_dataset = "AWS.M2.CARDDEMO.ACCTDATA.VSAM.KSDS";
    let before_accounts = carddemo_dataset_text_records(server, account_dataset)?;
    let before_account = before_accounts
        .iter()
        .find(|record| record.starts_with(card_account))
        .ok_or_else(|| {
            CorpusProblem::new(
                "carddemo.online.bill_account_missing",
                "bill account is missing",
            )
        })?;
    if before_account.get(12..24) == Some("00000000000{") {
        return Err(CorpusProblem::new(
            "carddemo.online.bill_fixture_zero",
            "bill account has no opening balance",
        ));
    }
    let bill = select_regular_option(server, app, 10, "COBIL00").await?;
    let review = carddemo_terminal_exchange(
        app,
        &bill.session,
        &bill.headers,
        0x7d,
        BTreeMap::from([("ACTIDIN".into(), card_account.into())]),
    )
    .await?;
    require_online_mapset(&review, "COBIL00", "bill review")?;
    let paid = carddemo_terminal_exchange(
        app,
        &bill.session,
        &bill.headers,
        0x7d,
        BTreeMap::from([
            ("ACTIDIN".into(), card_account.into()),
            ("CONFIRM".into(), "y".into()),
        ]),
    )
    .await?;
    require_online_mapset(&paid, "COBIL00", "bill payment")?;
    let after_accounts = carddemo_dataset_text_records(server, account_dataset)?;
    let after_account = after_accounts
        .iter()
        .find(|record| record.starts_with(card_account))
        .ok_or_else(|| {
            CorpusProblem::new(
                "carddemo.online.bill_account_missing",
                "bill account disappeared",
            )
        })?;
    let final_transactions = carddemo_dataset_text_records(server, transact_dataset)?;
    if after_account.get(12..24) != Some("00000000000{")
        || final_transactions.len() != after_transactions.len() + 1
        || !final_transactions
            .iter()
            .any(|record| record.contains("BILL PAYMENT - ONLINE"))
    {
        return Err(CorpusProblem::new(
            "carddemo.online.bill_payment_drift",
            format!(
                "balance {:?} -> {:?}; transaction count {} -> {}; error={:?}; trace={:?}",
                before_account.get(12..24),
                after_account.get(12..24),
                after_transactions.len(),
                final_transactions.len(),
                online_screen_fields(&paid)?.get("ERRMSG"),
                server.online_trace(&bill.session).unwrap_or_default()
            ),
        ));
    }
    Ok(3)
}

struct BaseOnlineControls {
    rollback_controls: usize,
    denial_controls: usize,
    concurrency_controls: usize,
    resource_controls: usize,
    observations: Vec<String>,
}

async fn exercise_base_online_controls(
    server: &ProductServer,
    app: &axum::Router,
) -> Result<BaseOnlineControls, CorpusProblem> {
    let regular_authorization = format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode("WEBUSER:transport-password")
    );
    let denied = terminal_http(
        app,
        Method::POST,
        "/mainframe-env/cics/v1/sessions",
        BTreeMap::from([
            ("authorization".into(), regular_authorization.clone()),
            ("x-csrf-zosmf-header".into(), "true".into()),
        ]),
        br#"{"transaction":"CU01"}"#.to_vec(),
    )
    .await?;
    if denied.0 != StatusCode::FORBIDDEN {
        return Err(CorpusProblem::new(
            "carddemo.online.regular_admin_denial",
            format!("regular-user CU01 launch returned {}", denied.0),
        ));
    }

    let left = open_carddemo_menu(
        server,
        app,
        "WEBUSER",
        "transport-password",
        "USER0001",
        "PASSWORD",
        "COMEN01",
    )
    .await?;
    let right = open_carddemo_menu(
        server,
        app,
        "WEBUSER",
        "transport-password",
        "USER0001",
        "PASSWORD",
        "COMEN01",
    )
    .await?;
    if left.session == right.session {
        return Err(CorpusProblem::new(
            "carddemo.online.concurrent_session_identity",
            "concurrent CardDemo sessions reused an identity",
        ));
    }
    let left_app = app.clone();
    let right_app = app.clone();
    let (left_result, right_result) = tokio::join!(
        carddemo_terminal_exchange(
            &left_app,
            &left.session,
            &left.headers,
            0x7d,
            BTreeMap::from([("OPTION".into(), "1".into())]),
        ),
        carddemo_terminal_exchange(
            &right_app,
            &right.session,
            &right.headers,
            0x7d,
            BTreeMap::from([("OPTION".into(), "3".into())]),
        )
    );
    let left_result = left_result?;
    let right_result = right_result?;
    require_online_mapset(&left_result, "COACTVW", "concurrent account session")?;
    require_online_mapset(&right_result, "COCRDLI", "concurrent card session")?;

    let oversized_launch = terminal_http(
        app,
        Method::POST,
        "/mainframe-env/cics/v1/sessions",
        BTreeMap::from([
            ("authorization".into(), regular_authorization),
            ("x-csrf-zosmf-header".into(), "true".into()),
        ]),
        br#"{"transaction":"CC00","rows":65535,"columns":65535}"#.to_vec(),
    )
    .await?;
    if oversized_launch.0 != StatusCode::TOO_MANY_REQUESTS {
        return Err(CorpusProblem::new(
            "carddemo.online.screen_bound",
            format!("oversized terminal launch returned {}", oversized_launch.0),
        ));
    }
    let bounded = open_carddemo_menu(
        server,
        app,
        "WEBUSER",
        "transport-password",
        "USER0001",
        "PASSWORD",
        "COMEN01",
    )
    .await?;
    let oversized_input = terminal_http(
        app,
        Method::PUT,
        &format!("/mainframe-env/cics/v1/sessions/{}/input", bounded.session),
        bounded.headers,
        serde_json::to_vec(&serde_json::json!({
            "aid":125,
            "fields":{"OPTION":"1234567890"}
        }))
        .map_err(|error| CorpusProblem::new("carddemo.online.request", error.to_string()))?,
    )
    .await?;
    if oversized_input.0 != StatusCode::FORBIDDEN {
        return Err(CorpusProblem::new(
            "carddemo.online.input_bound",
            format!("oversized BMS input returned {}", oversized_input.0),
        ));
    }
    Ok(BaseOnlineControls {
        rollback_controls: 0,
        denial_controls: 1,
        concurrency_controls: 1,
        resource_controls: 2,
        observations: vec![
            "denial:WEBUSER/CU01".into(),
            "concurrency:COACTVW|COCRDLI".into(),
            "bounds:screen|field".into(),
            "rollback:COACTUP/ACCTDAT+CUSTDAT".into(),
        ],
    })
}

async fn exercise_account_rollback_control(
    server: &ProductServer,
    app: &axum::Router,
) -> Result<usize, CorpusProblem> {
    const INPUT_FIELDS: &[&str] = &[
        "ACCTSID", "ACSTTUS", "OPNYEAR", "OPNMON", "OPNDAY", "ACRDLIM", "EXPYEAR", "EXPMON",
        "EXPDAY", "ACSHLIM", "RISYEAR", "RISMON", "RISDAY", "ACURBAL", "ACRCYCR", "AADDGRP",
        "ACRCYDB", "ACSTNUM", "ACTSSN1", "ACTSSN2", "ACTSSN3", "DOBYEAR", "DOBMON", "DOBDAY",
        "ACSTFCO", "ACSFNAM", "ACSMNAM", "ACSLNAM", "ACSADL1", "ACSSTTE", "ACSADL2", "ACSZIPC",
        "ACSCITY", "ACSCTRY", "ACSPH1A", "ACSPH1B", "ACSPH1C", "ACSGOVT", "ACSPH2A", "ACSPH2B",
        "ACSPH2C", "ACSEFTC", "ACSPFLG",
    ];
    let account_dataset = "AWS.M2.CARDDEMO.ACCTDATA.VSAM.KSDS";
    let customer_dataset = "AWS.M2.CARDDEMO.CUSTDATA.VSAM.KSDS";
    let accounts_before = carddemo_dataset_text_records(server, account_dataset)?;
    let customers_before = carddemo_dataset_text_records(server, customer_dataset)?;
    let route = select_regular_option(server, app, 2, "COACTUP").await?;
    let selected = carddemo_terminal_exchange(
        app,
        &route.session,
        &route.headers,
        0x7d,
        BTreeMap::from([("ACCTSID".into(), "00000000050".into())]),
    )
    .await
    .map_err(|problem| {
        CorpusProblem::new(
            "carddemo.online.account_rollback_select_failed",
            problem.detail,
        )
    })?;
    require_online_mapset(&selected, "COACTUP", "account rollback selection")?;
    let displayed = online_screen_fields(&selected)?;
    for required in ["ACCTSID", "ACSTTUS", "OPNYEAR", "ACRDLIM", "ACSFNAM"] {
        if displayed
            .get(required)
            .is_none_or(|value| value.iter().all(|byte| matches!(*byte, 0 | b' ')))
        {
            return Err(CorpusProblem::new(
                "carddemo.online.account_update_values",
                format!(
                    "account update field {required} is blank; error={:?}; info={:?}; trace={:?}",
                    displayed.get("ERRMSG"),
                    displayed.get("INFOMSG"),
                    server.online_trace(&route.session).unwrap_or_default()
                ),
            ));
        }
    }
    let mut edited = BTreeMap::new();
    for name in INPUT_FIELDS {
        let value = displayed.get(*name).ok_or_else(|| {
            CorpusProblem::new(
                "carddemo.online.account_update_values",
                format!("account update field {name} is missing"),
            )
        })?;
        edited.insert(
            (*name).to_string(),
            String::from_utf8(value.clone())
                .map_err(|_| {
                    CorpusProblem::new(
                        "carddemo.online.account_update_values",
                        format!("account update field {name} is not UTF-8"),
                    )
                })?
                .trim_end_matches([' ', '\0'])
                .to_string(),
        );
    }
    for (name, value) in [
        ("ACTSSN1", "123"),
        ("ACTSSN2", "45"),
        ("ACTSSN3", "6789"),
        ("DOBYEAR", "1960"),
        ("DOBMON", "01"),
        ("DOBDAY", "01"),
        ("ACSTFCO", "700"),
        ("ACSFNAM", "ROLLBACK"),
        ("ACSMNAM", "TEST"),
        ("ACSLNAM", "USER"),
        ("ACSADL1", "ONE MAIN STREET"),
        ("ACSSTTE", "CA"),
        ("ACSADL2", "SUITE ONE"),
        ("ACSZIPC", "90210"),
        ("ACSCITY", "LOS ANGELES"),
        ("ACSCTRY", "USA"),
        ("ACSPH1A", "212"),
        ("ACSPH1B", "234"),
        ("ACSPH1C", "1234"),
        ("ACSGOVT", "TESTID123"),
        ("ACSPH2A", "212"),
        ("ACSPH2B", "234"),
        ("ACSPH2C", "5678"),
        ("ACSEFTC", "1234567890"),
        ("ACSPFLG", "Y"),
    ] {
        edited.insert(name.into(), value.into());
    }
    let review =
        carddemo_terminal_exchange(app, &route.session, &route.headers, 0x7d, edited.clone())
            .await
            .map_err(|problem| {
                CorpusProblem::new(
                    "carddemo.online.account_rollback_review_failed",
                    problem.detail,
                )
            })?;
    require_online_mapset(&review, "COACTUP", "account rollback review")?;
    let review_fields = online_screen_fields(&review)?;
    if review_fields
        .get("ERRMSG")
        .is_some_and(|message| message.iter().any(|byte| !matches!(*byte, 0 | b' ')))
    {
        return Err(CorpusProblem::new(
            "carddemo.online.account_update_review",
            format!(
                "input dates open={:?}-{:?}-{:?}, expiry={:?}-{:?}-{:?}, reissue={:?}-{:?}-{:?}; error={:?}; trace={:?}",
                edited.get("OPNYEAR"),
                edited.get("OPNMON"),
                edited.get("OPNDAY"),
                edited.get("EXPYEAR"),
                edited.get("EXPMON"),
                edited.get("EXPDAY"),
                edited.get("RISYEAR"),
                edited.get("RISMON"),
                edited.get("RISDAY"),
                review_fields.get("ERRMSG"),
                server.online_trace(&route.session).unwrap_or_default()
            ),
        ));
    }
    if carddemo_dataset_text_records(server, account_dataset)? != accounts_before
        || carddemo_dataset_text_records(server, customer_dataset)? != customers_before
    {
        return Err(CorpusProblem::new(
            "carddemo.online.account_premature_update",
            "account review mutated data before PF5 confirmation",
        ));
    }
    server
        .cics_service()
        .inject_file_failure_once(CicsOperation::Rewrite, "CUSTDAT")
        .map_err(terminal_problem)?;
    let failed = carddemo_terminal_exchange(app, &route.session, &route.headers, 0xf5, edited)
        .await
        .map_err(|problem| {
            CorpusProblem::new(
                "carddemo.online.account_rollback_commit_failed",
                problem.detail,
            )
        })?;
    require_online_mapset(&failed, "COACTUP", "account rollback failure")?;
    if carddemo_dataset_text_records(server, account_dataset)? != accounts_before
        || carddemo_dataset_text_records(server, customer_dataset)? != customers_before
        || online_effects(server, &route.session, CicsOperation::Rewrite) < 2
        || online_effects(server, &route.session, CicsOperation::Syncpoint) == 0
    {
        let accounts_restored =
            carddemo_dataset_text_records(server, account_dataset)? == accounts_before;
        let customers_restored =
            carddemo_dataset_text_records(server, customer_dataset)? == customers_before;
        return Err(CorpusProblem::new(
            "carddemo.online.account_rollback_drift",
            format!(
                "accounts_restored={accounts_restored}; customers_restored={customers_restored}; rewrites={}; syncpoints={}; error={:?}; info={:?}; trace={:?}",
                online_effects(server, &route.session, CicsOperation::Rewrite),
                online_effects(server, &route.session, CicsOperation::Syncpoint),
                online_screen_fields(&failed)?.get("ERRMSG"),
                online_screen_fields(&failed)?.get("INFOMSG"),
                server.online_trace(&route.session).unwrap_or_default()
            ),
        ));
    }
    Ok(1)
}

async fn exercise_card_update_mutation(
    server: &ProductServer,
    app: &axum::Router,
) -> Result<usize, CorpusProblem> {
    let card = "0500024453765740";
    let dataset = "AWS.M2.CARDDEMO.CARDDATA.VSAM.KSDS";
    let before = carddemo_dataset_text_records(server, dataset)?;
    let before_record = before
        .iter()
        .find(|record| record.starts_with(card))
        .ok_or_else(|| {
            CorpusProblem::new(
                "carddemo.online.card_update_missing",
                "card update fixture is missing",
            )
        })?;
    let mut expected_record = before_record.as_bytes().to_vec();
    // COCRDUPC reconstructs CARD-UPDATE-RECORD from CCUP-NEW-DETAILS, whose
    // CVV field is initialized but never populated by the source program.
    expected_record[27..30].fill(b' ');
    expected_record[30..80].fill(b' ');
    expected_record[30..44].copy_from_slice(b"CERTIFIED USER");
    let expected_record = String::from_utf8(expected_record).map_err(|_| {
        CorpusProblem::new(
            "carddemo.online.card_update_expected",
            "card update expected record is not UTF-8",
        )
    })?;
    let route = open_carddemo_menu(
        server,
        app,
        "WEBUSER",
        "transport-password",
        "USER0001",
        "PASSWORD",
        "COMEN01",
    )
    .await?;
    let list = carddemo_terminal_exchange(
        app,
        &route.session,
        &route.headers,
        0x7d,
        BTreeMap::from([("OPTION".into(), "3".into())]),
    )
    .await
    .map_err(|problem| {
        CorpusProblem::new("carddemo.online.card_update_list_failed", problem.detail)
    })?;
    require_online_mapset(&list, "COCRDLI", "card update list")?;
    let list_fields = online_screen_fields(&list)?;
    let row = (1..=7)
        .find(|row| {
            list_fields
                .get(&format!("CRDNUM{row}"))
                .is_some_and(|value| String::from_utf8_lossy(value).trim() == card)
        })
        .ok_or_else(|| {
            CorpusProblem::new(
                "carddemo.online.card_update_list",
                "card update fixture is not on the selected card-list page",
            )
        })?;
    let select_field = format!("CRDSEL{row}");
    let session_id = SessionId::new(
        &route.session,
        InvocationLimits::default().max_binding_bytes,
    )
    .map_err(|_| CorpusProblem::new("carddemo.online.response", "session is invalid"))?;
    if server
        .cics_service()
        .terminal_field_protected(
            &session_id,
            &PrincipalId::new("WEBUSER", InvocationLimits::default()).expect("static principal"),
            &select_field,
            0,
        )
        .map_err(terminal_problem)?
    {
        return Err(CorpusProblem::new(
            "carddemo.online.card_update_protection",
            format!("card-list field {select_field} remained protected"),
        ));
    }
    let selected = carddemo_terminal_exchange(
        app,
        &route.session,
        &route.headers,
        0x7d,
        BTreeMap::from([(select_field, "U".into())]),
    )
    .await
    .map_err(|problem| {
        CorpusProblem::new("carddemo.online.card_update_select_failed", problem.detail)
    })?;
    require_online_mapset(&selected, "COCRDUP", "card update selection").map_err(|problem| {
        CorpusProblem::new(
            "carddemo.online.card_update_selection",
            format!(
                "{}; trace={:?}",
                problem.detail,
                server.online_trace(&route.session).unwrap_or_default()
            ),
        )
    })?;
    let fields = online_screen_fields(&selected)?;
    let mut edited = BTreeMap::new();
    for name in ["CRDNAME", "CRDSTCD", "EXPMON", "EXPYEAR"] {
        let value = fields.get(name).ok_or_else(|| {
            CorpusProblem::new(
                "carddemo.online.card_update_values",
                format!("card update field {name} is missing"),
            )
        })?;
        if value.iter().all(|byte| matches!(*byte, 0 | b' ')) {
            return Err(CorpusProblem::new(
                "carddemo.online.card_update_values",
                format!(
                    "card update field {name} is blank; error={:?}; info={:?}; trace={:?}",
                    fields.get("ERRMSG"),
                    fields.get("INFOMSG"),
                    server.online_trace(&route.session).unwrap_or_default()
                ),
            ));
        }
        edited.insert(
            name.to_string(),
            String::from_utf8(value.clone())
                .map_err(|_| {
                    CorpusProblem::new(
                        "carddemo.online.card_update_values",
                        format!("card update field {name} is not UTF-8"),
                    )
                })?
                .trim_end_matches([' ', '\0'])
                .to_string(),
        );
    }
    edited.insert("CRDNAME".into(), "CERTIFIED USER".into());
    let review =
        carddemo_terminal_exchange(app, &route.session, &route.headers, 0x7d, edited.clone())
            .await
            .map_err(|problem| {
                CorpusProblem::new("carddemo.online.card_update_review_failed", problem.detail)
            })?;
    require_online_mapset(&review, "COCRDUP", "card update review")?;
    if online_screen_fields(&review)?
        .get("ERRMSG")
        .is_some_and(|message| message.iter().any(|byte| !matches!(*byte, 0 | b' ')))
    {
        return Err(CorpusProblem::new(
            "carddemo.online.card_update_review",
            "card update review rejected valid source-backed fields",
        ));
    }
    let updated = carddemo_terminal_exchange(app, &route.session, &route.headers, 0xf5, edited)
        .await
        .map_err(|problem| {
            CorpusProblem::new("carddemo.online.card_update_commit_failed", problem.detail)
        })?;
    require_online_mapset(&updated, "COCRDUP", "card update commit")?;
    let after = carddemo_dataset_text_records(server, dataset)?;
    let expected = before
        .iter()
        .map(|record| {
            if record.starts_with(card) {
                expected_record.clone()
            } else {
                record.clone()
            }
        })
        .collect::<Vec<_>>();
    if after != expected || online_effects(server, &route.session, CicsOperation::Rewrite) != 1 {
        let mismatch = after
            .iter()
            .find(|record| record.starts_with(card))
            .map(|actual| {
                actual
                    .as_bytes()
                    .iter()
                    .zip(expected_record.as_bytes())
                    .enumerate()
                    .filter_map(|(offset, (actual, expected))| {
                        (actual != expected).then_some((offset, *actual, *expected))
                    })
                    .take(32)
                    .collect::<Vec<_>>()
            });
        return Err(CorpusProblem::new(
            "carddemo.online.card_update_drift",
            format!(
                "card update exact_state={}; rewrites={}; mismatch={mismatch:?}",
                after == expected,
                online_effects(server, &route.session, CicsOperation::Rewrite)
            ),
        ));
    }
    Ok(1)
}

fn carddemo_dataset_text_records(
    server: &ProductServer,
    dataset: &str,
) -> Result<Vec<String>, CorpusProblem> {
    let result = server
        .dataset_service()
        .invoke(DatasetRequest::Read {
            dataset: DatasetName::new(dataset, 128).map_err(|_| {
                CorpusProblem::new("carddemo.online.dataset_invalid", "dataset name is invalid")
            })?,
            member: None,
            key: None,
            max_records: 4096,
            control: Default::default(),
        })
        .map_err(terminal_problem)?;
    let DatasetResult::Records { records, .. } = result else {
        return Err(CorpusProblem::new(
            "carddemo.online.dataset_response",
            "dataset read returned the wrong result",
        ));
    };
    records
        .into_iter()
        .map(|record| {
            CodePage::Cp037
                .decode(&record, record.len().saturating_mul(4).max(1))
                .map_err(|_| {
                    CorpusProblem::new(
                        "carddemo.online.dataset_decode",
                        "dataset record is not valid CP037",
                    )
                })
        })
        .collect()
}

struct TerminalExercise {
    public_routes: usize,
    protocol_fetches: usize,
    input_submissions: usize,
    authentication_controls: usize,
    csrf_controls: usize,
    restart_resumes: usize,
    disconnects: usize,
    timeout_controls: usize,
    malformed_controls: usize,
    idle_workers: usize,
    map_shapes: Vec<String>,
}

async fn exercise_terminal_routes(
    config: ServerConfig,
    store: Arc<MemoryStore>,
    secrets: Arc<MemorySecretResolver>,
    maps: Vec<BmsMapDefinition>,
    login: BmsMapDefinition,
    input: BmsFieldDefinition,
) -> Result<TerminalExercise, CorpusProblem> {
    let server = ProductServer::open(
        config.clone(),
        store.clone(),
        secrets.clone(),
        default_program_router(),
    )
    .map_err(terminal_problem)?;
    server
        .bootstrap_user("WEBUSER", b"transport-password")
        .map_err(terminal_problem)?;
    let cics = server.cics_service();
    let mut map_shapes = Vec::new();
    for map in maps {
        map_shapes.push(format!(
            "{}/{}:{}/{}:{}",
            map.mapset,
            map.map,
            map.rows,
            map.columns,
            map.fields.len()
        ));
        cics.register_map(map).map_err(terminal_problem)?;
    }
    let app = server.router();
    let basic = format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode("WEBUSER:transport-password")
    );
    let unauthenticated = terminal_http(
        &app,
        Method::POST,
        "/mainframe-env/cics/v1/sessions",
        BTreeMap::from([("x-csrf-zosmf-header".into(), "true".into())]),
        br#"{"transaction":"CC00"}"#.to_vec(),
    )
    .await?;
    if unauthenticated.0 != StatusCode::UNAUTHORIZED {
        return Err(CorpusProblem::new(
            "carddemo.terminal.auth_control",
            "anonymous launch did not fail",
        ));
    }
    let authenticated = terminal_http(
        &app,
        Method::POST,
        "/zosmf/services/authenticate",
        BTreeMap::from([("authorization".into(), basic)]),
        Vec::new(),
    )
    .await?;
    require_terminal_status(authenticated.0, StatusCode::OK, "authenticate")?;
    let authenticated_json: serde_json::Value = serde_json::from_slice(&authenticated.1)
        .map_err(|error| CorpusProblem::new("carddemo.terminal.response", error.to_string()))?;
    let bearer = format!(
        "Bearer {}",
        authenticated_json["token"].as_str().ok_or_else(|| {
            CorpusProblem::new("carddemo.terminal.response", "auth token is missing")
        })?
    );
    let malformed_launch = terminal_http(
        &app,
        Method::POST,
        "/mainframe-env/cics/v1/sessions",
        BTreeMap::from([
            ("authorization".into(), bearer.clone()),
            ("x-csrf-zosmf-header".into(), "true".into()),
        ]),
        b"{".to_vec(),
    )
    .await?;
    if malformed_launch.0 != StatusCode::BAD_REQUEST {
        return Err(CorpusProblem::new(
            "carddemo.terminal.malformed_control",
            "malformed launch body did not fail",
        ));
    }
    let launched = terminal_http(
        &app,
        Method::POST,
        "/mainframe-env/cics/v1/sessions",
        BTreeMap::from([
            ("authorization".into(), bearer.clone()),
            ("x-csrf-zosmf-header".into(), "true".into()),
        ]),
        br#"{"transaction":"CC00","rows":24,"columns":80}"#.to_vec(),
    )
    .await?;
    require_terminal_status(launched.0, StatusCode::CREATED, "launch")?;
    let launched_json: serde_json::Value = serde_json::from_slice(&launched.1)
        .map_err(|error| CorpusProblem::new("carddemo.terminal.response", error.to_string()))?;
    let session = launched_json["session"]
        .as_str()
        .ok_or_else(|| CorpusProblem::new("carddemo.terminal.response", "session is missing"))?
        .to_string();
    let csrf = launched_json["csrf_token"]
        .as_str()
        .ok_or_else(|| CorpusProblem::new("carddemo.terminal.response", "CSRF is missing"))?
        .to_string();
    let run = RunUnitId::new(
        launched_json["terminal"]["run_unit"]
            .as_str()
            .ok_or_else(|| {
                CorpusProblem::new("carddemo.terminal.response", "run unit is missing")
            })?,
        InvocationLimits::default(),
    )
    .map_err(|_| CorpusProblem::new("carddemo.terminal.response", "run unit is invalid"))?;
    let mutation_key = IdempotencyKey::new("carddemo-terminal-send", InvocationLimits::default())
        .expect("static idempotency key");
    let send = CicsRequest {
        operation: CicsOperation::SendMap,
        arguments: BTreeMap::from([
            ("MAPSET".into(), terminal_argument(&login.mapset)?),
            ("MAP".into(), terminal_argument(&login.map)?),
        ]),
        condition_policy: CicsConditionPolicy::Default,
        mutation: Some(Mutation {
            sequence: 1,
            idempotency_key: mutation_key.clone(),
            transaction: Some("CC00".into()),
        }),
    };
    cics.invoke(
        &EffectRequest {
            run_unit: run,
            sequence: 1,
            deadline_tick: 100,
            idempotency_key: Some(mutation_key),
            request: mainframe_env_host_api::HostRequest::Cics(send.clone()),
        },
        send,
    )
    .map_err(terminal_problem)?;
    let session_uri = format!("/mainframe-env/cics/v1/sessions/{session}");
    let screen = terminal_http(
        &app,
        Method::GET,
        &session_uri,
        BTreeMap::from([("authorization".into(), bearer.clone())]),
        Vec::new(),
    )
    .await?;
    require_terminal_status(screen.0, StatusCode::OK, "screen")?;
    let tn_uri = format!("{session_uri}/tn3270");
    let tn_screen = terminal_http(
        &app,
        Method::GET,
        &tn_uri,
        BTreeMap::from([("authorization".into(), bearer.clone())]),
        Vec::new(),
    )
    .await?;
    require_terminal_status(tn_screen.0, StatusCode::OK, "TN3270 screen")?;
    if !tn_screen.1.starts_with(&[0xf5, 0xc3]) {
        return Err(CorpusProblem::new(
            "carddemo.terminal.tn3270_invalid",
            "TN3270 screen does not start with bounded Write/WCC",
        ));
    }
    let address = u16::try_from(
        (usize::from(input.row) - 1) * usize::from(login.columns) + usize::from(input.column) - 1,
    )
    .map_err(|_| CorpusProblem::new("carddemo.terminal.limit", "field address overflow"))?;
    let mut tn_input = vec![0x7d, 0, 0, 0x11, (address >> 8) as u8, address as u8];
    tn_input.extend(std::iter::repeat_n(b'U', usize::from(input.length).min(4)));
    let mutation_headers = BTreeMap::from([
        ("authorization".into(), bearer.clone()),
        ("x-csrf-zosmf-header".into(), "true".into()),
        ("x-csrf-token".into(), csrf.clone()),
    ]);
    let tn_submitted = terminal_http(
        &app,
        Method::PUT,
        &tn_uri,
        mutation_headers.clone(),
        tn_input,
    )
    .await?;
    require_terminal_status(tn_submitted.0, StatusCode::OK, "TN3270 input")?;
    let json_input = serde_json::to_vec(&serde_json::json!({
        "aid":125,
        "fields":{input.name.clone():"USER"}
    }))
    .map_err(|error| CorpusProblem::new("carddemo.terminal.response", error.to_string()))?;
    let input_uri = format!("{session_uri}/input");
    let submitted = terminal_http(
        &app,
        Method::PUT,
        &input_uri,
        mutation_headers.clone(),
        json_input,
    )
    .await?;
    require_terminal_status(submitted.0, StatusCode::OK, "JSON input")?;
    let missing_csrf = terminal_http(
        &app,
        Method::PUT,
        &input_uri,
        BTreeMap::from([
            ("authorization".into(), bearer.clone()),
            ("x-csrf-zosmf-header".into(), "true".into()),
        ]),
        br#"{"aid":125,"fields":{}}"#.to_vec(),
    )
    .await?;
    if missing_csrf.0 != StatusCode::FORBIDDEN {
        return Err(CorpusProblem::new(
            "carddemo.terminal.csrf_control",
            "missing session CSRF did not fail",
        ));
    }
    let wrong_csrf = terminal_http(
        &app,
        Method::PUT,
        &input_uri,
        BTreeMap::from([
            ("authorization".into(), bearer.clone()),
            ("x-csrf-zosmf-header".into(), "true".into()),
            ("x-csrf-token".into(), "wrong".into()),
        ]),
        br#"{"aid":125,"fields":{}}"#.to_vec(),
    )
    .await?;
    if wrong_csrf.0 != StatusCode::FORBIDDEN {
        return Err(CorpusProblem::new(
            "carddemo.terminal.csrf_control",
            "wrong session CSRF did not fail",
        ));
    }
    let malformed = terminal_http(
        &app,
        Method::PUT,
        &tn_uri,
        mutation_headers.clone(),
        vec![0x7d, 0],
    )
    .await?;
    if malformed.0 != StatusCode::BAD_REQUEST {
        return Err(CorpusProblem::new(
            "carddemo.terminal.malformed_control",
            "malformed TN3270 record did not fail",
        ));
    }
    let session_id = SessionId::new(&session, InvocationLimits::default().max_binding_bytes)
        .map_err(|_| CorpusProblem::new("carddemo.terminal.response", "session is invalid"))?;
    let before_expiry = launched_json["terminal"]["expires_at_tick"]
        .as_u64()
        .and_then(|value| value.checked_sub(1))
        .ok_or_else(|| CorpusProblem::new("carddemo.terminal.response", "expiry is invalid"))?;
    if cics.terminal_snapshot(
        &session_id,
        &PrincipalId::new("OTHER", InvocationLimits::default()).expect("static principal"),
        before_expiry,
    ) != Err(mainframe_env_host_api::HostProblem::Unauthorized)
    {
        return Err(CorpusProblem::new(
            "carddemo.terminal.auth_control",
            "cross-principal screen fetch did not fail",
        ));
    }
    if server.metrics().active != 0 || cics.active_worker_count() != 0 {
        return Err(CorpusProblem::new(
            "carddemo.terminal.idle_worker",
            "idle terminal retained an active worker",
        ));
    }
    drop(app);
    drop(cics);
    drop(server);

    let restarted = ProductServer::open(config, store, secrets, default_program_router())
        .map_err(terminal_problem)?;
    let restarted_app = restarted.router();
    let resumed = terminal_http(
        &restarted_app,
        Method::POST,
        &format!("{session_uri}/resume"),
        mutation_headers.clone(),
        Vec::new(),
    )
    .await?;
    require_terminal_status(resumed.0, StatusCode::OK, "restart resume")?;
    let second = terminal_http(
        &restarted_app,
        Method::POST,
        "/mainframe-env/cics/v1/sessions",
        BTreeMap::from([
            ("authorization".into(), bearer.clone()),
            ("x-csrf-zosmf-header".into(), "true".into()),
        ]),
        br#"{"transaction":"CC00"}"#.to_vec(),
    )
    .await?;
    require_terminal_status(second.0, StatusCode::CREATED, "timeout session launch")?;
    let second_json: serde_json::Value = serde_json::from_slice(&second.1)
        .map_err(|error| CorpusProblem::new("carddemo.terminal.response", error.to_string()))?;
    let second_session = SessionId::new(
        second_json["session"].as_str().ok_or_else(|| {
            CorpusProblem::new("carddemo.terminal.response", "second session is missing")
        })?,
        InvocationLimits::default().max_binding_bytes,
    )
    .map_err(|_| CorpusProblem::new("carddemo.terminal.response", "session is invalid"))?;
    let expires = second_json["terminal"]["expires_at_tick"]
        .as_u64()
        .ok_or_else(|| CorpusProblem::new("carddemo.terminal.response", "expiry is missing"))?;
    if restarted.cics_service().terminal_snapshot(
        &second_session,
        &PrincipalId::new("WEBUSER", InvocationLimits::default()).expect("static principal"),
        expires,
    ) != Err(mainframe_env_host_api::HostProblem::TimedOut)
    {
        return Err(CorpusProblem::new(
            "carddemo.terminal.timeout_control",
            "session expiry did not fail closed",
        ));
    }
    let disconnected = terminal_http(
        &restarted_app,
        Method::DELETE,
        &session_uri,
        mutation_headers,
        Vec::new(),
    )
    .await?;
    require_terminal_status(disconnected.0, StatusCode::NO_CONTENT, "disconnect")?;
    Ok(TerminalExercise {
        public_routes: 7,
        protocol_fetches: 2,
        input_submissions: 2,
        authentication_controls: 2,
        csrf_controls: 2,
        restart_resumes: 1,
        disconnects: 1,
        timeout_controls: 1,
        malformed_controls: 2,
        idle_workers: restarted.cics_service().active_worker_count(),
        map_shapes,
    })
}

async fn terminal_http(
    app: &axum::Router,
    method: Method,
    uri: &str,
    headers: BTreeMap<String, String>,
    body: Vec<u8>,
) -> Result<(StatusCode, Vec<u8>), CorpusProblem> {
    let mut request = Request::builder().method(method).uri(uri);
    for (name, value) in headers {
        request = request.header(name, value);
    }
    let response =
        app.clone()
            .oneshot(request.body(Body::from(body)).map_err(|error| {
                CorpusProblem::new("carddemo.terminal.request", error.to_string())
            })?)
            .await
            .map_err(|error| CorpusProblem::new("carddemo.terminal.request", error.to_string()))?;
    let status = response.status();
    let body = to_bytes(response.into_body(), 4 * 1024 * 1024)
        .await
        .map_err(|error| CorpusProblem::new("carddemo.terminal.response", error.to_string()))?;
    Ok((status, body.to_vec()))
}

fn require_terminal_status(
    actual: StatusCode,
    expected: StatusCode,
    operation: &str,
) -> Result<(), CorpusProblem> {
    if actual == expected {
        Ok(())
    } else {
        Err(CorpusProblem::new(
            "carddemo.terminal.status",
            format!("{operation} returned {actual}, expected {expected}"),
        ))
    }
}

fn terminal_argument(value: &str) -> Result<BoundedPayload, CorpusProblem> {
    BoundedPayload::new(
        "mainframe-env.cics.argument@1",
        value.as_bytes().to_vec(),
        InvocationLimits::default(),
    )
    .map_err(|_| CorpusProblem::new("carddemo.terminal.argument", "argument is too large"))
}

fn terminal_problem(problem: mainframe_env_host_api::HostProblem) -> CorpusProblem {
    CorpusProblem::new("carddemo.terminal.provider", problem.to_string())
}

fn screen_contains(screen: &[u8], text: &str) -> bool {
    screen
        .windows(text.len())
        .any(|value| value == text.as_bytes())
        || CodePage::Cp037
            .encode(text, 4096)
            .is_ok_and(|encoded| screen.windows(encoded.len()).any(|value| value == encoded))
}

fn carddemo_base_online_definition(
    corpus_dir: &Path,
) -> Result<OnlineApplicationDefinition, CorpusProblem> {
    let mut bundles = BTreeMap::new();
    for (primary, bundle) in explicit_carddemo_bundles(corpus_dir)? {
        if !primary.starts_with("app/cbl/") {
            continue;
        }
        let name = Path::new(&primary)
            .file_stem()
            .and_then(|value| value.to_str())
            .ok_or_else(|| {
                CorpusProblem::new("carddemo.online.program_invalid", "program name is invalid")
            })?
            .to_ascii_uppercase();
        if bundles.insert(name.clone(), (primary, bundle)).is_some() {
            return Err(CorpusProblem::new(
                "carddemo.online.program_duplicate",
                format!("{name} is duplicated"),
            ));
        }
    }
    let csd = String::from_utf8(read_corpus_file(
        corpus_dir,
        &corpus_dir.join("app/csd/CARDDEMO.CSD"),
    )?)
    .map_err(|_| CorpusProblem::new("carddemo.online.csd_invalid", "base CSD is not UTF-8"))?;
    let resources = parse_csd(&csd).map_err(package_problem)?;
    let mut transactions = BTreeMap::new();
    for resource in resources
        .into_iter()
        .filter(|resource| resource.kind == "TRANSACTION")
    {
        let program = resource.properties.get("PROGRAM").ok_or_else(|| {
            CorpusProblem::new(
                "carddemo.online.csd_invalid",
                format!("{} program is missing", resource.name),
            )
        })?;
        if bundles.contains_key(program) {
            transactions.insert(resource.name, program.clone());
        }
    }
    let mut needed = transactions.values().cloned().collect::<BTreeSet<_>>();
    needed.insert("CSUTLDTC".into());
    let compiler = CobolCompiler::default();
    let mut programs = Vec::new();
    let mut semantic_models = Vec::new();
    for name in &needed {
        let (primary, bundle) = bundles.get(name).ok_or_else(|| {
            CorpusProblem::new(
                "carddemo.online.program_missing",
                format!("{name} source closure is missing"),
            )
        })?;
        let analysis = compiler.analyze(bundle);
        semantic_models.push(analysis.semantic.ok_or_else(|| {
            CorpusProblem::new(
                "carddemo.online.compile_failed",
                format!("{primary}: semantic model is missing"),
            )
        })?);
        let result = compiler
            .compile(CompilerRequest {
                source: bundle.clone(),
                mode: CompilationMode::Executable,
                target: CompileTarget::new("reference").map_err(|error| {
                    CorpusProblem::new("carddemo.online.target_invalid", error.to_string())
                })?,
                options: CompileOptions::new(BTreeMap::new()).map_err(|error| {
                    CorpusProblem::new("carddemo.online.options_invalid", error.to_string())
                })?,
            })
            .map_err(|error| {
                CorpusProblem::new(
                    "carddemo.online.compile_failed",
                    format!("{primary}: {error}"),
                )
            })?;
        let artifact = match result {
            CompilerResult::Published { artifact, .. } => artifact,
            CompilerResult::Analysis { diagnostics, .. }
            | CompilerResult::Failed { diagnostics, .. } => {
                return Err(CorpusProblem::new(
                    "carddemo.online.compile_failed",
                    format!(
                        "{primary}: {}",
                        diagnostics.first().map_or("no diagnostic", |diagnostic| {
                            diagnostic.public_message()
                        })
                    ),
                ));
            }
        };
        programs.push(OnlineProgramDefinition::current(name.clone(), &artifact));
    }
    if programs.len() != 18 || transactions.len() != 17 {
        return Err(CorpusProblem::new(
            "carddemo.online.catalog_drift",
            "base program or source-backed transaction count differs",
        ));
    }
    Ok(OnlineApplicationDefinition {
        programs,
        transactions,
        maps: carddemo_base_maps(corpus_dir, &semantic_models)?,
    })
}

fn carddemo_db2_online_definition(
    corpus_dir: &Path,
) -> Result<OnlineApplicationDefinition, CorpusProblem> {
    let mut definition = carddemo_base_online_definition(corpus_dir)?;
    let bundles = carddemo_db2_bundles(corpus_dir)?
        .into_iter()
        .map(|(relative, bundle)| {
            let name = Path::new(&relative)
                .file_stem()
                .and_then(|value| value.to_str())
                .ok_or_else(|| {
                    CorpusProblem::new("carddemo.db2.program_invalid", "program name is invalid")
                })?
                .to_ascii_uppercase();
            Ok((name, (relative, bundle)))
        })
        .collect::<Result<BTreeMap<_, _>, CorpusProblem>>()?;
    let csd = String::from_utf8(read_corpus_file(
        corpus_dir,
        &corpus_dir.join("app/app-transaction-type-db2/csd/CRDDEMOD.csd"),
    )?)
    .map_err(|_| CorpusProblem::new("carddemo.db2.csd_invalid", "Db2 CSD is not UTF-8"))?;
    let transactions = parse_csd(&csd)
        .map_err(package_problem)?
        .into_iter()
        .filter(|resource| resource.kind == "TRANSACTION")
        .map(|resource| {
            let program = resource.properties.get("PROGRAM").ok_or_else(|| {
                CorpusProblem::new(
                    "carddemo.db2.csd_invalid",
                    format!("{} program is missing", resource.name),
                )
            })?;
            if !bundles.contains_key(program) {
                return Err(CorpusProblem::new(
                    "carddemo.db2.program_missing",
                    format!("{program} source closure is missing"),
                ));
            }
            Ok((resource.name, program.clone()))
        })
        .collect::<Result<BTreeMap<_, _>, CorpusProblem>>()?;
    let compiler = CobolCompiler::default();
    let mut semantic_models = Vec::new();
    for program in transactions.values().collect::<BTreeSet<_>>() {
        let (relative, bundle) = bundles.get(program).ok_or_else(|| {
            CorpusProblem::new(
                "carddemo.db2.program_missing",
                format!("{program} source closure is missing"),
            )
        })?;
        let analysis = compiler.analyze(bundle);
        semantic_models.push(analysis.semantic.ok_or_else(|| {
            CorpusProblem::new(
                "carddemo.db2.compile_failed",
                format!("{relative}: semantic model is missing"),
            )
        })?);
        let result = compiler
            .compile(CompilerRequest {
                source: bundle.clone(),
                mode: CompilationMode::Executable,
                target: CompileTarget::new("reference").expect("static target"),
                options: CompileOptions::new(BTreeMap::new()).expect("static options"),
            })
            .map_err(|problem| {
                CorpusProblem::new(
                    "carddemo.db2.compile_failed",
                    format!("{relative}: {problem:?}"),
                )
            })?;
        let CompilerResult::Published { artifact, .. } = result else {
            return Err(CorpusProblem::new(
                "carddemo.db2.compile_failed",
                format!("{relative} did not publish"),
            ));
        };
        definition.programs.push(OnlineProgramDefinition::current(
            (*program).clone(),
            &artifact,
        ));
    }
    definition.transactions.extend(transactions);
    definition.maps.extend(carddemo_maps(
        corpus_dir,
        &["app/app-transaction-type-db2/bms"],
        &semantic_models,
    )?);
    if definition.programs.len() != 20
        || definition.transactions.len() != 19
        || definition.maps.len() != 19
    {
        return Err(CorpusProblem::new(
            "carddemo.db2.catalog_drift",
            "Db2 application resource counts differ",
        ));
    }
    Ok(definition)
}

fn carddemo_base_seed_objects(corpus_dir: &Path) -> Result<Vec<DatasetSeedObject>, CorpusProblem> {
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
    mappings
        .into_iter()
        .map(|(source, target, record_length, key)| {
            let relative = format!("app/data/EBCDIC/{source}");
            let bytes = read_corpus_file(corpus_dir, &corpus_dir.join(&relative))?;
            Ok(DatasetSeedObject {
                source_id: relative,
                dataset: DatasetName::new(target, 128).map_err(|_| {
                    CorpusProblem::new("carddemo.online.seed_invalid", "seed target is invalid")
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
                sha256: format!("sha256:{:x}", Sha256::digest(&bytes)),
                bytes,
            })
        })
        .collect()
}

fn digest_field(digest: &mut Sha256, bytes: &[u8]) {
    digest.update((bytes.len() as u64).to_be_bytes());
    digest.update(bytes);
}

const LEGACY_COMPATIBILITY_COPYBOOK_CONTRACT: &str =
    "mainframe-env.cobol-compatibility-copybooks@1";

fn subsystem_abi_definitions() -> [HostAbiLibraryDefinition; 3] {
    [cics_abi_library(), db2_abi_library(), mq_abi_library()]
}

fn subsystem_abi_libraries(
    limits: SourceLimits,
) -> Result<MaterializedHostAbiLibraries, CorpusProblem> {
    materialize_host_abi_libraries(&subsystem_abi_definitions(), limits).map_err(|error| {
        CorpusProblem::new(
            "carddemo.abi.invalid",
            format!("subsystem ABI source libraries are invalid: {error}"),
        )
    })
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
    let abi = subsystem_abi_libraries(limits)?;
    let compatibility = abi.files;
    let compatibility_libraries = abi.libraries;
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
    libraries.extend(compatibility_libraries);
    let (dcl_files, dcl_library) = carddemo_db2_dcl_library(corpus_dir, limits)?;
    let mut bundles = Vec::new();
    for primary_path in source_paths {
        let is_db2 = primary_path.starts_with("app/app-transaction-type-db2/cbl/");
        let primary = source_file(corpus_dir, &primary_path, limits)?;
        let mut files = Vec::with_capacity(1 + copybooks.len() + compatibility.len());
        files.push(primary);
        files.extend(copybooks.iter().cloned());
        files.extend(compatibility.iter().cloned());
        let mut bundle_libraries = libraries.clone();
        let mut options = BTreeMap::new();
        if is_db2 {
            files.extend(dcl_files.iter().cloned());
            bundle_libraries.insert(
                bundle_libraries.len().saturating_sub(1),
                dcl_library.clone(),
            );
            options.insert("cobol.sql-precompile".into(), "true".into());
        }
        let logical = LogicalPath::new(&primary_path, limits.max_path_bytes).map_err(|error| {
            CorpusProblem::new(
                "carddemo.layout.closure_invalid",
                format!("program path is invalid: {error}"),
            )
        })?;
        let bundle = SourceBundle::with_libraries(
            &logical,
            files,
            bundle_libraries,
            options,
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

/// Loads the Db2 DCLGEN library shared by every Db2-program bundle.
fn carddemo_db2_dcl_library(
    corpus_dir: &Path,
    limits: SourceLimits,
) -> Result<(Vec<SourceFile>, SourceLibrary), CorpusProblem> {
    let dcl_paths = collect_paths(corpus_dir, &["app/app-transaction-type-db2/dcl"], "dcl")?;
    let dcl_files = dcl_paths
        .iter()
        .map(|path| source_file(corpus_dir, path, limits))
        .collect::<Result<Vec<_>, _>>()?;
    let dcl_members = dcl_paths
        .iter()
        .map(|path| LogicalPath::new(path, limits.max_path_bytes))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|problem| {
            CorpusProblem::new(
                "carddemo.db2.closure_invalid",
                format!("Db2 DCL path is invalid: {problem}"),
            )
        })?;
    let dcl_library = SourceLibrary::new("db2-dcl", dcl_members, limits).map_err(|problem| {
        CorpusProblem::new(
            "carddemo.db2.closure_invalid",
            format!("Db2 DCL library is invalid: {problem}"),
        )
    })?;
    Ok((dcl_files, dcl_library))
}

/// Db2-program bundles, built by `explicit_carddemo_bundles` itself; this is
/// only a filter, so no bundle construction is duplicated.
fn carddemo_db2_bundles(corpus_dir: &Path) -> Result<Vec<(String, SourceBundle)>, CorpusProblem> {
    Ok(explicit_carddemo_bundles(corpus_dir)?
        .into_iter()
        .filter(|(relative, _)| relative.starts_with("app/app-transaction-type-db2/cbl/"))
        .collect())
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

    #[test]
    fn submitted_job_reaches_terminal_state_through_public_route() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let artifact_root = env::temp_dir().join(format!(
            "mainframe-env-carddemo-job-poll-{}-{nonce}",
            std::process::id()
        ));
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let server = ProductServer::open(
                ServerConfig {
                    store_profile: StoreProfile::Memory,
                    artifact_root: artifact_root.clone(),
                    tls: TlsConfig {
                        enabled: false,
                        certificate_path: None,
                        private_key_reference: None,
                    },
                    ..ServerConfig::default()
                },
                Arc::new(MemoryStore::new(Default::default())),
                Arc::new(MemorySecretResolver::default()),
                default_program_router(),
            )
            .unwrap();
            server.bootstrap_user("IBMUSER", b"TESTPASS").unwrap();
            let app = server.router();
            let id = submit_job_with_retcode(
                &server,
                &app,
                "//POLLJOB JOB CLASS=A\n//STEP EXEC PGM=IEFBR14\n",
                "CC 0000",
            )
            .await
            .unwrap();
            assert_eq!(
                server.batch_service().get(&id).unwrap().state,
                JobState::Completed
            );
            drop(app);
            assert!(server.graceful_shutdown().await);
            drop(server);
        });
        let _ = fs::remove_dir_all(artifact_root);
    }

    #[test]
    fn internal_reader_child_reaches_terminal_state_on_background_worker() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let artifact_root = env::temp_dir().join(format!(
            "mainframe-env-carddemo-intrdr-poll-{}-{nonce}",
            std::process::id()
        ));
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let server = ProductServer::open(
                ServerConfig {
                    store_profile: StoreProfile::Memory,
                    artifact_root: artifact_root.clone(),
                    tls: TlsConfig {
                        enabled: false,
                        certificate_path: None,
                        private_key_reference: None,
                    },
                    ..ServerConfig::default()
                },
                Arc::new(MemoryStore::new(Default::default())),
                Arc::new(MemorySecretResolver::default()),
                default_program_router(),
            )
            .unwrap();
            server.bootstrap_user("IBMUSER", b"TESTPASS").unwrap();
            let app = server.router();
            submit_job_with_retcode(
                &server,
                &app,
                "//PARENT JOB CLASS=A\n//SUBMIT EXEC PGM=IEBGENER\n//SYSUT1 DD DATA,DLM=@@\n//CHILD JOB CLASS=A\n//RUN EXEC PGM=IEFBR14\n@@\n//SYSUT2 DD SYSOUT=(A,INTRDR)\n",
                "CC 0000",
            )
            .await
            .unwrap();
            let principal =
                PrincipalId::new("IBMUSER", InvocationLimits::default()).unwrap();
            let child = wait_for_listed_job(
                &server,
                &principal,
                "CHILD",
                16,
                "test.internal_reader.missing",
                "test.internal_reader.incomplete",
            )
            .await
            .unwrap();
            assert_eq!(child.state, JobState::Completed);
            assert_eq!(child.return_code, Some(0));
            drop(app);
            assert!(server.graceful_shutdown().await);
            drop(server);
        });
        let _ = fs::remove_dir_all(artifact_root);
    }

    #[test]
    fn accepted_cdv1_correction_compiles_and_runs_public_route() {
        let inventory = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../conformance/0.1.1/inventory/carddemo-corpus.json");
        let receipt =
            verify_cdv1_correction(&inventory, "59cc6c2fd7ebd7ef7925cad552a01a4b8b6e4d5e").unwrap();
        assert_eq!(receipt.disposition, "accepted-owned-source");
        assert_eq!(receipt.public_routes, 2);
        assert_eq!(
            receipt.source_sha256,
            "426fa98b0ec89b65dd3269d79082db436ecb97a4152bbb2219478c6e1b109981"
        );
        assert_eq!(receipt.artifact_sha256.len(), 64);
        assert_eq!(receipt.screen_sha256.len(), 64);
    }

    #[test]
    #[ignore = "manual Zowe CLI live-test server; stop with Ctrl-C"]
    fn carddemo_zowe_live_server() {
        let address =
            env::var("MAINFRAME_ENV_ZOWE_LIVE_ADDR").unwrap_or_else(|_| "127.0.0.1:10444".into());
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async move {
            let server = ProductServer::memory(ServerConfig {
                store_profile: StoreProfile::Memory,
                artifact_root: env::temp_dir().join(format!(
                    "mainframe-env-carddemo-zowe-live-{}",
                    std::process::id()
                )),
                tls: TlsConfig {
                    enabled: false,
                    certificate_path: None,
                    private_key_reference: None,
                },
                ..ServerConfig::default()
            })
            .unwrap();
            server.bootstrap_user("IBMUSER", b"TESTPASS").unwrap();
            server.start_background_workers().unwrap();
            server
                .racf_service()
                .define_profile("DATASET", "AWS.M2.CARDDEMO.**", "IBMUSER", None)
                .unwrap();
            server
                .racf_service()
                .permit(
                    "DATASET",
                    "AWS.M2.CARDDEMO.**",
                    "IBMUSER",
                    AccessIntent::Alter,
                )
                .unwrap();
            let listener = tokio::net::TcpListener::bind(&address).await.unwrap();
            println!("carddemo-zowe-live-ready {address}");
            axum::serve(listener, server.router())
                .with_graceful_shutdown(async {
                    tokio::signal::ctrl_c().await.unwrap();
                })
                .await
                .unwrap();
            assert!(server.graceful_shutdown().await);
        });
    }

    #[test]
    #[ignore = "requires MAINFRAME_ENV_POSTGRES_TEST_URL pointing at a disposable PostgreSQL 18 database"]
    fn carddemo_full_memory_sqlite_and_postgres_controls() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let exercise = runtime.block_on(exercise_full_certification()).unwrap();
        assert_eq!(exercise.mixed_requests_offered, 16);
        assert_eq!(exercise.mixed_requests_completed, 16);
        assert_eq!(exercise.sqlite_backup_restore_controls, 1);
        assert_eq!(exercise.postgres_restart_controls, 1);
        assert_eq!(exercise.cross_principal_controls, 2);
    }

    struct BundleCorpus {
        root: PathBuf,
    }

    impl BundleCorpus {
        fn create() -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let root = env::temp_dir().join(format!(
                "mainframe-env-carddemo-bundle-corpus-{}-{nonce}-{}",
                std::process::id(),
                NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)
            ));
            for directory in [
                "app/cbl",
                "app/cpy",
                "app/cpy-bms",
                "app/app-authorization-ims-db2-mq/cbl",
                "app/app-authorization-ims-db2-mq/cpy",
                "app/app-authorization-ims-db2-mq/cpy-bms",
                "app/app-transaction-type-db2/cbl",
                "app/app-transaction-type-db2/cpy",
                "app/app-transaction-type-db2/cpy-bms",
                "app/app-transaction-type-db2/dcl",
                "app/app-vsam-mq/cbl",
            ] {
                fs::create_dir_all(root.join(directory)).unwrap();
            }
            fs::write(
                root.join("app/cbl/CORTL01.cbl"),
                b"       IDENTIFICATION DIVISION.\n       PROGRAM-ID. CORTL01.\n",
            )
            .unwrap();
            fs::write(
                root.join("app/app-transaction-type-db2/cbl/COTRTLIC.cbl"),
                b"       IDENTIFICATION DIVISION.\n       PROGRAM-ID. COTRTLIC.\n",
            )
            .unwrap();
            fs::write(
                root.join("app/app-transaction-type-db2/dcl/DCLTRTYP.dcl"),
                b"       01  DCL-TRAN-TYPE.\n           05  DCL-TR-TYPE PIC X(02).\n",
            )
            .unwrap();
            for (directory, name) in [
                ("app/cpy", "CVACT01Y.cpy"),
                ("app/cpy-bms", "CVACT02Y.cpy"),
                ("app/app-authorization-ims-db2-mq/cpy", "CVACT03Y.cpy"),
                ("app/app-authorization-ims-db2-mq/cpy-bms", "CVACT04Y.cpy"),
                ("app/app-transaction-type-db2/cpy", "CVACT05Y.cpy"),
                ("app/app-transaction-type-db2/cpy-bms", "CVACT06Y.cpy"),
            ] {
                fs::write(
                    root.join(directory).join(name),
                    b"       01  DUMMY-COPYBOOK-FIELD PIC X(01).\n",
                )
                .unwrap();
            }
            Self { root }
        }
    }

    impl Drop for BundleCorpus {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn explicit_bundles_apply_db2_dcl_library_and_precompile_option_only_to_db2_programs() {
        let corpus = BundleCorpus::create();
        let bundles = explicit_carddemo_bundles(&corpus.root).unwrap();
        let db2_bundle = bundles
            .iter()
            .find(|(relative, _)| relative == "app/app-transaction-type-db2/cbl/COTRTLIC.cbl")
            .map(|(_, bundle)| bundle)
            .expect("db2 program bundle is present");
        assert_eq!(
            db2_bundle.options().get("cobol.sql-precompile"),
            Some(&"true".to_string()),
            "db2 program bundle must enable SQL precompilation"
        );
        assert!(
            db2_bundle
                .libraries()
                .iter()
                .any(|library| library.name() == "db2-dcl"),
            "db2 program bundle must carry the db2-dcl library"
        );

        let non_db2_bundle = bundles
            .iter()
            .find(|(relative, _)| relative == "app/cbl/CORTL01.cbl")
            .map(|(_, bundle)| bundle)
            .expect("non-db2 program bundle is present");
        assert!(
            !non_db2_bundle
                .options()
                .contains_key("cobol.sql-precompile"),
            "non-db2 program bundle must not enable SQL precompilation"
        );
        assert!(
            !non_db2_bundle
                .libraries()
                .iter()
                .any(|library| library.name() == "db2-dcl"),
            "non-db2 program bundle must not carry the db2-dcl library"
        );
    }

    #[test]
    fn db2_bundles_are_a_filter_over_explicit_bundles_with_no_duplicated_construction() {
        let corpus = BundleCorpus::create();
        let explicit = explicit_carddemo_bundles(&corpus.root).unwrap();
        let db2 = carddemo_db2_bundles(&corpus.root).unwrap();
        let expected: Vec<_> = explicit
            .into_iter()
            .filter(|(relative, _)| relative.starts_with("app/app-transaction-type-db2/cbl/"))
            .collect();
        assert_eq!(db2, expected);
    }
}
