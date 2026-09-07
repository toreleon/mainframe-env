//! Deterministic repository checks for mainframe-env.

#![forbid(unsafe_code)]

mod evidence_seal;
mod jcl_catalog;
mod jcl_conformance;
mod racf_catalog;
mod topic_manifests;
mod work_package_seal;

use clap::{Args, CommandFactory, Parser, Subcommand};
use mainframe_env_conformance::{
    DatasetConformanceRuntime, RACF_ORACLE_RELATIVE_PATH, RacfOracleCampaign,
    dataset_conformance_runtime, gnucobol_reference_fixture_digest, licensed_fixture_digest,
    run_dataset_reference_simulation, run_gnucobol_reference_campaign,
    verify_carddemo_application_package_from_env, verify_carddemo_base_batch_from_env,
    verify_carddemo_base_online_from_env, verify_carddemo_batch_programs_from_env,
    verify_carddemo_cics_abi_from_env, verify_carddemo_cics_runtime_from_env,
    verify_carddemo_control_flow_from_env, verify_carddemo_core_semantics_from_env,
    verify_carddemo_corpus_from_env, verify_carddemo_data_layouts_from_env,
    verify_carddemo_dataset_catalog_from_env, verify_carddemo_db2_from_env,
    verify_carddemo_file_call_semantics_from_env, verify_carddemo_full_from_env,
    verify_carddemo_host_operands_from_env, verify_carddemo_ims_from_env,
    verify_carddemo_jcl_from_env, verify_carddemo_mq_authorization_from_env,
    verify_carddemo_program_routing_from_env, verify_carddemo_resources_from_env,
    verify_carddemo_security_from_env, verify_carddemo_seeds_from_env,
    verify_carddemo_source_closures_from_env, verify_carddemo_source_preprocessing_from_env,
    verify_carddemo_terminal_from_env, verify_carddemo_utilities_from_env,
    verify_carddemo_vsam_from_env, verify_cobol_assurance_sources, verify_cobol_condition_fixtures,
    verify_cobol_data_runtime_fixtures, verify_cobol_exit, verify_cobol_file_runtime_fixtures,
    verify_cobol_frontend_fixtures, verify_cobol_function_boundary_runtime_fixtures,
    verify_cobol_function_fixtures, verify_cobol_function_runtime_fixtures,
    verify_cobol_licensed_receipt_from_env, verify_cobol_recovery_fixtures,
    verify_cobol_register_runtime_fixtures, verify_cobol_semantic_fixtures,
    verify_cobol_statement_fixtures, verify_cobol_statement_phrase_runtime_fixtures,
    verify_cobol_statement_runtime_fixtures, verify_gnucobol_reference_allowlist,
    verify_host_abi_libraries, verify_jcl_exit,
};
use mainframe_env_coverage::{
    BindingKey, CompiledSpec, ConformanceDriver, ConformanceLimits, ConformanceObservation,
    ConformancePredicate, ConformanceRunner, CoverageGate, DerivedConformanceLedger, DriverOutput,
    DriverRef, FixtureRef, GateState, ObligationId, ObservationCheck, ObservationRef,
    OfficialCatalogRow, OfficialRowId, PredicateRef, RunnerContext, RunnerSelection,
    RuntimeRegistry, SpecProblem, TestId, Verdict, VerdictEvent, validate_verdict_batches,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::ffi::OsStr;
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

type TaskResult<T = ()> = Result<T, String>;

const RETAINED_RELEASE_TARGETS: [&str; 2] = ["aarch64-apple-darwin", "x86_64-unknown-linux-gnu"];
const JES_ORACLE_CANDIDATE_DOMAIN: &[u8] = b"mainframe-env.jes-oracle-candidate@1\0";
const JES_ORACLE_DERIVED_PATHS: [&str; 2] = [
    "conformance/0.8/evidence/jes-806-matrix.json",
    "docs/delivery/coverage-versions/status/0.8.0.md",
];

#[derive(Debug, Parser)]
#[command(name = "xtask", disable_version_flag = true)]
struct Cli {
    #[command(subcommand)]
    command: Option<XtaskCommand>,
}

#[derive(Clone, Copy, Debug, Default, Args)]
struct CheckArgs {
    #[arg(long)]
    check: bool,
}

#[derive(Debug, Args)]
struct ReleaseArgs {
    #[arg(long)]
    target: String,
    #[arg(long)]
    check: bool,
}

#[derive(Debug, Args)]
struct EvidenceArgs {
    #[arg(long)]
    check: bool,
    #[command(subcommand)]
    command: Option<EvidenceCommand>,
}

#[derive(Debug, Args)]
struct ConformanceArgs {
    #[arg(long)]
    subsystem: Option<String>,
    #[arg(long)]
    gate: Option<String>,
    #[arg(long)]
    shard: Option<u16>,
    #[arg(long)]
    replay: Option<String>,
    #[arg(long)]
    check: bool,
}

#[derive(Debug, Args)]
struct CobolReferenceArgs {
    #[arg(long)]
    check: bool,
    #[arg(long)]
    receipt: PathBuf,
}

#[derive(Debug, Args)]
struct WorkPackageSealArgs {
    #[arg(long)]
    id: String,
    #[arg(long)]
    target_version: String,
    #[arg(long = "path", required = true)]
    paths: Vec<String>,
    #[arg(long)]
    check: bool,
}

#[derive(Debug, Subcommand)]
enum EvidenceCommand {
    Seal(CheckArgs),
    Callback,
}

#[derive(Debug, Subcommand)]
enum XtaskCommand {
    Versions(CheckArgs),
    Architecture(CheckArgs),
    ArchitectureFast(CheckArgs),
    EvidenceFast(CheckArgs),
    RuntimeArchitecture(CheckArgs),
    Profiles(CheckArgs),
    Schemas(CheckArgs),
    Inventory(CheckArgs),
    Evidence(EvidenceArgs),
    Coverage(CheckArgs),
    ApplicationPackages(CheckArgs),
    Db2Catalog(CheckArgs),
    BatchControllers(CheckArgs),
    AbiLibraries(CheckArgs),
    ProgramRegistry(CheckArgs),
    RouteRegistries(CheckArgs),
    Dehardcoding(CheckArgs),
    LedgerConsistency(CheckArgs),
    MigrationRollback(CheckArgs),
    FullRegression(CheckArgs),
    ReviewRepair(CheckArgs),
    #[command(name = "review-repair-round-2")]
    ReviewRepairRound2(CheckArgs),
    #[command(name = "review-repair-round-3")]
    ReviewRepairRound3(CheckArgs),
    #[command(name = "review-repair-round-4")]
    ReviewRepairRound4(CheckArgs),
    #[command(name = "review-repair-round-5")]
    ReviewRepairRound5(CheckArgs),
    SemanticIdentities(CheckArgs),
    DatasetContract(CheckArgs),
    DatasetOracle(CheckArgs),
    JesOracle(CheckArgs),
    JesOracleCandidate,
    CobolLanguage(CheckArgs),
    CobolExit(CheckArgs),
    CobolReference(CobolReferenceArgs),
    JclCatalog(CheckArgs),
    JclConformance(CheckArgs),
    JclExit(CheckArgs),
    RacfCatalog(CheckArgs),
    Spec(CheckArgs),
    WorkPackageSeal(WorkPackageSealArgs),
    Conformance(ConformanceArgs),
    Certification(CheckArgs),
    CarddemoCorpus(CheckArgs),
    CarddemoSource(CheckArgs),
    CarddemoClosure(CheckArgs),
    CarddemoLayout(CheckArgs),
    CarddemoControl(CheckArgs),
    CarddemoCore(CheckArgs),
    CarddemoFileCall(CheckArgs),
    CarddemoHost(CheckArgs),
    CarddemoPackage(CheckArgs),
    CarddemoResources(CheckArgs),
    CarddemoPrograms(CheckArgs),
    CarddemoCics(CheckArgs),
    CarddemoCicsRuntime(CheckArgs),
    CarddemoVsam(CheckArgs),
    CarddemoDatasetCatalog(CheckArgs),
    CarddemoSeeds(CheckArgs),
    CarddemoSecurity(CheckArgs),
    CarddemoTerminal(CheckArgs),
    CarddemoBaseOnline(CheckArgs),
    CarddemoJcl(CheckArgs),
    CarddemoUtilities(CheckArgs),
    CarddemoBatchPrograms(CheckArgs),
    CarddemoBaseBatch(CheckArgs),
    CarddemoDb2(CheckArgs),
    CarddemoIms(CheckArgs),
    CarddemoMqAuthorization(CheckArgs),
    CarddemoOperatorInstall(CheckArgs),
    CarddemoOperatorCompile(CheckArgs),
    CarddemoOperatorSubmit(CheckArgs),
    CarddemoOperatorReset(CheckArgs),
    CarddemoFull(CheckArgs),
    Digest(CheckArgs),
    Release(ReleaseArgs),
}

fn main() {
    if let Err(error) = run() {
        eprintln!("xtask: {error}");
        std::process::exit(1);
    }
}

fn run() -> TaskResult {
    let root = repository_root()?;
    let Some(command) = Cli::parse().command else {
        Cli::command()
            .print_help()
            .map_err(|error| error.to_string())?;
        println!();
        return Ok(());
    };
    let (name, check, result) = execute_command(&root, command);
    result?;
    if check {
        println!("{name}: pass");
    }
    Ok(())
}

fn execute_command(root: &Path, command: XtaskCommand) -> (&'static str, bool, TaskResult) {
    macro_rules! checked {
        ($name:literal, $args:expr, $call:expr) => {
            ($name, $args.check, $call)
        };
    }
    match command {
        XtaskCommand::Versions(args) => checked!("versions", args, check_versions(root)),
        XtaskCommand::ArchitectureFast(args) => {
            checked!("architecture-fast", args, check_architecture_fast(root))
        }
        XtaskCommand::EvidenceFast(args) => {
            checked!("evidence-fast", args, check_evidence_fast(root))
        }
        XtaskCommand::Architecture(args) => {
            checked!("architecture", args, check_architecture(root))
        }
        XtaskCommand::RuntimeArchitecture(args) => checked!(
            "runtime-architecture",
            args,
            check_runtime_architecture(root)
        ),
        XtaskCommand::Profiles(args) => checked!("profiles", args, check_profiles(root)),
        XtaskCommand::Schemas(args) => checked!("schemas", args, check_schemas(root)),
        XtaskCommand::Inventory(args) => checked!("inventory", args, check_inventory(root)),
        XtaskCommand::Evidence(args) => match (args.check, args.command) {
            (check, None) => ("evidence", check, check_evidence(root)),
            (false, Some(EvidenceCommand::Seal(args))) => (
                "evidence seal",
                args.check,
                if args.check {
                    check_evidence_seal(root)
                } else {
                    generate_evidence_seal(root)
                },
            ),
            (false, Some(EvidenceCommand::Callback)) => {
                ("evidence callback", false, write_evidence_callback(root))
            }
            (true, Some(_)) => (
                "evidence",
                true,
                Err("evidence --check cannot be combined with a nested subcommand".into()),
            ),
        },
        XtaskCommand::Coverage(args) => checked!("coverage", args, check_coverage(root)),
        XtaskCommand::ApplicationPackages(args) => checked!(
            "application-packages",
            args,
            check_application_packages(root)
        ),
        XtaskCommand::Db2Catalog(args) => checked!("db2-catalog", args, check_db2_catalog(root)),
        XtaskCommand::BatchControllers(args) => {
            checked!("batch-controllers", args, check_batch_controllers(root))
        }
        XtaskCommand::AbiLibraries(args) => checked!(
            "abi-libraries",
            args,
            if args.check {
                check_host_abi_libraries(root)
            } else {
                generate_host_abi_inventory(root)
            }
        ),
        XtaskCommand::ProgramRegistry(args) => checked!(
            "program-registry",
            args,
            if args.check {
                check_program_registry(root)
            } else {
                generate_program_registry(root)
            }
        ),
        XtaskCommand::RouteRegistries(args) => checked!(
            "route-registries",
            args,
            if args.check {
                check_route_registries(root)
            } else {
                generate_route_registries(root)
            }
        ),
        XtaskCommand::Dehardcoding(args) => {
            checked!("dehardcoding", args, check_dehardcoding(root))
        }
        XtaskCommand::LedgerConsistency(args) => checked!(
            "ledger-consistency",
            args,
            check_workload_ledger_consistency(root)
        ),
        XtaskCommand::MigrationRollback(args) => checked!(
            "migration-rollback",
            args,
            check_migration_rollback_rollup(root)
        ),
        XtaskCommand::FullRegression(args) => {
            checked!("full-regression", args, check_full_regression(root))
        }
        XtaskCommand::ReviewRepair(args) => {
            checked!("review-repair", args, check_review_repair(root))
        }
        XtaskCommand::ReviewRepairRound2(args) => checked!(
            "review-repair-round-2",
            args,
            check_review_repair_round_2(root)
        ),
        XtaskCommand::ReviewRepairRound3(args) => checked!(
            "review-repair-round-3",
            args,
            check_review_repair_round_3(root)
        ),
        XtaskCommand::ReviewRepairRound4(args) => checked!(
            "review-repair-round-4",
            args,
            check_review_repair_round_4(root)
        ),
        XtaskCommand::ReviewRepairRound5(args) => checked!(
            "review-repair-round-5",
            args,
            check_review_repair_round_5(root)
        ),
        XtaskCommand::SemanticIdentities(args) => checked!(
            "semantic-identities",
            args,
            if args.check {
                check_semantic_identities(root)
            } else {
                generate_semantic_identities(root)
            }
        ),
        XtaskCommand::DatasetContract(args) => checked!(
            "dataset-contract",
            args,
            if args.check {
                check_dataset_contract(root)
            } else {
                generate_dataset_contract(root)
            }
        ),
        XtaskCommand::DatasetOracle(args) => {
            checked!("dataset-oracle", args, check_dataset_oracle(root))
        }
        XtaskCommand::JesOracle(args) => {
            checked!("jes-oracle", args, check_jes_oracle(root))
        }
        XtaskCommand::JesOracleCandidate => (
            "jes-oracle-candidate",
            false,
            print_jes_oracle_candidate(root),
        ),
        XtaskCommand::CobolLanguage(args) => checked!(
            "cobol-language",
            args,
            if args.check {
                check_cobol_language_generated(root)
            } else {
                generate_cobol_language(root)
            }
        ),
        XtaskCommand::CobolExit(args) => checked!("cobol-exit", args, check_cobol_exit(root)),
        XtaskCommand::CobolReference(args) => checked!(
            "cobol-reference",
            args,
            check_cobol_reference(root, &args.receipt)
        ),
        XtaskCommand::JclCatalog(args) => checked!(
            "jcl-catalog",
            args,
            if args.check {
                jcl_catalog::check(root)
            } else {
                jcl_catalog::generate(root)
            }
        ),
        XtaskCommand::JclConformance(args) => checked!(
            "jcl-conformance",
            args,
            if args.check {
                jcl_conformance::check(root)
            } else {
                jcl_conformance::generate(root)
            }
        ),
        XtaskCommand::JclExit(args) => checked!("jcl-exit", args, check_jcl_exit(root)),
        XtaskCommand::RacfCatalog(args) => (
            "racf-catalog",
            args.check,
            if args.check {
                racf_catalog::check(root)
            } else {
                racf_catalog::generate(root)
            },
        ),
        XtaskCommand::Spec(args) => checked!("spec", args, check_spec(root)),
        XtaskCommand::Conformance(args) => {
            let focused = args.subsystem.is_some()
                || args.gate.is_some()
                || args.shard.is_some()
                || args.replay.is_some();
            (
                "conformance",
                args.check,
                if focused {
                    check_focused_conformance_interface(root, &args)
                } else {
                    check_conformance(root)
                },
            )
        }
        XtaskCommand::Certification(args) => {
            checked!("certification", args, check_certification(root))
        }
        XtaskCommand::CarddemoCorpus(args) => {
            checked!("carddemo-corpus", args, check_carddemo_corpus(root))
        }
        XtaskCommand::CarddemoSource(args) => {
            checked!("carddemo-source", args, check_carddemo_source(root))
        }
        XtaskCommand::CarddemoClosure(args) => {
            checked!("carddemo-closure", args, check_carddemo_closure(root))
        }
        XtaskCommand::CarddemoLayout(args) => {
            checked!("carddemo-layout", args, check_carddemo_layout(root))
        }
        XtaskCommand::CarddemoControl(args) => {
            checked!("carddemo-control", args, check_carddemo_control(root))
        }
        XtaskCommand::CarddemoCore(args) => {
            checked!("carddemo-core", args, check_carddemo_core(root))
        }
        XtaskCommand::CarddemoFileCall(args) => {
            checked!("carddemo-file-call", args, check_carddemo_file_call(root))
        }
        XtaskCommand::CarddemoHost(args) => {
            checked!("carddemo-host", args, check_carddemo_host(root))
        }
        XtaskCommand::CarddemoPackage(args) => {
            checked!("carddemo-package", args, check_carddemo_package(root))
        }
        XtaskCommand::CarddemoResources(args) => {
            checked!("carddemo-resources", args, check_carddemo_resources(root))
        }
        XtaskCommand::CarddemoPrograms(args) => {
            checked!("carddemo-programs", args, check_carddemo_programs(root))
        }
        XtaskCommand::CarddemoCics(args) => {
            checked!("carddemo-cics", args, check_carddemo_cics(root))
        }
        XtaskCommand::CarddemoCicsRuntime(args) => checked!(
            "carddemo-cics-runtime",
            args,
            check_carddemo_cics_runtime(root)
        ),
        XtaskCommand::CarddemoVsam(args) => {
            checked!("carddemo-vsam", args, check_carddemo_vsam(root))
        }
        XtaskCommand::CarddemoDatasetCatalog(args) => checked!(
            "carddemo-dataset-catalog",
            args,
            check_carddemo_dataset_catalog(root)
        ),
        XtaskCommand::CarddemoSeeds(args) => {
            checked!("carddemo-seeds", args, check_carddemo_seeds(root))
        }
        XtaskCommand::CarddemoSecurity(args) => {
            checked!("carddemo-security", args, check_carddemo_security(root))
        }
        XtaskCommand::CarddemoTerminal(args) => {
            checked!("carddemo-terminal", args, check_carddemo_terminal(root))
        }
        XtaskCommand::CarddemoBaseOnline(args) => checked!(
            "carddemo-base-online",
            args,
            check_carddemo_base_online(root)
        ),
        XtaskCommand::CarddemoJcl(args) => {
            checked!("carddemo-jcl", args, check_carddemo_jcl(root))
        }
        XtaskCommand::CarddemoUtilities(args) => {
            checked!("carddemo-utilities", args, check_carddemo_utilities(root))
        }
        XtaskCommand::CarddemoBatchPrograms(args) => checked!(
            "carddemo-batch-programs",
            args,
            check_carddemo_batch_programs(root)
        ),
        XtaskCommand::CarddemoBaseBatch(args) => {
            checked!("carddemo-base-batch", args, check_carddemo_base_batch(root))
        }
        XtaskCommand::CarddemoDb2(args) => {
            checked!("carddemo-db2", args, check_carddemo_db2(root))
        }
        XtaskCommand::CarddemoIms(args) => {
            checked!("carddemo-ims", args, check_carddemo_ims(root))
        }
        XtaskCommand::CarddemoMqAuthorization(args) => checked!(
            "carddemo-mq-authorization",
            args,
            check_carddemo_mq_authorization(root)
        ),
        XtaskCommand::CarddemoOperatorInstall(args) => checked!(
            "carddemo-operator-install",
            args,
            check_carddemo_operator_install(root)
        ),
        XtaskCommand::CarddemoOperatorCompile(args) => checked!(
            "carddemo-operator-compile",
            args,
            check_carddemo_operator_compile(root)
        ),
        XtaskCommand::CarddemoOperatorSubmit(args) => checked!(
            "carddemo-operator-submit",
            args,
            check_carddemo_operator_submit(root)
        ),
        XtaskCommand::CarddemoOperatorReset(args) => checked!(
            "carddemo-operator-reset",
            args,
            check_carddemo_operator_reset(root)
        ),
        XtaskCommand::CarddemoFull(args) => {
            checked!("carddemo-full", args, check_carddemo_full(root))
        }
        XtaskCommand::Digest(args) => checked!("digest", args, print_digest(root)),
        XtaskCommand::WorkPackageSeal(args) => (
            "work-package-seal",
            args.check,
            work_package_seal::run(root, &args),
        ),
        XtaskCommand::Release(args) => (
            "release",
            args.check,
            if args.check {
                check_release_artifacts(root, &args.target)
            } else {
                generate_release_artifacts(root, &args.target)
            },
        ),
    }
}

fn check_conformance(root: &Path) -> TaskResult {
    check_spec(root)?;
    racf_catalog::check(root)?;
    jcl_catalog::check(root)?;
    jcl_conformance::check(root)?;
    check_versions(root)?;
    check_architecture(root)?;
    check_profiles(root)?;
    check_schemas(root)?;
    check_inventory(root)?;
    check_evidence(root)?;
    check_coverage(root)?;
    check_semantic_identities(root)?;
    check_application_packages(root)?;
    check_db2_catalog(root)?;
    check_batch_controllers(root)?;
    check_host_abi_libraries(root)?;
    check_program_registry(root)?;
    check_route_registries(root)?;
    check_migration_rollback_rollup(root)?;
    check_full_regression(root)?;
    check_review_repair(root)?;
    check_review_repair_round_2(root)?;
    check_review_repair_round_3(root)?;
    check_review_repair_round_4(root)?;
    check_review_repair_round_5(root)
}

fn check_spec(root: &Path) -> TaskResult {
    let schema_directory = root.join("conformance/spec/schemas");
    let mut schemas = Vec::new();
    collect_extension(&schema_directory, OsStr::new("json"), &mut schemas)?;
    schemas.sort();
    let schema_names = schemas
        .iter()
        .filter_map(|path| path.file_name().and_then(OsStr::to_str))
        .collect::<BTreeSet<_>>();
    require(
        schema_names
            == BTreeSet::from([
                "cobol-language.schema.json",
                "cobol-gnucobol-reference-allowlist.schema.json",
                "cobol-gnucobol-reference-receipt.schema.json",
                "cobol-licensed-differential-adapter.schema.json",
                "cobol-licensed-differential-receipt.schema.json",
                "cobol-condition-fixtures.schema.json",
                "cobol-data-runtime-fixtures.schema.json",
                "cobol-frontend-fixtures.schema.json",
                "cobol-file-runtime-fixtures.schema.json",
                "cobol-function-boundary-runtime-fixtures.schema.json",
                "cobol-function-fixtures.schema.json",
                "cobol-function-runtime-fixtures.schema.json",
                "cobol-register-runtime-fixtures.schema.json",
                "cobol-recovery-fixtures.schema.json",
                "cobol-semantic-fixtures.schema.json",
                "cobol-statement-fixtures.schema.json",
                "cobol-statement-phrase-runtime-fixtures.schema.json",
                "cobol-statement-runtime-fixtures.schema.json",
                "conformance-inventory.schema.json",
                "conformance-spec.schema.json",
                "derived-ledger.schema.json",
                "verdict-event.schema.json",
            ]),
        "Conformance IR/Cobol v1 schema set is incomplete or unexpected",
    )?;
    for schema_path in &schemas {
        let schema = json(schema_path)?;
        compile_draft_2020_12_schema(&schema, schema_path)?;
    }
    let gnucobol_allowlist_path =
        root.join("conformance/0.4/cobol/gnucobol-reference-allowlist.json");
    let gnucobol_allowlist_schema_path =
        schema_directory.join("cobol-gnucobol-reference-allowlist.schema.json");
    validate_schema_instance(
        &json(&gnucobol_allowlist_schema_path)?,
        &json(&gnucobol_allowlist_path)?,
        &gnucobol_allowlist_path,
    )?;
    require(
        verify_gnucobol_reference_allowlist()? == 16
            && gnucobol_reference_fixture_digest().starts_with("sha256:"),
        "approved GnuCOBOL reference allowlist drifted",
    )?;
    let cobol_oracle_path = root.join("conformance/0.4/oracles/cobol-licensed-differential.json");
    let cobol_oracle_schema_path =
        schema_directory.join("cobol-licensed-differential-adapter.schema.json");
    validate_schema_instance(
        &json(&cobol_oracle_schema_path)?,
        &json(&cobol_oracle_path)?,
        &cobol_oracle_path,
    )?;
    let oracle_policy = json(&cobol_oracle_path)?;
    if let Ok(receipt_path) = env::var("MAINFRAME_ENV_COBOL65_LICENSED_ORACLE_RECEIPT") {
        let receipt_path = fs::canonicalize(PathBuf::from(receipt_path))
            .map_err(|error| format!("licensed COBOL receipt: {error}"))?;
        let canonical_root = fs::canonicalize(root).map_err(|error| error.to_string())?;
        require(
            !receipt_path.starts_with(canonical_root),
            "licensed COBOL receipt must remain external to the candidate tree",
        )?;
        validate_schema_instance(
            &json(&schema_directory.join("cobol-licensed-differential-receipt.schema.json"))?,
            &json(&receipt_path)?,
            &receipt_path,
        )?;
        require(
            oracle_policy["status"] == "pass",
            "licensed COBOL receipt is present but the reviewed adapter status is not pass",
        )?;
        require(
            verify_cobol_licensed_receipt_from_env()?,
            "licensed COBOL receipt environment unexpectedly disappeared",
        )?;
    } else {
        require(
            oracle_policy["status"] == "pending",
            "licensed COBOL differential status cannot pass without an external receipt",
        )?;
    }
    let spec_path = root.join("conformance/spec/v1/spec.json");
    let spec_value = json(&spec_path)?;
    let spec_schema_path = schema_directory.join("conformance-spec.schema.json");
    validate_schema_instance(&json(&spec_schema_path)?, &spec_value, &spec_path)?;
    let inventory_path = root.join("conformance/0.3/inventory/dependency-additions.json");
    let inventory_schema_path = schema_directory.join("conformance-inventory.schema.json");
    validate_schema_instance(
        &json(&inventory_schema_path)?,
        &json(&inventory_path)?,
        &inventory_path,
    )?;
    let cobol_path = root.join("conformance/0.3/cobol/language.json");
    let cobol_schema_path = schema_directory.join("cobol-language.schema.json");
    validate_schema_instance(&json(&cobol_schema_path)?, &json(&cobol_path)?, &cobol_path)?;
    check_cobol_language_catalog(root, &cobol_path)?;
    let frontend_path = root.join("conformance/0.3/cobol/frontend-fixtures.json");
    let frontend_schema_path = schema_directory.join("cobol-frontend-fixtures.schema.json");
    validate_schema_instance(
        &json(&frontend_schema_path)?,
        &json(&frontend_path)?,
        &frontend_path,
    )?;
    verify_cobol_frontend_fixtures()?;
    let semantic_path = root.join("conformance/0.3/cobol/semantic-fixtures.json");
    let semantic_schema_path = schema_directory.join("cobol-semantic-fixtures.schema.json");
    validate_schema_instance(
        &json(&semantic_schema_path)?,
        &json(&semantic_path)?,
        &semantic_path,
    )?;
    verify_cobol_semantic_fixtures()?;
    let statement_path = root.join("conformance/0.3/cobol/statement-fixtures.json");
    let statement_schema_path = schema_directory.join("cobol-statement-fixtures.schema.json");
    validate_schema_instance(
        &json(&statement_schema_path)?,
        &json(&statement_path)?,
        &statement_path,
    )?;
    verify_cobol_statement_fixtures()?;
    let statement_runtime_path = root.join("conformance/0.4/cobol/statement-runtime-fixtures.json");
    let statement_runtime_schema_path =
        schema_directory.join("cobol-statement-runtime-fixtures.schema.json");
    validate_schema_instance(
        &json(&statement_runtime_schema_path)?,
        &json(&statement_runtime_path)?,
        &statement_runtime_path,
    )?;
    verify_cobol_statement_runtime_fixtures()?;
    let statement_phrase_path =
        root.join("conformance/0.4/cobol/statement-phrase-runtime-fixtures.json");
    let statement_phrase_schema_path =
        schema_directory.join("cobol-statement-phrase-runtime-fixtures.schema.json");
    validate_schema_instance(
        &json(&statement_phrase_schema_path)?,
        &json(&statement_phrase_path)?,
        &statement_phrase_path,
    )?;
    verify_cobol_statement_phrase_runtime_fixtures()?;
    let data_runtime_path = root.join("conformance/0.4/cobol/data-runtime-fixtures.json");
    let data_runtime_schema_path = schema_directory.join("cobol-data-runtime-fixtures.schema.json");
    validate_schema_instance(
        &json(&data_runtime_schema_path)?,
        &json(&data_runtime_path)?,
        &data_runtime_path,
    )?;
    verify_cobol_data_runtime_fixtures()?;
    let register_runtime_path = root.join("conformance/0.4/cobol/register-runtime-fixtures.json");
    let register_runtime_schema_path =
        schema_directory.join("cobol-register-runtime-fixtures.schema.json");
    validate_schema_instance(
        &json(&register_runtime_schema_path)?,
        &json(&register_runtime_path)?,
        &register_runtime_path,
    )?;
    verify_cobol_register_runtime_fixtures()?;
    let file_runtime_path = root.join("conformance/0.4/cobol/file-runtime-fixtures.json");
    let file_runtime_schema_path = schema_directory.join("cobol-file-runtime-fixtures.schema.json");
    validate_schema_instance(
        &json(&file_runtime_schema_path)?,
        &json(&file_runtime_path)?,
        &file_runtime_path,
    )?;
    verify_cobol_file_runtime_fixtures()?;
    let recovery_path = root.join("conformance/0.4/cobol/recovery-fixtures.json");
    let recovery_schema_path = schema_directory.join("cobol-recovery-fixtures.schema.json");
    validate_schema_instance(
        &json(&recovery_schema_path)?,
        &json(&recovery_path)?,
        &recovery_path,
    )?;
    verify_cobol_recovery_fixtures()?;
    let condition_path = root.join("conformance/0.4/cobol/condition-fixtures.json");
    let condition_schema_path = schema_directory.join("cobol-condition-fixtures.schema.json");
    validate_schema_instance(
        &json(&condition_schema_path)?,
        &json(&condition_path)?,
        &condition_path,
    )?;
    verify_cobol_condition_fixtures()?;
    let function_runtime_path = root.join("conformance/0.4/cobol/function-runtime-fixtures.json");
    let function_runtime_schema_path =
        schema_directory.join("cobol-function-runtime-fixtures.schema.json");
    validate_schema_instance(
        &json(&function_runtime_schema_path)?,
        &json(&function_runtime_path)?,
        &function_runtime_path,
    )?;
    verify_cobol_function_runtime_fixtures()?;
    let function_boundary_path =
        root.join("conformance/0.4/cobol/function-boundary-runtime-fixtures.json");
    let function_boundary_schema_path =
        schema_directory.join("cobol-function-boundary-runtime-fixtures.schema.json");
    validate_schema_instance(
        &json(&function_boundary_schema_path)?,
        &json(&function_boundary_path)?,
        &function_boundary_path,
    )?;
    verify_cobol_function_boundary_runtime_fixtures()?;
    verify_cobol_assurance_sources()?;
    let function_path = root.join("conformance/0.3/cobol/function-fixtures.json");
    let function_schema_path = schema_directory.join("cobol-function-fixtures.schema.json");
    validate_schema_instance(
        &json(&function_schema_path)?,
        &json(&function_path)?,
        &function_path,
    )?;
    verify_cobol_function_fixtures()?;
    check_cobol_language_generated(root)?;
    let spec = compile_shared_spec(root)?;
    if let Ok(receipt_path) = env::var("MAINFRAME_ENV_COBOL65_LICENSED_ORACLE_RECEIPT") {
        let receipt_path = fs::canonicalize(PathBuf::from(receipt_path))
            .map_err(|error| format!("licensed COBOL receipt: {error}"))?;
        let receipt = json(&receipt_path)?;
        let candidate = candidate_digest(root)?;
        let fixtures = licensed_fixture_digest();
        require(
            receipt["candidate_digest"].as_str() == Some(candidate.as_str())
                && receipt["spec_digest"].as_str() == Some(spec.spec_digest())
                && receipt["fixture_digest"].as_str() == Some(fixtures.as_str()),
            "licensed COBOL receipt is for a different candidate, spec, or fixture corpus",
        )?;
    }
    check_dataset_fixture_bindings(root, &spec)?;
    check_cobol_frontend_bindings(root, &spec, &frontend_path)?;
    check_cobol_semantic_bindings(root, &spec, &semantic_path)?;
    check_cobol_statement_bindings(root, &spec, &statement_path)?;
    check_cobol_statement_runtime_bindings(root, &spec, &statement_runtime_path)?;
    check_cobol_statement_phrase_runtime_bindings(&spec, &statement_phrase_path)?;
    check_cobol_function_bindings(root, &spec, &function_path)?;
    check_cobol_function_runtime_bindings(root, &spec, &function_runtime_path)?;
    check_cobol_function_boundary_runtime_bindings(&spec, &function_boundary_path)?;
    check_cobol_data_runtime_bindings(root, &spec, &data_runtime_path)?;
    check_cobol_file_runtime_bindings(root, &spec, &file_runtime_path)?;
    check_cobol_assurance_bindings(
        root,
        &spec,
        [
            ("statement", statement_runtime_path.as_path()),
            ("function", function_runtime_path.as_path()),
            ("data", data_runtime_path.as_path()),
            ("file", file_runtime_path.as_path()),
        ],
    )?;
    check_cobol_recovery_bindings(root, &spec, &recovery_path)?;
    check_cobol_condition_bindings(root, &spec, &condition_path)?;
    check_cobol_reference_policy(root, &spec)?;
    validate_conformance_projections(&schema_directory, &spec)?;
    require(
        !root.join("conformance/spec/verdicts").exists()
            && !root.join("conformance/spec/ledgers").exists(),
        "committed per-run verdict or ledger directories are prohibited",
    )?;
    check_dataset_contract(root)?;
    println!(
        "spec-version={} catalog-rows={} claimed-rows={} obligations={} bindings={} scenarios={} shards={}",
        spec.spec_version(),
        official_catalog_rows(root)?.len(),
        spec.rows().count(),
        spec.obligations().count(),
        spec.cases().count(),
        spec.scenarios().count(),
        spec.expected_shards().len(),
    );
    Ok(())
}

fn check_dataset_fixture_bindings(root: &Path, spec: &CompiledSpec) -> TaskResult {
    let path = root.join("conformance/0.6/fixtures/dataset-organizations.json");
    let fixture = json(&path)?;
    let rows = array(&fixture, "cases", &path)?;
    unique_rows(rows, "id", &path)?;
    require(
        rows.len() == 10,
        "dataset organization fixture denominator must be 10",
    )?;
    let expected_digest = format!("sha256:{}", file_digest(&path)?);
    let expected_ids = rows
        .iter()
        .map(|row| text(row, "id", &path).map(str::to_string))
        .collect::<TaskResult<BTreeSet<_>>>()?;
    let actual = spec
        .registries()
        .fixtures()
        .iter()
        .filter(|(fixture, _)| fixture.as_str().starts_with("dataset."))
        .map(|(fixture, digest)| (fixture.as_str().to_string(), digest.clone()))
        .collect::<BTreeMap<_, _>>();
    require(
        actual.keys().cloned().collect::<BTreeSet<_>>() == expected_ids
            && actual.values().all(|digest| digest == &expected_digest),
        "dataset organization fixture registry is stale or incomplete",
    )?;
    let ams_path = root.join("conformance/0.6/fixtures/ams-commands.json");
    let ams_fixture = json(&ams_path)?;
    let ams_rows = array(&ams_fixture, "cases", &ams_path)?;
    unique_rows(ams_rows, "id", &ams_path)?;
    require(ams_rows.len() == 31, "AMS fixture denominator must be 31")?;
    let ams_digest = format!("sha256:{}", file_digest(&ams_path)?);
    let ams_ids = ams_rows
        .iter()
        .map(|row| text(row, "id", &ams_path).map(str::to_string))
        .collect::<TaskResult<BTreeSet<_>>>()?;
    let actual_ams = spec
        .registries()
        .fixtures()
        .iter()
        .filter(|(fixture, _)| fixture.as_str().starts_with("ams."))
        .map(|(fixture, digest)| (fixture.as_str().to_string(), digest.clone()))
        .collect::<BTreeMap<_, _>>();
    require(
        actual_ams.keys().cloned().collect::<BTreeSet<_>>() == ams_ids
            && actual_ams.values().all(|digest| digest == &ams_digest),
        "AMS fixture registry is stale or incomplete",
    )
}

fn validate_conformance_projections(schema_directory: &Path, spec: &CompiledSpec) -> TaskResult {
    let limits = ConformanceLimits::default();
    let context = RunnerContext::new(
        "sha256:3333333333333333333333333333333333333333333333333333333333333333",
        "schema-smoke",
        limits,
    )
    .map_err(|problem| problem.to_string())?;
    let ledger = DerivedConformanceLedger::derive_partial(spec, &context, Vec::new())
        .map_err(|problem| problem.to_string())?;
    let ledger_path = schema_directory.join("derived-ledger.schema.json");
    let ledger_bytes = ledger
        .canonical_json()
        .map_err(|problem| problem.to_string())?;
    let ledger_value: Value = serde_json::from_slice(&ledger_bytes)
        .map_err(|error| format!("derived ledger projection: {error}"))?;
    validate_schema_instance(&json(&ledger_path)?, &ledger_value, &ledger_path)?;

    let event = VerdictEvent::new(
        spec.spec_version(),
        BindingKey {
            row_id: OfficialRowId::new("schema:smoke:row", limits)
                .map_err(|problem| problem.to_string())?,
            obligation_id: ObligationId::new("schema-smoke", limits)
                .map_err(|problem| problem.to_string())?,
            gate: CoverageGate::Recognized,
        },
        TestId::new("schema.smoke", limits).map_err(|problem| problem.to_string())?,
        Verdict::Fail,
        "sha256:1111111111111111111111111111111111111111111111111111111111111111",
        "sha256:2222222222222222222222222222222222222222222222222222222222222222",
        "schema-smoke-locator",
        DriverRef::new("schema.driver", limits).map_err(|problem| problem.to_string())?,
        FixtureRef::new("schema.fixture", limits).map_err(|problem| problem.to_string())?,
        "expected observation",
        "actual observation",
        None,
        limits,
    )
    .map_err(|problem| problem.to_string())?;
    let verdict_path = schema_directory.join("verdict-event.schema.json");
    let verdict_bytes = event
        .canonical_json()
        .map_err(|problem| problem.to_string())?;
    let verdict_value: Value = serde_json::from_slice(&verdict_bytes)
        .map_err(|error| format!("verdict projection: {error}"))?;
    validate_schema_instance(&json(&verdict_path)?, &verdict_value, &verdict_path)
}

fn check_cobol_language_catalog(root: &Path, path: &Path) -> TaskResult {
    let language = json(path)?;
    let pinned_topics = cobol_language_pinned_topics(root, &language, path)?;
    let official_path = root.join("conformance/0.2/catalogs/cobol.json");
    let official = json(&official_path)?;
    let units = array(&official, "units", &official_path)?;
    let mut official_rows = BTreeMap::new();
    for unit in units {
        let unit_id = text(unit, "id", &official_path)?;
        for row in array(unit, "rows", &official_path)? {
            official_rows.insert(
                text(row, "id", &official_path)?.to_string(),
                (
                    unit_id.to_string(),
                    text(row, "label", &official_path)?.to_string(),
                    text(row, "source_locator", &official_path)?.to_string(),
                ),
            );
        }
    }
    let mut ids = BTreeSet::new();
    for (field, unit, expected) in [
        (
            "compiler_directing_statements",
            "compiler-directing-statements",
            15,
        ),
        ("compiler_directive_groups", "compiler-directive-groups", 5),
        ("procedure_statements", "procedure-statements", 44),
        ("intrinsic_functions", "intrinsic-functions", 82),
        ("file_description_clauses", "file-description-clauses", 10),
        ("data_description_clauses", "data-description-clauses", 17),
    ] {
        let entries = array(&language, field, path)?;
        require(
            entries.len() == expected,
            &format!("COBOL catalog {field} denominator drifted"),
        )?;
        for entry in entries {
            let id = text(entry, "id", path)?;
            require(
                ids.insert(format!("{unit}:{id}")),
                &format!("COBOL catalog repeats identity {unit}:{id}"),
            )?;
            let row_id = text(entry, "row_id", path)?;
            let Some((official_unit, label, locator)) = official_rows.get(row_id) else {
                return Err(format!(
                    "COBOL catalog references unknown official row {row_id}"
                ));
            };
            let catalog_label = entry["label"].as_str().or_else(|| entry["name"].as_str());
            let normalized_label = label
                .split_once(". ")
                .map_or(label.as_str(), |(_, name)| name);
            require(
                official_unit == unit
                    && catalog_label == Some(normalized_label)
                    && entry["source_locator"].as_str() == Some(locator),
                &format!("COBOL catalog drifted from official row {row_id}"),
            )?;
            require_pinned_topic(&pinned_topics, locator, &format!("row {row_id}"))?;
        }
    }
    let special_registers = array(&language, "special_registers", path)?;
    require(
        special_registers.len() == 28,
        "COBOL special-register denominator drifted",
    )?;
    let mut register_names = BTreeSet::new();
    for register in special_registers {
        let name = text(register, "name", path)?;
        require(
            ids.insert(format!("special-registers:{}", text(register, "id", path)?))
                && register_names.insert(name),
            "COBOL special-register identities are duplicated",
        )?;
        require(
            register["runtime_supported"].is_boolean(),
            "COBOL special-register runtime support flag is not boolean",
        )?;
        // A special register carries no row_id, so nothing above compares it to
        // an official row. Its locator is the only claim it makes about the
        // publication, and until it was checked against the pinned manifest the
        // schema regex was the whole of the check: any well-formed nonsense
        // passed.
        let locator = text(register, "source_locator", path)?;
        require_pinned_topic(&pinned_topics, locator, &format!("special register {name}"))?;
        require(
            locator
                .split_once(";heading:")
                .is_some_and(|(_, heading)| heading == name),
            &format!("COBOL special register {name} cites another register's heading"),
        )?;
    }
    Ok(())
}

/// The COBOL topics this repository actually read, for the file that cites them.
///
/// `conformance/0.3/cobol/language.json` names its manifest and the digest it
/// was written against; both must be the ones the 0.2 receipt pins, or the 0.3
/// catalog is describing a book the baselines never read.
fn cobol_language_pinned_topics(
    root: &Path,
    language: &Value,
    path: &Path,
) -> TaskResult<BTreeSet<String>> {
    let source = &language["source"];
    let manifest = text(source, "manifest", path)?;
    let digest = text(source, "sha256", path)?;
    let index_path = root.join("conformance/0.2/catalogs/index.json");
    let index = json(&index_path)?;
    let baseline_id = text(language, "baseline_id", path)?;
    let baseline = array(&index, "baselines", &index_path)?
        .iter()
        .find(|baseline| baseline["id"].as_str() == Some(baseline_id))
        .ok_or_else(|| format!("COBOL catalog cites unknown baseline {baseline_id}"))?;
    require(
        text(&baseline["source"], "manifest", &index_path)? == manifest
            && text(&baseline["source"], "sha256", &index_path)? == digest,
        &format!("COBOL catalog source does not name the manifest baseline {baseline_id} pins"),
    )?;
    topic_manifests::pinned_topic_paths(root, manifest, "the COBOL catalog", digest)
}

fn require_pinned_topic(pinned: &BTreeSet<String>, locator: &str, owner: &str) -> TaskResult {
    let topic = topic_manifests::locator_topic_path(locator)
        .ok_or_else(|| format!("COBOL catalog {owner} has no topic component: {locator}"))?;
    require(
        pinned.contains(topic),
        &format!("COBOL catalog {owner} cites topic {topic}, which is not in the pinned manifest"),
    )
}

fn check_cobol_frontend_bindings(
    root: &Path,
    spec: &CompiledSpec,
    fixture_path: &Path,
) -> TaskResult {
    let fixtures = json(fixture_path)?;
    let language_path = root.join("conformance/0.3/cobol/language.json");
    let language = json(&language_path)?;
    let mut catalog_targets = BTreeMap::new();
    for entry in array(&language, "compiler_directing_statements", &language_path)? {
        catalog_targets.insert(
            text(entry, "row_id", &language_path)?.to_string(),
            (
                "directing".to_string(),
                text(entry, "id", &language_path)?.to_string(),
            ),
        );
    }
    for entry in array(&language, "compiler_directive_groups", &language_path)? {
        catalog_targets.insert(
            text(entry, "row_id", &language_path)?.to_string(),
            (
                "directive-group".to_string(),
                text(entry, "id", &language_path)?.to_string(),
            ),
        );
    }
    let digest = format!("sha256:{}", file_digest(fixture_path)?);
    let mut expected_rows = BTreeSet::new();
    let mut expected_fixtures = BTreeSet::new();
    for fixture in array(&fixtures, "fixtures", fixture_path)? {
        let id = text(fixture, "id", fixture_path)?;
        let row_id = text(fixture, "row_id", fixture_path)?;
        require(
            expected_rows.insert(row_id.to_string()),
            &format!("COBOL frontend repeats official row {row_id}"),
        )?;
        let target = (
            text(fixture, "target_kind", fixture_path)?.to_string(),
            text(fixture, "target_id", fixture_path)?.to_string(),
        );
        require(
            catalog_targets.get(row_id) == Some(&target),
            &format!("COBOL frontend target drifts from generated catalog for {row_id}"),
        )?;
        for suffix in ["valid", "invalid"] {
            expected_fixtures.insert(format!("cobol.frontend.{id}.{suffix}"));
        }
    }
    let actual_rows = spec
        .rows()
        .filter(|row| expected_rows.contains(row.row_id().as_str()))
        .map(|row| row.row_id().as_str().to_string())
        .collect::<BTreeSet<_>>();
    require(
        actual_rows == expected_rows,
        "COBOL frontend fixtures and row specifications are not closed",
    )?;
    for row in spec
        .rows()
        .filter(|row| expected_rows.contains(row.row_id().as_str()))
    {
        let obligations = row
            .obligations()
            .iter()
            .map(|obligation| obligation.as_str())
            .collect::<BTreeSet<_>>();
        require(
            obligations == BTreeSet::from(["valid-forms", "invalid-forms"]),
            &format!("COBOL frontend obligations drifted for {}", row.row_id()),
        )?;
    }
    let actual_fixtures = spec
        .registries()
        .fixtures()
        .iter()
        .filter(|(fixture, _)| fixture.as_str().starts_with("cobol.frontend."))
        .map(|(fixture, fixture_digest)| {
            require(
                fixture_digest == &digest,
                &format!("COBOL frontend fixture digest drifted for {fixture}"),
            )?;
            Ok(fixture.as_str().to_string())
        })
        .collect::<TaskResult<BTreeSet<_>>>()?;
    require(
        actual_fixtures == expected_fixtures,
        "COBOL frontend fixture registry is incomplete or contains stale entries",
    )?;
    let actual_cases = spec
        .cases()
        .filter(|case| case.test_id().as_str().starts_with("cobol.frontend."))
        .map(|case| {
            require(
                case.driver().as_str() == "cobol.frontend.driver"
                    && case.input().as_str() == case.test_id().as_str()
                    && case.preconditions().len() == 1
                    && case.preconditions()[0].as_str() == "cobol.fixture.available"
                    && case.expected().len() == 1,
                &format!("COBOL frontend binding drifted for {}", case.test_id()),
            )?;
            let expected_observation = if case.key().gate == CoverageGate::Recognized {
                "cobol.frontend.accepted"
            } else if case.key().gate == CoverageGate::Validated {
                "cobol.frontend.rejected"
            } else {
                return Err(format!(
                    "COBOL frontend case claims a later gate: {}",
                    case.test_id()
                ));
            };
            require(
                case.expected()[0].as_str() == expected_observation,
                &format!("COBOL frontend expectation drifted for {}", case.test_id()),
            )?;
            Ok(case.test_id().as_str().to_string())
        })
        .collect::<TaskResult<BTreeSet<_>>>()?;
    require(
        actual_cases == expected_fixtures,
        "COBOL frontend executable bindings are incomplete or stale",
    )
}

fn check_cobol_semantic_bindings(
    root: &Path,
    spec: &CompiledSpec,
    fixture_path: &Path,
) -> TaskResult {
    let fixtures = json(fixture_path)?;
    let language_path = root.join("conformance/0.3/cobol/language.json");
    let language = json(&language_path)?;
    let mut catalog_targets = BTreeMap::new();
    for (field, target_kind) in [
        ("file_description_clauses", "file-clause"),
        ("data_description_clauses", "data-clause"),
    ] {
        for entry in array(&language, field, &language_path)? {
            catalog_targets.insert(
                text(entry, "row_id", &language_path)?.to_string(),
                (
                    target_kind.to_string(),
                    text(entry, "id", &language_path)?.to_string(),
                ),
            );
        }
    }
    let digest = format!("sha256:{}", file_digest(fixture_path)?);
    let mut expected_rows = BTreeSet::new();
    let mut expected_fixtures = BTreeSet::new();
    for fixture in array(&fixtures, "fixtures", fixture_path)? {
        let id = text(fixture, "id", fixture_path)?;
        let row_id = text(fixture, "row_id", fixture_path)?;
        require(
            expected_rows.insert(row_id.to_string()),
            &format!("COBOL semantic fixtures repeat official row {row_id}"),
        )?;
        let target = (
            text(fixture, "target_kind", fixture_path)?.to_string(),
            text(fixture, "target_id", fixture_path)?.to_string(),
        );
        require(
            catalog_targets.get(row_id) == Some(&target),
            &format!("COBOL semantic target drifts from generated catalog for {row_id}"),
        )?;
        for suffix in ["valid", "invalid"] {
            expected_fixtures.insert(format!("cobol.semantic.{id}.{suffix}"));
        }
    }
    let actual_rows = spec
        .rows()
        .filter(|row| expected_rows.contains(row.row_id().as_str()))
        .map(|row| row.row_id().as_str().to_string())
        .collect::<BTreeSet<_>>();
    require(
        actual_rows == expected_rows,
        "COBOL semantic fixtures and row specifications are not closed",
    )?;
    for row in spec
        .rows()
        .filter(|row| expected_rows.contains(row.row_id().as_str()))
    {
        let mut expected_obligations =
            if row.row_id().as_str().contains(":data-description-clauses:")
                || row.row_id().as_str().contains(":file-description-clauses:")
            {
                BTreeSet::from([
                    "checkpoint-identity",
                    "invalid-forms",
                    "licensed-equivalence",
                    "quantum-identity",
                    "resource-exhaustion",
                    "runtime-cancellation",
                    "runtime-normal",
                    "valid-forms",
                ])
            } else {
                BTreeSet::from(["valid-forms", "invalid-forms"])
            };
        if row
            .row_id()
            .as_str()
            .ends_with(":data-description-clauses:0002")
        {
            expected_obligations.insert("checkpoint-restart");
            expected_obligations.insert("runtime-condition");
        }
        require(
            row.obligations()
                .iter()
                .map(|obligation| obligation.as_str())
                .collect::<BTreeSet<_>>()
                == expected_obligations,
            &format!("COBOL semantic obligations drifted for {}", row.row_id()),
        )?;
    }
    let actual_fixtures = spec
        .registries()
        .fixtures()
        .iter()
        .filter(|(fixture, _)| fixture.as_str().starts_with("cobol.semantic."))
        .map(|(fixture, fixture_digest)| {
            require(
                fixture_digest == &digest,
                &format!("COBOL semantic fixture digest drifted for {fixture}"),
            )?;
            Ok(fixture.as_str().to_string())
        })
        .collect::<TaskResult<BTreeSet<_>>>()?;
    require(
        actual_fixtures == expected_fixtures,
        "COBOL semantic fixture registry is incomplete or stale",
    )?;
    let actual_cases = spec
        .cases()
        .filter(|case| case.test_id().as_str().starts_with("cobol.semantic."))
        .map(|case| {
            require(
                case.driver().as_str() == "cobol.semantic.driver"
                    && case.input().as_str() == case.test_id().as_str()
                    && case.preconditions().len() == 1
                    && case.preconditions()[0].as_str() == "cobol.semantic.fixture.available"
                    && case.expected().len() == 1,
                &format!("COBOL semantic binding drifted for {}", case.test_id()),
            )?;
            let expected = if case.key().gate == CoverageGate::Recognized {
                "cobol.semantic.accepted"
            } else if case.key().gate == CoverageGate::Validated {
                "cobol.semantic.rejected"
            } else {
                return Err(format!(
                    "COBOL semantic case claims a later gate: {}",
                    case.test_id()
                ));
            };
            require(
                case.expected()[0].as_str() == expected,
                &format!("COBOL semantic expectation drifted for {}", case.test_id()),
            )?;
            Ok(case.test_id().as_str().to_string())
        })
        .collect::<TaskResult<BTreeSet<_>>>()?;
    require(
        actual_cases == expected_fixtures,
        "COBOL semantic executable bindings are incomplete or stale",
    )
}

fn check_cobol_statement_bindings(
    root: &Path,
    spec: &CompiledSpec,
    fixture_path: &Path,
) -> TaskResult {
    let fixtures = json(fixture_path)?;
    let language_path = root.join("conformance/0.3/cobol/language.json");
    let language = json(&language_path)?;
    let catalog_targets = array(&language, "procedure_statements", &language_path)?
        .iter()
        .map(|entry| {
            Ok((
                text(entry, "row_id", &language_path)?.to_string(),
                text(entry, "id", &language_path)?.to_string(),
            ))
        })
        .collect::<TaskResult<BTreeMap<_, _>>>()?;
    let digest = format!("sha256:{}", file_digest(fixture_path)?);
    let mut expected_rows = BTreeSet::new();
    let mut expected_fixtures = BTreeSet::new();
    for fixture in array(&fixtures, "fixtures", fixture_path)? {
        let id = text(fixture, "id", fixture_path)?;
        let row_id = text(fixture, "row_id", fixture_path)?;
        require(
            expected_rows.insert(row_id.to_string()),
            &format!("COBOL statement fixtures repeat official row {row_id}"),
        )?;
        require(
            catalog_targets.get(row_id).map(String::as_str)
                == Some(text(fixture, "target_id", fixture_path)?),
            &format!("COBOL statement target drifts from generated catalog for {row_id}"),
        )?;
        for suffix in ["valid", "invalid"] {
            expected_fixtures.insert(format!("cobol.statement.{id}.{suffix}"));
        }
    }
    let actual_rows = spec
        .rows()
        .filter(|row| expected_rows.contains(row.row_id().as_str()))
        .map(|row| row.row_id().as_str().to_string())
        .collect::<BTreeSet<_>>();
    require(
        actual_rows == expected_rows,
        "COBOL statement fixtures and row specifications are not closed",
    )?;
    for row in spec
        .rows()
        .filter(|row| expected_rows.contains(row.row_id().as_str()))
    {
        let mut expected_obligations = BTreeSet::from([
            "checkpoint-identity",
            "invalid-forms",
            "licensed-equivalence",
            "quantum-identity",
            "resource-exhaustion",
            "runtime-cancellation",
            "runtime-normal",
            "valid-forms",
        ]);
        if matches!(
            row.row_id().as_str().rsplit(':').next(),
            Some("0029" | "0030" | "0031" | "0032" | "0034" | "0036" | "0044")
        ) {
            expected_obligations.insert("checkpoint-restart");
        }
        if matches!(
            row.row_id().as_str().rsplit(':').next(),
            Some(
                "0002"
                    | "0005"
                    | "0012"
                    | "0022"
                    | "0024"
                    | "0030"
                    | "0037"
                    | "0039"
                    | "0041"
                    | "0044"
            )
        ) {
            expected_obligations.insert("runtime-condition");
        }
        match row.row_id().as_str().rsplit(':').next() {
            Some("0002" | "0026") => {
                expected_obligations.insert("phrase-corresponding");
            }
            Some("0003") => {
                expected_obligations.insert("phrase-initialized-linkage");
            }
            Some("0007") => {
                expected_obligations.insert("phrase-multiple-with-lock");
                expected_obligations.insert("phrase-reel-removal-no-rewind");
            }
            Some("0008") => {
                expected_obligations.insert("phrase-rounded-result");
            }
            Some("0011") => {
                expected_obligations.insert("phrase-internal-numeric-compatible-sign");
                expected_obligations.insert("phrase-internal-numeric-separate-sign");
                expected_obligations.insert("phrase-upon-no-advancing");
            }
            Some("0012") => {
                expected_obligations.insert("phrase-giving-remainder");
            }
            Some("0013") => {
                expected_obligations.insert("phrase-alternate-entry");
            }
            Some("0015") => {
                expected_obligations.insert("phrase-paragraph");
                expected_obligations.insert("phrase-perform");
                expected_obligations.insert("phrase-section");
            }
            Some("0020") => {
                expected_obligations.insert("phrase-replacing-then-default");
                expected_obligations.insert("phrase-with-filler");
            }
            Some("0021") => {
                expected_obligations.insert("phrase-leading-first-before-after");
            }
            Some("0023") => {
                expected_obligations.insert("phrase-boolean-null-converting");
                expected_obligations.insert("phrase-conditional-suppress");
                expected_obligations.insert("phrase-condition-name-converting");
                expected_obligations.insert("phrase-count-in");
                expected_obligations.insert("phrase-count-modes");
                expected_obligations.insert("phrase-ebcdic-encoding");
                expected_obligations.insert("phrase-group-hierarchy");
                expected_obligations.insert("phrase-name-overrides");
                expected_obligations.insert("phrase-null-indicator");
                expected_obligations.insert("phrase-occurs-array");
                expected_obligations.insert("phrase-root-name-omitted");
                expected_obligations.insert("phrase-suppress-item");
            }
            Some("0043") => {
                expected_obligations.insert("phrase-count-in");
                expected_obligations.insert("phrase-group-hierarchy");
                expected_obligations.insert("phrase-occurs-elements");
            }
            Some("0024") => {
                expected_obligations.insert("phrase-boolean-null-converting");
                expected_obligations.insert("phrase-condition-name-converting");
                expected_obligations.insert("phrase-ebcdic-encoding");
                expected_obligations.insert("phrase-group-partial-exception");
                expected_obligations.insert("phrase-group-hierarchy");
                expected_obligations.insert("phrase-ignoring-null");
                expected_obligations.insert("phrase-name-overrides");
                expected_obligations.insert("phrase-null-indicator");
                expected_obligations.insert("phrase-null-status");
                expected_obligations.insert("phrase-occurs-array");
                expected_obligations.insert("phrase-root-name-omitted");
                expected_obligations.insert("phrase-suppress-item");
                expected_obligations.insert("phrase-with-detail");
            }
            Some("0029") => {
                expected_obligations.insert("phrase-inline-varying");
                expected_obligations.insert("phrase-out-of-line-times-until-varying");
                expected_obligations.insert("phrase-through");
            }
            Some("0028") => {
                expected_obligations.insert("phrase-multiple-files-and-modes");
            }
            Some("0030") => {
                expected_obligations.insert("phrase-declarative-error");
                expected_obligations.insert("phrase-file-status-error");
                expected_obligations.insert("phrase-lock-wait");
                expected_obligations.insert("phrase-next-record");
            }
            Some("0035") => {
                expected_obligations.insert("phrase-multiple-to");
                expected_obligations.insert("phrase-up-down-by");
            }
            Some("0031") => {
                expected_obligations.insert("phrase-from");
            }
            Some("0032") => {
                expected_obligations.insert("phrase-into-at-end");
            }
            Some("0034") => {
                expected_obligations.insert("phrase-all");
            }
            Some("0039") => {
                expected_obligations.insert("phrase-with-pointer");
            }
            Some("0041") => {
                expected_obligations.insert("phrase-pointer-tally");
            }
            Some("0042") => {
                expected_obligations.insert("phrase-advancing-end-page");
                expected_obligations.insert("phrase-file-status-error");
            }
            Some("0044") => {
                expected_obligations.insert("phrase-attribute-events");
                expected_obligations.insert("phrase-declaration-events");
                expected_obligations.insert("phrase-empty-element-events");
                expected_obligations.insert("phrase-group-hierarchy");
                expected_obligations.insert("phrase-namespace-events");
                expected_obligations.insert("phrase-nested-processing-events");
                expected_obligations.insert("phrase-numeric-character-reference");
                expected_obligations.insert("phrase-occurs-elements");
                expected_obligations.insert("phrase-processing-procedure-through");
            }
            _ => {}
        }
        require(
            row.obligations()
                .iter()
                .map(|obligation| obligation.as_str())
                .collect::<BTreeSet<_>>()
                == expected_obligations,
            &format!("COBOL statement obligations drifted for {}", row.row_id()),
        )?;
    }
    let actual_fixtures = spec
        .registries()
        .fixtures()
        .iter()
        .filter(|(fixture, _)| fixture.as_str().starts_with("cobol.statement."))
        .map(|(fixture, fixture_digest)| {
            require(
                fixture_digest == &digest,
                &format!("COBOL statement fixture digest drifted for {fixture}"),
            )?;
            Ok(fixture.as_str().to_string())
        })
        .collect::<TaskResult<BTreeSet<_>>>()?;
    require(
        actual_fixtures == expected_fixtures,
        "COBOL statement fixture registry is incomplete or contains stale entries",
    )?;
    let actual_cases = spec
        .cases()
        .filter(|case| case.test_id().as_str().starts_with("cobol.statement."))
        .map(|case| {
            require(
                case.driver().as_str() == "cobol.statement.driver"
                    && case.input().as_str() == case.test_id().as_str()
                    && case.preconditions().len() == 1
                    && case.preconditions()[0].as_str() == "cobol.statement.fixture.available"
                    && case.expected().len() == 1,
                &format!("COBOL statement binding drifted for {}", case.test_id()),
            )?;
            let expected = if case.key().gate == CoverageGate::Recognized {
                "cobol.statement.accepted"
            } else if case.key().gate == CoverageGate::Validated {
                "cobol.statement.rejected"
            } else {
                return Err(format!(
                    "COBOL statement case claims a later gate: {}",
                    case.test_id()
                ));
            };
            require(
                case.expected()[0].as_str() == expected,
                &format!("COBOL statement expectation drifted for {}", case.test_id()),
            )?;
            Ok(case.test_id().as_str().to_string())
        })
        .collect::<TaskResult<BTreeSet<_>>>()?;
    require(
        actual_cases == expected_fixtures,
        "COBOL statement executable bindings are incomplete or stale",
    )
}

fn check_cobol_statement_runtime_bindings(
    _root: &Path,
    spec: &CompiledSpec,
    fixture_path: &Path,
) -> TaskResult {
    let fixtures = json(fixture_path)?;
    let digest = format!("sha256:{}", file_digest(fixture_path)?);
    let expected = array(&fixtures, "fixtures", fixture_path)?
        .iter()
        .map(|fixture| {
            Ok((
                text(fixture, "row_id", fixture_path)?.to_string(),
                format!(
                    "cobol.statement-runtime.{}",
                    text(fixture, "id", fixture_path)?
                ),
            ))
        })
        .collect::<TaskResult<BTreeMap<_, _>>>()?;
    require(
        expected.len() == 44,
        "COBOL statement runtime fixture denominator drifted",
    )?;
    let registered = spec
        .registries()
        .fixtures()
        .iter()
        .filter(|(fixture, _)| fixture.as_str().starts_with("cobol.statement-runtime."))
        .map(|(fixture, fixture_digest)| {
            require(
                fixture_digest == &digest,
                &format!("COBOL statement runtime fixture digest drifted for {fixture}"),
            )?;
            Ok(fixture.as_str().to_string())
        })
        .collect::<TaskResult<BTreeSet<_>>>()?;
    require(
        registered == expected.values().cloned().collect(),
        "COBOL statement runtime fixture registry is incomplete or stale",
    )?;
    let cases = spec
        .cases()
        .filter(|case| {
            case.test_id()
                .as_str()
                .starts_with("cobol.statement-runtime.")
        })
        .map(|case| {
            let fixture = expected.get(case.key().row_id.as_str()).ok_or_else(|| {
                format!("unknown COBOL statement runtime row {}", case.key().row_id)
            })?;
            require(
                case.key().gate == CoverageGate::Executed
                    && case.key().obligation_id.as_str() == "runtime-normal"
                    && case.test_id().as_str() == fixture
                    && case.input().as_str() == fixture
                    && case.driver().as_str() == "cobol.statement-runtime.driver"
                    && case.preconditions().len() == 1
                    && case.preconditions()[0].as_str()
                        == "cobol.statement-runtime.fixture.available"
                    && case.expected().len() == 1
                    && case.expected()[0].as_str() == "cobol.statement-runtime.executed"
                    && case.recovery().is_none()
                    && has_cobol_licensed_oracle(case),
                &format!("COBOL statement runtime binding drifted for {fixture}"),
            )?;
            Ok(case.key().row_id.as_str().to_string())
        })
        .collect::<TaskResult<BTreeSet<_>>>()?;
    require(
        cases == expected.keys().cloned().collect(),
        "COBOL statement runtime cases are incomplete or stale",
    )?;
    for row in spec
        .rows()
        .filter(|row| expected.contains_key(row.row_id().as_str()))
    {
        require(
            row.operation().as_str() == "cobol.statement.semantics"
                && row.transition().as_str() == "cobol.statement.compile-and-execute"
                && row.input().as_str() == "cobol.statement-runtime.artifact"
                && row
                    .obligations()
                    .iter()
                    .any(|obligation| obligation.as_str() == "runtime-normal"),
            &format!(
                "COBOL statement runtime row contract drifted for {}",
                row.row_id()
            ),
        )?;
    }
    Ok(())
}

fn check_cobol_statement_phrase_runtime_bindings(
    spec: &CompiledSpec,
    fixture_path: &Path,
) -> TaskResult {
    let fixtures = json(fixture_path)?;
    let digest = format!("sha256:{}", file_digest(fixture_path)?);
    let expected = array(&fixtures, "fixtures", fixture_path)?
        .iter()
        .map(|fixture| {
            let id = text(fixture, "id", fixture_path)?;
            Ok((
                format!("cobol.statement-phrase-runtime.{id}"),
                (
                    text(fixture, "row_id", fixture_path)?.to_string(),
                    text(fixture, "obligation_id", fixture_path)?.to_string(),
                ),
            ))
        })
        .collect::<TaskResult<BTreeMap<_, _>>>()?;
    require(
        !expected.is_empty() && expected.len() <= 256,
        "COBOL statement phrase fixture denominator drifted",
    )?;
    let registered = spec
        .registries()
        .fixtures()
        .iter()
        .filter(|(fixture, _)| {
            fixture
                .as_str()
                .starts_with("cobol.statement-phrase-runtime.")
        })
        .map(|(fixture, fixture_digest)| {
            require(
                fixture_digest == &digest,
                &format!("COBOL statement phrase fixture digest drifted for {fixture}"),
            )?;
            Ok(fixture.as_str().to_string())
        })
        .collect::<TaskResult<BTreeSet<_>>>()?;
    require(
        registered == expected.keys().cloned().collect(),
        "COBOL statement phrase fixture registry is incomplete or stale",
    )?;
    let cases = spec
        .cases()
        .filter(|case| {
            case.test_id()
                .as_str()
                .starts_with("cobol.statement-phrase-runtime.")
        })
        .map(|case| {
            let (row_id, obligation) = expected
                .get(case.test_id().as_str())
                .ok_or_else(|| format!("unknown COBOL phrase case {}", case.test_id()))?;
            require(
                case.key().row_id.as_str() == row_id
                    && case.key().gate == CoverageGate::Executed
                    && case.key().obligation_id.as_str() == obligation
                    && case.input().as_str() == case.test_id().as_str()
                    && case.driver().as_str() == "cobol.statement-phrase-runtime.driver"
                    && case.preconditions().len() == 1
                    && case.preconditions()[0].as_str()
                        == "cobol.statement-phrase-runtime.fixture.available"
                    && case.expected().len() == 1
                    && case.expected()[0].as_str() == "cobol.statement-phrase-runtime.executed"
                    && case.recovery().is_none()
                    && has_cobol_licensed_oracle(case),
                &format!(
                    "COBOL statement phrase binding drifted for {}",
                    case.test_id()
                ),
            )?;
            Ok(case.test_id().as_str().to_string())
        })
        .collect::<TaskResult<BTreeSet<_>>>()?;
    require(
        cases == expected.keys().cloned().collect(),
        "COBOL statement phrase cases are incomplete or stale",
    )
}

fn check_cobol_function_bindings(
    root: &Path,
    spec: &CompiledSpec,
    fixture_path: &Path,
) -> TaskResult {
    let fixtures = json(fixture_path)?;
    let language_path = root.join("conformance/0.3/cobol/language.json");
    let language = json(&language_path)?;
    let catalog = array(&language, "intrinsic_functions", &language_path)?
        .iter()
        .map(|entry| {
            Ok((
                text(entry, "row_id", &language_path)?.to_string(),
                (
                    text(entry, "id", &language_path)?.to_string(),
                    text(entry, "name", &language_path)?.to_string(),
                ),
            ))
        })
        .collect::<TaskResult<BTreeMap<_, _>>>()?;
    let digest = format!("sha256:{}", file_digest(fixture_path)?);
    let mut expected_rows = BTreeSet::new();
    let mut expected_fixtures = BTreeSet::new();
    for fixture in array(&fixtures, "fixtures", fixture_path)? {
        let id = text(fixture, "id", fixture_path)?;
        let row_id = text(fixture, "row_id", fixture_path)?;
        require(
            expected_rows.insert(row_id.to_string()),
            &format!("COBOL function fixtures repeat official row {row_id}"),
        )?;
        require(
            catalog.get(row_id)
                == Some(&(
                    id.to_string(),
                    text(fixture, "name", fixture_path)?.to_string(),
                )),
            &format!("COBOL function target drifts from generated catalog for {row_id}"),
        )?;
        for suffix in ["valid", "invalid"] {
            expected_fixtures.insert(format!("cobol.function.{id}.{suffix}"));
        }
    }
    let actual_rows = spec
        .rows()
        .filter(|row| expected_rows.contains(row.row_id().as_str()))
        .map(|row| row.row_id().as_str().to_string())
        .collect::<BTreeSet<_>>();
    require(
        actual_rows == expected_rows,
        "COBOL function fixtures and row specifications are not closed",
    )?;
    for row in spec
        .rows()
        .filter(|row| expected_rows.contains(row.row_id().as_str()))
    {
        let mut expected_obligations = BTreeSet::from([
            "checkpoint-identity",
            "invalid-signature",
            "licensed-equivalence",
            "quantum-identity",
            "resource-exhaustion",
            "runtime-cancellation",
            "runtime-normal",
            "valid-signature",
        ]);
        if row.row_id().as_str().ends_with(":0053") {
            expected_obligations.insert("checkpoint-restart");
        }
        if matches!(
            row.row_id().as_str().rsplit(':').next(),
            Some(
                "0002"
                    | "0003"
                    | "0004"
                    | "0009"
                    | "0022"
                    | "0035"
                    | "0036"
                    | "0043"
                    | "0045"
                    | "0046"
                    | "0047"
                    | "0052"
                    | "0055"
                    | "0061"
                    | "0075"
            )
        ) {
            expected_obligations.insert("runtime-condition");
        }
        match row.row_id().as_str().rsplit(':').next() {
            Some("0006") => {
                expected_obligations.insert("boundary-packed-storage-bits");
            }
            Some("0008") => {
                expected_obligations.insert("boundary-national-byte-count");
                expected_obligations.insert("boundary-packed-storage-byte-count");
            }
            Some("0011") => {
                expected_obligations.insert("boundary-numeric-type-preservation");
            }
            Some("0015" | "0017") => {
                expected_obligations.insert("boundary-sliding-century");
            }
            Some("0018") => {
                expected_obligations.insert("boundary-explicit-output-ccsid");
            }
            Some("0027") => {
                expected_obligations.insert("boundary-packed-storage-hex");
            }
            Some("0024") => {
                expected_obligations.insert("boundary-integer-date-epoch");
            }
            Some("0025") => {
                expected_obligations.insert("boundary-integer-date-optional-offset");
            }
            Some("0026") => {
                expected_obligations.insert("boundary-optional-offset");
            }
            Some("0034") => {
                expected_obligations.insert("boundary-national-character-count");
                expected_obligations.insert("boundary-packed-storage-length");
            }
            Some("0037" | "0074") => {
                expected_obligations.insert("boundary-national-case-map");
            }
            Some("0041" | "0054") => {
                expected_obligations.insert("boundary-wide-decimal-exactness");
            }
            Some("0046") => {
                expected_obligations.insert("boundary-currency-symbol");
                expected_obligations.insert("boundary-default-currency");
            }
            Some("0044") => {
                expected_obligations.insert("boundary-explicit-input-ccsid");
            }
            Some("0053") => {
                expected_obligations.insert("boundary-sequence-and-reseed");
            }
            Some("0056") => {
                expected_obligations.insert("boundary-national-character-reversal");
            }
            Some("0067" | "0068") => {
                expected_obligations.insert("boundary-error-position");
                if row.row_id().as_str().ends_with(":0067") {
                    expected_obligations.insert("boundary-range-error-position");
                }
            }
            Some("0065" | "0066") => {
                expected_obligations.insert("boundary-error-subfield");
            }
            Some("0069") => {
                expected_obligations.insert("boundary-currency-position");
                expected_obligations.insert("boundary-default-currency-position");
            }
            Some("0071") => {
                expected_obligations.insert("boundary-national-space-trim");
            }
            Some("0072") => {
                expected_obligations.insert("boundary-byte-aligned-slice");
            }
            Some("0073") => {
                expected_obligations.insert("boundary-multibyte-position");
            }
            Some("0076") => {
                expected_obligations.insert("boundary-first-supplementary-index");
            }
            Some("0079") => {
                expected_obligations.insert("boundary-multibyte-width");
            }
            Some("0082") => {
                expected_obligations.insert("boundary-default-sliding-century");
            }
            _ => {}
        }
        require(
            row.obligations()
                .iter()
                .map(|obligation| obligation.as_str())
                .collect::<BTreeSet<_>>()
                == expected_obligations,
            &format!("COBOL function obligations drifted for {}", row.row_id()),
        )?;
    }
    let actual_fixtures = spec
        .registries()
        .fixtures()
        .iter()
        .filter(|(fixture, _)| fixture.as_str().starts_with("cobol.function."))
        .map(|(fixture, fixture_digest)| {
            require(
                fixture_digest == &digest,
                &format!("COBOL function fixture digest drifted for {fixture}"),
            )?;
            Ok(fixture.as_str().to_string())
        })
        .collect::<TaskResult<BTreeSet<_>>>()?;
    require(
        actual_fixtures == expected_fixtures,
        "COBOL function fixture registry is incomplete or contains stale entries",
    )?;
    let actual_cases = spec
        .cases()
        .filter(|case| case.test_id().as_str().starts_with("cobol.function."))
        .map(|case| {
            require(
                case.driver().as_str() == "cobol.function.driver"
                    && case.input().as_str() == case.test_id().as_str()
                    && case.preconditions().len() == 1
                    && case.preconditions()[0].as_str() == "cobol.function.fixture.available"
                    && case.expected().len() == 1,
                &format!("COBOL function binding drifted for {}", case.test_id()),
            )?;
            let expected = if case.key().gate == CoverageGate::Recognized {
                "cobol.function.accepted"
            } else if case.key().gate == CoverageGate::Validated {
                "cobol.function.rejected"
            } else {
                return Err(format!(
                    "COBOL function case claims a later gate: {}",
                    case.test_id()
                ));
            };
            require(
                case.expected()[0].as_str() == expected,
                &format!("COBOL function expectation drifted for {}", case.test_id()),
            )?;
            Ok(case.test_id().as_str().to_string())
        })
        .collect::<TaskResult<BTreeSet<_>>>()?;
    require(
        actual_cases == expected_fixtures,
        "COBOL function executable bindings are incomplete or stale",
    )
}

fn check_cobol_function_runtime_bindings(
    _root: &Path,
    spec: &CompiledSpec,
    fixture_path: &Path,
) -> TaskResult {
    let fixtures = json(fixture_path)?;
    let digest = format!("sha256:{}", file_digest(fixture_path)?);
    let expected = array(&fixtures, "fixtures", fixture_path)?
        .iter()
        .map(|fixture| {
            Ok((
                text(fixture, "row_id", fixture_path)?.to_string(),
                format!(
                    "cobol.function-runtime.{}",
                    text(fixture, "id", fixture_path)?
                ),
            ))
        })
        .collect::<TaskResult<BTreeMap<_, _>>>()?;
    require(
        expected.len() == 82,
        "COBOL function runtime fixture denominator drifted",
    )?;
    let registered = spec
        .registries()
        .fixtures()
        .iter()
        .filter(|(fixture, _)| fixture.as_str().starts_with("cobol.function-runtime."))
        .map(|(fixture, fixture_digest)| {
            require(
                fixture_digest == &digest,
                &format!("COBOL function runtime fixture digest drifted for {fixture}"),
            )?;
            Ok(fixture.as_str().to_string())
        })
        .collect::<TaskResult<BTreeSet<_>>>()?;
    require(
        registered == expected.values().cloned().collect(),
        "COBOL function runtime fixture registry is incomplete or stale",
    )?;
    let cases = spec
        .cases()
        .filter(|case| {
            case.test_id()
                .as_str()
                .starts_with("cobol.function-runtime.")
        })
        .map(|case| {
            let fixture = expected.get(case.key().row_id.as_str()).ok_or_else(|| {
                format!("unknown COBOL function runtime row {}", case.key().row_id)
            })?;
            require(
                case.key().gate == CoverageGate::Executed
                    && case.key().obligation_id.as_str() == "runtime-normal"
                    && case.test_id().as_str() == fixture
                    && case.input().as_str() == fixture
                    && case.driver().as_str() == "cobol.function-runtime.driver"
                    && case.preconditions().len() == 1
                    && case.preconditions()[0].as_str()
                        == "cobol.function-runtime.fixture.available"
                    && case.expected().len() == 1
                    && case.expected()[0].as_str() == "cobol.function-runtime.executed"
                    && case.recovery().is_none()
                    && has_cobol_licensed_oracle(case),
                &format!("COBOL function runtime binding drifted for {fixture}"),
            )?;
            Ok(case.key().row_id.as_str().to_string())
        })
        .collect::<TaskResult<BTreeSet<_>>>()?;
    require(
        cases == expected.keys().cloned().collect(),
        "COBOL function runtime cases are incomplete or stale",
    )?;
    for row in spec
        .rows()
        .filter(|row| expected.contains_key(row.row_id().as_str()))
    {
        require(
            row.operation().as_str() == "cobol.function.semantics"
                && row.transition().as_str() == "cobol.function.compile-and-execute"
                && row.input().as_str() == "cobol.function-runtime.artifact"
                && row
                    .obligations()
                    .iter()
                    .any(|obligation| obligation.as_str() == "runtime-normal"),
            &format!(
                "COBOL function runtime row contract drifted for {}",
                row.row_id()
            ),
        )?;
    }
    Ok(())
}

fn check_cobol_function_boundary_runtime_bindings(
    spec: &CompiledSpec,
    fixture_path: &Path,
) -> TaskResult {
    let fixtures = json(fixture_path)?;
    let digest = format!("sha256:{}", file_digest(fixture_path)?);
    let expected = array(&fixtures, "fixtures", fixture_path)?
        .iter()
        .map(|fixture| {
            let id = text(fixture, "id", fixture_path)?;
            Ok((
                format!("cobol.function-boundary-runtime.{id}"),
                (
                    text(fixture, "row_id", fixture_path)?.to_string(),
                    text(fixture, "obligation_id", fixture_path)?.to_string(),
                ),
            ))
        })
        .collect::<TaskResult<BTreeMap<_, _>>>()?;
    require(
        !expected.is_empty() && expected.len() <= 512,
        "COBOL function boundary fixture denominator drifted",
    )?;
    let registered = spec
        .registries()
        .fixtures()
        .iter()
        .filter(|(fixture, _)| {
            fixture
                .as_str()
                .starts_with("cobol.function-boundary-runtime.")
        })
        .map(|(fixture, fixture_digest)| {
            require(
                fixture_digest == &digest,
                &format!("COBOL function boundary fixture digest drifted for {fixture}"),
            )?;
            Ok(fixture.as_str().to_string())
        })
        .collect::<TaskResult<BTreeSet<_>>>()?;
    require(
        registered == expected.keys().cloned().collect(),
        "COBOL function boundary fixture registry is incomplete or stale",
    )?;
    let cases = spec
        .cases()
        .filter(|case| {
            case.test_id()
                .as_str()
                .starts_with("cobol.function-boundary-runtime.")
        })
        .map(|case| {
            let (row_id, obligation) = expected.get(case.test_id().as_str()).ok_or_else(|| {
                format!("unknown COBOL function boundary case {}", case.test_id())
            })?;
            require(
                case.key().row_id.as_str() == row_id
                    && case.key().gate == CoverageGate::Executed
                    && case.key().obligation_id.as_str() == obligation
                    && case.input().as_str() == case.test_id().as_str()
                    && case.driver().as_str() == "cobol.function-boundary-runtime.driver"
                    && case.preconditions().len() == 1
                    && case.preconditions()[0].as_str()
                        == "cobol.function-boundary-runtime.fixture.available"
                    && case.expected().len() == 1
                    && case.expected()[0].as_str() == "cobol.function-boundary-runtime.executed"
                    && case.recovery().is_none()
                    && has_cobol_licensed_oracle(case),
                &format!(
                    "COBOL function boundary binding drifted for {}",
                    case.test_id()
                ),
            )?;
            Ok(case.test_id().as_str().to_string())
        })
        .collect::<TaskResult<BTreeSet<_>>>()?;
    require(
        cases == expected.keys().cloned().collect(),
        "COBOL function boundary cases are incomplete or stale",
    )
}

fn check_cobol_data_runtime_bindings(
    _root: &Path,
    spec: &CompiledSpec,
    fixture_path: &Path,
) -> TaskResult {
    let fixtures = json(fixture_path)?;
    let digest = format!("sha256:{}", file_digest(fixture_path)?);
    let expected = array(&fixtures, "fixtures", fixture_path)?
        .iter()
        .map(|fixture| {
            Ok((
                text(fixture, "row_id", fixture_path)?.to_string(),
                format!("cobol.data-runtime.{}", text(fixture, "id", fixture_path)?),
            ))
        })
        .collect::<TaskResult<BTreeMap<_, _>>>()?;
    require(
        expected.len() == 17,
        "COBOL data runtime fixture denominator drifted",
    )?;
    let registered = spec
        .registries()
        .fixtures()
        .iter()
        .filter(|(fixture, _)| fixture.as_str().starts_with("cobol.data-runtime."))
        .map(|(fixture, fixture_digest)| {
            require(
                fixture_digest == &digest,
                &format!("COBOL data runtime fixture digest drifted for {fixture}"),
            )?;
            Ok(fixture.as_str().to_string())
        })
        .collect::<TaskResult<BTreeSet<_>>>()?;
    require(
        registered == expected.values().cloned().collect(),
        "COBOL data runtime fixture registry is incomplete or stale",
    )?;
    let cases = spec
        .cases()
        .filter(|case| case.test_id().as_str().starts_with("cobol.data-runtime."))
        .map(|case| {
            let fixture = expected
                .get(case.key().row_id.as_str())
                .ok_or_else(|| format!("unknown COBOL data runtime row {}", case.key().row_id))?;
            require(
                case.key().gate == CoverageGate::Executed
                    && case.key().obligation_id.as_str() == "runtime-normal"
                    && case.test_id().as_str() == fixture
                    && case.input().as_str() == fixture
                    && case.driver().as_str() == "cobol.data-runtime.driver"
                    && case.preconditions().len() == 1
                    && case.preconditions()[0].as_str() == "cobol.data-runtime.fixture.available"
                    && case.expected().len() == 1
                    && case.expected()[0].as_str() == "cobol.data-runtime.executed"
                    && case.recovery().is_none()
                    && has_cobol_licensed_oracle(case),
                &format!("COBOL data runtime binding drifted for {fixture}"),
            )?;
            Ok(case.key().row_id.as_str().to_string())
        })
        .collect::<TaskResult<BTreeSet<_>>>()?;
    require(
        cases == expected.keys().cloned().collect(),
        "COBOL data runtime cases are incomplete or stale",
    )?;
    for row in spec
        .rows()
        .filter(|row| expected.contains_key(row.row_id().as_str()))
    {
        require(
            row.operation().as_str() == "cobol.data.semantics"
                && row.transition().as_str() == "cobol.data.compile-and-execute"
                && row.input().as_str() == "cobol.data-runtime.artifact"
                && row
                    .obligations()
                    .iter()
                    .any(|obligation| obligation.as_str() == "runtime-normal"),
            &format!(
                "COBOL data runtime row contract drifted for {}",
                row.row_id()
            ),
        )?;
    }
    Ok(())
}

fn check_cobol_file_runtime_bindings(
    _root: &Path,
    spec: &CompiledSpec,
    fixture_path: &Path,
) -> TaskResult {
    let fixtures = json(fixture_path)?;
    let digest = format!("sha256:{}", file_digest(fixture_path)?);
    let expected = array(&fixtures, "fixtures", fixture_path)?
        .iter()
        .map(|fixture| {
            Ok((
                text(fixture, "row_id", fixture_path)?.to_string(),
                format!("cobol.file-runtime.{}", text(fixture, "id", fixture_path)?),
            ))
        })
        .collect::<TaskResult<BTreeMap<_, _>>>()?;
    require(
        expected.len() == 10,
        "COBOL file runtime fixture denominator drifted",
    )?;
    let registered = spec
        .registries()
        .fixtures()
        .iter()
        .filter(|(fixture, _)| fixture.as_str().starts_with("cobol.file-runtime."))
        .map(|(fixture, fixture_digest)| {
            require(
                fixture_digest == &digest,
                &format!("COBOL file runtime fixture digest drifted for {fixture}"),
            )?;
            Ok(fixture.as_str().to_string())
        })
        .collect::<TaskResult<BTreeSet<_>>>()?;
    require(
        registered == expected.values().cloned().collect(),
        "COBOL file runtime fixture registry is incomplete or stale",
    )?;
    let cases = spec
        .cases()
        .filter(|case| case.test_id().as_str().starts_with("cobol.file-runtime."))
        .map(|case| {
            let fixture = expected
                .get(case.key().row_id.as_str())
                .ok_or_else(|| format!("unknown COBOL file runtime row {}", case.key().row_id))?;
            require(
                case.key().gate == CoverageGate::Executed
                    && case.key().obligation_id.as_str() == "runtime-normal"
                    && case.test_id().as_str() == fixture
                    && case.input().as_str() == fixture
                    && case.driver().as_str() == "cobol.file-runtime.driver"
                    && case.preconditions().len() == 1
                    && case.preconditions()[0].as_str() == "cobol.file-runtime.fixture.available"
                    && case.expected().len() == 1
                    && case.expected()[0].as_str() == "cobol.file-runtime.executed"
                    && case.recovery().is_none()
                    && has_cobol_licensed_oracle(case),
                &format!("COBOL file runtime binding drifted for {fixture}"),
            )?;
            Ok(case.key().row_id.as_str().to_string())
        })
        .collect::<TaskResult<BTreeSet<_>>>()?;
    require(
        cases == expected.keys().cloned().collect(),
        "COBOL file runtime cases are incomplete or stale",
    )?;
    for row in spec
        .rows()
        .filter(|row| expected.contains_key(row.row_id().as_str()))
    {
        require(
            row.operation().as_str() == "cobol.file.semantics"
                && row.transition().as_str() == "cobol.file.compile-and-execute"
                && row.input().as_str() == "cobol.file-runtime.artifact"
                && row
                    .obligations()
                    .iter()
                    .any(|id| id.as_str() == "runtime-normal"),
            &format!(
                "COBOL file runtime row contract drifted for {}",
                row.row_id()
            ),
        )?;
    }
    Ok(())
}

fn check_cobol_assurance_bindings(
    root: &Path,
    spec: &CompiledSpec,
    fixture_paths: [(&str, &Path); 4],
) -> TaskResult {
    let mut expected = BTreeMap::new();
    for (kind, fixture_path) in fixture_paths {
        let fixtures = json(fixture_path)?;
        for fixture in array(&fixtures, "fixtures", fixture_path)? {
            let id = text(fixture, "id", fixture_path)?;
            let row_id = text(fixture, "row_id", fixture_path)?;
            let fixture_id = format!("cobol.{kind}-runtime.{id}");
            require(
                expected
                    .insert(
                        row_id.to_string(),
                        (kind.to_string(), id.to_string(), fixture_id),
                    )
                    .is_none(),
                &format!("COBOL assurance fixtures repeat official row {row_id}"),
            )?;
        }
    }
    require(
        expected.len() == 153,
        "COBOL assurance fixture denominator drifted",
    )?;

    let expected_tests = expected
        .values()
        .flat_map(|(kind, id, _)| {
            [
                format!("cobol.assurance.condition.{kind}.{id}"),
                format!("cobol.assurance.cancellation.{kind}.{id}"),
                format!("cobol.assurance.recovery.{kind}.{id}"),
                format!("cobol.assurance.quantum.{kind}.{id}"),
            ]
        })
        .collect::<BTreeSet<_>>();
    let actual_tests = spec
        .cases()
        .filter(|case| case.test_id().as_str().starts_with("cobol.assurance."))
        .map(|case| {
            let (kind, id, fixture_id) = expected
                .get(case.key().row_id.as_str())
                .ok_or_else(|| format!("unknown COBOL assurance row {}", case.key().row_id))?;
            let (gate, obligation, test_id, observation) = match case.key().obligation_id.as_str() {
                "resource-exhaustion" => (
                    CoverageGate::Conditioned,
                    "resource-exhaustion",
                    format!("cobol.assurance.condition.{kind}.{id}"),
                    "cobol.assurance.resource-conditioned",
                ),
                "checkpoint-identity" => (
                    CoverageGate::Recovered,
                    "checkpoint-identity",
                    format!("cobol.assurance.recovery.{kind}.{id}"),
                    "cobol.assurance.checkpoint-recovered",
                ),
                "runtime-cancellation" => (
                    CoverageGate::Conditioned,
                    "runtime-cancellation",
                    format!("cobol.assurance.cancellation.{kind}.{id}"),
                    "cobol.assurance.cancellation-conditioned",
                ),
                "quantum-identity" => (
                    CoverageGate::Executed,
                    "quantum-identity",
                    format!("cobol.assurance.quantum.{kind}.{id}"),
                    "cobol.assurance.quantum-invariant",
                ),
                other => return Err(format!("unknown COBOL assurance obligation {other}")),
            };
            require(
                case.key().gate == gate
                    && case.key().obligation_id.as_str() == obligation
                    && case.test_id().as_str() == test_id
                    && case.input().as_str() == fixture_id
                    && case.driver().as_str() == "cobol.assurance.driver"
                    && case.preconditions().len() == 1
                    && case.preconditions()[0].as_str() == "cobol.assurance.fixture.available"
                    && case.expected().len() == 1
                    && case.expected()[0].as_str() == observation
                    && case.recovery().is_none()
                    && has_cobol_licensed_oracle(case),
                &format!("COBOL assurance binding drifted for {}", case.test_id()),
            )?;
            Ok(test_id)
        })
        .collect::<TaskResult<BTreeSet<_>>>()?;
    require(
        actual_tests == expected_tests,
        "COBOL assurance cases are incomplete or stale",
    )?;

    let expected_licensed = expected
        .values()
        .map(|(kind, id, _)| format!("cobol.licensed.{kind}.{id}"))
        .collect::<BTreeSet<_>>();
    let actual_licensed = spec
        .cases()
        .filter(|case| case.test_id().as_str().starts_with("cobol.licensed."))
        .map(|case| {
            let (kind, id, fixture_id) = expected
                .get(case.key().row_id.as_str())
                .ok_or_else(|| format!("unknown licensed COBOL row {}", case.key().row_id))?;
            let test_id = format!("cobol.licensed.{kind}.{id}");
            require(
                case.key().gate == CoverageGate::Differential
                    && case.key().obligation_id.as_str() == "licensed-equivalence"
                    && case.test_id().as_str() == test_id
                    && case.input().as_str() == fixture_id
                    && case.driver().as_str() == "cobol.licensed.driver"
                    && case.preconditions().len() == 1
                    && case.preconditions()[0].as_str() == "cobol.licensed.receipt.available"
                    && case.expected().len() == 1
                    && case.expected()[0].as_str() == "cobol.licensed.row-equivalent"
                    && case.recovery().is_none()
                    && has_cobol_licensed_oracle(case),
                &format!("licensed COBOL binding drifted for {}", case.test_id()),
            )?;
            Ok(test_id)
        })
        .collect::<TaskResult<BTreeSet<_>>>()?;
    require(
        actual_licensed == expected_licensed,
        "licensed COBOL cases are incomplete or stale",
    )?;

    require(
        spec.cases()
            .filter(|case| expected.contains_key(case.key().row_id.as_str()))
            .all(has_cobol_licensed_oracle),
        "a COBOL programming case is detached from the licensed oracle identity",
    )?;

    for row in spec
        .rows()
        .filter(|row| expected.contains_key(row.row_id().as_str()))
    {
        let obligations = row
            .obligations()
            .iter()
            .map(|id| id.as_str())
            .collect::<BTreeSet<_>>();
        let preconditions = row
            .preconditions()
            .iter()
            .map(|predicate| predicate.as_str())
            .collect::<BTreeSet<_>>();
        let postconditions = row
            .postconditions()
            .iter()
            .map(|observation| observation.as_str())
            .collect::<BTreeSet<_>>();
        require(
            obligations.contains("resource-exhaustion")
                && obligations.contains("checkpoint-identity")
                && obligations.contains("runtime-cancellation")
                && obligations.contains("quantum-identity")
                && obligations.contains("licensed-equivalence")
                && preconditions.contains("cobol.assurance.fixture.available")
                && preconditions.contains("cobol.licensed.receipt.available")
                && postconditions.contains("cobol.assurance.resource-conditioned")
                && postconditions.contains("cobol.assurance.checkpoint-recovered")
                && postconditions.contains("cobol.assurance.cancellation-conditioned")
                && postconditions.contains("cobol.assurance.quantum-invariant")
                && postconditions.contains("cobol.licensed.row-equivalent")
                && row
                    .oracle()
                    .is_some_and(|oracle| oracle.as_str() == "cobol.enterprise-6.5.licensed"),
            &format!("COBOL assurance row contract drifted for {}", row.row_id()),
        )?;
    }
    for (kind, name) in [
        ("driver", "cobol.assurance.driver"),
        ("predicate", "cobol.assurance.fixture.available"),
        ("observation", "cobol.assurance.resource-conditioned"),
        ("observation", "cobol.assurance.checkpoint-recovered"),
        ("observation", "cobol.assurance.cancellation-conditioned"),
        ("observation", "cobol.assurance.quantum-invariant"),
        ("driver", "cobol.licensed.driver"),
        ("predicate", "cobol.licensed.receipt.available"),
        ("observation", "cobol.licensed.row-equivalent"),
    ] {
        let registered = match kind {
            "driver" => spec
                .registries()
                .drivers()
                .iter()
                .any(|value| value.as_str() == name),
            "predicate" => spec
                .registries()
                .predicates()
                .iter()
                .any(|value| value.as_str() == name),
            _ => spec
                .registries()
                .observations()
                .iter()
                .any(|value| value.as_str() == name),
        };
        require(
            registered,
            &format!("COBOL assurance {kind} registry entry is missing: {name}"),
        )?;
    }
    let oracle_digest = format!(
        "sha256:{}",
        file_digest(&root.join("conformance/0.4/oracles/cobol-licensed-differential.json"))?
    );
    require(
        spec.registries().oracles().iter().any(|(oracle, digest)| {
            oracle.as_str() == "cobol.enterprise-6.5.licensed" && digest == &oracle_digest
        }),
        "licensed COBOL oracle registry identity or digest drifted",
    )?;
    Ok(())
}

fn has_cobol_licensed_oracle(case: &mainframe_env_coverage::ConformanceCase) -> bool {
    case.oracle()
        .is_some_and(|oracle| oracle.as_str() == "cobol.enterprise-6.5.licensed")
}

fn check_cobol_reference_policy(root: &Path, spec: &CompiledSpec) -> TaskResult {
    require(
        spec.cases().all(|case| {
            !case.test_id().as_str().contains("gnucobol")
                && !case.driver().as_str().contains("gnucobol")
                && case
                    .oracle()
                    .is_none_or(|oracle| !oracle.as_str().contains("gnucobol"))
        }) && spec
            .registries()
            .oracles()
            .iter()
            .all(|(oracle, _)| !oracle.as_str().contains("gnucobol")),
        "GnuCOBOL reference results must not enter the Conformance IR or oracle registry",
    )?;
    let adapter_path = root.join("crates/tooling/mainframe-env-conformance/src/cobol_reference.rs");
    let adapter = read(&adapter_path)?;
    for required in [
        "reference: \"gnucobol\"",
        "licensed_differential_credit: 0",
        "licensed_cases_pending: 153",
        "-std=ibm-strict",
        "PROCESS_TIMEOUT",
        "MAX_OUTPUT_BYTES",
        "product bug, harness bug, or documented dialect/runtime divergence",
    ] {
        require(
            adapter.contains(required),
            &format!("GnuCOBOL reference adapter omits {required}"),
        )?;
    }
    let mut manifests = Vec::new();
    for component in ["apps", "contracts", "kernel", "providers", "stores"] {
        collect_extension(
            &root.join("crates").join(component),
            OsStr::new("toml"),
            &mut manifests,
        )?;
    }
    manifests.push(root.join("Cargo.toml"));
    manifests.push(root.join("Cargo.lock"));
    for manifest in manifests {
        let source = read(&manifest)?.to_ascii_lowercase();
        require(
            !source.contains("gnucobol") && !source.contains("libcob"),
            &format!(
                "production dependency closure links or packages GnuCOBOL in {}",
                manifest.display()
            ),
        )?;
    }
    for relative in [
        "docs/prompts/coverage-versions/IMPLEMENT_0_4_0.md",
        "docs/delivery/coverage-versions/0.4.0.md",
        "docs/delivery/coverage-versions/0.4.0-local-assurance.md",
        "docs/delivery/coverage-versions/status/0.4.0.md",
    ] {
        let document = read(&root.join(relative))?;
        require(
            document.contains("pass-with-licensed-differential-pending")
                && document.contains("GnuCOBOL")
                && document.contains("0/153")
                && document.contains("0.17"),
            &format!("{relative} omits the approved GnuCOBOL completion disposition"),
        )?;
    }
    for relative in [
        "docs/prompts/coverage-versions/IMPLEMENT_0_17_0.md",
        "docs/delivery/coverage-versions/0.17.0.md",
    ] {
        let document = read(&root.join(relative))?;
        require(
            document.contains("GnuCOBOL")
                && document.contains("0/153")
                && document.contains("153-row"),
            &format!("{relative} omits the deferred licensed COBOL campaign handoff"),
        )?;
    }
    Ok(())
}

fn check_cobol_recovery_bindings(
    _root: &Path,
    spec: &CompiledSpec,
    fixture_path: &Path,
) -> TaskResult {
    let fixtures = json(fixture_path)?;
    let digest = format!("sha256:{}", file_digest(fixture_path)?);
    let expected = array(&fixtures, "fixtures", fixture_path)?
        .iter()
        .map(|fixture| {
            Ok((
                text(fixture, "row_id", fixture_path)?.to_string(),
                format!("cobol.recovery.{}", text(fixture, "id", fixture_path)?),
            ))
        })
        .collect::<TaskResult<BTreeMap<_, _>>>()?;
    require(
        (5..=512).contains(&expected.len()),
        "COBOL recovery fixture denominator drifted",
    )?;
    let registered = spec
        .registries()
        .fixtures()
        .iter()
        .filter(|(fixture, _)| fixture.as_str().starts_with("cobol.recovery."))
        .map(|(fixture, fixture_digest)| {
            require(
                fixture_digest == &digest,
                &format!("COBOL recovery fixture digest drifted for {fixture}"),
            )?;
            Ok(fixture.as_str().to_string())
        })
        .collect::<TaskResult<BTreeSet<_>>>()?;
    require(
        registered == expected.values().cloned().collect(),
        "COBOL recovery fixture registry is incomplete or stale",
    )?;
    let cases = spec
        .cases()
        .filter(|case| case.test_id().as_str().starts_with("cobol.recovery."))
        .map(|case| {
            let fixture = expected
                .get(case.key().row_id.as_str())
                .ok_or_else(|| format!("unknown COBOL recovery row {}", case.key().row_id))?;
            require(
                case.key().gate == CoverageGate::Recovered
                    && case.key().obligation_id.as_str() == "checkpoint-restart"
                    && case.test_id().as_str() == fixture
                    && case.input().as_str() == fixture
                    && case.driver().as_str() == "cobol.recovery.driver"
                    && case.preconditions().len() == 1
                    && case.preconditions()[0].as_str() == "cobol.recovery.fixture.available"
                    && case.expected().len() == 1
                    && case.expected()[0].as_str() == "cobol.recovery.exact"
                    && case.recovery().is_none()
                    && has_cobol_licensed_oracle(case),
                &format!("COBOL recovery binding drifted for {fixture}"),
            )?;
            Ok(case.key().row_id.as_str().to_string())
        })
        .collect::<TaskResult<BTreeSet<_>>>()?;
    require(
        cases == expected.keys().cloned().collect(),
        "COBOL recovery cases are incomplete or stale",
    )
}

fn check_cobol_condition_bindings(
    _root: &Path,
    spec: &CompiledSpec,
    fixture_path: &Path,
) -> TaskResult {
    let fixtures = json(fixture_path)?;
    let digest = format!("sha256:{}", file_digest(fixture_path)?);
    let expected = array(&fixtures, "fixtures", fixture_path)?
        .iter()
        .map(|fixture| {
            Ok((
                text(fixture, "row_id", fixture_path)?.to_string(),
                format!("cobol.condition.{}", text(fixture, "id", fixture_path)?),
            ))
        })
        .collect::<TaskResult<BTreeMap<_, _>>>()?;
    require(
        (10..=512).contains(&expected.len()),
        "COBOL condition fixture denominator drifted",
    )?;
    let registered = spec
        .registries()
        .fixtures()
        .iter()
        .filter(|(fixture, _)| fixture.as_str().starts_with("cobol.condition."))
        .map(|(fixture, fixture_digest)| {
            require(
                fixture_digest == &digest,
                &format!("COBOL condition fixture digest drifted for {fixture}"),
            )?;
            Ok(fixture.as_str().to_string())
        })
        .collect::<TaskResult<BTreeSet<_>>>()?;
    require(
        registered == expected.values().cloned().collect(),
        "COBOL condition fixture registry is incomplete or stale",
    )?;
    let cases = spec
        .cases()
        .filter(|case| case.test_id().as_str().starts_with("cobol.condition."))
        .map(|case| {
            let fixture = expected
                .get(case.key().row_id.as_str())
                .ok_or_else(|| format!("unknown COBOL condition row {}", case.key().row_id))?;
            require(
                case.key().gate == CoverageGate::Conditioned
                    && case.key().obligation_id.as_str() == "runtime-condition"
                    && case.test_id().as_str() == fixture
                    && case.input().as_str() == fixture
                    && case.driver().as_str() == "cobol.condition.driver"
                    && case.preconditions().len() == 1
                    && case.preconditions()[0].as_str() == "cobol.condition.fixture.available"
                    && case.expected().len() == 1
                    && case.expected()[0].as_str() == "cobol.condition.exact"
                    && case.recovery().is_none()
                    && has_cobol_licensed_oracle(case),
                &format!("COBOL condition binding drifted for {fixture}"),
            )?;
            Ok(case.key().row_id.as_str().to_string())
        })
        .collect::<TaskResult<BTreeSet<_>>>()?;
    require(
        cases == expected.keys().cloned().collect(),
        "COBOL condition cases are incomplete or stale",
    )
}

fn generate_cobol_language(root: &Path) -> TaskResult {
    let path = root.join("crates/kernel/mainframe-env-compiler/src/generated/cobol_language.rs");
    fs::create_dir_all(path.parent().ok_or("generated COBOL path has no parent")?)
        .map_err(|error| error.to_string())?;
    fs::write(&path, render_cobol_language(root)?)
        .map_err(|error| format!("{}: {error}", path.display()))
}

fn check_cobol_language_generated(root: &Path) -> TaskResult {
    let path = root.join("crates/kernel/mainframe-env-compiler/src/generated/cobol_language.rs");
    let actual = fs::read(&path).map_err(|error| format!("{}: {error}", path.display()))?;
    require(
        actual == render_cobol_language(root)?,
        "generated COBOL language identities are stale; run cargo xtask cobol-language",
    )
}

fn render_cobol_language(root: &Path) -> TaskResult<Vec<u8>> {
    let path = root.join("conformance/0.3/cobol/language.json");
    let language = json(&path)?;
    check_cobol_language_catalog(root, &path)?;
    let statements = array(&language, "compiler_directing_statements", &path)?;
    let groups = array(&language, "compiler_directive_groups", &path)?;
    let procedure_statements = array(&language, "procedure_statements", &path)?;
    let file_clauses = array(&language, "file_description_clauses", &path)?;
    let data_clauses = array(&language, "data_description_clauses", &path)?;
    let intrinsic_functions = array(&language, "intrinsic_functions", &path)?;
    let special_registers = array(&language, "special_registers", &path)?;
    let literal = |value: &str| serde_json::to_string(value).map_err(|error| error.to_string());
    let mut source =
        String::from("// @generated by `cargo xtask cobol-language`; do not edit.\n\n");
    source.push_str("#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]\n");
    source.push_str("pub enum CompilerDirectingKind {\n");
    for entry in statements {
        source.push_str(&format!(
            "    {},\n",
            rust_variant(text(entry, "id", &path)?)?
        ));
    }
    source.push_str("}\n\n");
    source.push_str("#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]\n");
    source.push_str("pub enum ProcedureStatementKind {\n");
    for entry in procedure_statements {
        source.push_str(&format!(
            "    {},\n",
            rust_variant(text(entry, "id", &path)?)?
        ));
    }
    source.push_str("}\n\n");
    source.push_str(&format!(
        "impl ProcedureStatementKind {{\n    pub const ALL: [Self; {}] = [\n",
        procedure_statements.len()
    ));
    for entry in procedure_statements {
        source.push_str(&format!(
            "        Self::{},\n",
            rust_variant(text(entry, "id", &path)?)?
        ));
    }
    source.push_str("    ];\n}\n\n");
    source.push_str("#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]\n");
    source.push_str("pub enum IntrinsicFunctionKind {\n");
    for entry in intrinsic_functions {
        source.push_str(&format!(
            "    {},\n",
            rust_variant(text(entry, "id", &path)?)?
        ));
    }
    source.push_str("}\n\n");
    source.push_str(&format!(
        "impl IntrinsicFunctionKind {{\n    pub const ALL: [Self; {}] = [\n",
        intrinsic_functions.len()
    ));
    for entry in intrinsic_functions {
        source.push_str(&format!(
            "        Self::{},\n",
            rust_variant(text(entry, "id", &path)?)?
        ));
    }
    source.push_str("    ];\n}\n\n");
    source.push_str("#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]\n");
    source.push_str("pub enum SpecialRegisterKind {\n");
    for entry in special_registers {
        source.push_str(&format!(
            "    {},\n",
            rust_variant(text(entry, "id", &path)?)?
        ));
    }
    source.push_str("}\n\n");
    source.push_str(&format!(
        "impl SpecialRegisterKind {{\n    pub const ALL: [Self; {}] = [\n",
        special_registers.len()
    ));
    for entry in special_registers {
        source.push_str(&format!(
            "        Self::{},\n",
            rust_variant(text(entry, "id", &path)?)?
        ));
    }
    source.push_str("    ];\n}\n\n");
    source.push_str("#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]\npub enum IntrinsicArgumentClass { Alphabetic, Alphanumeric, Dbcs, Integer, Numeric, National, Utf8, Other, Keyword }\n\n");
    source.push_str("#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]\npub enum IntrinsicResultRule { Integer, Numeric, Alphanumeric, National, Utf8, PreserveNumeric, Content, StringFirst, Comparable, RangeSum }\n\n");
    source.push_str("#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]\npub enum SpecialRegisterValueType { Alphanumeric, Integer, National, Other, Group }\n\n");
    source.push_str("#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]\npub enum SpecialRegisterUsage { Display, Binary, NativeBinary, National, Pointer, Group }\n\n");
    source.push_str("#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]\npub enum SpecialRegisterLengthKind { Fixed, Lp, Dynamic, Dependent }\n\n");
    source.push_str("#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]\npub enum SpecialRegisterOperand { None, DataItem, File, DebugContext }\n\n");
    source.push_str(&format!(
        "impl FileDescriptionClauseKind {{\n    pub const ALL: [Self; {}] = [\n",
        file_clauses.len()
    ));
    for entry in file_clauses {
        source.push_str(&format!(
            "        Self::{},\n",
            rust_variant(text(entry, "id", &path)?)?
        ));
    }
    source.push_str("    ];\n}\n\n");
    source.push_str("#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]\n");
    source.push_str("pub enum FileDescriptionClauseKind {\n");
    for entry in file_clauses {
        source.push_str(&format!(
            "    {},\n",
            rust_variant(text(entry, "id", &path)?)?
        ));
    }
    source.push_str("}\n\n");
    source.push_str(&format!(
        "impl DataDescriptionClauseKind {{\n    pub const ALL: [Self; {}] = [\n",
        data_clauses.len()
    ));
    for entry in data_clauses {
        source.push_str(&format!(
            "        Self::{},\n",
            rust_variant(text(entry, "id", &path)?)?
        ));
    }
    source.push_str("    ];\n}\n\n");
    source.push_str("#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]\n");
    source.push_str("pub enum DataDescriptionClauseKind {\n");
    for entry in data_clauses {
        source.push_str(&format!(
            "    {},\n",
            rust_variant(text(entry, "id", &path)?)?
        ));
    }
    source.push_str("}\n\n");
    source.push_str("#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]\n");
    source.push_str("pub enum CompilerDirectiveGroup {\n");
    for entry in groups {
        source.push_str(&format!(
            "    {},\n",
            rust_variant(text(entry, "id", &path)?)?
        ));
    }
    source.push_str("}\n\n");
    let mut directives = Vec::new();
    for group in groups {
        let group_variant = rust_variant(text(group, "id", &path)?)?;
        for directive in array(group, "directives", &path)? {
            let id = directive
                .as_str()
                .ok_or_else(|| format!("{} directive ID is not a string", path.display()))?;
            directives.push((id.to_string(), group_variant.clone()));
        }
    }
    source.push_str("#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]\n");
    source.push_str("pub enum CompilerDirectiveKind {\n");
    for (id, _) in &directives {
        source.push_str(&format!("    {},\n", rust_variant(id)?));
    }
    source.push_str("}\n\n");
    source.push_str("#[derive(Clone, Copy, Debug, Eq, PartialEq)]\n");
    source.push_str("pub struct CompilerDirectingDescriptor {\n");
    source.push_str("    pub kind: CompilerDirectingKind,\n    pub id: &'static str,\n    pub row_id: &'static str,\n    pub label: &'static str,\n    pub source_locator: &'static str,\n    pub forms: &'static [&'static str],\n    pub placement: &'static str,\n    pub effect: &'static str,\n}\n\n");
    source.push_str("#[derive(Clone, Copy, Debug, Eq, PartialEq)]\n");
    source.push_str("pub struct CompilerDirectiveGroupDescriptor {\n");
    source.push_str("    pub group: CompilerDirectiveGroup,\n    pub id: &'static str,\n    pub row_id: &'static str,\n    pub label: &'static str,\n    pub source_locator: &'static str,\n    pub placement: &'static str,\n    pub effect: &'static str,\n}\n\n");
    source.push_str("#[derive(Clone, Copy, Debug, Eq, PartialEq)]\n");
    source.push_str("pub struct CompilerDirectiveDescriptor {\n");
    source.push_str("    pub kind: CompilerDirectiveKind,\n    pub group: CompilerDirectiveGroup,\n    pub id: &'static str,\n}\n\n");
    source.push_str("#[derive(Clone, Copy, Debug, Eq, PartialEq)]\n");
    source.push_str("pub struct ClauseDescriptor<K> {\n");
    source.push_str("    pub kind: K,\n    pub id: &'static str,\n    pub row_id: &'static str,\n    pub label: &'static str,\n    pub source_locator: &'static str,\n    pub forms: &'static [&'static str],\n    pub placement: &'static str,\n}\n\n");
    source.push_str("#[derive(Clone, Copy, Debug, Eq, PartialEq)]\n");
    source.push_str("pub struct ProcedureStatementDescriptor {\n");
    source.push_str("    pub kind: ProcedureStatementKind,\n    pub id: &'static str,\n    pub row_id: &'static str,\n    pub label: &'static str,\n    pub source_locator: &'static str,\n    pub forms: &'static [&'static str],\n    pub grammar_keywords: &'static [&'static str],\n}\n\n");
    source.push_str("#[derive(Clone, Copy, Debug, Eq, PartialEq)]\n");
    source.push_str("pub struct IntrinsicSignature {\n    pub arguments: &'static [&'static [IntrinsicArgumentClass]],\n    pub variadic: bool,\n    pub homogeneous: bool,\n    pub result: IntrinsicResultRule,\n}\n\n");
    source.push_str("#[derive(Clone, Copy, Debug, Eq, PartialEq)]\n");
    source.push_str("pub struct IntrinsicFunctionDescriptor {\n    pub kind: IntrinsicFunctionKind,\n    pub id: &'static str,\n    pub row_id: &'static str,\n    pub name: &'static str,\n    pub source_locator: &'static str,\n    pub signatures: &'static [IntrinsicSignature],\n    pub literal_arguments: &'static [usize],\n    pub fixed_length: Option<usize>,\n    pub runtime_supported: bool,\n}\n\n");
    source.push_str("#[derive(Clone, Copy, Debug, Eq, PartialEq)]\n");
    source.push_str("pub struct SpecialRegisterDescriptor {\n    pub kind: SpecialRegisterKind,\n    pub id: &'static str,\n    pub name: &'static str,\n    pub source_locator: &'static str,\n    pub value_type: SpecialRegisterValueType,\n    pub usage: SpecialRegisterUsage,\n    pub length_kind: SpecialRegisterLengthKind,\n    pub fixed_length: Option<usize>,\n    pub writable: bool,\n    pub operand: SpecialRegisterOperand,\n    pub runtime_supported: bool,\n}\n\n");
    source.push_str(
        "pub static COMPILER_DIRECTING_STATEMENTS: &[CompilerDirectingDescriptor] = &[\n",
    );
    for entry in statements {
        let id = text(entry, "id", &path)?;
        source.push_str("    CompilerDirectingDescriptor {\n");
        source.push_str(&format!(
            "        kind: CompilerDirectingKind::{},\n",
            rust_variant(id)?
        ));
        for field in ["id", "row_id", "label", "source_locator"] {
            source.push_str(&format!(
                "        {field}: {},\n",
                literal(text(entry, field, &path)?)?
            ));
        }
        source.push_str("        forms: &[");
        for form in array(entry, "forms", &path)? {
            source.push_str(&literal(
                form.as_str().ok_or("COBOL form is not a string")?,
            )?);
            source.push(',');
        }
        source.push_str("],\n");
        for field in ["placement", "effect"] {
            source.push_str(&format!(
                "        {field}: {},\n",
                literal(text(entry, field, &path)?)?
            ));
        }
        source.push_str("    },\n");
    }
    source.push_str("];\n\n");
    source.push_str("pub static PROCEDURE_STATEMENTS: &[ProcedureStatementDescriptor] = &[\n");
    for entry in procedure_statements {
        let id = text(entry, "id", &path)?;
        source.push_str("    ProcedureStatementDescriptor {\n");
        source.push_str(&format!(
            "        kind: ProcedureStatementKind::{},\n",
            rust_variant(id)?
        ));
        for field in ["id", "row_id", "label", "source_locator"] {
            source.push_str(&format!(
                "        {field}: {},\n",
                literal(text(entry, field, &path)?)?
            ));
        }
        source.push_str("        forms: &[");
        for form in array(entry, "forms", &path)? {
            source.push_str(&literal(
                form.as_str().ok_or("COBOL form is not a string")?,
            )?);
            source.push(',');
        }
        source.push_str("],\n        grammar_keywords: &[");
        for keyword in cobol_form_keywords(entry, &path)? {
            source.push_str(&literal(&keyword)?);
            source.push(',');
        }
        source.push_str("],\n    },\n");
    }
    source.push_str("];\n\n");
    source.push_str("pub static INTRINSIC_FUNCTIONS: &[IntrinsicFunctionDescriptor] = &[\n");
    for entry in intrinsic_functions {
        let id = text(entry, "id", &path)?;
        source.push_str("    IntrinsicFunctionDescriptor {\n");
        source.push_str(&format!(
            "        kind: IntrinsicFunctionKind::{},\n",
            rust_variant(id)?
        ));
        for field in ["id", "row_id", "name", "source_locator"] {
            source.push_str(&format!(
                "        {field}: {},\n",
                literal(text(entry, field, &path)?)?
            ));
        }
        source.push_str("        signatures: &[\n");
        for signature in array(entry, "signatures", &path)? {
            source.push_str("            IntrinsicSignature { arguments: &[");
            for position in array(signature, "arguments", &path)? {
                let classes = position
                    .as_array()
                    .ok_or("intrinsic argument set is not an array")?;
                source.push_str("&[");
                for class in classes {
                    source.push_str(&format!(
                        "IntrinsicArgumentClass::{},",
                        rust_variant(class.as_str().ok_or("intrinsic class is not a string")?)?
                    ));
                }
                source.push_str("],");
            }
            source.push_str(&format!(
                "], variadic: {}, homogeneous: {}, result: IntrinsicResultRule::{} }},\n",
                signature["variadic"]
                    .as_bool()
                    .ok_or("intrinsic variadic flag is not boolean")?,
                signature["homogeneous"]
                    .as_bool()
                    .ok_or("intrinsic homogeneous flag is not boolean")?,
                rust_variant(text(signature, "result", &path)?)?
            ));
        }
        source.push_str("        ],\n");
        source.push_str("        literal_arguments: &[");
        for index in array(entry, "literal_arguments", &path)? {
            source.push_str(&format!(
                "{}usize,",
                index
                    .as_u64()
                    .ok_or("intrinsic literal argument index is not an integer")?
            ));
        }
        source.push_str("],\n");
        source.push_str(&format!(
            "        fixed_length: {},\n",
            entry["fixed_length"]
                .as_u64()
                .map_or_else(|| "None".to_string(), |value| format!("Some({value}usize)"))
        ));
        source.push_str(&format!(
            "        runtime_supported: {},\n",
            entry["runtime_supported"]
                .as_bool()
                .ok_or("intrinsic runtime support flag is not boolean")?
        ));
        source.push_str("    },\n");
    }
    source.push_str("];\n\n");
    source.push_str("pub static SPECIAL_REGISTERS: &[SpecialRegisterDescriptor] = &[\n");
    for entry in special_registers {
        let id = text(entry, "id", &path)?;
        source.push_str("    SpecialRegisterDescriptor {\n");
        source.push_str(&format!(
            "        kind: SpecialRegisterKind::{},\n",
            rust_variant(id)?
        ));
        for field in ["id", "name", "source_locator"] {
            source.push_str(&format!(
                "        {field}: {},\n",
                literal(text(entry, field, &path)?)?
            ));
        }
        for (field, kind) in [
            ("value_type", "SpecialRegisterValueType"),
            ("usage", "SpecialRegisterUsage"),
            ("length_kind", "SpecialRegisterLengthKind"),
            ("operand", "SpecialRegisterOperand"),
        ] {
            source.push_str(&format!(
                "        {field}: {kind}::{},\n",
                rust_variant(text(entry, field, &path)?)?
            ));
        }
        source.push_str(&format!(
            "        fixed_length: {},\n",
            entry["fixed_length"]
                .as_u64()
                .map_or_else(|| "None".to_string(), |value| format!("Some({value}usize)"))
        ));
        source.push_str(&format!(
            "        writable: {},\n",
            entry["writable"]
                .as_bool()
                .ok_or("special-register writable flag is not boolean")?
        ));
        source.push_str(&format!(
            "        runtime_supported: {},\n",
            entry["runtime_supported"]
                .as_bool()
                .ok_or("special-register runtime support flag is not boolean")?
        ));
        source.push_str("    },\n");
    }
    source.push_str("];\n\n");
    source.push_str("pub static FILE_DESCRIPTION_CLAUSES: &[ClauseDescriptor<FileDescriptionClauseKind>] = &[\n");
    for entry in file_clauses {
        let id = text(entry, "id", &path)?;
        source.push_str("    ClauseDescriptor {\n");
        source.push_str(&format!(
            "        kind: FileDescriptionClauseKind::{},\n",
            rust_variant(id)?
        ));
        for field in ["id", "row_id", "label", "source_locator"] {
            source.push_str(&format!(
                "        {field}: {},\n",
                literal(text(entry, field, &path)?)?
            ));
        }
        source.push_str("        forms: &[");
        for form in array(entry, "forms", &path)? {
            source.push_str(&literal(
                form.as_str().ok_or("COBOL form is not a string")?,
            )?);
            source.push(',');
        }
        source.push_str("],\n");
        source.push_str(&format!(
            "        placement: {},\n",
            literal(text(entry, "placement", &path)?)?
        ));
        source.push_str("    },\n");
    }
    source.push_str("];\n\n");
    source.push_str("pub static DATA_DESCRIPTION_CLAUSES: &[ClauseDescriptor<DataDescriptionClauseKind>] = &[\n");
    for entry in data_clauses {
        let id = text(entry, "id", &path)?;
        source.push_str("    ClauseDescriptor {\n");
        source.push_str(&format!(
            "        kind: DataDescriptionClauseKind::{},\n",
            rust_variant(id)?
        ));
        for field in ["id", "row_id", "label", "source_locator"] {
            source.push_str(&format!(
                "        {field}: {},\n",
                literal(text(entry, field, &path)?)?
            ));
        }
        source.push_str("        forms: &[");
        for form in array(entry, "forms", &path)? {
            source.push_str(&literal(
                form.as_str().ok_or("COBOL form is not a string")?,
            )?);
            source.push(',');
        }
        source.push_str("],\n");
        source.push_str(&format!(
            "        placement: {},\n",
            literal(text(entry, "placement", &path)?)?
        ));
        source.push_str("    },\n");
    }
    source.push_str("];\n\n");
    source.push_str(
        "pub static COMPILER_DIRECTIVE_GROUPS: &[CompilerDirectiveGroupDescriptor] = &[\n",
    );
    for entry in groups {
        let id = text(entry, "id", &path)?;
        source.push_str("    CompilerDirectiveGroupDescriptor {\n");
        source.push_str(&format!(
            "        group: CompilerDirectiveGroup::{},\n",
            rust_variant(id)?
        ));
        for field in [
            "id",
            "row_id",
            "label",
            "source_locator",
            "placement",
            "effect",
        ] {
            source.push_str(&format!(
                "        {field}: {},\n",
                literal(text(entry, field, &path)?)?
            ));
        }
        source.push_str("    },\n");
    }
    source.push_str("];\n\n");
    source.push_str("pub static COMPILER_DIRECTIVES: &[CompilerDirectiveDescriptor] = &[\n");
    for (id, group) in directives {
        source.push_str(&format!(
            "    CompilerDirectiveDescriptor {{ kind: CompilerDirectiveKind::{}, group: CompilerDirectiveGroup::{group}, id: {} }},\n",
            rust_variant(&id)?,
            literal(&id)?
        ));
    }
    source.push_str("];\n\n");
    source.push_str("pub fn compiler_directing_descriptor(kind: CompilerDirectingKind) -> &'static CompilerDirectingDescriptor {\n    COMPILER_DIRECTING_STATEMENTS.iter().find(|entry| entry.kind == kind).expect(\"generated directing kind\")\n}\n\n");
    source.push_str("pub fn compiler_directive_descriptor(kind: CompilerDirectiveKind) -> &'static CompilerDirectiveDescriptor {\n    COMPILER_DIRECTIVES.iter().find(|entry| entry.kind == kind).expect(\"generated directive kind\")\n}\n");
    source.push_str("\npub fn file_description_clause_descriptor(kind: FileDescriptionClauseKind) -> &'static ClauseDescriptor<FileDescriptionClauseKind> {\n    FILE_DESCRIPTION_CLAUSES.iter().find(|entry| entry.kind == kind).expect(\"generated file clause kind\")\n}\n");
    source.push_str("\npub fn data_description_clause_descriptor(kind: DataDescriptionClauseKind) -> &'static ClauseDescriptor<DataDescriptionClauseKind> {\n    DATA_DESCRIPTION_CLAUSES.iter().find(|entry| entry.kind == kind).expect(\"generated data clause kind\")\n}\n");
    source.push_str("\npub fn procedure_statement_descriptor(kind: ProcedureStatementKind) -> &'static ProcedureStatementDescriptor {\n    PROCEDURE_STATEMENTS.iter().find(|entry| entry.kind == kind).expect(\"generated procedure statement kind\")\n}\n");
    source.push_str("\npub fn intrinsic_function_descriptor(kind: IntrinsicFunctionKind) -> &'static IntrinsicFunctionDescriptor {\n    INTRINSIC_FUNCTIONS.iter().find(|entry| entry.kind == kind).expect(\"generated intrinsic function kind\")\n}\n");
    source.push_str("\npub fn intrinsic_function_named(name: &str) -> Option<&'static IntrinsicFunctionDescriptor> {\n    INTRINSIC_FUNCTIONS.iter().find(|entry| entry.name.eq_ignore_ascii_case(name))\n}\n");
    source.push_str("\npub fn special_register_descriptor(kind: SpecialRegisterKind) -> &'static SpecialRegisterDescriptor {\n    SPECIAL_REGISTERS.iter().find(|entry| entry.kind == kind).expect(\"generated special register kind\")\n}\n");
    source.push_str("\npub fn special_register_named(name: &str) -> Option<&'static SpecialRegisterDescriptor> {\n    SPECIAL_REGISTERS.iter().find(|entry| entry.name.eq_ignore_ascii_case(name))\n}\n");
    format_generated_rust(root, source.into_bytes())
}

fn cobol_form_keywords(entry: &Value, path: &Path) -> TaskResult<Vec<String>> {
    let mut keywords = BTreeSet::new();
    for form in array(entry, "forms", path)? {
        let form = form.as_str().ok_or("COBOL form is not a string")?;
        for word in
            form.split(|character: char| !(character.is_ascii_alphanumeric() || character == '-'))
        {
            if !word.is_empty()
                && word.bytes().any(|byte| byte.is_ascii_alphabetic())
                && word
                    .bytes()
                    .filter(|byte| byte.is_ascii_alphabetic())
                    .all(|byte| byte.is_ascii_uppercase())
            {
                keywords.insert(word.to_string());
            }
        }
    }
    Ok(keywords.into_iter().collect())
}

fn format_generated_rust(root: &Path, source: Vec<u8>) -> TaskResult<Vec<u8>> {
    let mut child = Command::new("rustfmt")
        .args(["--edition", "2024", "--emit", "stdout"])
        .current_dir(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("spawn rustfmt for generated COBOL: {error}"))?;
    child
        .stdin
        .take()
        .ok_or("rustfmt stdin is unavailable")?
        .write_all(&source)
        .map_err(|error| format!("write generated COBOL to rustfmt: {error}"))?;
    let output = child
        .wait_with_output()
        .map_err(|error| format!("wait for generated COBOL rustfmt: {error}"))?;
    require(
        output.status.success(),
        &format!(
            "rustfmt rejected generated COBOL: {}",
            String::from_utf8_lossy(&output.stderr)
        ),
    )?;
    Ok(output.stdout)
}

fn compile_shared_spec(root: &Path) -> TaskResult<CompiledSpec> {
    let index_path = root.join("conformance/0.2/catalogs/index.json");
    let catalog_digest = format!("sha256:{}", file_digest(&index_path)?);
    let spec_path = root.join("conformance/spec/v1/spec.json");
    let mut spec_value = json(&spec_path)?;
    augment_ams_spec(root, &mut spec_value)?;
    let bytes = serde_json::to_vec(&spec_value).map_err(|error| error.to_string())?;
    CompiledSpec::compile_json(
        &catalog_digest,
        official_catalog_rows(root)?,
        &bytes,
        ConformanceLimits::default(),
    )
    .map_err(|problem| problem.to_string())
}

fn augment_ams_spec(root: &Path, spec: &mut Value) -> TaskResult {
    let fixture_path = root.join("conformance/0.6/fixtures/ams-commands.json");
    let fixture = json(&fixture_path)?;
    let cases = array(&fixture, "cases", &fixture_path)?;
    let digest = format!("sha256:{}", file_digest(&fixture_path)?);
    let registries = spec
        .get_mut("registries")
        .and_then(Value::as_object_mut)
        .ok_or("conformance spec registries are missing")?;
    for (name, value) in [
        ("operations", "dataset.ams.command"),
        ("input_shapes", "dataset.ams.fixture"),
        ("transitions", "dataset.ams.transition"),
        ("conditions", "dataset.ams.condition"),
        ("recoveries", "dataset.ams.restart"),
        ("drivers", "dataset.ams.driver"),
    ] {
        registries
            .get_mut(name)
            .and_then(Value::as_array_mut)
            .ok_or_else(|| format!("conformance registry {name} is missing"))?
            .push(Value::String(value.into()));
    }
    let mut new_observations = Vec::new();
    let mut new_fixtures = Vec::new();
    for case in cases {
        let fixture_id = text(case, "id", &fixture_path)?;
        new_observations.push(Value::String(format!("observe.{fixture_id}")));
        new_fixtures.push(json!({"id": fixture_id, "digest": digest.clone()}));
    }
    registries
        .get_mut("observations")
        .and_then(Value::as_array_mut)
        .ok_or("conformance observation registry is missing")?
        .extend(new_observations);
    registries
        .get_mut("fixtures")
        .and_then(Value::as_array_mut)
        .ok_or("conformance fixture registry is missing")?
        .extend(new_fixtures);

    let mut new_rows = Vec::new();
    let mut new_obligations = Vec::new();
    let mut new_bindings = Vec::new();
    for (position, case) in cases.iter().enumerate() {
        let command = text(case, "command_id", &fixture_path)?;
        let fixture_id = text(case, "id", &fixture_path)?;
        let row_id = format!(
            "ibm-zos-3.2-dfsms-ams-2026-06:ams-functional-commands:{:04}",
            position + 1
        );
        new_rows.push(json!({
            "row_id": row_id,
            "operation": "dataset.ams.command",
            "input": "dataset.ams.fixture",
            "preconditions": [],
            "transition": "dataset.ams.transition",
            "postconditions": [format!("observe.{fixture_id}")],
            "conditions": ["dataset.ams.condition"],
            "recovery": "dataset.ams.restart",
            "oracle": null,
            "applicable_gates": ["recognized", "validated", "executed", "conditioned", "recovered", "differential"],
            "obligations": ["command-contract"]
        }));
        new_obligations.push(json!({
            "row_id": row_id,
            "obligation_id": "command-contract",
            "applicable_gates": ["recognized", "validated", "executed", "conditioned", "recovered"]
        }));
        for gate in [
            "recognized",
            "validated",
            "executed",
            "conditioned",
            "recovered",
        ] {
            new_bindings.push(json!({
                "spec_version": "mainframe-env.conformance-ir@1",
                "row_id": row_id,
                "obligation_id": "command-contract",
                "gate": gate,
                "test_id": format!("dataset.ams.{command}.{gate}"),
                "driver": "dataset.ams.driver",
                "input": fixture_id,
                "preconditions": [],
                "expected": [format!("observe.{fixture_id}")],
                "recovery": "dataset.ams.restart",
                "oracle": null
            }));
        }
    }
    spec.get_mut("rows")
        .and_then(Value::as_array_mut)
        .ok_or("conformance spec rows are missing")?
        .extend(new_rows);
    spec.get_mut("obligations")
        .and_then(Value::as_array_mut)
        .ok_or("conformance spec obligations are missing")?
        .extend(new_obligations);
    spec.get_mut("cases")
        .and_then(Value::as_array_mut)
        .ok_or("conformance spec cases are missing")?
        .extend(new_bindings);
    Ok(())
}

fn official_catalog_rows(root: &Path) -> TaskResult<Vec<OfficialCatalogRow>> {
    let index_path = root.join("conformance/0.2/catalogs/index.json");
    let index = json(&index_path)?;
    let catalogs = indexed_catalog_closure(root, &index, &index_path)?;
    let mut rows = Vec::new();
    for (subsystem, catalog_path) in catalogs {
        let catalog = json(&catalog_path)?;
        for unit in array(&catalog, "units", &catalog_path)? {
            let family = text(unit, "id", &catalog_path)?;
            for row in array(unit, "rows", &catalog_path)? {
                rows.push(
                    OfficialCatalogRow::new(
                        text(row, "id", &catalog_path)?,
                        &subsystem,
                        family,
                        text(row, "source_locator", &catalog_path)?,
                        CoverageGate::ALL,
                        ConformanceLimits::default(),
                    )
                    .map_err(|problem| problem.to_string())?,
                );
            }
        }
    }
    require(
        rows.len() == 1_506,
        "shared spec compiler did not load the frozen 1,506-row catalog",
    )?;
    Ok(rows)
}

fn indexed_catalog_closure(
    root: &Path,
    index: &Value,
    index_path: &Path,
) -> TaskResult<Vec<(String, PathBuf)>> {
    let mut catalogs = Vec::new();
    for baseline in array(index, "baselines", index_path)? {
        let subsystem = text(baseline, "subsystem", index_path)?.to_string();
        let catalog_relative = text(baseline, "catalog", index_path)?;
        require(
            catalog_relative.starts_with("conformance/0.2/catalogs/")
                && catalog_relative.ends_with(".json")
                && !catalog_relative.contains(".."),
            &format!("indexed catalog path is unsafe: {catalog_relative}"),
        )?;
        let catalog_path = root.join(catalog_relative);
        let expected = text(baseline, "catalog_sha256", index_path)?;
        validate_sha256_identity(expected, "indexed catalog digest")?;
        let actual = format!("sha256:{}", file_digest(&catalog_path)?);
        require(
            actual == expected,
            &format!("indexed catalog digest drifted: {catalog_relative}"),
        )?;
        catalogs.push((subsystem, catalog_path));
    }
    Ok(catalogs)
}

fn check_focused_conformance_interface(root: &Path, args: &ConformanceArgs) -> TaskResult {
    require(
        args.replay.is_none() || args.subsystem.is_none(),
        "--replay and --subsystem are mutually exclusive",
    )?;
    require(
        args.subsystem.is_some() || args.replay.is_some(),
        "focused conformance requires --subsystem or --replay",
    )?;
    require(
        args.replay.is_none() || (args.gate.is_none() && args.shard.is_none()),
        "--replay cannot be combined with --gate or --shard",
    )?;
    let dataset_racf_or_jcl_selected = matches!(
        args.subsystem.as_deref(),
        Some("dataset-vsam-ams" | "racf-saf" | "jcl-jes2")
    ) || args.replay.as_deref().is_some_and(|replay| {
        replay.starts_with("dataset.") || replay.starts_with("racf.") || replay.starts_with("jcl.")
    });
    if dataset_racf_or_jcl_selected {
        return check_focused_dataset_or_jcl_conformance_interface(root, args);
    }
    let cobol_selected = args.subsystem.as_deref() == Some("cobol")
        || args
            .replay
            .as_deref()
            .is_some_and(|replay| replay.starts_with("cobol."));
    require(
        cobol_selected,
        "selected subsystem product driver registry is not installed",
    )?;
    let limits = ConformanceLimits::default();
    let gate = args.gate.as_deref().map(parse_coverage_gate).transpose()?;
    let spec = compile_shared_spec(root)?;
    let selection = if let Some(replay) = args.replay.as_deref() {
        RunnerSelection::replay(replay, limits).map_err(|problem| problem.to_string())?
    } else {
        RunnerSelection::focused(
            args.subsystem
                .as_deref()
                .ok_or("focused conformance requires --subsystem")?,
            gate,
            args.shard,
            limits,
        )
        .map_err(|problem| problem.to_string())?
    };
    let dataset_handlers = dataset_conformance_runtime();
    let jcl_handlers = jcl_conformance::runtime();
    let runtime = combined_conformance_runtime(&spec, &dataset_handlers, &jcl_handlers, limits)?;
    let context = RunnerContext::new(candidate_digest(root)?, "local", limits)
        .map_err(|problem| problem.to_string())?;
    let report = ConformanceRunner::new(&spec, runtime, limits)
        .run(&selection, &context)
        .map_err(|problem| problem.to_string())?;
    let mut passed = 0usize;
    let mut failed = 0usize;
    for event in report.batches.iter().flat_map(|batch| batch.events.iter()) {
        println!(
            "{}",
            String::from_utf8(
                event
                    .canonical_json()
                    .map_err(|problem| problem.to_string())?
            )
            .map_err(|error| error.to_string())?
        );
        match event.verdict {
            Verdict::Pass => passed += 1,
            Verdict::Fail => failed += 1,
        }
    }
    println!(
        "conformance-ledger spec-digest={} batches={} verdicts={} pass={} fail={}",
        spec.spec_digest(),
        report.batches.len(),
        passed + failed,
        passed,
        failed
    );
    require(failed == 0, "focused conformance produced failing verdicts")
}

fn check_cobol_exit(root: &Path) -> TaskResult {
    check_spec(root)?;
    let receipt = verify_cobol_exit()?;
    require(
        command_text(root, "git", &["rev-parse", "mainframe-env-v0.1.1^{}"])?
            == "44f3081eb2fdf22d09e1a97725f5a4163431ca70",
        "accepted 0.1.1 release commit moved",
    )?;
    require(
        command_text(root, "git", &["rev-parse", "mainframe-env-v0.1.1^{tree}"])?
            == "3d504ece02f1c09e124606ded695b00ba984d104",
        "accepted 0.1.1 release tree moved",
    )?;
    let versions: Value = serde_json::from_str(&command_text(
        root,
        "git",
        &[
            "show",
            "mainframe-env-v0.1.1:conformance/0.1/inventory/versions.json",
        ],
    )?)
    .map_err(|error| format!("accepted 0.1.1 version inventory: {error}"))?;
    require(
        versions["contracts"]["artifact"] == "mainframe-env.artifact@1"
            && versions["contracts"]["ir_binary"] == "mainframe-env.ir-binary@1"
            && versions["contracts"]["ir_envelope"] == "mainframe-env.ir-envelope@1",
        "accepted 0.1.1 artifact compatibility contracts drifted",
    )?;

    let limits = ConformanceLimits::default();
    let spec = compile_shared_spec(root)?;
    let selection = RunnerSelection::focused("cobol", None, None, limits)
        .map_err(|problem| problem.to_string())?;
    let context = RunnerContext::new(candidate_digest(root)?, "local", limits)
        .map_err(|problem| problem.to_string())?;
    let dataset_handlers = dataset_conformance_runtime();
    let jcl_handlers = jcl_conformance::runtime();
    let runtime = combined_conformance_runtime(&spec, &dataset_handlers, &jcl_handlers, limits)?;
    let report = ConformanceRunner::new(&spec, runtime, limits)
        .run(&selection, &context)
        .map_err(|problem| problem.to_string())?;
    let events = report
        .batches
        .iter()
        .flat_map(|batch| batch.events.iter())
        .collect::<Vec<_>>();
    let structural_events = events
        .iter()
        .copied()
        .filter(|event| {
            matches!(
                event.key.gate,
                CoverageGate::Recognized | CoverageGate::Validated
            )
        })
        .collect::<Vec<_>>();
    require(
        structural_events.len() == 346
            && structural_events
                .iter()
                .all(|event| event.verdict == Verdict::Pass)
            && structural_events
                .iter()
                .map(|event| event.cache_identity.as_str())
                .collect::<BTreeSet<_>>()
                .len()
                == 346
            && structural_events.iter().all(|event| {
                event.replay == format!("cargo xtask conformance --replay {}", event.test_id)
            }),
        "COBOL verdict, cache, or replay closure drifted",
    )?;
    let expected_batches = spec
        .expected_shards()
        .keys()
        .filter(|shard| shard.subsystem == "cobol")
        .count();
    require(
        report.batches.len() == expected_batches,
        "COBOL shard closure drifted",
    )?;
    let cobol_rows = report
        .ledger
        .rows
        .values()
        .filter(|row| row.subsystem == "cobol")
        .collect::<Vec<_>>();
    require(
        cobol_rows.len() == 173
            && cobol_rows.iter().all(|row| {
                row.gates[&CoverageGate::Recognized].state == GateState::Passed
                    && row.gates[&CoverageGate::Validated].state == GateState::Passed
            }),
        "COBOL structural ledger numerators drifted",
    )?;
    let mut incomplete = report.batches.clone();
    incomplete.pop();
    require(
        validate_verdict_batches(&spec, &selection, &incomplete).is_err(),
        "COBOL omitted-shard mutant survived",
    )?;
    println!(
        "cobol-exit rows={} bindings={} shards={} malformed={} limits={} recovery={} prior-artifact-bytes={}",
        receipt.official_rows,
        structural_events.len(),
        report.batches.len(),
        receipt.malformed_classes,
        receipt.limit_classes,
        receipt.recovery_classes,
        receipt.prior_artifact_bytes,
    );
    Ok(())
}

fn check_cobol_reference(root: &Path, receipt_path: &Path) -> TaskResult {
    require(
        receipt_path.is_absolute(),
        "GnuCOBOL reference receipt path must be absolute",
    )?;
    let parent = receipt_path
        .parent()
        .ok_or("GnuCOBOL reference receipt has no parent directory")?;
    let canonical_parent = fs::canonicalize(parent)
        .map_err(|error| format!("GnuCOBOL reference receipt parent: {error}"))?;
    let canonical_root = fs::canonicalize(root).map_err(|error| error.to_string())?;
    require(
        !canonical_parent.starts_with(&canonical_root),
        "GnuCOBOL reference receipt must remain outside the candidate tree",
    )?;
    let file_name = receipt_path
        .file_name()
        .ok_or("GnuCOBOL reference receipt path has no file name")?;
    let receipt_path = canonical_parent.join(file_name);
    require(
        !receipt_path.exists(),
        "GnuCOBOL reference receipt path already exists; use a fresh path",
    )?;
    let candidate = candidate_digest(root)?;
    let receipt = run_gnucobol_reference_campaign(&candidate)?;
    require(
        receipt.reference == "gnucobol"
            && receipt.licensed_differential_credit == 0
            && receipt.licensed_cases_pending == 153
            && receipt.case_count == 16
            && receipt.passed_cases == 16
            && receipt.mutants_killed >= 4,
        "GnuCOBOL reference campaign report drifted",
    )?;
    let value = serde_json::to_value(&receipt).map_err(|error| error.to_string())?;
    let schema_path =
        root.join("conformance/spec/schemas/cobol-gnucobol-reference-receipt.schema.json");
    validate_schema_instance(&json(&schema_path)?, &value, &receipt_path)?;
    let mut bytes = serde_json::to_vec_pretty(&value).map_err(|error| error.to_string())?;
    bytes.push(b'\n');
    let mut output = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&receipt_path)
        .map_err(|error| format!("create GnuCOBOL reference receipt: {error}"))?;
    output
        .write_all(&bytes)
        .map_err(|error| format!("write GnuCOBOL reference receipt: {error}"))?;
    output
        .sync_all()
        .map_err(|error| format!("sync GnuCOBOL reference receipt: {error}"))?;
    println!(
        "cobol-reference reference=gnucobol version=3.2.0 cases={}/{} mutants={} licensed-differential=0/153-pending candidate={} fixtures={} receipt={} receipt-digest=sha256:{:x}",
        receipt.passed_cases,
        receipt.case_count,
        receipt.mutants_killed,
        receipt.candidate_identity,
        receipt.fixture_digest,
        receipt_path.display(),
        Sha256::digest(&bytes),
    );
    Ok(())
}

fn check_focused_dataset_or_jcl_conformance_interface(
    root: &Path,
    args: &ConformanceArgs,
) -> TaskResult {
    let gate = args.gate.as_deref().map(parse_coverage_gate).transpose()?;
    let spec = compile_shared_spec(root)?;
    let selected = spec
        .cases()
        .filter(|case| {
            if let Some(replay) = args.replay.as_deref() {
                return case.test_id().as_str() == replay;
            }
            let Some(row) = spec.catalog_row(&case.key().row_id) else {
                return false;
            };
            let subsystem_matches = args
                .subsystem
                .as_deref()
                .is_some_and(|subsystem| subsystem == row.subsystem());
            let gate_matches = gate.is_none_or(|gate| gate == case.key().gate);
            let shard_matches = args.shard.is_none_or(|bucket| {
                spec.expected_shards().iter().any(|(shard, bindings)| {
                    shard.bucket == bucket && bindings.contains(case.key())
                })
            });
            subsystem_matches && gate_matches && shard_matches
        })
        .count();
    require(
        selected > 0,
        "focused conformance selection has no executable bindings",
    )?;
    let racf_selected = args.subsystem.as_deref() == Some("racf-saf")
        || args
            .replay
            .as_deref()
            .is_some_and(|replay| replay.starts_with("racf."));
    let is_dataset = args.subsystem.as_deref() == Some("dataset-vsam-ams")
        || args
            .replay
            .as_deref()
            .is_some_and(|test| test.starts_with("dataset."));
    if is_dataset {
        let selection = if let Some(replay) = args.replay.as_deref() {
            RunnerSelection::replay(replay, ConformanceLimits::default())
        } else {
            RunnerSelection::focused(
                args.subsystem
                    .as_deref()
                    .ok_or("dataset conformance subsystem is missing")?,
                gate,
                args.shard,
                ConformanceLimits::default(),
            )
        }
        .map_err(|problem| problem.to_string())?;
        let context = RunnerContext::new(
            repository_digest(root)?,
            "local-focused",
            ConformanceLimits::default(),
        )
        .map_err(|problem| problem.to_string())?;
        let dataset_handlers = dataset_conformance_runtime();
        let jcl_handlers = jcl_conformance::runtime();
        let runtime = combined_conformance_runtime(
            &spec,
            &dataset_handlers,
            &jcl_handlers,
            ConformanceLimits::default(),
        )
        .map_err(|problem| problem.to_string())?;
        let report = ConformanceRunner::new(&spec, runtime, ConformanceLimits::default())
            .run(&selection, &context)
            .map_err(|problem| problem.to_string())?;
        let simulation = run_dataset_reference_simulation()?;
        let failures = report
            .batches
            .iter()
            .flat_map(|batch| &batch.events)
            .filter(|event| event.verdict != Verdict::Pass)
            .map(|event| format!("{}: {}", event.test_id, event.actual))
            .collect::<Vec<_>>();
        require(
            failures.is_empty(),
            &format!("dataset conformance failures: {failures:?}"),
        )?;
        let events = report
            .batches
            .iter()
            .map(|batch| batch.events.len())
            .sum::<usize>();
        println!(
            "dataset-conformance bindings={selected} events={events} batches={} reference-organizations={} reference-commands={} reference-properties={} observation-perturbations-rejected={} differential-credit={}",
            report.batches.len(),
            simulation.organization_rows,
            simulation.command_rows,
            simulation.property_cases,
            simulation.observation_perturbations_rejected,
            simulation.differential_credit,
        );
        return Ok(());
    }
    let jcl_selected = args.subsystem.as_deref() == Some("jcl-jes2")
        || args
            .replay
            .as_deref()
            .is_some_and(|replay| replay.starts_with("jcl."));
    require(
        racf_selected || jcl_selected,
        "selected subsystem product driver registry is not installed",
    )?;
    if racf_selected {
        return run_focused_racf(root, args, gate, &spec, selected);
    }
    run_focused_jcl(root, args, gate, &spec, selected)
}

fn run_focused_racf(
    root: &Path,
    args: &ConformanceArgs,
    gate: Option<CoverageGate>,
    spec: &CompiledSpec,
    selected: usize,
) -> TaskResult {
    let limits = ConformanceLimits::default();
    let selection = if let Some(replay) = args.replay.as_deref() {
        RunnerSelection::replay(replay, limits).map_err(|problem| problem.to_string())?
    } else {
        RunnerSelection::focused("racf-saf", gate, args.shard, limits)
            .map_err(|problem| problem.to_string())?
    };
    let context = RunnerContext::new(repository_digest(root)?, "local-deterministic", limits)
        .map_err(|problem| problem.to_string())?;
    let dataset_handlers = dataset_conformance_runtime();
    let jcl_handlers = jcl_conformance::runtime();
    let runtime = combined_conformance_runtime(spec, &dataset_handlers, &jcl_handlers, limits)?;
    let report = ConformanceRunner::new(spec, runtime, limits)
        .run(&selection, &context)
        .map_err(|problem| problem.to_string())?;
    if let Some(failure) = report
        .batches
        .iter()
        .flat_map(|batch| &batch.events)
        .find(|event| event.verdict == Verdict::Fail)
    {
        return Err(format!(
            "{} failed: expected={} actual={} replay={}",
            failure.test_id.as_str(),
            failure.expected,
            failure.actual,
            failure.replay
        ));
    }
    let counts = CoverageGate::ALL
        .into_iter()
        .map(|gate| {
            let count = &report.ledger.counts[&gate];
            format!(
                "{}={}/{}/{}/{}",
                gate.slug(),
                count.pass,
                count.fail,
                count.pending,
                count.non_applicable
            )
        })
        .collect::<Vec<_>>()
        .join(" ");
    println!("bindings={selected} {counts}");
    Ok(())
}

fn run_focused_jcl(
    root: &Path,
    args: &ConformanceArgs,
    gate: Option<CoverageGate>,
    spec: &CompiledSpec,
    selected: usize,
) -> TaskResult {
    let limits = ConformanceLimits::default();
    let selection = if let Some(replay) = args.replay.as_deref() {
        RunnerSelection::replay(replay, limits).map_err(|problem| problem.to_string())?
    } else {
        RunnerSelection::focused(
            args.subsystem.as_deref().unwrap_or("jcl-jes2"),
            gate,
            args.shard,
            limits,
        )
        .map_err(|problem| problem.to_string())?
    };
    let context = RunnerContext::new(repository_digest(root)?, "local-deterministic", limits)
        .map_err(|problem| problem.to_string())?;
    let dataset_handlers = dataset_conformance_runtime();
    let jcl_handlers = jcl_conformance::runtime();
    let runtime = combined_conformance_runtime(spec, &dataset_handlers, &jcl_handlers, limits)?;
    let report = ConformanceRunner::new(spec, runtime, limits)
        .run(&selection, &context)
        .map_err(|problem| problem.to_string())?;
    require(
        report
            .batches
            .iter()
            .flat_map(|batch| &batch.events)
            .all(|event| event.verdict == Verdict::Pass),
        "focused JCL conformance emitted one or more failing verdicts",
    )?;
    let artifact_directory = root.join("target/conformance/jcl-jes2");
    fs::create_dir_all(&artifact_directory).map_err(|error| error.to_string())?;
    let events = report
        .batches
        .iter()
        .flat_map(|batch| batch.events.iter())
        .map(|event| {
            let bytes = event
                .canonical_json()
                .map_err(|problem| problem.to_string())?;
            serde_json::from_slice::<Value>(&bytes).map_err(|error| error.to_string())
        })
        .collect::<TaskResult<Vec<_>>>()?;
    let ledger_bytes = report
        .ledger
        .canonical_json()
        .map_err(|problem| problem.to_string())?;
    fs::write(
        artifact_directory.join("verdicts.json"),
        pretty_json(&json!({
            "schema_version": "mainframe-env.conformance-verdict-stream@1",
            "spec_digest": spec.spec_digest(),
            "selected_bindings": selected,
            "events": events,
        }))?,
    )
    .map_err(|error| error.to_string())?;
    fs::write(artifact_directory.join("ledger.json"), &ledger_bytes)
        .map_err(|error| error.to_string())?;
    let counts = CoverageGate::ALL
        .into_iter()
        .map(|gate| {
            let mut pass = 0usize;
            let mut fail = 0usize;
            let mut pending = 0usize;
            let mut not_applicable = 0usize;
            for row in report
                .ledger
                .rows
                .values()
                .filter(|row| row.subsystem == "jcl-jes2")
            {
                match row.gates[&gate].state {
                    mainframe_env_coverage::GateState::Passed => pass += 1,
                    mainframe_env_coverage::GateState::Failed => fail += 1,
                    mainframe_env_coverage::GateState::Pending => pending += 1,
                    mainframe_env_coverage::GateState::NotApplicable => not_applicable += 1,
                }
            }
            (gate.slug(), pass, fail, pending, not_applicable)
        })
        .collect::<Vec<_>>();
    println!(
        "jcl-conformance spec={} bindings={} verdicts={} shards={} counts={counts:?}",
        spec.spec_digest(),
        selected,
        events.len(),
        report.batches.len(),
    );
    Ok(())
}

fn combined_conformance_runtime<'a>(
    spec: &CompiledSpec,
    dataset: &'a DatasetConformanceRuntime,
    jcl: &'a jcl_conformance::JclConformanceRuntime,
    limits: ConformanceLimits,
) -> Result<RuntimeRegistry<'a>, String> {
    let cobol = mainframe_env_conformance::cobol_conformance_handlers(limits)?;
    let mut drivers = cobol.drivers;
    drivers.extend(
        dataset
            .drivers(limits)
            .map_err(|problem| problem.to_string())?,
    );
    drivers.extend(jcl.drivers(limits).map_err(|problem| problem.to_string())?);
    let mut predicates = cobol.predicates;
    predicates.extend(
        jcl.predicates(limits)
            .map_err(|problem| problem.to_string())?,
    );
    let mut observations = cobol.observations;
    observations.extend(
        dataset
            .observations(limits)
            .map_err(|problem| problem.to_string())?,
    );
    observations.extend(
        jcl.observations(limits)
            .map_err(|problem| problem.to_string())?,
    );
    mainframe_env_conformance::racf_runtime_with(spec, drivers, predicates, observations, limits)
        .map_err(|problem| problem.to_string())
}

fn candidate_digest(root: &Path) -> TaskResult<String> {
    let output = Command::new("git")
        .args([
            "ls-files",
            "--cached",
            "--others",
            "--exclude-standard",
            "-z",
        ])
        .current_dir(root)
        .output()
        .map_err(|error| format!("list candidate files: {error}"))?;
    require(
        output.status.success(),
        "git ls-files failed for candidate identity",
    )?;
    let mut files = output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .map(|path| String::from_utf8(path.to_vec()).map_err(|error| error.to_string()))
        .collect::<TaskResult<Vec<_>>>()?;
    files.sort();
    let mut digest = Sha256::new();
    for relative in files {
        let bytes = fs::read(root.join(&relative))
            .map_err(|error| format!("candidate file {relative}: {error}"))?;
        digest.update((relative.len() as u64).to_be_bytes());
        digest.update(relative.as_bytes());
        digest.update((bytes.len() as u64).to_be_bytes());
        digest.update(bytes);
    }
    Ok(format!("sha256:{:x}", digest.finalize()))
}

fn check_jcl_exit(root: &Path) -> TaskResult {
    let receipt = verify_jcl_exit()?;
    require(
        receipt.status == "pass-with-licensed-differential-pending"
            && receipt.official_rows == 237
            && receipt.valid_fixture_plans == 237
            && receipt.invalid_fixture_rejections == 237
            && receipt.deterministic_plan_pairs == 237
            && receipt.forbidden_mutation_cases == 474
            && receipt.malformed_recovery_cases >= 6
            && receipt.scale_boundary_cases == 4
            && receipt.compatibility_plans >= 9
            && receipt.carddemo_representative_plans >= 9
            && receipt.licensed_differential == "pending-no-pinned-licensed-oracle-receipt",
        "JCL-706 exit matrix is incomplete",
    )?;
    let oracle_path = root.join("conformance/0.7/oracles/jcl-licensed-differential.json");
    let oracle = json(&oracle_path)?;
    validate_schema_instance(
        &json(&root.join("conformance/0.7/schemas/jcl-licensed-differential-adapter.schema.json"))?,
        &oracle,
        &oracle_path,
    )?;
    require(
        oracle["schema_version"]
            == Value::String("mainframe-env.jcl-licensed-differential-adapter@1".into())
            && oracle["target_version"] == Value::String("0.7.0".into())
            && oracle["status"] == Value::String("pending".into())
            && oracle["licensed_receipt_required_for_pass"] == Value::Bool(true)
            && oracle["generated_or_historical_result_counts_as_pass"] == Value::Bool(false),
        "JCL licensed differential adapter policy is invalid",
    )?;
    let gate_map_path = root.join("conformance/0.7/inventory/jcl-path-gate-map.json");
    let gate_map = json(&gate_map_path)?;
    validate_schema_instance(
        &json(&root.join("conformance/0.7/schemas/jcl-path-gate-map.schema.json"))?,
        &gate_map,
        &gate_map_path,
    )?;
    require(
        gate_map["default_policy"] == Value::String("fail-closed".into())
            && gate_map["mappings"]
                .as_array()
                .is_some_and(|mappings| mappings.len() == 6),
        "JCL affected path/contract-to-gate map is incomplete",
    )?;
    println!(
        "{}",
        serde_json::to_string_pretty(&receipt).map_err(|error| error.to_string())?
    );
    Ok(())
}

fn parse_coverage_gate(value: &str) -> TaskResult<CoverageGate> {
    CoverageGate::ALL
        .into_iter()
        .find(|gate| gate.slug() == value)
        .ok_or_else(|| format!("unknown coverage gate {value}"))
}

fn generate_evidence_seal(root: &Path) -> TaskResult {
    evidence_seal::generate(root)
}

fn check_evidence_seal(root: &Path) -> TaskResult {
    evidence_seal::check(root)
}

fn write_evidence_callback(root: &Path) -> TaskResult {
    evidence_seal::callback(root)
}

fn check_carddemo_cics(root: &Path) -> TaskResult {
    let receipt = verify_carddemo_cics_abi_from_env(
        &root.join("conformance/0.1.1/inventory/carddemo-corpus.json"),
    )
    .map_err(|problem| problem.to_string())?;
    let receipt_value = serde_json::to_value(&receipt).map_err(|error| error.to_string())?;
    let receipt_digest = format!(
        "sha256:{:x}",
        Sha256::digest(serde_json::to_vec(&receipt_value).map_err(|e| e.to_string())?)
    );
    println!(
        "{}",
        serde_json::to_string_pretty(&receipt).map_err(|e| e.to_string())?
    );
    let evidence = json(&root.join("conformance/0.1.1/evidence/issues/CD-012.json"))?;
    require(
        evidence["cics_receipt"] == receipt_value,
        "CD-012 CICS receipt is stale",
    )?;
    require(
        evidence["evidence_digest"].as_str() == Some(receipt_digest.as_str()),
        "CD-012 evidence digest differs",
    )?;
    Ok(())
}

fn check_carddemo_cics_runtime(root: &Path) -> TaskResult {
    let receipt = verify_carddemo_cics_runtime_from_env(
        &root.join("conformance/0.1.1/inventory/carddemo-corpus.json"),
    )
    .map_err(|problem| problem.to_string())?;
    let receipt_value = serde_json::to_value(&receipt).map_err(|error| error.to_string())?;
    let receipt_digest = format!(
        "sha256:{:x}",
        Sha256::digest(serde_json::to_vec(&receipt_value).map_err(|error| error.to_string())?)
    );
    println!(
        "{}",
        serde_json::to_string_pretty(&receipt).map_err(|error| error.to_string())?
    );
    let evidence = json(&root.join("conformance/0.1.1/evidence/issues/CD-013.json"))?;
    require(
        evidence["issue"] == Value::String("CD-013".into())
            && evidence["derived"] == Value::Bool(true)
            && evidence["status"] == Value::String("pass".into()),
        "CD-013 evidence is not a derived pass",
    )?;
    require(
        evidence["cics_runtime_receipt"] == receipt_value,
        "CD-013 CICS runtime receipt is stale",
    )?;
    require(
        evidence["evidence_digest"].as_str() == Some(receipt_digest.as_str()),
        "CD-013 evidence digest differs",
    )?;
    Ok(())
}

fn check_carddemo_vsam(root: &Path) -> TaskResult {
    let receipt = verify_carddemo_vsam_from_env(
        &root.join("conformance/0.1.1/inventory/carddemo-corpus.json"),
    )
    .map_err(|problem| problem.to_string())?;
    let receipt_value = serde_json::to_value(&receipt).map_err(|error| error.to_string())?;
    let receipt_digest = format!(
        "sha256:{:x}",
        Sha256::digest(serde_json::to_vec(&receipt_value).map_err(|error| error.to_string())?)
    );
    println!(
        "{}",
        serde_json::to_string_pretty(&receipt).map_err(|error| error.to_string())?
    );
    let evidence = json(&root.join("conformance/0.1.1/evidence/issues/CD-014.json"))?;
    require(
        evidence["issue"] == Value::String("CD-014".into())
            && evidence["derived"] == Value::Bool(true)
            && evidence["status"] == Value::String("pass".into()),
        "CD-014 evidence is not a derived pass",
    )?;
    require(
        evidence["vsam_receipt"] == receipt_value,
        "CD-014 VSAM receipt is stale",
    )?;
    require(
        evidence["evidence_digest"].as_str() == Some(receipt_digest.as_str()),
        "CD-014 evidence digest differs",
    )?;
    Ok(())
}

fn check_carddemo_dataset_catalog(root: &Path) -> TaskResult {
    let receipt = verify_carddemo_dataset_catalog_from_env(
        &root.join("conformance/0.1.1/inventory/carddemo-corpus.json"),
    )
    .map_err(|problem| problem.to_string())?;
    let receipt_value = serde_json::to_value(&receipt).map_err(|error| error.to_string())?;
    let receipt_digest = format!(
        "sha256:{:x}",
        Sha256::digest(serde_json::to_vec(&receipt_value).map_err(|error| error.to_string())?)
    );
    println!(
        "{}",
        serde_json::to_string_pretty(&receipt).map_err(|error| error.to_string())?
    );
    let evidence = json(&root.join("conformance/0.1.1/evidence/issues/CD-015.json"))?;
    require(
        evidence["issue"] == Value::String("CD-015".into())
            && evidence["derived"] == Value::Bool(true)
            && evidence["status"] == Value::String("pass".into()),
        "CD-015 evidence is not a derived pass",
    )?;
    require(
        evidence["dataset_catalog_receipt"] == receipt_value,
        "CD-015 dataset catalog receipt is stale",
    )?;
    require(
        evidence["evidence_digest"].as_str() == Some(receipt_digest.as_str()),
        "CD-015 evidence digest differs",
    )?;
    Ok(())
}

fn check_carddemo_seeds(root: &Path) -> TaskResult {
    let receipt = verify_carddemo_seeds_from_env(
        &root.join("conformance/0.1.1/inventory/carddemo-corpus.json"),
    )
    .map_err(|problem| problem.to_string())?;
    let receipt_value = serde_json::to_value(&receipt).map_err(|error| error.to_string())?;
    let receipt_digest = format!(
        "sha256:{:x}",
        Sha256::digest(serde_json::to_vec(&receipt_value).map_err(|error| error.to_string())?)
    );
    println!(
        "{}",
        serde_json::to_string_pretty(&receipt).map_err(|error| error.to_string())?
    );
    let evidence = json(&root.join("conformance/0.1.1/evidence/issues/CD-016.json"))?;
    require(
        evidence["issue"] == Value::String("CD-016".into())
            && evidence["derived"] == Value::Bool(true)
            && evidence["status"] == Value::String("pass".into()),
        "CD-016 evidence is not a derived pass",
    )?;
    require(
        evidence["seed_receipt"] == receipt_value,
        "CD-016 seed receipt is stale",
    )?;
    require(
        evidence["evidence_digest"].as_str() == Some(receipt_digest.as_str()),
        "CD-016 evidence digest differs",
    )?;
    Ok(())
}

fn check_carddemo_security(root: &Path) -> TaskResult {
    let receipt = verify_carddemo_security_from_env(
        &root.join("conformance/0.1.1/inventory/carddemo-corpus.json"),
    )
    .map_err(|problem| problem.to_string())?;
    println!(
        "{}",
        serde_json::to_string_pretty(&receipt).map_err(|error| error.to_string())?
    );
    let evidence = json(&root.join("conformance/0.1.1/evidence/issues/CD-017.json"))?;
    let historical = evidence["security_receipt"]
        .as_object()
        .ok_or("CD-017 historical security receipt is malformed")?;
    let historical_digest = canonical_evidence_digest(historical)?;
    require(
        evidence["issue"] == Value::String("CD-017".into())
            && evidence["derived"] == Value::Bool(true)
            && evidence["status"] == Value::String("pass".into())
            && evidence["evidence_digest"].as_str() == Some(historical_digest.as_str()),
        "CD-017 evidence is not a derived pass",
    )?;
    require(
        receipt.status == "pass"
            && receipt.schema_version == "mainframe-env.carddemo-security-receipt@1"
            && receipt.corpus_commit == historical["corpus_commit"]
            && receipt.transport_users
                == historical["transport_users"].as_u64().unwrap_or(0) as usize
            && receipt.application_signon_records
                == historical["application_signon_records"]
                    .as_u64()
                    .unwrap_or(0) as usize
            && receipt.identities_distinct
            && receipt.groups == historical["groups"].as_u64().unwrap_or(0) as usize
            && receipt.profiles == historical["profiles"].as_u64().unwrap_or(0) as usize
            && receipt.permissions > 0
            && receipt.permissions <= historical["permissions"].as_u64().unwrap_or(0) as usize
            && receipt.resource_classes
                == serde_json::from_value::<Vec<String>>(historical["resource_classes"].clone())
                    .map_err(|error| error.to_string())?
            && receipt.transaction_profiles
                == historical["transaction_profiles"].as_u64().unwrap_or(0) as usize
            && receipt.program_profiles
                == historical["program_profiles"].as_u64().unwrap_or(0) as usize
            && receipt.dataset_profiles
                == historical["dataset_profiles"].as_u64().unwrap_or(0) as usize
            && receipt.queue_profiles
                == historical["queue_profiles"].as_u64().unwrap_or(0) as usize
            && receipt.regular_allow_checks
                == historical["regular_allow_checks"].as_u64().unwrap_or(0) as usize
            && receipt.regular_deny_checks
                == historical["regular_deny_checks"].as_u64().unwrap_or(0) as usize
            && receipt.admin_allow_checks
                == historical["admin_allow_checks"].as_u64().unwrap_or(0) as usize
            && receipt.redacted_fields
                == historical["redacted_fields"].as_u64().unwrap_or(0) as usize
            && receipt.manifest_replay
            && receipt.security_shape_sha256.len() == 64
            && receipt
                .security_shape_sha256
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit()),
        "current CardDemo security profile is not an exact least-privilege pass",
    )?;
    Ok(())
}

fn check_carddemo_terminal(root: &Path) -> TaskResult {
    let receipt = verify_carddemo_terminal_from_env(
        &root.join("conformance/0.1.1/inventory/carddemo-corpus.json"),
    )
    .map_err(|problem| problem.to_string())?;
    let receipt_value = serde_json::to_value(&receipt).map_err(|error| error.to_string())?;
    let receipt_digest = format!(
        "sha256:{:x}",
        Sha256::digest(serde_json::to_vec(&receipt_value).map_err(|error| error.to_string())?)
    );
    println!(
        "{}",
        serde_json::to_string_pretty(&receipt).map_err(|error| error.to_string())?
    );
    let evidence = json(&root.join("conformance/0.1.1/evidence/issues/CD-018.json"))?;
    require(
        evidence["issue"] == Value::String("CD-018".into())
            && evidence["derived"] == Value::Bool(true)
            && evidence["status"] == Value::String("pass".into()),
        "CD-018 evidence is not a derived pass",
    )?;
    require(
        evidence["terminal_receipt"] == receipt_value,
        "CD-018 terminal receipt is stale",
    )?;
    require(
        evidence["evidence_digest"].as_str() == Some(receipt_digest.as_str()),
        "CD-018 evidence digest differs",
    )?;
    Ok(())
}

fn check_carddemo_base_online(root: &Path) -> TaskResult {
    let receipt = verify_carddemo_base_online_from_env(
        &root.join("conformance/0.1.1/inventory/carddemo-corpus.json"),
    )
    .map_err(|problem| problem.to_string())?;
    let receipt_value = serde_json::to_value(&receipt).map_err(|error| error.to_string())?;
    let receipt_digest = format!(
        "sha256:{:x}",
        Sha256::digest(serde_json::to_vec(&receipt_value).map_err(|error| error.to_string())?)
    );
    println!(
        "{}",
        serde_json::to_string_pretty(&receipt).map_err(|error| error.to_string())?
    );
    let evidence = json(&root.join("conformance/0.1.1/evidence/issues/CD-019.json"))?;
    require(
        receipt.status == "pass"
            && receipt.journeys_passed == 9
            && evidence["issue"] == Value::String("CD-019".into())
            && evidence["derived"] == Value::Bool(true)
            && evidence["status"] == Value::String("pass".into()),
        "CD-019 evidence is not a complete derived pass",
    )?;
    require(
        evidence["base_online_receipt"] == receipt_value,
        "CD-019 base-online receipt is stale",
    )?;
    require(
        evidence["evidence_digest"].as_str() == Some(receipt_digest.as_str()),
        "CD-019 evidence digest differs",
    )?;
    Ok(())
}

fn check_carddemo_jcl(root: &Path) -> TaskResult {
    let receipt = verify_carddemo_jcl_from_env(
        &root.join("conformance/0.1.1/inventory/carddemo-corpus.json"),
    )
    .map_err(|problem| problem.to_string())?;
    let receipt_value = serde_json::to_value(&receipt).map_err(|error| error.to_string())?;
    println!(
        "{}",
        serde_json::to_string_pretty(&receipt).map_err(|error| error.to_string())?
    );
    let evidence = json(&root.join("conformance/0.1.1/evidence/issues/CD-020.json"))?;
    require(
        receipt.status == "pass"
            && receipt.jcl_files == 46
            && receipt.parsed_files + receipt.accepted_unsupported_files == 46
            && evidence["issue"] == Value::String("CD-020".into())
            && evidence["derived"] == Value::Bool(true)
            && evidence["status"] == Value::String("pass".into()),
        "CD-020 evidence is not a complete derived pass",
    )?;
    let mut current_projection = receipt_value.clone();
    let mut historical_projection = evidence["jcl_receipt"].clone();
    current_projection
        .as_object_mut()
        .ok_or("current CardDemo JCL receipt is not an object")?
        .remove("jcl_shape_sha256");
    historical_projection
        .as_object_mut()
        .ok_or("historical CardDemo JCL receipt is not an object")?
        .remove("jcl_shape_sha256");
    require(
        current_projection == historical_projection,
        "CD-020 semantic JCL receipt projection drifted",
    )?;
    let historical_digest = format!(
        "sha256:{:x}",
        Sha256::digest(
            serde_json::to_vec(&evidence["jcl_receipt"]).map_err(|error| error.to_string())?
        )
    );
    require(
        evidence["evidence_digest"].as_str() == Some(historical_digest.as_str()),
        "CD-020 historical evidence digest differs",
    )?;
    Ok(())
}

fn check_carddemo_utilities(root: &Path) -> TaskResult {
    let receipt = verify_carddemo_utilities_from_env(
        &root.join("conformance/0.1.1/inventory/carddemo-corpus.json"),
    )
    .map_err(|problem| problem.to_string())?;
    let receipt_value = serde_json::to_value(&receipt).map_err(|error| error.to_string())?;
    let receipt_digest = format!(
        "sha256:{:x}",
        Sha256::digest(serde_json::to_vec(&receipt_value).map_err(|error| error.to_string())?)
    );
    println!(
        "{}",
        serde_json::to_string_pretty(&receipt).map_err(|error| error.to_string())?
    );
    let evidence = json(&root.join("conformance/0.1.1/evidence/issues/CD-021.json"))?;
    require(
        receipt.status == "pass"
            && receipt.jcl_files == 46
            && evidence["issue"] == Value::String("CD-021".into())
            && evidence["derived"] == Value::Bool(true)
            && evidence["status"] == Value::String("pass".into()),
        "CD-021 evidence is not a complete derived pass",
    )?;
    require(
        evidence["utility_receipt"] == receipt_value,
        "CD-021 utility receipt is stale",
    )?;
    require(
        evidence["evidence_digest"].as_str() == Some(receipt_digest.as_str()),
        "CD-021 evidence digest differs",
    )?;
    Ok(())
}

fn check_carddemo_batch_programs(root: &Path) -> TaskResult {
    let receipt = verify_carddemo_batch_programs_from_env(
        &root.join("conformance/0.1.1/inventory/carddemo-corpus.json"),
    )
    .map_err(|problem| problem.to_string())?;
    let receipt_value = serde_json::to_value(&receipt).map_err(|error| error.to_string())?;
    let receipt_digest = format!(
        "sha256:{:x}",
        Sha256::digest(serde_json::to_vec(&receipt_value).map_err(|error| error.to_string())?)
    );
    println!(
        "{}",
        serde_json::to_string_pretty(&receipt).map_err(|error| error.to_string())?
    );
    let evidence = json(&root.join("conformance/0.1.1/evidence/issues/CD-022.json"))?;
    require(
        receipt.status == "pass"
            && receipt.named_programs.len() == 11
            && receipt.compiled_artifacts == 12
            && evidence["issue"] == Value::String("CD-022".into())
            && evidence["derived"] == Value::Bool(true)
            && evidence["status"] == Value::String("pass".into()),
        "CD-022 evidence is not a complete derived pass",
    )?;
    require(
        evidence["batch_program_receipt"] == receipt_value,
        "CD-022 batch-program receipt is stale",
    )?;
    require(
        evidence["evidence_digest"].as_str() == Some(receipt_digest.as_str()),
        "CD-022 evidence digest differs",
    )?;
    Ok(())
}

fn check_carddemo_base_batch(root: &Path) -> TaskResult {
    let receipt = verify_carddemo_base_batch_from_env(
        &root.join("conformance/0.1.1/inventory/carddemo-corpus.json"),
    )
    .map_err(|problem| problem.to_string())?;
    let receipt_value = serde_json::to_value(&receipt).map_err(|error| error.to_string())?;
    let receipt_digest = format!(
        "sha256:{:x}",
        Sha256::digest(serde_json::to_vec(&receipt_value).map_err(|error| error.to_string())?)
    );
    println!(
        "{}",
        serde_json::to_string_pretty(&receipt).map_err(|error| error.to_string())?
    );
    let historical_path = root.join("conformance/0.1.1/evidence/issues/CD-023.json");
    let evidence = json(&historical_path)?;
    require(
        receipt.status == "pass"
            && receipt.journeys_passed == 3
            && evidence["issue"] == Value::String("CD-023".into())
            && evidence["derived"] == Value::Bool(true)
            && evidence["status"] == Value::String("pass".into()),
        "CD-023 evidence is not a complete derived pass",
    )?;
    let historical_receipt = evidence["base_batch_receipt"]
        .as_object()
        .ok_or("CD-023 historical base-batch receipt is malformed")?;
    let historical_digest = canonical_evidence_digest(historical_receipt)?;
    require(
        evidence["evidence_digest"].as_str() == Some(historical_digest.as_str()),
        "CD-023 historical evidence digest differs",
    )?;
    let historical_completion = find_completion_commit(
        root,
        "Certify CardDemo base batch journeys",
        &[
            ("CardDemo-Issue", "CD-023=pass"),
            ("Evidence-Digest", historical_digest.as_str()),
            ("Target-Product", "0.1.1"),
        ],
    )?;
    verify_commit_bound_live_file(
        root,
        &historical_completion,
        "conformance/0.1.1/evidence/issues/CD-023.json",
        &historical_path,
    )?;

    let versioned_path = root.join("conformance/0.8/evidence/carddemo-base-batch.json");
    let expected = if versioned_path.is_file() {
        let versioned = json(&versioned_path)?;
        require(
            versioned["schema_version"]
                == Value::String("mainframe-env.carddemo-base-batch-version-evidence@1".into())
                && versioned["target_version"] == Value::String("0.8.0".into())
                && versioned["supersedes"]
                    == Value::String("conformance/0.1.1/evidence/issues/CD-023.json".into())
                && versioned["historical_receipt_rewritten"] == Value::Bool(false),
            "0.8 CardDemo base-batch evidence header is invalid",
        )?;
        let expected = versioned["receipt"]
            .as_object()
            .ok_or("0.8 CardDemo base-batch receipt is malformed")?;
        let expected_digest = canonical_evidence_digest(expected)?;
        require(
            versioned["evidence_digest"].as_str() == Some(expected_digest.as_str()),
            "0.8 CardDemo base-batch evidence digest differs",
        )?;
        for field in [
            "schema_version",
            "status",
            "corpus_commit",
            "journeys_passed",
            "initialization_jobs",
            "operational_jobs",
            "cics_file_controls",
            "internal_submissions",
            "warm_restart_controls",
            "rollback_controls",
            "cancellation_controls",
        ] {
            require(
                expected.get(field) == historical_receipt.get(field),
                &format!("0.8 CardDemo compatibility projection changed {field}"),
            )?;
        }
        Value::Object(expected.clone())
    } else {
        Value::Object(historical_receipt.clone())
    };
    require(
        expected == receipt_value,
        "CardDemo base-batch receipt is stale",
    )?;
    require(
        versioned_path.is_file()
            || evidence["evidence_digest"].as_str() == Some(receipt_digest.as_str()),
        "CD-023 evidence digest differs",
    )?;
    Ok(())
}

fn check_carddemo_db2(root: &Path) -> TaskResult {
    let receipt = verify_carddemo_db2_from_env(
        &root.join("conformance/0.1.1/inventory/carddemo-corpus.json"),
    )
    .map_err(|problem| problem.to_string())?;
    let receipt_value = serde_json::to_value(&receipt).map_err(|error| error.to_string())?;
    let receipt_digest = format!(
        "sha256:{:x}",
        Sha256::digest(serde_json::to_vec(&receipt_value).map_err(|error| error.to_string())?)
    );
    println!(
        "{}",
        serde_json::to_string_pretty(&receipt).map_err(|error| error.to_string())?
    );
    let evidence = json(&root.join("conformance/0.1.1/evidence/issues/CD-024.json"))?;
    require(
        receipt.status == "pass"
            && receipt.programs_compiled == 3
            && receipt.online_routes == 2
            && receipt.batch_routes == 3
            && evidence["issue"] == Value::String("CD-024".into())
            && evidence["derived"] == Value::Bool(true)
            && evidence["status"] == Value::String("pass".into()),
        "CD-024 evidence is not a complete derived pass",
    )?;
    require(
        evidence["db2_receipt"] == receipt_value,
        "CD-024 Db2 receipt is stale",
    )?;
    require(
        evidence["evidence_digest"].as_str() == Some(receipt_digest.as_str()),
        "CD-024 evidence digest differs",
    )?;
    Ok(())
}

fn check_carddemo_ims(root: &Path) -> TaskResult {
    let receipt = verify_carddemo_ims_from_env(
        &root.join("conformance/0.1.1/inventory/carddemo-corpus.json"),
    )
    .map_err(|problem| problem.to_string())?;
    let receipt_value = serde_json::to_value(&receipt).map_err(|error| error.to_string())?;
    let receipt_digest = format!(
        "sha256:{:x}",
        Sha256::digest(serde_json::to_vec(&receipt_value).map_err(|error| error.to_string())?)
    );
    println!(
        "{}",
        serde_json::to_string_pretty(&receipt).map_err(|error| error.to_string())?
    );
    let evidence = json(&root.join("conformance/0.1.1/evidence/issues/CD-025.json"))?;
    require(
        receipt.status == "pass"
            && receipt.definitions_checked == 8
            && receipt.databases_installed == 2
            && receipt.psbs_installed == 3
            && receipt.application_routes == 3
            && evidence["issue"] == Value::String("CD-025".into())
            && evidence["derived"] == Value::Bool(true)
            && evidence["status"] == Value::String("pass".into()),
        "CD-025 evidence is not a complete derived pass",
    )?;
    require(
        evidence["ims_receipt"] == receipt_value,
        "CD-025 IMS receipt is stale",
    )?;
    require(
        evidence["evidence_digest"].as_str() == Some(receipt_digest.as_str()),
        "CD-025 evidence digest differs",
    )?;
    Ok(())
}

fn check_carddemo_mq_authorization(root: &Path) -> TaskResult {
    let receipt = verify_carddemo_mq_authorization_from_env(
        &root.join("conformance/0.1.1/inventory/carddemo-corpus.json"),
    )
    .map_err(|problem| problem.to_string())?;
    let receipt_value = serde_json::to_value(&receipt).map_err(|error| error.to_string())?;
    let receipt_digest = format!(
        "sha256:{:x}",
        Sha256::digest(serde_json::to_vec(&receipt_value).map_err(|error| error.to_string())?)
    );
    println!(
        "{}",
        serde_json::to_string_pretty(&receipt).map_err(|error| error.to_string())?
    );
    let evidence = json(&root.join("conformance/0.1.1/evidence/issues/CD-026.json"))?;
    require(
        receipt.status == "pass"
            && receipt.programs_compiled == 3
            && receipt.queues_installed == 7
            && receipt.triggers_installed == 3
            && receipt.journeys_passed == 4
            && evidence["issue"] == Value::String("CD-026".into())
            && evidence["derived"] == Value::Bool(true)
            && evidence["status"] == Value::String("pass".into()),
        "CD-026 evidence is not a complete derived pass",
    )?;
    require(
        evidence["mq_authorization_receipt"] == receipt_value,
        "CD-026 MQ/authorization receipt is stale",
    )?;
    require(
        evidence["evidence_digest"].as_str() == Some(receipt_digest.as_str()),
        "CD-026 evidence digest differs",
    )?;
    Ok(())
}

fn check_carddemo_operator_install(root: &Path) -> TaskResult {
    let inventory = root.join("conformance/0.1.1/inventory/carddemo-corpus.json");
    let package = verify_carddemo_application_package_from_env(&inventory)
        .map_err(|problem| problem.to_string())?;
    let resources =
        verify_carddemo_resources_from_env(&inventory).map_err(|problem| problem.to_string())?;
    let catalog = verify_carddemo_dataset_catalog_from_env(&inventory)
        .map_err(|problem| problem.to_string())?;
    let seeds =
        verify_carddemo_seeds_from_env(&inventory).map_err(|problem| problem.to_string())?;
    require(
        package.status == "pass"
            && resources.status == "pass"
            && catalog.status == "pass"
            && seeds.status == "pass",
        "CardDemo operator install did not derive ready state",
    )?;
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "schema_version":"mainframe-env.carddemo-operator-install@1",
            "status":"pass",
            "package_identity":package.package_identity,
            "resources":resources.cross_references,
            "datasets":catalog.runtime_definitions,
            "seed_objects":seeds.seed_objects
        }))
        .map_err(|error| error.to_string())?
    );
    Ok(())
}

fn check_carddemo_operator_compile(root: &Path) -> TaskResult {
    let inventory = root.join("conformance/0.1.1/inventory/carddemo-corpus.json");
    let closure = verify_carddemo_source_closures_from_env(&inventory)
        .map_err(|problem| problem.to_string())?;
    let batch = verify_carddemo_batch_programs_from_env(&inventory)
        .map_err(|problem| problem.to_string())?;
    require(
        closure.status == "pass" && batch.status == "pass",
        "CardDemo operator compile did not publish its complete closure",
    )?;
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "schema_version":"mainframe-env.carddemo-operator-compile@1",
            "status":"pass",
            "programs":closure.programs_checked,
            "batch_artifacts":batch.compiled_artifacts
        }))
        .map_err(|error| error.to_string())?
    );
    Ok(())
}

fn check_carddemo_operator_submit(root: &Path) -> TaskResult {
    let receipt = verify_carddemo_base_batch_from_env(
        &root.join("conformance/0.1.1/inventory/carddemo-corpus.json"),
    )
    .map_err(|problem| problem.to_string())?;
    require(
        receipt.status == "pass" && receipt.journeys_passed == 3,
        "CardDemo operator submit did not complete the declared job set",
    )?;
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "schema_version":"mainframe-env.carddemo-operator-submit@1",
            "status":"pass",
            "initialization_jobs":receipt.initialization_jobs,
            "operational_jobs":receipt.operational_jobs,
            "spool_digests":receipt.spool_sha256.len()
        }))
        .map_err(|error| error.to_string())?
    );
    Ok(())
}

fn check_carddemo_operator_reset(root: &Path) -> TaskResult {
    let receipt = verify_carddemo_utilities_from_env(
        &root.join("conformance/0.1.1/inventory/carddemo-corpus.json"),
    )
    .map_err(|problem| problem.to_string())?;
    require(
        receipt.status == "pass" && receipt.selected_job_routes > 0,
        "CardDemo operator reset did not execute public utility routes",
    )?;
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "schema_version":"mainframe-env.carddemo-operator-reset@1",
            "status":"pass",
            "public_routes":receipt.selected_job_routes,
            "exact_mutations":receipt.exact_dataset_mutations
        }))
        .map_err(|error| error.to_string())?
    );
    Ok(())
}

fn check_carddemo_full(root: &Path) -> TaskResult {
    let receipt = verify_carddemo_full_from_env(
        &root.join("conformance/0.1.1/inventory/carddemo-corpus.json"),
    )
    .map_err(|problem| problem.to_string())?;
    let receipt_value = serde_json::to_value(&receipt).map_err(|error| error.to_string())?;
    let receipt_digest = format!(
        "sha256:{:x}",
        Sha256::digest(serde_json::to_vec(&receipt_value).map_err(|error| error.to_string())?)
    );
    println!(
        "{}",
        serde_json::to_string_pretty(&receipt).map_err(|error| error.to_string())?
    );
    let evidence = json(&root.join("conformance/0.1.1/evidence/issues/CD-027.json"))?;
    require(
        receipt.status == "pass"
            && receipt.issues_input_passed == 26
            && receipt.journeys_passed == 20
            && receipt.owned_commands.len() == 4
            && receipt.mixed_requests_completed == receipt.mixed_requests_offered
            && receipt.cdv1_disposition == "accepted-owned-source"
            && receipt.cdv1_public_routes == 2
            && !receipt.native_or_legacy_fallback_present
            && evidence["issue"] == Value::String("CD-027".into())
            && evidence["derived"] == Value::Bool(true)
            && evidence["status"] == Value::String("pass".into()),
        "CD-027 evidence is not a complete derived pass",
    )?;
    require(
        evidence["full_receipt"] == receipt_value,
        "CD-027 full receipt is stale",
    )?;
    require(
        evidence["evidence_digest"].as_str() == Some(receipt_digest.as_str()),
        "CD-027 evidence digest differs",
    )?;
    Ok(())
}

fn check_carddemo_programs(root: &Path) -> TaskResult {
    let receipt = verify_carddemo_program_routing_from_env(
        &root.join("conformance/0.1.1/inventory/carddemo-corpus.json"),
    )
    .map_err(|problem| problem.to_string())?;
    let receipt_value = serde_json::to_value(&receipt).map_err(|error| error.to_string())?;
    let receipt_digest = format!(
        "sha256:{:x}",
        Sha256::digest(serde_json::to_vec(&receipt_value).map_err(|error| error.to_string())?)
    );
    println!(
        "{}",
        serde_json::to_string_pretty(&receipt).map_err(|e| e.to_string())?
    );
    let evidence = json(&root.join("conformance/0.1.1/evidence/issues/CD-011.json"))?;
    require(
        evidence["issue"] == Value::String("CD-011".into())
            && evidence["derived"] == Value::Bool(true)
            && evidence["status"] == Value::String("pass".into()),
        "CD-011 evidence is not a derived pass",
    )?;
    require(
        evidence["program_receipt"] == receipt_value,
        "CD-011 program receipt is stale",
    )?;
    require(
        evidence["evidence_digest"].as_str() == Some(receipt_digest.as_str()),
        "CD-011 evidence digest differs",
    )?;
    Ok(())
}

fn check_carddemo_resources(root: &Path) -> TaskResult {
    let inventory_path = root.join("conformance/0.1.1/inventory/carddemo-corpus.json");
    let receipt = verify_carddemo_resources_from_env(&inventory_path)
        .map_err(|problem| problem.to_string())?;
    let receipt_value = serde_json::to_value(&receipt).map_err(|error| error.to_string())?;
    let receipt_bytes = serde_json::to_vec(&receipt_value).map_err(|error| error.to_string())?;
    let receipt_digest = format!("sha256:{:x}", Sha256::digest(receipt_bytes));
    println!(
        "{}",
        serde_json::to_string_pretty(&receipt).map_err(|e| e.to_string())?
    );
    let evidence = json(&root.join("conformance/0.1.1/evidence/issues/CD-010.json"))?;
    require(
        evidence["issue"] == Value::String("CD-010".into())
            && evidence["derived"] == Value::Bool(true)
            && evidence["status"] == Value::String("pass".into()),
        "CD-010 evidence is not a derived pass",
    )?;
    require(
        evidence["resource_receipt"] == receipt_value,
        "CD-010 resource receipt is stale",
    )?;
    require(
        evidence["evidence_digest"].as_str() == Some(receipt_digest.as_str()),
        "CD-010 evidence digest differs from resource receipt",
    )?;
    Ok(())
}

fn check_carddemo_package(root: &Path) -> TaskResult {
    let inventory_path = root.join("conformance/0.1.1/inventory/carddemo-corpus.json");
    let receipt = verify_carddemo_application_package_from_env(&inventory_path)
        .map_err(|problem| problem.to_string())?;
    let receipt_value = serde_json::to_value(&receipt).map_err(|error| error.to_string())?;
    let receipt_bytes = serde_json::to_vec(&receipt_value).map_err(|error| error.to_string())?;
    let receipt_digest = format!("sha256:{:x}", Sha256::digest(receipt_bytes));
    println!(
        "{}",
        serde_json::to_string_pretty(&receipt).map_err(|e| e.to_string())?
    );
    let package_inventory_path =
        root.join("conformance/0.1.1/inventory/carddemo-application-package.json");
    let package_inventory = json(&package_inventory_path)?;
    require(
        package_inventory["identity"] == receipt_value["package_identity"]
            && package_inventory["entries"] == receipt_value["entries"]
            && package_inventory["name"] == receipt_value["package_name"]
            && package_inventory["version"] == receipt_value["package_version"],
        "CardDemo application package inventory is stale",
    )?;
    let evidence_path = root.join("conformance/0.1.1/evidence/issues/CD-009.json");
    let evidence = json(&evidence_path)?;
    require(
        evidence["issue"] == Value::String("CD-009".into())
            && evidence["derived"] == Value::Bool(true)
            && evidence["status"] == Value::String("pass".into()),
        "CD-009 evidence is not a derived pass",
    )?;
    require(
        evidence["package_receipt"] == receipt_value,
        "CD-009 package receipt is stale",
    )?;
    require(
        evidence["evidence_digest"].as_str() == Some(receipt_digest.as_str()),
        "CD-009 evidence digest differs from its canonical package receipt",
    )?;
    Ok(())
}

fn check_carddemo_host(root: &Path) -> TaskResult {
    let inventory_path = root.join("conformance/0.1.1/inventory/carddemo-corpus.json");
    let receipt = verify_carddemo_host_operands_from_env(&inventory_path)
        .map_err(|problem| problem.to_string())?;
    let receipt_value = serde_json::to_value(&receipt)
        .map_err(|error| format!("CardDemo host receipt conversion failed: {error}"))?;
    let receipt_bytes = serde_json::to_vec(&receipt_value)
        .map_err(|error| format!("CardDemo host receipt canonicalization failed: {error}"))?;
    let receipt_digest = format!("sha256:{:x}", Sha256::digest(receipt_bytes));
    println!(
        "{}",
        serde_json::to_string_pretty(&receipt)
            .map_err(|error| format!("CardDemo host receipt serialization failed: {error}"))?
    );
    let evidence_path = root.join("conformance/0.1.1/evidence/issues/CD-008.json");
    let evidence = json(&evidence_path)?;
    require(
        evidence["issue"] == Value::String("CD-008".into())
            && evidence["derived"] == Value::Bool(true)
            && evidence["status"] == Value::String("pass".into()),
        "CD-008 evidence is not a derived pass",
    )?;
    require(
        evidence["host_receipt"] == receipt_value,
        "CD-008 evidence host receipt is stale",
    )?;
    require(
        evidence["evidence_digest"].as_str() == Some(receipt_digest.as_str()),
        "CD-008 evidence digest differs from its canonical host receipt",
    )?;
    Ok(())
}

fn check_carddemo_file_call(root: &Path) -> TaskResult {
    let inventory_path = root.join("conformance/0.1.1/inventory/carddemo-corpus.json");
    let receipt = verify_carddemo_file_call_semantics_from_env(&inventory_path)
        .map_err(|problem| problem.to_string())?;
    let receipt_value = serde_json::to_value(&receipt)
        .map_err(|error| format!("CardDemo file/call receipt conversion failed: {error}"))?;
    let receipt_bytes = serde_json::to_vec(&receipt_value)
        .map_err(|error| format!("CardDemo file/call receipt canonicalization failed: {error}"))?;
    let receipt_digest = format!("sha256:{:x}", Sha256::digest(receipt_bytes));
    println!(
        "{}",
        serde_json::to_string_pretty(&receipt)
            .map_err(|error| format!("CardDemo file/call receipt serialization failed: {error}"))?
    );
    let evidence_path = root.join("conformance/0.1.1/evidence/issues/CD-007.json");
    let evidence = json(&evidence_path)?;
    require(
        evidence["issue"] == Value::String("CD-007".to_string())
            && evidence["derived"] == Value::Bool(true)
            && evidence["status"] == Value::String("pass".to_string()),
        "CD-007 evidence is not a derived pass",
    )?;
    require(
        evidence["file_call_receipt"] == receipt_value,
        "CD-007 evidence file/call receipt is stale",
    )?;
    require(
        evidence["evidence_digest"].as_str() == Some(receipt_digest.as_str()),
        "CD-007 evidence digest differs from its canonical file/call receipt",
    )?;
    Ok(())
}

fn check_carddemo_core(root: &Path) -> TaskResult {
    let inventory_path = root.join("conformance/0.1.1/inventory/carddemo-corpus.json");
    let receipt = verify_carddemo_core_semantics_from_env(&inventory_path)
        .map_err(|problem| problem.to_string())?;
    let receipt_value = serde_json::to_value(&receipt)
        .map_err(|error| format!("CardDemo core receipt conversion failed: {error}"))?;
    let receipt_bytes = serde_json::to_vec(&receipt_value)
        .map_err(|error| format!("CardDemo core receipt canonicalization failed: {error}"))?;
    let receipt_digest = format!("sha256:{:x}", Sha256::digest(receipt_bytes));
    println!(
        "{}",
        serde_json::to_string_pretty(&receipt)
            .map_err(|error| format!("CardDemo core receipt serialization failed: {error}"))?
    );
    let evidence_path = root.join("conformance/0.1.1/evidence/issues/CD-006.json");
    let evidence = json(&evidence_path)?;
    require(
        evidence["issue"] == Value::String("CD-006".to_string())
            && evidence["derived"] == Value::Bool(true)
            && evidence["status"] == Value::String("pass".to_string()),
        "CD-006 evidence is not a derived pass",
    )?;
    require(
        evidence["core_receipt"] == receipt_value,
        "CD-006 evidence core receipt is stale",
    )?;
    require(
        evidence["evidence_digest"].as_str() == Some(receipt_digest.as_str()),
        "CD-006 evidence digest differs from its canonical core receipt",
    )?;
    Ok(())
}

fn check_carddemo_control(root: &Path) -> TaskResult {
    let inventory_path = root.join("conformance/0.1.1/inventory/carddemo-corpus.json");
    let receipt = verify_carddemo_control_flow_from_env(&inventory_path)
        .map_err(|problem| problem.to_string())?;
    let receipt_value = serde_json::to_value(&receipt)
        .map_err(|error| format!("CardDemo control receipt conversion failed: {error}"))?;
    let receipt_bytes = serde_json::to_vec(&receipt_value)
        .map_err(|error| format!("CardDemo control receipt canonicalization failed: {error}"))?;
    let receipt_digest = format!("sha256:{:x}", Sha256::digest(receipt_bytes));
    println!(
        "{}",
        serde_json::to_string_pretty(&receipt)
            .map_err(|error| format!("CardDemo control receipt serialization failed: {error}"))?
    );
    let evidence_path = root.join("conformance/0.1.1/evidence/issues/CD-005.json");
    let evidence = json(&evidence_path)?;
    require(
        evidence["issue"] == Value::String("CD-005".to_string())
            && evidence["derived"] == Value::Bool(true)
            && evidence["status"] == Value::String("pass".to_string()),
        "CD-005 evidence is not a derived pass",
    )?;
    require(
        evidence["control_receipt"] == receipt_value,
        "CD-005 evidence control receipt is stale",
    )?;
    require(
        evidence["evidence_digest"].as_str() == Some(receipt_digest.as_str()),
        "CD-005 evidence digest differs from its canonical control receipt",
    )?;
    Ok(())
}

fn check_carddemo_layout(root: &Path) -> TaskResult {
    let inventory_path = root.join("conformance/0.1.1/inventory/carddemo-corpus.json");
    let receipt = verify_carddemo_data_layouts_from_env(&inventory_path)
        .map_err(|problem| problem.to_string())?;
    let receipt_value = serde_json::to_value(&receipt)
        .map_err(|error| format!("CardDemo layout receipt conversion failed: {error}"))?;
    let receipt_bytes = serde_json::to_vec(&receipt_value)
        .map_err(|error| format!("CardDemo layout receipt canonicalization failed: {error}"))?;
    let receipt_digest = format!("sha256:{:x}", Sha256::digest(receipt_bytes));
    let evidence_path = root.join("conformance/0.1.1/evidence/issues/CD-004.json");
    let evidence = json(&evidence_path)?;
    require(
        evidence["issue"] == Value::String("CD-004".to_string())
            && evidence["derived"] == Value::Bool(true)
            && evidence["status"] == Value::String("pass".to_string()),
        "CD-004 evidence is not a derived pass",
    )?;
    require(
        evidence["layout_receipt"] == receipt_value,
        "CD-004 evidence layout receipt is stale",
    )?;
    require(
        evidence["evidence_digest"].as_str() == Some(receipt_digest.as_str()),
        "CD-004 evidence digest differs from its canonical layout receipt",
    )?;
    println!(
        "{}",
        serde_json::to_string_pretty(&receipt)
            .map_err(|error| format!("CardDemo layout receipt serialization failed: {error}"))?
    );
    Ok(())
}

fn check_carddemo_closure(root: &Path) -> TaskResult {
    let inventory_path = root.join("conformance/0.1.1/inventory/carddemo-corpus.json");
    let receipt = verify_carddemo_source_closures_from_env(&inventory_path)
        .map_err(|problem| problem.to_string())?;
    let receipt_value = serde_json::to_value(&receipt)
        .map_err(|error| format!("CardDemo closure receipt conversion failed: {error}"))?;
    let receipt_bytes = serde_json::to_vec(&receipt_value)
        .map_err(|error| format!("CardDemo closure receipt canonicalization failed: {error}"))?;
    let receipt_digest = format!("sha256:{:x}", Sha256::digest(receipt_bytes));
    let evidence_path = root.join("conformance/0.1.1/evidence/issues/CD-003.json");
    let evidence = json(&evidence_path)?;
    require(
        evidence["issue"] == Value::String("CD-003".to_string())
            && evidence["derived"] == Value::Bool(true)
            && evidence["status"] == Value::String("pass".to_string()),
        "CD-003 evidence is not a derived pass",
    )?;
    require(
        evidence["closure_receipt"] == receipt_value,
        "CD-003 evidence closure receipt is stale",
    )?;
    require(
        evidence["evidence_digest"].as_str() == Some(receipt_digest.as_str()),
        "CD-003 evidence digest differs from its canonical closure receipt",
    )?;
    println!(
        "{}",
        serde_json::to_string_pretty(&receipt)
            .map_err(|error| format!("CardDemo closure receipt serialization failed: {error}"))?
    );
    Ok(())
}

fn check_carddemo_source(root: &Path) -> TaskResult {
    let inventory_path = root.join("conformance/0.1.1/inventory/carddemo-corpus.json");
    let receipt = verify_carddemo_source_preprocessing_from_env(&inventory_path)
        .map_err(|problem| problem.to_string())?;
    let receipt_value = serde_json::to_value(&receipt)
        .map_err(|error| format!("CardDemo source receipt conversion failed: {error}"))?;
    let receipt_bytes = serde_json::to_vec(&receipt_value)
        .map_err(|error| format!("CardDemo source receipt canonicalization failed: {error}"))?;
    let receipt_digest = format!("sha256:{:x}", Sha256::digest(receipt_bytes));
    let evidence_path = root.join("conformance/0.1.1/evidence/issues/CD-002.json");
    let evidence = json(&evidence_path)?;
    require(
        evidence["issue"] == Value::String("CD-002".to_string())
            && evidence["derived"] == Value::Bool(true)
            && evidence["status"] == Value::String("pass".to_string()),
        "CD-002 evidence is not a derived pass",
    )?;
    require(
        evidence["source_receipt"] == receipt_value,
        "CD-002 evidence source receipt is stale",
    )?;
    require(
        evidence["evidence_digest"].as_str() == Some(receipt_digest.as_str()),
        "CD-002 evidence digest differs from its canonical source receipt",
    )?;
    println!(
        "{}",
        serde_json::to_string_pretty(&receipt)
            .map_err(|error| format!("CardDemo source receipt serialization failed: {error}"))?
    );
    Ok(())
}

fn check_carddemo_corpus(root: &Path) -> TaskResult {
    let inventory_path = root.join("conformance/0.1.1/inventory/carddemo-corpus.json");
    let inventory = json(&inventory_path)?;
    let historical = text(&inventory, "content_sha256", &inventory_path)?;
    let fixtures_path = root.join("conformance/0.1/fixtures/manifest.json");
    let fixtures = json(&fixtures_path)?;
    let frozen = array(&fixtures, "fixtures", &fixtures_path)?
        .iter()
        .find(|fixture| fixture["id"].as_str() == Some("aws-carddemo"))
        .and_then(|fixture| fixture["content_sha256"].as_str())
        .ok_or("historical aws-carddemo fixture identity is missing")?;
    require(
        historical == frozen,
        "0.1.1 corpus inventory rewrites the historical 0.1 content identity",
    )?;

    let receipt =
        verify_carddemo_corpus_from_env(&inventory_path).map_err(|problem| problem.to_string())?;
    let receipt_value = serde_json::to_value(&receipt)
        .map_err(|error| format!("CardDemo receipt conversion failed: {error}"))?;
    let receipt_bytes = serde_json::to_vec(&receipt_value)
        .map_err(|error| format!("CardDemo receipt canonicalization failed: {error}"))?;
    let receipt_digest = format!("sha256:{:x}", Sha256::digest(receipt_bytes));
    let evidence_path = root.join("conformance/0.1.1/evidence/issues/CD-001.json");
    let evidence = json(&evidence_path)?;
    require(
        evidence["issue"] == Value::String("CD-001".to_string())
            && evidence["derived"] == Value::Bool(true)
            && evidence["status"] == Value::String("pass".to_string()),
        "CD-001 evidence is not a derived pass",
    )?;
    require(
        evidence["corpus_receipt"] == receipt_value,
        "CD-001 evidence corpus receipt is stale",
    )?;
    require(
        evidence["evidence_digest"].as_str() == Some(receipt_digest.as_str()),
        "CD-001 evidence digest differs from its canonical corpus receipt",
    )?;
    println!(
        "{}",
        serde_json::to_string_pretty(&receipt)
            .map_err(|error| format!("CardDemo receipt serialization failed: {error}"))?
    );
    Ok(())
}

fn repository_root() -> TaskResult<PathBuf> {
    let mut current = env::current_dir().map_err(|error| error.to_string())?;
    loop {
        if current.join("release.toml").is_file() && current.join("Cargo.toml").is_file() {
            return Ok(current);
        }
        if !current.pop() {
            return Err("could not locate repository root".to_string());
        }
    }
}

fn read(path: &Path) -> TaskResult<String> {
    fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))
}

fn json(path: &Path) -> TaskResult<Value> {
    serde_json::from_str(&read(path)?).map_err(|error| format!("{}: {error}", path.display()))
}

fn object<'a>(value: &'a Value, path: &Path) -> TaskResult<&'a serde_json::Map<String, Value>> {
    value
        .as_object()
        .ok_or_else(|| format!("{} must contain a JSON object", path.display()))
}

fn array<'a>(value: &'a Value, key: &str, path: &Path) -> TaskResult<&'a Vec<Value>> {
    value
        .get(key)
        .and_then(Value::as_array)
        .ok_or_else(|| format!("{} must contain array {key:?}", path.display()))
}

fn text<'a>(value: &'a Value, key: &str, path: &Path) -> TaskResult<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{} must contain string {key:?}", path.display()))
}

fn stable_zero_version(value: &str) -> TaskResult<(u64, u64)> {
    let components = value.split('.').collect::<Vec<_>>();
    require(
        components.len() == 3 && components[0] == "0",
        "product version must be stable SemVer 0.x.y",
    )?;
    let parse_component = |component: &str| -> TaskResult<u64> {
        require(
            !component.is_empty()
                && component.bytes().all(|byte| byte.is_ascii_digit())
                && (component == "0" || !component.starts_with('0')),
            "product version must be stable SemVer 0.x.y",
        )?;
        component
            .parse::<u64>()
            .map_err(|_| "product version component is out of range".into())
    };
    Ok((
        parse_component(components[1])?,
        parse_component(components[2])?,
    ))
}

fn release_line_for_version(version: &str) -> TaskResult<String> {
    let (minor, _) = stable_zero_version(version)?;
    Ok(format!("0.{minor}"))
}

fn product_version(root: &Path) -> TaskResult<String> {
    let version = read(&root.join("VERSION"))?.trim().to_string();
    stable_zero_version(&version)?;
    Ok(version)
}

fn release_target_directory(version: &str, target: &str) -> TaskResult<PathBuf> {
    stable_zero_version(version)?;
    validate_release_target(target)?;
    Ok(PathBuf::from(format!("release/{version}/targets/{target}")))
}

fn check_versions(root: &Path) -> TaskResult {
    let version = product_version(root)?;
    let expected_release_line = release_line_for_version(&version)?;
    let cargo: toml::Value = read(&root.join("Cargo.toml"))?
        .parse()
        .map_err(|error| format!("Cargo.toml: {error}"))?;
    let release: toml::Value = read(&root.join("release.toml"))?
        .parse()
        .map_err(|error| format!("release.toml: {error}"))?;

    let cargo_version = cargo["workspace"]["package"]["version"]
        .as_str()
        .ok_or("workspace.package.version is missing")?;
    let release_version = release["product"]["version"]
        .as_str()
        .ok_or("product.version is missing")?;
    let release_line = release["product"]["release_line"]
        .as_str()
        .ok_or("product.release_line is missing")?;
    let channel = release["product"]["channel"]
        .as_str()
        .ok_or("product.channel is missing")?;
    let publish = release["product"]["publish"]
        .as_bool()
        .ok_or("product.publish is missing")?;
    let msrv = cargo["workspace"]["package"]["rust-version"]
        .as_str()
        .ok_or("workspace.package.rust-version is missing")?;
    let pinned = release["rust"]["pinned"]
        .as_str()
        .ok_or("release rust.pinned is missing")?;

    require(
        cargo_version == version,
        "Cargo workspace version differs from VERSION",
    )?;
    require(
        release_version == version,
        "release.toml version differs from VERSION",
    )?;
    require(
        release_line == expected_release_line,
        "release.toml release line is not derived from VERSION",
    )?;
    require(channel == "stable", "product channel must be stable")?;
    require(
        !publish,
        "checked-in release configuration must not publish",
    )?;
    require(msrv == "1.95", "workspace MSRV must be 1.95")?;
    require(pinned == "1.98.0", "pinned Rust toolchain must be 1.98.0")?;

    let mut manifests = Vec::new();
    collect_named(root, OsStr::new("Cargo.toml"), &mut manifests)?;
    let mut workspace_packages = BTreeSet::new();
    for manifest in manifests {
        let parsed: toml::Value = read(&manifest)?
            .parse()
            .map_err(|error| format!("{}: {error}", manifest.display()))?;
        if manifest == root.join("Cargo.toml") {
            continue;
        }
        let package_name = parsed["package"]["name"]
            .as_str()
            .ok_or_else(|| format!("{} package.name is missing", manifest.display()))?;
        workspace_packages.insert(package_name.to_string());
        let crate_version = &parsed["package"]["version"];
        let inherits = crate_version
            .get("workspace")
            .and_then(toml::Value::as_bool)
            == Some(true);
        let exact = crate_version.as_str() == Some(version.as_str());
        require(
            inherits || exact,
            &format!("{} does not use the product version", manifest.display()),
        )?;
    }

    let lock_path = root.join("Cargo.lock");
    let lock: toml::Value = read(&lock_path)?
        .parse()
        .map_err(|error| format!("Cargo.lock: {error}"))?;
    let locked_packages = lock["package"]
        .as_array()
        .ok_or("Cargo.lock package inventory is missing")?;
    for package_name in &workspace_packages {
        let matches = locked_packages
            .iter()
            .filter(|package| package["name"].as_str() == Some(package_name))
            .collect::<Vec<_>>();
        require(
            matches.len() == 1 && matches[0]["version"].as_str() == Some(version.as_str()),
            &format!("Cargo.lock workspace package {package_name} differs from VERSION"),
        )?;
    }

    let inventory_path = root.join("conformance/0.2/inventory/versions.json");
    let inventory = json(&inventory_path)?;
    require(
        text(&inventory, "product", &inventory_path)? == version,
        "machine version inventory differs from VERSION",
    )?;
    require(
        text(&inventory, "release_line", &inventory_path)? == expected_release_line,
        "machine version inventory release line differs from VERSION",
    )?;
    require(
        text(&inventory, "channel", &inventory_path)? == channel,
        "machine version inventory channel differs from release.toml",
    )?;

    let notes = read(&root.join(format!("docs/releases/{release_line}.md")))?;
    require(
        notes.contains(&version),
        "release notes omit current version",
    )?;
    validate_historical_0_2_release(root)?;
    Ok(())
}

fn check_architecture(root: &Path) -> TaskResult {
    check_architecture_fast(root)?;
    check_runtime_unit_gates(root)
}

/// Static dependency/ownership/route checks; no release build or runtime campaign.
fn check_architecture_fast(root: &Path) -> TaskResult {
    let mut manifests = Vec::new();
    collect_named(root, OsStr::new("Cargo.toml"), &mut manifests)?;
    let excluded = excluded_names(root)?;

    for manifest in manifests {
        let source = read(&manifest)?;
        require(
            !source.contains("OpenMainframe") && !source.contains("open-mainframe"),
            &format!("{} imports the compatibility oracle", manifest.display()),
        )?;
        let parsed: toml::Value = source
            .parse()
            .map_err(|error| format!("{}: {error}", manifest.display()))?;
        let package = parsed
            .get("package")
            .and_then(|value| value.get("name"))
            .and_then(toml::Value::as_str)
            .unwrap_or("workspace-root");
        for section in ["dependencies", "dev-dependencies", "build-dependencies"] {
            if let Some(dependencies) = parsed.get(section).and_then(toml::Value::as_table) {
                for dependency in dependencies.keys() {
                    check_dependency(package, dependency, &excluded)?;
                }
            }
        }
    }
    check_declared_dependency_graph(root)?;
    check_common_execution_route(root)?;
    check_dehardcoding(root)?;
    if root
        .join("crates/contracts/mainframe-env-host-api/src/canonical.rs")
        .is_file()
    {
        let guard = root.join("tools/check_effect_encoding.py");
        require(
            guard.is_file(),
            "canonical effect encoding architecture guard is missing",
        )?;
        let status = Command::new("python3")
            .arg("-B")
            .arg(&guard)
            .current_dir(root)
            .status()
            .map_err(|error| format!("effect encoding guard: {error}"))?;
        require(
            status.success(),
            "canonical effect encoding architecture guard failed",
        )?;
    }
    Ok(())
}

fn check_declared_dependency_graph(root: &Path) -> TaskResult {
    let metadata = workspace_metadata(root)?;
    let actual = normal_internal_edges(&metadata)?;
    let path = root.join("conformance/0.1/inventory/dependency-graph.json");
    let graph = json(&path)?;
    let mut declared = array(&graph, "edges", &path)?
        .iter()
        .map(|edge| {
            let values = edge
                .as_array()
                .ok_or_else(|| format!("{} contains a non-array edge", path.display()))?;
            if values.len() != 2 {
                return Err(format!("{} contains a malformed edge", path.display()));
            }
            Ok((
                values[0]
                    .as_str()
                    .ok_or_else(|| format!("{} edge source is not a string", path.display()))?
                    .to_string(),
                values[1]
                    .as_str()
                    .ok_or_else(|| format!("{} edge target is not a string", path.display()))?
                    .to_string(),
            ))
        })
        .collect::<TaskResult<BTreeSet<_>>>()?;
    for additions_path in [
        root.join("conformance/0.1.1/inventory/dependency-graph-additions.json"),
        root.join("conformance/0.2/inventory/dependency-additions.json"),
        root.join("conformance/0.3/inventory/dependency-additions.json"),
        root.join("conformance/0.5/inventory/dependency-additions.json"),
        root.join("conformance/0.6/inventory/dependency-additions.json"),
        root.join("conformance/0.7/inventory/dependency-additions.json"),
        root.join("conformance/0.8/inventory/dependency-additions.json"),
    ] {
        if !additions_path.is_file() {
            continue;
        }
        let additions = json(&additions_path)?;
        for edge in array(&additions, "edges", &additions_path)? {
            let values = edge
                .as_array()
                .ok_or_else(|| format!("{} contains a non-array edge", additions_path.display()))?;
            if values.len() != 2 {
                return Err(format!(
                    "{} contains a malformed edge",
                    additions_path.display()
                ));
            }
            declared.insert((
                values[0]
                    .as_str()
                    .ok_or_else(|| {
                        format!("{} edge source is not a string", additions_path.display())
                    })?
                    .to_string(),
                values[1]
                    .as_str()
                    .ok_or_else(|| {
                        format!("{} edge target is not a string", additions_path.display())
                    })?
                    .to_string(),
            ));
        }
    }
    if actual != declared {
        let missing = actual.difference(&declared).cloned().collect::<Vec<_>>();
        let stale = declared.difference(&actual).cloned().collect::<Vec<_>>();
        return Err(format!(
            "declared dependency graph differs from Cargo metadata; missing={missing:?} stale={stale:?}"
        ));
    }
    Ok(())
}

fn check_common_execution_route(root: &Path) -> TaskResult {
    for directory in ["crates/apps", "crates/gateways"] {
        let mut files = Vec::new();
        collect_extension(&root.join(directory), OsStr::new("rs"), &mut files)?;
        for file in files {
            let source = read(&file)?;
            require(
                !source.contains(".drive("),
                &format!(
                    "{} drives a machine outside ExecutionCoordinator",
                    file.display()
                ),
            )?;
        }
    }
    Ok(())
}

fn check_runtime_unit_gates(root: &Path) -> TaskResult {
    for (package, test) in [
        (
            "mainframe-env-store",
            "sqlite_adapter_runs_inside_multithread_and_current_thread_async_shells",
        ),
        (
            "mainframe-env-server",
            "cobol_cics_job_uses_scoped_principal_and_typed_host_route",
        ),
    ] {
        let status = Command::new("cargo")
            .args([
                "test",
                "-p",
                package,
                test,
                "--all-features",
                "--locked",
                "--quiet",
            ])
            .current_dir(root)
            .status()
            .map_err(|error| format!("runtime architecture test {test}: {error}"))?;
        require(
            status.success(),
            &format!("runtime architecture test {test} failed"),
        )?;
    }
    Ok(())
}

fn check_dependency(package: &str, dependency: &str, excluded: &BTreeSet<String>) -> TaskResult {
    require(
        !excluded.contains(dependency),
        &format!("{package} depends on excluded component {dependency}"),
    )?;
    let core = package.starts_with("mainframe-env-source")
        || package.starts_with("mainframe-env-diagnostics")
        || package.starts_with("mainframe-env-encoding")
        || package.starts_with("mainframe-env-ir")
        || package == "mainframe-env-coverage"
        || package.ends_with("-api");
    if core {
        require(
            !matches!(dependency, "tokio" | "axum" | "tower" | "sqlx" | "tracing"),
            &format!("deterministic package {package} depends on infrastructure {dependency}"),
        )?;
    }
    if dependency.starts_with("mainframe-env-") && !allowed_internal_dependency(package, dependency)
    {
        return Err(format!(
            "dependency direction forbids {package} -> {dependency}"
        ));
    }
    if matches!(package, "mainframe-env-conformance" | "xtask") {
        return Ok(());
    }
    require(
        dependency != "mainframe-env-conformance",
        &format!("production package {package} depends on conformance"),
    )
}

fn allowed_internal_dependency(package: &str, dependency: &str) -> bool {
    let allowed: &[&str] = match package {
        "mainframe-env-source" | "mainframe-env-encoding" | "mainframe-env-coverage" => &[],
        "mainframe-env-diagnostics" => &["mainframe-env-source"],
        "mainframe-env-ir" => &["mainframe-env-source", "mainframe-env-diagnostics"],
        "mainframe-env-compiler-api" => &[
            "mainframe-env-source",
            "mainframe-env-diagnostics",
            "mainframe-env-ir",
        ],
        "mainframe-env-execution-api" => &["mainframe-env-diagnostics"],
        "mainframe-env-host-api" => &["mainframe-env-execution-api"],
        "mainframe-env-store-api" => &["mainframe-env-execution-api"],
        "mainframe-env-store" => &["mainframe-env-execution-api", "mainframe-env-store-api"],
        "mainframe-env-compiler" => &[
            "mainframe-env-source",
            "mainframe-env-diagnostics",
            "mainframe-env-encoding",
            "mainframe-env-ir",
            "mainframe-env-compiler-api",
        ],
        "mainframe-env-interpreter" => &[
            "mainframe-env-diagnostics",
            "mainframe-env-encoding",
            "mainframe-env-ir",
            "mainframe-env-execution-api",
            "mainframe-env-host-api",
            "mainframe-env-store-api",
        ],
        _ => return true,
    };
    allowed.contains(&dependency)
}

fn workspace_metadata(root: &Path) -> TaskResult<Value> {
    let output = Command::new("cargo")
        .args(["metadata", "--no-deps", "--format-version", "1", "--locked"])
        .current_dir(root)
        .output()
        .map_err(|error| format!("cargo metadata: {error}"))?;
    require(output.status.success(), "cargo metadata failed")?;
    serde_json::from_slice(&output.stdout).map_err(|error| format!("cargo metadata JSON: {error}"))
}

fn normal_internal_edges(metadata: &Value) -> TaskResult<BTreeSet<(String, String)>> {
    let packages = metadata["packages"]
        .as_array()
        .ok_or("metadata packages missing")?;
    let names = packages
        .iter()
        .filter_map(|package| package["name"].as_str().map(str::to_string))
        .collect::<BTreeSet<_>>();
    let mut edges = BTreeSet::new();
    for package in packages {
        let source = package["name"].as_str().ok_or("package name missing")?;
        for dependency in package["dependencies"]
            .as_array()
            .ok_or("package dependencies missing")?
        {
            if !dependency["kind"].is_null() {
                continue;
            }
            let target = dependency["name"]
                .as_str()
                .ok_or("dependency name missing")?;
            if names.contains(target) {
                edges.insert((source.to_string(), target.to_string()));
            }
        }
    }
    Ok(edges)
}

fn check_profiles(root: &Path) -> TaskResult {
    let inventory_path = root.join("conformance/0.1/inventory/packages.json");
    let profiles_path = root.join("conformance/0.1/inventory/profiles.json");
    let inventory = json(&inventory_path)?;
    let profiles = json(&profiles_path)?;
    let mut known = array(&inventory, "packages", &inventory_path)?
        .iter()
        .filter_map(|row| row.get("name").and_then(Value::as_str).map(str::to_string))
        .collect::<BTreeSet<_>>();
    for additions_path in [
        root.join("conformance/0.2/inventory/package-additions.json"),
        root.join("conformance/0.8/inventory/package-additions.json"),
    ] {
        if additions_path.is_file() {
            let additions = json(&additions_path)?;
            for package in array(&additions, "packages", &additions_path)? {
                require(
                    known.insert(text(package, "name", &additions_path)?.to_string()),
                    "package addition duplicates a historical profile package",
                )?;
            }
        }
    }
    let excluded = excluded_names(root)?;
    let mut profile_additions = BTreeMap::<String, BTreeSet<String>>::new();
    for additions_path in [
        root.join("conformance/0.3/inventory/dependency-additions.json"),
        root.join("conformance/0.8/inventory/dependency-additions.json"),
    ] {
        let additions = json(&additions_path)?;
        let additions = additions
            .get("profile_additions")
            .and_then(Value::as_array)
            .ok_or_else(|| format!("{} omits profile_additions", additions_path.display()))?;
        for addition in additions {
            let values = addition.as_array().ok_or_else(|| {
                format!(
                    "{} contains a malformed profile addition",
                    additions_path.display()
                )
            })?;
            let profile = values.first().and_then(Value::as_str).ok_or_else(|| {
                format!(
                    "{} profile addition has no profile",
                    additions_path.display()
                )
            })?;
            let package = values.get(1).and_then(Value::as_str).ok_or_else(|| {
                format!(
                    "{} profile addition has no package",
                    additions_path.display()
                )
            })?;
            require(
                values.len() == 2
                    && matches!(profile, "core-server" | "conformance")
                    && known.contains(package),
                "profile addition is outside the reviewed package boundary",
            )?;
            require(
                profile_additions
                    .entry(profile.into())
                    .or_default()
                    .insert(package.into()),
                "profile addition is duplicated",
            )?;
        }
    }

    for profile in array(&profiles, "profiles", &profiles_path)? {
        let id = text(profile, "id", &profiles_path)?;
        let packages = array(profile, "packages", &profiles_path)?;
        let mut unique = BTreeSet::new();
        for package in packages {
            let name = package
                .as_str()
                .ok_or_else(|| format!("profile {id} has a non-string package"))?;
            require(
                known.contains(name),
                &format!("profile {id} names unknown package {name}"),
            )?;
            require(
                !excluded.contains(name),
                &format!("profile {id} includes excluded {name}"),
            )?;
            require(
                unique.insert(name),
                &format!("profile {id} repeats package {name}"),
            )?;
        }
    }
    let metadata = workspace_metadata(root)?;
    let edges = normal_internal_edges(&metadata)?;
    let adjacency = edges.into_iter().fold(
        BTreeMap::<String, BTreeSet<String>>::new(),
        |mut map, edge| {
            map.entry(edge.0).or_default().insert(edge.1);
            map
        },
    );
    for (profile_id, roots) in [
        ("core-server", vec!["mainframe-env-server"]),
        (
            "conformance",
            vec![
                "mainframe-env-server",
                "mainframe-env-cli",
                "mainframe-env-conformance",
            ],
        ),
    ] {
        let mut declared = array(&profiles, "profiles", &profiles_path)?
            .iter()
            .find(|profile| profile["id"].as_str() == Some(profile_id))
            .ok_or_else(|| format!("profile {profile_id} is missing"))?["packages"]
            .as_array()
            .ok_or_else(|| format!("profile {profile_id} packages are missing"))?
            .iter()
            .map(|package| {
                package
                    .as_str()
                    .map(str::to_string)
                    .ok_or_else(|| format!("profile {profile_id} has a non-string package"))
            })
            .collect::<TaskResult<BTreeSet<_>>>()?;
        declared.extend(
            profile_additions
                .get(profile_id)
                .into_iter()
                .flatten()
                .cloned(),
        );
        let mut closure = BTreeSet::new();
        let mut pending = roots.into_iter().map(str::to_string).collect::<Vec<_>>();
        while let Some(package) = pending.pop() {
            if !closure.insert(package.clone()) {
                continue;
            }
            pending.extend(adjacency.get(&package).into_iter().flatten().cloned());
        }
        if declared != closure {
            return Err(format!(
                "profile {profile_id} differs from its Cargo closure; missing={:?} extra={:?}",
                closure.difference(&declared).collect::<Vec<_>>(),
                declared.difference(&closure).collect::<Vec<_>>()
            ));
        }
    }
    Ok(())
}

fn check_schemas(root: &Path) -> TaskResult {
    let mut files = Vec::new();
    collect_extension(
        &root.join("conformance/0.1/schemas"),
        OsStr::new("json"),
        &mut files,
    )?;
    collect_extension(
        &root.join("conformance/0.2/schemas"),
        OsStr::new("json"),
        &mut files,
    )?;
    collect_extension(
        &root.join("conformance/0.5/schemas"),
        OsStr::new("json"),
        &mut files,
    )?;
    collect_extension(
        &root.join("conformance/0.6/schemas"),
        OsStr::new("json"),
        &mut files,
    )?;
    let jcl_schemas = root.join("conformance/0.7/schemas");
    if jcl_schemas.is_dir() {
        collect_extension(&jcl_schemas, OsStr::new("json"), &mut files)?;
    }
    let jes_schemas = root.join("conformance/0.8/schemas");
    if jes_schemas.is_dir() {
        collect_extension(&jes_schemas, OsStr::new("json"), &mut files)?;
    }
    require(!files.is_empty(), "no evidence schemas found")?;
    files.sort();
    for file in &files {
        let value = json(file)?;
        let root_object = object(&value, file)?;
        require(
            root_object.contains_key("$schema"),
            &format!("{} lacks $schema", file.display()),
        )?;
        require(
            root_object.contains_key("title"),
            &format!("{} lacks title", file.display()),
        )?;
        require(
            root_object.get("type") == Some(&Value::String("object".to_string())),
            &format!("{} must describe an object", file.display()),
        )?;
        compile_draft_2020_12_schema(&value, file)?;
    }
    validate_0_2_schema_artifacts(root)?;
    let inventory_path = root.join("conformance/0.6/inventory/dataset-programming-surface.json");
    let schema_path = root.join("conformance/0.6/schemas/dataset-programming-surface.schema.json");
    validate_schema_instance(
        &json(&schema_path)?,
        &json(&inventory_path)?,
        &inventory_path,
    )?;
    let dependency_path = root.join("conformance/0.6/inventory/dependency-additions.json");
    let dependency_schema =
        root.join("conformance/0.6/schemas/dataset-dependency-additions.schema.json");
    validate_schema_instance(
        &json(&dependency_schema)?,
        &json(&dependency_path)?,
        &dependency_path,
    )?;
    let migration_path = root.join("conformance/0.6/migrations/dataset-state-v2-to-v3.json");
    let migration_schema = root.join("conformance/0.6/schemas/dataset-state-migration.schema.json");
    validate_schema_instance(
        &json(&migration_schema)?,
        &json(&migration_path)?,
        &migration_path,
    )?;
    let migration_v4_path = root.join("conformance/0.6/migrations/dataset-state-v3-to-v4.json");
    let migration_v4_schema =
        root.join("conformance/0.6/schemas/dataset-state-v4-migration.schema.json");
    validate_schema_instance(
        &json(&migration_v4_schema)?,
        &json(&migration_v4_path)?,
        &migration_v4_path,
    )?;
    let migration_v5_path = root.join("conformance/0.6/migrations/dataset-state-v4-to-v5.json");
    let migration_v5_schema =
        root.join("conformance/0.6/schemas/dataset-state-v5-migration.schema.json");
    validate_schema_instance(
        &json(&migration_v5_schema)?,
        &json(&migration_v5_path)?,
        &migration_v5_path,
    )?;
    let migration_v6_path = root.join("conformance/0.6/migrations/dataset-state-v5-to-v6.json");
    let migration_v6_schema =
        root.join("conformance/0.6/schemas/dataset-state-v6-migration.schema.json");
    validate_schema_instance(
        &json(&migration_v6_schema)?,
        &json(&migration_v6_path)?,
        &migration_v6_path,
    )?;
    let organization_fixture = root.join("conformance/0.6/fixtures/dataset-organizations.json");
    let organization_schema =
        root.join("conformance/0.6/schemas/dataset-organization-fixtures.schema.json");
    validate_schema_instance(
        &json(&organization_schema)?,
        &json(&organization_fixture)?,
        &organization_fixture,
    )?;
    let ams_fixture = root.join("conformance/0.6/fixtures/ams-commands.json");
    let ams_schema = root.join("conformance/0.6/schemas/ams-command-fixtures.schema.json");
    validate_schema_instance(&json(&ams_schema)?, &json(&ams_fixture)?, &ams_fixture)?;
    let jes_migration = root.join("conformance/0.8/migrations/jes-durable-job-v1-to-v2.json");
    let jes_migration_schema = root.join("conformance/0.8/schemas/jes-state-migration.schema.json");
    validate_schema_instance(
        &json(&jes_migration_schema)?,
        &json(&jes_migration)?,
        &jes_migration,
    )?;
    let jes_differential = root.join("conformance/0.8/oracles/jes-licensed-differential.json");
    let jes_differential_schema =
        root.join("conformance/0.8/schemas/jes-licensed-differential-adapter.schema.json");
    validate_schema_instance(
        &json(&jes_differential_schema)?,
        &json(&jes_differential)?,
        &jes_differential,
    )?;
    let jes_dependency_additions = root.join("conformance/0.8/inventory/dependency-additions.json");
    let jes_dependency_additions_schema =
        root.join("conformance/0.8/schemas/jes-dependency-additions.schema.json");
    validate_schema_instance(
        &json(&jes_dependency_additions_schema)?,
        &json(&jes_dependency_additions)?,
        &jes_dependency_additions,
    )?;
    let jes_package_additions = root.join("conformance/0.8/inventory/package-additions.json");
    let jes_package_additions_schema =
        root.join("conformance/0.8/schemas/jes-package-additions.schema.json");
    validate_schema_instance(
        &json(&jes_package_additions_schema)?,
        &json(&jes_package_additions)?,
        &jes_package_additions,
    )?;
    for (artifact, schema) in [
        (
            "conformance/0.8/evidence/carddemo-base-batch.json",
            "conformance/0.8/schemas/carddemo-base-batch-evidence.schema.json",
        ),
        (
            "conformance/0.8/inventory/jes-dd-surface.json",
            "conformance/0.8/schemas/jes-dd-surface.schema.json",
        ),
        (
            "conformance/0.8/inventory/jes-operations-surface.json",
            "conformance/0.8/schemas/jes-operations-surface.schema.json",
        ),
        (
            "conformance/0.8/inventory/jes-recovery-surface.json",
            "conformance/0.8/schemas/jes-recovery-surface.schema.json",
        ),
        (
            "conformance/0.8/inventory/jes-spool-output-surface.json",
            "conformance/0.8/schemas/jes-spool-output-surface.schema.json",
        ),
        (
            "conformance/0.8/inventory/jes-utility-surface.json",
            "conformance/0.8/schemas/jes-utility-surface.schema.json",
        ),
        (
            "conformance/0.8/migrations/jes-embedded-spool-to-artifacts.json",
            "conformance/0.8/schemas/jes-spool-migration.schema.json",
        ),
    ] {
        let artifact = root.join(artifact);
        let schema = root.join(schema);
        validate_schema_instance(&json(&schema)?, &json(&artifact)?, &artifact)?;
    }
    let recovery_path = root.join("conformance/0.8/inventory/jes-recovery-surface.json");
    let recovery = json(&recovery_path)?;
    let state_migration_digests = recovery["state_migration_digests"]
        .as_object()
        .ok_or_else(|| format!("{} omits state_migration_digests", recovery_path.display()))?;
    for (relative, expected) in state_migration_digests {
        let actual = format!("sha256:{}", file_digest(&root.join(relative))?);
        require(
            expected.as_str() == Some(actual.as_str()),
            &format!("JES state/migration digest drifted for {relative}"),
        )?;
    }
    let jes_evidence_schema =
        json(&root.join("conformance/0.8/schemas/jes-work-package-evidence.schema.json"))?;
    for work_package in ["JES-802", "JES-803", "JES-804", "JES-805", "JES-806"] {
        let path = root.join(format!(
            "conformance/0.8/evidence/{}-matrix.json",
            work_package.to_ascii_lowercase()
        ));
        let evidence = json(&path)?;
        validate_schema_instance(&jes_evidence_schema, &evidence, &path)?;
        let rows = array(&evidence, "rows", &path)?;
        unique_rows(rows, "id", &path)?;
        let pending = rows
            .iter()
            .filter(|row| row["result"] == Value::String("pending".into()))
            .count();
        let status = text(&evidence, "status", &path)?;
        require(
            (pending == 0 && status == "pass")
                || (pending == 1 && status == "pass-with-licensed-differential-pending"),
            &format!("{} status does not match its row results", path.display()),
        )?;
        if work_package == "JES-806" && pending == 1 {
            require(
                rows.iter().any(|row| {
                    row["id"] == Value::String("licensed-zos-3.2-jes2".into())
                        && row["result"] == Value::String("pending".into())
                }),
                "JES-806 may defer only its licensed z/OS 3.2/JES2 row",
            )?;
        }
    }
    let jes_adapter = json(&jes_differential)?;
    require(
        jes_adapter["licensed_receipt_required_for_pass"] == Value::Bool(true)
            && jes_adapter["generated_historical_or_local_result_counts_as_pass"]
                == Value::Bool(false),
        "JES pending completion policy weakened the fail-closed licensed adapter",
    )?;
    for relative in [
        "docs/prompts/coverage-versions/IMPLEMENT_0_8_0.md",
        "docs/delivery/coverage-versions/0.8.0.md",
        "docs/delivery/coverage-versions/status/0.8.0.md",
    ] {
        let document = read(&root.join(relative))?;
        require(
            document.contains("pass-with-licensed-differential-pending")
                && document.contains("0/16")
                && document.contains("Hercules"),
            &format!("{relative} omits the approved 0.8 pending-differential disposition"),
        )?;
    }
    for relative in [
        "docs/prompts/coverage-versions/IMPLEMENT_0_17_0.md",
        "docs/delivery/coverage-versions/0.17.0.md",
    ] {
        let document = read(&root.join(relative))?;
        require(
            document.contains("0/16")
                && document.contains("16-scenario")
                && document.contains("JES2"),
            &format!("{relative} omits the deferred licensed JES2 campaign handoff"),
        )?;
    }
    let certification = root.join("conformance/0.6/evidence/dataset-certification.json");
    let certification_schema =
        root.join("conformance/0.6/schemas/dataset-certification.schema.json");
    validate_schema_instance(
        &json(&certification_schema)?,
        &json(&certification)?,
        &certification,
    )?;
    let surface_audit = root.join("conformance/0.6/evidence/dataset-surface-audit.json");
    let surface_audit_schema =
        root.join("conformance/0.6/schemas/dataset-surface-audit.schema.json");
    validate_schema_instance(
        &json(&surface_audit_schema)?,
        &json(&surface_audit)?,
        &surface_audit,
    )
}

fn compile_draft_2020_12_schema(schema: &Value, path: &Path) -> TaskResult<jsonschema::Validator> {
    jsonschema::draft202012::meta::validate(schema)
        .map_err(|error| format!("{} is not valid Draft 2020-12: {error}", path.display()))?;
    jsonschema::draft202012::options()
        .offline()
        .build(schema)
        .map_err(|error| format!("{} did not compile: {error}", path.display()))
}

fn check_dataset_oracle(root: &Path) -> TaskResult {
    check_dataset_oracle_receipt(
        root,
        env::var_os("MAINFRAME_ENV_ZOS_AMS_ORACLE_RECEIPT").map(PathBuf::from),
    )
}

fn check_dataset_oracle_receipt(root: &Path, receipt_path: Option<PathBuf>) -> TaskResult {
    let receipt_path = receipt_path.ok_or(
            "licensed z/OS 3.2 differential is pending; set MAINFRAME_ENV_ZOS_AMS_ORACLE_RECEIPT to a reviewed receipt",
        )?;
    require(
        receipt_path.is_file(),
        "MAINFRAME_ENV_ZOS_AMS_ORACLE_RECEIPT is not a readable file",
    )?;
    let schema_path = root.join("conformance/0.6/schemas/dataset-oracle-receipt.schema.json");
    let receipt = json(&receipt_path)?;
    validate_schema_instance(&json(&schema_path)?, &receipt, &receipt_path)?;
    let candidate = repository_digest(root)?;
    require(
        receipt["candidate_digest"].as_str() == Some(candidate.as_str()),
        "licensed dataset oracle receipt was produced from a different candidate",
    )?;
    println!(
        "dataset-oracle baseline=ibm-zos-3.2-dfsms-ams-2026-06 cases=36 status=pass candidate={candidate}"
    );
    Ok(())
}

fn check_jes_oracle(root: &Path) -> TaskResult {
    check_jes_oracle_receipt(
        root,
        env::var_os("MAINFRAME_ENV_JES_LICENSED_ORACLE_RECEIPT").map(PathBuf::from),
    )
}

fn check_jes_oracle_receipt(root: &Path, receipt_path: Option<PathBuf>) -> TaskResult {
    let receipt_path = receipt_path.ok_or(
            "licensed z/OS 3.2 JES2 differential is pending; set MAINFRAME_ENV_JES_LICENSED_ORACLE_RECEIPT to a reviewed receipt",
        )?;
    require(
        receipt_path.is_file(),
        "MAINFRAME_ENV_JES_LICENSED_ORACLE_RECEIPT is not a readable file",
    )?;
    let receipt_path = fs::canonicalize(&receipt_path)
        .map_err(|error| format!("licensed JES2 receipt: {error}"))?;
    let canonical_root =
        fs::canonicalize(root).map_err(|error| format!("repository root: {error}"))?;
    require(
        !receipt_path.starts_with(canonical_root),
        "licensed JES2 receipt must remain external to the candidate tree",
    )?;
    let schema_path =
        root.join("conformance/0.8/schemas/jes-licensed-differential-receipt.schema.json");
    let receipt = json(&receipt_path)?;
    validate_schema_instance(&json(&schema_path)?, &receipt, &receipt_path)?;
    let adapter = validate_jes_oracle_adapter(root)?;
    let candidate = jes_oracle_candidate_digest(root)?;
    require(
        receipt["candidate_digest"].as_str() == Some(candidate.as_str()),
        "licensed JES2 oracle receipt was produced from a different candidate",
    )?;
    let adapter_path = root.join("conformance/0.8/oracles/jes-licensed-differential.json");
    let required = array(&adapter, "required_scenarios", &adapter_path)?
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(str::to_string)
                .ok_or_else(|| "JES2 adapter contains a non-string scenario".to_string())
        })
        .collect::<TaskResult<BTreeSet<_>>>()?;
    let observed = array(&receipt, "scenarios", &receipt_path)?
        .iter()
        .map(|scenario| {
            require(
                scenario["status"] == Value::String("pass".into()),
                "licensed JES2 receipt contains a non-passing scenario",
            )?;
            Ok(text(scenario, "id", &receipt_path)?.to_string())
        })
        .collect::<TaskResult<BTreeSet<_>>>()?;
    require(
        observed == required,
        "licensed JES2 receipt scenario set differs from the required campaign",
    )?;
    println!(
        "jes-oracle baseline=ibm-zos-3.2-jcl-jes2-2026-06 cases=16 status=pass candidate={candidate}"
    );
    Ok(())
}

fn validate_jes_oracle_adapter(root: &Path) -> TaskResult<Value> {
    let adapter_path = root.join("conformance/0.8/oracles/jes-licensed-differential.json");
    let schema_path =
        root.join("conformance/0.8/schemas/jes-licensed-differential-adapter.schema.json");
    let adapter = json(&adapter_path)?;
    validate_schema_instance(&json(&schema_path)?, &adapter, &adapter_path)?;
    Ok(adapter)
}

fn print_jes_oracle_candidate(root: &Path) -> TaskResult {
    validate_jes_oracle_adapter(root)?;
    println!("{}", jes_oracle_candidate_digest(root)?);
    Ok(())
}

#[derive(Debug, Eq, PartialEq)]
struct GitIndexFile {
    mode: String,
    path: String,
}

fn jes_oracle_candidate_digest(root: &Path) -> TaskResult<String> {
    let unstaged = git_null_paths(root, &["diff", "--name-only", "-z", "--"])?
        .into_iter()
        .filter(|path| !jes_oracle_candidate_excluded(path))
        .collect::<Vec<_>>();
    require(
        unstaged.is_empty(),
        &format!(
            "licensed JES2 candidate has unstaged tracked paths: {unstaged:?}; stage the exact candidate before producing or checking a receipt"
        ),
    )?;

    let mut files = git_index_files(root)?;
    files.retain(|file| !jes_oracle_candidate_excluded(&file.path));
    let mut digest = Sha256::new();
    digest.update(JES_ORACLE_CANDIDATE_DOMAIN);
    digest.update(
        u64::try_from(files.len())
            .map_err(|_| "too many Git index files for JES2 candidate".to_string())?
            .to_be_bytes(),
    );
    for file in files {
        let bytes = git_index_file_bytes(root, &file.path)?;
        digest_length_prefixed(&mut digest, file.mode.as_bytes())?;
        digest_length_prefixed(&mut digest, file.path.as_bytes())?;
        digest_length_prefixed(&mut digest, &bytes)?;
    }
    Ok(format!("sha256:{:x}", digest.finalize()))
}

fn git_index_files(root: &Path) -> TaskResult<Vec<GitIndexFile>> {
    let output = Command::new("git")
        .args(["ls-files", "--cached", "--stage", "-z"])
        .current_dir(root)
        .output()
        .map_err(|error| format!("git ls-files: {error}"))?;
    require(output.status.success(), "git ls-files failed")?;
    let listing = String::from_utf8(output.stdout)
        .map_err(|_| "Git index contains a non-UTF-8 path".to_string())?;
    let mut files = Vec::new();
    for row in listing.split_terminator('\0') {
        let (metadata, path) = row
            .split_once('\t')
            .ok_or_else(|| format!("invalid Git index row {row:?}"))?;
        let fields = metadata.split_whitespace().collect::<Vec<_>>();
        require(
            fields.len() == 3 && fields[2] == "0",
            &format!("unmerged or invalid Git index entry for {path:?}"),
        )?;
        require(
            matches!(fields[0], "100644" | "100755"),
            &format!("unsupported Git index mode {} for {path:?}", fields[0]),
        )?;
        let candidate = Path::new(path);
        require(
            !candidate.is_absolute()
                && !candidate
                    .components()
                    .any(|component| matches!(component, std::path::Component::ParentDir)),
            &format!("unsafe Git index path {path:?}"),
        )?;
        files.push(GitIndexFile {
            mode: fields[0].to_string(),
            path: path.to_string(),
        });
    }
    files.sort_by(|left, right| left.path.cmp(&right.path));
    require(
        files.windows(2).all(|pair| pair[0].path != pair[1].path),
        "Git index contains duplicate paths",
    )?;
    Ok(files)
}

fn git_null_paths(root: &Path, arguments: &[&str]) -> TaskResult<Vec<String>> {
    let output = Command::new("git")
        .args(arguments)
        .current_dir(root)
        .output()
        .map_err(|error| format!("git: {error}"))?;
    require(output.status.success(), "git failed")?;
    let listing = String::from_utf8(output.stdout)
        .map_err(|_| "Git reported a non-UTF-8 path".to_string())?;
    Ok(listing.split_terminator('\0').map(str::to_string).collect())
}

fn git_index_file_bytes(root: &Path, relative: &str) -> TaskResult<Vec<u8>> {
    let object = format!(":{relative}");
    let output = Command::new("git")
        .args(["show", &object])
        .current_dir(root)
        .output()
        .map_err(|error| format!("git show {object}: {error}"))?;
    require(
        output.status.success(),
        &format!("Git index object is missing: {relative}"),
    )?;
    Ok(output.stdout)
}

fn digest_length_prefixed(digest: &mut Sha256, value: &[u8]) -> TaskResult {
    digest.update(
        u64::try_from(value.len())
            .map_err(|_| "JES2 candidate field is too large".to_string())?
            .to_be_bytes(),
    );
    digest.update(value);
    Ok(())
}

fn jes_oracle_candidate_excluded(path: &str) -> bool {
    JES_ORACLE_DERIVED_PATHS.contains(&path)
}

fn validate_schema_instance(schema: &Value, instance: &Value, path: &Path) -> TaskResult {
    let validator = compile_draft_2020_12_schema(schema, path)?;
    validator
        .validate(instance)
        .map_err(|error| format!("{} violates its schema: {error}", path.display()))
}

fn validate_0_2_schema_artifacts(root: &Path) -> TaskResult {
    let schemas = root.join("conformance/0.2/schemas");
    let mut artifacts = Vec::new();
    collect_extension(
        &root.join("conformance/0.2"),
        OsStr::new("json"),
        &mut artifacts,
    )?;
    artifacts.retain(|path| !path.starts_with(&schemas));
    artifacts.sort();
    for path in artifacts {
        let instance = json(&path)?;
        let version = text(&instance, "schema_version", &path)?;
        let schema_name = schema_for_0_2_artifact(version).ok_or_else(|| {
            format!(
                "{} has no Draft 2020-12 schema binding for {version}",
                path.display()
            )
        })?;
        let schema_path = schemas.join(schema_name);
        validate_schema_instance(&json(&schema_path)?, &instance, &path)?;
    }

    let release = root.join("release/0.2.0/targets");
    if release.is_dir() {
        for entry in
            fs::read_dir(&release).map_err(|error| format!("{}: {error}", release.display()))?
        {
            let target = entry.map_err(|error| error.to_string())?.path();
            if !target.is_dir() {
                return Err(format!(
                    "release target entry is not a directory: {}",
                    target.display()
                ));
            }
            for (artifact, schema) in [
                ("manifest.json", "release-manifest.schema.json"),
                ("sbom.cdx.json", "release-sbom.schema.json"),
                ("provenance.intoto.json", "release-provenance.schema.json"),
                ("build-inputs.json", "release-build-inputs.schema.json"),
            ] {
                let path = target.join(artifact);
                let schema_path = schemas.join(schema);
                validate_schema_instance(&json(&schema_path)?, &json(&path)?, &path)?;
            }
        }
    }
    Ok(())
}

fn schema_for_0_2_artifact(version: &str) -> Option<&'static str> {
    match version {
        "mainframe-env.official-source-receipts@1" => Some("official-source-receipts.schema.json"),
        "mainframe-env.official-catalog@1" => Some("official-catalog.schema.json"),
        "mainframe-env.topic-manifest@1" => Some("topic-manifest.schema.json"),
        "mainframe-env.coverage-ledger@1" => Some("coverage-ledger.schema.json"),
        "mainframe-env.coverage-program-status@1" => Some("program-status.schema.json"),
        "mainframe-env.coverage-work-package-evidence@1" => {
            Some("work-package-evidence.schema.json")
        }
        "mainframe-env.coverage-workload-ledger@1" => Some("workload-ledger.schema.json"),
        "mainframe-env.migration-rollback-rollup@1" => {
            Some("migration-rollback-rollup.schema.json")
        }
        "mainframe-env.full-regression@1" => Some("full-regression.schema.json"),
        "mainframe-env.review-repair@1" => Some("review-repair.schema.json"),
        "mainframe-env.review-repair-round-2@1" => Some("review-repair-round-2.schema.json"),
        "mainframe-env.review-repair-round-3@1" => Some("review-repair-round-3.schema.json"),
        "mainframe-env.review-repair-round-4@1" => Some("review-repair-round-4.schema.json"),
        "mainframe-env.review-repair-round-4-inputs@1" => {
            Some("review-repair-round-4-inputs.schema.json")
        }
        "mainframe-env.seal-profile@1" => Some("review-repair-round-5-profile.schema.json"),
        "mainframe-env.review-repair-round-5@1" => Some("review-repair-round-5.schema.json"),
        "mainframe-env.github-actions-receipt@1" => Some("github-actions-receipt.schema.json"),
        "mainframe-env.work-package-amendments@1" => Some("work-package-amendments.schema.json"),
        "mainframe-env.common-program-catalog@1" => Some("common-program-catalog.schema.json"),
        "mainframe-env.zosmf-official-route-bindings@1"
        | "mainframe-env.custom-route-catalog@1" => Some("route-registry.schema.json"),
        "mainframe-env.generated-route-registries@1" => {
            Some("generated-route-registries.schema.json")
        }
        "mainframe-env.generated-semantic-identities@1" => {
            Some("generated-semantic-identities.schema.json")
        }
        "mainframe-env.subsystem-handler-registry@1" => {
            Some("subsystem-handler-registry.schema.json")
        }
        "mainframe-env.host-abi-inventory@1" => Some("host-abi-inventory.schema.json"),
        "mainframe-env.coverage-contract-inventory@1"
        | "mainframe-env.coverage-dependency-additions@1"
        | "mainframe-env.coverage-package-additions@1"
        | "mainframe-env.version-inventory@1" => Some("coverage-inventory.schema.json"),
        "mainframe-env.application-package-migration@1"
        | "mainframe-env.batch-controller-migration@1"
        | "mainframe-env.db2-catalog-migration@1"
        | "mainframe-env.host-abi-migration@1"
        | "mainframe-env.registry-migration@1" => Some("migration.schema.json"),
        "mainframe-env.application-hardcode-scan@1"
        | "mainframe-env.host-abi-ownership-scan@1"
        | "mainframe-env.no-application-hardcode@1"
        | "mainframe-env.no-string-dispatch@1" => Some("hardcode-evidence.schema.json"),
        _ => None,
    }
}

fn check_inventory(root: &Path) -> TaskResult {
    let inventory = root.join("conformance/0.1/inventory");
    let required = [
        "profiles.json",
        "packages.json",
        "selectors.json",
        "cobol-constructs.json",
        "cobol-cohorts.json",
        "cics-operations.json",
        "jcl-jes-coverage.json",
        "dataset-coverage.json",
        "racf-saf-coverage.json",
        "zosmf-routes.json",
        "dependency-graph.json",
        "authority-graph.json",
        "state-catalog.json",
        "excluded-components.json",
        "oracle.json",
        "known-gaps.json",
        "operation-catalog.json",
        "versions.json",
    ];
    for name in required {
        let path = inventory.join(name);
        object(&json(&path)?, &path)?;
    }

    let selectors_path = inventory.join("selectors.json");
    let selectors = json(&selectors_path)?;
    let rows = array(&selectors, "selectors", &selectors_path)?;
    require(!rows.is_empty(), "selector inventory is empty")?;
    unique_rows(rows, "id", &selectors_path)?;
    for row in rows {
        require(
            text(row, "owner", &selectors_path)? == "Thang Le",
            "selector owner is not recorded",
        )?;
        require(
            text(row, "target_authority", &selectors_path)?.starts_with("mainframe-env"),
            "selector target authority is not owned",
        )?;
    }

    let cobol_path = inventory.join("cobol-constructs.json");
    let cobol = json(&cobol_path)?;
    require(
        array(&cobol, "constructs", &cobol_path)?.len() == 43,
        "COBOL inventory must contain 43 frozen statement variants",
    )?;
    let cics_path = inventory.join("cics-operations.json");
    let cics = json(&cics_path)?;
    require(
        array(&cics, "operations", &cics_path)?.len() == 22,
        "CICS inventory must contain 22 CardDemo-reached operations",
    )?;
    let routes_path = inventory.join("zosmf-routes.json");
    let routes = json(&routes_path)?;
    let route_rows = array(&routes, "routes", &routes_path)?;
    require(
        route_rows.len() == 23,
        "z/OSMF inventory must contain 23 selected routes",
    )?;
    unique_rows(route_rows, "id", &routes_path)?;

    let exclusions_path = inventory.join("excluded-components.json");
    let exclusions = json(&exclusions_path)?;
    let excluded_rows = array(&exclusions, "components", &exclusions_path)?;
    unique_rows(excluded_rows, "name", &exclusions_path)?;
    for row in excluded_rows {
        require(
            text(row, "reason", &exclusions_path)? == "excluded_from_v1",
            "invalid exclusion reason",
        )?;
    }

    let oracle_path = inventory.join("oracle.json");
    let oracle = json(&oracle_path)?;
    require(
        text(&oracle, "revision", &oracle_path)?.len() == 40,
        "oracle revision is not a Git SHA-1",
    )?;
    require(
        oracle.get("dirty") == Some(&Value::Bool(false)),
        "oracle must be recorded clean",
    )?;
    let commands = array(&oracle, "commands", &oracle_path)?;
    require(!commands.is_empty(), "oracle command inventory is empty")?;
    require(
        commands
            .iter()
            .all(|row| row.get("exit_code") == Some(&Value::from(0))),
        "an oracle entry command failed",
    )?;
    Ok(())
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct GeneratedSemanticRow {
    id: String,
    baseline: String,
    subsystem: String,
    unit: String,
    label: String,
}

fn generate_dataset_contract(root: &Path) -> TaskResult {
    let path = root.join(
        "crates/contracts/mainframe-env-host-api/src/generated/dataset_programming_surface.rs",
    );
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("{}: {error}", parent.display()))?;
    }
    fs::write(&path, render_dataset_contract(root)?)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    let ams_path = root.join("crates/apps/mainframe-env-batch/src/generated/ams_grammar.rs");
    if let Some(parent) = ams_path.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("{}: {error}", parent.display()))?;
    }
    fs::write(&ams_path, render_ams_grammar(root)?)
        .map_err(|error| format!("{}: {error}", ams_path.display()))
}

fn check_dataset_contract(root: &Path) -> TaskResult {
    let path = root.join(
        "crates/contracts/mainframe-env-host-api/src/generated/dataset_programming_surface.rs",
    );
    let expected = render_dataset_contract(root)?;
    require(
        fs::read(&path).map_err(|error| format!("{}: {error}", path.display()))? == expected,
        "generated dataset programming surface is stale; run cargo xtask dataset-contract",
    )?;
    let ams_path = root.join("crates/apps/mainframe-env-batch/src/generated/ams_grammar.rs");
    require(
        fs::read(&ams_path).map_err(|error| format!("{}: {error}", ams_path.display()))?
            == render_ams_grammar(root)?,
        "generated AMS grammar is stale; run cargo xtask dataset-contract",
    )?;
    check_dataset_surface_audit(root)?;
    check_dataset_reference_independence(root)
}

fn check_dataset_reference_independence(root: &Path) -> TaskResult {
    let path = root.join("crates/tooling/mainframe-env-conformance/src/dataset_reference.rs");
    let source = read(&path)?;
    for forbidden in [
        "mainframe_env_batch",
        "mainframe_env_dataset",
        "mainframe_env_host_api",
        "mainframe_env_store",
        "DatasetRequest",
        "DatasetResult",
        "DatasetService",
        "AmsCommand",
        "HostProblem",
        "ProviderState",
        "MAINFRAME_ENV_ZOS_AMS_ORACLE_RECEIPT",
    ] {
        require(
            !source.contains(forbidden),
            &format!(
                "independent dataset reference simulation contains forbidden product authority {forbidden}"
            ),
        )?;
    }
    for line in source
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with("use "))
    {
        require(
            line.starts_with("use serde_json::")
                || line.starts_with("use sha2::")
                || line.starts_with("use std::")
                || line == "use super::*;",
            &format!("independent dataset reference simulation has disallowed import {line}"),
        )?;
    }
    for required in [
        "conformance/0.2/catalogs/dataset-vsam-ams.json",
        "conformance/0.6/fixtures/dataset-organizations.json",
        "conformance/0.6/fixtures/ams-commands.json",
        "const MAX_DATASETS",
        "const MAX_RECORDS",
        "const MAX_TOTAL_BYTES",
        "differential_credit: 0",
        "production-observation-reuse",
        "partial-failure-publication",
        "physical-installation-unknown-boundaries",
    ] {
        require(
            source.contains(required),
            &format!("independent dataset reference simulation omits {required}"),
        )?;
    }
    let integration_path = root.join("crates/tooling/mainframe-env-conformance/src/dataset.rs");
    require(
        read(&integration_path)?.contains("run_dataset_reference_simulation"),
        "focused dataset conformance does not execute the independent reference simulation",
    )?;
    for relative in [
        "docs/prompts/coverage-versions/IMPLEMENT_0_6_0.md",
        "docs/delivery/coverage-versions/0.6.0.md",
        "docs/delivery/coverage-versions/status/0.6.0.md",
    ] {
        let document = read(&root.join(relative))?;
        require(
            document.contains("pass-with-licensed-differential-pending")
                && document.contains("0/36"),
            &format!("{relative} omits the approved 0.6 pending-differential disposition"),
        )?;
    }
    for relative in [
        "docs/prompts/coverage-versions/IMPLEMENT_0_17_0.md",
        "docs/delivery/coverage-versions/0.17.0.md",
    ] {
        let document = read(&root.join(relative))?;
        require(
            document.contains("0/36")
                && document.contains("36-row")
                && document.contains("reference")
                && document.contains("simulation"),
            &format!("{relative} omits the deferred licensed dataset campaign handoff"),
        )?;
    }
    Ok(())
}

fn check_dataset_surface_audit(root: &Path) -> TaskResult {
    let inventory_path = root.join("conformance/0.6/inventory/dataset-programming-surface.json");
    let audit_path = root.join("conformance/0.6/evidence/dataset-surface-audit.json");
    let schema_path = root.join("conformance/0.6/schemas/dataset-surface-audit.schema.json");
    let inventory = json(&inventory_path)?;
    let audit = json(&audit_path)?;
    validate_schema_instance(&json(&schema_path)?, &audit, &audit_path)?;
    require(
        audit["inventory_sha256"]
            == Value::String(format!("sha256:{}", file_digest(&inventory_path)?)),
        "dataset surface audit is bound to a different detailed inventory",
    )?;

    let mut expected = BTreeMap::<String, String>::new();
    for family in array(&inventory, "families", &inventory_path)? {
        let family_id = text(family, "id", &inventory_path)?;
        for item in array(family, "items", &inventory_path)? {
            let descriptor = format!("{family_id}:{}", text(item, "id", &inventory_path)?);
            require(
                expected
                    .insert(
                        descriptor.clone(),
                        text(item, "implementation", &inventory_path)?.to_string(),
                    )
                    .is_none(),
                &format!("duplicate detailed dataset descriptor {descriptor}"),
            )?;
        }
    }

    let mut manifests = Vec::new();
    collect_named(root, OsStr::new("Cargo.toml"), &mut manifests)?;
    let mut test_sources = BTreeMap::<String, String>::new();
    for manifest in manifests {
        let parsed: toml::Value = read(&manifest)?
            .parse()
            .map_err(|error| format!("{}: {error}", manifest.display()))?;
        let Some(package) = parsed
            .get("package")
            .and_then(|value| value.get("name"))
            .and_then(toml::Value::as_str)
        else {
            continue;
        };
        let source_root = manifest
            .parent()
            .ok_or_else(|| format!("{} has no parent", manifest.display()))?
            .join("src");
        let mut sources = Vec::new();
        if source_root.is_dir() {
            collect_extension(&source_root, OsStr::new("rs"), &mut sources)?;
        }
        sources.sort();
        let mut combined = String::new();
        for source in sources {
            combined.push_str(&read(&source)?);
            combined.push('\n');
        }
        test_sources.insert(package.to_string(), combined);
    }

    let mut seen_bindings = BTreeSet::new();
    let mut seen_descriptors = BTreeSet::new();
    let mut counts = BTreeMap::<String, usize>::new();
    for binding in array(&audit, "bindings", &audit_path)? {
        let binding_id = text(binding, "id", &audit_path)?;
        require(
            seen_bindings.insert(binding_id.to_string()),
            &format!("duplicate dataset surface audit binding {binding_id}"),
        )?;
        let disposition = text(binding, "disposition", &audit_path)?;
        for descriptor in array(binding, "descriptors", &audit_path)? {
            let descriptor = descriptor
                .as_str()
                .ok_or_else(|| format!("{audit_path:?} contains a non-text descriptor"))?;
            let implementation = expected.get(descriptor).ok_or_else(|| {
                format!("dataset surface audit references unknown descriptor {descriptor}")
            })?;
            require(
                seen_descriptors.insert(descriptor.to_string()),
                &format!("dataset surface descriptor {descriptor} has duplicate evidence"),
            )?;
            require(
                (implementation == "required" && disposition == "required-pass")
                    || (implementation == "capability-gated"
                        && matches!(disposition, "capability-pass" | "capability-conditioned")),
                &format!(
                    "dataset surface descriptor {descriptor} has disposition {disposition} incompatible with {implementation}"
                ),
            )?;
            *counts.entry(disposition.to_string()).or_default() += 1;
        }
        for test in array(binding, "tests", &audit_path)? {
            let test = test
                .as_str()
                .ok_or_else(|| format!("{audit_path:?} contains a non-text test id"))?;
            let mut components = test.split("::");
            let package = components
                .next()
                .ok_or_else(|| format!("invalid dataset evidence test id {test}"))?;
            let test_name = test
                .rsplit("::")
                .next()
                .ok_or_else(|| format!("invalid dataset evidence test id {test}"))?;
            let sources = test_sources
                .get(package)
                .ok_or_else(|| format!("dataset evidence test package {package} does not exist"))?;
            let needle = format!("fn {test_name}(");
            let at = sources.find(&needle).ok_or_else(|| {
                format!("dataset evidence test {test} does not resolve to a Rust test")
            })?;
            let prefix = &sources[at.saturating_sub(512)..at];
            let last_function = prefix.rfind("fn ").unwrap_or(0);
            let last_test_attribute = prefix.rfind("#[test]").or_else(|| prefix.rfind("::test]"));
            require(
                last_test_attribute.is_some_and(|test| test >= last_function),
                &format!("dataset evidence target {test} is not marked as a test"),
            )?;
        }
    }
    require(
        seen_descriptors == expected.keys().cloned().collect::<BTreeSet<_>>(),
        "dataset surface audit has missing or surplus detailed descriptors",
    )?;
    require(
        counts
            == BTreeMap::from([
                ("capability-conditioned".into(), 25usize),
                ("capability-pass".into(), 9usize),
                ("required-pass".into(), 97usize),
            ]),
        "dataset surface audit disposition counts drifted",
    )?;
    Ok(())
}

fn render_ams_grammar(root: &Path) -> TaskResult<Vec<u8>> {
    let grammar_path = root.join("conformance/0.6/ams/grammar.json");
    let schema_path = root.join("conformance/0.6/schemas/ams-grammar.schema.json");
    let grammar = json(&grammar_path)?;
    validate_schema_instance(&json(&schema_path)?, &grammar, &grammar_path)?;
    let commands = array(&grammar, "commands", &grammar_path)?;
    unique_rows(commands, "id", &grammar_path)?;
    require(
        commands.len() == 31,
        "AMS grammar must contain exactly 31 commands",
    )?;

    let inventory_path = root.join("conformance/0.6/inventory/dataset-programming-surface.json");
    let inventory = json(&inventory_path)?;
    let ams_family = array(&inventory, "families", &inventory_path)?
        .iter()
        .find(|family| family["id"] == Value::String("ams-commands".into()))
        .ok_or("dataset surface AMS family is missing")?;
    let ams_items = array(ams_family, "items", &inventory_path)?;
    require(
        ams_items.len() == commands.len(),
        "AMS grammar and programming surface counts differ",
    )?;
    for (command, surface) in commands.iter().zip(ams_items) {
        for field in ["id", "label", "implementation"] {
            require(
                text(command, field, &grammar_path)? == text(surface, field, &inventory_path)?,
                &format!("AMS grammar {field} differs from the programming surface"),
            )?;
        }
        let capability = command["capability"].as_str();
        require(
            (text(command, "implementation", &grammar_path)? == "required" && capability.is_none())
                || (text(command, "implementation", &grammar_path)? == "capability-gated"
                    && capability.is_some()),
            "AMS grammar capability does not match implementation class",
        )?;
    }

    let mut source =
        String::from("// @generated by `cargo xtask dataset-contract`; do not edit.\n\n");
    source.push_str(&format!(
        "pub const AMS_GRAMMAR_SHA256: &str = \"sha256:{}\";\n\n",
        file_digest(&grammar_path)?
    ));
    source.push_str("pub(crate) static AMS_GRAMMAR: &[super::AmsGrammarEntry] = &[\n");
    for command in commands {
        source.push_str("    super::AmsGrammarEntry {\n");
        for field in ["id", "label"] {
            source.push_str(&format!(
                "        {field}: {},\n",
                serde_json::to_string(text(command, field, &grammar_path)?)
                    .map_err(|error| error.to_string())?
            ));
        }
        source.push_str("        keywords: &[");
        for keyword in array(command, "keywords", &grammar_path)? {
            source.push_str(
                &serde_json::to_string(
                    keyword
                        .as_str()
                        .ok_or_else(|| "AMS grammar keyword is not text".to_string())?,
                )
                .map_err(|error| error.to_string())?,
            );
            source.push_str(", ");
        }
        source.push_str("],\n");
        source.push_str(&format!(
            "        capability: {},\n",
            command["capability"].as_str().map_or_else(
                || "None".to_string(),
                |value| format!(
                    "Some({})",
                    serde_json::to_string(value).unwrap_or_else(|_| "\"invalid\"".into())
                )
            )
        ));
        source.push_str("    },\n");
    }
    source.push_str("];\n");
    Ok(source.into_bytes())
}

fn render_dataset_contract(root: &Path) -> TaskResult<Vec<u8>> {
    let inventory_path = root.join("conformance/0.6/inventory/dataset-programming-surface.json");
    let schema_path = root.join("conformance/0.6/schemas/dataset-programming-surface.schema.json");
    let inventory = json(&inventory_path)?;
    validate_schema_instance(&json(&schema_path)?, &inventory, &inventory_path)?;
    let expected_counts = BTreeMap::from([
        ("access-modes", 5usize),
        ("allocation", 12),
        ("ams-commands", 31),
        ("catalog", 12),
        ("dcb", 12),
        ("lifecycle", 10),
        ("organizations", 12),
        ("provider-capabilities", 20),
        ("sms", 9),
        ("volume", 8),
    ]);
    let families = array(&inventory, "families", &inventory_path)?;
    let actual_counts = families
        .iter()
        .map(|family| {
            Ok((
                text(family, "id", &inventory_path)?,
                array(family, "items", &inventory_path)?.len(),
            ))
        })
        .collect::<TaskResult<BTreeMap<_, _>>>()?;
    require(
        actual_counts == expected_counts,
        "dataset programming family inventory count drifted",
    )?;

    let official_path = root.join("conformance/0.2/catalogs/dataset-vsam-ams.json");
    let official = json(&official_path)?;
    let official_rows = array(&official, "units", &official_path)?
        .iter()
        .flat_map(|unit| array(unit, "rows", &official_path).into_iter().flatten())
        .map(|row| text(row, "id", &official_path).map(str::to_string))
        .collect::<TaskResult<BTreeSet<_>>>()?;
    require(
        official_rows.len() == 36,
        "dataset contract did not load the frozen 36-row official denominator",
    )?;

    let mut descriptors = Vec::new();
    let mut identities = BTreeSet::new();
    let mut ams_rows = Vec::new();
    let mut organization_rows = BTreeSet::new();
    for family in families {
        let family_id = text(family, "id", &inventory_path)?;
        for item in array(family, "items", &inventory_path)? {
            let item_id = text(item, "id", &inventory_path)?;
            require(
                identities.insert(format!("{family_id}:{item_id}")),
                &format!("duplicate dataset surface identity {family_id}:{item_id}"),
            )?;
            let rows = array(item, "official_rows", &inventory_path)?;
            for row in rows {
                let row_id = row.as_str().ok_or_else(|| {
                    format!("{} official row is not text", inventory_path.display())
                })?;
                require(
                    official_rows.contains(row_id),
                    &format!("dataset surface references unknown official row {row_id}"),
                )?;
                if family_id == "ams-commands" {
                    ams_rows.push(row_id.to_string());
                } else if family_id == "organizations" {
                    organization_rows.insert(row_id.to_string());
                }
            }
            require(
                text(item, "implementation", &inventory_path)? != "capability-gated"
                    || text(item, "effect", &inventory_path)?
                        .to_ascii_lowercase()
                        .contains("capability"),
                &format!("{family_id}:{item_id} does not state its capability behavior"),
            )?;
            descriptors.push((family_id, item));
        }
    }
    require(
        descriptors.len() == 131 && ams_rows.len() == 31 && organization_rows.len() == 5,
        "dataset surface denominator or official mapping is incomplete",
    )?;

    let official_units = array(&official, "units", &official_path)?;
    let command_rows = array(&official_units[0], "rows", &official_path)?;
    let expected_ams_rows = command_rows
        .iter()
        .map(|row| text(row, "id", &official_path).map(str::to_string))
        .collect::<TaskResult<Vec<_>>>()?;
    require(
        ams_rows == expected_ams_rows,
        "31-command inventory differs from the frozen official command order",
    )?;
    let ams_items = families
        .iter()
        .find(|family| family["id"] == Value::String("ams-commands".into()))
        .ok_or("AMS command family is missing")?;
    for (item, official_row) in array(ams_items, "items", &inventory_path)?
        .iter()
        .zip(command_rows)
    {
        let official_label = text(official_row, "label", &official_path)?;
        let command = official_label
            .split_once(". ")
            .map(|(_, command)| command)
            .ok_or_else(|| format!("official AMS label has no chapter prefix: {official_label}"))?;
        require(
            text(item, "label", &inventory_path)? == command,
            &format!("AMS command label differs from official row: {command}"),
        )?;
    }

    descriptors.sort_by(|left, right| {
        (
            left.0,
            text(left.1, "id", &inventory_path).unwrap_or_default(),
        )
            .cmp(&(
                right.0,
                text(right.1, "id", &inventory_path).unwrap_or_default(),
            ))
    });
    let mut source =
        String::from("// @generated by `cargo xtask dataset-contract`; do not edit.\n\n");
    source.push_str(&format!(
        "pub const DATASET_SURFACE_INVENTORY_SHA256: &str = \"sha256:{}\";\n\n",
        file_digest(&inventory_path)?
    ));
    source.push_str("pub const DATASET_SURFACE_DESCRIPTORS: &[DatasetSurfaceDescriptor] = &[\n");
    for (family, item) in descriptors {
        source.push_str("    DatasetSurfaceDescriptor {\n");
        for (field, value) in [
            ("family", family),
            ("id", text(item, "id", &inventory_path)?),
            ("label", text(item, "label", &inventory_path)?),
            ("authority", text(item, "authority", &inventory_path)?),
            (
                "implementation",
                text(item, "implementation", &inventory_path)?,
            ),
            ("effect", text(item, "effect", &inventory_path)?),
        ] {
            source.push_str(&format!(
                "        {field}: {},\n",
                serde_json::to_string(value).map_err(|error| error.to_string())?
            ));
        }
        for (field, values) in [
            ("operands", array(item, "operands", &inventory_path)?),
            (
                "official_rows",
                array(item, "official_rows", &inventory_path)?,
            ),
        ] {
            source.push_str(&format!("        {field}: &["));
            for value in values {
                source.push_str(
                    &serde_json::to_string(value.as_str().ok_or_else(|| {
                        format!("{} {field} value is not text", inventory_path.display())
                    })?)
                    .map_err(|error| error.to_string())?,
                );
                source.push_str(", ");
            }
            source.push_str("],\n");
        }
        source.push_str("    },\n");
    }
    source.push_str("];\n");
    Ok(source.into_bytes())
}

fn generate_semantic_identities(root: &Path) -> TaskResult {
    let path = root.join(
        "crates/contracts/mainframe-env-host-api/src/generated/official_semantic_identities.rs",
    );
    let manifest_path = root.join("conformance/0.2/generated/semantic-identities.json");
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("{}: {error}", parent.display()))?;
    }
    if let Some(parent) = manifest_path.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("{}: {error}", parent.display()))?;
    }
    let (source, manifest) = semantic_identity_documents(root)?;
    fs::write(&path, source).map_err(|error| format!("{}: {error}", path.display()))?;
    fs::write(&manifest_path, manifest)
        .map_err(|error| format!("{}: {error}", manifest_path.display()))
}

fn check_semantic_identities(root: &Path) -> TaskResult {
    let path = root.join(
        "crates/contracts/mainframe-env-host-api/src/generated/official_semantic_identities.rs",
    );
    let manifest_path = root.join("conformance/0.2/generated/semantic-identities.json");
    let (expected, expected_manifest) = semantic_identity_documents(root)?;
    let actual = fs::read(&path).map_err(|error| format!("{}: {error}", path.display()))?;
    require(
        actual == expected,
        "generated semantic identities are stale; run cargo xtask semantic-identities",
    )?;
    let actual_manifest = fs::read(&manifest_path)
        .map_err(|error| format!("{}: {error}", manifest_path.display()))?;
    require(
        actual_manifest == expected_manifest,
        "generated semantic identity manifest is stale; run cargo xtask semantic-identities",
    )?;
    let identity_manifest: Value = serde_json::from_slice(&actual_manifest)
        .map_err(|error| format!("{}: {error}", manifest_path.display()))?;
    let handlers_path = root.join("conformance/0.2/generated/subsystem-handlers.json");
    let handlers = json(&handlers_path)?;
    require(
        handlers["schema_version"]
            == Value::String("mainframe-env.subsystem-handler-registry@1".into())
            && handlers["generation"].as_u64() == Some(1)
            && handlers["generated_identity_set_sha256"]
                == identity_manifest["identity_set_sha256"]
            && handlers["automatic_registration"] == Value::Bool(false)
            && handlers["handlers"].as_array().is_some_and(Vec::is_empty)
            && handlers["coverage_credit"].as_u64() == Some(0),
        "initial subsystem handler registry must remain explicit, empty, and zero-credit",
    )
}

fn semantic_identity_documents(root: &Path) -> TaskResult<(Vec<u8>, Vec<u8>)> {
    let index_path = root.join("conformance/0.2/catalogs/index.json");
    let index = json(&index_path)?;
    let mut rows = Vec::new();
    for baseline in array(&index, "baselines", &index_path)? {
        let baseline_id = text(baseline, "id", &index_path)?;
        let subsystem = text(baseline, "subsystem", &index_path)?;
        let catalog_path = root.join(text(baseline, "catalog", &index_path)?);
        let catalog = json(&catalog_path)?;
        for unit in array(&catalog, "units", &catalog_path)? {
            let unit_id = text(unit, "id", &catalog_path)?;
            for row in array(unit, "rows", &catalog_path)? {
                rows.push(GeneratedSemanticRow {
                    id: text(row, "id", &catalog_path)?.to_string(),
                    baseline: baseline_id.to_string(),
                    subsystem: subsystem.to_string(),
                    unit: unit_id.to_string(),
                    label: text(row, "label", &catalog_path)?.to_string(),
                });
            }
        }
    }
    rows.sort();
    require(
        rows.len() == index["mandatory_rows"].as_u64().unwrap_or_default() as usize,
        "generated semantic identity count differs from official denominator",
    )?;
    require(
        rows.windows(2).all(|pair| pair[0].id != pair[1].id),
        "generated semantic identity rows are not unique",
    )?;
    let mut set_digest = Sha256::new();
    for row in &rows {
        for field in [
            row.id.as_bytes(),
            row.baseline.as_bytes(),
            row.subsystem.as_bytes(),
            row.unit.as_bytes(),
            row.label.as_bytes(),
        ] {
            set_digest.update((field.len() as u64).to_be_bytes());
            set_digest.update(field);
        }
    }
    let catalog_digest = file_digest(&index_path)?;
    let set_digest = format!("{:x}", set_digest.finalize());
    let mut source =
        String::from("// @generated by `cargo xtask semantic-identities`; do not edit.\n\n");
    source.push_str(&format!(
        "pub const GENERATED_IDENTITY_CATALOG_SHA256: &str = \"sha256:{catalog_digest}\";\n"
    ));
    source.push_str(&format!(
        "pub const GENERATED_IDENTITY_SET_SHA256: &str = \"sha256:{set_digest}\";\n\n"
    ));
    source.push_str("pub const OFFICIAL_SEMANTIC_IDENTITIES: &[SemanticIdentityDescriptor] = &[\n");
    for row in &rows {
        source.push_str("    SemanticIdentityDescriptor {\n");
        source.push_str(&format!("        id: {:?},\n", row.id));
        source.push_str(&format!("        baseline: {:?},\n", row.baseline));
        source.push_str(&format!("        subsystem: {:?},\n", row.subsystem));
        source.push_str(&format!("        unit: {:?},\n", row.unit));
        source.push_str(&format!("        label: {:?},\n", row.label));
        source.push_str("    },\n");
    }
    source.push_str("];\n");
    let manifest = json!({
        "schema_version":"mainframe-env.generated-semantic-identities@1",
        "target_version":"0.2.0",
        "contract":"mainframe-env.generated-semantic-identity@1",
        "catalog_index":"conformance/0.2/catalogs/index.json",
        "catalog_index_sha256":format!("sha256:{catalog_digest}"),
        "identity_set_sha256":format!("sha256:{set_digest}"),
        "official_identity_count":rows.len(),
        "handler_registration_count":0,
        "coverage_credit":0
    });
    Ok((source.into_bytes(), pretty_json(&manifest)?))
}

fn check_application_packages(root: &Path) -> TaskResult {
    let contracts_path = root.join("conformance/0.2/inventory/contracts.json");
    let contracts = json(&contracts_path)?;
    let expected = [
        (
            "application_package_v2",
            "mainframe-env.application-package@2",
        ),
        (
            "application_install_generation",
            "mainframe-env.application-install-generation@1",
        ),
        (
            "host_abi_library_section",
            "mainframe-env.application.host-abi-libraries@1",
        ),
        ("sql_section", "mainframe-env.application.sql@1"),
        ("ims_section", "mainframe-env.application.ims@1"),
        ("mq_section", "mainframe-env.application.mq@1"),
        (
            "batch_controller_section",
            "mainframe-env.application.batch-controllers@1",
        ),
        (
            "security_resource_section",
            "mainframe-env.application.security-resources@1",
        ),
        (
            "application_installer_state",
            "mainframe-env.application-installer@1",
        ),
        (
            "application_publication_state",
            "mainframe-env.application-publication@1",
        ),
    ];
    for (name, identity) in expected {
        require(
            contracts["contracts"][name].as_str() == Some(identity),
            &format!("application package contract inventory omits {identity}"),
        )?;
    }
    let migration_path = root.join("conformance/0.2/migrations/application-package-v1-to-v2.json");
    let migration = json(&migration_path)?;
    require(
        migration["schema_version"]
            == Value::String("mainframe-env.application-package-migration@1".into())
            && migration["from_contract"]
                == Value::String("mainframe-env.application-package@1".into())
            && migration["to_contract"]
                == Value::String("mainframe-env.application-package@2".into())
            && migration["destructive"] == Value::Bool(false)
            && migration["old_reader_retained"] == Value::Bool(true)
            && migration["provider_state_changed"] == Value::Bool(true)
            && migration["atomicity"]["validate_all_references_before_stage"] == Value::Bool(true)
            && migration["atomicity"]["staged_generation_selectable"] == Value::Bool(false)
            && migration["atomicity"]["ready_and_selection_same_critical_section"]
                == Value::Bool(true)
            && migration["atomicity"]["production_trust_authority_injected"] == Value::Bool(true)
            && migration["atomicity"]["selected_handle_required_for_publication"]
                == Value::Bool(true)
            && migration["atomicity"]["retained_packages_persisted_and_reverified_on_open"]
                == Value::Bool(true)
            && migration["atomicity"]["publication_identity_bound_to_expected_generation"]
                == Value::Bool(true)
            && migration["atomicity"]["publication_sections_recover_from_explicit_durable_states"]
                == Value::Bool(true)
            && migration["atomicity"]["partial_publication_never_marks_generation_selected"]
                == Value::Bool(true)
            && migration["atomicity"]["aggregate_and_retained_bytes_bounded_before_clone"]
                == Value::Bool(true)
            && migration["rollback"]["supported"] == Value::Bool(true)
            && migration["rollback"]["partial_generation_selected"] == Value::Bool(false),
        "application package migration or rollback contract is unsafe",
    )?;
    for path in [
        "conformance/0.2/schemas/application-package-v2.schema.json",
        "conformance/0.2/schemas/application-install-generation.schema.json",
        "conformance/0.2/schemas/application-installer-state.schema.json",
        "conformance/0.2/schemas/application-publication-state.schema.json",
        "docs/architecture/APPLICATION-PACKAGES.md",
        "crates/kernel/mainframe-env-application/README.md",
    ] {
        require(
            root.join(path).is_file(),
            &format!("application package contract artifact is missing: {path}"),
        )?;
    }
    let implementation =
        read(&root.join("crates/kernel/mainframe-env-application/src/package_v2.rs"))?;
    for required in [
        "PackageSignatureVerifier",
        "host_abi_libraries",
        "sql_tables",
        "ims_definitions",
        "mq_resources",
        "batch_controllers",
        "security_resources",
        "pub fn stage",
        "pub fn commit",
        "pub fn rollback",
        "max_total_blob_bytes",
        "max_total_nested_items",
        "max_retained_package_bytes",
        "max_total_retained_package_bytes",
        "validate_aggregate_bounds",
        "pub fn selected_generation",
    ] {
        require(
            implementation.contains(required),
            &format!("application package implementation omits {required}"),
        )?;
    }
    let product = read(&root.join("crates/apps/mainframe-env-server/src/product.rs"))?;
    let production_product = product.split("#[cfg(test)]").next().unwrap_or(&product);
    for required in [
        "applications_v2: Mutex<DurableApplicationsV2>",
        "pub fn open_with_package_trust",
        "HmacSha256PackageTrust",
        "MAINFRAME_ENV_PACKAGE_HMAC_KEY_REFS",
        "APPLICATION_PUBLICATION_CONTRACT",
        "pub fn publish_application_generation",
        "pub fn rollback_application_generation",
        "fn selected_application_v2",
    ] {
        require(
            production_product.contains(required),
            &format!("production composition omits {required}"),
        )?;
    }
    Ok(())
}

fn check_db2_catalog(root: &Path) -> TaskResult {
    let service_path = root.join("crates/providers/mainframe-env-db2/src/service.rs");
    let service = read(&service_path)?;
    let production = service.split("#[cfg(test)]").next().unwrap_or(&service);
    let upper = production.to_ascii_uppercase();
    for forbidden in [
        "CARDDEMO",
        "TRANSACTION_TYPE",
        "TRANSACTION_TYPE_CATEGORY",
        "AUTHFRDS",
        "TYPE-CD-FILTER",
        "TYPE-DESC-FILTER",
        "CARD-NUM",
        "AUTH-TS",
        "AUTH-FRAUD",
    ] {
        require(
            !upper.contains(forbidden),
            &format!("Db2 production source contains application identity {forbidden}"),
        )?;
    }
    for required in [
        "pub fn install_catalog",
        "pub fn rollback_catalog",
        "Db2CatalogGeneration",
        "snapshot_selected_catalog",
        "CatalogGenerationSnapshot",
        "schemas_compatible",
        "delete_is_restricted",
        "selected_column_indices",
        "update_assignments",
        "validate_foreign_keys",
    ] {
        require(
            production.contains(required),
            &format!("generic Db2 catalog implementation omits {required}"),
        )?;
    }
    let contracts_path = root.join("conformance/0.2/inventory/contracts.json");
    let contracts = json(&contracts_path)?;
    require(
        contracts["contracts"]["db2_application_catalog"]
            == Value::String("mainframe-env.db2-application-catalog@1".into()),
        "Db2 application catalog contract is not frozen",
    )?;
    let migration_path = root.join("conformance/0.2/migrations/db2-catalog-v1-to-v2.json");
    let migration = json(&migration_path)?;
    require(
        migration["schema_version"]
            == Value::String("mainframe-env.db2-catalog-migration@1".into())
            && migration["destructive"] == Value::Bool(false)
            && migration["old_rows_readable"] == Value::Bool(true)
            && migration["schema_fields_have_defaults"] == Value::Bool(true)
            && migration["installation_requires_quiescence"] == Value::Bool(true)
            && migration["atomic_catalog_write"] == Value::Bool(true)
            && migration["compatible_rows_preserved_on_first_install"] == Value::Bool(true)
            && migration["compatible_rows_preserved_on_upgrade"] == Value::Bool(true)
            && migration["retained_generation_snapshots"] == Value::Bool(true)
            && migration["restart_reload_supported"] == Value::Bool(true)
            && migration["rollback"]["supported"] == Value::Bool(true)
            && migration["rollback"]["requires_backup"] == Value::Bool(true),
        "Db2 catalog migration or rollback contract is unsafe",
    )?;
    let scan_path = root.join("conformance/0.2/evidence/hardcode/CV-205-db2.json");
    let scan = json(&scan_path)?;
    require(
        scan["before"]["h1_h3_lines"].as_u64() == Some(29)
            && scan["after"]["h1_h3_lines"].as_u64() == Some(0)
            && scan["after"]["application_table_identifiers"].as_u64() == Some(0)
            && scan["after"]["application_host_variable_identifiers"].as_u64() == Some(0)
            && scan["after"]["application_string_dispatch"].as_u64() == Some(0),
        "Db2 hardcode scan receipt does not prove 29 to zero",
    )?;
    for path in [
        "conformance/0.2/schemas/db2-application-catalog.schema.json",
        "docs/architecture/DB2-APPLICATION-CATALOG.md",
        "crates/providers/mainframe-env-db2/README.md",
    ] {
        require(
            root.join(path).is_file(),
            &format!("Db2 catalog artifact is missing: {path}"),
        )?;
    }
    Ok(())
}

fn check_batch_controllers(root: &Path) -> TaskResult {
    let service_path = root.join("crates/apps/mainframe-env-batch/src/service.rs");
    let controller_path = root.join("crates/apps/mainframe-env-batch/src/controller.rs");
    let service = read(&service_path)?;
    let controller = read(&controller_path)?;
    let production_service = service.split("#[cfg(test)]").next().unwrap_or(&service);
    let production_controller = controller
        .split("#[cfg(test)]")
        .next()
        .unwrap_or(&controller);
    let production = format!("{production_service}\n{production_controller}").to_ascii_uppercase();
    for forbidden in [
        "COBTUPDT",
        "CBPAUP0C",
        "PAUDBLOD",
        "PAUDBUNL",
        "DBPAUTP0",
        "PSBPAUTB",
        "PAUTSUM0",
        "PAUTDTL1",
        "AUTHORIZATION-ADJUSTED",
    ] {
        require(
            !production.contains(forbidden),
            &format!("batch production source contains application identity {forbidden}"),
        )?;
    }
    for required in [
        "BatchControllerRegistry",
        "BatchControllerGeneration",
        "BatchControllerSelector",
        "BatchControllerPlan",
        "BatchControllerProgram",
        "pub(crate) fn install",
        "pub(crate) fn select",
        "pub(crate) fn state",
        "pub(crate) fn from_state",
        "pub(crate) fn resolve",
        "Publish only after the full generation validates",
    ] {
        require(
            production_controller.contains(required),
            &format!("batch controller registry omits {required}"),
        )?;
    }
    for required in [
        "pub fn install_controllers",
        "pub fn rollback_controllers",
        "persist_controllers",
        "CONTROLLER_STATE_NAMESPACE",
        "ims_controller_selector",
        "resolve_controller",
        "execute_program_controller",
    ] {
        require(
            production_service.contains(required),
            &format!("batch controller service integration omits {required}"),
        )?;
    }
    let product = read(&root.join("crates/apps/mainframe-env-server/src/product.rs"))?;
    let production_product = product.split("#[cfg(test)]").next().unwrap_or(&product);
    require(
        production_product.contains("pub fn publish_application_generation")
            && production_product.contains("selected_application_v2")
            && production_product.contains("apply_application_batch_controllers")
            && production_product.contains("BatchControllerProgram")
            && production_product.contains("decode_application_batch_controller")
            && !production_product.contains("selected_identity: &str"),
        "composition does not derive controllers from a verified selected package handle",
    )?;
    let contracts_path = root.join("conformance/0.2/inventory/contracts.json");
    let contracts = json(&contracts_path)?;
    require(
        contracts["contracts"]["batch_controller_registry"]
            == Value::String("mainframe-env.batch-controller-registry@1".into()),
        "batch controller registry contract is not frozen",
    )?;
    let migration_path = root.join("conformance/0.2/migrations/batch-controller-v0-to-v1.json");
    let migration = json(&migration_path)?;
    require(
        migration["schema_version"]
            == Value::String("mainframe-env.batch-controller-migration@1".into())
            && migration["destructive"] == Value::Bool(false)
            && migration["production_branch_fallback_retained"] == Value::Bool(false)
            && migration["installation"]["bounded"] == Value::Bool(true)
            && migration["installation"]["validate_complete_generation_before_publish"]
                == Value::Bool(true)
            && migration["installation"]["selector_bound_to_program_artifact"] == Value::Bool(true)
            && migration["installation"]["selector_conflicts_fail_closed"] == Value::Bool(true)
            && migration["installation"]["retained_generations_persisted_atomically"]
                == Value::Bool(true)
            && migration["installation"]["selected_generation_reloaded_on_open"]
                == Value::Bool(true)
            && migration["rollback"]["supported"] == Value::Bool(true)
            && migration["rollback"]["partial_generation_selected"] == Value::Bool(false)
            && migration["rollback"]["unseen_older_generation_accepted"] == Value::Bool(false),
        "batch controller migration or rollback contract is unsafe",
    )?;
    let scan_path = root.join("conformance/0.2/evidence/hardcode/CV-206-batch.json");
    let scan = json(&scan_path)?;
    require(
        scan["before"]["h1_h3_lines"].as_u64() == Some(3)
            && scan["after"]["h1_h3_lines"].as_u64() == Some(0)
            && scan["after"]["application_program_identifiers"].as_u64() == Some(0)
            && scan["after"]["application_data_identifiers"].as_u64() == Some(0)
            && scan["after"]["application_string_dispatch"].as_u64() == Some(0),
        "batch hardcode scan receipt does not prove three to zero",
    )?;
    for path in [
        "conformance/0.2/schemas/batch-controller-registry.schema.json",
        "docs/architecture/BATCH-CONTROLLER-REGISTRY.md",
        "crates/apps/mainframe-env-batch/README.md",
    ] {
        require(
            root.join(path).is_file(),
            &format!("batch controller artifact is missing: {path}"),
        )?;
    }
    Ok(())
}

fn generate_host_abi_inventory(root: &Path) -> TaskResult {
    let receipt = verify_host_abi_libraries()?;
    let value = serde_json::to_value(receipt).map_err(|error| error.to_string())?;
    let path = root.join("conformance/0.2/abi/libraries.json");
    fs::create_dir_all(path.parent().ok_or("host ABI inventory has no parent")?)
        .map_err(|error| error.to_string())?;
    fs::write(&path, pretty_json(&value)?).map_err(|error| format!("{}: {error}", path.display()))
}

fn check_host_abi_libraries(root: &Path) -> TaskResult {
    let receipt = verify_host_abi_libraries()?;
    let expected = serde_json::to_value(receipt).map_err(|error| error.to_string())?;
    let inventory_path = root.join("conformance/0.2/abi/libraries.json");
    require(
        json(&inventory_path)? == expected,
        "subsystem ABI inventory is stale; run cargo xtask abi-libraries",
    )?;
    let mut compiler_files = Vec::new();
    collect_extension(
        &root.join("crates/kernel/mainframe-env-compiler/src"),
        OsStr::new("rs"),
        &mut compiler_files,
    )?;
    let compiler = compiler_files
        .iter()
        .map(|path| read(path))
        .collect::<TaskResult<Vec<_>>>()?
        .join("\n")
        .to_ascii_uppercase();
    for forbidden in [
        "DFHAID",
        "DFHBMSCA",
        "SQLCA",
        "CMQGMOV",
        "CMQMDV",
        "CMQODV",
        "CMQPMOV",
        "CMQTML",
        "CMQV",
        "OWNED_COMPATIBILITY_LIBRARY",
        "COMPATIBILITY_COPYBOOKS",
    ] {
        require(
            !compiler.contains(forbidden),
            &format!("compiler still owns or names host ABI member {forbidden}"),
        )?;
    }
    require(
        !root
            .join("crates/kernel/mainframe-env-compiler/src/compatibility.rs")
            .exists(),
        "compiler compatibility asset module still exists",
    )?;
    for path in [
        "crates/providers/mainframe-env-cics/abi/DFHAID.cpy",
        "crates/providers/mainframe-env-cics/abi/DFHBMSCA.cpy",
        "crates/providers/mainframe-env-db2/abi/SQLCA.cpy",
        "crates/providers/mainframe-env-mq/abi/CMQGMOV.cpy",
        "crates/providers/mainframe-env-mq/abi/CMQMDV.cpy",
        "crates/providers/mainframe-env-mq/abi/CMQODV.cpy",
        "crates/providers/mainframe-env-mq/abi/CMQPMOV.cpy",
        "crates/providers/mainframe-env-mq/abi/CMQTML.cpy",
        "crates/providers/mainframe-env-mq/abi/CMQV.cpy",
        "conformance/0.2/schemas/host-abi-source-library.schema.json",
        "docs/architecture/HOST-ABI-SOURCE-LIBRARIES.md",
    ] {
        require(
            root.join(path).is_file(),
            &format!("host ABI artifact is missing: {path}"),
        )?;
    }
    let contracts_path = root.join("conformance/0.2/inventory/contracts.json");
    let contracts = json(&contracts_path)?;
    require(
        contracts["contracts"]["host_abi_source_library"]
            == Value::String("mainframe-env.host-abi-source-library@1".into()),
        "host ABI source-library contract is not frozen",
    )?;
    let migration_path = root.join("conformance/0.2/migrations/host-abi-v0-to-v1.json");
    let migration = json(&migration_path)?;
    require(
        migration["schema_version"] == Value::String("mainframe-env.host-abi-migration@1".into())
            && migration["destructive"] == Value::Bool(false)
            && migration["compiler_asset_fallback_retained"] == Value::Bool(false)
            && migration["old_source_bundle_receipts_reproducible"] == Value::Bool(true)
            && migration["rollback"]["supported"] == Value::Bool(true)
            && migration["rollback"]["partial_library_selected"] == Value::Bool(false),
        "host ABI migration or rollback contract is unsafe",
    )?;
    let scan_path = root.join("conformance/0.2/evidence/hardcode/CV-207-abi.json");
    let scan = json(&scan_path)?;
    require(
        scan["before"]["compiler_owned_members"].as_u64() == Some(9)
            && scan["after"]["compiler_owned_members"].as_u64() == Some(0)
            && scan["after"]["subsystem_owned_members"].as_u64() == Some(9)
            && scan["after"]["generated_coverage_credit"].as_u64() == Some(0),
        "host ABI ownership scan does not prove nine compiler members moved with zero credit",
    )
}

fn generate_program_registry(root: &Path) -> TaskResult {
    let source = render_program_registry(root)?;
    let path = root.join("crates/apps/mainframe-env-batch/src/generated/common_programs.rs");
    fs::create_dir_all(path.parent().ok_or("program registry has no parent")?)
        .map_err(|error| error.to_string())?;
    fs::write(&path, source).map_err(|error| format!("{}: {error}", path.display()))
}

fn check_program_registry(root: &Path) -> TaskResult {
    let expected = render_program_registry(root)?;
    let path = root.join("crates/apps/mainframe-env-batch/src/generated/common_programs.rs");
    require(
        fs::read(&path).map_err(|error| format!("{}: {error}", path.display()))? == expected,
        "common program registry is stale; run cargo xtask program-registry",
    )?;
    let implementation = read(&root.join("crates/apps/mainframe-env-batch/src/program.rs"))?;
    for forbidden in [
        "match program.to_ascii_uppercase().as_str()",
        "for name in [",
        "match self.0 {",
    ] {
        require(
            !implementation.contains(forbidden),
            &format!("batch program routing retains handwritten string dispatch {forbidden}"),
        )?;
    }
    for required in [
        "generated/common_programs.rs",
        "COMMON_PROGRAMS",
        "ProgramExecution",
        "BuiltinProgram",
    ] {
        require(
            implementation.contains(required),
            &format!("batch program registry integration omits {required}"),
        )?;
    }
    let service = read(&root.join("crates/apps/mainframe-env-batch/src/service.rs"))?;
    let execution_region = service
        .split("let input = ProgramInput")
        .nth(1)
        .and_then(|tail| tail.split("self.write_dd_outputs").next())
        .ok_or("batch execution region is missing")?;
    for forbidden in ["eq_ignore_ascii_case(\"", "utility_disposition("] {
        require(
            !execution_region.contains(forbidden),
            &format!("batch execution retains name dispatch {forbidden}"),
        )?;
    }
    let contracts = json(&root.join("conformance/0.2/inventory/contracts.json"))?;
    require(
        contracts["contracts"]["common_program_catalog"]
            == Value::String("mainframe-env.common-program-catalog@1".into()),
        "common program catalog contract is not frozen",
    )?;
    for path in [
        "conformance/0.2/programs/common-programs.json",
        "conformance/0.2/schemas/common-program-catalog.schema.json",
        "docs/architecture/PROGRAM-AND-ROUTE-REGISTRIES.md",
    ] {
        require(
            root.join(path).is_file(),
            &format!("common program registry artifact is missing: {path}"),
        )?;
    }
    Ok(())
}

fn render_program_registry(root: &Path) -> TaskResult<Vec<u8>> {
    let catalog_path = root.join("conformance/0.2/programs/common-programs.json");
    let catalog = json(&catalog_path)?;
    require(
        catalog["schema_version"] == Value::String("mainframe-env.common-program-catalog@1".into())
            && catalog["target_version"] == Value::String("0.2.0".into())
            && catalog["generated_coverage_credit"].as_u64() == Some(0),
        "common program catalog identity or coverage policy is invalid",
    )?;
    let programs = array(&catalog, "programs", &catalog_path)?;
    let expected_names = BTreeSet::from([
        "CEE3ABD", "CEEDAYS", "COBDATFT", "DFSRRC00", "DSNTEP4", "DSNTIAD", "DSNTIAUL", "FTP",
        "IEBCOMPR", "IEBCOPY", "IEBDG", "IEBEDIT", "IEBGENER", "IEBUPDTE", "IEFBR14", "IDCAMS",
        "IKJEFT01", "IKJEFT1B", "MVSWAIT", "SDSF", "SORT",
    ]);
    let names = programs
        .iter()
        .map(|program| text(program, "name", &catalog_path))
        .collect::<TaskResult<BTreeSet<_>>>()?;
    require(
        names == expected_names && names.len() == programs.len(),
        "common program catalog names are missing or duplicated",
    )?;
    let mut builtin_variants = Vec::new();
    let mut tso_variants = Vec::new();
    let mut system_service_variants = Vec::new();
    for program in programs {
        let name = text(program, "name", &catalog_path)?;
        require(
            name == name.to_ascii_uppercase()
                && !name.is_empty()
                && name.len() <= 128
                && name.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric() || matches!(byte, b'$' | b'@' | b'#' | b'_' | b'-')
                }),
            &format!("common program name {name:?} is invalid"),
        )?;
        let disposition = program["disposition"].as_str();
        let execution = text(program, "execution", &catalog_path)?;
        require(
            disposition.is_none_or(|disposition| {
                matches!(
                    disposition,
                    "implemented"
                        | "cics-file-control"
                        | "network-ftp"
                        | "report-rexx"
                        | "db2-tso"
                        | "ims-controller"
                )
            }) && matches!(
                execution,
                "program-service"
                    | "idcams"
                    | "sdsf"
                    | "db2-tso"
                    | "ims-controller"
                    | "unsupported"
            ),
            &format!("common program {name} disposition or execution is invalid"),
        )?;
        if let Some(builtin) = program["builtin"].as_str() {
            require(
                disposition == Some("implemented")
                    && matches!(execution, "program-service" | "idcams")
                    && builtin == builtin.to_ascii_lowercase(),
                &format!("common program {name} builtin binding is invalid"),
            )?;
            builtin_variants.push(rust_variant(builtin)?);
        } else if disposition.is_some() {
            require(
                execution != "program-service" && execution != "idcams",
                &format!("common program {name} is missing its builtin binding"),
            )?;
        }
        if let Some(action) = program["tso_action"].as_str() {
            require(
                disposition.is_none()
                    && execution == "unsupported"
                    && program["builtin"].is_null()
                    && program["system_service"].is_null()
                    && matches!(action, "execute-script" | "extract"),
                &format!("common program {name} TSO action is invalid"),
            )?;
            tso_variants.push(rust_variant(action)?);
        }
        if let Some(service) = program["system_service"].as_str() {
            require(
                disposition.is_none()
                    && execution == "unsupported"
                    && program["builtin"].is_null()
                    && program["tso_action"].is_null(),
                &format!("common program {name} system service is invalid"),
            )?;
            system_service_variants.push(rust_variant(service)?);
        }
        require(
            disposition.is_some()
                || program["tso_action"].is_string()
                || program["system_service"].is_string(),
            &format!("common program {name} has no registry role"),
        )?;
    }
    let unique_builtins = builtin_variants.iter().collect::<BTreeSet<_>>();
    require(
        unique_builtins.len() == builtin_variants.len() && builtin_variants.len() == 9,
        "common program builtin variants are missing or duplicated",
    )?;
    require(
        tso_variants.iter().collect::<BTreeSet<_>>().len() == 2
            && system_service_variants
                .iter()
                .collect::<BTreeSet<_>>()
                .len()
                == 4,
        "common program internal variants are missing or duplicated",
    )?;
    let mut source =
        String::from("// @generated by `cargo xtask program-registry`; do not edit.\n\n");
    source.push_str("#[derive(Clone, Copy, Debug, Eq, PartialEq)]\n");
    source.push_str("pub(crate) enum BuiltinProgram {\n");
    builtin_variants.sort();
    for variant in &builtin_variants {
        source.push_str(&format!("    {variant},\n"));
    }
    source.push_str("}\n\n");
    tso_variants.sort();
    tso_variants.dedup();
    source.push_str("#[derive(Clone, Copy, Debug, Eq, PartialEq)]\n");
    source.push_str("pub(crate) enum TsoProgramExecution {\n");
    for variant in &tso_variants {
        source.push_str(&format!("    {variant},\n"));
    }
    source.push_str("}\n\n");
    system_service_variants.sort();
    source.push_str("#[derive(Clone, Copy, Debug, Eq, PartialEq)]\n");
    source.push_str("pub enum SystemServiceProgram {\n");
    for variant in &system_service_variants {
        source.push_str(&format!("    {variant},\n"));
    }
    source.push_str("}\n\n");
    source.push_str(&format!(
        "pub(crate) const COMMON_PROGRAM_CATALOG_SHA256: &str =\n    \"sha256:{}\";\n\n",
        file_digest(&catalog_path)?
    ));
    source.push_str("pub(crate) static COMMON_PROGRAMS: &[super::CommonProgramEntry] = &[\n");
    for program in programs {
        let Some(disposition) = program["disposition"].as_str() else {
            continue;
        };
        let name = text(program, "name", &catalog_path)?;
        let disposition = rust_variant(disposition)?;
        let execution = rust_variant(text(program, "execution", &catalog_path)?)?;
        let builtin = program["builtin"]
            .as_str()
            .map(rust_variant)
            .transpose()?
            .map_or_else(
                || "None".to_string(),
                |variant| format!("Some(BuiltinProgram::{variant})"),
            );
        source.push_str("    super::CommonProgramEntry {\n");
        source.push_str(&format!("        name: \"{name}\",\n"));
        source.push_str(&format!(
            "        disposition: super::UtilityDisposition::{disposition},\n"
        ));
        source.push_str(&format!(
            "        execution: super::ProgramExecution::{execution},\n"
        ));
        source.push_str(&format!("        builtin: {builtin},\n"));
        source.push_str("    },\n");
    }
    source.push_str("];\n");
    source.push_str("\npub(crate) static TSO_PROGRAMS: &[(&str, TsoProgramExecution)] = &[\n");
    for program in programs {
        if let Some(action) = program["tso_action"].as_str() {
            let name = text(program, "name", &catalog_path)?;
            source.push_str(&format!(
                "    (\"{name}\", TsoProgramExecution::{}),\n",
                rust_variant(action)?
            ));
        }
    }
    source.push_str("];\n");
    source.push_str("\npub(crate) static SYSTEM_SERVICES: &[(&str, SystemServiceProgram)] = &[\n");
    for program in programs {
        if let Some(service) = program["system_service"].as_str() {
            let name = text(program, "name", &catalog_path)?;
            source.push_str(&format!(
                "    (\"{name}\", SystemServiceProgram::{}),\n",
                rust_variant(service)?
            ));
        }
    }
    source.push_str("];\n");
    Ok(source.into_bytes())
}

fn rust_variant(value: &str) -> TaskResult<String> {
    let mut output = String::new();
    for part in value.split(|character: char| !character.is_ascii_alphanumeric()) {
        if part.is_empty() {
            continue;
        }
        let mut characters = part.chars();
        let first = characters.next().ok_or("empty Rust variant component")?;
        output.push(first.to_ascii_uppercase());
        output.extend(characters.map(|character| character.to_ascii_lowercase()));
    }
    require(
        output
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_alphabetic()),
        &format!("cannot derive Rust variant from {value:?}"),
    )?;
    Ok(output)
}

fn generate_route_registries(root: &Path) -> TaskResult {
    let (official, custom, manifest) = render_route_registries(root)?;
    let generated = root.join("crates/gateways/mainframe-env-zosmf/src/generated");
    fs::create_dir_all(&generated).map_err(|error| error.to_string())?;
    for (path, bytes) in [
        (generated.join("official_routes.rs"), official),
        (generated.join("custom_routes.rs"), custom),
        (
            root.join("conformance/0.2/generated/route-registries.json"),
            pretty_json(&manifest)?,
        ),
    ] {
        fs::write(&path, bytes).map_err(|error| format!("{}: {error}", path.display()))?;
    }
    Ok(())
}

fn check_route_registries(root: &Path) -> TaskResult {
    let (official, custom, manifest) = render_route_registries(root)?;
    for (path, expected) in [
        (
            root.join("crates/gateways/mainframe-env-zosmf/src/generated/official_routes.rs"),
            official,
        ),
        (
            root.join("crates/gateways/mainframe-env-zosmf/src/generated/custom_routes.rs"),
            custom,
        ),
        (
            root.join("conformance/0.2/generated/route-registries.json"),
            pretty_json(&manifest)?,
        ),
    ] {
        require(
            fs::read(&path).map_err(|error| format!("{}: {error}", path.display()))? == expected,
            &format!(
                "{} is stale; run cargo xtask route-registries",
                path.display()
            ),
        )?;
    }
    let gateway = read(&root.join("crates/gateways/mainframe-env-zosmf/src/gateway.rs"))?;
    let registration = gateway
        .split("pub fn router")
        .nth(1)
        .and_then(|tail| tail.split("async fn info").next())
        .ok_or("z/OSMF router registration region is missing")?;
    require(
        !registration.contains(".route(")
            && registration.contains("official_routes::register")
            && registration.contains("custom_routes::register"),
        "z/OSMF router still contains handwritten route registration",
    )?;
    let contracts = json(&root.join("conformance/0.2/inventory/contracts.json"))?;
    require(
        contracts["contracts"]["official_route_registry"]
            == Value::String("mainframe-env.zosmf-official-route-bindings@1".into())
            && contracts["contracts"]["custom_route_registry"]
                == Value::String("mainframe-env.custom-route-catalog@1".into()),
        "route registry contracts are not frozen",
    )?;
    let migration_path = root.join("conformance/0.2/migrations/registry-v0-to-v1.json");
    let migration = json(&migration_path)?;
    require(
        migration["schema_version"] == Value::String("mainframe-env.registry-migration@1".into())
            && migration["destructive"] == Value::Bool(false)
            && migration["runtime_state_changed"] == Value::Bool(false)
            && migration["catalog_presence_coverage_credit"].as_u64() == Some(0)
            && migration["generation"]["deterministic"] == Value::Bool(true)
            && migration["generation"]["official_routes_equal_frozen_catalog"] == Value::Bool(true)
            && migration["generation"]["custom_routes_disjoint"] == Value::Bool(true)
            && migration["rollback"]["supported"] == Value::Bool(true)
            && migration["rollback"]["partial_registry_selected"] == Value::Bool(false),
        "program/route registry migration or rollback contract is unsafe",
    )?;
    for path in [
        "conformance/0.2/routes/official-route-bindings.json",
        "conformance/0.2/routes/custom-routes.json",
        "conformance/0.2/schemas/route-registry.schema.json",
        "conformance/0.2/schemas/generated-route-registries.schema.json",
        "docs/architecture/PROGRAM-AND-ROUTE-REGISTRIES.md",
    ] {
        require(
            root.join(path).is_file(),
            &format!("route registry artifact is missing: {path}"),
        )?;
    }
    Ok(())
}

fn render_route_registries(root: &Path) -> TaskResult<(Vec<u8>, Vec<u8>, Value)> {
    let catalog_path = root.join("conformance/0.1/inventory/zosmf-routes.json");
    let official_path = root.join("conformance/0.2/routes/official-route-bindings.json");
    let custom_path = root.join("conformance/0.2/routes/custom-routes.json");
    let catalog = json(&catalog_path)?;
    let official = json(&official_path)?;
    let custom = json(&custom_path)?;
    require(
        official["schema_version"]
            == Value::String("mainframe-env.zosmf-official-route-bindings@1".into())
            && official["namespace"] == Value::String("official-zosmf".into())
            && official["catalog"]
                == Value::String("conformance/0.1/inventory/zosmf-routes.json".into())
            && custom["schema_version"]
                == Value::String("mainframe-env.custom-route-catalog@1".into())
            && custom["namespace"] == Value::String("mainframe-env-custom".into())
            && custom["generated_coverage_credit"].as_u64() == Some(0),
        "route registry identity, namespace, or coverage policy is invalid",
    )?;
    let catalog_ids = array(&catalog, "routes", &catalog_path)?
        .iter()
        .map(|route| text(route, "id", &catalog_path).map(str::to_string))
        .collect::<TaskResult<Vec<_>>>()?;
    let official_rows = route_rows(&official, &official_path, "/zosmf/")?;
    let custom_rows = route_rows(&custom, &custom_path, "/mainframe-env/")?;
    require(
        official_rows
            .iter()
            .map(|row| row.0.clone())
            .collect::<Vec<_>>()
            == catalog_ids,
        "generated official route bindings differ from the frozen owned route catalog",
    )?;
    let official_ids = official_rows
        .iter()
        .map(|row| row.0.as_str())
        .collect::<BTreeSet<_>>();
    let custom_ids = custom_rows
        .iter()
        .map(|row| row.0.as_str())
        .collect::<BTreeSet<_>>();
    require(
        official_ids.is_disjoint(&custom_ids)
            && official_rows.len() == 23
            && custom_rows.len() == 7,
        "official and custom route namespaces overlap or have wrong denominators",
    )?;
    let gateway = read(&root.join("crates/gateways/mainframe-env-zosmf/src/gateway.rs"))?;
    for (_, _, _, handler) in official_rows.iter().chain(&custom_rows) {
        require(
            gateway.contains(&format!("async fn {handler}(")),
            &format!("route binding references missing handler {handler}"),
        )?;
    }
    let official_source =
        render_route_source("route-registries", "OFFICIAL_ROUTE_IDS", &official_rows);
    let custom_source = render_route_source("route-registries", "CUSTOM_ROUTE_IDS", &custom_rows);
    let manifest = json!({
        "schema_version":"mainframe-env.generated-route-registries@1",
        "target_version":"0.2.0",
        "official":{
            "namespace":"official-zosmf",
            "catalog":"conformance/0.1/inventory/zosmf-routes.json",
            "catalog_sha256":format!("sha256:{}", file_digest(&catalog_path)?),
            "bindings_sha256":format!("sha256:{}", file_digest(&official_path)?),
            "route_count":official_rows.len()
        },
        "custom":{
            "namespace":"mainframe-env-custom",
            "catalog":"conformance/0.2/routes/custom-routes.json",
            "catalog_sha256":format!("sha256:{}", file_digest(&custom_path)?),
            "route_count":custom_rows.len()
        },
        "namespaces_disjoint":true,
        "generated_coverage_credit":0
    });
    Ok((official_source, custom_source, manifest))
}

fn route_rows(
    catalog: &Value,
    path: &Path,
    namespace_prefix: &str,
) -> TaskResult<Vec<(String, String, String, String)>> {
    let mut seen = BTreeSet::new();
    array(catalog, "routes", path)?
        .iter()
        .map(|route| {
            let id = text(route, "id", path)?.to_string();
            let handler = text(route, "handler", path)?.to_string();
            let (method, route_path) = id
                .split_once(' ')
                .ok_or_else(|| format!("{} route {id:?} is malformed", path.display()))?;
            require(
                matches!(method, "GET" | "POST" | "PUT" | "DELETE")
                    && route_path.starts_with(namespace_prefix)
                    && !handler.is_empty()
                    && handler.bytes().all(|byte| {
                        byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_'
                    })
                    && seen.insert(id.clone()),
                &format!("{} route {id:?} is invalid or duplicated", path.display()),
            )?;
            let method = method.to_ascii_lowercase();
            let route_path = route_path.to_string();
            Ok((id, method, route_path, handler))
        })
        .collect()
}

fn render_route_source(
    generator: &str,
    ids_name: &str,
    rows: &[(String, String, String, String)],
) -> Vec<u8> {
    let mut groups: Vec<(String, Vec<(&str, &str)>)> = Vec::new();
    for (_, method, path, handler) in rows {
        if let Some((_, methods)) = groups.iter_mut().find(|(known, _)| known == path) {
            methods.push((method, handler));
        } else {
            groups.push((path.clone(), vec![(method, handler)]));
        }
    }
    let mut source = format!(
        "// @generated by `cargo xtask {generator}`; do not edit.\n\nuse axum::Router;\nuse axum::routing::{{get, post, put}};\n\npub(super) const {ids_name}: &[&str] = &[\n"
    );
    for (id, _, _, _) in rows {
        source.push_str(&format!("    \"{id}\",\n"));
    }
    source.push_str(
        "];

#[rustfmt::skip]
pub(super) fn register(router: Router<super::GatewayState>) -> Router<super::GatewayState> {
    router",
    );
    for (path, methods) in groups {
        let (first_method, first_handler) = methods[0];
        source.push_str(&format!(
            "\n        .route(\"{path}\", {first_method}(super::{first_handler})"
        ));
        for (method, handler) in methods.into_iter().skip(1) {
            source.push_str(&format!(".{method}(super::{handler})"));
        }
        source.push(')');
    }
    source.push_str("\n}\n");
    source.into_bytes()
}

fn check_dehardcoding(root: &Path) -> TaskResult {
    let mut rust_files = Vec::new();
    collect_extension(&root.join("crates"), OsStr::new("rs"), &mut rust_files)?;
    rust_files.sort();
    let conformance = root.join("crates/tooling/mainframe-env-conformance");
    let forbidden_application_identities = [
        "CARDDEMO",
        "COBTUPDT",
        "CBPAUP0C",
        "PAUDBLOD",
        "PAUDBUNL",
        "DBPAUTP0",
        "PSBPAUTB",
        "PAUTSUM0",
        "PAUTDTL1",
        "AUTHFRDS",
        "TRANSACTION_TYPE",
        "TRANSACTION_TYPE_CATEGORY",
    ];
    let mut hits = Vec::new();
    for rust_file in rust_files {
        if rust_file.starts_with(&conformance) {
            continue;
        }
        let source = read(&rust_file)?;
        let production = source.split("#[cfg(test)]").next().unwrap_or(&source);
        let upper = production.to_ascii_uppercase();
        for identity in forbidden_application_identities {
            if upper.contains(identity) {
                hits.push(format!(
                    "{}:{identity}",
                    rust_file.strip_prefix(root).unwrap_or(&rust_file).display()
                ));
            }
        }
    }
    require(
        hits.is_empty(),
        &format!("production application hardcode scan found {hits:?}"),
    )?;
    let program = read(&root.join("crates/apps/mainframe-env-batch/src/program.rs"))?;
    let batch = read(&root.join("crates/apps/mainframe-env-batch/src/service.rs"))?;
    let server = read(&root.join("crates/apps/mainframe-env-server/src/cobol.rs"))?;
    for (scope, source, forbidden) in [
        (
            "common program registry",
            program.as_str(),
            vec![
                "match program.to_ascii_uppercase().as_str()",
                "for name in [",
                "match self.0 {",
            ],
        ),
        (
            "batch execution",
            batch.split("#[cfg(test)]").next().unwrap_or(&batch),
            vec![
                "match program.as_str()",
                "step.program.eq_ignore_ascii_case(\"",
            ],
        ),
        (
            "installed system services",
            server.split("#[cfg(test)]").next().unwrap_or(&server),
            vec!["program.eq_ignore_ascii_case(\""],
        ),
    ] {
        for pattern in forbidden {
            require(
                !source.contains(pattern),
                &format!("{scope} retains handwritten string dispatch {pattern}"),
            )?;
        }
    }
    check_program_registry(root)?;
    check_route_registries(root)?;
    let accepted_scanned_files = rust_file_count_at_commit(
        root,
        &accepted_0_2_completion(root)?,
        "crates/tooling/mainframe-env-conformance/",
    )?;
    let hardcode_path = root.join("conformance/0.2/evidence/hardcode/no-application-hardcode.json");
    let hardcode = json(&hardcode_path)?;
    require(
        hardcode["schema_version"]
            == Value::String("mainframe-env.no-application-hardcode@1".into())
            && hardcode["before"]["production_h1_h3_lines"].as_u64() == Some(32)
            && hardcode["after"]["production_h1_h3_lines"].as_u64() == Some(0)
            && hardcode["after"]["scanned_rust_files"].as_u64()
                == Some(accepted_scanned_files as u64)
            && hardcode["after"]["application_string_dispatch"].as_u64() == Some(0),
        "no-application-hardcode receipt is stale or incomplete",
    )?;
    let dispatch_path = root.join("conformance/0.2/evidence/hardcode/no-string-dispatch.json");
    let dispatch = json(&dispatch_path)?;
    require(
        dispatch["schema_version"] == Value::String("mainframe-env.no-string-dispatch@1".into())
            && dispatch["before"]["handwritten_program_dispatch_sites"].as_u64() == Some(12)
            && dispatch["after"]["handwritten_program_dispatch_sites"].as_u64() == Some(0)
            && dispatch["before"]["handwritten_route_registration_sites"].as_u64() == Some(22)
            && dispatch["after"]["handwritten_route_registration_sites"].as_u64() == Some(0)
            && dispatch["after"]["official_generated_routes"].as_u64() == Some(23)
            && dispatch["after"]["custom_generated_routes"].as_u64() == Some(7)
            && dispatch["after"]["application_program_table_transaction_exceptions"].as_u64()
                == Some(0),
        "no-string-dispatch receipt is stale or incomplete",
    )
}

fn rust_file_count_at_commit(
    root: &Path,
    commit: &str,
    excluded_prefix: &str,
) -> TaskResult<usize> {
    let listing = command_text(root, "git", &["ls-tree", "-r", "--name-only", commit])?;
    Ok(listing
        .lines()
        .filter(|path| {
            path.starts_with("crates/")
                && path.ends_with(".rs")
                && !path.starts_with(excluded_prefix)
        })
        .count())
}

fn check_coverage(root: &Path) -> TaskResult {
    let index_path = root.join("conformance/0.2/catalogs/index.json");
    let index = json(&index_path)?;
    require(
        text(&index, "schema_version", &index_path)? == "mainframe-env.official-source-receipts@1",
        "official source receipt schema changed",
    )?;
    require(
        text(&index, "target_version", &index_path)? == "0.2.0",
        "official source receipts target the wrong product version",
    )?;
    require(
        text(&index, "catalog_contract", &index_path)? == "mainframe-env.official-catalog@1",
        "official catalog contract changed",
    )?;
    let gates = array(&index, "coverage_gates", &index_path)?;
    let expected_gates = [
        "recognized",
        "validated",
        "executed",
        "conditioned",
        "recovered",
        "differential",
    ];
    require(
        gates.len() == expected_gates.len()
            && gates
                .iter()
                .zip(expected_gates)
                .all(|(actual, expected)| actual.as_str() == Some(expected)),
        "coverage gates are not the six ordered independent gates",
    )?;
    for policy in [
        "catalog_presence_counts_as_execution",
        "generated_identity_counts_as_execution",
        "partial_rows_round_up",
    ] {
        require(
            index["claim_policy"][policy] == Value::Bool(false),
            &format!("coverage claim policy {policy} must be false"),
        )?;
    }
    require(
        index["claim_policy"]["licensed_ibm_oracle_required_for_differential_pass"]
            == Value::Bool(true),
        "licensed IBM oracle policy must remain required",
    )?;
    require(
        index["review"]["status"] == Value::String("reviewed".into())
            && index["review"]["publication_bytes_redistributed"] == Value::Bool(false),
        "official source review receipt is incomplete",
    )?;

    let expected = BTreeMap::from([
        ("ibm-cics-ts-6x-2026-08-31", "cics"),
        ("ibm-db2-for-zos-13-2026-08-13", "db2"),
        ("ibm-enterprise-cobol-6.5-2026-05-31", "cobol"),
        ("ibm-ims-15.6-dli-2026-08-31", "ims"),
        ("ibm-mq-9.4-mqi-2026-08-31", "mq"),
        ("ibm-zos-3.2-dfsms-ams-2026-06", "dataset-vsam-ams"),
        ("ibm-zos-3.2-jcl-jes2-2026-06", "jcl-jes2"),
        ("ibm-zos-3.2-racf-saf-2026", "racf-saf"),
        ("ibm-zosmf-3.2-2026-07-27", "zosmf"),
    ]);
    let baselines = array(&index, "baselines", &index_path)?;
    require(
        baselines.len() == expected.len()
            && index["baseline_count"].as_u64() == Some(expected.len() as u64),
        "official source receipt index must contain exactly nine baselines",
    )?;
    let mut baseline_ids = BTreeSet::new();
    let mut subsystem_ids = BTreeSet::new();
    let mut row_ids = BTreeSet::new();
    let mut total_rows = 0_u64;
    for baseline in baselines {
        let id = text(baseline, "id", &index_path)?;
        let subsystem = text(baseline, "subsystem", &index_path)?;
        require(
            expected.get(id).copied() == Some(subsystem),
            &format!("unexpected official baseline {id}/{subsystem}"),
        )?;
        require(
            baseline_ids.insert(id) && subsystem_ids.insert(subsystem),
            &format!("duplicate official baseline or subsystem {id}/{subsystem}"),
        )?;
        for field in ["product", "version", "publication_identity"] {
            require(
                !text(baseline, field, &index_path)?.trim().is_empty(),
                &format!("baseline {id} has no {field}"),
            )?;
        }
        validate_official_source(&baseline["source"], &index_path, id)?;
        if let Some(sources) = baseline.get("supporting_sources") {
            for source in sources.as_array().ok_or_else(|| {
                format!(
                    "{} baseline {id} supporting_sources is not an array",
                    index_path.display()
                )
            })? {
                validate_official_source(source, &index_path, id)?;
            }
        }

        let catalog_name = text(baseline, "catalog", &index_path)?;
        require(
            catalog_name.starts_with("conformance/0.2/catalogs/")
                && catalog_name.ends_with(".json")
                && !catalog_name.contains(".."),
            &format!("baseline {id} has an unsafe catalog path"),
        )?;
        let catalog_path = root.join(catalog_name);
        let expected_digest = text(baseline, "catalog_sha256", &index_path)?;
        validate_sha256_identity(expected_digest, &format!("baseline {id} catalog digest"))?;
        require(
            expected_digest == format!("sha256:{}", file_digest(&catalog_path)?),
            &format!("baseline {id} catalog digest drifted"),
        )?;
        let catalog = json(&catalog_path)?;
        require(
            text(&catalog, "schema_version", &catalog_path)? == "mainframe-env.official-catalog@1"
                && text(&catalog, "baseline_id", &catalog_path)? == id
                && text(&catalog, "subsystem", &catalog_path)? == subsystem,
            &format!("baseline {id} catalog identity differs from its receipt"),
        )?;
        let denominators = baseline["immutable_denominators"]
            .as_object()
            .ok_or_else(|| format!("baseline {id} immutable_denominators is not an object"))?;
        let mut unit_ids = BTreeSet::new();
        let mut baseline_rows = 0_u64;
        let units = array(&catalog, "units", &catalog_path)?;
        require(
            units.len() == denominators.len(),
            &format!("baseline {id} unit count differs from immutable receipt"),
        )?;
        for unit in units {
            let unit_id = text(unit, "id", &catalog_path)?;
            require(
                unit_ids.insert(unit_id),
                &format!("baseline {id} repeats unit {unit_id}"),
            )?;
            require(
                !text(unit, "normalization", &catalog_path)?.is_empty(),
                &format!("baseline {id}/{unit_id} has no normalization state"),
            )?;
            let denominator = unit["denominator"]
                .as_u64()
                .ok_or_else(|| format!("baseline {id}/{unit_id} denominator is invalid"))?;
            let immutable = denominators
                .get(unit_id)
                .and_then(Value::as_u64)
                .ok_or_else(|| format!("baseline {id}/{unit_id} has no immutable denominator"))?;
            let rows = array(unit, "rows", &catalog_path)?;
            require(
                denominator == immutable && rows.len() as u64 == denominator,
                &format!("baseline {id}/{unit_id} denominator does not equal catalog rows"),
            )?;
            for row in rows {
                let row_id = text(row, "id", &catalog_path)?;
                require(
                    row_id.starts_with(&format!("{id}:{unit_id}:"))
                        && row_ids.insert(row_id.to_string()),
                    &format!("invalid or duplicate official row {row_id}"),
                )?;
                require(
                    row["mandatory"] == Value::Bool(true)
                        && !text(row, "label", &catalog_path)?.is_empty()
                        && !text(row, "source_locator", &catalog_path)?.is_empty(),
                    &format!("official row {row_id} is incomplete or optional"),
                )?;
            }
            baseline_rows += denominator;
        }
        require(
            catalog["mandatory_rows"].as_u64() == Some(baseline_rows)
                && baseline["mandatory_rows"].as_u64() == Some(baseline_rows),
            &format!("baseline {id} total denominator drifted"),
        )?;
        total_rows += baseline_rows;
    }
    require(
        index["mandatory_rows"].as_u64() == Some(total_rows) && total_rows == 1_506,
        "official catalog global denominator must remain 1506",
    )?;
    check_coverage_ledger(root, &index)?;
    topic_manifests::check(root)?;
    check_publication_bytes(root)?;
    check_coverage_work_package_evidence(root)?;
    check_coverage_program_status(root)?;
    check_workload_ledger_consistency(root)
}

/// The two markers IBM's content endpoint stamps into every body it serves.
///
/// They are not chosen for being distinctive strings; they are the two fields
/// this project's own reader takes out of a topic. `HEADING` and `LAST_MODIFIED`
/// in `conformance/tools/docs_api.py` match exactly these; the nine manifests
/// under `conformance/0.2/manifests/` record the `last_modified` the second
/// yields for all 4,488 pinned topics and
/// `conformance/0.7/generated/jcl-topic-manifest.json` records the `heading` the
/// first yields for its 626; and both markers appear in every one of the 6,598
/// bodies cached across the nine baselines -- 100%, where `role="article"`
/// misses three. A file the readers could read as a topic is a file this
/// repository must not hold.
const SERVED_TOPIC_MARKERS: [&str; 2] = ["topictitle1", "id=\"lastModifiedDate\""];

/// No IBM publication bytes anywhere in the repository, by content and not by name.
///
/// The guard used to walk `conformance/` only, on the reasoning that bytes
/// arrive where the fetchers are pointed and the fetchers live there. They
/// arrive wherever they are pointed: `fetch_jcl_topics.py --destination
/// docs/generated/leak` wrote 626 topic bodies into `docs/` with this gate, and
/// eight others, still green. A backstop that covers one subdirectory is not a
/// backstop, so this walks the whole tree.
///
/// Widening it means the extension rule has to go, because it was only ever
/// tenable while nothing legitimate could carry those extensions. Something can:
/// `conformance/experiments/docs-semantics/fixtures/cics-pilot.html` on
/// `codex/docs-semantic-experiment` is 2,294 bytes of markup this project wrote
/// to exercise a reader, and refusing it would teach the next author to rename
/// their fixture rather than to keep publications out. Extensions are the wrong
/// evidence in both directions anyway -- `cp topic.htm docs/notes.md` renames a
/// publication out of reach of a rule that reads names.
///
/// So the rule reads the bytes. A file is refused when it is a PDF -- `%PDF-`
/// magic, whatever it is called, and this repository reads no PDFs at all -- or
/// when it is markup carrying both markers IBM stamps into a served body.
/// "Markup" is the file's first non-whitespace byte being `<`, which is the
/// clause that makes the check safe to apply to source: `docs_api.py` and
/// `conformance/tools/tests/test_locator_tools.py` both quote both markers,
/// because one defines them and the other tests them, and neither is markup.
/// A body renamed, re-extensioned or dropped in a new directory still opens with
/// its `<div><article>` and still carries its own heading and `Last Updated`
/// stamp, so it is still caught; defeating this needs editing IBM's bytes, which
/// is no longer keeping a publication.
fn check_publication_bytes(root: &Path) -> TaskResult {
    let mut files = Vec::new();
    collect_files(root, &mut files)?;
    for path in files {
        let data = fs::read(&path).map_err(|error| format!("{}: {error}", path.display()))?;
        if let Some(reason) = publication_body(&data) {
            return Err(format!(
                "publication bytes must not be checked into the repository: {} is {reason}",
                path.strip_prefix(root).unwrap_or(&path).display()
            ));
        }
    }
    Ok(())
}

/// Why these bytes are a publication, or `None` if this project could have written them.
fn publication_body(data: &[u8]) -> Option<&'static str> {
    if data.starts_with(b"%PDF-") {
        return Some("a PDF document");
    }
    let body = data.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(data);
    if body.iter().find(|byte| !byte.is_ascii_whitespace()) != Some(&b'<') {
        return None;
    }
    let text = String::from_utf8_lossy(body);
    SERVED_TOPIC_MARKERS
        .iter()
        .all(|marker| text.contains(marker))
        .then_some("a topic body served by IBM Documentation")
}

fn check_coverage_ledger(root: &Path, index: &Value) -> TaskResult {
    let path = root.join("conformance/0.2/evidence/coverage-ledger.json");
    let ledger = json(&path)?;
    require(
        text(&ledger, "schema_version", &path)? == "mainframe-env.coverage-ledger@1"
            && ledger["generation"].as_u64().is_some_and(|value| value > 0)
            && ledger["catalog_index"]
                == Value::String("conformance/0.2/catalogs/index.json".into()),
        "coverage ledger identity is invalid",
    )?;
    let index_path = root.join("conformance/0.2/catalogs/index.json");
    require(
        ledger["catalog_index_sha256"]
            == Value::String(format!("sha256:{}", file_digest(&index_path)?)),
        "coverage ledger catalog index digest drifted",
    )?;
    require(
        ledger["generated_catalog_credit"].as_u64() == Some(0)
            && ledger["official_compatibility_numerator"].as_u64() == Some(0)
            && ledger["evidence_records"]
                .as_array()
                .is_some_and(Vec::is_empty),
        "catalog generation or empty evidence credited official compatibility",
    )?;
    let expected = array(index, "baselines", &index_path)?
        .iter()
        .map(|baseline| {
            Ok((
                text(baseline, "id", &index_path)?.to_string(),
                (
                    text(baseline, "catalog_sha256", &index_path)?.to_string(),
                    baseline["mandatory_rows"]
                        .as_u64()
                        .ok_or("baseline mandatory row count is invalid")?,
                ),
            ))
        })
        .collect::<TaskResult<BTreeMap<_, _>>>()?;
    let rows = array(&ledger, "baselines", &path)?;
    require(
        rows.len() == expected.len(),
        "coverage ledger does not contain every official baseline",
    )?;
    let mut seen = BTreeSet::new();
    for row in rows {
        let id = text(row, "id", &path)?;
        let (catalog_digest, denominator) = expected
            .get(id)
            .ok_or_else(|| format!("coverage ledger contains unknown baseline {id}"))?;
        require(
            seen.insert(id)
                && row["catalog_sha256"].as_str() == Some(catalog_digest)
                && row["mandatory_rows"].as_u64() == Some(*denominator)
                && row["complete_rows"].as_u64() == Some(0),
            &format!("coverage ledger baseline {id} identity or denominator drifted"),
        )?;
        let gates = row["gates"]
            .as_object()
            .ok_or_else(|| format!("coverage ledger baseline {id} gates are missing"))?;
        let gate_names = [
            "recognized",
            "validated",
            "executed",
            "conditioned",
            "recovered",
            "differential",
        ];
        require(
            gates.len() == gate_names.len(),
            &format!("coverage ledger baseline {id} does not contain six gates"),
        )?;
        for gate in gate_names {
            require(
                gates[gate]["numerator"].as_u64() == Some(0)
                    && gates[gate]["denominator"].as_u64() == Some(*denominator),
                &format!("coverage ledger baseline {id}/{gate} credited generated coverage"),
            )?;
        }
    }

    let contracts_path = root.join("conformance/0.2/inventory/contracts.json");
    let contracts = json(&contracts_path)?;
    for contract in [
        "mainframe-env.official-source-receipts@1",
        "mainframe-env.official-catalog@1",
        "mainframe-env.coverage-row@1",
        "mainframe-env.coverage-evidence@1",
        "mainframe-env.coverage-ledger@1",
    ] {
        require(
            contracts["contracts"].as_object().is_some_and(|values| {
                values
                    .values()
                    .any(|value| value.as_str() == Some(contract))
            }),
            &format!("coverage contract inventory omits {contract}"),
        )?;
    }
    let additions_path = root.join("conformance/0.2/inventory/package-additions.json");
    let additions = json(&additions_path)?;
    require(
        array(&additions, "packages", &additions_path)?
            .iter()
            .any(|package| {
                package["name"] == Value::String("mainframe-env-coverage".into())
                    && package["work_package"] == Value::String("CV-202".into())
            })
            && additions["production_profile_membership"] == Value::Bool(false),
        "coverage contract package inventory is invalid",
    )?;
    let metadata = workspace_metadata(root)?;
    require(
        metadata["packages"].as_array().is_some_and(|packages| {
            packages
                .iter()
                .any(|package| package["name"] == "mainframe-env-coverage")
        }),
        "coverage contract package is absent from the workspace",
    )
}

fn check_coverage_work_package_evidence(root: &Path) -> TaskResult {
    let directory = root.join("conformance/0.2/evidence/work-packages");
    let mut files = Vec::new();
    collect_extension(&directory, OsStr::new("json"), &mut files)?;
    require(!files.is_empty(), "0.2 has no work-package evidence")?;
    files.sort();
    for path in files {
        let evidence = json(&path)?;
        let work_package = text(&evidence, "work_package", &path)?;
        require(
            text(&evidence, "schema_version", &path)?
                == "mainframe-env.coverage-work-package-evidence@1"
                && evidence["derived"] == Value::Bool(true)
                && evidence["status"] == Value::String("pass".into())
                && path.file_stem().and_then(OsStr::to_str) == Some(work_package),
            &format!("{} is not a derived work-package pass", path.display()),
        )?;
        let receipt = evidence
            .get("receipt")
            .and_then(Value::as_object)
            .ok_or_else(|| format!("{} has no receipt object", path.display()))?;
        let digest = canonical_evidence_digest(receipt)?;
        require(
            evidence["evidence_digest"].as_str() == Some(digest.as_str()),
            &format!("{} evidence digest is stale", path.display()),
        )?;
        let receipt = Value::Object(receipt.clone());
        let subject = work_package_completion_subject(work_package)?;
        let package_trailer = format!("{work_package}=pass");
        let completion = find_completion_commit(
            root,
            subject,
            &[
                ("Work-Package", package_trailer.as_str()),
                ("Target-Version", "0.2.0"),
                ("Evidence-Digest", digest.as_str()),
            ],
        )?;
        let parent = command_text(root, "git", &["rev-parse", &format!("{completion}^")])?;
        verify_completion_parent(root, &completion, &parent)?;
        require(
            receipt["target_version"] == Value::String("0.2.0".into())
                && receipt["source_identity"].as_str() == Some(parent.as_str())
                && receipt["commands"]
                    .as_array()
                    .is_some_and(|commands| !commands.is_empty())
                && receipt["invariants"].is_object(),
            &format!("{} receipt is incomplete", path.display()),
        )?;
        let relative_evidence = path
            .strip_prefix(root)
            .map_err(|error| error.to_string())?
            .to_string_lossy();
        verify_commit_bound_live_file(root, &completion, &relative_evidence, &path)?;
        for artifact in receipt["artifacts"]
            .as_array()
            .ok_or_else(|| format!("{} receipt artifacts are missing", path.display()))?
        {
            let artifact_path = text(artifact, "path", &path)?;
            require(
                !artifact_path.contains("..") && !Path::new(artifact_path).is_absolute(),
                &format!("{} contains an unsafe artifact path", path.display()),
            )?;
            let expected = text(artifact, "sha256", &path)?;
            validate_sha256_identity(expected, "work-package artifact digest")?;
            require(
                expected
                    == format!(
                        "sha256:{}",
                        git_file_digest(root, &completion, artifact_path)?
                    ),
                &format!(
                    "{} historical artifact {artifact_path} drifted",
                    path.display()
                ),
            )?;
        }
    }
    check_work_package_amendments(root)?;
    Ok(())
}

fn check_work_package_amendments(root: &Path) -> TaskResult {
    let path = root.join("conformance/0.2/evidence/work-package-amendments.json");
    let evidence = json(&path)?;
    let repair_completion = accepted_0_2_completion(root)?;
    require(
        evidence["schema_version"]
            == Value::String("mainframe-env.work-package-amendments@1".into())
            && evidence["target_version"] == Value::String("0.2.0".into())
            && evidence["status"] == Value::String("pass".into())
            && evidence["historical_receipts_rewritten"] == Value::Bool(false),
        "work-package amendment evidence header is invalid",
    )?;
    let amendments = array(&evidence, "amendments", &path)?;
    require(
        amendments.len() == 4
            && amendments
                .iter()
                .map(|row| row["work_package"].as_str())
                .eq([
                    Some("CV-204"),
                    Some("CV-205"),
                    Some("CV-206"),
                    Some("CV-209"),
                ]),
        "work-package amendments must cover CV-204/CV-205/CV-206/CV-209 exactly",
    )?;
    for amendment in amendments {
        let work_package = text(amendment, "work_package", &path)?;
        let historical = amendment
            .get("historical")
            .and_then(Value::as_object)
            .ok_or_else(|| format!("{work_package} historical identity is missing"))?;
        let historical = Value::Object(historical.clone());
        let completion = text(&historical, "completion_commit", &path)?;
        let receipt_path = text(&historical, "receipt_path", &path)?;
        let receipt_digest = text(&historical, "evidence_digest", &path)?;
        validate_sha256_identity(receipt_digest, "historical work-package evidence digest")?;
        let live_receipt = json(&root.join(receipt_path))?;
        require(
            live_receipt["evidence_digest"].as_str() == Some(receipt_digest),
            &format!("{work_package} historical evidence digest drifted"),
        )?;
        verify_commit_bound_live_file(root, completion, receipt_path, &root.join(receipt_path))?;
        for artifact in amendment["repair_artifacts"]
            .as_array()
            .ok_or_else(|| format!("{work_package} repair artifacts are missing"))?
        {
            let relative = text(artifact, "path", &path)?;
            let expected = text(artifact, "sha256", &path)?;
            require(
                !relative.contains("..")
                    && !Path::new(relative).is_absolute()
                    && expected
                        == format!(
                            "sha256:{}",
                            git_file_digest(root, &repair_completion, relative)?
                        ),
                &format!("{work_package} repair artifact drifted: {relative}"),
            )?;
        }
    }
    Ok(())
}

fn work_package_completion_subject(work_package: &str) -> TaskResult<&'static str> {
    match work_package {
        "CV-201" => Ok("Complete CV-201 official coverage catalogs"),
        "CV-202" => Ok("Complete CV-202 coverage evidence contracts"),
        "CV-203" => Ok("Complete CV-203 generated semantic dispatch"),
        "CV-204" => Ok("Complete CV-204 application package generations"),
        "CV-205" => Ok("Complete CV-205 generic Db2 package catalog"),
        "CV-206" => Ok("Complete CV-206 installed batch controllers"),
        "CV-207" => Ok("Complete CV-207 subsystem ABI libraries"),
        "CV-208" => Ok("Complete CV-208 generated dispatch registries"),
        "CV-209" => Ok("Complete CV-209 evidence ledger closure"),
        _ => Err(format!(
            "unknown work-package completion subject: {work_package}"
        )),
    }
}

fn check_coverage_program_status(root: &Path) -> TaskResult {
    let path = root.join("conformance/0.2/evidence/program-status.json");
    let status = json(&path)?;
    require(
        text(&status, "schema_version", &path)? == "mainframe-env.coverage-program-status@1"
            && text(&status, "target_version", &path)? == "0.2.0",
        "0.2 program status identity is invalid",
    )?;
    let source = status["source_identity"]
        .as_object()
        .ok_or("0.2 program status source identity is not an object")?;
    let expected_source = [
        (
            "accepted_release_commit",
            "44f3081eb2fdf22d09e1a97725f5a4163431ca70",
        ),
        (
            "accepted_release_tree",
            "3d504ece02f1c09e124606ded695b00ba984d104",
        ),
        (
            "accepted_carddemo_candidate_commit",
            "857115b907ce7098c965a51117a079048ea8182e",
        ),
        (
            "accepted_carddemo_candidate_tree",
            "7e1fa73f4c808a89ddb4f98c0e3f7d0207f861ea",
        ),
        (
            "implementation_start_commit",
            "1afbf402c56c085a46cc3599c91b1b77002b1e16",
        ),
    ];
    for (field, expected) in expected_source {
        require(
            source.get(field).and_then(Value::as_str) == Some(expected),
            &format!("0.2 program status {field} drifted"),
        )?;
    }
    require(
        command_text(root, "git", &["rev-parse", "mainframe-env-v0.1.1^{}"])?
            == source["accepted_release_commit"],
        "accepted 0.1.1 release tag moved",
    )?;
    require(
        command_text(root, "git", &["rev-parse", "mainframe-env-v0.1.1^{tree}"])?
            == source["accepted_release_tree"],
        "accepted 0.1.1 release tree moved",
    )?;
    let work_packages = array(&status, "work_packages", &path)?;
    require(
        work_packages.len() == 9,
        "0.2 program status must track nine work packages",
    )?;
    for (index, work_package) in work_packages.iter().enumerate() {
        let expected = format!("CV-{:03}", index + 201);
        let state = text(work_package, "state", &path)?;
        require(
            text(work_package, "id", &path)? == expected
                && matches!(state, "pending" | "in-progress" | "pass" | "blocked"),
            &format!("0.2 program status work package {expected} is invalid"),
        )?;
        if state == "pass" {
            let evidence = work_package["evidence"]
                .as_str()
                .ok_or_else(|| format!("0.2 program status {expected} pass has no evidence"))?;
            require(
                evidence == format!("conformance/0.2/evidence/work-packages/{expected}.json")
                    && root.join(evidence).is_file(),
                &format!("0.2 program status {expected} evidence is invalid"),
            )?;
        }
    }
    require(
        !array(&status, "commands", &path)?.is_empty()
            && status["dependency_receipts"]
                .as_array()
                .is_some_and(|receipts| !receipts.is_empty())
            && status["blockers"].is_array()
            && status["open_decisions"].is_array()
            && status["next_smallest_executable_step"]
                .as_str()
                .is_some_and(|step| !step.is_empty()),
        "0.2 program status is not resumable",
    )?;
    let recorded_digest = status["dirty_tree_identity"]["digest"]
        .as_str()
        .ok_or("0.2 program status dirty-tree digest is missing")?;
    require(
        recorded_digest == accepted_0_2_candidate_digest(root)?,
        "0.2 program status accepted-candidate digest drifted",
    )?;
    require(
        root.join("docs/delivery/coverage-versions/status/0.2.0.md")
            .is_file()
            && root
                .join("conformance/0.2/schemas/program-status.schema.json")
                .is_file(),
        "0.2 program status documentation or schema is missing",
    )
}

fn check_workload_ledger_consistency(root: &Path) -> TaskResult {
    let workload_path = root.join("conformance/0.2/evidence/workload-ledger.json");
    let status_path = root.join("conformance/0.2/evidence/program-status.json");
    let workload = json(&workload_path)?;
    let status = json(&status_path)?;
    compare_workload_and_program_status(&workload, &status)?;
    let work_packages = array(&workload, "work_packages", &workload_path)?;
    require(
        work_packages.len() == 9,
        "workload ledger must contain exactly CV-201 through CV-209",
    )?;
    for (index, work_package) in work_packages.iter().enumerate() {
        let expected_id = format!("CV-{:03}", index + 201);
        let id = text(work_package, "id", &workload_path)?;
        let state = text(work_package, "state", &workload_path)?;
        require(
            id == expected_id,
            &format!("workload ledger work package {expected_id} is missing or out of order"),
        )?;
        if state == "pass" {
            let evidence_path = text(work_package, "evidence", &workload_path)?;
            let evidence = json(&root.join(evidence_path))?;
            require(
                evidence["work_package"].as_str() == Some(id)
                    && work_package["evidence_digest"] == evidence["evidence_digest"],
                &format!("workload ledger {id} evidence identity is stale"),
            )?;
        } else {
            require(
                work_package["evidence"].is_null() && work_package["evidence_digest"].is_null(),
                &format!("incomplete workload ledger row {id} claims evidence"),
            )?;
        }
    }
    let exit_gates = array(&workload, "exit_gates", &workload_path)?;
    let expected_exit_gates = BTreeSet::from([
        "official-catalogs",
        "coverage-contracts",
        "dehardcoding",
        "migration-rollback",
        "workspace-test-floor",
        "postgresql-controls",
        "carddemo-full",
        "live-zowe-route",
        "release-checks",
        "evidence-sealing",
        "content-evidence-sealing",
    ]);
    let actual_exit_gates = exit_gates
        .iter()
        .map(|gate| text(gate, "id", &workload_path))
        .collect::<TaskResult<BTreeSet<_>>>()?;
    require(
        actual_exit_gates == expected_exit_gates && actual_exit_gates.len() == exit_gates.len(),
        "workload ledger exit gates are missing or duplicated",
    )?;
    for gate in exit_gates {
        let state = text(gate, "state", &workload_path)?;
        require(
            matches!(state, "pending" | "pass" | "blocked")
                && (state == "pass") == gate["evidence"].is_string(),
            &format!(
                "workload ledger exit gate {} state/evidence contradict",
                text(gate, "id", &workload_path)?
            ),
        )?;
        if let Some(evidence) = gate["evidence"].as_str() {
            require(
                root.join(evidence).is_file(),
                &format!("workload exit-gate evidence is missing: {evidence}"),
            )?;
        }
    }
    let coverage_path = root.join("conformance/0.2/evidence/coverage-ledger.json");
    let coverage = json(&coverage_path)?;
    let mandatory_rows = array(&coverage, "baselines", &coverage_path)?
        .iter()
        .map(|baseline| baseline["mandatory_rows"].as_u64().unwrap_or_default())
        .sum::<u64>();
    let complete_rows = array(&coverage, "baselines", &coverage_path)?
        .iter()
        .map(|baseline| baseline["complete_rows"].as_u64().unwrap_or_default())
        .sum::<u64>();
    require(
        workload["coverage"]["official_baselines"].as_u64() == Some(9)
            && workload["coverage"]["mandatory_rows"].as_u64() == Some(mandatory_rows)
            && workload["coverage"]["complete_rows"].as_u64() == Some(complete_rows)
            && workload["coverage"]["official_compatibility_numerator"]
                == coverage["official_compatibility_numerator"]
            && workload["coverage"]["generated_catalog_credit"]
                == coverage["generated_catalog_credit"],
        "workload and coverage ledgers contradict",
    )?;
    let markdown = read(&root.join("docs/delivery/coverage-versions/status/0.2.0.md"))?;
    for work_package in work_packages {
        let id = text(work_package, "id", &workload_path)?;
        let state = text(work_package, "state", &workload_path)?;
        let evidence = work_package["evidence"]
            .as_str()
            .map_or_else(|| "pending".to_string(), |path| format!("`{path}`"));
        let markdown_state = state.replace('-', " ");
        require(
            markdown.contains(&format!("| {id} | {markdown_state} | {evidence} |")),
            &format!("Markdown workload ledger contradicts {id}"),
        )?;
    }
    let contracts = json(&root.join("conformance/0.2/inventory/contracts.json"))?;
    require(
        contracts["contracts"]["workload_ledger"]
            == Value::String("mainframe-env.coverage-workload-ledger@1".into())
            && root
                .join("conformance/0.2/schemas/workload-ledger.schema.json")
                .is_file(),
        "workload ledger contract or schema is missing",
    )
}

fn compare_workload_and_program_status(workload: &Value, status: &Value) -> TaskResult {
    require(
        workload["schema_version"]
            == Value::String("mainframe-env.coverage-workload-ledger@1".into())
            && status["schema_version"]
                == Value::String("mainframe-env.coverage-program-status@1".into())
            && workload["target_version"] == status["target_version"]
            && workload["implementation_status"] == status["implementation_status"]
            && workload["current_work_package"] == status["current_work_package"],
        "workload and program-status ledger headers contradict",
    )?;
    let workload_rows = workload["work_packages"]
        .as_array()
        .ok_or("workload ledger rows are missing")?;
    let status_rows = status["work_packages"]
        .as_array()
        .ok_or("program-status ledger rows are missing")?;
    require(
        workload_rows.len() == status_rows.len(),
        "workload and program-status ledger lengths contradict",
    )?;
    for (workload_row, status_row) in workload_rows.iter().zip(status_rows) {
        require(
            workload_row["id"] == status_row["id"]
                && workload_row["state"] == status_row["state"]
                && workload_row["evidence"] == status_row["evidence"],
            "workload and program-status work-package rows contradict",
        )?;
    }
    let implementation = workload["implementation_status"]
        .as_str()
        .ok_or("workload implementation status is invalid")?;
    let in_progress = workload_rows
        .iter()
        .filter(|row| row["state"] == "in-progress")
        .collect::<Vec<_>>();
    match implementation {
        "complete" => require(
            workload["current_work_package"].is_null()
                && workload_rows.iter().all(|row| row["state"] == "pass")
                && workload["exit_gates"]
                    .as_array()
                    .is_some_and(|gates| gates.iter().all(|gate| gate["state"] == "pass")),
            "complete workload ledger contains incomplete work or exit gates",
        ),
        "in-progress" => require(
            in_progress.len() == 1 && in_progress[0]["id"] == workload["current_work_package"],
            "in-progress workload ledger does not identify exactly one current package",
        ),
        "blocked" => require(
            workload_rows.iter().any(|row| row["state"] == "blocked"),
            "blocked workload ledger contains no blocked work package",
        ),
        _ => Err("workload ledger implementation status is unknown".into()),
    }
}

fn check_migration_rollback_rollup(root: &Path) -> TaskResult {
    let rollup_path = root.join("conformance/0.2/evidence/migration-rollback-rollup.json");
    let rollup = json(&rollup_path)?;
    require(
        rollup["schema_version"]
            == Value::String("mainframe-env.migration-rollback-rollup@1".into())
            && rollup["target_version"] == Value::String("0.2.0".into())
            && rollup["status"] == Value::String("pass".into())
            && rollup["migration_count"].as_u64() == Some(5)
            && rollup["destructive_migrations"].as_u64() == Some(0)
            && rollup["rollback_supported"].as_u64() == Some(5)
            && rollup["destructive_action_performed"] == Value::Bool(false),
        "migration/rollback rollup header is invalid",
    )?;
    let expected = BTreeSet::from([
        "conformance/0.2/migrations/application-package-v1-to-v2.json",
        "conformance/0.2/migrations/batch-controller-v0-to-v1.json",
        "conformance/0.2/migrations/db2-catalog-v1-to-v2.json",
        "conformance/0.2/migrations/host-abi-v0-to-v1.json",
        "conformance/0.2/migrations/registry-v0-to-v1.json",
    ]);
    let migrations = array(&rollup, "migrations", &rollup_path)?;
    let actual = migrations
        .iter()
        .map(|migration| text(migration, "path", &rollup_path))
        .collect::<TaskResult<BTreeSet<_>>>()?;
    require(
        actual == expected && actual.len() == migrations.len(),
        "migration/rollback rollup paths are missing or duplicated",
    )?;
    for migration in migrations {
        let relative = text(migration, "path", &rollup_path)?;
        let path = root.join(relative);
        let contract = json(&path)?;
        require(
            migration["sha256"] == Value::String(format!("sha256:{}", file_digest(&path)?))
                && migration["destructive"] == Value::Bool(false)
                && migration["rollback_supported"] == Value::Bool(true)
                && migration["partial_state_selected"] == Value::Bool(false)
                && contract["destructive"] == Value::Bool(false)
                && contract["rollback"]["supported"] == Value::Bool(true),
            &format!("migration/rollback rollup entry drifted: {relative}"),
        )?;
        let evidence = text(migration, "evidence", &rollup_path)?;
        require(
            root.join(evidence).is_file(),
            &format!("migration evidence is missing: {evidence}"),
        )?;
    }
    let contracts = json(&root.join("conformance/0.2/inventory/contracts.json"))?;
    require(
        contracts["contracts"]["migration_rollback_rollup"]
            == Value::String("mainframe-env.migration-rollback-rollup@1".into())
            && root
                .join("conformance/0.2/schemas/migration-rollback-rollup.schema.json")
                .is_file(),
        "migration/rollback rollup contract or schema is missing",
    )
}

fn check_full_regression(root: &Path) -> TaskResult {
    let path = root.join("conformance/0.2/evidence/full-regression.json");
    let receipt = json(&path)?;
    require(
        receipt["schema_version"] == Value::String("mainframe-env.full-regression@1".into())
            && receipt["target_version"] == Value::String("0.2.0".into())
            && receipt["status"] == Value::String("pass".into())
            && receipt["candidate_unchanged"] == Value::Bool(true),
        "full regression receipt header is invalid",
    )?;
    let candidate = &receipt["candidate"];
    let accepted_candidate_digest = accepted_0_2_candidate_digest(root)?;
    require(
        candidate["source_digest"].as_str() == Some(accepted_candidate_digest.as_str())
            && candidate["work_package_base_commit"]
                == Value::String("edb4558c987a56ee9f0c96438f756d663199942f".into())
            && candidate["accepted_release_commit"]
                == Value::String("44f3081eb2fdf22d09e1a97725f5a4163431ca70".into())
            && candidate["accepted_release_tree"]
                == Value::String("3d504ece02f1c09e124606ded695b00ba984d104".into())
            && candidate["accepted_carddemo_commit"]
                == Value::String("857115b907ce7098c965a51117a079048ea8182e".into())
            && candidate["accepted_carddemo_tree"]
                == Value::String("7e1fa73f4c808a89ddb4f98c0e3f7d0207f861ea".into()),
        "full regression candidate identity drifted",
    )?;
    require(
        command_text(root, "git", &["rev-parse", "mainframe-env-v0.1.1^{}"])?
            == candidate["accepted_release_commit"],
        "full regression accepted release tag moved",
    )?;
    let results = array(&receipt, "results", &path)?;
    require(
        results.len() >= 15
            && results
                .iter()
                .all(|result| result["exit_code"].as_i64() == Some(0)),
        "full regression result matrix is incomplete or failed",
    )?;
    let commands = results
        .iter()
        .filter_map(|result| result["command"].as_str())
        .collect::<Vec<_>>();
    for required in [
        "cargo fmt --all -- --check",
        "cargo check --workspace --all-targets --all-features --locked",
        "cargo test --workspace --all-features --locked --no-fail-fast",
        "cargo clippy --workspace --all-targets --all-features --locked -- -D warnings",
        "cargo doc --workspace --all-features --no-deps --locked",
        "cargo +1.95.0 check",
        "cargo deny check",
        "cargo xtask runtime-architecture --check",
        "cargo xtask conformance --check",
        "cargo xtask certification --check",
        "cargo xtask carddemo-full --check",
        "Zowe CLI 8.36.0",
        "cargo xtask release --check --target aarch64-apple-darwin",
        "cargo xtask release --check --target x86_64-unknown-linux-gnu",
        "git diff --check",
    ] {
        require(
            commands.iter().any(|command| command.contains(required)),
            &format!("full regression matrix omits {required}"),
        )?;
    }
    require(
        receipt["workspace"]["tests_passed"]
            .as_u64()
            .is_some_and(|count| count >= 260)
            && receipt["workspace"]["test_floor"].as_u64() == Some(260)
            && receipt["postgresql"]["version"].as_str() == Some("18")
            && receipt["postgresql"]["controls_passed"]
                .as_u64()
                .is_some_and(|count| count >= 2)
            && receipt["carddemo"]["journeys_passed"].as_u64() == Some(20)
            && receipt["carddemo"]["corpus_commit"]
                == Value::String("59cc6c2fd7ebd7ef7925cad552a01a4b8b6e4d5e".into())
            && receipt["live_zowe"]["cli_version"].as_str() == Some("8.36.0")
            && receipt["live_zowe"]["routes_passed"]
                .as_u64()
                .is_some_and(|count| count > 0)
            && receipt["release"]["artifacts_verified"] == Value::Bool(true),
        "full regression workload, PostgreSQL, CardDemo, Zowe, or release result is incomplete",
    )?;
    for target in ["aarch64-apple-darwin", "x86_64-unknown-linux-gnu"] {
        for (name, file) in [
            ("manifest_sha256", "manifest.json"),
            ("sbom_sha256", "sbom.cdx.json"),
            ("provenance_sha256", "provenance.intoto.json"),
            ("checksums_sha256", "checksums.sha256"),
            ("licenses_sha256", "LICENSES.md"),
            ("build_inputs_sha256", "build-inputs.json"),
        ] {
            let relative = format!("release/0.2.0/targets/{target}/{file}");
            require(
                receipt["release"]["targets"][target][name].as_str()
                    == Some(file_digest(&root.join(&relative))?.as_str()),
                &format!("full regression release artifact digest drifted: {relative}"),
            )?;
        }
    }
    require(
        receipt["remote_actions"]["tagged"] == Value::Bool(false)
            && receipt["remote_actions"]["published"] == Value::Bool(false)
            && receipt["remote_actions"]["deployed"] == Value::Bool(false)
            && receipt["remote_actions"]["merged"] == Value::Bool(false),
        "full regression receipt claims an unauthorized remote action",
    )?;
    let workload = json(&root.join("conformance/0.2/evidence/workload-ledger.json"))?;
    let status = json(&root.join("conformance/0.2/evidence/program-status.json"))?;
    require(
        workload["implementation_status"] == Value::String("complete".into())
            && status["implementation_status"] == Value::String("complete".into()),
        "full regression passed before both ledgers reached complete",
    )?;
    let contracts = json(&root.join("conformance/0.2/inventory/contracts.json"))?;
    require(
        contracts["contracts"]["full_regression"]
            == Value::String("mainframe-env.full-regression@1".into())
            && root
                .join("conformance/0.2/schemas/full-regression.schema.json")
                .is_file(),
        "full regression contract or schema is missing",
    )
}

fn check_review_repair(root: &Path) -> TaskResult {
    let path = root.join("conformance/0.2/evidence/review-repair.json");
    let evidence = json(&path)?;
    require(
        evidence["schema_version"] == Value::String("mainframe-env.review-repair@1".into())
            && evidence["derived"] == Value::Bool(true)
            && evidence["status"] == Value::String("pass".into()),
        "review repair evidence header is invalid",
    )?;
    let receipt = evidence
        .get("receipt")
        .and_then(Value::as_object)
        .ok_or("review repair receipt is missing")?;
    let digest = canonical_evidence_digest(receipt)?;
    require(
        evidence["evidence_digest"].as_str() == Some(digest.as_str()),
        "review repair evidence digest is stale",
    )?;
    let receipt = Value::Object(receipt.clone());
    let completion = find_completion_commit(
        root,
        "Repair 0.2.0 review findings",
        &[
            ("Review-Findings", "7=closed"),
            ("Target-Version", "0.2.0"),
            ("Evidence-Digest", digest.as_str()),
        ],
    )?;
    let parent = command_text(root, "git", &["rev-parse", &format!("{completion}^")])?;
    require(
        receipt["target_version"] == Value::String("0.2.0".into())
            && receipt["source_identity"] == Value::String(parent)
            && receipt["candidate_source_digest"].as_str()
                == Some(repository_digest_at_commit(root, &completion)?.as_str()),
        "review repair candidate identity drifted",
    )?;
    let findings = array(&receipt, "findings", &path)?;
    require(
        findings.len() == 7
            && findings.iter().enumerate().all(|(index, finding)| {
                finding["id"].as_u64() == Some((index + 1) as u64)
                    && finding["state"] == Value::String("closed".into())
                    && finding["regression"]
                        .as_str()
                        .is_some_and(|test| !test.is_empty())
            }),
        "review repair must close exactly findings 1 through 7 with regressions",
    )?;
    let commands = array(&receipt, "commands", &path)?;
    require(
        commands.len() >= 15
            && commands
                .iter()
                .all(|command| command["exit_code"].as_i64() == Some(0)),
        "review repair validation matrix is incomplete or failed",
    )?;
    let command_texts = commands
        .iter()
        .filter_map(|command| command["command"].as_str())
        .collect::<Vec<_>>();
    for required in [
        "cargo test -p mainframe-env-application",
        "cargo test -p mainframe-env-batch",
        "cargo test -p mainframe-env-db2",
        "cargo test -p mainframe-env-server",
        "cargo fmt --all -- --check",
        "cargo check --workspace --all-targets --all-features --locked",
        "cargo test --workspace --all-features --locked --no-fail-fast",
        "cargo clippy --workspace --all-targets --all-features --locked -- -D warnings",
        "cargo doc --workspace --all-features --no-deps --locked",
        "cargo +1.95.0 check",
        "cargo deny check",
        "cargo xtask runtime-architecture --check",
        "cargo xtask carddemo-full --check",
        "cargo xtask release --check --target aarch64-apple-darwin",
        "git diff --check",
    ] {
        require(
            command_texts
                .iter()
                .any(|command| command.contains(required)),
            &format!("review repair validation omits {required}"),
        )?;
    }
    for artifact in array(&receipt, "artifacts", &path)? {
        let relative = text(artifact, "path", &path)?;
        let expected = text(artifact, "sha256", &path)?;
        require(
            expected == format!("sha256:{}", git_file_digest(root, &completion, relative)?),
            &format!("review repair historical artifact drifted: {relative}"),
        )?;
    }
    let workflow = String::from_utf8(git_file_bytes(
        root,
        &completion,
        ".github/workflows/ci.yml",
    )?)
    .map_err(|error| error.to_string())?;
    require(
        workflow.contains("fetch-depth: 0")
            && workflow.contains("fetch-tags: true")
            && workflow.contains("cargo xtask release --check --target x86_64-unknown-linux-gnu"),
        "CI does not exercise full-history conformance and portable release verification",
    )?;
    let manifest: Value = serde_json::from_slice(&git_file_bytes(
        root,
        &completion,
        "release/0.2.0/targets/aarch64-apple-darwin/manifest.json",
    )?)
    .map_err(|error| error.to_string())?;
    require(
        manifest["target"] == Value::String("aarch64-apple-darwin".into())
            && manifest["published"] == Value::Bool(false)
            && receipt["remote_actions"]["tagged"] == Value::Bool(false)
            && receipt["remote_actions"]["published"] == Value::Bool(false)
            && receipt["remote_actions"]["deployed"] == Value::Bool(false)
            && receipt["remote_actions"]["merged"] == Value::Bool(false),
        "review repair crossed the release boundary",
    )
}

fn check_review_repair_round_2(root: &Path) -> TaskResult {
    check_review_repair(root)?;
    let path = root.join("conformance/0.2/evidence/review-repair-round-2.json");
    let evidence = json(&path)?;
    require(
        evidence["schema_version"] == Value::String("mainframe-env.review-repair-round-2@1".into())
            && evidence["derived"] == Value::Bool(true)
            && evidence["status"] == Value::String("pass".into()),
        "round-2 review repair evidence header is invalid",
    )?;
    let receipt = evidence
        .get("receipt")
        .and_then(Value::as_object)
        .ok_or("round-2 review repair receipt is missing")?;
    let digest = canonical_evidence_digest(receipt)?;
    require(
        evidence["evidence_digest"].as_str() == Some(digest.as_str()),
        "round-2 review repair evidence digest is stale",
    )?;
    let receipt = Value::Object(receipt.clone());
    let completion = find_completion_commit(
        root,
        "Repair 0.2.0 follow-up review findings",
        &[
            ("Follow-Up-Findings", "10=closed"),
            ("Target-Version", "0.2.0"),
            ("Evidence-Digest", digest.as_str()),
        ],
    )?;
    let prerequisite = text(&receipt, "prerequisite_commit", &path)?;
    let source = text(&receipt, "source_identity", &path)?;
    let prerequisite_source = text(&receipt, "prerequisite_source", &path)?;
    require(
        receipt["target_version"] == Value::String("0.2.0".into())
            && receipt["candidate_source_digest"].as_str()
                == Some(repository_digest_at_commit(root, &completion)?.as_str())
            && command_text(root, "git", &["rev-parse", &format!("{completion}^")])?
                == prerequisite
            && command_text(root, "git", &["rev-parse", &format!("{prerequisite}^")])? == source,
        "round-2 review repair candidate identity drifted",
    )?;
    require(
        command_text(root, "git", &["show", "-s", "--format=%s", prerequisite])?
            == "Clarify coverage worker PR authorization"
            && command_text(root, "git", &["rev-parse", prerequisite_source])?
                == prerequisite_source,
        "round-2 prerequisite cherry-pick identity drifted",
    )?;
    let round_one = &receipt["round_one"];
    require(
        round_one["completion_commit"] == Value::String(source.into())
            && round_one["evidence_path"]
                == Value::String("conformance/0.2/evidence/review-repair.json".into())
            && round_one["evidence_digest"]
                == Value::String(
                    "sha256:0c9fe6171576d7730ff69b7dc271a87ca5021df7cca1264c03b32b73eb8d03b8"
                        .into(),
                )
            && round_one["validation"] == Value::String("commit-bound".into()),
        "round-2 receipt does not preserve the round-1 evidence identity",
    )?;
    let findings = array(&receipt, "findings", &path)?;
    require(
        findings.len() == 10
            && findings.iter().enumerate().all(|(index, finding)| {
                finding["id"].as_u64() == Some((index + 1) as u64)
                    && finding["state"] == Value::String("closed".into())
                    && finding["repair"]
                        .as_str()
                        .is_some_and(|repair| !repair.is_empty())
                    && finding["regression"]
                        .as_str()
                        .is_some_and(|test| !test.is_empty())
            }),
        "round-2 review repair must close exactly findings 1 through 10",
    )?;
    let commands = array(&receipt, "commands", &path)?;
    require(
        commands.len() >= 20
            && commands
                .iter()
                .all(|command| command["exit_code"].as_i64() == Some(0)),
        "round-2 review repair validation matrix is incomplete or failed",
    )?;
    let command_texts = commands
        .iter()
        .filter_map(|command| command["command"].as_str())
        .collect::<Vec<_>>();
    for required in [
        "cargo test --locked -p mainframe-env-db2",
        "cargo test --locked -p mainframe-env-batch",
        "cargo test --locked -p mainframe-env-application",
        "cargo test --locked -p mainframe-env-server",
        "cargo test -p xtask --locked",
        "cargo xtask schemas --check",
        "cargo xtask carddemo-db2 --check",
        "cargo xtask carddemo-ims --check",
        "cargo xtask carddemo-mq-authorization --check",
        "cargo fmt --all -- --check",
        "cargo check --workspace --all-targets --all-features --locked",
        "cargo test --workspace --all-features --locked --no-fail-fast",
        "cargo clippy --workspace --all-targets --all-features --locked -- -D warnings",
        "cargo doc --workspace --all-features --no-deps --locked",
        "cargo +1.95.0 check",
        "cargo deny check",
        "cargo xtask runtime-architecture --check",
        "PostgreSQL 18",
        "cargo xtask carddemo-full --check",
        "Zowe CLI 8.36.0",
        "cargo xtask conformance --check",
        "cargo xtask certification --check",
        "cargo xtask release --check --target aarch64-apple-darwin",
        "cargo xtask release --check --target x86_64-unknown-linux-gnu",
        "git diff --check",
    ] {
        require(
            command_texts
                .iter()
                .any(|command| command.contains(required)),
            &format!("round-2 review repair validation omits {required}"),
        )?;
    }
    for artifact in array(&receipt, "artifacts", &path)? {
        let relative = text(artifact, "path", &path)?;
        require(
            !relative.contains("..") && !Path::new(relative).is_absolute(),
            "round-2 review repair artifact path is unsafe",
        )?;
        let expected = text(artifact, "sha256", &path)?;
        require(
            expected == format!("sha256:{}", git_file_digest(root, &completion, relative)?),
            &format!("round-2 historical artifact digest drifted: {relative}"),
        )?;
    }
    let workflow = String::from_utf8(git_file_bytes(
        root,
        &completion,
        ".github/workflows/ci.yml",
    )?)
    .map_err(|error| error.to_string())?;
    require(
        workflow.contains("fetch-depth: 0")
            && workflow.contains("fetch-tags: true")
            && workflow.contains("cargo xtask release --check --target x86_64-unknown-linux-gnu"),
        "CI does not retain the round-2 Linux release gate",
    )?;
    let manifest: Value = serde_json::from_slice(&git_file_bytes(
        root,
        &completion,
        "release/0.2.0/targets/aarch64-apple-darwin/manifest.json",
    )?)
    .map_err(|error| error.to_string())?;
    require(
        manifest["target"] == Value::String("aarch64-apple-darwin".into())
            && manifest["published"] == Value::Bool(false)
            && receipt["remote_actions"]["tagged"] == Value::Bool(false)
            && receipt["remote_actions"]["published"] == Value::Bool(false)
            && receipt["remote_actions"]["deployed"] == Value::Bool(false)
            && receipt["remote_actions"]["merged"] == Value::Bool(false),
        "round-2 review repair crossed the release boundary",
    )
}

fn check_review_repair_round_3(root: &Path) -> TaskResult {
    check_review_repair_round_2(root)?;
    let path = root.join("conformance/0.2/evidence/review-repair-round-3.json");
    let evidence = json(&path)?;
    require(
        evidence["schema_version"] == Value::String("mainframe-env.review-repair-round-3@1".into())
            && evidence["derived"] == Value::Bool(true)
            && evidence["status"] == Value::String("pass".into()),
        "round-3 review repair evidence header is invalid",
    )?;
    let receipt = evidence
        .get("receipt")
        .and_then(Value::as_object)
        .ok_or("round-3 review repair receipt is missing")?;
    let digest = canonical_evidence_digest(receipt)?;
    require(
        evidence["evidence_digest"].as_str() == Some(digest.as_str()),
        "round-3 review repair evidence digest is stale",
    )?;
    let receipt = Value::Object(receipt.clone());
    let completion = find_completion_commit(
        root,
        "Repair 0.2.0 third review findings",
        &[
            ("Third-Review-Findings", "11=closed"),
            ("Target-Version", "0.2.0"),
            ("Evidence-Digest", digest.as_str()),
        ],
    )?;
    let source = text(&receipt, "source_identity", &path)?;
    let prerequisite_source = text(&receipt, "prerequisite_source", &path)?;
    let prerequisite = text(&receipt, "prerequisite_commit", &path)?;
    verify_completion_parent(root, &completion, prerequisite)?;
    verify_completion_parent(root, prerequisite, source)?;
    require(
        receipt["target_version"] == Value::String("0.2.0".into())
            && source == "2513e9e56f330369e4a5f4e9a06c043ca99a024a"
            && prerequisite_source == "d4b94362af56f2847bbd62d106867e6f0bb0648a"
            && command_text(root, "git", &["show", "-s", "--format=%s", prerequisite])?
                == "Clarify coverage worker PR authorization"
            && command_text(root, "git", &["rev-parse", prerequisite_source])?
                == prerequisite_source
            && receipt["candidate_source_digest"].as_str()
                == Some(repository_digest_at_commit(root, &completion)?.as_str()),
        "round-3 review repair source, prerequisite, or candidate identity drifted",
    )?;
    let historical = array(&receipt, "historical_repairs", &path)?;
    require(
        historical.len() == 2
            && historical.iter().enumerate().all(|(index, row)| {
                row["round"].as_u64() == Some((index + 1) as u64)
                    && row["validation"] == Value::String("commit-bound".into())
            }),
        "round-3 repair must preserve rounds one and two as commit-bound evidence",
    )?;
    for (round, subject, trailer_name, trailer_value) in [
        (
            1_u64,
            "Repair 0.2.0 review findings",
            "Review-Findings",
            "7=closed",
        ),
        (
            2,
            "Repair 0.2.0 follow-up review findings",
            "Follow-Up-Findings",
            "10=closed",
        ),
    ] {
        let row = historical
            .iter()
            .find(|row| row["round"].as_u64() == Some(round))
            .ok_or_else(|| format!("round-{round} historical repair identity is missing"))?;
        let historical_digest = text(row, "evidence_digest", &path)?;
        let historical_completion = find_completion_commit(
            root,
            subject,
            &[
                (trailer_name, trailer_value),
                ("Target-Version", "0.2.0"),
                ("Evidence-Digest", historical_digest),
            ],
        )?;
        require(
            row["completion_commit"].as_str() == Some(historical_completion.as_str()),
            &format!("round-{round} historical completion identity drifted"),
        )?;
    }
    let findings = array(&receipt, "findings", &path)?;
    require(
        findings.len() == 11
            && findings.iter().enumerate().all(|(index, finding)| {
                finding["id"].as_u64() == Some((index + 1) as u64)
                    && finding["state"] == Value::String("closed".into())
                    && finding["repair"]
                        .as_str()
                        .is_some_and(|repair| !repair.is_empty())
                    && finding["regression"]
                        .as_str()
                        .is_some_and(|test| !test.is_empty())
            }),
        "round-3 review repair must close exactly findings 1 through 11",
    )?;
    let commands = array(&receipt, "commands", &path)?;
    require(
        commands.len() >= 25
            && commands
                .iter()
                .all(|command| command["exit_code"].as_i64() == Some(0)),
        "round-3 review repair validation matrix is incomplete or failed",
    )?;
    let command_texts = commands
        .iter()
        .filter_map(|command| command["command"].as_str())
        .collect::<Vec<_>>();
    for required in [
        "cargo test --locked -p mainframe-env-application",
        "cargo test --locked -p mainframe-env-db2",
        "cargo test --locked -p mainframe-env-racf",
        "cargo test --locked -p mainframe-env-server",
        "cargo test -p xtask --locked",
        "cargo fmt --all -- --check",
        "cargo check --workspace --all-targets --all-features --locked",
        "cargo test --workspace --all-features --locked --no-fail-fast",
        "cargo clippy --workspace --all-targets --all-features --locked -- -D warnings",
        "cargo doc --workspace --all-features --no-deps --locked",
        "cargo +1.95.0 check",
        "cargo deny check",
        "cargo xtask runtime-architecture --check",
        "cargo xtask schemas --check",
        "cargo xtask application-packages --check",
        "cargo xtask db2-catalog --check",
        "cargo xtask batch-controllers --check",
        "cargo xtask migration-rollback --check",
        "PostgreSQL 18",
        "cargo xtask carddemo-full --check",
        "Zowe CLI 8.36.0",
        "cargo xtask release --check --target aarch64-apple-darwin",
        "cargo xtask release --check --target x86_64-unknown-linux-gnu",
        "cargo xtask conformance --check",
        "cargo xtask certification --check",
        "git diff --check",
    ] {
        require(
            command_texts
                .iter()
                .any(|command| command.contains(required)),
            &format!("round-3 review repair validation omits {required}"),
        )?;
    }
    require(
        receipt["toolchains"]["rust"] == Value::String("1.98.0".into())
            && receipt["toolchains"]["msrv"] == Value::String("1.95.0".into())
            && receipt["toolchains"]["zowe_cli"] == Value::String("8.36.0".into())
            && receipt["toolchains"]["postgresql"] == Value::String("18".into())
            && receipt["toolchains"]["targets"]
                .as_array()
                .is_some_and(|targets| {
                    targets.as_slice()
                        == [
                            Value::String("aarch64-apple-darwin".into()),
                            Value::String("x86_64-unknown-linux-gnu".into()),
                        ]
                })
            && receipt["pull_request"]["number"].as_u64() == Some(1)
            && receipt["pull_request"]["head"] == Value::String("impl/0.2.0".into())
            && receipt["pull_request"]["base"] == Value::String("main".into())
            && receipt["pull_request"]["check_identities"]
                .as_array()
                .is_some_and(|checks| checks.len() == 2),
        "round-3 toolchain, target, PR, or check identities are incomplete",
    )?;
    for artifact in array(&receipt, "artifacts", &path)? {
        let relative = text(artifact, "path", &path)?;
        let expected = text(artifact, "sha256", &path)?;
        require(
            !relative.contains("..")
                && !Path::new(relative).is_absolute()
                && expected == format!("sha256:{}", git_file_digest(root, &completion, relative)?),
            &format!("round-3 historical artifact digest drifted: {relative}"),
        )?;
    }
    let workflow = String::from_utf8(git_file_bytes(
        root,
        &completion,
        ".github/workflows/ci.yml",
    )?)
    .map_err(|error| error.to_string())?;
    require(
        workflow.contains("fetch-depth: 0")
            && workflow.contains("fetch-tags: true")
            && workflow.contains("rust:1.98.0-bookworm@sha256:")
            && workflow.contains("cargo xtask release --check --target x86_64-unknown-linux-gnu"),
        "CI does not retain the pinned round-3 Linux release gate",
    )?;
    for target in ["aarch64-apple-darwin", "x86_64-unknown-linux-gnu"] {
        let manifest_path = format!("release/0.2.0/targets/{target}/manifest.json");
        let manifest: Value =
            serde_json::from_slice(&git_file_bytes(root, &completion, &manifest_path)?)
                .map_err(|error| error.to_string())?;
        require(
            manifest["target"] == Value::String(target.into())
                && manifest["published"] == Value::Bool(false)
                && manifest["artifacts"]
                    .as_array()
                    .is_some_and(|artifacts| artifacts.len() == 6),
            &format!("round-3 {target} release evidence is incomplete"),
        )?;
    }
    require(
        receipt["remote_actions"]["tagged"] == Value::Bool(false)
            && receipt["remote_actions"]["published"] == Value::Bool(false)
            && receipt["remote_actions"]["deployed"] == Value::Bool(false)
            && receipt["remote_actions"]["merged"] == Value::Bool(false),
        "round-3 review repair crossed the release boundary",
    )
}

fn check_review_repair_round_4(root: &Path) -> TaskResult {
    check_review_repair_round_3(root)?;
    check_evidence_seal(root)?;
    let path = root.join("conformance/0.2/evidence/review-repair-round-4.json");
    let evidence = json(&path)?;
    require(
        evidence["schema_version"] == Value::String("mainframe-env.review-repair-round-4@1".into())
            && evidence["derived"] == Value::Bool(true)
            && evidence["status"] == Value::String("pass".into()),
        "round-four review repair evidence header is invalid",
    )?;
    let receipt = evidence
        .get("receipt")
        .and_then(Value::as_object)
        .ok_or("round-four review repair receipt is missing")?;
    let digest = canonical_evidence_digest(receipt)?;
    require(
        evidence["evidence_digest"].as_str() == Some(digest.as_str()),
        "round-four review repair evidence digest is stale",
    )?;
    let receipt = Value::Object(receipt.clone());
    require(
        receipt["target_version"] == Value::String("0.2.0".into())
            && receipt["accepted_parent"]
                == Value::String("b61d604fbdf4867154f235d45fd4453ef7e3b79d".into())
            && receipt["branch"] == Value::String("impl/0.2.0".into())
            && receipt["base"] == Value::String("main".into())
            && receipt["pull_request"].as_u64() == Some(1)
            && receipt["future_ci_success_claimed"] == Value::Bool(false),
        "round-four source, branch, or future-CI boundary is invalid",
    )?;
    let findings = array(&receipt, "findings", &path)?;
    require(
        findings.len() == 3
            && findings.iter().enumerate().all(|(index, finding)| {
                finding["id"].as_u64() == Some((index + 1) as u64)
                    && finding["state"] == Value::String("closed".into())
                    && finding["repair"]
                        .as_str()
                        .is_some_and(|text| !text.is_empty())
                    && finding["regression"]
                        .as_str()
                        .is_some_and(|text| !text.is_empty())
            }),
        "round-four repair must close exactly findings 1 through 3",
    )?;
    let supersession = &receipt["round_three_supersession"];
    require(
        supersession["status"] == Value::String("superseded".into())
            && supersession["original"]["completion_commit"]
                == Value::String("ab4894d6111910b08f37579134520a1de93a1e8a".into())
            && supersession["original"]["evidence_digest"]
                == Value::String(
                    "sha256:bc89e54756b11557bb5828357a1de8661e9d821f7fa9d481129be92994fdbbf9"
                        .into(),
                )
            && supersession["accepted_follow_up"]["tip"]
                == Value::String("b61d604fbdf4867154f235d45fd4453ef7e3b79d".into())
            && supersession["accepted_follow_up"]["candidate_source_digest"]
                == Value::String(
                    "sha256:b812dbbae8d65467c5576c97314a0a0a2437efb13fccad3a8edebb02657e7435"
                        .into(),
                ),
        "round-three supersession does not preserve and correct the historical identity",
    )?;
    require(
        supersession["accepted_follow_up"]["candidate_source_digest"].as_str()
            == Some(
                repository_digest_at_commit(root, "b61d604fbdf4867154f235d45fd4453ef7e3b79d")?
                    .as_str(),
            ),
        "accepted round-three follow-up candidate digest drifted",
    )?;
    let failed = supersession["failed_candidates"]
        .as_array()
        .ok_or("round-three supersession failed candidates are missing")?;
    require(
        failed.len() == 3
            && failed.iter().all(|run| {
                run["event"] == Value::String("pull_request".into())
                    && run["run_status"] == Value::String("completed".into())
                    && run["run_conclusion"] == Value::String("failure".into())
                    && run["job"]["name"] == Value::String("v0-foundation".into())
                    && run["job"]["conclusion"] == Value::String("failure".into())
            })
            && failed
                .iter()
                .filter_map(|run| run["run_id"].as_u64())
                .collect::<BTreeSet<_>>()
                == BTreeSet::from([33420718088, 33422012078, 33424133663]),
        "round-three supersession failed-run identities are incomplete",
    )?;
    let accepted = supersession["accepted_follow_up"]["checks"]
        .as_array()
        .ok_or("round-three supersession accepted checks are missing")?;
    require(
        accepted.len() == 2
            && accepted.iter().all(|run| {
                run["head_sha"] == Value::String("b61d604fbdf4867154f235d45fd4453ef7e3b79d".into())
                    && run["run_status"] == Value::String("completed".into())
                    && run["run_conclusion"] == Value::String("success".into())
                    && run["job"]["name"] == Value::String("v0-foundation".into())
                    && run["job"]["conclusion"] == Value::String("success".into())
            })
            && accepted
                .iter()
                .filter_map(|run| run["event"].as_str())
                .collect::<BTreeSet<_>>()
                == BTreeSet::from(["pull_request", "push"])
            && accepted
                .iter()
                .filter_map(|run| run["run_id"].as_u64())
                .collect::<BTreeSet<_>>()
                == BTreeSet::from([33425915373, 33425919905]),
        "round-three supersession accepted run identities are incomplete",
    )?;
    let source_path = supersession["source_provenance"]["path"]
        .as_str()
        .ok_or("round-three supersession source path is missing")?;
    evidence_seal::validate_actions_source(root, &json(&root.join(source_path))?)?;
    require(
        fs::read(root.join("conformance/0.2/evidence/review-repair-round-3.json"))
            .map_err(|error| error.to_string())?
            == git_file_bytes(
                root,
                "ab4894d6111910b08f37579134520a1de93a1e8a",
                "conformance/0.2/evidence/review-repair-round-3.json",
            )?,
        "historical round-three receipt was rewritten instead of superseded",
    )?;
    let completions = evidence_seal::round_four_completion_commits(root, Some(&digest))?;
    require(
        completions.len() <= 1,
        "round-four completion identity is duplicated",
    )?;
    if let Some(completion) = completions.first() {
        require(
            command_text(root, "git", &["rev-parse", &format!("{completion}^")])?
                == "b61d604fbdf4867154f235d45fd4453ef7e3b79d",
            "round-four completion parent is not the accepted b61d604 tip",
        )?;
        for artifact in array(&receipt, "artifacts", &path)? {
            let relative = text(artifact, "path", &path)?;
            let expected = text(artifact, "sha256", &path)?;
            require(
                expected == format!("sha256:{}", git_file_digest(root, completion, relative)?),
                &format!("round-four sealed artifact drifted: {relative}"),
            )?;
        }
    }
    require(
        receipt["remote_actions"]
            == json!({"tagged": false, "published": false, "deployed": false, "merged": false}),
        "round-four repair crossed the remote-action boundary",
    )?;
    Ok(())
}

fn check_review_repair_round_5(root: &Path) -> TaskResult {
    check_review_repair_round_4(root)?;
    let path = root.join("conformance/0.2/evidence/review-repair-round-5.json");
    let evidence = json(&path)?;
    require(
        evidence["schema_version"] == Value::String("mainframe-env.review-repair-round-5@1".into())
            && evidence["derived"] == Value::Bool(true)
            && evidence["status"] == Value::String("pass".into()),
        "round-five review repair evidence header is invalid",
    )?;
    let receipt = evidence
        .get("receipt")
        .and_then(Value::as_object)
        .ok_or("round-five review repair receipt is missing")?;
    let digest = canonical_evidence_digest(receipt)?;
    require(
        evidence["evidence_digest"].as_str() == Some(digest.as_str()),
        "round-five review repair evidence digest is stale",
    )?;
    let receipt = Value::Object(receipt.clone());
    require(
        receipt["target_version"] == Value::String("0.2.0".into())
            && receipt["accepted_parent"]
                == Value::String("36344c542e82a4174f5be7da6038f7095ce6cba8".into())
            && receipt["branch"] == Value::String("impl/0.2.0".into())
            && receipt["historical_round_four"]["completion_commit"]
                == Value::String("36344c542e82a4174f5be7da6038f7095ce6cba8".into())
            && receipt["historical_round_four"]["remote_evidence_credit"]
                == Value::String("none".into()),
        "round-five source or historical-evidence boundary is invalid",
    )?;
    fn contains_forbidden_key(value: &Value) -> bool {
        const FORBIDDEN: [&str; 13] = [
            "commands",
            "command",
            "exit_code",
            "result",
            "workspace",
            "tests_passed",
            "toolchains",
            "github_jobs",
            "run_id",
            "job_id",
            "conclusion",
            "future_ci_success_claimed",
            "check_identities",
        ];
        match value {
            Value::Array(values) => values.iter().any(contains_forbidden_key),
            Value::Object(values) => {
                values.keys().any(|key| FORBIDDEN.contains(&key.as_str()))
                    || values.values().any(contains_forbidden_key)
            }
            _ => false,
        }
    }
    require(
        !contains_forbidden_key(&receipt),
        "round-five content receipt contains execution or remote-attestation claims",
    )?;
    let findings = array(&receipt, "findings", &path)?;
    require(
        findings.len() == 4
            && findings.iter().enumerate().all(|(index, finding)| {
                finding["id"].as_u64() == Some((index + 1) as u64)
                    && finding["state"] == Value::String("closed".into())
                    && finding["content_rule"]
                        .as_str()
                        .is_some_and(|value| !value.is_empty())
            }),
        "round-five repair must close exactly findings 1 through 4",
    )?;
    let profile_path = receipt["profile"]["path"]
        .as_str()
        .ok_or("round-five profile path is missing")?;
    let profile = json(&root.join(profile_path))?;
    evidence_seal::validate_profile(&profile)?;
    require(
        receipt["profile"]["sha256"].as_str()
            == Some(format!("sha256:{}", file_digest(&root.join(profile_path))?).as_str()),
        "round-five profile digest drifted",
    )?;
    let inventory = receipt["release_inventory"]
        .as_array()
        .ok_or("round-five release inventory is missing")?;
    require(
        inventory.len() == 2
            && inventory[0]["target"] == Value::String("aarch64-apple-darwin".into())
            && inventory[1]["target"] == Value::String("x86_64-unknown-linux-gnu".into())
            && inventory.iter().all(|target| {
                target["documents"].as_array().is_some_and(|documents| {
                    documents.as_slice()
                        == [
                            Value::String("LICENSES.md".into()),
                            Value::String("build-inputs.json".into()),
                            Value::String("checksums.sha256".into()),
                            Value::String("manifest.json".into()),
                            Value::String("provenance.intoto.json".into()),
                            Value::String("sbom.cdx.json".into()),
                        ]
                })
            }),
        "round-five release inventory is not the exact two-target six-document set",
    )?;
    let completions = evidence_seal::round_five_completion_commits(root, Some(&digest))?;
    require(
        completions.len() <= 1,
        "round-five completion identity is duplicated",
    )?;
    if let Some(completion) = completions.first() {
        require(
            command_text(root, "git", &["rev-parse", &format!("{completion}^")])?
                == "36344c542e82a4174f5be7da6038f7095ce6cba8",
            "round-five completion parent is not the accepted round-four tip",
        )?;
        let mut artifact_paths = BTreeSet::new();
        for artifact in array(&receipt, "artifacts", &path)? {
            let relative = text(artifact, "path", &path)?;
            let expected = text(artifact, "sha256", &path)?;
            require(
                artifact_paths.insert(relative)
                    && expected
                        == format!("sha256:{}", git_file_digest(root, completion, relative)?),
                &format!("round-five content artifact drifted or duplicated: {relative}"),
            )?;
        }
    }
    Ok(())
}

fn validate_official_source(source: &Value, path: &Path, baseline: &str) -> TaskResult {
    let source = source.as_object().ok_or_else(|| {
        format!(
            "{} baseline {baseline} source is not an object",
            path.display()
        )
    })?;
    let url = source
        .get("url")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("baseline {baseline} source URL is missing"))?;
    require(
        url.starts_with("https://www.ibm.com/"),
        &format!("baseline {baseline} source is not an official IBM HTTPS URL"),
    )?;
    let digest = source
        .get("sha256")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("baseline {baseline} source digest is missing"))?;
    validate_sha256_identity(digest, &format!("baseline {baseline} source digest"))?;
    require(
        source
            .get("bytes")
            .and_then(Value::as_u64)
            .is_some_and(|bytes| bytes > 0)
            && source
                .get("snapshot_date")
                .and_then(Value::as_str)
                .is_some_and(|date| date.len() >= 4)
            && source.get("retained_in_repository") == Some(&Value::Bool(false)),
        &format!("baseline {baseline} source provenance is incomplete"),
    )
}

fn validate_sha256_identity(value: &str, field: &str) -> TaskResult {
    require(
        value.len() == 71
            && value.starts_with("sha256:")
            && value[7..]
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()),
        &format!("{field} is not a lowercase SHA-256 identity"),
    )
}

fn check_evidence(root: &Path) -> TaskResult {
    let evidence = root.join("conformance/0.1/evidence");
    let entry_path = evidence.join("entry.json");
    let entry = json(&entry_path)?;
    require(
        entry.get("derived") == Some(&Value::Bool(true)),
        "entry evidence is not derived",
    )?;
    require(
        entry.get("status") == Some(&Value::String("pass".to_string())),
        "entry gate is not pass",
    )?;
    let status_path = evidence.join("program-status.json");
    let status = json(&status_path)?;
    require(
        text(&status, "current_phase", &status_path)?.starts_with("ME.V"),
        "program phase is invalid",
    )?;
    require(
        !array(&status, "commands", &status_path)?.is_empty(),
        "program status has no command receipts",
    )?;
    let current = text(&status, "current_phase", &status_path)?
        .strip_prefix("ME.V")
        .ok_or("current phase prefix is invalid")?
        .parse::<usize>()
        .map_err(|error| format!("current phase number is invalid: {error}"))?;
    for phase in 0..=current {
        let path = evidence.join(format!("phase-v{phase}.json"));
        let result = json(&path)?;
        require(
            result.get("derived") == Some(&Value::Bool(true))
                && result.get("status") == Some(&Value::String("pass".to_string())),
            &format!("ME.V{phase} evidence does not derive pass"),
        )?;
        require(
            text(&result, "digest", &path)?.starts_with("sha256:"),
            &format!("ME.V{phase} evidence digest is missing"),
        )?;
    }
    Ok(())
}

/// Schema/instance consistency and historical sealed-input verification, not
/// a new full conformance campaign or licensed-equivalence result.
fn check_evidence_fast(root: &Path) -> TaskResult {
    check_schemas(root)?;
    evidence_seal::check(root)
}

fn check_runtime_architecture(root: &Path) -> TaskResult {
    check_architecture(root)?;
    let status = Command::new("cargo")
        .args([
            "build",
            "--release",
            "-p",
            "mainframe-env-server",
            "--all-features",
            "--locked",
        ])
        .current_dir(root)
        .status()
        .map_err(|error| format!("release server build: {error}"))?;
    require(status.success(), "release server build failed")?;
    release_server_sqlite_smoke(root)
}

fn release_server_sqlite_smoke(root: &Path) -> TaskResult {
    let binary = cargo_target_directory(root).join("release/mainframe-env-server");
    require(binary.is_file(), "release server binary is missing")?;
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_nanos();
    let directory = env::temp_dir().join(format!(
        "mainframe-env-runtime-architecture-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir(&directory).map_err(|error| format!("{}: {error}", directory.display()))?;
    let database = directory.join("state.db");
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|error| error.to_string())?;
    let port = listener
        .local_addr()
        .map_err(|error| error.to_string())?
        .port();
    drop(listener);
    let mut child = Command::new(&binary)
        .arg(root.join("config/mainframe-env.toml"))
        .current_dir(&directory)
        .env("MAINFRAME_ENV_STORE", "sqlite")
        .env("MAINFRAME_ENV_TLS", "false")
        .env("MAINFRAME_ENV_LISTEN", format!("127.0.0.1:{port}"))
        .env(
            "MAINFRAME_ENV_SQLITE_URL",
            format!("sqlite://{}?mode=rwc", database.display()),
        )
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("start release server: {error}"))?;
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut passed = false;
    let mut failure = None;
    while Instant::now() < deadline {
        if let Some(status) = child.try_wait().map_err(|error| error.to_string())? {
            let mut stderr = String::new();
            if let Some(mut pipe) = child.stderr.take() {
                let _ = pipe.read_to_string(&mut stderr);
            }
            failure = Some(format!("release server exited {status}: {stderr}"));
            break;
        }
        if let Ok(mut stream) = TcpStream::connect(("127.0.0.1", port)) {
            let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
            let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
            if stream
                .write_all(
                    b"GET /zosmf/info HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
                )
                .is_ok()
            {
                let mut response = String::new();
                if stream.read_to_string(&mut response).is_ok()
                    && response.contains(" 200 ")
                    && response.contains("\"ready\":true")
                {
                    passed = true;
                    break;
                }
            }
        }
        thread::sleep(Duration::from_millis(50));
    }
    if child
        .try_wait()
        .map_err(|error| error.to_string())?
        .is_none()
    {
        let _ = child.kill();
        let _ = child.wait();
    }
    if database.exists() {
        fs::remove_file(&database).map_err(|error| format!("{}: {error}", database.display()))?;
    }
    let artifacts = directory.join("mainframe-env-artifacts");
    if artifacts.exists() {
        fs::remove_dir_all(&artifacts)
            .map_err(|error| format!("{}: {error}", artifacts.display()))?;
    }
    fs::remove_dir(&directory).map_err(|error| format!("{}: {error}", directory.display()))?;
    require(
        passed,
        failure
            .as_deref()
            .unwrap_or("release SQLite server did not become ready"),
    )
}

fn check_certification(root: &Path) -> TaskResult {
    check_versions(root)?;
    check_profiles(root)?;
    check_schemas(root)?;
    check_inventory(root)?;
    check_evidence(root)?;
    check_coverage(root)?;
    check_semantic_identities(root)?;
    check_application_packages(root)?;
    check_db2_catalog(root)?;
    check_review_repair_round_4(root)?;
    check_review_repair_round_5(root)?;
    check_evidence_seal(root)?;
    check_runtime_architecture(root)?;

    let inventory = root.join("conformance/0.1/inventory");
    let packages_path = inventory.join("packages.json");
    let packages = json(&packages_path)?;
    let mut package_names = array(&packages, "packages", &packages_path)?
        .iter()
        .map(|row| text(row, "name", &packages_path).map(str::to_string))
        .collect::<TaskResult<BTreeSet<_>>>()?;
    let additions_path = root.join("conformance/0.2/inventory/package-additions.json");
    if additions_path.is_file() {
        let additions = json(&additions_path)?;
        for package in array(&additions, "packages", &additions_path)? {
            require(
                package_names.insert(text(package, "name", &additions_path)?.to_string()),
                "0.2 package addition duplicates a historical package",
            )?;
        }
    }
    let metadata_output = Command::new("cargo")
        .args(["metadata", "--no-deps", "--format-version", "1", "--locked"])
        .current_dir(root)
        .output()
        .map_err(|error| format!("cargo metadata: {error}"))?;
    require(metadata_output.status.success(), "cargo metadata failed")?;
    let metadata: Value = serde_json::from_slice(&metadata_output.stdout)
        .map_err(|error| format!("cargo metadata JSON: {error}"))?;
    let workspace_names = metadata["packages"]
        .as_array()
        .ok_or("metadata packages missing")?
        .iter()
        .filter_map(|package| package["name"].as_str().map(str::to_string))
        .collect::<BTreeSet<_>>();
    require(
        workspace_names == package_names,
        "package inventory differs from Cargo workspace",
    )?;

    let selectors_path = inventory.join("selectors.json");
    let selectors = json(&selectors_path)?;
    for selector in array(&selectors, "selectors", &selectors_path)? {
        let authority = text(selector, "target_authority", &selectors_path)?;
        require(
            package_names.contains(authority),
            &format!("selector authority {authority} is not a workspace package"),
        )?;
        let coverage = text(selector, "coverage", &selectors_path)?;
        require(
            inventory.join(coverage).is_file(),
            &format!("selector coverage {coverage} is missing"),
        )?;
    }

    let authority_path = inventory.join("authority-graph.json");
    let authority = json(&authority_path)?;
    let mut families = BTreeSet::new();
    let mut defaults = BTreeSet::new();
    for row in array(&authority, "authorities", &authority_path)? {
        require(
            families.insert(text(row, "family", &authority_path)?),
            "authority family has more than one default",
        )?;
        let selected = text(row, "default", &authority_path)?;
        require(
            selected.starts_with("mainframe-env-") && package_names.contains(selected),
            &format!("default authority {selected} is invalid"),
        )?;
        defaults.insert(selected);
    }
    require(
        authority.get("automatic_fallback") == Some(&Value::Bool(false)),
        "automatic fallback is enabled",
    )?;

    let gaps_path = inventory.join("known-gaps.json");
    let gaps = json(&gaps_path)?;
    for gap in array(&gaps, "gaps", &gaps_path)? {
        let status = text(gap, "status", &gaps_path)?;
        let blocks = text(gap, "blocks", &gaps_path)?;
        require(
            !status.starts_with("open") || blocks == "none" || blocks.starts_with("none;"),
            &format!(
                "open certification blocker: {}",
                text(gap, "id", &gaps_path)?
            ),
        )?;
    }

    let evidence = root.join("conformance/0.1/evidence");
    for name in [
        "compatibility-summary.json",
        "profile-closure.json",
        "resource-summary.json",
        "security-summary.json",
        "recovery-summary.json",
        "verification-summary.json",
        "cutover-summary.json",
        "release-exit-gates.json",
    ] {
        let path = evidence.join(name);
        let value = json(&path)?;
        require(
            value
                .get("status")
                .and_then(Value::as_str)
                .is_some_and(|status| status.starts_with("pass")),
            &format!("{name} does not pass"),
        )?;
    }
    for name in [
        "cobol-hello.json",
        "cics-carddemo.json",
        "jcl-jes.json",
        "zosmf.json",
        "dataset.json",
        "racf.json",
    ] {
        let path = evidence.join("differential").join(name);
        let value = json(&path)?;
        require(
            value
                .get("status")
                .and_then(Value::as_str)
                .is_some_and(|status| status.starts_with("pass")),
            &format!("differential {name} does not pass"),
        )?;
    }
    for phase in 0..=7 {
        let subject = format!("Complete ME.V{phase}");
        let output = Command::new("git")
            .args(["log", "--format=%B%x00", "--grep", &format!("^{subject}")])
            .current_dir(root)
            .output()
            .map_err(|error| format!("git log: {error}"))?;
        require(output.status.success(), "git log failed")?;
        let message = String::from_utf8(output.stdout).map_err(|error| error.to_string())?;
        require(
            message.contains(&format!("Phase-Gate: ME.V{phase}=pass"))
                && message.contains("Evidence-Digest: sha256:")
                && message.contains("Product-Version: 0.1.0-alpha.0"),
            &format!("ME.V{phase} completion commit or trailers are missing"),
        )?;
    }
    for subject in [
        "Fix SQL stores in async server contexts",
        "Route execution through the common coordinator",
        "Wire transactional execution durability",
        "Enforce scoped provider capabilities",
        "Record the twenty-package architecture decision",
        "Make architecture certification executable",
    ] {
        let output = Command::new("git")
            .args([
                "log",
                "-1",
                "--format=%H",
                "--grep",
                &format!("^{subject}$"),
            ])
            .current_dir(root)
            .output()
            .map_err(|error| format!("git log for {subject}: {error}"))?;
        require(
            output.status.success() && !output.stdout.is_empty(),
            &format!("architecture remediation commit is missing: {subject}"),
        )?;
    }
    let exit_gates_path = evidence.join("release-exit-gates.json");
    let exit_gates = json(&exit_gates_path)?;
    for condition in [
        "architecture_audit_remediated",
        "runtime_architecture_gate_pass",
        "issue_commits_present",
    ] {
        require(
            exit_gates["conditions"][condition] == Value::Bool(true),
            &format!("release exit condition {condition} is not derived true"),
        )?;
    }
    let workspace_tests = Command::new("cargo")
        .args([
            "test",
            "--workspace",
            "--all-features",
            "--locked",
            "--no-fail-fast",
            "--quiet",
        ])
        .current_dir(root)
        .status()
        .map_err(|error| format!("certification workspace tests: {error}"))?;
    require(
        workspace_tests.success(),
        "certification workspace tests failed",
    )?;
    check_release_artifacts(root, &host_target(root)?)?;
    Ok(())
}

fn print_digest(root: &Path) -> TaskResult {
    println!("{}", repository_digest(root)?);
    Ok(())
}

fn accepted_0_2_candidate_digest(root: &Path) -> TaskResult<String> {
    repository_digest_at_commit(root, &accepted_0_2_completion(root)?)
}

fn accepted_0_2_completion(root: &Path) -> TaskResult<String> {
    let evidence = json(&root.join("conformance/0.2/evidence/review-repair-round-5.json"))?;
    let evidence_digest = evidence["evidence_digest"]
        .as_str()
        .ok_or("round-five accepted-candidate evidence digest is missing")?;
    let completions = evidence_seal::round_five_completion_commits(root, Some(evidence_digest))?;
    match completions.as_slice() {
        [completion] => Ok(completion.clone()),
        [] => Err("round-five accepted-candidate completion is missing".into()),
        _ => Err("round-five accepted-candidate completion is duplicated".into()),
    }
}

fn repository_digest(root: &Path) -> TaskResult<String> {
    let mut files = Vec::new();
    collect_files(root, &mut files)?;
    files.sort();
    let mut digest = Sha256::new();
    for file in files {
        let relative = file.strip_prefix(root).map_err(|error| error.to_string())?;
        if repository_digest_excluded(relative) {
            continue;
        }
        let bytes = fs::read(&file).map_err(|error| format!("{}: {error}", file.display()))?;
        let path = relative.to_string_lossy();
        digest.update((path.len() as u64).to_be_bytes());
        digest.update(path.as_bytes());
        digest.update((bytes.len() as u64).to_be_bytes());
        digest.update(bytes);
    }
    Ok(format!("sha256:{:x}", digest.finalize()))
}

fn repository_digest_at_commit(root: &Path, commit: &str) -> TaskResult<String> {
    let listing = command_text(root, "git", &["ls-tree", "-r", "--name-only", commit])?;
    let mut files = listing.lines().map(PathBuf::from).collect::<Vec<_>>();
    files.sort();
    let mut digest = Sha256::new();
    for relative in files {
        if repository_digest_excluded(&relative) {
            continue;
        }
        let relative = relative.to_string_lossy();
        let bytes = git_file_bytes(root, commit, &relative)?;
        digest.update((relative.len() as u64).to_be_bytes());
        digest.update(relative.as_bytes());
        digest.update((bytes.len() as u64).to_be_bytes());
        digest.update(bytes);
    }
    Ok(format!("sha256:{:x}", digest.finalize()))
}

fn repository_digest_excluded(relative: &Path) -> bool {
    let relative_text = relative.to_string_lossy();
    relative.starts_with(".git")
        || relative.starts_with("target")
        || relative.starts_with("conformance/0.1/evidence/raw")
        || relative == Path::new("conformance/0.1/evidence/program-status.json")
        || relative == Path::new("conformance/0.2/evidence/program-status.json")
        || relative == Path::new("conformance/0.2/evidence/workload-ledger.json")
        || relative == Path::new("conformance/0.2/evidence/full-regression.json")
        || relative == Path::new("conformance/0.2/evidence/review-repair.json")
        || relative == Path::new("conformance/0.2/evidence/review-repair-round-2.json")
        || relative == Path::new("conformance/0.2/evidence/review-repair-round-3.json")
        || relative == Path::new("conformance/0.2/evidence/review-repair-round-4.json")
        || relative == Path::new("conformance/0.2/evidence/review-repair-round-5.json")
        || relative == Path::new("conformance/0.2/evidence/work-packages/CV-209.json")
        || relative == Path::new("docs/delivery/coverage-versions/status/0.2.0.md")
        || relative == Path::new("conformance/0.6/evidence/dataset-certification.json")
        || relative == Path::new("docs/delivery/coverage-versions/status/0.6.0.md")
        || (relative_text.starts_with("conformance/0.1/evidence/phase-v")
            && relative.extension() == Some(OsStr::new("json")))
}

fn accepted_0_2_release_source_digest(root: &Path) -> TaskResult<String> {
    let accepted_candidate = accepted_0_2_completion(root)?;
    let listing = command_text(
        root,
        "git",
        &["ls-tree", "-r", "--name-only", &accepted_candidate],
    )?;
    let mut files = listing.lines().map(PathBuf::from).collect::<Vec<_>>();
    files.sort();
    let mut digest = Sha256::new();
    for relative in files {
        if relative.starts_with(".git")
            || relative.starts_with("target")
            || relative.starts_with("release")
            || relative.starts_with("conformance/0.1/evidence/raw")
            || relative == Path::new("conformance/0.2/evidence/program-status.json")
            || relative == Path::new("conformance/0.2/evidence/workload-ledger.json")
            || relative == Path::new("conformance/0.2/evidence/full-regression.json")
            || relative == Path::new("conformance/0.2/evidence/review-repair.json")
            || relative == Path::new("conformance/0.2/evidence/review-repair-round-2.json")
            || relative == Path::new("conformance/0.2/evidence/review-repair-round-3.json")
            || relative == Path::new("conformance/0.2/evidence/review-repair-round-4.json")
            || relative == Path::new("conformance/0.2/evidence/review-repair-round-4-inputs.json")
            || relative == Path::new("conformance/0.2/evidence/review-repair-round-5.json")
            || relative == Path::new("conformance/0.2/evidence/review-repair-round-5-profile.json")
            || relative
                == Path::new("conformance/0.2/schemas/review-repair-round-5-profile.schema.json")
            || relative == Path::new("conformance/0.2/schemas/review-repair-round-5.schema.json")
            || relative == Path::new("conformance/0.2/evidence/sources/round-3-ci-runs.json")
            || relative == Path::new("conformance/0.2/evidence/work-packages/CV-209.json")
            || relative == Path::new("docs/delivery/coverage-versions/status/0.2.0.md")
        {
            continue;
        }
        let bytes = if relative == Path::new("xtask/src/main.rs")
            || relative == Path::new("xtask/src/evidence_seal.rs")
            || relative == Path::new("docs/releases/0.2.md")
            || relative == Path::new("conformance/0.2/evidence/work-package-amendments.json")
        {
            git_file_bytes(
                root,
                "36344c542e82a4174f5be7da6038f7095ce6cba8",
                &relative.to_string_lossy(),
            )?
        } else {
            git_file_bytes(root, &accepted_candidate, &relative.to_string_lossy())?
        };
        let path = relative.to_string_lossy();
        digest.update((path.len() as u64).to_be_bytes());
        digest.update(path.as_bytes());
        digest.update((bytes.len() as u64).to_be_bytes());
        digest.update(bytes);
    }
    Ok(format!("sha256:{:x}", digest.finalize()))
}

fn release_source_digest(root: &Path) -> TaskResult<String> {
    let version = product_version(root)?;
    if version == "0.2.0" {
        return accepted_0_2_release_source_digest(root);
    }
    live_release_source_digest(root, &version)
}

fn live_release_source_digest(root: &Path, version: &str) -> TaskResult<String> {
    stable_zero_version(version)?;
    require_clean_release_source(root, version)?;
    let head = command_text(root, "git", &["rev-parse", "HEAD"])?;
    let listing = command_text(root, "git", &["ls-tree", "-r", "--name-only", &head])?;
    let mut files = listing.lines().map(PathBuf::from).collect::<Vec<_>>();
    files.sort();
    let mut digest = Sha256::new();
    for relative in files {
        if relative.starts_with("release") {
            continue;
        }
        let relative_text = relative.to_string_lossy();
        let bytes = git_file_bytes(root, &head, &relative_text)?;
        digest.update((relative_text.len() as u64).to_be_bytes());
        digest.update(relative_text.as_bytes());
        digest.update((bytes.len() as u64).to_be_bytes());
        digest.update(bytes);
    }
    Ok(format!("sha256:{:x}", digest.finalize()))
}

fn require_clean_release_source(root: &Path, version: &str) -> TaskResult {
    stable_zero_version(version)?;
    let allowed = format!("release/{version}/targets/");
    let mut changed = BTreeSet::new();
    for arguments in [
        vec!["diff", "--name-only", "-z"],
        vec!["diff", "--cached", "--name-only", "-z"],
        vec!["ls-files", "--others", "--exclude-standard", "-z"],
    ] {
        let output = Command::new("git")
            .args(arguments)
            .current_dir(root)
            .output()
            .map_err(|error| format!("git release source status: {error}"))?;
        require(
            output.status.success(),
            "could not inspect release source cleanliness",
        )?;
        for path in output.stdout.split(|byte| *byte == 0) {
            if path.is_empty() {
                continue;
            }
            let path = std::str::from_utf8(path).map_err(|_| "release source path is not UTF-8")?;
            if !path.starts_with(&allowed) {
                changed.insert(path.to_string());
            }
        }
    }
    require(
        changed.is_empty(),
        &format!(
            "release source is not clean: {}",
            changed.into_iter().collect::<Vec<_>>().join(", ")
        ),
    )
}

#[cfg(test)]
fn explicit_release_target(arguments: &[String]) -> TaskResult<String> {
    let positions = arguments
        .iter()
        .enumerate()
        .filter_map(|(index, argument)| (argument == "--target").then_some(index))
        .collect::<Vec<_>>();
    require(
        positions.len() == 1,
        "release requires exactly one --target <triple>",
    )?;
    let target = arguments
        .get(positions[0] + 1)
        .ok_or("release --target value is missing")?;
    validate_release_target(target)?;
    Ok(target.clone())
}

fn validate_release_target(target: &str) -> TaskResult {
    require(
        !target.is_empty()
            && target.len() <= 128
            && target
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
            && RETAINED_RELEASE_TARGETS.contains(&target),
        "release target is invalid",
    )
}

fn host_target(root: &Path) -> TaskResult<String> {
    let verbose = command_text(root, "rustc", &["-vV"])?;
    let target = verbose
        .lines()
        .find_map(|line| line.strip_prefix("host: "))
        .ok_or("rustc host target is missing")?;
    validate_release_target(target)?;
    Ok(target.into())
}

fn build_release_target(root: &Path, target: &str) -> TaskResult {
    validate_release_target(target)?;
    let mut rustflags = vec![format!(
        "--remap-path-prefix={}=/workspace/mainframe-env",
        root.display()
    )];
    let user_home = env::var_os("HOME").map(PathBuf::from);
    for (path, replacement) in [
        (
            env::var_os("CARGO_HOME")
                .map(PathBuf::from)
                .or_else(|| user_home.as_ref().map(|home| home.join(".cargo"))),
            "/cargo",
        ),
        (
            env::var_os("RUSTUP_HOME")
                .map(PathBuf::from)
                .or_else(|| user_home.as_ref().map(|home| home.join(".rustup"))),
            "/rustup",
        ),
    ] {
        if let Some(path) = path {
            rustflags.push(format!(
                "--remap-path-prefix={}={replacement}",
                path.display()
            ));
        }
    }
    if target.contains("apple-darwin") {
        rustflags.push("-C link-arg=-Wl,-no_uuid".into());
    } else if target.contains("linux-gnu") {
        rustflags.push("-C link-arg=-Wl,--build-id=sha1".into());
    }
    let mut command = Command::new("cargo");
    command
        .args([
            "build",
            "--release",
            "-p",
            "mainframe-env-server",
            "-p",
            "mainframe-env-cli",
            "--all-features",
            "--locked",
            "--target",
            target,
        ])
        .env("CARGO_INCREMENTAL", "0")
        .env("SOURCE_DATE_EPOCH", "0")
        .env("ZERO_AR_DATE", "1")
        .env("LC_ALL", "C")
        .env("TZ", "UTC")
        .env("RUSTFLAGS", rustflags.join(" "))
        .current_dir(root)
        .env_remove("CARGO_ENCODED_RUSTFLAGS");
    if target.contains("apple-darwin") {
        command.env("MACOSX_DEPLOYMENT_TARGET", "15.0");
    }
    let status = command
        .status()
        .map_err(|error| format!("cargo release build for {target}: {error}"))?;
    require(
        status.success(),
        &format!("release build failed for {target}"),
    )
}

fn cargo_target_directory(root: &Path) -> PathBuf {
    env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .map(|path| {
            if path.is_absolute() {
                path
            } else {
                root.join(path)
            }
        })
        .unwrap_or_else(|| root.join("target"))
}

fn generate_release_artifacts(root: &Path, target: &str) -> TaskResult {
    if retained_accepted_release(root)?.is_some() {
        return validate_retained_accepted_release(root, target);
    }
    build_release_target(root, target)?;
    let documents = release_documents(root, target)?;
    validate_release_documents(root, &documents, target)?;
    for (relative, bytes) in documents {
        let path = root.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| format!("{}: {error}", parent.display()))?;
        }
        fs::write(&path, bytes).map_err(|error| format!("{}: {error}", path.display()))?;
    }
    Ok(())
}

fn check_release_artifacts(root: &Path, target: &str) -> TaskResult {
    if retained_accepted_release(root)?.is_some() {
        return validate_retained_accepted_release(root, target);
    }
    let retained = retained_release_documents(root, target)?;
    build_release_target(root, target)?;
    let documents = release_documents(root, target)?;
    validate_release_documents(root, &documents, target)?;
    compare_release_documents(&retained, &documents)?;
    validate_checked_in_release_targets(root)?;
    Ok(())
}

fn retained_accepted_release(root: &Path) -> TaskResult<Option<String>> {
    if product_version(root)? != "0.2.0" {
        return Ok(None);
    }
    let accepted = accepted_0_2_completion(root)?;
    let head = command_text(root, "git", &["rev-parse", "HEAD"])?;
    if head == accepted {
        return Ok(None);
    }
    let status = Command::new("git")
        .args(["merge-base", "--is-ancestor", &accepted, &head])
        .current_dir(root)
        .status()
        .map_err(|error| format!("git merge-base for accepted 0.2 release: {error}"))?;
    require(
        status.success(),
        "live tree is not descended from the accepted 0.2 release candidate",
    )?;
    Ok(Some(accepted))
}

fn validate_retained_accepted_release(root: &Path, target: &str) -> TaskResult {
    let accepted = retained_accepted_release(root)?
        .ok_or("retained accepted-release validation requires a later descendant")?;
    let retained = retained_release_documents_for_version(root, "0.2.0", target)?;
    let source_digest = accepted_0_2_release_source_digest(root)?;
    validate_release_documents_for_identity(&retained, target, "0.2.0", "stable", &source_digest)?;
    for (relative, actual) in &retained {
        let expected = git_file_bytes(root, &accepted, &relative.to_string_lossy())?;
        require(
            actual == &expected,
            &format!(
                "accepted 0.2 release document drifted after completion: {}",
                relative.display()
            ),
        )?;
    }
    validate_historical_0_2_release(root)
}

fn validate_historical_0_2_release(root: &Path) -> TaskResult {
    let accepted = accepted_0_2_completion(root)?;
    let source_digest = accepted_0_2_release_source_digest(root)?;
    validate_checked_in_release_targets_for_identity(root, "0.2.0", "stable", &source_digest)?;
    for target in RETAINED_RELEASE_TARGETS {
        let documents = retained_release_documents_for_version(root, "0.2.0", target)?;
        for (relative, actual) in documents {
            let expected = git_file_bytes(root, &accepted, &relative.to_string_lossy())?;
            require(
                actual == expected,
                &format!(
                    "accepted 0.2 release document drifted after completion: {}",
                    relative.display()
                ),
            )?;
        }
    }
    Ok(())
}

fn compare_release_documents(
    retained: &BTreeMap<PathBuf, Vec<u8>>,
    generated: &BTreeMap<PathBuf, Vec<u8>>,
) -> TaskResult {
    require(
        retained.keys().eq(generated.keys()),
        "retained target evidence file set drifted",
    )?;
    for (relative, expected) in generated {
        let actual = retained
            .get(relative)
            .ok_or_else(|| format!("retained target evidence omits {}", relative.display()))?;
        require(
            actual == expected,
            &format!(
                "{} is stale; regenerate target evidence independently",
                relative.display()
            ),
        )?;
    }
    Ok(())
}

fn retained_release_documents(root: &Path, target: &str) -> TaskResult<BTreeMap<PathBuf, Vec<u8>>> {
    let version = product_version(root)?;
    retained_release_documents_for_version(root, &version, target)
}

fn retained_release_documents_for_version(
    root: &Path,
    version: &str,
    target: &str,
) -> TaskResult<BTreeMap<PathBuf, Vec<u8>>> {
    let directory = root.join(release_target_directory(version, target)?);
    require(
        directory.is_dir(),
        &format!("retained target evidence is missing for {target}"),
    )?;
    let mut documents = BTreeMap::new();
    for name in [
        "manifest.json",
        "sbom.cdx.json",
        "provenance.intoto.json",
        "checksums.sha256",
        "LICENSES.md",
        "build-inputs.json",
    ] {
        let path = directory.join(name);
        require(
            path.is_file(),
            &format!("retained target evidence omits {name} for {target}"),
        )?;
        documents.insert(
            path.strip_prefix(root)
                .map_err(|error| error.to_string())?
                .to_path_buf(),
            fs::read(&path).map_err(|error| format!("{}: {error}", path.display()))?,
        );
    }
    Ok(documents)
}

fn validate_checked_in_release_targets(root: &Path) -> TaskResult {
    let version = product_version(root)?;
    let release_config: toml::Value = read(&root.join("release.toml"))?
        .parse()
        .map_err(|error| format!("release.toml: {error}"))?;
    let channel = release_config["product"]["channel"]
        .as_str()
        .ok_or("product.channel is missing")?;
    let source_digest = release_source_digest(root)?;
    validate_checked_in_release_targets_for_identity(root, &version, channel, &source_digest)
}

fn validate_checked_in_release_targets_for_identity(
    root: &Path,
    version: &str,
    channel: &str,
    source_digest: &str,
) -> TaskResult {
    stable_zero_version(version)?;
    let directory = root.join(format!("release/{version}/targets"));
    require(
        directory.is_dir(),
        &format!("retained target evidence is missing for {version}"),
    )?;
    let mut targets = BTreeSet::new();
    for entry in
        fs::read_dir(&directory).map_err(|error| format!("{}: {error}", directory.display()))?
    {
        let path = entry.map_err(|error| error.to_string())?.path();
        if !path.is_dir() {
            return Err(format!(
                "release target directory contains non-directory {}",
                path.display()
            ));
        }
        let target = path
            .file_name()
            .and_then(OsStr::to_str)
            .ok_or("release receipt target is not UTF-8")?;
        validate_release_target(target)?;
        targets.insert(target.to_string());
        let documents = retained_release_documents_for_version(root, version, target)?;
        validate_release_documents_for_identity(
            &documents,
            target,
            version,
            channel,
            source_digest,
        )?;
    }
    require(
        targets
            == RETAINED_RELEASE_TARGETS
                .into_iter()
                .map(str::to_string)
                .collect(),
        "retained release target set differs from the advertised targets",
    )?;
    Ok(())
}

fn validate_release_documents(
    root: &Path,
    documents: &BTreeMap<PathBuf, Vec<u8>>,
    target: &str,
) -> TaskResult {
    let version = product_version(root)?;
    let release_config: toml::Value = read(&root.join("release.toml"))?
        .parse()
        .map_err(|error| format!("release.toml: {error}"))?;
    let channel = release_config["product"]["channel"]
        .as_str()
        .ok_or("product.channel is missing")?;
    let source_digest = release_source_digest(root)?;
    validate_release_documents_for_identity(documents, target, &version, channel, &source_digest)
}

fn validate_release_documents_for_identity(
    documents: &BTreeMap<PathBuf, Vec<u8>>,
    target: &str,
    version: &str,
    channel: &str,
    source_digest: &str,
) -> TaskResult {
    stable_zero_version(version)?;
    validate_release_target(target)?;
    validate_sha256_identity(source_digest, "release source digest")?;
    require(documents.len() == 6, "target release receipt is incomplete")?;
    let document = |name: &str| {
        documents
            .iter()
            .find_map(|(path, bytes)| (path.file_name() == Some(OsStr::new(name))).then_some(bytes))
            .ok_or_else(|| format!("target release receipt omits {name}"))
    };
    let manifest: Value = serde_json::from_slice(document("manifest.json")?)
        .map_err(|error| format!("release manifest: {error}"))?;
    let sbom: Value = serde_json::from_slice(document("sbom.cdx.json")?)
        .map_err(|error| format!("release SBOM: {error}"))?;
    let provenance: Value = serde_json::from_slice(document("provenance.intoto.json")?)
        .map_err(|error| format!("release provenance: {error}"))?;
    let checksums = std::str::from_utf8(document("checksums.sha256")?)
        .map_err(|error| format!("release checksums: {error}"))?;
    let licenses = std::str::from_utf8(document("LICENSES.md")?)
        .map_err(|error| format!("release licenses: {error}"))?;
    let build_inputs: Value = serde_json::from_slice(document("build-inputs.json")?)
        .map_err(|error| format!("release build inputs: {error}"))?;
    validate_release_identity(
        &manifest,
        &sbom,
        &provenance,
        &build_inputs,
        licenses,
        target,
        version,
        channel,
        source_digest,
    )?;
    let artifacts = manifest["artifacts"]
        .as_array()
        .ok_or("release manifest artifacts are missing")?;
    require(
        artifacts.len() == 6,
        "release manifest artifact count drifted",
    )?;
    let build_inputs_digest = format!("{:x}", Sha256::digest(document("build-inputs.json")?));
    require(
        artifacts.iter().any(|artifact| {
            artifact["path"].as_str() == Some("build-inputs.json")
                && artifact["sha256"].as_str() == Some(build_inputs_digest.as_str())
        }) && provenance["predicate"]["buildDefinition"]["resolvedDependencies"]
            .as_array()
            .is_some_and(|dependencies| {
                dependencies.iter().any(|dependency| {
                    dependency["uri"].as_str() == Some("build-inputs.json")
                        && dependency["digest"]["sha256"].as_str()
                            == Some(build_inputs_digest.as_str())
                })
            }),
        "release build-input identity is not bound into manifest and provenance",
    )?;
    for artifact in artifacts {
        let path = artifact["path"]
            .as_str()
            .ok_or("release artifact path is missing")?;
        let digest = artifact["sha256"]
            .as_str()
            .ok_or("release artifact digest is missing")?;
        validate_sha256_hex(digest, "release artifact digest")?;
        require(
            checksums
                .lines()
                .any(|line| line == format!("{digest}  {path}")),
            &format!("release checksums omit {path}"),
        )?;
    }
    for (name, path) in [
        ("mainframe-env-server", "bin/mainframe-env-server"),
        ("mainframe-env", "bin/mainframe-env"),
    ] {
        let digest = artifacts
            .iter()
            .find(|artifact| artifact["path"].as_str() == Some(path))
            .and_then(|artifact| artifact["sha256"].as_str())
            .ok_or_else(|| format!("release manifest omits {path}"))?;
        require(
            provenance["subject"].as_array().is_some_and(|subjects| {
                subjects.iter().any(|subject| {
                    subject["name"].as_str() == Some(name)
                        && subject["digest"]["sha256"].as_str() == Some(digest)
                })
            }),
            &format!("release provenance omits {name}"),
        )?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn validate_release_identity(
    manifest: &Value,
    sbom: &Value,
    provenance: &Value,
    build_inputs: &Value,
    licenses: &str,
    target: &str,
    version: &str,
    channel: &str,
    source_digest: &str,
) -> TaskResult {
    require(
        manifest["schema_version"] == Value::String("mainframe-env.release-manifest@1".into())
            && manifest["version"] == Value::String(version.into())
            && manifest["channel"] == Value::String(channel.into())
            && manifest["tag"] == Value::String(format!("mainframe-env-v{version}"))
            && manifest["target"] == Value::String(target.into())
            && manifest["published"] == Value::Bool(false)
            && sbom["bomFormat"] == Value::String("CycloneDX".into())
            && sbom["specVersion"] == Value::String("1.6".into())
            && sbom["metadata"]["component"]["version"] == Value::String(version.into())
            && provenance["predicateType"]
                == Value::String("https://slsa.dev/provenance/v1".into())
            && provenance["predicate"]["buildDefinition"]["externalParameters"]["target"]
                == Value::String(target.into())
            && build_inputs["schema_version"]
                == Value::String("mainframe-env.release-build-inputs@1".into())
            && build_inputs["target"] == Value::String(target.into())
            && build_inputs["rustc_verbose"].as_str().is_some_and(|value| {
                value.starts_with("rustc 1.98.0") && value.contains("release: 1.98.0")
            })
            && build_inputs["cargo_lock_sha256"].as_str().is_some()
            && build_inputs["source_digest"] == Value::String(source_digest.into())
            && build_inputs["deterministic_environment"]["SOURCE_DATE_EPOCH"]
                == Value::String("0".into())
            && licenses.starts_with("# License expressions\n"),
        "target release receipt metadata is inconsistent",
    )?;
    require(
        provenance["predicate"]["buildDefinition"]["resolvedDependencies"]
            .as_array()
            .is_some_and(|dependencies| {
                dependencies.iter().any(|dependency| {
                    dependency["uri"].as_str() == Some("source-tree")
                        && dependency["digest"]["sha256"].as_str()
                            == source_digest.strip_prefix("sha256:")
                })
            }),
        "release provenance is not bound to the canonical source digest",
    )?;
    Ok(())
}

fn validate_sha256_hex(value: &str, field: &str) -> TaskResult {
    require(
        value.len() == 64
            && value
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()),
        &format!("{field} is not a lowercase SHA-256 digest"),
    )
}

fn release_documents(root: &Path, target: &str) -> TaskResult<BTreeMap<PathBuf, Vec<u8>>> {
    validate_release_target(target)?;
    let version = product_version(root)?;
    let release_config: toml::Value = read(&root.join("release.toml"))?
        .parse()
        .map_err(|error| format!("release.toml: {error}"))?;
    let channel = release_config["product"]["channel"]
        .as_str()
        .ok_or("product.channel is missing")?;
    let directory = release_target_directory(&version, target)?;
    let executable_suffix = if target.contains("windows") {
        ".exe"
    } else {
        ""
    };
    let target_directory = cargo_target_directory(root);
    let server = target_directory.join(format!(
        "{target}/release/mainframe-env-server{executable_suffix}"
    ));
    let cli = target_directory.join(format!("{target}/release/mainframe-env{executable_suffix}"));
    require(
        server.is_file(),
        &format!("release server binary was not built for {target}"),
    )?;
    require(
        cli.is_file(),
        &format!("release CLI binary was not built for {target}"),
    )?;
    let server_digest = file_digest(&server)?;
    let cli_digest = file_digest(&cli)?;
    let lock_digest = file_digest(&root.join("Cargo.lock"))?;
    let config_digest = file_digest(&root.join("config/mainframe-env.toml"))?;
    let sqlite_migration = file_digest(
        &root.join("crates/stores/mainframe-env-store/migrations/sqlite/0001-durable-state.sql"),
    )?;
    let postgres_migration = file_digest(
        &root.join("crates/stores/mainframe-env-store/migrations/postgres/0001-durable-state.sql"),
    )?;
    let phase_base = command_text(
        root,
        "git",
        &["log", "-1", "--format=%H", "--grep=^Complete ME.V6"],
    )?;
    let rustc = command_text(root, "rustc", &["--version"])?;
    let rustc_verbose = command_text(root, "rustc", &["-vV"])?
        .lines()
        .filter(|line| !line.starts_with("host: "))
        .collect::<Vec<_>>()
        .join("\n");
    let cargo_version = command_text(root, "cargo", &["--version"])?;
    let source_digest = release_source_digest(root)?;
    let metadata_output = Command::new("cargo")
        .args(["metadata", "--format-version", "1", "--locked"])
        .current_dir(root)
        .output()
        .map_err(|error| format!("cargo metadata: {error}"))?;
    require(metadata_output.status.success(), "cargo metadata failed")?;
    let metadata: Value = serde_json::from_slice(&metadata_output.stdout)
        .map_err(|error| format!("cargo metadata JSON: {error}"))?;
    let packages = metadata
        .get("packages")
        .and_then(Value::as_array)
        .ok_or("cargo metadata has no packages")?;
    let mut components = packages
        .iter()
        .filter(|package| {
            !matches!(
                package.get("name").and_then(Value::as_str),
                Some("mainframe-env-conformance" | "xtask")
            )
        })
        .map(|package| {
            let name = package.get("name").and_then(Value::as_str).unwrap_or("");
            let version = package.get("version").and_then(Value::as_str).unwrap_or("");
            let license = package.get("license").and_then(Value::as_str);
            let source = package.get("source").and_then(Value::as_str);
            let mut component = json!({
                "type":if name.starts_with("mainframe-env") {"application"} else {"library"},
                "bom-ref":format!("pkg:cargo/{name}@{version}"),
                "name":name,
                "version":version,
                "purl":format!("pkg:cargo/{name}@{version}")
            });
            if let Some(license) = license {
                component["licenses"] = json!([{"expression":license}]);
            }
            if let Some(source) = source {
                component["properties"] = json!([{"name":"cargo:source","value":source}]);
            }
            component
        })
        .collect::<Vec<_>>();
    components.sort_by(|left, right| {
        left["name"]
            .as_str()
            .cmp(&right["name"].as_str())
            .then(left["version"].as_str().cmp(&right["version"].as_str()))
    });
    let sbom = json!({
        "bomFormat":"CycloneDX",
        "specVersion":"1.6",
        "version":1,
        "metadata":{
            "component":{"type":"application","name":"mainframe-env","version":version},
            "properties":[
                {"name":"mainframe-env:phase-base","value":phase_base},
                {"name":"mainframe-env:cargo-lock-sha256","value":lock_digest},
                {"name":"mainframe-env:release-profile","value":"core-server"},
                {"name":"mainframe-env:excluded-non-production-workspace-package-count","value":"2"}
            ]
        },
        "components":components
    });
    let mut deterministic_environment = json!({
        "CARGO_INCREMENTAL":"0",
        "LC_ALL":"C",
        "SOURCE_DATE_EPOCH":"0",
        "TZ":"UTC",
        "ZERO_AR_DATE":"1"
    });
    if target.contains("apple-darwin") {
        deterministic_environment["MACOSX_DEPLOYMENT_TARGET"] = Value::String("15.0".into());
    }
    let build_inputs = json!({
        "schema_version":"mainframe-env.release-build-inputs@1",
        "target":target,
        "rustc_verbose":rustc_verbose,
        "cargo_version":cargo_version,
        "profile":"release",
        "profile_settings":{"codegen_units":1,"debug":0,"incremental":false,"strip":"symbols"},
        "features":"all",
        "locked":true,
        "cargo_lock_sha256":lock_digest,
        "source_digest":source_digest,
        "path_remapping":{"repository":"/workspace/mainframe-env","cargo_home":"/cargo","rustup_home":"/rustup"},
        "deterministic_environment":deterministic_environment,
        "linker_determinism":if target.contains("apple-darwin") {"-Wl,-no_uuid"} else {"-Wl,--build-id=sha1"}
    });
    let build_inputs_bytes = pretty_json(&build_inputs)?;
    let build_inputs_digest = format!("{:x}", Sha256::digest(&build_inputs_bytes));
    let manifest = json!({
        "schema_version":"mainframe-env.release-manifest@1",
        "product":"mainframe-env",
        "version":version,
        "channel":channel,
        "phase_base_revision":phase_base,
        "toolchain":rustc,
        "target":target,
        "profile":"release/core-server",
        "contracts":"conformance/0.2/inventory/versions.json",
        "migration_head":"0001-durable-state",
        "artifacts":[
            {"path":"bin/mainframe-env-server","sha256":server_digest},
            {"path":"bin/mainframe-env","sha256":cli_digest},
            {"path":"config/mainframe-env.toml","sha256":config_digest},
            {"path":"migrations/sqlite/0001-durable-state.sql","sha256":sqlite_migration},
            {"path":"migrations/postgres/0001-durable-state.sql","sha256":postgres_migration},
            {"path":"build-inputs.json","sha256":build_inputs_digest}
        ],
        "tag":format!("mainframe-env-v{version}"),
        "published":false
    });
    let provenance = json!({
        "_type":"https://in-toto.io/Statement/v1",
        "subject":[
            {"name":"mainframe-env-server","digest":{"sha256":server_digest}},
            {"name":"mainframe-env","digest":{"sha256":cli_digest}}
        ],
        "predicateType":"https://slsa.dev/provenance/v1",
        "predicate":{
            "buildDefinition":{
                "buildType":"mainframe-env.cargo-release@1",
                "externalParameters":{"profile":"release","locked":true,"all_features":true,"target":target},
                "resolvedDependencies":[
                    {"uri":"Cargo.lock","digest":{"sha256":lock_digest}},
                    {"uri":"build-inputs.json","digest":{"sha256":build_inputs_digest}},
                    {"uri":"source-tree","digest":{"sha256":source_digest.trim_start_matches("sha256:")}}
                ]
            },
            "runDetails":{"builder":{"id":format!("mainframe-env-cargo/{target}")},"metadata":{"invocationId":format!("mainframe-env-v{version}-{target}-local")}}
        }
    });
    let mut licenses = packages
        .iter()
        .filter_map(|package| package.get("license").and_then(Value::as_str))
        .map(str::to_string)
        .collect::<BTreeSet<_>>();
    licenses.insert("Apache-2.0 (mainframe-env)".into());
    let notices = format!(
        "# License expressions\n\nGenerated from locked Cargo metadata. Full dependency texts remain in their source packages.\n\n{}\n",
        licenses
            .into_iter()
            .map(|license| format!("- {license}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
    let checksums = format!(
        "{server_digest}  bin/mainframe-env-server\n{cli_digest}  bin/mainframe-env\n{config_digest}  config/mainframe-env.toml\n{sqlite_migration}  migrations/sqlite/0001-durable-state.sql\n{postgres_migration}  migrations/postgres/0001-durable-state.sql\n{build_inputs_digest}  build-inputs.json\n"
    );
    Ok(BTreeMap::from([
        (directory.join("manifest.json"), pretty_json(&manifest)?),
        (directory.join("sbom.cdx.json"), pretty_json(&sbom)?),
        (
            directory.join("provenance.intoto.json"),
            pretty_json(&provenance)?,
        ),
        (directory.join("checksums.sha256"), checksums.into_bytes()),
        (directory.join("LICENSES.md"), notices.into_bytes()),
        (directory.join("build-inputs.json"), build_inputs_bytes),
    ]))
}

fn pretty_json(value: &Value) -> TaskResult<Vec<u8>> {
    let mut bytes = serde_json::to_vec_pretty(value).map_err(|error| error.to_string())?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn canonical_evidence_digest(receipt: &serde_json::Map<String, Value>) -> TaskResult<String> {
    let canonical = serde_json::to_vec(receipt).map_err(|error| error.to_string())?;
    Ok(format!("sha256:{:x}", Sha256::digest(canonical)))
}

fn file_digest(path: &Path) -> TaskResult<String> {
    let bytes = fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

fn git_file_digest(root: &Path, commit: &str, relative: &str) -> TaskResult<String> {
    Ok(format!(
        "{:x}",
        Sha256::digest(git_file_bytes(root, commit, relative)?)
    ))
}

fn git_file_bytes(root: &Path, commit: &str, relative: &str) -> TaskResult<Vec<u8>> {
    require(
        !relative.contains("..") && !Path::new(relative).is_absolute(),
        "historical artifact path is unsafe",
    )?;
    let object = format!("{commit}:{relative}");
    let output = Command::new("git")
        .args(["show", &object])
        .current_dir(root)
        .output()
        .map_err(|error| format!("git show {object}: {error}"))?;
    require(
        output.status.success(),
        &format!("historical artifact is missing: {object}"),
    )?;
    Ok(output.stdout)
}

fn verify_commit_bound_live_file(
    root: &Path,
    commit: &str,
    relative: &str,
    live_path: &Path,
) -> TaskResult {
    require(
        git_file_bytes(root, commit, relative)?
            == fs::read(live_path).map_err(|error| format!("{}: {error}", live_path.display()))?,
        &format!(
            "{} was rewritten or superseded without a recorded amendment after {commit}",
            live_path.display()
        ),
    )
}

fn verify_completion_parent(root: &Path, completion: &str, expected_parent: &str) -> TaskResult {
    require(
        command_text(root, "git", &["rev-parse", &format!("{completion}^")])? == expected_parent,
        &format!("completion {completion} does not descend from {expected_parent}"),
    )
}

fn find_completion_commit(
    root: &Path,
    subject: &str,
    trailers: &[(&str, &str)],
) -> TaskResult<String> {
    let output = Command::new("git")
        .args(["log", "--format=%H%x1f%B%x1e", "HEAD"])
        .current_dir(root)
        .output()
        .map_err(|error| format!("git log: {error}"))?;
    require(output.status.success(), "git log failed")?;
    let history = String::from_utf8(output.stdout).map_err(|error| error.to_string())?;
    let matches = history
        .split('\u{1e}')
        .filter_map(|record| record.trim().split_once('\u{1f}'))
        .filter(|(_, message)| completion_message_matches(message, subject, trailers))
        .map(|(commit, _)| commit.trim().to_string())
        .collect::<Vec<_>>();
    require(
        matches.len() == 1,
        &format!(
            "completion history for {subject:?} is missing or ambiguous ({})",
            matches.len()
        ),
    )?;
    Ok(matches[0].clone())
}

fn completion_message_matches(message: &str, subject: &str, trailers: &[(&str, &str)]) -> bool {
    if message.lines().next() != Some(subject) {
        return false;
    }
    trailers.iter().all(|(name, expected)| {
        let prefix = format!("{name}:");
        let values = message
            .lines()
            .filter(|line| line.starts_with(&prefix))
            .collect::<Vec<_>>();
        values.len() == 1 && values[0] == format!("{name}: {expected}")
    })
}

fn command_text(root: &Path, program: &str, arguments: &[&str]) -> TaskResult<String> {
    let output = Command::new(program)
        .args(arguments)
        .current_dir(root)
        .output()
        .map_err(|error| format!("{program}: {error}"))?;
    require(output.status.success(), &format!("{program} failed"))?;
    String::from_utf8(output.stdout)
        .map(|value| value.trim().to_string())
        .map_err(|error| error.to_string())
}

fn excluded_names(root: &Path) -> TaskResult<BTreeSet<String>> {
    let path = root.join("conformance/0.1/inventory/excluded-components.json");
    let value = json(&path)?;
    Ok(array(&value, "components", &path)?
        .iter()
        .filter_map(|row| row.get("name").and_then(Value::as_str).map(str::to_string))
        .collect())
}

fn unique_rows(rows: &[Value], key: &str, path: &Path) -> TaskResult {
    let mut seen = BTreeSet::new();
    for row in rows {
        let id = text(row, key, path)?;
        require(
            seen.insert(id),
            &format!("{} repeats {key} {id:?}", path.display()),
        )?;
    }
    Ok(())
}

fn collect_named(root: &Path, name: &OsStr, files: &mut Vec<PathBuf>) -> TaskResult {
    if root.file_name() == Some(OsStr::new(".git"))
        || root.file_name() == Some(OsStr::new("target"))
    {
        return Ok(());
    }
    for entry in fs::read_dir(root).map_err(|error| format!("{}: {error}", root.display()))? {
        let path = entry.map_err(|error| error.to_string())?.path();
        if path.is_dir() {
            collect_named(&path, name, files)?;
        } else if path.file_name() == Some(name) {
            files.push(path);
        }
    }
    Ok(())
}

fn collect_extension(root: &Path, extension: &OsStr, files: &mut Vec<PathBuf>) -> TaskResult {
    for entry in fs::read_dir(root).map_err(|error| format!("{}: {error}", root.display()))? {
        let path = entry.map_err(|error| error.to_string())?.path();
        if path.is_dir() {
            collect_extension(&path, extension, files)?;
        } else if path.extension() == Some(extension) {
            files.push(path);
        }
    }
    Ok(())
}

fn collect_files(root: &Path, files: &mut Vec<PathBuf>) -> TaskResult {
    if root.file_name() == Some(OsStr::new(".git"))
        || root.file_name() == Some(OsStr::new("target"))
    {
        return Ok(());
    }
    for entry in fs::read_dir(root).map_err(|error| format!("{}: {error}", root.display()))? {
        let path = entry.map_err(|error| error.to_string())?.path();
        if path.is_dir() {
            collect_files(&path, files)?;
        } else {
            files.push(path);
        }
    }
    Ok(())
}

fn require(condition: bool, message: &str) -> TaskResult {
    if condition {
        Ok(())
    } else {
        Err(message.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dataset_reference_simulation_cannot_satisfy_the_licensed_receipt_gate() {
        let root = repository_root().expect("repository root");
        let pending = check_dataset_oracle_receipt(&root, None)
            .expect_err("missing licensed receipt must remain pending");
        assert!(pending.contains("licensed z/OS 3.2 differential is pending"));
        let simulation =
            root.join("crates/tooling/mainframe-env-conformance/src/dataset_reference.rs");
        assert!(check_dataset_oracle_receipt(&root, Some(simulation)).is_err());
        let local_certification = root.join("conformance/0.6/evidence/dataset-certification.json");
        assert!(check_dataset_oracle_receipt(&root, Some(local_certification)).is_err());
    }

    #[test]
    fn a_publication_body_is_recognised_by_its_bytes_and_not_by_its_name() {
        // The opening of an IBM-served topic, in the shape every one of them has.
        let served = br#"<div><article role="article" aria-labelledby="t__1">
<h1 class="topictitle1" id="t__1">DD statement</h1><div id="lastModifiedDate"><span>Last Updated</span>: 2026-01-28</div>
<div class="body"><p class="p">The DD statement describes a data set.</p></div></article></div>
"#;
        assert_eq!(
            publication_body(served),
            Some("a topic body served by IBM Documentation")
        );
        // Renaming is what the extension rule could not survive, so the bytes
        // decide: the same body called anything at all is the same body.
        assert!(publication_body(b"\xef\xbb\xbf  \n<div><article role=\"article\"><h1 class=\"topictitle1\">X</h1><div id=\"lastModifiedDate\">1</div></div>").is_some());
        assert_eq!(
            publication_body(b"%PDF-1.7\n1 0 obj"),
            Some("a PDF document")
        );

        // Markup this project wrote: the fixture on codex/docs-semantic-experiment
        // is markup with no served stamp on it, and it stays legal.
        assert_eq!(
            publication_body(
                b"<main data-product=\"CICS\">\n<article data-topic-id=\"cics-rewrite\"><h1 id=\"rewrite\">REWRITE</h1></article>\n</main>\n"
            ),
            None
        );
        // Source that quotes both markers, which is why the markup clause exists:
        // docs_api.py defines them and test_locator_tools.py tests them.
        assert_eq!(
            publication_body(
                b"#!/usr/bin/env python3\nHEADING = '<h1 class=\"topictitle1\">'\nLAST = '<div id=\"lastModifiedDate\">'\n"
            ),
            None
        );
        // Markup that carries only one of the two is not a served body.
        assert_eq!(
            publication_body(b"<div><h1 class=\"topictitle1\">hand written</h1></div>"),
            None
        );

        // And the tree this test runs in holds none.
        check_publication_bytes(&repository_root().expect("repository root"))
            .expect("the repository holds no publication bytes");
    }

    #[test]
    fn jes_oracle_requires_an_external_reviewed_receipt() {
        let root = repository_root().expect("repository root");
        let pending = check_jes_oracle_receipt(&root, None)
            .expect_err("missing licensed receipt must remain pending");
        assert!(pending.contains("licensed z/OS 3.2 JES2 differential is pending"));
        let local_artifact = root.join("conformance/0.8/oracles/jes-licensed-differential.json");
        let error = check_jes_oracle_receipt(&root, Some(local_artifact))
            .expect_err("a candidate-tree artifact must not substitute for a licensed receipt");
        assert!(error.contains("must remain external to the candidate tree"));
    }

    #[test]
    fn strict_cli_rejects_unknown_duplicate_and_surplus_arguments() {
        for arguments in [
            vec!["xtask", "evidence", "seal", "--unknown"],
            vec!["xtask", "evidence", "seal", "--check", "--check"],
            vec!["xtask", "evidence", "seal", "surplus"],
            vec!["xtask", "release", "--target", "host", "--target", "other"],
            vec![
                "xtask",
                "evidence",
                "callback",
                "--output",
                "/tmp/ROUND_5_DONE.json",
            ],
            vec![
                "xtask",
                "evidence",
                "callback",
                "--output",
                "/tmp/ROUND_4_DONE.json",
                "--tip",
                "forged",
            ],
        ] {
            assert!(Cli::try_parse_from(arguments).is_err());
        }
        assert!(Cli::try_parse_from(["xtask", "evidence", "seal", "--check"]).is_ok());
        assert!(Cli::try_parse_from(["xtask", "evidence", "callback"]).is_ok());
        assert!(Cli::try_parse_from(["xtask", "jes-oracle-candidate"]).is_ok());
        assert!(Cli::try_parse_from(["xtask", "jes-oracle-candidate", "surplus"]).is_err());
        assert!(
            Cli::try_parse_from([
                "xtask",
                "release",
                "--check",
                "--target",
                "x86_64-unknown-linux-gnu",
            ])
            .is_ok()
        );
    }

    fn temporary_git_repository(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let path = env::temp_dir().join(format!(
            "mainframe-env-xtask-{name}-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("temporary repository");
        for arguments in [
            vec!["init", "--quiet"],
            vec!["config", "user.email", "xtask@example.invalid"],
            vec!["config", "user.name", "xtask"],
        ] {
            assert!(
                Command::new("git")
                    .args(arguments)
                    .current_dir(&path)
                    .status()
                    .expect("git")
                    .success()
            );
        }
        path
    }

    fn commit_all(root: &Path, message: &str) -> String {
        assert!(
            Command::new("git")
                .args(["add", "."])
                .current_dir(root)
                .status()
                .expect("git add")
                .success()
        );
        assert!(
            Command::new("git")
                .args(["commit", "--quiet", "-m", message])
                .current_dir(root)
                .status()
                .expect("git commit")
                .success()
        );
        command_text(root, "git", &["rev-parse", "HEAD"]).expect("commit identity")
    }

    #[test]
    fn jes_oracle_candidate_is_index_bound_and_ignores_only_derived_reports() {
        let root = temporary_git_repository("jes-oracle-index-candidate");
        fs::write(root.join("source.txt"), b"accepted source\n").expect("source");
        commit_all(&root, "baseline");

        let baseline = jes_oracle_candidate_digest(&root).expect("baseline candidate");
        fs::write(root.join("unrelated-untracked.txt"), b"not in candidate\n")
            .expect("untracked file");
        assert_eq!(
            jes_oracle_candidate_digest(&root).expect("untracked-insensitive candidate"),
            baseline
        );

        fs::write(root.join("source.txt"), b"staged source\n").expect("source update");
        let error = jes_oracle_candidate_digest(&root)
            .expect_err("unstaged tracked source must invalidate campaign readiness");
        assert!(error.contains("unstaged tracked paths"));
        assert!(
            Command::new("git")
                .args(["add", "source.txt"])
                .current_dir(&root)
                .status()
                .expect("git add")
                .success()
        );
        let staged = jes_oracle_candidate_digest(&root).expect("staged candidate");
        assert_ne!(staged, baseline);

        for relative in JES_ORACLE_DERIVED_PATHS {
            let path = root.join(relative);
            fs::create_dir_all(path.parent().expect("derived parent")).expect("derived directory");
            fs::write(&path, b"pending report\n").expect("derived report");
            assert!(
                Command::new("git")
                    .args(["add", "--", relative])
                    .current_dir(&root)
                    .status()
                    .expect("git add derived report")
                    .success()
            );
        }
        assert_eq!(
            jes_oracle_candidate_digest(&root).expect("derived-report-insensitive candidate"),
            staged
        );
        fs::remove_dir_all(root).expect("temporary repository cleanup");
    }

    #[test]
    fn draft_2020_12_compilation_rejects_invalid_keyword_values() {
        let invalid = json!({
            "$schema":"https://json-schema.org/draft/2020-12/schema",
            "type":"not-a-json-schema-type"
        });
        assert!(compile_draft_2020_12_schema(&invalid, Path::new("invalid.schema.json")).is_err());
    }

    #[test]
    fn compiled_schema_rejects_schema_invalid_artifact() {
        let schema = json!({
            "$schema":"https://json-schema.org/draft/2020-12/schema",
            "type":"object",
            "required":["generation"],
            "properties":{"generation":{"type":"integer","minimum":1}}
        });
        let invalid = json!({"generation":0});
        assert!(validate_schema_instance(&schema, &invalid, Path::new("artifact.json")).is_err());
    }

    #[test]
    fn release_target_is_explicit_unique_and_safely_bounded() {
        assert_eq!(
            explicit_release_target(&["--check".into()]),
            Err("release requires exactly one --target <triple>".into())
        );
        assert_eq!(
            explicit_release_target(&[
                "--target".into(),
                "x86_64-unknown-linux-gnu".into(),
                "--target".into(),
                "aarch64-apple-darwin".into(),
            ]),
            Err("release requires exactly one --target <triple>".into())
        );
        assert_eq!(
            explicit_release_target(&["--target".into(), "../host".into()]),
            Err("release target is invalid".into())
        );
        assert_eq!(
            explicit_release_target(&["--target".into(), "powerpc64-unknown-linux-gnu".into()]),
            Err("release target is invalid".into())
        );
        assert_eq!(
            explicit_release_target(&[
                "--check".into(),
                "--target".into(),
                "x86_64-unknown-linux-gnu".into(),
            ])
            .unwrap(),
            "x86_64-unknown-linux-gnu"
        );
    }

    #[test]
    fn stable_product_versions_are_generic_strict_and_derive_the_release_line() {
        for (version, release_line) in [("0.2.0", "0.2"), ("0.7.0", "0.7"), ("0.17.4", "0.17")] {
            assert!(stable_zero_version(version).is_ok());
            assert_eq!(release_line_for_version(version).unwrap(), release_line);
        }
        for invalid in [
            "1.0.0",
            "0.07.0",
            "0.7.00",
            "0.7",
            "0.7.0-rc.1",
            "0.7.0+build",
            "../0.7.0",
            "0.7.0/targets/forged",
            "0.x.0",
        ] {
            assert!(stable_zero_version(invalid).is_err(), "accepted {invalid}");
        }
    }

    #[test]
    fn post_0_2_release_digest_binds_a_clean_live_source_tree() {
        let root = temporary_git_repository("live-release-source");
        fs::write(root.join("VERSION"), b"0.3.0\n").expect("VERSION");
        fs::write(root.join("source.txt"), b"one\n").expect("source");
        commit_all(&root, "source one");
        let first = live_release_source_digest(&root, "0.3.0").expect("first digest");

        fs::write(root.join("source.txt"), b"two\n").expect("dirty source");
        assert!(live_release_source_digest(&root, "0.3.0").is_err());
        commit_all(&root, "source two");
        let second = live_release_source_digest(&root, "0.3.0").expect("second digest");
        assert_ne!(first, second);

        let receipt = root.join("release/0.3.0/targets/aarch64-apple-darwin/manifest.json");
        fs::create_dir_all(receipt.parent().expect("receipt parent")).expect("release directory");
        fs::write(&receipt, b"generated receipt\n").expect("receipt");
        assert_eq!(
            live_release_source_digest(&root, "0.3.0").expect("receipt is excluded"),
            second
        );

        let historical = root.join("release/0.2.0/targets/aarch64-apple-darwin/manifest.json");
        fs::create_dir_all(historical.parent().expect("historical parent"))
            .expect("historical release directory");
        fs::write(historical, b"forged historical receipt\n").expect("historical receipt");
        assert!(live_release_source_digest(&root, "0.3.0").is_err());
        fs::remove_dir_all(root).expect("temporary repository cleanup");
    }

    #[test]
    fn release_identity_rejects_a_stale_source_digest() {
        let source_digest = format!("sha256:{}", "a".repeat(64));
        let manifest = json!({
            "schema_version":"mainframe-env.release-manifest@1",
            "version":"0.3.0",
            "channel":"stable",
            "tag":"mainframe-env-v0.3.0",
            "target":"aarch64-apple-darwin",
            "published":false
        });
        let sbom = json!({
            "bomFormat":"CycloneDX",
            "specVersion":"1.6",
            "metadata":{"component":{"version":"0.3.0"}}
        });
        let provenance = json!({
            "predicateType":"https://slsa.dev/provenance/v1",
            "predicate":{"buildDefinition":{
                "externalParameters":{"target":"aarch64-apple-darwin"},
                "resolvedDependencies":[{
                    "uri":"source-tree",
                    "digest":{"sha256":"a".repeat(64)}
                }]
            }}
        });
        let build_inputs = json!({
            "schema_version":"mainframe-env.release-build-inputs@1",
            "target":"aarch64-apple-darwin",
            "rustc_verbose":"rustc 1.98.0\nrelease: 1.98.0",
            "cargo_lock_sha256":"b".repeat(64),
            "source_digest":source_digest,
            "deterministic_environment":{"SOURCE_DATE_EPOCH":"0"}
        });
        assert!(
            validate_release_identity(
                &manifest,
                &sbom,
                &provenance,
                &build_inputs,
                "# License expressions\n",
                "aarch64-apple-darwin",
                "0.3.0",
                "stable",
                build_inputs["source_digest"].as_str().unwrap(),
            )
            .is_ok()
        );
        let stale = format!("sha256:{}", "c".repeat(64));
        assert!(
            validate_release_identity(
                &manifest,
                &sbom,
                &provenance,
                &build_inputs,
                "# License expressions\n",
                "aarch64-apple-darwin",
                "0.3.0",
                "stable",
                &stale,
            )
            .is_err()
        );
    }

    #[test]
    fn round_five_tooling_preserves_retained_release_source_digest() {
        let root = repository_root().unwrap();
        let inputs =
            json(&root.join("release/0.2.0/targets/aarch64-apple-darwin/build-inputs.json"))
                .unwrap();
        assert_eq!(
            accepted_0_2_release_source_digest(&root).unwrap(),
            inputs["source_digest"].as_str().unwrap()
        );
    }

    #[test]
    fn release_verification_rejects_missing_binary_toolchain_flag_and_target_drift() {
        let documents = |target: &str, binary: &[u8], inputs: &[u8]| {
            let root = PathBuf::from(format!("release/0.2.0/targets/{target}"));
            BTreeMap::from([
                (root.join("manifest.json"), binary.to_vec()),
                (root.join("build-inputs.json"), inputs.to_vec()),
            ])
        };
        let generated = documents(
            "aarch64-apple-darwin",
            b"expected-binary-digest",
            b"rust=1.98.0;codegen-units=1;incremental=false",
        );
        assert!(compare_release_documents(&BTreeMap::new(), &generated).is_err());
        assert!(
            compare_release_documents(
                &documents(
                    "aarch64-apple-darwin",
                    b"changed-binary-digest",
                    b"rust=1.98.0;codegen-units=1;incremental=false",
                ),
                &generated,
            )
            .is_err()
        );
        assert!(
            compare_release_documents(
                &documents(
                    "aarch64-apple-darwin",
                    b"expected-binary-digest",
                    b"rust=1.97.0;codegen-units=1;incremental=false",
                ),
                &generated,
            )
            .is_err()
        );
        assert!(
            compare_release_documents(
                &documents(
                    "aarch64-apple-darwin",
                    b"expected-binary-digest",
                    b"rust=1.98.0;codegen-units=16;incremental=true",
                ),
                &generated,
            )
            .is_err()
        );
        assert!(
            compare_release_documents(
                &documents(
                    "x86_64-unknown-linux-gnu",
                    b"expected-binary-digest",
                    b"rust=1.98.0;codegen-units=1;incremental=false",
                ),
                &generated,
            )
            .is_err()
        );
    }

    #[test]
    fn completion_discovery_rejects_wrong_duplicate_and_missing_trailers() {
        let subject = "Repair 0.2.0 third review findings";
        let expected = [
            ("Third-Review-Findings", "11=closed"),
            ("Target-Version", "0.2.0"),
            (
                "Evidence-Digest",
                "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            ),
        ];
        let valid = format!(
            "{subject}\n\nThird-Review-Findings: 11=closed\nTarget-Version: 0.2.0\nEvidence-Digest: sha256:{}\n",
            "a".repeat(64)
        );
        assert!(completion_message_matches(&valid, subject, &expected));
        assert!(!completion_message_matches(
            &valid.replace("11=closed", "10=closed"),
            subject,
            &expected
        ));
        assert!(!completion_message_matches(
            &valid.replace("Target-Version: 0.2.0\n", ""),
            subject,
            &expected
        ));
        assert!(!completion_message_matches(
            &format!("{valid}Target-Version: 0.2.0\n"),
            subject,
            &expected
        ));

        let root = temporary_git_repository("ambiguous-completion");
        fs::write(root.join("source"), b"one").expect("source");
        commit_all(&root, &valid);
        fs::write(root.join("source"), b"two").expect("source");
        commit_all(&root, &valid);
        assert!(find_completion_commit(&root, subject, &expected).is_err());
        fs::remove_dir_all(root).expect("temporary repository cleanup");
    }

    #[test]
    fn commit_bound_evidence_survives_later_source_and_rejects_substitution() {
        let root = temporary_git_repository("commit-bound-evidence");
        fs::write(root.join("source.txt"), b"accepted source\n").expect("source");
        let source = commit_all(&root, "Source");
        let receipt = json!({"receipt":{"source_identity":source,"result":"pass"}});
        fs::write(
            root.join("receipt.json"),
            pretty_json(&receipt).expect("receipt bytes"),
        )
        .expect("receipt");
        let completion = commit_all(&root, "Completion");
        let bound_digest = repository_digest_at_commit(&root, &completion).expect("bound digest");
        assert!(verify_completion_parent(&root, &completion, &source).is_ok());
        assert!(verify_completion_parent(&root, &completion, &completion).is_err());
        assert!(
            verify_commit_bound_live_file(
                &root,
                &completion,
                "receipt.json",
                &root.join("receipt.json")
            )
            .is_ok()
        );

        fs::write(root.join("later.txt"), b"unrelated later source\n").expect("later source");
        commit_all(&root, "Later unrelated source");
        assert_eq!(
            repository_digest_at_commit(&root, &completion).expect("stable historical digest"),
            bound_digest
        );
        assert_ne!(repository_digest(&root).expect("live digest"), bound_digest);

        let original = git_file_bytes(&root, &completion, "receipt.json").expect("Git receipt");
        fs::write(
            root.join("receipt.json"),
            pretty_json(&json!({"receipt":{"source_identity":source,"result":"rewritten"}}))
                .expect("rewritten receipt"),
        )
        .expect("live substitution");
        assert!(
            verify_commit_bound_live_file(
                &root,
                &completion,
                "receipt.json",
                &root.join("receipt.json")
            )
            .is_err()
        );
        assert_ne!(
            fs::read(root.join("receipt.json")).expect("live receipt"),
            original
        );
        assert_ne!(
            format!("sha256:{:x}", Sha256::digest(&original)),
            format!(
                "sha256:{:x}",
                Sha256::digest(fs::read(root.join("receipt.json")).expect("rewritten receipt"))
            )
        );
        fs::remove_dir_all(root).expect("temporary repository cleanup");
    }

    #[test]
    fn accepted_0_2_candidate_receipts_survive_later_versions() {
        let root = repository_root().expect("repository root");
        let accepted = accepted_0_2_candidate_digest(&root).expect("accepted 0.2 candidate");
        let status = json(&root.join("conformance/0.2/evidence/program-status.json"))
            .expect("0.2 program status");
        let regression = json(&root.join("conformance/0.2/evidence/full-regression.json"))
            .expect("0.2 full regression");
        assert_eq!(status["dirty_tree_identity"]["digest"], accepted);
        assert_eq!(regression["candidate"]["source_digest"], accepted);
        assert_ne!(
            repository_digest(&root).expect("later live digest"),
            accepted
        );
        check_work_package_amendments(&root)
            .expect("0.2 amendments remain bound to accepted candidate");
        assert_eq!(
            retained_accepted_release(&root).expect("accepted release mode"),
            (product_version(&root).unwrap() == "0.2.0")
                .then(|| "e8dfa89583d866a365f496297d50aeb602e468bf".into())
        );
        validate_historical_0_2_release(&root)
            .expect("both retained targets remain byte-bound to accepted 0.2");
    }

    #[test]
    fn fast_spec_gate_rejects_same_count_catalog_mutation_under_stale_index() {
        let root = temporary_git_repository("catalog-closure-drift");
        let catalog_relative = "conformance/0.2/catalogs/mock.json";
        let catalog_path = root.join(catalog_relative);
        fs::create_dir_all(catalog_path.parent().expect("catalog parent"))
            .expect("catalog directory");
        fs::write(&catalog_path, br#"{"rows":[{"label":"one"}]}"#).expect("catalog");
        let expected = format!(
            "sha256:{}",
            file_digest(&catalog_path).expect("catalog digest")
        );
        let index_path = root.join("conformance/0.2/catalogs/index.json");
        let index = json!({
            "baselines": [{
                "subsystem": "mock",
                "catalog": catalog_relative,
                "catalog_sha256": expected
            }]
        });
        assert!(indexed_catalog_closure(&root, &index, &index_path).is_ok());

        // The row count and byte length remain unchanged; only closure hashing
        // can detect this catalog substitution under the stale index.
        fs::write(&catalog_path, br#"{"rows":[{"label":"two"}]}"#).expect("mutated catalog");
        assert!(indexed_catalog_closure(&root, &index, &index_path).is_err());
        fs::remove_dir_all(root).expect("temporary repository cleanup");
    }

    #[test]
    fn forbidden_core_infrastructure_edge_is_rejected() {
        let excluded = BTreeSet::new();
        assert!(check_dependency("mainframe-env-source", "tokio", &excluded).is_err());
    }

    #[test]
    fn excluded_component_edge_is_rejected() {
        let excluded = BTreeSet::from(["open-mainframe-db2".to_string()]);
        assert!(check_dependency("mainframe-env-server", "open-mainframe-db2", &excluded).is_err());
    }

    #[test]
    fn allowed_tooling_edge_is_accepted() {
        let excluded = BTreeSet::new();
        assert!(check_dependency("xtask", "serde_json", &excluded).is_ok());
        assert!(check_dependency("xtask", "mainframe-env-conformance", &excluded).is_ok());
    }

    #[test]
    fn reverse_internal_dependency_is_rejected() {
        let excluded = BTreeSet::new();
        assert!(
            check_dependency(
                "mainframe-env-source",
                "mainframe-env-compiler-api",
                &excluded,
            )
            .is_err()
        );
    }

    #[test]
    fn canonical_maps_have_stable_key_order() {
        let map = std::collections::BTreeMap::from([("b", 2), ("a", 1)]);
        let keys: Vec<_> = map.keys().copied().collect();
        assert_eq!(keys, ["a", "b"]);
    }

    #[test]
    fn contradictory_workload_and_program_status_ledgers_are_rejected() {
        let workload = json!({
            "schema_version":"mainframe-env.coverage-workload-ledger@1",
            "target_version":"0.2.0",
            "implementation_status":"in-progress",
            "current_work_package":"CV-209",
            "work_packages":[{"id":"CV-209","state":"in-progress","evidence":null}],
            "exit_gates":[]
        });
        let status = json!({
            "schema_version":"mainframe-env.coverage-program-status@1",
            "target_version":"0.2.0",
            "implementation_status":"in-progress",
            "current_work_package":"CV-209",
            "work_packages":[{"id":"CV-209","state":"in-progress","evidence":null}]
        });
        assert!(compare_workload_and_program_status(&workload, &status).is_ok());
        let mut contradiction = status.clone();
        contradiction["work_packages"][0]["state"] = Value::String("pass".into());
        assert!(compare_workload_and_program_status(&workload, &contradiction).is_err());

        let mut false_complete = workload;
        false_complete["implementation_status"] = Value::String("complete".into());
        let mut false_complete_status = status;
        false_complete_status["implementation_status"] = Value::String("complete".into());
        assert!(
            compare_workload_and_program_status(&false_complete, &false_complete_status).is_err()
        );
    }
}
