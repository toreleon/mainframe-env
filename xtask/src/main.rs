//! Deterministic repository checks for mainframe-env.

#![forbid(unsafe_code)]

mod license_notices_cli;

#[cfg(test)]
mod carddemo_base_batch_provenance;
mod carddemo_host_integration;
mod carddemo_readacct;
mod carddemo_serve;
mod changelog;
mod cics_system_families;
mod cobol_differential;
mod common_program_policy;
mod conformance_catalog;
mod conformance_spec_export;
mod coverage_projection;
mod db2_statement_catalog;
mod dependency_licenses;
mod docs;
mod ims_assurance_matrix;
mod ims_catalog;
mod ims_conformance;
mod jcl_catalog;
mod jcl_conformance;
mod mq_conformance;
mod mq_status_catalog;
#[cfg(test)]
mod package_schema_tests;
mod production_scanner;
mod profile_intake;
mod racf_catalog;
#[cfg(test)]
mod serialized_coverage_schema_tests;
mod topic_manifests;
mod work_package_seal;
mod zosmf_contracts;

use clap::{Args, CommandFactory, Parser, Subcommand};
#[cfg(test)]
use conformance_catalog::indexed_catalog_closure;
use conformance_catalog::official_catalog_rows;
use conformance_spec_export::compile_shared_spec;
use mainframe_env_conformance::{
    CicsOracleExpectation, CicsOracleImport, CicsOracleObservation, CicsPilotRuntime,
    CobolArithmeticPilotRuntime, CobolMovePilotRuntime, DatasetConformanceRuntime,
    OracleCandidateExpectation, OracleHarnessValidationKind, RACF_ORACLE_RELATIVE_PATH,
    RacfOracleCampaign, cics_pilot_runtime, cobol_arithmetic_pilot_runtime,
    cobol_move_pilot_runtime, dataset_conformance_runtime, gnucobol_reference_fixture_digest,
    import_cics_oracle_capture, licensed_fixture_digest, run_dataset_reference_simulation,
    run_gnucobol_reference_campaign, validate_oracle_harness_receipt,
    validate_oracle_harness_registry, verify_carddemo_application_package_from_env,
    verify_carddemo_base_batch_from_env, verify_carddemo_base_online_from_env,
    verify_carddemo_batch_programs_from_env, verify_carddemo_cics_abi_from_env,
    verify_carddemo_cics_runtime_from_env, verify_carddemo_control_flow_from_env,
    verify_carddemo_core_semantics_from_env, verify_carddemo_corpus_from_env,
    verify_carddemo_data_layouts_from_env, verify_carddemo_dataset_catalog_from_env,
    verify_carddemo_db2_from_env, verify_carddemo_file_call_semantics_from_env,
    verify_carddemo_full_from_env, verify_carddemo_host_operands_from_env,
    verify_carddemo_ims_from_env, verify_carddemo_jcl_from_env,
    verify_carddemo_mq_authorization_from_env, verify_carddemo_program_routing_from_env,
    verify_carddemo_resources_from_env, verify_carddemo_security_from_env,
    verify_carddemo_seeds_from_env, verify_carddemo_source_closures_from_env,
    verify_carddemo_source_preprocessing_from_env, verify_carddemo_terminal_from_env,
    verify_carddemo_utilities_from_env, verify_carddemo_vsam_from_env,
    verify_cobol_assurance_sources, verify_cobol_condition_fixtures,
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
use mq_conformance::run_focused_mq;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::ffi::OsStr;
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

type TaskResult<T = ()> = Result<T, String>;

const RETAINED_RUNTIME_TARGETS: [&str; 2] = ["aarch64-apple-darwin", "x86_64-unknown-linux-gnu"];
const JES_ORACLE_CANDIDATE_DOMAIN: &[u8] = b"mainframe-env.jes-oracle-candidate@1\0";
const JES_ORACLE_DERIVED_PATHS: [&str; 1] = ["docs/delivery/subsystems/jes/execution-status.md"];

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
struct ProfileIntakeArgs {
    #[arg(long)]
    manifest: PathBuf,
    #[arg(long)]
    corpus: PathBuf,
    #[arg(long)]
    json: PathBuf,
    #[arg(long)]
    markdown: PathBuf,
}

#[derive(Debug, Args)]
struct ConformanceArgs {
    #[arg(long)]
    subsystem: Option<String>,
    #[arg(
        long,
        value_name = "GATE",
        help = "One coverage gate, or local for all non-differential gates"
    )]
    gate: Option<String>,
    #[arg(long)]
    shard: Option<u16>,
    #[arg(long)]
    replay: Option<String>,
    #[arg(
        long,
        help = "Run noncredit candidate preparation; never emit official verdicts"
    )]
    prepare_candidates: bool,
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
struct CicsOracleArgs {
    #[arg(long)]
    capture: PathBuf,
    #[arg(long, help = "Sealed family ID; omit for the existing file/UOW pilot")]
    family: Option<String>,
    #[arg(long, help = "Exact external environment JSON for a family capture")]
    environment_manifest: Option<PathBuf>,
    #[arg(
        long,
        help = "Optional raw 32-byte Ed25519 public key from the protected runner"
    )]
    public_key: Option<PathBuf>,
}

#[derive(Debug, Args)]
struct WorkPackageSealArgs {
    #[arg(long)]
    id: String,
    #[arg(long)]
    target_subsystem: String,
    #[arg(long = "path", required = true)]
    paths: Vec<String>,
    #[arg(long)]
    check: bool,
}

#[derive(Debug, Args)]
struct CicsSystemFamilyArgs {
    #[arg(long)]
    family: Option<String>,
    #[arg(long)]
    check: bool,
}

#[derive(Debug, Subcommand)]
enum XtaskCommand {
    ProfileIntake(ProfileIntakeArgs),

    Changelog(CheckArgs),
    Docs(CheckArgs),
    Subsystems(CheckArgs),
    Architecture(CheckArgs),
    ArchitectureFast(CheckArgs),

    RuntimeArchitecture(CheckArgs),
    Profiles(CheckArgs),
    Schemas(CheckArgs),
    CicsSystemFamilies(CicsSystemFamilyArgs),
    Inventory(CheckArgs),
    MqMqiRegistry(CheckArgs),
    MqLicensedContract(CheckArgs),
    ImsLicensedContract(CheckArgs),

    Coverage(CheckArgs),
    ApplicationPackages(CheckArgs),
    Db2Catalog(CheckArgs),
    Db2StatementCatalog(CheckArgs),
    BatchControllers(CheckArgs),
    AbiLibraries(CheckArgs),
    ProgramRegistry(CheckArgs),
    RouteRegistries(CheckArgs),
    ZosmfContracts(CheckArgs),
    Dehardcoding(CheckArgs),

    SemanticIdentities(CheckArgs),
    DatasetContract(CheckArgs),
    DatasetOracle(CheckArgs),
    JesOracle(CheckArgs),
    JesOracleCandidate,
    LicensedHarness(CheckArgs),
    CobolLanguage(CheckArgs),
    CobolExit(CheckArgs),
    CobolReference(CobolReferenceArgs),
    CobolDifferential(cobol_differential::Args),
    CicsOracle(CicsOracleArgs),
    JclCatalog(CheckArgs),
    JclConformance(CheckArgs),
    JclExit(CheckArgs),
    ImsCatalog(CheckArgs),
    ImsAssuranceMatrix(CheckArgs),
    RacfCatalog(CheckArgs),
    Spec(CheckArgs),
    ConformanceSpecExport,
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
    #[command(name = "carddemo-host-integration")]
    CarddemoHostIntegration(CheckArgs),
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
    /// Start the real CardDemo online application with a browser terminal.
    CarddemoServe(carddemo_serve::ServeArgs),
    CarddemoJcl(CheckArgs),
    CarddemoUtilities(CheckArgs),
    CarddemoBatchPrograms(CheckArgs),
    CarddemoBaseBatch(CheckArgs),
    CarddemoReadacct(CheckArgs),
    CarddemoDb2(CheckArgs),
    CarddemoIms(CheckArgs),
    CarddemoMqAuthorization(CheckArgs),
    CarddemoOperatorInstall(CheckArgs),
    CarddemoOperatorCompile(CheckArgs),
    CarddemoOperatorSubmit(CheckArgs),
    CarddemoOperatorReset(CheckArgs),
    CarddemoFull(CheckArgs),
    LicenseNotices(license_notices_cli::NoticeArgs),
    Digest(CheckArgs),
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
        XtaskCommand::ProfileIntake(args) => {
            ("profile-intake", false, profile_intake::run(root, &args))
        }

        XtaskCommand::Changelog(args) => checked!(
            "changelog",
            args,
            changelog::run(root, args.check).and_then(|()| {
                if args.check {
                    Ok(())
                } else {
                    docs::run(root, false, &Cli::command())
                }
            })
        ),
        XtaskCommand::Docs(args) => checked!(
            "docs",
            args,
            changelog::validate(root).and_then(|()| docs::run(root, args.check, &Cli::command()))
        ),
        XtaskCommand::Subsystems(args) => {
            checked!("subsystems", args, docs::run(root, true, &Cli::command()))
        }
        XtaskCommand::ArchitectureFast(args) => {
            checked!("architecture-fast", args, check_architecture_fast(root))
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
        XtaskCommand::CicsSystemFamilies(args) => checked!(
            "cics-system-families",
            args,
            cics_system_families::check(root, args.family.as_deref())
        ),
        XtaskCommand::Inventory(args) => checked!("inventory", args, check_inventory(root)),
        XtaskCommand::MqMqiRegistry(args) => {
            checked!("mq-mqi-registry", args, check_mq_mqi_registry(root))
        }
        XtaskCommand::MqLicensedContract(args) => {
            checked!(
                "mq-licensed-contract",
                args,
                check_mq_licensed_contract(root)
            )
        }
        XtaskCommand::ImsLicensedContract(args) => {
            checked!(
                "ims-licensed-contract",
                args,
                check_ims_licensed_contract(root)
            )
        }

        XtaskCommand::Coverage(args) => checked!("coverage", args, check_coverage(root)),
        XtaskCommand::ApplicationPackages(args) => checked!(
            "application-packages",
            args,
            check_application_packages(root)
        ),
        XtaskCommand::Db2Catalog(args) => checked!("db2-catalog", args, check_db2_catalog(root)),
        XtaskCommand::Db2StatementCatalog(args) => checked!(
            "db2-statement-catalog",
            args,
            if args.check {
                db2_statement_catalog::check(root)
            } else {
                db2_statement_catalog::generate(root)
            }
        ),
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
        XtaskCommand::ZosmfContracts(args) => checked!(
            "zosmf-contracts",
            args,
            zosmf_contracts::run(root, args.check)
        ),
        XtaskCommand::Dehardcoding(args) => {
            checked!("dehardcoding", args, check_dehardcoding(root))
        }

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
        XtaskCommand::LicensedHarness(args) => {
            checked!("licensed-harness", args, check_licensed_harness(root))
        }
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
        XtaskCommand::CobolDifferential(args) => (
            "cobol-differential",
            args.check,
            cobol_differential::run(root, &args),
        ),
        XtaskCommand::CicsOracle(args) => (
            "cics-oracle",
            false,
            import_cics_oracle(
                root,
                &args.capture,
                args.family.as_deref(),
                args.environment_manifest.as_deref(),
                args.public_key.as_deref(),
            ),
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
        XtaskCommand::ImsCatalog(args) => checked!(
            "ims-catalog",
            args,
            if args.check {
                ims_catalog::check(root)
            } else {
                ims_catalog::generate(root)
            }
        ),
        XtaskCommand::ImsAssuranceMatrix(args) => {
            checked!(
                "ims-assurance-matrix",
                args,
                ims_assurance_matrix::check(root)
            )
        }
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
        XtaskCommand::ConformanceSpecExport => (
            "conformance-spec-export",
            false,
            conformance_spec_export::run(root),
        ),
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
        XtaskCommand::CarddemoHostIntegration(args) => {
            checked!(
                "carddemo-host-integration",
                args,
                carddemo_host_integration::check(root)
            )
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
        XtaskCommand::CarddemoServe(args) => {
            ("carddemo-serve", false, carddemo_serve::run(root, args))
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
        XtaskCommand::CarddemoReadacct(args) => {
            checked!(
                "carddemo-readacct",
                args,
                carddemo_readacct::run(root, args.check)
            )
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
        XtaskCommand::LicenseNotices(args) => {
            checked!(
                "license-notices",
                args,
                license_notices_cli::run(root, &args)
            )
        }
        XtaskCommand::Digest(args) => checked!("digest", args, print_digest(root)),
        XtaskCommand::WorkPackageSeal(args) => (
            "work-package-seal",
            args.check,
            work_package_seal::run(root, &args),
        ),
    }
}

fn check_conformance(root: &Path) -> TaskResult {
    check_spec(root)?;
    ims_catalog::check(root)?;
    ims_assurance_matrix::check(root)?;
    racf_catalog::check(root)?;
    jcl_catalog::check(root)?;
    jcl_conformance::check(root)?;

    check_architecture(root)?;
    check_profiles(root)?;
    check_schemas(root)?;
    check_inventory(root)?;

    check_coverage(root)?;
    check_semantic_identities(root)?;
    check_application_packages(root)?;
    check_db2_catalog(root)?;
    db2_statement_catalog::check(root)?;
    check_batch_controllers(root)?;
    check_host_abi_libraries(root)?;
    check_program_registry(root)?;
    check_route_registries(root)?;

    Ok(())
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
                "cobol-differential-receipt.schema.json",
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
                "conformance-candidate.schema.json",
                "conformance-spec.schema.json",
                "ims-db-fixtures.schema.json",
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
        root.join("conformance/subsystems/cobol/execution/cobol/gnucobol-reference-allowlist.json");
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
    let cobol_oracle_path = root
        .join("conformance/subsystems/cobol/execution/oracles/cobol-licensed-differential.json");
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
    let inventory_path =
        root.join("conformance/subsystems/cobol/structure/inventory/dependency-additions.json");
    let inventory_schema_path = schema_directory.join("conformance-inventory.schema.json");
    validate_schema_instance(
        &json(&inventory_schema_path)?,
        &json(&inventory_path)?,
        &inventory_path,
    )?;
    let cobol_path = root.join("conformance/subsystems/cobol/structure/cobol/language.json");
    let cobol_schema_path = schema_directory.join("cobol-language.schema.json");
    validate_schema_instance(&json(&cobol_schema_path)?, &json(&cobol_path)?, &cobol_path)?;
    check_cobol_language_catalog(root, &cobol_path)?;
    let frontend_path =
        root.join("conformance/subsystems/cobol/structure/cobol/frontend-fixtures.json");
    let frontend_schema_path = schema_directory.join("cobol-frontend-fixtures.schema.json");
    validate_schema_instance(
        &json(&frontend_schema_path)?,
        &json(&frontend_path)?,
        &frontend_path,
    )?;
    verify_cobol_frontend_fixtures()?;
    let semantic_path =
        root.join("conformance/subsystems/cobol/structure/cobol/semantic-fixtures.json");
    let semantic_schema_path = schema_directory.join("cobol-semantic-fixtures.schema.json");
    validate_schema_instance(
        &json(&semantic_schema_path)?,
        &json(&semantic_path)?,
        &semantic_path,
    )?;
    verify_cobol_semantic_fixtures()?;
    let statement_path =
        root.join("conformance/subsystems/cobol/structure/cobol/statement-fixtures.json");
    let statement_schema_path = schema_directory.join("cobol-statement-fixtures.schema.json");
    validate_schema_instance(
        &json(&statement_schema_path)?,
        &json(&statement_path)?,
        &statement_path,
    )?;
    verify_cobol_statement_fixtures()?;
    let statement_runtime_path =
        root.join("conformance/subsystems/cobol/execution/cobol/statement-runtime-fixtures.json");
    let statement_runtime_schema_path =
        schema_directory.join("cobol-statement-runtime-fixtures.schema.json");
    validate_schema_instance(
        &json(&statement_runtime_schema_path)?,
        &json(&statement_runtime_path)?,
        &statement_runtime_path,
    )?;
    verify_cobol_statement_runtime_fixtures()?;
    let statement_phrase_path = root.join(
        "conformance/subsystems/cobol/execution/cobol/statement-phrase-runtime-fixtures.json",
    );
    let statement_phrase_schema_path =
        schema_directory.join("cobol-statement-phrase-runtime-fixtures.schema.json");
    validate_schema_instance(
        &json(&statement_phrase_schema_path)?,
        &json(&statement_phrase_path)?,
        &statement_phrase_path,
    )?;
    verify_cobol_statement_phrase_runtime_fixtures()?;
    let data_runtime_path =
        root.join("conformance/subsystems/cobol/execution/cobol/data-runtime-fixtures.json");
    let data_runtime_schema_path = schema_directory.join("cobol-data-runtime-fixtures.schema.json");
    validate_schema_instance(
        &json(&data_runtime_schema_path)?,
        &json(&data_runtime_path)?,
        &data_runtime_path,
    )?;
    verify_cobol_data_runtime_fixtures()?;
    let register_runtime_path =
        root.join("conformance/subsystems/cobol/execution/cobol/register-runtime-fixtures.json");
    let register_runtime_schema_path =
        schema_directory.join("cobol-register-runtime-fixtures.schema.json");
    validate_schema_instance(
        &json(&register_runtime_schema_path)?,
        &json(&register_runtime_path)?,
        &register_runtime_path,
    )?;
    verify_cobol_register_runtime_fixtures()?;
    let file_runtime_path =
        root.join("conformance/subsystems/cobol/execution/cobol/file-runtime-fixtures.json");
    let file_runtime_schema_path = schema_directory.join("cobol-file-runtime-fixtures.schema.json");
    validate_schema_instance(
        &json(&file_runtime_schema_path)?,
        &json(&file_runtime_path)?,
        &file_runtime_path,
    )?;
    verify_cobol_file_runtime_fixtures()?;
    let recovery_path =
        root.join("conformance/subsystems/cobol/execution/cobol/recovery-fixtures.json");
    let recovery_schema_path = schema_directory.join("cobol-recovery-fixtures.schema.json");
    validate_schema_instance(
        &json(&recovery_schema_path)?,
        &json(&recovery_path)?,
        &recovery_path,
    )?;
    verify_cobol_recovery_fixtures()?;
    let condition_path =
        root.join("conformance/subsystems/cobol/execution/cobol/condition-fixtures.json");
    let condition_schema_path = schema_directory.join("cobol-condition-fixtures.schema.json");
    validate_schema_instance(
        &json(&condition_schema_path)?,
        &json(&condition_path)?,
        &condition_path,
    )?;
    verify_cobol_condition_fixtures()?;
    let function_runtime_path =
        root.join("conformance/subsystems/cobol/execution/cobol/function-runtime-fixtures.json");
    let function_runtime_schema_path =
        schema_directory.join("cobol-function-runtime-fixtures.schema.json");
    validate_schema_instance(
        &json(&function_runtime_schema_path)?,
        &json(&function_runtime_path)?,
        &function_runtime_path,
    )?;
    verify_cobol_function_runtime_fixtures()?;
    let function_boundary_path = root.join(
        "conformance/subsystems/cobol/execution/cobol/function-boundary-runtime-fixtures.json",
    );
    let function_boundary_schema_path =
        schema_directory.join("cobol-function-boundary-runtime-fixtures.schema.json");
    validate_schema_instance(
        &json(&function_boundary_schema_path)?,
        &json(&function_boundary_path)?,
        &function_boundary_path,
    )?;
    verify_cobol_function_boundary_runtime_fixtures()?;
    verify_cobol_assurance_sources()?;
    let function_path =
        root.join("conformance/subsystems/cobol/structure/cobol/function-fixtures.json");
    let function_schema_path = schema_directory.join("cobol-function-fixtures.schema.json");
    validate_schema_instance(
        &json(&function_schema_path)?,
        &json(&function_path)?,
        &function_path,
    )?;
    verify_cobol_function_fixtures()?;
    check_cobol_language_generated(root)?;
    let spec = compile_shared_spec(root)?;
    check_cics_pilot_inputs(root, &spec)?;
    ims_conformance::check(root, &spec)?;
    check_cobol_move_pilot_inputs(root, &spec)?;
    check_cobol_arithmetic_pilot_inputs(root, &spec)?;
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

fn check_cics_pilot_inputs(root: &Path, spec: &CompiledSpec) -> TaskResult {
    let manifest_path =
        root.join("conformance/subsystems/cics/application/manifests/cics-file-uow-topics.json");
    if !manifest_path.is_file() {
        return Ok(());
    }
    let manifest = json(&manifest_path)?;
    let topics = array(&manifest, "topics", &manifest_path)?;
    require(
        topics.len() == 9
            && manifest["topic_count"].as_u64() == Some(9)
            && topics
                .iter()
                .map(|topic| topic["bytes"].as_u64().unwrap_or(0))
                .sum::<u64>()
                == manifest["total_bytes"].as_u64().unwrap_or(u64::MAX),
        "CICS pilot topic manifest is incomplete",
    )?;
    let mut lines = topics
        .iter()
        .map(|topic| {
            Ok(format!(
                "{} {}\n",
                text(topic, "topic_path", &manifest_path)?,
                text(topic, "sha256", &manifest_path)?
            ))
        })
        .collect::<TaskResult<Vec<_>>>()?;
    lines.sort();
    let manifest_digest = format!("{:x}", Sha256::digest(lines.concat().as_bytes()));
    require(
        manifest["topic_manifest_digest"].as_str() == Some(&manifest_digest),
        "CICS pilot topic-manifest digest drifted",
    )?;

    let candidates_path = root.join(
        "conformance/subsystems/cics/application/generated/cics-pilot-semantic-candidates.json",
    );
    let review_path =
        root.join("conformance/subsystems/cics/application/cics/pilot-rule-review.json");
    let environment_path =
        root.join("conformance/subsystems/cics/application/cics/pilot-environment.json");
    let fixture_path =
        root.join("conformance/subsystems/cics/application/cics/pilot-fixtures.json");
    let adapter_path = root
        .join("conformance/subsystems/cics/application/oracles/cics-licensed-differential.json");
    let candidates = json(&candidates_path)?;
    let review = json(&review_path)?;
    let environment = json(&environment_path)?;
    let fixture = json(&fixture_path)?;
    let adapter = json(&adapter_path)?;
    let adapter_schema_path =
        root.join("conformance/subsystems/cics/application/schemas/cics-licensed-differential-adapter.schema.json");
    validate_schema_instance(&json(&adapter_schema_path)?, &adapter, &adapter_path)?;
    require(
        candidates["coverage_credit"].as_u64() == Some(0)
            && candidates["retained_publication_bytes"].as_bool() == Some(false)
            && candidates["topic_manifest_digest"] == manifest["topic_manifest_digest"],
        "CICS pilot candidate evidence boundary drifted",
    )?;
    let expected_projection = format!("sha256:{}", file_digest(&candidates_path)?);
    require(
        review["candidate_projection_sha256"].as_str() == Some(&expected_projection),
        "CICS pilot rule review is stale against its candidate projection",
    )?;
    let inventory = array(&candidates, "inventory", &candidates_path)?;
    let totals = &candidates["totals"];
    require(
        totals["normative_fragments"].as_u64() == Some(inventory.len() as u64)
            && totals["normative_fragments"]
                == review["inventory_disposition"]["normative_fragments"]
            && totals["candidates"] == review["inventory_disposition"]["candidate_fragments"]
            && totals["unsupported"] == review["inventory_disposition"]["unsupported_fragments"]
            && totals["conflicting"] == review["inventory_disposition"]["conflicting_fragments"]
            && totals["outside_scope"]
                == review["inventory_disposition"]["outside_scope_fragments"]
            && totals["informative"] == review["inventory_disposition"]["informative_fragments"],
        "CICS pilot source-fragment review counts are stale",
    )?;
    let candidate_rules = array(&candidates, "candidates", &candidates_path)?
        .iter()
        .flat_map(|candidate| {
            candidate["rule_ids"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(str::to_string)
        })
        .collect::<BTreeSet<_>>();
    let decisions = array(&review, "decisions", &review_path)?;
    let reviewed_rules = decisions
        .iter()
        .map(|decision| text(decision, "rule_id", &review_path).map(str::to_string))
        .collect::<TaskResult<BTreeSet<_>>>()?;
    require(
        candidate_rules == reviewed_rules
            && decisions.iter().all(|decision| {
                matches!(
                    decision["decision"].as_str(),
                    Some("proposed-accept" | "accepted" | "defer-pending")
                )
            }),
        "CICS pilot candidate decisions are incomplete or unknown",
    )?;
    require(
        environment["profiles"]
            .as_array()
            .is_some_and(|profiles| profiles.len() == 2)
            && environment["resource"]["recoverable"].as_bool() == Some(true)
            && fixture["comparison_policy"]["normalizable_fields"]
                .as_array()
                .is_some_and(Vec::is_empty),
        "CICS pilot environment or independent comparison policy drifted",
    )?;
    let durable_profiles = environment["profiles"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|profile| profile["durable_restart_credit"] == true)
        .filter_map(|profile| profile["id"].as_str())
        .collect::<BTreeSet<_>>();
    let fixture_durable_profiles = fixture["expected"]["durable_restart"]["claimed_profiles"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect::<BTreeSet<_>>();
    require(
        durable_profiles == BTreeSet::from(["sqlite"])
            && fixture_durable_profiles == durable_profiles
            && fixture["expected"]["closed_unenabled"]["precondition"]["open_status"] == "CLOSED"
            && fixture["expected"]["closed_unenabled"]["precondition"]["enable_status"]
                == "UNENABLED"
            && fixture["expected"]["closed_enabled"]["precondition"]["enable_status"] == "ENABLED",
        "CICS closed-file or durable-profile applicability drifted",
    )?;
    let adapter_scenarios = adapter["required_scenarios"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect::<BTreeSet<_>>();
    let licensed_fixture_scenarios = fixture["licensed_expected_observations"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|observation| observation["scenario_id"].as_str())
        .collect::<BTreeSet<_>>();
    require(
        adapter["schema_version"] == "mainframe-env.cics-licensed-differential-adapter@1"
            && adapter["required_scenarios"]
                .as_array()
                .is_some_and(|scenarios| scenarios.len() == 12)
            && adapter_scenarios == licensed_fixture_scenarios
            && adapter["origin_policy"]["trusted_authority_id"] == "ibm-cics-protected-runner"
            && adapter["origin_policy"]["self_declared_origin_is_evidence"] == false
            && adapter["licensed_campaign_status"] == "not-run"
            && adapter["differential_gate"] == "pending",
        "CICS licensed adapter readiness or pending boundary drifted",
    )?;
    let cics_promoted = spec.rows().any(|row| {
        spec.catalog_row(row.row_id())
            .is_some_and(|catalog| catalog.subsystem() == "cics")
    });
    match review["review_status"].as_str() {
        Some("pending-maintainer") => require(
            !cics_promoted,
            "unreviewed CICS candidates were promoted into Conformance IR",
        )?,
        Some("accepted") => require(
            cics_promoted
                && decisions.iter().all(|decision| {
                    decision["decision"] == "accepted" || decision["decision"] == "defer-pending"
                }),
            "accepted CICS review is not fully promoted or dispositioned",
        )?,
        _ => return Err("CICS pilot review status is unknown".into()),
    }
    let tests = root.join("conformance/subsystems/cics/application/tools/tests");
    let status = Command::new("python3")
        .args(["-B", "-m", "unittest", "discover", "-s"])
        .arg(&tests)
        .args(["-p", "test_*.py"])
        .current_dir(root)
        .status()
        .map_err(|error| format!("CICS pilot extractor tests: {error}"))?;
    require(status.success(), "CICS pilot extractor tests failed")
}

fn check_cobol_move_pilot_inputs(root: &Path, spec: &CompiledSpec) -> TaskResult {
    let manifest_path = root
        .join("conformance/subsystems/cics/application/manifests/cobol-numeric-move-topics.json");
    if !manifest_path.is_file() {
        return Ok(());
    }
    let manifest = json(&manifest_path)?;
    let topics = array(&manifest, "topics", &manifest_path)?;
    require(
        topics.len() == 6
            && manifest["topic_count"].as_u64() == Some(6)
            && topics
                .iter()
                .map(|topic| topic["bytes"].as_u64().unwrap_or(0))
                .sum::<u64>()
                == manifest["total_bytes"].as_u64().unwrap_or(u64::MAX),
        "COBOL MOVE pilot topic manifest is incomplete",
    )?;
    let mut lines = topics
        .iter()
        .map(|topic| {
            Ok(format!(
                "{} {}\n",
                text(topic, "topic_path", &manifest_path)?,
                text(topic, "sha256", &manifest_path)?
            ))
        })
        .collect::<TaskResult<Vec<_>>>()?;
    lines.sort();
    let manifest_digest = format!("{:x}", Sha256::digest(lines.concat().as_bytes()));
    require(
        manifest["topic_manifest_digest"].as_str() == Some(&manifest_digest),
        "COBOL MOVE pilot topic-manifest digest drifted",
    )?;

    let projection_path = root.join(
        "conformance/subsystems/cics/application/generated/cobol-move-semantic-candidates.json",
    );
    let review_path =
        root.join("conformance/subsystems/cics/application/cobol/move-rule-review.json");
    let fixture_path = root.join("conformance/subsystems/cics/application/cobol/move-fixture.json");
    let projection = json(&projection_path)?;
    let review = json(&review_path)?;
    let fixture = json(&fixture_path)?;
    require(
        projection["coverage_credit"].as_u64() == Some(0)
            && projection["retained_publication_bytes"].as_bool() == Some(false)
            && projection["topic_manifest_digest"] == manifest["topic_manifest_digest"],
        "COBOL MOVE candidate evidence boundary drifted",
    )?;
    require(
        review["candidate_projection_sha256"].as_str()
            == Some(&format!("sha256:{}", file_digest(&projection_path)?)),
        "COBOL MOVE rule review is stale against its projection",
    )?;
    let totals = &projection["totals"];
    let inventory = array(&projection, "inventory", &projection_path)?;
    require(
        totals["normative_fragments"].as_u64() == Some(inventory.len() as u64)
            && totals["normative_fragments"]
                == review["inventory_disposition"]["normative_fragments"]
            && totals["candidates"] == review["inventory_disposition"]["candidate_fragments"]
            && totals["outside_scope"]
                == review["inventory_disposition"]["outside_scope_fragments"]
            && totals["unsupported"] == review["inventory_disposition"]["unsupported_fragments"]
            && totals["conflicting"] == review["inventory_disposition"]["conflicting_fragments"],
        "COBOL MOVE source-fragment review counts are stale",
    )?;
    let candidate_rules = array(&projection, "candidates", &projection_path)?
        .iter()
        .flat_map(|candidate| {
            candidate["rule_ids"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(str::to_string)
        })
        .collect::<BTreeSet<_>>();
    let decisions = array(&review, "decisions", &review_path)?;
    let reviewed = decisions
        .iter()
        .map(|decision| text(decision, "rule_id", &review_path).map(str::to_string))
        .collect::<TaskResult<BTreeSet<_>>>()?;
    require(
        candidate_rules == reviewed
            && fixture["row_id"] == "ibm-enterprise-cobol-6.5-2026-05-31:procedure-statements:0026"
            && fixture["scope"]["values"] == json!([100, -911, 0, 99999, 999999])
            && fixture["expected_output_hex"]
                == "20203130300a202d3931310a20202020300a20393939390a20393939390a"
            && fixture["expectation_authority"]["kind"] == "maintainer-reviewed-golden"
            && fixture["expectation_authority"]["independent_control"]["observed_output_hex"]
                == fixture["expected_output_hex"]
            && fixture["expectation_authority"]["independent_control"]["coverage_credit"] == 0
            && fixture["expectation_authority"]["independent_control"]["licensed_credit"] == 0
            && fixture["comparison_policy"]["normalizable_fields"]
                .as_array()
                .is_some_and(Vec::is_empty)
            && review["old_new_mapping"]["old_scope"] == "alphanumeric literal MOVED to PIC X(8)"
            && review["old_new_mapping"]["new_scope"]
                == "signed S9(9) COMP-5 to PIC ----9 numeric-edited conversion, sign insertion, zero suppression, and truncation boundary"
            && review["old_new_mapping"]["cutover_rule"]
                == "Retain both bindings because their behavioral scopes do not overlap; each remains the sole current claim authority for its own fixture and observation.",
        "COBOL MOVE candidate decisions or exact-byte fixture are incomplete",
    )?;
    let promoted = spec
        .cases()
        .any(|case| case.test_id().as_str() == "cobol.numeric-move.bytes.executed");
    match review["review_status"].as_str() {
        Some("pending-maintainer") => require(
            !promoted,
            "unreviewed COBOL MOVE candidates were promoted into Conformance IR",
        ),
        Some("accepted") => require(
            promoted
                && decisions
                    .iter()
                    .all(|decision| decision["decision"] == "accepted"),
            "accepted COBOL MOVE review is not promoted through the shared runner",
        ),
        _ => Err("COBOL MOVE pilot review status is unknown".into()),
    }
}

fn check_cobol_arithmetic_pilot_inputs(root: &Path, spec: &CompiledSpec) -> TaskResult {
    let review_path =
        root.join("conformance/subsystems/cics/application/cobol/arithmetic-rule-review.json");
    if !review_path.is_file() {
        return Ok(());
    }
    let fixture_path =
        root.join("conformance/subsystems/cics/application/cobol/arithmetic-fixtures.json");
    let manifest_path =
        root.join("conformance/subsystems/cobol/structure/generated/cobol-topic-manifest.json");
    let projection_path = root.join(
        "conformance/subsystems/cobol/structure/generated/cobol-html-grammar-projection.json",
    );
    let review = json(&review_path)?;
    let fixture = json(&fixture_path)?;
    let manifest = json(&manifest_path)?;
    let projection = json(&projection_path)?;
    let row_id = "ibm-enterprise-cobol-6.5-2026-05-31:procedure-statements:0002";

    let add_topic = array(&manifest, "topics", &manifest_path)?
        .iter()
        .find(|topic| topic["id"] == "add")
        .ok_or("pinned COBOL ADD topic is missing")?;
    let add_topics = array(add_topic, "topics", &manifest_path)?;
    require(
        manifest["product"] == "SS6SG3_6.5"
            && manifest["retained_in_repository"] == false
            && manifest["coverage_credit"] == 0
            && add_topic["row_id"] == row_id
            && add_topics.len() == 1
            && add_topics[0]["topic_path"] == "SS6SG3_6.5/lr/ref/rlpsadd.html"
            && add_topics[0]["sha256"]
                == "sha256:0d41bb5458c78b6477731c50c532c36731f89a4a12e6ce88e1123c138b6b6e35"
            && review["pinned_source"]["manifest"]
                == "conformance/subsystems/cobol/structure/generated/cobol-topic-manifest.json"
            && review["pinned_source"]["product"] == manifest["product"]
            && review["pinned_source"]["row_id"] == add_topic["row_id"]
            && review["pinned_source"]["topic_path"] == add_topics[0]["topic_path"]
            && review["pinned_source"]["sha256"] == add_topics[0]["sha256"]
            && review["pinned_source"]["retained_publication_bytes"] == false
            && review["pinned_source"]["coverage_credit"] == 0,
        "COBOL arithmetic review drifted from the already pinned ADD topic",
    )?;
    require(
        review["semantic_publication"]["product"] == "SS6SG3_6.5"
            && review["semantic_publication"]["publication_number"] == "SC27-8713-04"
            && review["semantic_publication"]["revision_date"] == "2026-05-31"
            && review["semantic_publication"]["url"]
                == "https://www.ibm.com/docs/en/SS6SG3_6.5/pdf/lrmvs.pdf"
            && review["semantic_publication"]["sha256"]
                == "sha256:8b86cbd2d838d8f460dcbe8d1e74d2266799ce1534f4cfdbc08469c36748e85e"
            && review["semantic_publication"]["retained_in_repository"] == false
            && review["semantic_publication"]["coverage_credit"] == 0
            && review["semantic_publication"]["locators"]["size_error"] == "SIZE ERROR phrases"
            && review["semantic_publication"]["locators"]["corresponding"]
                == "CORRESPONDING phrase"
            && review["semantic_publication"]["locators"]["add_format_3"]
                == "ADD statement, Format 3",
        "COBOL arithmetic semantic rules lack an exact publication digest or locator",
    )?;

    let add_projection = array(&projection, "rows", &projection_path)?
        .iter()
        .find(|row| row["row_id"] == row_id)
        .ok_or("COBOL ADD grammar projection is missing")?;
    let required_titles = review["grammar_projection"]["required_format_titles"]
        .as_array()
        .ok_or("COBOL arithmetic grammar review titles are missing")?
        .iter()
        .filter_map(Value::as_str)
        .collect::<BTreeSet<_>>();
    let projected_titles = add_projection["format_titles"]
        .as_array()
        .ok_or("COBOL ADD projected format titles are missing")?
        .iter()
        .filter_map(Value::as_str)
        .collect::<BTreeSet<_>>();
    require(
        review["grammar_projection"]["path"]
            == "conformance/subsystems/cobol/structure/generated/cobol-html-grammar-projection.json"
            && required_titles
                == BTreeSet::from([
                    "Format 1: ADD statement",
                    "Format 2: ADD statement with GIVING phrase",
                    "Format 3: ADD statement with CORRESPONDING phrase",
                ])
            && required_titles.is_subset(&projected_titles),
        "COBOL arithmetic review is not closed over the pinned ADD grammar forms",
    )?;

    let decisions = array(&review, "decisions", &review_path)?;
    let reviewed_rules = decisions
        .iter()
        .map(|decision| text(decision, "rule_id", &review_path).map(str::to_string))
        .collect::<TaskResult<BTreeSet<_>>>()?;
    let reviewed_cases = decisions
        .iter()
        .map(|decision| text(decision, "fixture_case", &review_path).map(str::to_string))
        .collect::<TaskResult<BTreeSet<_>>>()?;
    let cases = array(&fixture, "cases", &fixture_path)?;
    let fixture_cases = cases
        .iter()
        .map(|case| text(case, "case_id", &fixture_path).map(str::to_string))
        .collect::<TaskResult<BTreeSet<_>>>()?;
    let fixture_rules = cases
        .iter()
        .flat_map(|case| {
            case["rule_ids"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(str::to_string)
        })
        .collect::<BTreeSet<_>>();
    require(
        review["review_status"] == "accepted"
            && decisions.len() == 6
            && decisions.iter().all(|decision| {
                decision["decision"] == "accepted"
                    && matches!(
                        decision["source_locator"].as_str(),
                        Some("SIZE ERROR phrases" | "CORRESPONDING phrase")
                    )
            })
            && reviewed_rules == fixture_rules
            && reviewed_cases == fixture_cases
            && fixture_cases
                == BTreeSet::from([
                    "receiver-local-size-error".to_string(),
                    "receiver-overflow-without-handler".to_string(),
                    "corresponding-relative-qualifiers".to_string(),
                    "corresponding-no-pair-no-op".to_string(),
                    "corresponding-numeric-edited-eligible".to_string(),
                ]),
        "COBOL arithmetic decisions and fixture cases are not closed",
    )?;

    let expected_contracts = BTreeMap::from([
        (
            "receiver-local-size-error",
            json!(["3639310a", [2], [2], ["GOOD-X", "SMALL-X"]]),
        ),
        (
            "receiver-overflow-without-handler",
            json!(["36310a", [2], [2], ["GOOD-X", "SMALL-X"]]),
        ),
        (
            "corresponding-relative-qualifiers",
            json!(["31310a32300a", [2], [1], ["DST-G.MATCH-X"]]),
        ),
        (
            "corresponding-no-pair-no-op",
            json!(["33340a", [2], [0], []]),
        ),
        (
            "corresponding-numeric-edited-eligible",
            json!(["31310a2b32320a", [2], [2], ["DST-G.GOOD-X", "DST-G.EDIT-X"]]),
        ),
    ]);
    require(
        fixture["schema_version"] == "mainframe-env.cobol-arithmetic-pilot-fixtures@1"
            && fixture["fixture_id"] == "cobol.typed-arithmetic.issue-140-141-v1"
            && fixture["row_id"] == row_id
            && cases.iter().all(|case| {
                case["program_source"]
                    .as_str()
                    .is_some_and(|source| source.contains("ADD"))
                    && expected_contracts
                        .get(case["case_id"].as_str().unwrap_or_default())
                        .is_some_and(|expected| {
                            *expected
                                == json!([
                                    case["expected_output_hex"],
                                    case["expected_typed_plan"]["operation_majors"],
                                    case["expected_typed_plan"]["assignment_counts"],
                                    case["expected_typed_plan"]["assignment_targets"],
                                ])
                        })
                    && case["expected_typed_plan"]["semantic_origins"] == json!(["cobol.add@1"])
                    && case["expected_typed_plan"]["execution_policies"]
                        == json!(["decimal34-v1/cobol-numeric-v1/captured-operands-receiver-local-v1/cobol-size-error-v1"])
            })
            && fixture["expectation_authority"]["external_control"]["status"] == "not-run"
            && fixture["expectation_authority"]["external_control"]["coverage_credit"] == 0
            && fixture["expectation_authority"]["external_control"]["licensed_credit"] == 0
            && fixture["comparison_policy"]["normalizable_fields"]
                .as_array()
                .is_some_and(Vec::is_empty)
            && review["evidence_boundary"]["licensed_ibm_differential_run"] == false
            && review["evidence_boundary"]["licensed_credit"] == 0
            && review["evidence_boundary"]["differential_credit"] == 0
            && review["evidence_boundary"]["candidate_outputs_are_source_authority"] == false,
        "COBOL arithmetic independent exact-byte or typed-plan contracts drifted",
    )?;
    require(
        spec.cases()
            .any(|case| case.test_id().as_str() == "cobol.typed-arithmetic.contract.executed"),
        "accepted COBOL arithmetic review is not promoted through the shared runner",
    )
}

fn import_cics_oracle(
    root: &Path,
    capture_path: &Path,
    family_id: Option<&str>,
    environment_manifest_path: Option<&Path>,
    public_key_path: Option<&Path>,
) -> TaskResult {
    let canonical_root = fs::canonicalize(root).map_err(|error| error.to_string())?;
    let capture_path =
        fs::canonicalize(capture_path).map_err(|error| format!("CICS oracle capture: {error}"))?;
    require(
        !capture_path.starts_with(&canonical_root),
        "CICS oracle capture must remain outside the candidate tree",
    )?;
    let capture = fs::read(&capture_path).map_err(|error| error.to_string())?;
    let public_key = public_key_path
        .map(|path| fs::read(path).map_err(|error| format!("CICS oracle public key: {error}")))
        .transpose()?;
    if let Some(key) = &public_key {
        require(
            key.len() == 32,
            "CICS oracle Ed25519 public key must contain exactly 32 raw bytes",
        )?;
    }
    let (adapter_path, fixture_path, review_path, environment_path, family_manifest_digest) =
        if let Some(family) = family_id {
            require(
                !family.is_empty()
                    && family.len() <= 80
                    && family.bytes().all(|byte| {
                        byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-'
                    }),
                "CICS oracle family ID is malformed",
            )?;
            let adapter_path = root.join(format!(
                "conformance/subsystems/cics/application/oracles/cics-licensed-family-{family}.json"
            ));
            let adapter = json(&adapter_path)?;
            let schema_path = root.join(
                "conformance/subsystems/cics/application/schemas/cics-licensed-family.schema.json",
            );
            validate_schema_instance(&json(&schema_path)?, &adapter, &adapter_path)?;
            require(
                adapter["family_id"].as_str() == Some(family)
                    && adapter["capture_contract"] == "mainframe-env.cics-oracle-capture@2"
                    && adapter["trusted_authority_id"] == "ibm-cics-protected-runner"
                    && adapter["signature_algorithm"] == "ed25519"
                    && adapter["public_key_source"] == "--public-key"
                    && adapter["local_model_and_synthetic_credit"] == 0,
                "CICS oracle family manifest identity is stale",
            )?;
            let environment_path = environment_manifest_path
                .ok_or_else(|| "CICS oracle family needs --environment-manifest".to_string())?;
            let environment_path = fs::canonicalize(environment_path)
                .map_err(|error| format!("CICS oracle environment manifest: {error}"))?;
            require(
                !environment_path.starts_with(&canonical_root),
                "CICS oracle exact environment manifest must remain outside the candidate tree",
            )?;
            require(
                fs::metadata(&environment_path)
                    .map_err(|error| format!("CICS oracle environment manifest: {error}"))?
                    .len()
                    <= 64 * 1024,
                "CICS oracle exact environment manifest exceeds its byte limit",
            )?;
            let environment = json(&environment_path)?;
            let fields = array(&adapter, "environment_manifest_fields", &adapter_path)?;
            let expected_keys = fields
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .chain(std::iter::once("schema_version".to_string()))
                .collect::<BTreeSet<_>>();
            require(
                environment["schema_version"] == "mainframe-env.cics-oracle-environment@1"
                    && environment.as_object().is_some_and(|object| {
                        object.keys().cloned().collect::<BTreeSet<_>>() == expected_keys
                            && fields.iter().filter_map(Value::as_str).all(|field| {
                                object[field]
                                    .as_str()
                                    .is_some_and(|value| !value.is_empty() && value.len() <= 4096)
                            })
                    }),
                "CICS oracle exact environment manifest is incomplete or has unknown fields",
            )?;
            let fixture_path = root.join(text(&adapter, "fixture_path", &adapter_path)?);
            let review_path = root.join(text(&adapter, "source_review_path", &adapter_path)?);
            (
                adapter_path.clone(),
                fixture_path,
                review_path,
                environment_path,
                Some(format!("sha256:{}", file_digest(&adapter_path)?)),
            )
        } else {
            require(
                environment_manifest_path.is_none(),
                "pilot capture does not take --environment-manifest",
            )?;
            (
                root.join("conformance/subsystems/cics/application/oracles/cics-licensed-differential.json"),
                root.join("conformance/subsystems/cics/application/cics/pilot-fixtures.json"),
                root.join("conformance/subsystems/cics/application/cics/pilot-rule-review.json"),
                root.join("conformance/subsystems/cics/application/cics/pilot-environment.json"),
                None,
            )
        };
    let adapter = json(&adapter_path)?;
    let fixture = json(&fixture_path)?;
    let review = json(&review_path)?;
    let scenario_values = if family_id.is_some() {
        array(&adapter, "observations", &adapter_path)?
            .iter()
            .map(|value| &value["scenario_id"])
            .collect::<Vec<_>>()
    } else {
        array(&adapter, "required_scenarios", &adapter_path)?
            .iter()
            .collect::<Vec<_>>()
    };
    let required_scenarios = scenario_values
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(str::to_string)
                .ok_or_else(|| "CICS oracle required scenario is not a string".to_string())
        })
        .collect::<TaskResult<BTreeSet<_>>>()?;
    let required_order = family_id.map(|_| {
        scenario_values
            .iter()
            .filter_map(|value| value.as_str())
            .map(str::to_string)
            .collect::<Vec<_>>()
    });
    require(
        required_scenarios.len() == scenario_values.len(),
        "CICS oracle family scenario IDs are duplicate",
    )?;
    if family_id.is_some() {
        let rows = array(&adapter, "command_rows", &adapter_path)?
            .iter()
            .filter_map(Value::as_str)
            .collect::<BTreeSet<_>>();
        require(
            array(&adapter, "observations", &adapter_path)?
                .iter()
                .all(|observation| {
                    observation["command_row"]
                        .as_str()
                        .is_some_and(|row| rows.contains(row))
                })
                && fixture["family_id"].as_str() == family_id
                && fixture["schema_version"] == "mainframe-env.cics-oracle-family-fixtures@1"
                && review["source_review_credit"] == 0
                && review["licensed_execution_credit"] == 0,
            "CICS oracle family fixture/source scope is stale",
        )?;
        let pins_path =
            root.join("conformance/subsystems/cics/application/manifests/cics-application-api-sources-a-topics.json");
        let pins = json(&pins_path)?;
        let registrations_path = root.join(
            "conformance/subsystems/cics/application/cics/typed-execution-registrations.json",
        );
        let registrations = json(&registrations_path)?;
        let reviewed_topics = array(&review, "topics", &review_path)?;
        require(
            review["schema_version"] == "mainframe-env.cics-oracle-source-review@1"
                && review["baseline"] == pins["baseline_id"]
                && review["catalog_baseline"] == "ibm-cics-ts-6x-2026-08-31:api-commands"
                && reviewed_topics.len() == rows.len()
                && reviewed_topics
                    .iter()
                    .filter_map(|topic| topic["command_row"].as_str())
                    .collect::<BTreeSet<_>>()
                    .len()
                    == rows.len()
                && reviewed_topics.iter().all(|topic| {
                    let Some(short_row) = topic["command_row"].as_str() else {
                        return false;
                    };
                    let full_row = format!("ibm-cics-ts-6x-2026-08-31:api-commands:{short_row}");
                    rows.contains(full_row.as_str())
                        && pins["topics"].as_array().is_some_and(|pinned| {
                            pinned.iter().any(|pin| {
                                pin["topic_path"] == topic["topic_path"]
                                    && pin["sha256"] == topic["sha256"]
                            })
                        })
                })
                && rows.iter().all(|row| {
                    registrations["registrations"]
                        .as_array()
                        .is_some_and(|items| {
                            items.iter().any(|item| {
                                item["official_row"].as_str() == Some(*row)
                                    && item["interface"] == "api"
                            })
                        })
                }),
            "CICS oracle family source pins or typed rows are stale",
        )?;
    }
    let spec = compile_shared_spec(root)?;
    let candidate = candidate_digest(root)?;
    let fixture_digest = format!("sha256:{}", file_digest(&fixture_path)?);
    let review_digest = format!("sha256:{}", file_digest(&review_path)?);
    let environment_digest = format!("sha256:{}", file_digest(&environment_path)?);
    let comparison_policy = if family_id.is_some() {
        fixture["comparison_policy"]
            .as_str()
            .ok_or_else(|| "CICS oracle family comparison policy is missing".to_string())?
    } else {
        text(&fixture["comparison_policy"], "version", &fixture_path)?
    };
    let fixture_observations = if family_id.is_some() {
        &fixture["expected_observations"]
    } else {
        &fixture["licensed_expected_observations"]
    };
    let fixture_observations = fixture_observations
        .as_array()
        .ok_or_else(|| "CICS licensed expected observations are missing".to_string())?
        .iter()
        .map(|value| {
            serde_json::from_value::<CicsOracleObservation>(value.clone())
                .map_err(|error| format!("CICS licensed expected observation: {error}"))
        })
        .collect::<TaskResult<Vec<_>>>()?;
    if let Some(order) = &required_order {
        require(
            fixture_observations
                .iter()
                .map(|observation| &observation.scenario_id)
                .collect::<Vec<_>>()
                == order.iter().collect::<Vec<_>>(),
            "CICS oracle reviewed fixture order differs from its sealed family manifest",
        )?;
    }
    require(
        fixture_observations
            .iter()
            .map(|observation| observation.scenario_id.as_str())
            .collect::<BTreeSet<_>>()
            .len()
            == fixture_observations.len(),
        "CICS oracle reviewed fixture has duplicate observations",
    )?;
    let expected_observations = fixture_observations
        .into_iter()
        .map(|observation| (observation.scenario_id.clone(), observation))
        .collect::<BTreeMap<_, _>>();
    let expected = CicsOracleExpectation {
        family_id,
        family_manifest_digest: family_manifest_digest.as_deref(),
        candidate_digest: &candidate,
        spec_digest: spec.spec_digest(),
        fixture_digest: &fixture_digest,
        source_review_digest: &review_digest,
        environment_manifest_digest: &environment_digest,
        comparison_policy,
        required_scenarios: &required_scenarios,
        required_order,
        expected_observations: &expected_observations,
    };
    let outcome = import_cics_oracle_capture(&capture, &expected, public_key.as_deref())?;
    match outcome {
        CicsOracleImport::AdapterContractValid => println!(
            "cics-oracle adapter-contract=valid licensed-credit=0 differential=pending capture={}",
            capture_path.display()
        ),
        CicsOracleImport::Licensed {
            authority,
            run_job_id,
            receipt_digest,
        } => {
            if family_id.is_none() {
                require(
                    review["review_status"] == "accepted"
                        && spec.scenarios().any(|scenario| {
                            scenario.scenario_id().as_str() == "cics.file-uow.local"
                        }),
                    "licensed CICS capture cannot import before reviewed pilot promotion",
                )?;
            }
            if let Some(family) = family_id {
                println!(
                    "cics-oracle protected-origin={authority} run-job={run_job_id} receipt={receipt_digest} licensed-credit=1 scoped-family-only family={family} differential=pending"
                );
            } else {
                println!(
                    "cics-oracle protected-origin={authority} run-job={run_job_id} receipt={receipt_digest} licensed-credit=1 scoped-pilot-only"
                );
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod docs_driven_pipeline_tests {
    use super::*;

    #[test]
    fn cics_bif_family_local_import_binds_manifest_fixture_and_external_environment() {
        let root = repository_root().unwrap();
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let external = std::env::temp_dir().join(format!("cics-bif-oracle-test-{nonce}"));
        fs::create_dir(&external).unwrap();
        let environment_path = external.join("environment.json");
        let environment = serde_json::json!({
            "schema_version": "mainframe-env.cics-oracle-environment@1",
            "cics_version_and_maintenance": "test-only-local",
            "enterprise_cobol_version_and_maintenance": "test-only-local",
            "compiler_options": "test-only-local",
            "encoding": "CP037",
            "cpacf_msa_availability": "test-only-local",
            "program_and_transaction_definition": "test-only-local",
            "principal_and_saf_configuration": "test-only-local",
            "capture_serialization_version": "test-only-local",
        });
        fs::write(&environment_path, serde_json::to_vec(&environment).unwrap()).unwrap();
        let manifest_path =
            root.join("conformance/subsystems/cics/application/oracles/cics-licensed-family-bif-builtins-v1.json");
        let fixture_path = root.join(
            "conformance/subsystems/cics/application/oracles/cics-bif-builtins-fixtures.json",
        );
        let source_path = root.join(
            "conformance/subsystems/cics/application/oracles/cics-bif-builtins-source-review.json",
        );
        let fixture = json(&fixture_path).unwrap();
        let observations = fixture["expected_observations"].clone();
        let typed_observations =
            serde_json::from_value::<Vec<CicsOracleObservation>>(observations.clone()).unwrap();
        let capture_path = external.join("capture.json");
        let capture = serde_json::json!({
            "schema_version": "mainframe-env.cics-oracle-capture@2",
            "family_id": "bif-builtins-v1",
            "family_manifest_digest": format!("sha256:{}", file_digest(&manifest_path).unwrap()),
            "candidate_digest": candidate_digest(&root).unwrap(),
            "spec_digest": compile_shared_spec(&root).unwrap().spec_digest(),
            "fixture_digest": format!("sha256:{}", file_digest(&fixture_path).unwrap()),
            "source_review_digest": format!("sha256:{}", file_digest(&source_path).unwrap()),
            "environment_manifest_digest": format!("sha256:{}", file_digest(&environment_path).unwrap()),
            "comparison_policy": fixture["comparison_policy"],
            "raw_capture_digest": format!("sha256:{:x}", Sha256::digest(serde_json::to_vec(&typed_observations).unwrap())),
            "observations": observations,
            "origin": {"kind": "local", "authority": "test-only-local", "run_job_id": "test-1", "signature": null},
        });
        fs::write(&capture_path, serde_json::to_vec(&capture).unwrap()).unwrap();
        let imported = import_cics_oracle(
            &root,
            &capture_path,
            Some("bif-builtins-v1"),
            Some(&environment_path),
            None,
        );
        assert!(imported.is_ok(), "{imported:?}");
        let mut changed_environment = environment;
        changed_environment["encoding"] = serde_json::json!("other-test-encoding");
        fs::write(
            &environment_path,
            serde_json::to_vec(&changed_environment).unwrap(),
        )
        .unwrap();
        assert!(
            import_cics_oracle(
                &root,
                &capture_path,
                Some("bif-builtins-v1"),
                Some(&environment_path),
                None
            )
            .is_err()
        );
        fs::remove_dir_all(external).unwrap();
    }

    #[test]
    fn cics_file_uow_pilot_v1_still_imports_exact_twelve_local_observations() {
        let root = repository_root().unwrap();
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let external = std::env::temp_dir().join(format!("cics-pilot-oracle-test-{nonce}"));
        fs::create_dir(&external).unwrap();
        let fixture_path =
            root.join("conformance/subsystems/cics/application/cics/pilot-fixtures.json");
        let source_path =
            root.join("conformance/subsystems/cics/application/cics/pilot-rule-review.json");
        let environment_path =
            root.join("conformance/subsystems/cics/application/cics/pilot-environment.json");
        let fixture = json(&fixture_path).unwrap();
        let observations = fixture["licensed_expected_observations"].clone();
        let typed_observations =
            serde_json::from_value::<Vec<CicsOracleObservation>>(observations.clone()).unwrap();
        assert_eq!(typed_observations.len(), 12);
        let capture_path = external.join("capture.json");
        let capture = serde_json::json!({
            "schema_version": "mainframe-env.cics-oracle-capture@1",
            "candidate_digest": candidate_digest(&root).unwrap(),
            "spec_digest": compile_shared_spec(&root).unwrap().spec_digest(),
            "fixture_digest": format!("sha256:{}", file_digest(&fixture_path).unwrap()),
            "source_review_digest": format!("sha256:{}", file_digest(&source_path).unwrap()),
            "environment_manifest_digest": format!("sha256:{}", file_digest(&environment_path).unwrap()),
            "comparison_policy": fixture["comparison_policy"]["version"],
            "raw_capture_digest": format!("sha256:{:x}", Sha256::digest(serde_json::to_vec(&typed_observations).unwrap())),
            "observations": observations,
            "origin": {"kind": "local", "authority": "test-only-local", "run_job_id": "test-1", "signature": null},
        });
        fs::write(&capture_path, serde_json::to_vec(&capture).unwrap()).unwrap();
        assert!(import_cics_oracle(&root, &capture_path, None, None, None).is_ok());
        let mut wrong_spec = capture;
        wrong_spec["spec_digest"] = serde_json::json!(
            "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
        );
        fs::write(&capture_path, serde_json::to_vec(&wrong_spec).unwrap()).unwrap();
        assert!(import_cics_oracle(&root, &capture_path, None, None, None).is_err());
        fs::remove_dir_all(external).unwrap();
    }

    #[test]
    fn stale_source_review_fails_the_fast_spec_gate_before_execution() {
        let source_root = repository_root().unwrap();
        let spec = compile_shared_spec(&source_root).unwrap();
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = env::temp_dir().join(format!(
            "mainframe-env-cics-review-negative-{}-{nonce}",
            std::process::id()
        ));
        for relative in [
            "conformance/subsystems/cics/application/manifests/cics-file-uow-topics.json",
            "conformance/subsystems/cics/application/generated/cics-pilot-semantic-candidates.json",
            "conformance/subsystems/cics/application/cics/pilot-rule-review.json",
            "conformance/subsystems/cics/application/cics/pilot-environment.json",
            "conformance/subsystems/cics/application/cics/pilot-fixtures.json",
            "conformance/subsystems/cics/application/oracles/cics-licensed-differential.json",
            "conformance/subsystems/cics/application/schemas/cics-licensed-differential-adapter.schema.json",
        ] {
            let source = source_root.join(relative);
            let target = root.join(relative);
            fs::create_dir_all(target.parent().unwrap()).unwrap();
            fs::copy(source, target).unwrap();
        }
        let review_path =
            root.join("conformance/subsystems/cics/application/cics/pilot-rule-review.json");
        let mut review = json(&review_path).unwrap();
        review["candidate_projection_sha256"] = Value::String(format!("sha256:{}", "f".repeat(64)));
        fs::write(&review_path, pretty_json(&review).unwrap()).unwrap();
        let problem = check_cics_pilot_inputs(&root, &spec).unwrap_err();
        let _ = fs::remove_dir_all(&root);
        assert!(problem.contains("stale against its candidate projection"));
    }
}

fn check_dataset_fixture_bindings(root: &Path, spec: &CompiledSpec) -> TaskResult {
    let path = root.join("conformance/subsystems/dataset/fixtures/dataset-organizations.json");
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
    let ams_path = root.join("conformance/subsystems/dataset/fixtures/ams-commands.json");
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
        "sha256:3333333333333333333333333333333333333333333333333333333333333333",
        spec.catalog_digest(),
        spec.spec_digest(),
        context.environment_manifest_digest(),
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
    let official_path = root.join("conformance/subsystems/coverage/catalogs/cobol.json");
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
/// `conformance/subsystems/cobol/structure/cobol/language.json` names its manifest and the digest it
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
    let index_path = root.join("conformance/subsystems/coverage/catalogs/index.json");
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
    let language_path = root.join("conformance/subsystems/cobol/structure/cobol/language.json");
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
    let language_path = root.join("conformance/subsystems/cobol/structure/cobol/language.json");
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
    let language_path = root.join("conformance/subsystems/cobol/structure/cobol/language.json");
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
        if row
            .row_id()
            .as_str()
            .ends_with(":procedure-statements:0026")
            && spec
                .cases()
                .any(|case| case.test_id().as_str() == "cobol.numeric-move.bytes.executed")
        {
            expected_obligations.insert("numeric-move-bytes");
        }
        if row
            .row_id()
            .as_str()
            .ends_with(":procedure-statements:0002")
            && spec
                .cases()
                .any(|case| case.test_id().as_str() == "cobol.typed-arithmetic.contract.executed")
        {
            expected_obligations.insert("typed-arithmetic-semantics");
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
    let language_path = root.join("conformance/subsystems/cobol/structure/cobol/language.json");
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
        file_digest(&root.join(
            "conformance/subsystems/cobol/execution/oracles/cobol-licensed-differential.json"
        ))?
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
        "docs/prompts/subsystems/cobol/IMPLEMENT_EXECUTION.md",
        "docs/delivery/subsystems/cobol/execution-plan.md",
        "docs/delivery/subsystems/cobol/local-assurance.md",
        "docs/delivery/subsystems/cobol/execution-status.md",
    ] {
        let document = read(&root.join(relative))?;
        require(
            document.contains("pass-with-licensed-differential-pending")
                && document.contains("GnuCOBOL")
                && document.contains("0/153")
                && document.contains("certification.licensed"),
            &format!("{relative} omits the approved GnuCOBOL completion disposition"),
        )?;
    }
    for relative in [
        "docs/prompts/subsystems/certification/IMPLEMENT_LICENSED.md",
        "docs/delivery/subsystems/certification/licensed-plan.md",
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
        .map_err(|error| format!("{}: {error}", path.display()))?;
    let reserved = root.join("crates/foundation/mainframe-env-ir/src/cobol_reserved_words.rs");
    fs::write(&reserved, render_cobol_reserved_words(root)?)
        .map_err(|error| format!("{}: {error}", reserved.display()))
}

fn check_cobol_language_generated(root: &Path) -> TaskResult {
    let path = root.join("crates/kernel/mainframe-env-compiler/src/generated/cobol_language.rs");
    let actual = fs::read(&path).map_err(|error| format!("{}: {error}", path.display()))?;
    require(
        actual == render_cobol_language(root)?,
        "generated COBOL language identities are stale; run cargo xtask cobol-language",
    )?;
    let reserved = root.join("crates/foundation/mainframe-env-ir/src/cobol_reserved_words.rs");
    let actual = fs::read(&reserved).map_err(|error| format!("{}: {error}", reserved.display()))?;
    require(
        actual == render_cobol_reserved_words(root)?,
        "generated COBOL reserved words are stale; run cargo xtask cobol-language",
    )
}

fn render_cobol_reserved_words(root: &Path) -> TaskResult<Vec<u8>> {
    let path = root.join("conformance/subsystems/cobol/structure/cobol/language.json");
    let language = json(&path)?;
    let words = cobol_undefinable_words(root, &language, &path)?;
    let mut source =
        String::from("// @generated by `cargo xtask cobol-language`; do not edit.\n\n");
    source.push_str(&format!(
        "pub(crate) static COBOL_UNDEFINABLE_WORDS: [&str; {}] = [\n",
        words.len()
    ));
    for word in words {
        source.push_str(&format!(
            "    {},\n",
            serde_json::to_string(&word).map_err(|error| error.to_string())?
        ));
    }
    source.push_str(
        "];\n\npub(crate) fn is_cobol_undefinable_word(word: &str) -> bool {\n    COBOL_UNDEFINABLE_WORDS.binary_search(&word).is_ok()\n}\n",
    );
    format_generated_rust(root, source.into_bytes())
}

fn render_cobol_language(root: &Path) -> TaskResult<Vec<u8>> {
    let path = root.join("conformance/subsystems/cobol/structure/cobol/language.json");
    let language = json(&path)?;
    check_cobol_language_catalog(root, &path)?;
    let statements = array(&language, "compiler_directing_statements", &path)?;
    let groups = array(&language, "compiler_directive_groups", &path)?;
    let procedure_statements = array(&language, "procedure_statements", &path)?;
    let file_clauses = array(&language, "file_description_clauses", &path)?;
    let data_clauses = array(&language, "data_description_clauses", &path)?;
    let intrinsic_functions = array(&language, "intrinsic_functions", &path)?;
    let special_registers = array(&language, "special_registers", &path)?;
    let undefinable = cobol_undefinable_words(root, &language, &path)?;
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
        for keyword in cobol_form_keywords(entry, &path, &undefinable)? {
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

/// The words a conforming program may not spell as a user-defined name.
///
/// `is_grammar_keyword` unions every row's `grammar_keywords` and
/// `is_operand_atom` then refuses any token in that union as an operand, so the
/// harvest below is not a statement of what a row's own syntax reserves -- it is
/// a statement about the whole language. A word only belongs in it if the
/// reference says a program may not use it as a data-name, and the reference
/// says that in one place: `Reserved words` (`lr/ref/rlres.html`), whose word
/// column is marked `Reserved` for function this compiler implements,
/// `Standard only` for 85 COBOL Standard function it does not, and `Potential
/// reserved words` for words a future release might take. The topic states the
/// consequence of each: the first two are flagged with an S-level message, the
/// third with an I-level one. So the first two forbid a data-name and the third
/// only advises against it, which is why `potential` is read out of the
/// appendix and then deliberately not used here.
///
/// `conformance/subsystems/cobol/structure/cobol/reserved-words.json` is that appendix, read by
/// `conformance/subsystems/cobol/structure/tools/extract_cobol_reserved_words.py` from the same pinned
/// topic the 0.2 baseline pins. This checks it names the catalog's own baseline
/// and manifest and cites its topics at the digests that manifest records, so
/// the list cannot come to describe a different book than the forms do.
fn cobol_undefinable_words(
    root: &Path,
    language: &Value,
    language_path: &Path,
) -> TaskResult<BTreeSet<String>> {
    let path = root.join("conformance/subsystems/cobol/structure/cobol/reserved-words.json");
    let document = json(&path)?;
    require(
        text(&document, "schema_version", &path)? == "mainframe-env.cobol-reserved-words@1",
        "the COBOL reserved word list is not a reserved word list",
    )?;
    require(
        text(&document, "baseline_id", &path)? == text(language, "baseline_id", language_path)?,
        "the COBOL reserved word list and the COBOL catalog cite different baselines",
    )?;
    let source = &document["source"];
    require(
        document["coverage_credit"].as_u64() == Some(0)
            && source["retained_in_repository"] == Value::Bool(false),
        "the COBOL reserved word list claims coverage credit or retained publication bytes",
    )?;
    let manifest_relative = text(source, "manifest", &path)?;
    require(
        manifest_relative == text(&language["source"], "manifest", language_path)?,
        "the COBOL reserved word list and the COBOL catalog cite different topic manifests",
    )?;
    let manifest_path = root.join(manifest_relative);
    let manifest = json(&manifest_path)?;
    let topics = array(&manifest, "topics", &manifest_path)?;
    let cited = array(source, "topics", &path)?;
    require(
        !cited.is_empty(),
        "the COBOL reserved word list cites no topic",
    )?;
    for topic in cited {
        let topic_path = text(topic, "topic_path", &path)?;
        let pinned = topics
            .iter()
            .find(|entry| entry["topic_path"].as_str() == Some(topic_path))
            .ok_or_else(|| {
                format!("the COBOL reserved word list cites {topic_path}, which {manifest_relative} does not pin")
            })?;
        require(
            pinned["sha256"].as_str() == Some(text(topic, "sha256", &path)?),
            &format!(
                "the COBOL reserved word list cites {topic_path} at a digest {manifest_relative} does not pin"
            ),
        )?;
    }
    let mut undefinable = BTreeSet::new();
    for column in ["reserved", "standard_only"] {
        for word in array(&document, column, &path)? {
            let word = word
                .as_str()
                .ok_or("a COBOL reserved word is not a string")?;
            require(
                !word.is_empty()
                    && word.bytes().all(|byte| {
                        byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'-'
                    }),
                &format!("{word} is not shaped like a COBOL word"),
            )?;
            require(
                undefinable.insert(word.to_string()),
                &format!("the COBOL reserved word list lists {word} twice"),
            )?;
        }
    }
    Ok(undefinable)
}

/// The reserved words one row's forms spell, and only those.
///
/// Every all-uppercase word in a form is a candidate, because that is how the
/// catalog writes a keyword, but a form is a picture of one statement's syntax
/// and this list feeds a language-wide test. `XML GENERATE` really does draw
/// `NAME`, `ENCODING` and `NAMESPACE`; none of the three is a reserved word, and
/// letting them through made `MOVE NAME TO DEST` a malformed operand in every
/// program the compiler reads. So a candidate the appendix does not publish is
/// dropped here and left to be recognised positionally by the statement parser
/// that draws it, the way `json_conversion_value` already recognises the
/// figurative constants.
fn cobol_form_keywords(
    entry: &Value,
    path: &Path,
    undefinable: &BTreeSet<String>,
) -> TaskResult<Vec<String>> {
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
                && undefinable.contains(word)
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

fn augment_ams_spec(root: &Path, spec: &mut Value) -> TaskResult {
    let fixture_path = root.join("conformance/subsystems/dataset/fixtures/ams-commands.json");
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

fn augment_docs_driven_pilots(root: &Path, spec: &mut Value) -> TaskResult {
    augment_cics_pilot_spec(root, spec)?;
    augment_cobol_move_pilot_spec(root, spec)?;
    augment_cobol_arithmetic_pilot_spec(root, spec)
}

fn registry_values_mut<'a>(spec: &'a mut Value, name: &str) -> TaskResult<&'a mut Vec<Value>> {
    let registries = spec
        .get_mut("registries")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| "conformance registries are missing".to_string())?;
    if name == "reviewed_rules" && !registries.contains_key(name) {
        registries.insert(name.into(), Value::Array(Vec::new()));
    }
    registries
        .get_mut(name)
        .and_then(Value::as_array_mut)
        .ok_or_else(|| format!("conformance registry {name} is missing"))
}

fn document_values_mut<'a>(spec: &'a mut Value, name: &str) -> TaskResult<&'a mut Vec<Value>> {
    spec.get_mut(name)
        .and_then(Value::as_array_mut)
        .ok_or_else(|| format!("conformance spec {name} are missing"))
}

fn augment_cics_pilot_spec(root: &Path, spec: &mut Value) -> TaskResult {
    let review_path =
        root.join("conformance/subsystems/cics/application/cics/pilot-rule-review.json");
    if !review_path.is_file() {
        return Ok(());
    }
    let review = json(&review_path)?;
    if review["review_status"] == "pending-maintainer" {
        return Ok(());
    }
    require(
        review["review_status"] == "accepted",
        "CICS pilot review status is unknown",
    )?;
    let decisions = array(&review, "decisions", &review_path)?;
    require(
        decisions.iter().all(|decision| {
            decision["decision"] == "accepted" || decision["decision"] == "defer-pending"
        }),
        "CICS pilot cannot promote proposed or unknown review decisions",
    )?;
    let review_digest = format!("sha256:{}", file_digest(&review_path)?);
    let accepted_rules = decisions
        .iter()
        .filter(|decision| decision["decision"] == "accepted")
        .map(|decision| text(decision, "rule_id", &review_path).map(str::to_string))
        .collect::<TaskResult<BTreeSet<_>>>()?;
    let expected_rules = BTreeSet::from([
        "cics.read.update-context".to_string(),
        "cics.read.notfound-80".to_string(),
        "cics.file.notauth-101".to_string(),
        "cics.read.notopen-60".to_string(),
        "cics.rewrite.requires-read-update".to_string(),
        "cics.rewrite.invreq-30".to_string(),
        "cics.update.context-lifetime".to_string(),
        "cics.syncpoint.commit".to_string(),
        "cics.syncpoint.rollback".to_string(),
        "cics.recovery.current-uow-only".to_string(),
        "cics.recovery.backout-logging".to_string(),
    ]);
    require(
        accepted_rules == expected_rules,
        "CICS pilot accepted rule set is incomplete",
    )?;

    for (registry, values) in [
        (
            "operations",
            vec!["cics.file.read", "cics.file.rewrite", "cics.uow.syncpoint"],
        ),
        ("input_shapes", vec!["cics.file-uow.fixture"]),
        (
            "transitions",
            vec![
                "cics.file.read-transition",
                "cics.file.rewrite-transition",
                "cics.uow.syncpoint-transition",
            ],
        ),
        ("conditions", vec!["cics.file.condition"]),
        ("recoveries", vec!["cics.uow.rollback"]),
        (
            "drivers",
            vec!["cics.pilot.product-path", "cics.pilot.readback-path"],
        ),
        (
            "scenario_steps",
            vec![
                "cics.pilot.compile",
                "cics.pilot.invoke",
                "cics.pilot.provider-effects",
                "cics.pilot.readback",
            ],
        ),
        (
            "failure_points",
            vec![
                "cics.file.before-intent",
                "cics.file.after-intent",
                "cics.file.after-mutation",
            ],
        ),
    ] {
        registry_values_mut(spec, registry)?
            .extend(values.into_iter().map(|value| Value::String(value.into())));
    }
    let obligations = vec![
        (
            "ibm-cics-ts-6x-2026-08-31:api-commands:0156",
            "plain-read",
            vec!["recognized", "executed", "conditioned"],
            vec!["cics.read.update-context"],
        ),
        (
            "ibm-cics-ts-6x-2026-08-31:api-commands:0156",
            "read-update",
            vec!["recognized", "validated", "executed", "conditioned"],
            vec!["cics.read.update-context"],
        ),
        (
            "ibm-cics-ts-6x-2026-08-31:api-commands:0156",
            "missing-record",
            vec!["validated", "conditioned"],
            vec!["cics.read.notfound-80"],
        ),
        (
            "ibm-cics-ts-6x-2026-08-31:api-commands:0156",
            "unauthorized",
            vec!["validated", "conditioned"],
            vec!["cics.file.notauth-101"],
        ),
        (
            "ibm-cics-ts-6x-2026-08-31:api-commands:0156",
            "closed-file",
            vec!["validated", "conditioned"],
            vec!["cics.read.notopen-60"],
        ),
        (
            "ibm-cics-ts-6x-2026-08-31:api-commands:0181",
            "requires-read-update",
            vec!["validated", "conditioned"],
            vec![
                "cics.rewrite.requires-read-update",
                "cics.rewrite.invreq-30",
            ],
        ),
        (
            "ibm-cics-ts-6x-2026-08-31:api-commands:0181",
            "rewrite-record",
            vec!["recognized", "executed", "conditioned"],
            vec!["cics.rewrite.requires-read-update"],
        ),
        (
            "ibm-cics-ts-6x-2026-08-31:api-commands:0181",
            "forbidden-mutation-on-invreq",
            vec!["executed", "conditioned"],
            vec!["cics.rewrite.invreq-30"],
        ),
        (
            "ibm-cics-ts-6x-2026-08-31:api-commands:0181",
            "context-invalidated-at-syncpoint",
            vec!["validated", "conditioned", "recovered"],
            vec!["cics.update.context-lifetime"],
        ),
        (
            "ibm-cics-ts-6x-2026-08-31:api-commands:0218",
            "commit-boundary",
            vec!["recognized", "executed", "recovered"],
            vec!["cics.syncpoint.commit", "cics.recovery.current-uow-only"],
        ),
        (
            "ibm-cics-ts-6x-2026-08-31:api-commands:0218",
            "rollback-boundary",
            vec!["recognized", "executed", "recovered"],
            vec!["cics.syncpoint.rollback", "cics.recovery.current-uow-only"],
        ),
        (
            "ibm-cics-ts-6x-2026-08-31:api-commands:0218",
            "durable-restart",
            vec!["recovered"],
            vec!["cics.syncpoint.rollback", "cics.recovery.backout-logging"],
        ),
    ];
    registry_values_mut(spec, "observations")?.extend(
        obligations
            .iter()
            .map(|(_, obligation, _, _)| Value::String(format!("cics.pilot.{obligation}"))),
    );
    registry_values_mut(spec, "fixtures")?.push(json!({
        "id": "cics.file-uow.pilot-v1",
        "digest": format!("sha256:{}", file_digest(&root.join("conformance/subsystems/cics/application/cics/pilot-fixtures.json"))?)
    }));
    registry_values_mut(spec, "reviewed_rules")?.extend(
        accepted_rules
            .iter()
            .map(|rule| json!({"id": rule, "digest": review_digest})),
    );
    let all_gates = vec![
        "recognized",
        "validated",
        "executed",
        "conditioned",
        "recovered",
        "differential",
    ];
    let rows = [
        (
            "ibm-cics-ts-6x-2026-08-31:api-commands:0156",
            "cics.file.read",
            "cics.file.read-transition",
            None,
        ),
        (
            "ibm-cics-ts-6x-2026-08-31:api-commands:0181",
            "cics.file.rewrite",
            "cics.file.rewrite-transition",
            Some("cics.uow.rollback"),
        ),
        (
            "ibm-cics-ts-6x-2026-08-31:api-commands:0218",
            "cics.uow.syncpoint",
            "cics.uow.syncpoint-transition",
            Some("cics.uow.rollback"),
        ),
    ];
    for (row_id, operation, transition, recovery) in rows {
        let row_obligations = obligations
            .iter()
            .filter(|(row, _, _, _)| *row == row_id)
            .collect::<Vec<_>>();
        let reviewed_rules = row_obligations
            .iter()
            .flat_map(|(_, _, _, rules)| rules.iter().copied())
            .collect::<BTreeSet<_>>();
        document_values_mut(spec, "rows")?.push(json!({
            "row_id": row_id,
            "operation": operation,
            "input": "cics.file-uow.fixture",
            "preconditions": [],
            "transition": transition,
            "postconditions": row_obligations.iter().map(|(_, obligation, _, _)| format!("cics.pilot.{obligation}")).collect::<Vec<_>>(),
            "conditions": ["cics.file.condition"],
            "recovery": recovery,
            "oracle": null,
            "applicable_gates": all_gates,
            "obligations": row_obligations.iter().map(|(_, obligation, _, _)| *obligation).collect::<Vec<_>>(),
            "reviewed_rules": reviewed_rules,
        }));
    }
    let mut credits = Vec::new();
    for (row_id, obligation, gates, rules) in obligations {
        document_values_mut(spec, "obligations")?.push(json!({
            "row_id": row_id,
            "obligation_id": obligation,
            "applicable_gates": gates,
        }));
        let recovery = if row_id.ends_with(":0156") {
            None
        } else {
            Some("cics.uow.rollback")
        };
        for gate in gates {
            document_values_mut(spec, "cases")?.push(json!({
                "spec_version": "mainframe-env.conformance-ir@1",
                "row_id": row_id,
                "obligation_id": obligation,
                "gate": gate,
                "test_id": format!("cics.pilot.{obligation}.{gate}"),
                "driver": "cics.pilot.product-path",
                "input": "cics.file-uow.pilot-v1",
                "preconditions": [],
                "expected": [format!("cics.pilot.{obligation}")],
                "recovery": recovery,
                "oracle": null,
                "reviewed_rules": rules,
                "scenario": "cics.file-uow.local",
            }));
            credits.push(json!({
                "row_id": row_id,
                "obligation_id": obligation,
                "gate": gate,
            }));
        }
    }
    document_values_mut(spec, "scenarios")?.push(json!({
        "scenario_id": "cics.file-uow.local",
        "drivers": ["cics.pilot.product-path", "cics.pilot.readback-path"],
        "ordered_steps": ["cics.pilot.compile", "cics.pilot.invoke", "cics.pilot.provider-effects", "cics.pilot.readback"],
        "failure_points": ["cics.file.before-intent", "cics.file.after-intent", "cics.file.after-mutation"],
        "credits": credits,
    }));
    Ok(())
}

fn augment_cobol_move_pilot_spec(root: &Path, spec: &mut Value) -> TaskResult {
    let review_path =
        root.join("conformance/subsystems/cics/application/cobol/move-rule-review.json");
    if !review_path.is_file() {
        return Ok(());
    }
    let review = json(&review_path)?;
    if review["review_status"] == "pending-maintainer" {
        return Ok(());
    }
    require(
        review["review_status"] == "accepted",
        "COBOL MOVE review status is unknown",
    )?;
    let decisions = array(&review, "decisions", &review_path)?;
    require(
        decisions
            .iter()
            .all(|decision| decision["decision"] == "accepted"),
        "COBOL MOVE cannot promote proposed review decisions",
    )?;
    let rules = decisions
        .iter()
        .map(|decision| text(decision, "rule_id", &review_path).map(str::to_string))
        .collect::<TaskResult<BTreeSet<_>>>()?;
    require(
        rules.len() == 8,
        "COBOL MOVE accepted rule set is incomplete",
    )?;
    let review_digest = format!("sha256:{}", file_digest(&review_path)?);
    registry_values_mut(spec, "drivers")?
        .push(Value::String("cobol.numeric-move.product-path".into()));
    registry_values_mut(spec, "observations")?
        .push(Value::String("cobol.numeric-move.exact-bytes".into()));
    registry_values_mut(spec, "fixtures")?.push(json!({
        "id": "cobol.numeric-move.floating-sign-v1",
        "digest": format!("sha256:{}", file_digest(&root.join("conformance/subsystems/cics/application/cobol/move-fixture.json"))?)
    }));
    registry_values_mut(spec, "reviewed_rules")?.extend(
        rules
            .iter()
            .map(|rule| json!({"id": rule, "digest": review_digest})),
    );
    let row_id = "ibm-enterprise-cobol-6.5-2026-05-31:procedure-statements:0026";
    require(
        document_values_mut(spec, "obligations")?
            .iter()
            .any(|obligation| {
                obligation["row_id"] == row_id && obligation["obligation_id"] == "runtime-normal"
            })
            && document_values_mut(spec, "cases")?.iter().any(|case| {
                case["row_id"] == row_id
                    && case["obligation_id"] == "runtime-normal"
                    && case["test_id"] == "cobol.statement-runtime.move"
                    && case["input"] == "cobol.statement-runtime.move"
            }),
        "COBOL MOVE non-overlapping alphanumeric binding is missing before numeric cutover",
    )?;
    let rows = document_values_mut(spec, "rows")?;
    let row = rows
        .iter_mut()
        .find(|row| row["row_id"] == row_id)
        .ok_or("COBOL MOVE row is missing from current Conformance IR")?;
    row["obligations"]
        .as_array_mut()
        .ok_or("COBOL MOVE obligations are missing")?
        .push(Value::String("numeric-move-bytes".into()));
    row["postconditions"]
        .as_array_mut()
        .ok_or("COBOL MOVE postconditions are missing")?
        .push(Value::String("cobol.numeric-move.exact-bytes".into()));
    row["reviewed_rules"] = Value::Array(rules.iter().cloned().map(Value::String).collect());
    document_values_mut(spec, "obligations")?.push(json!({
        "row_id": row_id,
        "obligation_id": "numeric-move-bytes",
        "applicable_gates": ["executed"],
    }));
    document_values_mut(spec, "cases")?.push(json!({
        "spec_version": "mainframe-env.conformance-ir@1",
        "row_id": row_id,
        "obligation_id": "numeric-move-bytes",
        "gate": "executed",
        "test_id": "cobol.numeric-move.bytes.executed",
        "driver": "cobol.numeric-move.product-path",
        "input": "cobol.numeric-move.floating-sign-v1",
        "preconditions": [],
        "expected": ["cobol.numeric-move.exact-bytes"],
        "recovery": null,
        "oracle": "cobol.enterprise-6.5.licensed",
        "reviewed_rules": rules,
        "scenario": null,
    }));
    Ok(())
}

fn augment_cobol_arithmetic_pilot_spec(root: &Path, spec: &mut Value) -> TaskResult {
    let review_path =
        root.join("conformance/subsystems/cics/application/cobol/arithmetic-rule-review.json");
    if !review_path.is_file() {
        return Ok(());
    }
    let review = json(&review_path)?;
    if review["review_status"] == "pending-maintainer" {
        return Ok(());
    }
    require(
        review["review_status"] == "accepted",
        "COBOL arithmetic review status is unknown",
    )?;
    let decisions = array(&review, "decisions", &review_path)?;
    require(
        decisions.len() == 6
            && decisions
                .iter()
                .all(|decision| decision["decision"] == "accepted"),
        "COBOL arithmetic cannot promote incomplete review decisions",
    )?;
    let rules = decisions
        .iter()
        .map(|decision| text(decision, "rule_id", &review_path).map(str::to_string))
        .collect::<TaskResult<BTreeSet<_>>>()?;
    require(
        rules.len() == 6,
        "COBOL arithmetic accepted rule set is incomplete",
    )?;
    let review_digest = format!("sha256:{}", file_digest(&review_path)?);
    registry_values_mut(spec, "drivers")?
        .push(Value::String("cobol.typed-arithmetic.product-path".into()));
    registry_values_mut(spec, "observations")?.push(Value::String(
        "cobol.typed-arithmetic.exact-contract".into(),
    ));
    registry_values_mut(spec, "fixtures")?.push(json!({
        "id": "cobol.typed-arithmetic.issue-140-141-v1",
        "digest": format!("sha256:{}", file_digest(&root.join("conformance/subsystems/cics/application/cobol/arithmetic-fixtures.json"))?)
    }));
    registry_values_mut(spec, "reviewed_rules")?.extend(
        rules
            .iter()
            .map(|rule| json!({"id": rule, "digest": review_digest})),
    );
    let row_id = "ibm-enterprise-cobol-6.5-2026-05-31:procedure-statements:0002";
    require(
        document_values_mut(spec, "cases")?.iter().any(|case| {
            case["row_id"] == row_id
                && case["test_id"] == "cobol.condition.add-size-error"
                && case["gate"] == "conditioned"
        }) && document_values_mut(spec, "cases")?.iter().any(|case| {
            case["row_id"] == row_id
                && case["test_id"] == "cobol.statement-phrase-runtime.add-corresponding"
                && case["gate"] == "executed"
        }),
        "COBOL arithmetic pilot must extend the existing ADD condition and phrase bindings",
    )?;
    let rows = document_values_mut(spec, "rows")?;
    let row = rows
        .iter_mut()
        .find(|row| row["row_id"] == row_id)
        .ok_or("COBOL ADD row is missing from current Conformance IR")?;
    row["obligations"]
        .as_array_mut()
        .ok_or("COBOL ADD obligations are missing")?
        .push(Value::String("typed-arithmetic-semantics".into()));
    row["postconditions"]
        .as_array_mut()
        .ok_or("COBOL ADD postconditions are missing")?
        .push(Value::String(
            "cobol.typed-arithmetic.exact-contract".into(),
        ));
    let mut row_rules = row["reviewed_rules"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect::<BTreeSet<_>>();
    row_rules.extend(rules.iter().cloned());
    row["reviewed_rules"] = Value::Array(row_rules.into_iter().map(Value::String).collect());
    document_values_mut(spec, "obligations")?.push(json!({
        "row_id": row_id,
        "obligation_id": "typed-arithmetic-semantics",
        "applicable_gates": ["executed"],
    }));
    document_values_mut(spec, "cases")?.push(json!({
        "spec_version": "mainframe-env.conformance-ir@1",
        "row_id": row_id,
        "obligation_id": "typed-arithmetic-semantics",
        "gate": "executed",
        "test_id": "cobol.typed-arithmetic.contract.executed",
        "driver": "cobol.typed-arithmetic.product-path",
        "input": "cobol.typed-arithmetic.issue-140-141-v1",
        "preconditions": [],
        "expected": ["cobol.typed-arithmetic.exact-contract"],
        "recovery": null,
        "oracle": "cobol.enterprise-6.5.licensed",
        "reviewed_rules": rules,
        "scenario": null,
    }));
    Ok(())
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
    if args.subsystem.as_deref() == Some("ims")
        || args
            .replay
            .as_deref()
            .is_some_and(|id| id.starts_with("ims."))
    {
        return ims_conformance::run(root, args);
    }
    require(
        !args.prepare_candidates,
        "candidate preparation is not installed for this selector",
    )?;
    if args.subsystem.as_deref() == Some("mq")
        || args
            .replay
            .as_deref()
            .is_some_and(|id| id.starts_with("mq."))
    {
        return run_focused_mq(root, args);
    }
    let dataset_racf_or_jcl_selected = matches!(
        args.subsystem.as_deref(),
        Some("dataset-vsam-ams" | "racf-saf" | "jcl-jes2")
    ) || args.replay.as_deref().is_some_and(|replay| {
        replay.starts_with("dataset.") || replay.starts_with("racf.") || replay.starts_with("jcl.")
    });
    if dataset_racf_or_jcl_selected {
        return check_focused_dataset_or_jcl_conformance_interface(root, args);
    }
    let cics_selected = args.subsystem.as_deref() == Some("cics")
        || args
            .replay
            .as_deref()
            .is_some_and(|replay| replay.starts_with("cics."));
    if cics_selected {
        return run_focused_cics(root, args);
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
    let (gate, local_gate) = parse_focused_gate(args.gate.as_deref())?;
    let spec = compile_shared_spec(root)?;
    let selection = if let Some(replay) = args.replay.as_deref() {
        RunnerSelection::replay(replay, limits).map_err(|problem| problem.to_string())?
    } else {
        make_focused_selection(
            args.subsystem
                .as_deref()
                .ok_or("focused conformance requires --subsystem")?,
            gate,
            local_gate,
            args.shard,
            limits,
        )?
    };
    let dataset_handlers = dataset_conformance_runtime();
    let jcl_handlers = jcl_conformance::runtime();
    let cics_handlers = cics_pilot_runtime();
    let cobol_move_handlers = cobol_move_pilot_runtime();
    let cobol_arithmetic_handlers = cobol_arithmetic_pilot_runtime();
    let runtime = combined_conformance_runtime(
        &spec,
        &dataset_handlers,
        &jcl_handlers,
        &cics_handlers,
        &cobol_move_handlers,
        &cobol_arithmetic_handlers,
        limits,
    )?;
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

fn run_focused_cics(root: &Path, args: &ConformanceArgs) -> TaskResult {
    require(
        args.shard.is_none(),
        "CICS pilot scenario execution does not accept --shard",
    )?;
    let (gate, local_gate) = parse_focused_gate(args.gate.as_deref())?;
    require(
        gate.is_none() && (args.gate.is_none() || local_gate),
        "CICS pilot scenario execution accepts only --gate local",
    )?;
    let limits = ConformanceLimits::default();
    let spec = compile_shared_spec(root)?;
    let scenario_id = if let Some(replay) = args.replay.as_deref() {
        spec.cases()
            .find(|case| case.test_id().as_str() == replay)
            .and_then(|case| case.scenario())
            .ok_or_else(|| format!("CICS replay {replay} is not scenario-bound"))?
            .as_str()
            .to_string()
    } else {
        "cics.file-uow.local".to_string()
    };
    let selection =
        RunnerSelection::scenario(scenario_id, limits).map_err(|problem| problem.to_string())?;
    let dataset_handlers = dataset_conformance_runtime();
    let jcl_handlers = jcl_conformance::runtime();
    let cics_handlers = cics_pilot_runtime();
    let cobol_move_handlers = cobol_move_pilot_runtime();
    let cobol_arithmetic_handlers = cobol_arithmetic_pilot_runtime();
    let runtime = combined_conformance_runtime(
        &spec,
        &dataset_handlers,
        &jcl_handlers,
        &cics_handlers,
        &cobol_move_handlers,
        &cobol_arithmetic_handlers,
        limits,
    )?;
    let context = RunnerContext::new_with_environment_manifest(
        candidate_digest(root)?,
        "local",
        format!(
            "sha256:{}",
            file_digest(
                &root.join("conformance/subsystems/cics/application/cics/pilot-environment.json")
            )?
        ),
        limits,
    )
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
                    .map_err(|problem| problem.to_string())?,
            )
            .map_err(|error| error.to_string())?
        );
        match event.verdict {
            Verdict::Pass => passed += 1,
            Verdict::Fail => failed += 1,
        }
    }
    println!(
        "cics-pilot-ledger spec-digest={} batches={} verdicts={} pass={} fail={}",
        spec.spec_digest(),
        report.batches.len(),
        passed + failed,
        passed,
        failed
    );
    require(failed == 0, "CICS pilot produced failing verdicts")
}

fn check_cobol_exit(root: &Path) -> TaskResult {
    check_spec(root)?;
    let receipt = verify_cobol_exit()?;
    let limits = ConformanceLimits::default();
    let spec = compile_shared_spec(root)?;
    let selection = RunnerSelection::focused("cobol", None, None, limits)
        .map_err(|problem| problem.to_string())?;
    let context = RunnerContext::new(candidate_digest(root)?, "local", limits)
        .map_err(|problem| problem.to_string())?;
    let dataset_handlers = dataset_conformance_runtime();
    let jcl_handlers = jcl_conformance::runtime();
    let cics_handlers = cics_pilot_runtime();
    let cobol_move_handlers = cobol_move_pilot_runtime();
    let cobol_arithmetic_handlers = cobol_arithmetic_pilot_runtime();
    let runtime = combined_conformance_runtime(
        &spec,
        &dataset_handlers,
        &jcl_handlers,
        &cics_handlers,
        &cobol_move_handlers,
        &cobol_arithmetic_handlers,
        limits,
    )?;
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
    let (gate, local_gate) = parse_focused_gate(args.gate.as_deref())?;
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
            let gate_matches = focused_gate_matches(case.key().gate, gate, local_gate);
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
                .map_err(|problem| problem.to_string())
        } else {
            make_focused_selection(
                args.subsystem
                    .as_deref()
                    .ok_or("dataset conformance subsystem is missing")?,
                gate,
                local_gate,
                args.shard,
                ConformanceLimits::default(),
            )
        }?;
        let context = RunnerContext::new(
            repository_digest(root)?,
            "local-focused",
            ConformanceLimits::default(),
        )
        .map_err(|problem| problem.to_string())?;
        let dataset_handlers = dataset_conformance_runtime();
        let jcl_handlers = jcl_conformance::runtime();
        let cics_handlers = cics_pilot_runtime();
        let cobol_move_handlers = cobol_move_pilot_runtime();
        let cobol_arithmetic_handlers = cobol_arithmetic_pilot_runtime();
        let runtime = combined_conformance_runtime(
            &spec,
            &dataset_handlers,
            &jcl_handlers,
            &cics_handlers,
            &cobol_move_handlers,
            &cobol_arithmetic_handlers,
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
        return run_focused_racf(root, args, gate, local_gate, &spec, selected);
    }
    run_focused_jcl(root, args, gate, local_gate, &spec, selected)
}

fn run_focused_racf(
    root: &Path,
    args: &ConformanceArgs,
    gate: Option<CoverageGate>,
    local_gate: bool,
    spec: &CompiledSpec,
    selected: usize,
) -> TaskResult {
    let limits = ConformanceLimits::default();
    let selection = if let Some(replay) = args.replay.as_deref() {
        RunnerSelection::replay(replay, limits).map_err(|problem| problem.to_string())?
    } else {
        make_focused_selection("racf-saf", gate, local_gate, args.shard, limits)?
    };
    let context = RunnerContext::new(repository_digest(root)?, "local-deterministic", limits)
        .map_err(|problem| problem.to_string())?;
    let dataset_handlers = dataset_conformance_runtime();
    let jcl_handlers = jcl_conformance::runtime();
    let cics_handlers = cics_pilot_runtime();
    let cobol_move_handlers = cobol_move_pilot_runtime();
    let cobol_arithmetic_handlers = cobol_arithmetic_pilot_runtime();
    let runtime = combined_conformance_runtime(
        spec,
        &dataset_handlers,
        &jcl_handlers,
        &cics_handlers,
        &cobol_move_handlers,
        &cobol_arithmetic_handlers,
        limits,
    )?;
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
    local_gate: bool,
    spec: &CompiledSpec,
    selected: usize,
) -> TaskResult {
    let limits = ConformanceLimits::default();
    let selection = if let Some(replay) = args.replay.as_deref() {
        RunnerSelection::replay(replay, limits).map_err(|problem| problem.to_string())?
    } else {
        make_focused_selection(
            args.subsystem.as_deref().unwrap_or("jcl-jes2"),
            gate,
            local_gate,
            args.shard,
            limits,
        )?
    };
    let context = RunnerContext::new(repository_digest(root)?, "local-deterministic", limits)
        .map_err(|problem| problem.to_string())?;
    let dataset_handlers = dataset_conformance_runtime();
    let jcl_handlers = jcl_conformance::runtime();
    let cics_handlers = cics_pilot_runtime();
    let cobol_move_handlers = cobol_move_pilot_runtime();
    let cobol_arithmetic_handlers = cobol_arithmetic_pilot_runtime();
    let runtime = combined_conformance_runtime(
        spec,
        &dataset_handlers,
        &jcl_handlers,
        &cics_handlers,
        &cobol_move_handlers,
        &cobol_arithmetic_handlers,
        limits,
    )?;
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
    cics: &'a CicsPilotRuntime,
    cobol_move: &'a CobolMovePilotRuntime,
    cobol_arithmetic: &'a CobolArithmeticPilotRuntime,
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
    let scenario_drivers = cics
        .bind(spec, &mut drivers, &mut observations, limits)
        .map_err(|problem| problem.to_string())?;
    cobol_move
        .bind(spec, &mut drivers, &mut observations, limits)
        .map_err(|problem| problem.to_string())?;
    cobol_arithmetic
        .bind(spec, &mut drivers, &mut observations, limits)
        .map_err(|problem| problem.to_string())?;
    mainframe_env_conformance::bind_mq_selected(spec, &mut drivers, &mut observations, limits)
        .map_err(|problem| problem.to_string())?;
    let runtime = mainframe_env_conformance::racf_runtime_with(
        spec,
        drivers,
        predicates,
        observations,
        limits,
    )
    .map_err(|problem| problem.to_string())?;
    runtime
        .with_scenario_drivers(spec, scenario_drivers, limits)
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
    let oracle_path =
        root.join("conformance/subsystems/jcl/oracles/jcl-licensed-differential.json");
    let oracle = json(&oracle_path)?;
    validate_schema_instance(
        &json(&root.join(
            "conformance/subsystems/jcl/schemas/jcl-licensed-differential-adapter.schema.json",
        ))?,
        &oracle,
        &oracle_path,
    )?;
    require(
        oracle["schema_version"]
            == Value::String("mainframe-env.jcl-licensed-differential-adapter@1".into())
            && oracle["target_subsystem"] == Value::String("jcl.planning".into())
            && oracle["status"] == Value::String("pending".into())
            && oracle["licensed_receipt_required_for_pass"] == Value::Bool(true)
            && oracle["generated_or_historical_result_counts_as_pass"] == Value::Bool(false),
        "JCL licensed differential adapter policy is invalid",
    )?;
    let gate_map_path = root.join("conformance/subsystems/jcl/inventory/jcl-path-gate-map.json");
    let gate_map = json(&gate_map_path)?;
    validate_schema_instance(
        &json(&root.join("conformance/subsystems/jcl/schemas/jcl-path-gate-map.schema.json"))?,
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

fn parse_focused_gate(value: Option<&str>) -> TaskResult<(Option<CoverageGate>, bool)> {
    match value {
        Some("local") => Ok((None, true)),
        Some(value) => Ok((Some(parse_coverage_gate(value)?), false)),
        None => Ok((None, false)),
    }
}

fn focused_gate_matches(
    candidate: CoverageGate,
    selected: Option<CoverageGate>,
    local: bool,
) -> bool {
    if local {
        candidate != CoverageGate::Differential
    } else {
        selected.is_none_or(|gate| gate == candidate)
    }
}

fn make_focused_selection(
    subsystem: &str,
    gate: Option<CoverageGate>,
    local: bool,
    shard: Option<u16>,
    limits: ConformanceLimits,
) -> TaskResult<RunnerSelection> {
    let selection = if local {
        RunnerSelection::local(subsystem, shard, limits)
    } else {
        RunnerSelection::focused(subsystem, gate, shard, limits)
    };
    selection.map_err(|problem| problem.to_string())
}

fn check_carddemo_cics(root: &Path) -> TaskResult {
    let receipt = verify_carddemo_cics_abi_from_env(
        &root.join("conformance/profiles/carddemo/inventory/carddemo-corpus.json"),
    )
    .map_err(|problem| problem.to_string())?;
    println!(
        "{}",
        serde_json::to_string_pretty(&receipt).map_err(|error| error.to_string())?
    );
    Ok(())
}

fn check_carddemo_cics_runtime(root: &Path) -> TaskResult {
    let receipt = verify_carddemo_cics_runtime_from_env(
        &root.join("conformance/profiles/carddemo/inventory/carddemo-corpus.json"),
    )
    .map_err(|problem| problem.to_string())?;
    println!(
        "{}",
        serde_json::to_string_pretty(&receipt).map_err(|error| error.to_string())?
    );
    Ok(())
}

fn check_carddemo_vsam(root: &Path) -> TaskResult {
    let receipt = verify_carddemo_vsam_from_env(
        &root.join("conformance/profiles/carddemo/inventory/carddemo-corpus.json"),
    )
    .map_err(|problem| problem.to_string())?;
    println!(
        "{}",
        serde_json::to_string_pretty(&receipt).map_err(|error| error.to_string())?
    );
    Ok(())
}

fn check_carddemo_dataset_catalog(root: &Path) -> TaskResult {
    let receipt = verify_carddemo_dataset_catalog_from_env(
        &root.join("conformance/profiles/carddemo/inventory/carddemo-corpus.json"),
    )
    .map_err(|problem| problem.to_string())?;
    println!(
        "{}",
        serde_json::to_string_pretty(&receipt).map_err(|error| error.to_string())?
    );
    Ok(())
}

fn check_carddemo_seeds(root: &Path) -> TaskResult {
    let receipt = verify_carddemo_seeds_from_env(
        &root.join("conformance/profiles/carddemo/inventory/carddemo-corpus.json"),
    )
    .map_err(|problem| problem.to_string())?;
    println!(
        "{}",
        serde_json::to_string_pretty(&receipt).map_err(|error| error.to_string())?
    );
    Ok(())
}

fn check_carddemo_security(root: &Path) -> TaskResult {
    let receipt = verify_carddemo_security_from_env(
        &root.join("conformance/profiles/carddemo/inventory/carddemo-corpus.json"),
    )
    .map_err(|problem| problem.to_string())?;
    println!(
        "{}",
        serde_json::to_string_pretty(&receipt).map_err(|error| error.to_string())?
    );
    Ok(())
}

fn check_carddemo_terminal(root: &Path) -> TaskResult {
    let receipt = verify_carddemo_terminal_from_env(
        &root.join("conformance/profiles/carddemo/inventory/carddemo-corpus.json"),
    )
    .map_err(|problem| problem.to_string())?;
    println!(
        "{}",
        serde_json::to_string_pretty(&receipt).map_err(|error| error.to_string())?
    );
    Ok(())
}

fn check_carddemo_base_online(root: &Path) -> TaskResult {
    let receipt = verify_carddemo_base_online_from_env(
        &root.join("conformance/profiles/carddemo/inventory/carddemo-corpus.json"),
    )
    .map_err(|problem| problem.to_string())?;
    println!(
        "{}",
        serde_json::to_string_pretty(&receipt).map_err(|error| error.to_string())?
    );
    Ok(())
}

fn check_carddemo_jcl(root: &Path) -> TaskResult {
    let receipt = verify_carddemo_jcl_from_env(
        &root.join("conformance/profiles/carddemo/inventory/carddemo-corpus.json"),
    )
    .map_err(|problem| problem.to_string())?;
    println!(
        "{}",
        serde_json::to_string_pretty(&receipt).map_err(|error| error.to_string())?
    );
    Ok(())
}

fn check_carddemo_utilities(root: &Path) -> TaskResult {
    let receipt = verify_carddemo_utilities_from_env(
        &root.join("conformance/profiles/carddemo/inventory/carddemo-corpus.json"),
    )
    .map_err(|problem| problem.to_string())?;
    println!(
        "{}",
        serde_json::to_string_pretty(&receipt).map_err(|error| error.to_string())?
    );
    Ok(())
}

fn check_carddemo_batch_programs(root: &Path) -> TaskResult {
    let receipt = verify_carddemo_batch_programs_from_env(
        &root.join("conformance/profiles/carddemo/inventory/carddemo-corpus.json"),
    )
    .map_err(|problem| problem.to_string())?;
    println!(
        "{}",
        serde_json::to_string_pretty(&receipt).map_err(|error| error.to_string())?
    );
    Ok(())
}

fn check_carddemo_base_batch(root: &Path) -> TaskResult {
    let receipt = verify_carddemo_base_batch_from_env(
        &root.join("conformance/profiles/carddemo/inventory/carddemo-corpus.json"),
    )
    .map_err(|problem| problem.to_string())?;
    println!(
        "{}",
        serde_json::to_string_pretty(&receipt).map_err(|error| error.to_string())?
    );
    Ok(())
}

fn check_carddemo_db2(root: &Path) -> TaskResult {
    let receipt = verify_carddemo_db2_from_env(
        &root.join("conformance/profiles/carddemo/inventory/carddemo-corpus.json"),
    )
    .map_err(|problem| problem.to_string())?;
    println!(
        "{}",
        serde_json::to_string_pretty(&receipt).map_err(|error| error.to_string())?
    );
    Ok(())
}

fn check_carddemo_ims(root: &Path) -> TaskResult {
    let receipt = verify_carddemo_ims_from_env(
        &root.join("conformance/profiles/carddemo/inventory/carddemo-corpus.json"),
    )
    .map_err(|problem| problem.to_string())?;
    println!(
        "{}",
        serde_json::to_string_pretty(&receipt).map_err(|error| error.to_string())?
    );
    Ok(())
}

fn check_carddemo_mq_authorization(root: &Path) -> TaskResult {
    let receipt = verify_carddemo_mq_authorization_from_env(
        &root.join("conformance/profiles/carddemo/inventory/carddemo-corpus.json"),
    )
    .map_err(|problem| problem.to_string())?;
    println!(
        "{}",
        serde_json::to_string_pretty(&receipt).map_err(|error| error.to_string())?
    );
    Ok(())
}

fn check_carddemo_operator_install(root: &Path) -> TaskResult {
    let inventory = root.join("conformance/profiles/carddemo/inventory/carddemo-corpus.json");
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
    let inventory = root.join("conformance/profiles/carddemo/inventory/carddemo-corpus.json");
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
        &root.join("conformance/profiles/carddemo/inventory/carddemo-corpus.json"),
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
        &root.join("conformance/profiles/carddemo/inventory/carddemo-corpus.json"),
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
        &root.join("conformance/profiles/carddemo/inventory/carddemo-corpus.json"),
    )
    .map_err(|problem| problem.to_string())?;
    println!(
        "{}",
        serde_json::to_string_pretty(&receipt).map_err(|error| error.to_string())?
    );
    Ok(())
}

fn check_carddemo_programs(root: &Path) -> TaskResult {
    let receipt = verify_carddemo_program_routing_from_env(
        &root.join("conformance/profiles/carddemo/inventory/carddemo-corpus.json"),
    )
    .map_err(|problem| problem.to_string())?;
    println!(
        "{}",
        serde_json::to_string_pretty(&receipt).map_err(|error| error.to_string())?
    );
    Ok(())
}

fn check_carddemo_resources(root: &Path) -> TaskResult {
    let receipt = verify_carddemo_resources_from_env(
        &root.join("conformance/profiles/carddemo/inventory/carddemo-corpus.json"),
    )
    .map_err(|problem| problem.to_string())?;
    println!(
        "{}",
        serde_json::to_string_pretty(&receipt).map_err(|error| error.to_string())?
    );
    Ok(())
}

fn check_carddemo_package(root: &Path) -> TaskResult {
    let receipt = verify_carddemo_application_package_from_env(
        &root.join("conformance/profiles/carddemo/inventory/carddemo-corpus.json"),
    )
    .map_err(|problem| problem.to_string())?;
    println!(
        "{}",
        serde_json::to_string_pretty(&receipt).map_err(|error| error.to_string())?
    );
    Ok(())
}

fn check_carddemo_host(root: &Path) -> TaskResult {
    let receipt = verify_carddemo_host_operands_from_env(
        &root.join("conformance/profiles/carddemo/inventory/carddemo-corpus.json"),
    )
    .map_err(|problem| problem.to_string())?;
    println!(
        "{}",
        serde_json::to_string_pretty(&receipt).map_err(|error| error.to_string())?
    );
    Ok(())
}

fn check_carddemo_file_call(root: &Path) -> TaskResult {
    let receipt = verify_carddemo_file_call_semantics_from_env(
        &root.join("conformance/profiles/carddemo/inventory/carddemo-corpus.json"),
    )
    .map_err(|problem| problem.to_string())?;
    println!(
        "{}",
        serde_json::to_string_pretty(&receipt).map_err(|error| error.to_string())?
    );
    Ok(())
}

fn check_carddemo_core(root: &Path) -> TaskResult {
    let receipt = verify_carddemo_core_semantics_from_env(
        &root.join("conformance/profiles/carddemo/inventory/carddemo-corpus.json"),
    )
    .map_err(|problem| problem.to_string())?;
    println!(
        "{}",
        serde_json::to_string_pretty(&receipt).map_err(|error| error.to_string())?
    );
    Ok(())
}

fn check_carddemo_control(root: &Path) -> TaskResult {
    let receipt = verify_carddemo_control_flow_from_env(
        &root.join("conformance/profiles/carddemo/inventory/carddemo-corpus.json"),
    )
    .map_err(|problem| problem.to_string())?;
    println!(
        "{}",
        serde_json::to_string_pretty(&receipt).map_err(|error| error.to_string())?
    );
    Ok(())
}

fn check_carddemo_layout(root: &Path) -> TaskResult {
    let receipt = verify_carddemo_data_layouts_from_env(
        &root.join("conformance/profiles/carddemo/inventory/carddemo-corpus.json"),
    )
    .map_err(|problem| problem.to_string())?;
    println!(
        "{}",
        serde_json::to_string_pretty(&receipt).map_err(|error| error.to_string())?
    );
    Ok(())
}

fn check_carddemo_closure(root: &Path) -> TaskResult {
    let receipt = verify_carddemo_source_closures_from_env(
        &root.join("conformance/profiles/carddemo/inventory/carddemo-corpus.json"),
    )
    .map_err(|problem| problem.to_string())?;
    println!(
        "{}",
        serde_json::to_string_pretty(&receipt).map_err(|error| error.to_string())?
    );
    Ok(())
}

fn check_carddemo_source(root: &Path) -> TaskResult {
    let receipt = verify_carddemo_source_preprocessing_from_env(
        &root.join("conformance/profiles/carddemo/inventory/carddemo-corpus.json"),
    )
    .map_err(|problem| problem.to_string())?;
    println!(
        "{}",
        serde_json::to_string_pretty(&receipt).map_err(|error| error.to_string())?
    );
    Ok(())
}

fn check_carddemo_corpus(root: &Path) -> TaskResult {
    let inventory_path = root.join("conformance/profiles/carddemo/inventory/carddemo-corpus.json");
    let inventory = json(&inventory_path)?;
    let historical = text(&inventory, "content_sha256", &inventory_path)?;
    let fixtures_path = root.join("conformance/subsystems/platform/fixtures/manifest.json");
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
        if current.join("docs/documentation-registry.json").is_file()
            && current.join("Cargo.toml").is_file()
        {
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

fn check_architecture(root: &Path) -> TaskResult {
    check_architecture_fast(root)?;
    check_runtime_unit_gates(root)
}

fn check_mq_mqi_registry(root: &Path) -> TaskResult {
    mq_status_catalog::check(root)?;
    let generator = root.join("tools/generate_mq_mqi_registry.py");
    require(
        generator.is_file(),
        "MQ MQI call-registry generator is missing",
    )?;
    let source_list = root.join("conformance/subsystems/mq/mq/source-call-list.json");
    let source_list_schema =
        root.join("conformance/subsystems/mq/schemas/mq-source-call-list.schema.json");
    validate_schema_instance(
        &json(&source_list_schema)?,
        &json(&source_list)?,
        &source_list,
    )?;
    let contract_catalog = root.join("conformance/subsystems/mq/mq/structure-status-catalog.json");
    let contract_schema =
        root.join("conformance/subsystems/mq/schemas/mq-structure-status-catalog.schema.json");
    validate_schema_instance(
        &json(&contract_schema)?,
        &json(&contract_catalog)?,
        &contract_catalog,
    )?;
    let status = Command::new("python3")
        .arg("-B")
        .arg(&generator)
        .arg("--check")
        .current_dir(root)
        .status()
        .map_err(|error| format!("MQ MQI call-registry freshness guard: {error}"))?;
    require(
        status.success(),
        "MQ MQI call-registry freshness guard failed",
    )
}

fn check_mq_licensed_contract(root: &Path) -> TaskResult {
    let adapter = root.join("conformance/subsystems/mq/oracles/mq-licensed-differential.json");
    let adapter_schema =
        root.join("conformance/subsystems/mq/schemas/mq-licensed-differential-adapter.schema.json");
    validate_schema_instance(&json(&adapter_schema)?, &json(&adapter)?, &adapter)?;

    let fixtures =
        root.join("conformance/subsystems/mq/fixtures/mq-licensed-differential-cases.json");
    let fixture_schema = root
        .join("conformance/subsystems/mq/schemas/mq-licensed-differential-fixtures.schema.json");
    validate_schema_instance(&json(&fixture_schema)?, &json(&fixtures)?, &fixtures)?;

    let receipt_schema =
        root.join("conformance/subsystems/mq/schemas/mq-licensed-differential-receipt.schema.json");
    compile_draft_2020_12_schema(&json(&receipt_schema)?, &receipt_schema)?;

    for (label, tool, arguments) in [
        (
            "MQ licensed fixture freshness guard",
            "conformance/subsystems/mq/tools/generate_mq_licensed_fixtures.py",
            ["--check"].as_slice(),
        ),
        (
            "MQ licensed receipt verifier",
            "conformance/subsystems/mq/tools/verify_mq_licensed_differential.py",
            ["--check"].as_slice(),
        ),
    ] {
        let status = Command::new("python3")
            .arg("-B")
            .arg(root.join(tool))
            .args(arguments)
            .current_dir(root)
            .status()
            .map_err(|error| format!("{label}: {error}"))?;
        require(status.success(), &format!("{label} failed"))?;
    }
    Ok(())
}

fn check_ims_licensed_contract(root: &Path) -> TaskResult {
    for (instance, schema) in [
        (
            "conformance/subsystems/ims/oracles/ims-licensed-differential.json",
            "conformance/subsystems/ims/schemas/ims-licensed-differential-adapter.schema.json",
        ),
        (
            "conformance/subsystems/ims/fixtures/ims-licensed-differential-cases.json",
            "conformance/subsystems/ims/schemas/ims-licensed-differential-fixtures.schema.json",
        ),
    ] {
        let instance = root.join(instance);
        let schema = root.join(schema);
        validate_schema_instance(&json(&schema)?, &json(&instance)?, &instance)?;
    }
    let receipt_schema = root
        .join("conformance/subsystems/ims/schemas/ims-licensed-differential-receipt.schema.json");
    compile_draft_2020_12_schema(&json(&receipt_schema)?, &receipt_schema)?;
    for tool in [
        "conformance/subsystems/ims/tools/generate_ims_licensed_fixtures.py",
        "conformance/subsystems/ims/tools/verify_ims_licensed_differential.py",
    ] {
        let status = Command::new("python3")
            .arg("-B")
            .arg(root.join(tool))
            .arg("--check")
            .current_dir(root)
            .status()
            .map_err(|error| format!("IMS licensed contract {tool}: {error}"))?;
        require(
            status.success(),
            &format!("IMS licensed contract {tool} failed"),
        )?;
    }
    Ok(())
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
    let participant_contract =
        root.join("conformance/subsystems/integration/contracts/transaction-participant.json");
    let participant_schema =
        root.join("conformance/subsystems/integration/schemas/transaction-participant.schema.json");
    validate_schema_instance(
        &json(&participant_schema)?,
        &json(&participant_contract)?,
        &participant_contract,
    )?;
    let participant_fixtures = root.join(
        "conformance/subsystems/integration/fixtures/transaction-participant-compatibility.json",
    );
    let participant_fixture_schema = root.join(
        "conformance/subsystems/integration/schemas/transaction-participant-fixtures.schema.json",
    );
    validate_schema_instance(
        &json(&participant_fixture_schema)?,
        &json(&participant_fixtures)?,
        &participant_fixtures,
    )?;
    let participant_generator = root.join("tools/generate_transaction_participant.py");
    require(
        participant_generator.is_file(),
        "transaction participant generator is missing",
    )?;
    let status = Command::new("python3")
        .arg("-B")
        .arg(&participant_generator)
        .arg("--check")
        .current_dir(root)
        .status()
        .map_err(|error| format!("transaction participant freshness guard: {error}"))?;
    require(
        status.success(),
        "transaction participant freshness guard failed",
    )?;
    let participant_bindings = root.join("tools/check_transaction_participant.py");
    require(
        participant_bindings.is_file(),
        "transaction participant binding guard is missing",
    )?;
    let status = Command::new("python3")
        .arg("-B")
        .arg(&participant_bindings)
        .current_dir(root)
        .status()
        .map_err(|error| format!("transaction participant binding guard: {error}"))?;
    require(
        status.success(),
        "transaction participant binding guard failed",
    )?;
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
    let provider_rows = root.join("tools/check_provider_rows.py");
    require(
        provider_rows.is_file(),
        "provider row persistence architecture guard is missing",
    )?;
    let status = Command::new("python3")
        .arg("-B")
        .arg(&provider_rows)
        .current_dir(root)
        .status()
        .map_err(|error| format!("provider row persistence guard: {error}"))?;
    require(
        status.success(),
        "provider row persistence architecture guard failed",
    )?;
    let storage_profile = root.join("tools/check_storage_profile.py");
    require(
        storage_profile.is_file(),
        "durable storage profile architecture guard is missing",
    )?;
    let status = Command::new("python3")
        .arg("-B")
        .arg(&storage_profile)
        .current_dir(root)
        .status()
        .map_err(|error| format!("durable storage profile guard: {error}"))?;
    require(
        status.success(),
        "durable storage profile architecture guard failed",
    )?;
    let enterprise_authorization = root.join("tools/check_enterprise_authorization.py");
    require(
        enterprise_authorization.is_file(),
        "enterprise authorization architecture guard is missing",
    )?;
    let status = Command::new("python3")
        .arg("-B")
        .arg(&enterprise_authorization)
        .current_dir(root)
        .status()
        .map_err(|error| format!("enterprise authorization guard: {error}"))?;
    require(
        status.success(),
        "enterprise authorization architecture guard failed",
    )?;
    let retention = root.join("tools/check_retention_lifecycle.py");
    require(
        retention.is_file(),
        "durable retention lifecycle architecture guard is missing",
    )?;
    let status = Command::new("python3")
        .arg("-B")
        .arg(&retention)
        .current_dir(root)
        .status()
        .map_err(|error| format!("durable retention lifecycle guard: {error}"))?;
    require(
        status.success(),
        "durable retention lifecycle architecture guard failed",
    )?;
    check_mq_mqi_registry(root)?;
    let cics_descriptors = root.join("tools/generate_cics_descriptors.py");
    require(
        cics_descriptors.is_file(),
        "CICS command descriptor generator is missing",
    )?;
    let cics_descriptor_catalog =
        root.join("conformance/subsystems/cics/application/cics/command-descriptors.json");
    let cics_descriptor_schema = root.join(
        "conformance/subsystems/cics/application/schemas/cics-command-descriptors.schema.json",
    );
    validate_schema_instance(
        &json(&cics_descriptor_schema)?,
        &json(&cics_descriptor_catalog)?,
        &cics_descriptor_catalog,
    )?;
    let cics_contract_catalog = root.join(
        "conformance/subsystems/cics/application/generated/cics-application-command-contracts.json",
    );
    let cics_contract_schema =
        root.join("conformance/subsystems/cics/application/schemas/cics-application-command-contracts.schema.json");
    validate_schema_instance(
        &json(&cics_contract_schema)?,
        &json(&cics_contract_catalog)?,
        &cics_contract_catalog,
    )?;
    let status = Command::new("python3")
        .arg("-B")
        .arg(&cics_descriptors)
        .arg("--check")
        .current_dir(root)
        .status()
        .map_err(|error| format!("CICS command descriptor freshness guard: {error}"))?;
    require(
        status.success(),
        "CICS command descriptor freshness guard failed",
    )?;
    let cics_source_map = root.join("tools/generate_cics_source_map.py");
    require(
        cics_source_map.is_file(),
        "CICS sources-a map generator is missing",
    )?;
    check_cics_source_map_schemas(root)?;
    let status = Command::new("python3")
        .arg("-B")
        .arg(&cics_source_map)
        .args(["--batch", "all"])
        .arg("--check")
        .current_dir(root)
        .status()
        .map_err(|error| format!("CICS source-map freshness guard: {error}"))?;
    require(status.success(), "CICS source-map freshness guard failed")?;
    for family in ["spi", "fepi"] {
        let status = Command::new("python3")
            .arg("-B")
            .arg(&cics_source_map)
            .args(["--family", family, "--check"])
            .current_dir(root)
            .status()
            .map_err(|error| format!("CICS {family} source-map freshness guard: {error}"))?;
        require(
            status.success(),
            &format!("CICS {family} source-map freshness guard failed"),
        )?;
    }
    let cics_source_corpus = root
        .join("conformance/subsystems/cics/application/tools/fetch_cics_application_sources.py");
    require(
        cics_source_corpus.is_file(),
        "CICS source corpus generator is missing",
    )?;
    let cics_source_corpus_schema =
        root.join("conformance/subsystems/cics/application/schemas/cics-source-corpus.schema.json");
    let topic_manifest_schema =
        root.join("conformance/subsystems/coverage/schemas/topic-manifest.schema.json");
    for batch in ["a", "b", "c"] {
        let corpus = root.join(format!(
            "conformance/subsystems/cics/application/cics/application-api-sources-{batch}-corpus.json"
        ));
        validate_schema_instance(&json(&cics_source_corpus_schema)?, &json(&corpus)?, &corpus)?;
        let manifest = root.join(format!(
            "conformance/subsystems/cics/application/manifests/cics-application-api-sources-{batch}-topics.json"
        ));
        validate_schema_instance(&json(&topic_manifest_schema)?, &json(&manifest)?, &manifest)?;
    }
    let cics_browser_receipt =
        root.join("conformance/subsystems/cics/application/cics/application-api-sources-a-browser-verification.json");
    let cics_browser_receipt_schema =
        root.join("conformance/subsystems/cics/application/schemas/cics-browser-source-verification.schema.json");
    validate_schema_instance(
        &json(&cics_browser_receipt_schema)?,
        &json(&cics_browser_receipt)?,
        &cics_browser_receipt,
    )?;
    let status = Command::new("python3")
        .arg("-B")
        .arg(&cics_source_corpus)
        .args(["--batch", "all"])
        .arg("--check")
        .current_dir(root)
        .status()
        .map_err(|error| format!("CICS source corpus freshness guard: {error}"))?;
    require(
        status.success(),
        "CICS source corpus freshness guard failed",
    )?;
    let cics_source_extraction_schema = root
        .join("conformance/subsystems/cics/application/schemas/cics-source-extraction.schema.json");
    let cics_source_candidates_schema = root
        .join("conformance/subsystems/cics/application/schemas/cics-source-candidates.schema.json");
    let cics_source_projector = root
        .join("conformance/subsystems/cics/application/tools/extract_cics_application_sources.py");
    require(
        cics_source_projector.is_file(),
        "CICS source projector is missing",
    )?;
    for batch in ["a", "b", "c"] {
        let extraction = root.join(format!(
            "conformance/subsystems/cics/application/cics/application-api-sources-{batch}-extraction.json"
        ));
        validate_schema_instance(
            &json(&cics_source_extraction_schema)?,
            &json(&extraction)?,
            &extraction,
        )?;
        let candidates = root.join(format!(
            "conformance/subsystems/cics/application/generated/cics-application-api-sources-{batch}-candidates.json"
        ));
        validate_schema_instance(
            &json(&cics_source_candidates_schema)?,
            &json(&candidates)?,
            &candidates,
        )?;
        let status = Command::new("python3")
            .arg("-B")
            .arg(&cics_source_projector)
            .args(["--batch", batch, "--check"])
            .current_dir(root)
            .status()
            .map_err(|error| format!("CICS sources-{batch} projection freshness guard: {error}"))?;
        require(
            status.success(),
            &format!("CICS sources-{batch} projection freshness guard failed"),
        )?;
    }
    check_cics_source_review(root)?;
    let module_boundaries = root.join("tools/check_module_boundaries.py");
    require(
        module_boundaries.is_file(),
        "Rust module boundary architecture guard is missing",
    )?;
    let status = Command::new("python3")
        .arg("-B")
        .arg(&module_boundaries)
        .current_dir(root)
        .status()
        .map_err(|error| format!("Rust module boundary guard: {error}"))?;
    require(
        status.success(),
        "Rust module boundary architecture guard failed",
    )?;
    let typed_semantic_boundaries = root.join("tools/check_typed_semantic_boundaries.py");
    require(
        typed_semantic_boundaries.is_file(),
        "typed semantic boundary architecture guard is missing",
    )?;
    let status = Command::new("python3")
        .arg("-B")
        .arg(&typed_semantic_boundaries)
        .current_dir(root)
        .status()
        .map_err(|error| format!("typed semantic boundary guard: {error}"))?;
    require(
        status.success(),
        "typed semantic boundary architecture guard failed",
    )?;
    Ok(())
}

fn check_cics_source_review(root: &Path) -> TaskResult {
    for batch in ["a", "b", "c"] {
        check_cics_source_review_batch(root, batch)?;
    }
    Ok(())
}

fn check_cics_source_review_batch(root: &Path, batch: &str) -> TaskResult {
    let review = root.join(format!(
        "conformance/subsystems/cics/application/cics/application-api-sources-{batch}-review.json"
    ));
    let schema =
        root.join("conformance/subsystems/cics/application/schemas/cics-source-review.schema.json");
    validate_schema_instance(&json(&schema)?, &json(&review)?, &review)?;

    let checker = root
        .join("conformance/subsystems/cics/application/tools/review_cics_application_sources.py");
    require(
        checker.is_file(),
        "CICS sources-a review checker is missing",
    )?;
    let status = Command::new("python3")
        .arg("-B")
        .arg(&checker)
        .args(["--batch", batch, "--check"])
        .current_dir(root)
        .status()
        .map_err(|error| format!("CICS sources-{batch} review freshness guard: {error}"))?;
    require(
        status.success(),
        &format!("CICS sources-{batch} review freshness guard failed"),
    )
}

fn check_declared_dependency_graph(root: &Path) -> TaskResult {
    let metadata = workspace_metadata(root)?;
    let actual = normal_internal_edges(&metadata)?;
    let path = root.join("conformance/subsystems/platform/inventory/dependency-graph.json");
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
        root.join("conformance/profiles/carddemo/inventory/dependency-graph-additions.json"),
        root.join("conformance/subsystems/coverage/inventory/dependency-additions.json"),
        root.join("conformance/subsystems/cobol/structure/inventory/dependency-additions.json"),
        root.join("conformance/subsystems/racf/inventory/dependency-additions.json"),
        root.join("conformance/subsystems/dataset/inventory/dependency-additions.json"),
        root.join("conformance/subsystems/jcl/inventory/dependency-additions.json"),
        root.join("conformance/subsystems/jes/inventory/dependency-additions.json"),
        root.join("conformance/subsystems/cics/system/inventory/dependency-additions.json"),
        root.join("conformance/subsystems/ims/inventory/dependency-additions.json"),
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
    let status = Command::new("python3")
        .args(["-B", "tools/check_execution_route.py"])
        .current_dir(root)
        .status()
        .map_err(|error| format!("common execution route guard: {error}"))?;
    require(
        status.success(),
        "common production execution route guard failed",
    )
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
    let inventory_path = root.join("conformance/subsystems/platform/inventory/packages.json");
    let profiles_path = root.join("conformance/subsystems/platform/inventory/profiles.json");
    let inventory = json(&inventory_path)?;
    let profiles = json(&profiles_path)?;
    let mut known = array(&inventory, "packages", &inventory_path)?
        .iter()
        .filter_map(|row| row.get("name").and_then(Value::as_str).map(str::to_string))
        .collect::<BTreeSet<_>>();
    for additions_path in [
        root.join("conformance/subsystems/coverage/inventory/package-additions.json"),
        root.join("conformance/subsystems/jes/inventory/package-additions.json"),
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
        root.join("conformance/subsystems/cobol/structure/inventory/dependency-additions.json"),
        root.join("conformance/subsystems/jes/inventory/dependency-additions.json"),
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

fn subsystem_schema_files(root: &Path) -> TaskResult<Vec<PathBuf>> {
    let mut files = Vec::new();
    for directory in ["conformance/subsystems", "conformance/profiles"] {
        if root.join(directory).is_dir() {
            collect_extension(&root.join(directory), OsStr::new("json"), &mut files)?;
        }
    }
    // Vendor bundles have their own pinned, offline registry and may contain
    // metadata documents alongside schemas.
    files.retain(|file| {
        file.components().any(|part| part.as_os_str() == "schemas")
            && !file.components().any(|part| part.as_os_str() == "vendor")
    });
    files.sort();
    Ok(files)
}

fn check_schemas(root: &Path) -> TaskResult {
    check_cics_source_map_schemas(root)?;
    cics_system_families::check_present(root)?;
    let files = subsystem_schema_files(root)?;
    require(!files.is_empty(), "no subsystem schemas found")?;
    carddemo_readacct::check_vendor_schemas(root)?;
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
    let family_path = root.join(
        "conformance/subsystems/cics/application/oracles/cics-licensed-family-bif-builtins-v1.json",
    );
    let family_schema = root
        .join("conformance/subsystems/cics/application/schemas/cics-licensed-family.schema.json");
    let family = json(&family_path)?;
    validate_schema_instance(&json(&family_schema)?, &family, &family_path)?;
    let fixture_path = root.join(text(&family, "fixture_path", &family_path)?);
    let fixture = json(&fixture_path)?;
    require(
        fixture["family_id"] == family["family_id"]
            && fixture["schema_version"] == "mainframe-env.cics-oracle-family-fixtures@1"
            && family["observations"].as_array().is_some_and(|scenarios| {
                fixture["expected_observations"]
                    .as_array()
                    .is_some_and(|expected| {
                        scenarios.len() == expected.len()
                            && scenarios
                                .iter()
                                .zip(expected)
                                .all(|(scenario, observation)| {
                                    scenario["scenario_id"] == observation["scenario_id"]
                                        && serde_json::from_value::<CicsOracleObservation>(
                                            observation.clone(),
                                        )
                                        .is_ok()
                                })
                    })
            }),
        "CICS BIF oracle family manifest/fixture closure drifted",
    )?;
    validate_0_2_schema_artifacts(root)?;
    let zosmf_normalization_path =
        root.join("conformance/subsystems/zosmf/catalogs/zosmf-normalization.json");
    let zosmf_normalization_schema =
        root.join("conformance/subsystems/zosmf/schemas/zosmf-normalization.schema.json");
    validate_schema_instance(
        &json(&zosmf_normalization_schema)?,
        &json(&zosmf_normalization_path)?,
        &zosmf_normalization_path,
    )?;
    for (artifact, schema) in [
        (
            "conformance/subsystems/zosmf/generated/zosmf-contracts.json",
            "conformance/subsystems/zosmf/schemas/zosmf-generated-contracts.schema.json",
        ),
        (
            "conformance/subsystems/zosmf/generated/zosmf-collision-report.json",
            "conformance/subsystems/zosmf/schemas/zosmf-collision-report.schema.json",
        ),
        (
            "conformance/subsystems/zosmf/generated/zosmf-closure-report.json",
            "conformance/subsystems/zosmf/schemas/zosmf-closure-report.schema.json",
        ),
    ] {
        let artifact_path = root.join(artifact);
        validate_schema_instance(
            &json(&root.join(schema))?,
            &json(&artifact_path)?,
            &artifact_path,
        )?;
    }
    let spi_fepi_source_authority =
        root.join("conformance/subsystems/cics/system/cics/spi-fepi-source-authority.json");
    let spi_fepi_source_schema = root.join(
        "conformance/subsystems/cics/system/schemas/cics-spi-fepi-source-authority.schema.json",
    );
    validate_schema_instance(
        &json(&spi_fepi_source_schema)?,
        &json(&spi_fepi_source_authority)?,
        &spi_fepi_source_authority,
    )?;
    let spi_fepi_identity_catalog = root
        .join("conformance/subsystems/cics/system/generated/cics-spi-fepi-identity-catalog.json");
    let spi_fepi_identity_schema = root.join(
        "conformance/subsystems/cics/system/schemas/cics-spi-fepi-identity-catalog.schema.json",
    );
    validate_schema_instance(
        &json(&spi_fepi_identity_schema)?,
        &json(&spi_fepi_identity_catalog)?,
        &spi_fepi_identity_catalog,
    )?;
    check_licensed_environment_requirements(root)?;
    let inventory_path =
        root.join("conformance/subsystems/dataset/inventory/dataset-programming-surface.json");
    let schema_path =
        root.join("conformance/subsystems/dataset/schemas/dataset-programming-surface.schema.json");
    validate_schema_instance(
        &json(&schema_path)?,
        &json(&inventory_path)?,
        &inventory_path,
    )?;
    let dependency_path =
        root.join("conformance/subsystems/dataset/inventory/dependency-additions.json");
    let dependency_schema = root
        .join("conformance/subsystems/dataset/schemas/dataset-dependency-additions.schema.json");
    validate_schema_instance(
        &json(&dependency_schema)?,
        &json(&dependency_path)?,
        &dependency_path,
    )?;
    let migration_path =
        root.join("conformance/subsystems/dataset/migrations/dataset-state-v2-to-v3.json");
    let migration_schema =
        root.join("conformance/subsystems/dataset/schemas/dataset-state-migration.schema.json");
    validate_schema_instance(
        &json(&migration_schema)?,
        &json(&migration_path)?,
        &migration_path,
    )?;
    let migration_v4_path =
        root.join("conformance/subsystems/dataset/migrations/dataset-state-v3-to-v4.json");
    let migration_v4_schema =
        root.join("conformance/subsystems/dataset/schemas/dataset-state-v4-migration.schema.json");
    validate_schema_instance(
        &json(&migration_v4_schema)?,
        &json(&migration_v4_path)?,
        &migration_v4_path,
    )?;
    let migration_v5_path =
        root.join("conformance/subsystems/dataset/migrations/dataset-state-v4-to-v5.json");
    let migration_v5_schema =
        root.join("conformance/subsystems/dataset/schemas/dataset-state-v5-migration.schema.json");
    validate_schema_instance(
        &json(&migration_v5_schema)?,
        &json(&migration_v5_path)?,
        &migration_v5_path,
    )?;
    let migration_v6_path =
        root.join("conformance/subsystems/dataset/migrations/dataset-state-v5-to-v6.json");
    let migration_v6_schema =
        root.join("conformance/subsystems/dataset/schemas/dataset-state-v6-migration.schema.json");
    validate_schema_instance(
        &json(&migration_v6_schema)?,
        &json(&migration_v6_path)?,
        &migration_v6_path,
    )?;
    let organization_fixture =
        root.join("conformance/subsystems/dataset/fixtures/dataset-organizations.json");
    let organization_schema = root
        .join("conformance/subsystems/dataset/schemas/dataset-organization-fixtures.schema.json");
    validate_schema_instance(
        &json(&organization_schema)?,
        &json(&organization_fixture)?,
        &organization_fixture,
    )?;
    let ams_fixture = root.join("conformance/subsystems/dataset/fixtures/ams-commands.json");
    let ams_schema =
        root.join("conformance/subsystems/dataset/schemas/ams-command-fixtures.schema.json");
    validate_schema_instance(&json(&ams_schema)?, &json(&ams_fixture)?, &ams_fixture)?;
    let jes_migration =
        root.join("conformance/subsystems/jes/migrations/jes-durable-job-v1-to-v2.json");
    let jes_migration_schema =
        root.join("conformance/subsystems/jes/schemas/jes-state-migration.schema.json");
    validate_schema_instance(
        &json(&jes_migration_schema)?,
        &json(&jes_migration)?,
        &jes_migration,
    )?;
    let jes_differential =
        root.join("conformance/subsystems/jes/oracles/jes-licensed-differential.json");
    let jes_differential_schema = root
        .join("conformance/subsystems/jes/schemas/jes-licensed-differential-adapter.schema.json");
    validate_schema_instance(
        &json(&jes_differential_schema)?,
        &json(&jes_differential)?,
        &jes_differential,
    )?;
    let jes_dependency_additions =
        root.join("conformance/subsystems/jes/inventory/dependency-additions.json");
    let jes_dependency_additions_schema =
        root.join("conformance/subsystems/jes/schemas/jes-dependency-additions.schema.json");
    validate_schema_instance(
        &json(&jes_dependency_additions_schema)?,
        &json(&jes_dependency_additions)?,
        &jes_dependency_additions,
    )?;
    let jes_package_additions =
        root.join("conformance/subsystems/jes/inventory/package-additions.json");
    let jes_package_additions_schema =
        root.join("conformance/subsystems/jes/schemas/jes-package-additions.schema.json");
    validate_schema_instance(
        &json(&jes_package_additions_schema)?,
        &json(&jes_package_additions)?,
        &jes_package_additions,
    )?;
    for (artifact, schema) in [
        (
            "conformance/subsystems/jes/oracles/carddemo-tranrept-reference@1.json",
            "conformance/subsystems/jes/schemas/carddemo-tranrept-reference.schema.json",
        ),
        (
            "conformance/subsystems/jes/inventory/jes-dd-surface.json",
            "conformance/subsystems/jes/schemas/jes-dd-surface.schema.json",
        ),
        (
            "conformance/subsystems/jes/inventory/jes-operations-surface.json",
            "conformance/subsystems/jes/schemas/jes-operations-surface.schema.json",
        ),
        (
            "conformance/subsystems/jes/inventory/jes-recovery-surface.json",
            "conformance/subsystems/jes/schemas/jes-recovery-surface.schema.json",
        ),
        (
            "conformance/subsystems/jes/inventory/jes-spool-output-surface.json",
            "conformance/subsystems/jes/schemas/jes-spool-output-surface.schema.json",
        ),
        (
            "conformance/subsystems/jes/inventory/jes-utility-surface.json",
            "conformance/subsystems/jes/schemas/jes-utility-surface.schema.json",
        ),
        (
            "conformance/subsystems/jes/migrations/jes-embedded-spool-to-artifacts.json",
            "conformance/subsystems/jes/schemas/jes-spool-migration.schema.json",
        ),
    ] {
        let artifact = root.join(artifact);
        let schema = root.join(schema);
        validate_schema_instance(&json(&schema)?, &json(&artifact)?, &artifact)?;
    }
    let recovery_path = root.join("conformance/subsystems/jes/inventory/jes-recovery-surface.json");
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
    Ok(())
}

fn check_licensed_environment_requirements(root: &Path) -> TaskResult {
    let environment_requirements =
        root.join("conformance/subsystems/certification/environment/requirements.json");
    let environment_requirements_schema =
        root.join("conformance/subsystems/certification/schemas/licensed-environment-requirements.schema.json");
    let environment_requirements_value = json(&environment_requirements)?;
    validate_schema_instance(
        &json(&environment_requirements_schema)?,
        &environment_requirements_value,
        &environment_requirements,
    )?;
    let slots = array(
        &environment_requirements_value,
        "slots",
        &environment_requirements,
    )?;
    unique_rows(slots, "slot_id", &environment_requirements)?;
    require(
        slots.iter().all(|slot| {
            slot["environment_status"] == "pending"
                && slot["differential_status"] == "pending"
                && slot["recorded_pending"]["numerator"] == 0
        }),
        "CER-1701 environment slots must remain pending with zero licensed credit",
    )?;
    for (slot_id, denominator) in [
        ("cobol", 153),
        ("racf-saf", 48),
        ("dataset-vsam-ams", 36),
        ("jes2", 16),
    ] {
        require(
            slots.iter().any(|slot| {
                slot["slot_id"] == slot_id && slot["recorded_pending"]["denominator"] == denominator
            }),
            &format!("CER-1701 changed the historical {slot_id} pending denominator"),
        )?;
    }
    let source_index_path = root.join("conformance/subsystems/coverage/catalogs/index.json");
    let source_index = json(&source_index_path)?;
    let baselines = array(&source_index, "baselines", &source_index_path)?;
    for slot in slots {
        for required in array(slot, "required_baselines", &environment_requirements)? {
            let baseline_id = text(required, "baseline_id", &environment_requirements)?;
            let baseline = baselines
                .iter()
                .find(|baseline| baseline["id"] == baseline_id)
                .ok_or_else(|| format!("CER-1701 names unknown baseline {baseline_id}"))?;
            require(
                required["topic_manifest_sha256"] == baseline["source"]["sha256"]
                    && required["catalog_sha256"] == baseline["catalog_sha256"],
                &format!("CER-1701 source identity drifted for {baseline_id}"),
            )?;
        }
    }

    let harness_path = root.join("conformance/subsystems/certification/oracles/harnesses.json");
    let harness_schema_path = root
        .join("conformance/subsystems/certification/schemas/oracle-harness-registry.schema.json");
    let harness_value = json(&harness_path)?;
    validate_schema_instance(&json(&harness_schema_path)?, &harness_value, &harness_path)?;
    let harness_bytes =
        fs::read(&harness_path).map_err(|error| format!("{}: {error}", harness_path.display()))?;
    let harness = validate_oracle_harness_registry(&harness_bytes)?;
    let harness_slots = array(&harness_value, "slots", &harness_path)?;
    unique_rows(harness_slots, "slot_id", &harness_path)?;
    require(
        harness.slot_ids().collect::<BTreeSet<_>>()
            == slots
                .iter()
                .filter_map(|slot| slot["slot_id"].as_str())
                .collect::<BTreeSet<_>>(),
        "CER-1701 environment and harness slot sets differ",
    )?;
    for harness_slot in harness_slots {
        let slot_id = text(harness_slot, "slot_id", &harness_path)?;
        let requirement = slots
            .iter()
            .find(|slot| slot["slot_id"] == slot_id)
            .ok_or_else(|| format!("CER-1701 harness slot {slot_id} has no requirement"))?;
        let harness_baselines = array(harness_slot, "required_baselines", &harness_path)?
            .iter()
            .filter_map(Value::as_str)
            .collect::<BTreeSet<_>>();
        let requirement_baselines =
            array(requirement, "required_baselines", &environment_requirements)?
                .iter()
                .filter_map(|baseline| baseline["baseline_id"].as_str())
                .collect::<BTreeSet<_>>();
        require(
            harness_baselines == requirement_baselines,
            &format!("CER-1701 harness baseline set drifted for {slot_id}"),
        )?;
        if let Some(policy_path) = harness_slot["adapter"]["policy_path"].as_str() {
            require(
                root.join(policy_path).is_file(),
                &format!("CER-1701 adapter policy is missing for {slot_id}"),
            )?;
        }
        match harness_slot["fixture"]["digest_rule"].as_str() {
            Some("cobol-four-fixture-length-prefix-sha256") => require(
                harness_slot["fixture"]["digest"] == licensed_fixture_digest(),
                "CER-1701 COBOL independent fixture digest drifted",
            )?,
            Some("file-bytes-sha256") => {
                let fixture_path = harness_slot["fixture"]["path"]
                    .as_str()
                    .ok_or_else(|| format!("CER-1701 fixture path is missing for {slot_id}"))?;
                let actual = format!("sha256:{}", file_digest(&root.join(fixture_path))?);
                require(
                    harness_slot["fixture"]["digest"].as_str() == Some(actual.as_str()),
                    &format!("CER-1701 independent fixture digest drifted for {slot_id}"),
                )?;
            }
            Some("pending") => require(
                harness_slot["fixture"]["path"].is_null()
                    && harness_slot["fixture"]["digest"].is_null(),
                &format!("CER-1701 pending fixture has an identity for {slot_id}"),
            )?,
            _ => {
                return Err(format!(
                    "CER-1701 fixture digest rule is unknown for {slot_id}"
                ));
            }
        }
    }
    Ok(())
}

fn check_licensed_harness(root: &Path) -> TaskResult {
    check_licensed_environment_requirements(root)?;
    let registry_path = root.join("conformance/subsystems/certification/oracles/harnesses.json");
    let environment_path =
        root.join("conformance/subsystems/certification/fixtures/synthetic-environment.json");
    let receipt_path =
        root.join("conformance/subsystems/certification/fixtures/synthetic-receipt.json");
    let legacy_path =
        root.join("conformance/subsystems/certification/fixtures/synthetic-cics-capture.json");
    let registry = fs::read(&registry_path)
        .map_err(|error| format!("{}: {error}", registry_path.display()))?;
    let environment = fs::read(&environment_path)
        .map_err(|error| format!("{}: {error}", environment_path.display()))?;
    let receipt =
        fs::read(&receipt_path).map_err(|error| format!("{}: {error}", receipt_path.display()))?;
    let legacy =
        fs::read(&legacy_path).map_err(|error| format!("{}: {error}", legacy_path.display()))?;
    let expectation = OracleCandidateExpectation {
        slot_id: "cics".into(),
        source_commit: "5ab706b1dd069e26db7cb9a2b66e921c9001fc39".into(),
        source_tree_digest:
            "sha256:c4e5c4d7d40d6c5618ae4cde2e478d18a531f690f4eebacf44cf16959f70c984".into(),
        artifacts: BTreeMap::from([(
            "synthetic-candidate-artifact".into(),
            "sha256:6a1c9843c16282e72cc4acfd454948e3c2ca56898510d21e12fb0c52e5e34a04".into(),
        )]),
        catalogs: BTreeMap::from([(
            "ibm-cics-ts-6x-2026-08-31".into(),
            format!(
                "sha256:{}",
                file_digest(&root.join("conformance/subsystems/coverage/catalogs/cics.json"))?
            ),
        )]),
        conformance_spec_digest: format!(
            "sha256:{}",
            file_digest(&root.join("conformance/spec/v1/spec.json"))?
        ),
        fixture_digest: format!(
            "sha256:{}",
            file_digest(
                &root.join("conformance/subsystems/cics/application/cics/pilot-fixtures.json")
            )?
        ),
        oracle_adapter_digest: format!(
            "sha256:{}",
            file_digest(&root.join(
                "conformance/subsystems/cics/application/oracles/cics-licensed-differential.json"
            ))?
        ),
        normalization_policy_digest:
            "sha256:111f3304ea7c6c9f3a7d380756ebb7cc5180283b59fa4e38dc8f12d358cd2ce9".into(),
    };
    let validation =
        validate_oracle_harness_receipt(&receipt, &environment, &registry, &legacy, &expectation)?;
    require(
        validation.kind() == OracleHarnessValidationKind::PlumbingOnly
            && validation.licensed_differential_credit() == 0,
        "CER-1701 synthetic fixture attempted to claim licensed differential credit",
    )?;
    println!(
        "licensed-harness slot={} receipt={} plumbing=pass licensed-credit=0 differential=pending",
        validation.slot_id(),
        validation.receipt_digest()
    );
    Ok(())
}

#[cfg(test)]
mod licensed_environment_schema_tests {
    use super::*;

    #[test]
    fn cer_1701_environment_requirements_are_schema_valid_and_pending() {
        let root = repository_root().expect("repository root");
        check_licensed_environment_requirements(&root).expect("CER-1701 environment authority");
    }

    #[test]
    fn cer_1701_synthetic_harness_is_zero_credit() {
        let root = repository_root().expect("repository root");
        check_licensed_harness(&root).expect("CER-1701 synthetic harness");
    }
}

fn check_cics_source_map_schemas(root: &Path) -> TaskResult {
    let schema = json(&root.join(
        "conformance/subsystems/cics/application/schemas/cics-command-source-map.schema.json",
    ))?;
    for relative in [
        "conformance/subsystems/cics/application/cics/command-summary-topics.json",
        "conformance/subsystems/cics/application/cics/application-api-sources-a-map.json",
        "conformance/subsystems/cics/application/cics/application-api-sources-b-map.json",
        "conformance/subsystems/cics/application/cics/application-api-sources-c-map.json",
        "conformance/subsystems/cics/system/cics/spi-command-topics.json",
        "conformance/subsystems/cics/system/cics/spi-command-source-map.json",
        "conformance/subsystems/cics/system/cics/fepi-command-topics.json",
        "conformance/subsystems/cics/system/cics/fepi-command-source-map.json",
        "conformance/subsystems/cics/system/cics/command-form-locators.json",
    ] {
        let artifact = root.join(relative);
        validate_schema_instance(&schema, &json(&artifact)?, &artifact)?;
    }
    Ok(())
}

fn compile_draft_2020_12_schema(schema: &Value, path: &Path) -> TaskResult<jsonschema::Validator> {
    coverage_projection::compile_schema(schema, path)
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
    let schema_path =
        root.join("conformance/subsystems/dataset/schemas/dataset-oracle-receipt.schema.json");
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
    let schema_path = root
        .join("conformance/subsystems/jes/schemas/jes-licensed-differential-receipt.schema.json");
    let receipt = json(&receipt_path)?;
    validate_schema_instance(&json(&schema_path)?, &receipt, &receipt_path)?;
    let adapter = validate_jes_oracle_adapter(root)?;
    let candidate = jes_oracle_candidate_digest(root)?;
    require(
        receipt["candidate_digest"].as_str() == Some(candidate.as_str()),
        "licensed JES2 oracle receipt was produced from a different candidate",
    )?;
    let adapter_path =
        root.join("conformance/subsystems/jes/oracles/jes-licensed-differential.json");
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
    let adapter_path =
        root.join("conformance/subsystems/jes/oracles/jes-licensed-differential.json");
    let schema_path = root
        .join("conformance/subsystems/jes/schemas/jes-licensed-differential-adapter.schema.json");
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
    let schemas = root.join("conformance/subsystems/coverage/schemas");
    let mut artifacts = Vec::new();
    collect_extension(
        &root.join("conformance/subsystems/coverage"),
        OsStr::new("json"),
        &mut artifacts,
    )?;
    artifacts.retain(|path| !path.starts_with(&schemas));
    artifacts.sort();
    for path in &artifacts {
        let instance = json(path)?;
        let version = text(&instance, "schema_version", path)?;
        let schema_name = schema_for_0_2_artifact(version).ok_or_else(|| {
            format!(
                "{} has no Draft 2020-12 schema binding for {version}",
                path.display()
            )
        })?;
        let schema_path = schemas.join(schema_name);
        validate_schema_instance(&json(&schema_path)?, &instance, path)?;
    }

    coverage_projection::validate_artifacts(root, &artifacts)
}

fn schema_for_0_2_artifact(version: &str) -> Option<&'static str> {
    match version {
        "mainframe-env.official-source-receipts@1" => Some("official-source-receipts.schema.json"),
        "mainframe-env.official-catalog@1" => Some("official-catalog.schema.json"),
        "mainframe-env.topic-manifest@1" => Some("topic-manifest.schema.json"),
        "mainframe-env.coverage-ledger@1" => Some("coverage-ledger.schema.json"),
        "mainframe-env.coverage-row@1" => Some("coverage-row.schema.json"),
        "mainframe-env.coverage-evidence@1" => Some("coverage-evidence.schema.json"),
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
        | "mainframe-env.coverage-package-additions@1" => Some("coverage-inventory.schema.json"),
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
    let inventory = root.join("conformance/subsystems/platform/inventory");
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
        "contracts.json",
    ];
    let contract_path = inventory.join("contracts.json");
    validate_schema_instance(
        &json(
            &root.join("conformance/subsystems/platform/schemas/contract-inventory.schema.json"),
        )?,
        &json(&contract_path)?,
        &contract_path,
    )?;
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
        "conformance/subsystems/coverage/catalogs/dataset-vsam-ams.json",
        "conformance/subsystems/dataset/fixtures/dataset-organizations.json",
        "conformance/subsystems/dataset/fixtures/ams-commands.json",
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
        "docs/prompts/subsystems/dataset/IMPLEMENT_DATA.md",
        "docs/delivery/subsystems/dataset/data-plan.md",
        "docs/delivery/subsystems/dataset/data-status.md",
    ] {
        let document = read(&root.join(relative))?;
        require(
            document.contains("pass-with-licensed-differential-pending")
                && document.contains("0/36"),
            &format!("{relative} omits the approved 0.6 pending-differential disposition"),
        )?;
    }
    for relative in [
        "docs/prompts/subsystems/certification/IMPLEMENT_LICENSED.md",
        "docs/delivery/subsystems/certification/licensed-plan.md",
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

fn render_ams_grammar(root: &Path) -> TaskResult<Vec<u8>> {
    let grammar_path = root.join("conformance/subsystems/dataset/ams/grammar.json");
    let schema_path = root.join("conformance/subsystems/dataset/schemas/ams-grammar.schema.json");
    let grammar = json(&grammar_path)?;
    validate_schema_instance(&json(&schema_path)?, &grammar, &grammar_path)?;
    let commands = array(&grammar, "commands", &grammar_path)?;
    unique_rows(commands, "id", &grammar_path)?;
    require(
        commands.len() == 31,
        "AMS grammar must contain exactly 31 commands",
    )?;

    let inventory_path =
        root.join("conformance/subsystems/dataset/inventory/dataset-programming-surface.json");
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
    let inventory_path =
        root.join("conformance/subsystems/dataset/inventory/dataset-programming-surface.json");
    let schema_path =
        root.join("conformance/subsystems/dataset/schemas/dataset-programming-surface.schema.json");
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

    let official_path = root.join("conformance/subsystems/coverage/catalogs/dataset-vsam-ams.json");
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
    let manifest_path =
        root.join("conformance/subsystems/coverage/generated/semantic-identities.json");
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
    let manifest_path =
        root.join("conformance/subsystems/coverage/generated/semantic-identities.json");
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
    let handlers_path =
        root.join("conformance/subsystems/coverage/generated/subsystem-handlers.json");
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
    let index_path = root.join("conformance/subsystems/coverage/catalogs/index.json");
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
        "target_subsystem":"coverage.foundation",
        "contract":"mainframe-env.generated-semantic-identity@1",
        "catalog_index":"conformance/subsystems/coverage/catalogs/index.json",
        "catalog_index_sha256":format!("sha256:{catalog_digest}"),
        "identity_set_sha256":format!("sha256:{set_digest}"),
        "official_identity_count":rows.len(),
        "handler_registration_count":0,
        "coverage_credit":0
    });
    Ok((source.into_bytes(), pretty_json(&manifest)?))
}

fn check_application_packages(root: &Path) -> TaskResult {
    let contracts_path = root.join("conformance/subsystems/coverage/inventory/contracts.json");
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
    let migration_path =
        root.join("conformance/subsystems/coverage/migrations/application-package-v1-to-v2.json");
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
        "conformance/subsystems/coverage/schemas/application-package-v2.schema.json",
        "conformance/subsystems/coverage/schemas/application-install-generation.schema.json",
        "conformance/subsystems/coverage/schemas/application-installer-state.schema.json",
        "conformance/subsystems/coverage/schemas/application-publication-state.schema.json",
        "docs/architecture/APPLICATION-PACKAGES.md",
        "crates/kernel/mainframe-env-application/README.md",
    ] {
        require(
            root.join(path).is_file(),
            &format!("application package contract artifact is missing: {path}"),
        )?;
    }
    let package_schema = json(
        &root.join("conformance/subsystems/coverage/schemas/application-package-v2.schema.json"),
    )?;
    require(
        package_schema["required"]
            == serde_json::json!(["base", "generation", "sections", "signature"])
            && package_schema["properties"].get("schema_version").is_none()
            && package_schema["properties"]
                .get("base_manifest_identity")
                .is_none()
            && package_schema["properties"]["base"]["$ref"] == "#/$defs/base"
            && package_schema["properties"]["sections"]["properties"]["schema_version"]["enum"]
                == serde_json::json!([
                    "mainframe-env.application-package@2",
                    "mainframe-env.application-package@3"
                ]),
        "application package schema must describe the owned DTO with finite @2/@3 sections",
    )?;
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
        "APPLICATION_PACKAGE_V3_CONTRACT",
        "pub fn package_v2_identity",
        "pub fn package_generation_identity",
        "pub fn package_generation_identity_with_limits",
    ] {
        require(
            implementation.contains(required),
            &format!("application package implementation omits {required}"),
        )?;
    }
    for path in [
        "crates/tooling/mainframe-env-conformance/src/carddemo.rs",
        "crates/tooling/mainframe-env-conformance/src/carddemo/ims_packages.rs",
        "crates/tooling/mainframe-env-conformance/src/carddemo/ims_process_tests.rs",
    ] {
        let producer = read(&root.join(path))?;
        require(
            producer.contains("package_generation_identity")
                && !producer.contains("package_v2_identity"),
            &format!("current application package producer lacks finite identity dispatch: {path}"),
        )?;
    }
    let production_product = production_scanner::read_source(
        root,
        &root.join("crates/apps/mainframe-env-server/src/product.rs"),
    )?;
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
    let production = production_scanner::read_source(root, &service_path)?;
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
    let contracts_path = root.join("conformance/subsystems/coverage/inventory/contracts.json");
    let contracts = json(&contracts_path)?;
    require(
        contracts["contracts"]["db2_application_catalog"]
            == Value::String("mainframe-env.db2-application-catalog@1".into()),
        "Db2 application catalog contract is not frozen",
    )?;
    let migration_path =
        root.join("conformance/subsystems/coverage/migrations/db2-catalog-v1-to-v2.json");
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
    for path in [
        "conformance/subsystems/coverage/schemas/db2-application-catalog.schema.json",
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
    let production_service = production_scanner::read_source(root, &service_path)?;
    let production_controller = production_scanner::read_source(root, &controller_path)?;
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
    let production_product = production_scanner::read_source(
        root,
        &root.join("crates/apps/mainframe-env-server/src/product.rs"),
    )?;
    require(
        production_product.contains("pub fn publish_application_generation")
            && production_product.contains("selected_application_v2")
            && production_product.contains("apply_application_batch_controllers")
            && production_product.contains("BatchControllerProgram")
            && production_product.contains("decode_application_batch_controller")
            && !production_product.contains("selected_identity: &str"),
        "composition does not derive controllers from a verified selected package handle",
    )?;
    let contracts_path = root.join("conformance/subsystems/coverage/inventory/contracts.json");
    let contracts = json(&contracts_path)?;
    require(
        contracts["contracts"]["batch_controller_registry"]
            == Value::String("mainframe-env.batch-controller-registry@1".into()),
        "batch controller registry contract is not frozen",
    )?;
    let migration_path =
        root.join("conformance/subsystems/coverage/migrations/batch-controller-v0-to-v1.json");
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
    for path in [
        "conformance/subsystems/coverage/schemas/batch-controller-registry.schema.json",
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
    let path = root.join("conformance/subsystems/coverage/abi/libraries.json");
    fs::create_dir_all(path.parent().ok_or("host ABI inventory has no parent")?)
        .map_err(|error| error.to_string())?;
    fs::write(&path, pretty_json(&value)?).map_err(|error| format!("{}: {error}", path.display()))
}

fn check_host_abi_libraries(root: &Path) -> TaskResult {
    let receipt = verify_host_abi_libraries()?;
    let expected = serde_json::to_value(receipt).map_err(|error| error.to_string())?;
    let inventory_path = root.join("conformance/subsystems/coverage/abi/libraries.json");
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
        "conformance/subsystems/coverage/schemas/host-abi-source-library.schema.json",
        "docs/architecture/HOST-ABI-SOURCE-LIBRARIES.md",
    ] {
        require(
            root.join(path).is_file(),
            &format!("host ABI artifact is missing: {path}"),
        )?;
    }
    let contracts_path = root.join("conformance/subsystems/coverage/inventory/contracts.json");
    let contracts = json(&contracts_path)?;
    require(
        contracts["contracts"]["host_abi_source_library"]
            == Value::String("mainframe-env.host-abi-source-library@1".into()),
        "host ABI source-library contract is not frozen",
    )?;
    let migration_path =
        root.join("conformance/subsystems/coverage/migrations/host-abi-v0-to-v1.json");
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
    Ok(())
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
    let sources = production_scanner::read_sources(
        root,
        &[
            root.join("crates/apps/mainframe-env-batch/src/program.rs"),
            root.join("crates/apps/mainframe-env-batch/src/service.rs"),
        ],
    )?;
    let implementation = &sources[0];
    for forbidden in [
        "match program.to_ascii_uppercase().as_str()",
        "for name in [",
        "match self.0 {",
        "match program {",
        "program == \"",
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
    let service = &sources[1];
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
    let contracts = json(&root.join("conformance/subsystems/coverage/inventory/contracts.json"))?;
    require(
        contracts["contracts"]["common_program_catalog"]
            == Value::String("mainframe-env.common-program-catalog@1".into()),
        "common program catalog contract is not frozen",
    )?;
    for path in [
        "conformance/subsystems/coverage/programs/common-programs.json",
        "conformance/subsystems/coverage/schemas/common-program-catalog.schema.json",
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
    let catalog_path = root.join("conformance/subsystems/coverage/programs/common-programs.json");
    let catalog = json(&catalog_path)?;
    let schema_path =
        root.join("conformance/subsystems/coverage/schemas/common-program-catalog.schema.json");
    validate_schema_instance(&json(&schema_path)?, &catalog, &catalog_path)?;
    require(
        catalog["schema_version"] == Value::String("mainframe-env.common-program-catalog@1".into())
            && catalog["target_subsystem"] == Value::String("coverage.foundation".into())
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
            common_program_policy::ControlPolicy::utility(program, &catalog_path)?;
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
            common_program_policy::ControlPolicy::tso(action)?;
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
    source.push_str("impl TsoProgramExecution {\n");
    source.push_str("    pub(super) fn control_declaration(self) -> super::ControlDeclaration {\n");
    source.push_str("        match self {\n");
    for program in programs {
        if let Some(action) = program["tso_action"].as_str() {
            let variant = rust_variant(action)?;
            if tso_variants.contains(&variant) {
                source.push_str(&format!("            Self::{variant} => "));
                common_program_policy::ControlPolicy::tso(action)?
                    .render(&mut source, "            ");
                source.push_str(",\n");
                tso_variants.retain(|value| *value != variant);
            }
        }
    }
    source.push_str("        }\n    }\n}\n\n");
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
        source.push_str("        control: ");
        common_program_policy::ControlPolicy::utility(program, &catalog_path)?
            .render(&mut source, "        ");
        source.push_str(",\n");
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
            root.join("conformance/subsystems/coverage/generated/route-registries.json"),
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
            root.join("conformance/subsystems/coverage/generated/route-registries.json"),
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
    let contracts = json(&root.join("conformance/subsystems/coverage/inventory/contracts.json"))?;
    require(
        contracts["contracts"]["official_route_registry"]
            == Value::String("mainframe-env.zosmf-official-route-bindings@1".into())
            && contracts["contracts"]["custom_route_registry"]
                == Value::String("mainframe-env.custom-route-catalog@1".into()),
        "route registry contracts are not frozen",
    )?;
    let migration_path =
        root.join("conformance/subsystems/coverage/migrations/registry-v0-to-v1.json");
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
        "conformance/subsystems/coverage/routes/official-route-bindings.json",
        "conformance/subsystems/coverage/routes/custom-routes.json",
        "conformance/subsystems/coverage/schemas/route-registry.schema.json",
        "conformance/subsystems/coverage/schemas/generated-route-registries.schema.json",
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
    let catalog_path = root.join("conformance/subsystems/platform/inventory/zosmf-routes.json");
    let official_path =
        root.join("conformance/subsystems/coverage/routes/official-route-bindings.json");
    let custom_path = root.join("conformance/subsystems/coverage/routes/custom-routes.json");
    let catalog = json(&catalog_path)?;
    let official = json(&official_path)?;
    let custom = json(&custom_path)?;
    require(
        official["schema_version"]
            == Value::String("mainframe-env.zosmf-official-route-bindings@1".into())
            && official["namespace"] == Value::String("official-zosmf".into())
            && official["catalog"]
                == Value::String(
                    "conformance/subsystems/platform/inventory/zosmf-routes.json".into(),
                )
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
        "target_subsystem":"coverage.foundation",
        "official":{
            "namespace":"official-zosmf",
            "catalog":"conformance/subsystems/platform/inventory/zosmf-routes.json",
            "catalog_sha256":format!("sha256:{}", file_digest(&catalog_path)?),
            "bindings_sha256":format!("sha256:{}", file_digest(&official_path)?),
            "route_count":official_rows.len()
        },
        "custom":{
            "namespace":"mainframe-env-custom",
            "catalog":"conformance/subsystems/coverage/routes/custom-routes.json",
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
    rust_files.retain(|rust_file| {
        !(rust_file.starts_with(&conformance)
            || rust_file.file_name() == Some(OsStr::new("tests.rs"))
            || rust_file
                .components()
                .any(|part| part.as_os_str() == OsStr::new("tests")))
    });
    production_scanner::check_application_hardcodes(root, &rust_files)?;
    let program = production_scanner::read_source(
        root,
        &root.join("crates/apps/mainframe-env-batch/src/program.rs"),
    )?;
    let batch = production_scanner::read_source(
        root,
        &root.join("crates/apps/mainframe-env-batch/src/service.rs"),
    )?;
    let server = production_scanner::read_source(
        root,
        &root.join("crates/apps/mainframe-env-server/src/cobol.rs"),
    )?;
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
            batch.as_str(),
            vec![
                "match program.as_str()",
                "step.program.eq_ignore_ascii_case(\"",
            ],
        ),
        (
            "installed system services",
            server.as_str(),
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
    Ok(())
}

fn check_coverage(root: &Path) -> TaskResult {
    let index_path = root.join("conformance/subsystems/coverage/catalogs/index.json");
    let index = json(&index_path)?;
    require(
        text(&index, "schema_version", &index_path)? == "mainframe-env.official-source-receipts@1",
        "official source receipt schema changed",
    )?;
    require(
        text(&index, "target_subsystem", &index_path)? == "coverage.foundation",
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
            catalog_name.starts_with("conformance/subsystems/coverage/catalogs/")
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

    conformance_catalog::check_catalog_locator_membership(root, &index, &index_path)?;
    check_publication_bytes(root)?;
    check_probe_record_figures(root)?;

    Ok(())
}

/// The probe record's numbers, against the artifacts they are drawn from.
///
/// `docs/research/publication-source-probe.md` went stale in five consecutive
/// waves of this work, every time the same way: a change moved a count, the
/// record had been written before it landed, and nobody diffs prose. Four
/// hand-corrections each fixed the waves behind them and nothing about the next
/// one. `report_probe_figures.py --check` ends that by recomputing every figure
/// the record cites -- each carries a `<!--f:key-->` marker naming the figure --
/// out of the committed catalogs, manifests, projections and ledger, and failing
/// when a cited number and its artifact disagree.
///
/// It runs here because a checker nothing invokes is a checker that measures
/// whether someone remembered. It is offline by construction: it reads only
/// committed files and asks IBM nothing, which is what makes it a gate rather
/// than a review activity. The guard is the same shape as the canonical effect
/// encoding one above -- shell out, require success -- because the figures are
/// derived by the same Python that derives the artifacts, and reimplementing
/// that derivation in Rust would give two answers to check against each other
/// rather than one answer to check the record against.
fn check_probe_record_figures(root: &Path) -> TaskResult {
    let checker = root.join("conformance/tools/report_probe_figures.py");
    if !checker.is_file() {
        return Ok(());
    }
    let status = Command::new("python3")
        .arg("-B")
        .arg(&checker)
        .arg("--check")
        .current_dir(root)
        .status()
        .map_err(|error| format!("probe record figure check: {error}"))?;
    require(
        status.success(),
        "the probe record cites a figure its artifact does not produce",
    )
}

/// The two markers IBM's content endpoint stamps into every body it serves.
///
/// They are not chosen for being distinctive strings; they are the two fields
/// this project's own reader takes out of a topic. `HEADING` and `LAST_MODIFIED`
/// in `conformance/tools/docs_api.py` match exactly these; the nine manifests
/// under `conformance/subsystems/coverage/manifests/` record the `last_modified` the second
/// yields for all 4,488 pinned topics and
/// `conformance/subsystems/jcl/generated/jcl-topic-manifest.json` records the `heading` the
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
/// when it carries a served topic, by either of two tests.
///
/// The first test is the original one: the file's first non-whitespace byte is
/// `<` and it carries both markers. That clause is what made the check safe to
/// apply to source -- `docs_api.py` and `conformance/tools/tests/
/// test_locator_tools.py` both quote both markers, because one defines them and
/// the other tests them, and neither is markup -- but it also meant position
/// decided the answer. One line of prose in front of a verbatim body, or the
/// same body as the value of a JSON field, and the file no longer opens with
/// `<`. The docstring here used to claim that defeating the check needed editing
/// IBM's bytes. It needed a text editor and no understanding at all, and a guard
/// that overstates itself is worse than a narrow one because it stops people
/// looking.
///
/// The second test closes both of those and does not care where in the file the
/// body sits. It looks for the seam IBM's own renderer emits, the topic heading
/// closing directly onto the `Last Updated` stamp: `</h1>` followed by the
/// `<div>` that carries `id="lastModifiedDate"`. Every one of the 6,775 topic
/// bodies cached across the baselines carries that seam, and every one of them
/// carries it with no whitespace at all between the two elements; whitespace is
/// tolerated here anyway, against a reflow. Source that quotes the markers does
/// not carry it: 6,782 cached files hold both markers and the seven that are not
/// bodies are the wave-1 fetch scripts, where `docs_api.py`'s two patterns sit
/// 55 bytes apart with a `re.compile(` between them. This file's own tests
/// assemble their sample body from two halves for the same reason, so that no
/// file in this repository holds a served body verbatim -- there is no exception
/// for the tests that exist to look for served bodies.
///
/// `\"` is unescaped before the seam is looked for, which is what catches the
/// JSON envelope: a body embedded in a JSON string has every quote escaped, so
/// `id=\"lastModifiedDate\"` would otherwise not match the marker at all.
///
/// What still gets past, stated plainly rather than left to be discovered:
/// re-serialising the body so the heading and the stamp are no longer adjacent
/// -- pretty-printing the markup, or JSON-encoding it with a literal `\n`
/// between the two elements -- and any transformation that drops the stamp.
/// That is a real gap and this is a backstop, not the control. The control is
/// `conformance/tools/docs_api.py`: retrieved bytes are written by one function,
/// which refuses any path inside the tree, and `python3
/// conformance/tools/docs_api.py --audit` names every write every
/// retrieval-capable tool makes and why it is allowed.
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
    let served = "a topic body served by IBM Documentation";
    if carries_the_served_seam(body) {
        return Some(served);
    }
    if body.iter().find(|byte| !byte.is_ascii_whitespace()) != Some(&b'<') {
        return None;
    }
    let text = String::from_utf8_lossy(body);
    SERVED_TOPIC_MARKERS
        .iter()
        .all(|marker| text.contains(marker))
        .then_some(served)
}

/// Whether the topic heading closes directly onto the `Last Updated` stamp.
///
/// Anywhere in the file, so prose in front of the body or a JSON envelope around
/// it changes nothing. The cheap gate first: nearly every file in the tree does
/// not contain the string at all, and only the handful that do pay for the
/// unescaping pass.
fn carries_the_served_seam(body: &[u8]) -> bool {
    if !body.windows(16).any(|window| window == b"lastModifiedDate") {
        return false;
    }
    let plain = String::from_utf8_lossy(body).replace("\\\"", "\"");
    let mut rest = plain.as_str();
    while let Some(at) = rest.find("id=\"lastModifiedDate\"") {
        let (before, after) = rest.split_at(at);
        if let Some(open) = before.rfind('<') {
            let opens_a_div = before[open..].starts_with("<div");
            let heading = before[..open].trim_end_matches(|c: char| c.is_ascii_whitespace());
            if opens_a_div && heading.ends_with("</h1>") {
                return true;
            }
        }
        rest = &after[1..];
    }
    false
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

fn check_runtime_architecture(root: &Path) -> TaskResult {
    check_architecture(root)?;
    let target = host_target(root)?;
    build_runtime_target(root, &target)?;
    smoke_runtime_target(root, &target)
}

fn spawn_runtime_server(
    root: &Path,
    binary: &Path,
    directory: &Path,
    database: &Path,
    bootstrap_secret: bool,
) -> TaskResult<(Child, u16)> {
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|error| error.to_string())?;
    let port = listener
        .local_addr()
        .map_err(|error| error.to_string())?
        .port();
    drop(listener);
    let mut command = Command::new(binary);
    command
        .arg(root.join("config/mainframe-env.toml"))
        .current_dir(directory)
        .env("MAINFRAME_ENV_STORE", "sqlite")
        .env("MAINFRAME_ENV_ARTIFACT_STORE", "local")
        .env("MAINFRAME_ENV_TLS", "false")
        .env("MAINFRAME_ENV_LISTEN", format!("127.0.0.1:{port}"))
        .env(
            "MAINFRAME_ENV_SQLITE_URL",
            format!("sqlite://{}?mode=rwc", database.display()),
        )
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    if bootstrap_secret {
        command.env("MAINFRAME_ENV_SECRET_BOOTSTRAP_ADMIN", "VEVTVFBBU1M=");
    } else {
        command.env_remove("MAINFRAME_ENV_SECRET_BOOTSTRAP_ADMIN");
    }
    command
        .spawn()
        .map(|child| (child, port))
        .map_err(|error| format!("start runtime server: {error}"))
}

fn wait_for_runtime_readiness(child: &mut Child, port: u16) -> TaskResult {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if let Some(status) = child.try_wait().map_err(|error| error.to_string())? {
            let mut stderr = String::new();
            if let Some(mut pipe) = child.stderr.take() {
                let _ = pipe.read_to_string(&mut stderr);
            }
            return Err(format!("runtime server exited {status}: {stderr}"));
        }
        if let Ok(response) = runtime_http_request(
            port,
            b"GET /zosmf/info HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
        ) && response
            .lines()
            .next()
            .is_some_and(|line| line.contains(" 200 "))
            && let Some((_, body)) = response.split_once("\r\n\r\n")
            && let Ok(info) = serde_json::from_str::<Value>(body)
        {
            let component_ready = [
                "accepting",
                "writable_store",
                "bootstrap_identity",
                "host_capabilities",
                "artifact_store",
                "jes_workers",
            ]
            .into_iter()
            .all(|component| info["readiness"][component].as_bool() == Some(true));
            if info["live"].as_bool() == Some(true)
                && info["ready"].as_bool() == Some(true)
                && info["listen"].as_str() == Some(format!("127.0.0.1:{port}").as_str())
                && info["zosmf_port"].as_str() == Some(port.to_string().as_str())
                && component_ready
            {
                return Ok(());
            }
        }
        thread::sleep(Duration::from_millis(50));
    }
    Err("runtime SQLite server did not become fully ready".into())
}

fn authenticate_runtime_administrator(port: u16) -> TaskResult {
    let response = runtime_http_request(
        port,
        b"POST /zosmf/services/authenticate HTTP/1.1\r\nHost: localhost\r\nAuthorization: Basic QURNSU46VEVTVFBBU1M=\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
    )?;
    require(
        response
            .lines()
            .next()
            .is_some_and(|line| line.contains(" 200 ")),
        "restarted runtime server rejected the persisted administrator",
    )?;
    let (_, body) = response
        .split_once("\r\n\r\n")
        .ok_or("runtime authentication response has no body")?;
    let authentication: Value = serde_json::from_str(body).map_err(|error| error.to_string())?;
    require(
        authentication["user"] == Value::String("ADMIN".into())
            && authentication["token"]
                .as_str()
                .is_some_and(|token| token.starts_with("session-")),
        "restarted runtime server returned an invalid authentication receipt",
    )
}

fn runtime_http_request(port: u16, request: &[u8]) -> TaskResult<String> {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).map_err(|error| error.to_string())?;
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .map_err(|error| error.to_string())?;
    stream
        .set_write_timeout(Some(Duration::from_secs(2)))
        .map_err(|error| error.to_string())?;
    stream
        .write_all(request)
        .map_err(|error| error.to_string())?;
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .map_err(|error| error.to_string())?;
    Ok(response)
}

fn stop_runtime_server(child: &mut Child) {
    if child.try_wait().ok().flatten().is_none() {
        let _ = child.kill();
        let _ = child.wait();
    }
}

fn check_certification(root: &Path) -> TaskResult {
    check_profiles(root)?;
    check_schemas(root)?;
    check_inventory(root)?;

    check_coverage(root)?;
    check_semantic_identities(root)?;
    check_application_packages(root)?;
    check_db2_catalog(root)?;

    check_runtime_architecture(root)?;

    let inventory = root.join("conformance/subsystems/platform/inventory");
    let packages_path = inventory.join("packages.json");
    let packages = json(&packages_path)?;
    let mut package_names = array(&packages, "packages", &packages_path)?
        .iter()
        .map(|row| text(row, "name", &packages_path).map(str::to_string))
        .collect::<TaskResult<BTreeSet<_>>>()?;
    for additions_path in [
        root.join("conformance/subsystems/coverage/inventory/package-additions.json"),
        root.join("conformance/subsystems/jes/inventory/package-additions.json"),
    ] {
        let additions = json(&additions_path)?;
        for package in array(&additions, "packages", &additions_path)? {
            require(
                package_names.insert(text(package, "name", &additions_path)?.to_string()),
                "package addition duplicates an earlier inventory package",
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
    Ok(())
}

fn print_digest(root: &Path) -> TaskResult {
    println!("{}", repository_digest(root)?);
    Ok(())
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

fn repository_digest_excluded(relative: &Path) -> bool {
    relative.starts_with(".git")
        || relative.starts_with("target")
        || relative
            .components()
            .any(|part| part.as_os_str() == "__pycache__")
}

fn validate_runtime_target(target: &str) -> TaskResult {
    require(
        !target.is_empty()
            && target.len() <= 128
            && target
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
            && RETAINED_RUNTIME_TARGETS.contains(&target),
        "runtime target is invalid",
    )
}

fn host_target(root: &Path) -> TaskResult<String> {
    let verbose = command_text(root, "rustc", &["-vV"])?;
    let target = verbose
        .lines()
        .find_map(|line| line.strip_prefix("host: "))
        .ok_or("rustc host target is missing")?;
    validate_runtime_target(target)?;
    Ok(target.into())
}

fn build_runtime_target(root: &Path, target: &str) -> TaskResult {
    let status = Command::new("cargo")
        .args([
            "build",
            "--locked",
            "--release",
            "--target",
            target,
            "-p",
            "mainframe-env-server",
            "-p",
            "mainframe-env-cli",
            "--bins",
        ])
        .current_dir(root)
        .status()
        .map_err(|error| error.to_string())?;
    require(status.success(), "runtime smoke binary build failed")
}

fn runtime_binary(root: &Path, target: &str, name: &str) -> PathBuf {
    cargo_target_directory(root).join(format!("{target}/release/{name}"))
}

fn probe_runtime_binary(binary: &Path, argument: &str, expected: &str) -> TaskResult {
    let output = Command::new(binary)
        .arg(argument)
        .output()
        .map_err(|error| format!("run {} {argument}: {error}", binary.display()))?;
    require(
        output.status.success(),
        &format!(
            "{} {argument} exited {}: {}",
            binary.display(),
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ),
    )?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    require(
        stdout.contains(expected),
        &format!("{} {argument} output is invalid", binary.display()),
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

fn pretty_json(value: &Value) -> TaskResult<Vec<u8>> {
    let mut bytes = serde_json::to_vec_pretty(value).map_err(|error| error.to_string())?;
    bytes.push(b'\n');
    Ok(bytes)
}

#[cfg(test)]
fn canonical_evidence_digest(receipt: &serde_json::Map<String, Value>) -> TaskResult<String> {
    let canonical = serde_json::to_vec(receipt).map_err(|error| error.to_string())?;
    Ok(format!("sha256:{:x}", Sha256::digest(canonical)))
}

fn file_digest(path: &Path) -> TaskResult<String> {
    let bytes = fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
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
    let path = root.join("conformance/subsystems/platform/inventory/excluded-components.json");
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
            // A nested Git worktree has a `.git` file rather than a `.git`
            // directory. It is a separate checkout, not part of this candidate.
            if linked_worktree_root(&path) {
                continue;
            }
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
            if linked_worktree_root(&path) {
                continue;
            }
            collect_extension(&path, extension, files)?;
        } else if path.extension() == Some(extension) {
            files.push(path);
        }
    }
    Ok(())
}

/// Every file in the tree, minus Cargo's root build directory, git's root
/// object store, and any linked worktree nested below this checkout. A linked
/// worktree is a separate candidate even when a tool stores it below this path.
///
/// The skip used to read `file_name() == "target" || file_name() == ".git"`, and
/// it was evaluated at every level of the recursion, so it said "the build
/// directory" and meant "anything anyone named `target`". That is enough to walk
/// straight past the guard 8389f98 widened to the whole tree:
/// `cp <an IBM topic body> docs/target/leak.md && cargo xtask coverage --check`
/// printed `coverage: pass`, and so did the same body at
/// `conformance/subsystems/dataset/target/`, at `crates/<crate>/target/deep/` and at
/// `docs/.git/`. A leak into a nested `target` is worse than one anywhere else,
/// not better: `.gitignore` reads `/target/`, anchored at the root, so
/// `docs/target/leak.md` is untracked, `git add -A` stages it, and the one check
/// that exists to stop it was looking the other way. The `docs/.git/` twin is
/// the same evasion with a different ending -- git refuses to track any path
/// under a `.git` component, so those bytes cannot be committed, but they still
/// sit in the tree with nothing reporting them.
///
/// Anchoring both names to `root` costs nothing measurable, because the
/// directory that has to stay skipped is the build directory at the workspace
/// root -- 338,191 files and 49 GB here -- and that one is still matched, by
/// path now rather than by name. Three runs of `cargo xtask coverage --check`
/// each way, with that directory present at the root for both, spend 2.5-3.0s
/// user and 2.0-3.0s system before and 2.6s user and 2.1-2.3s system after; the
/// wall clock is 7.7-19.7s against 7.2-7.9s and is the machine's load rather
/// than the walk. Walking that directory instead would not be a rounding error:
/// enumerating it alone is 7.1s warm, and `check_publication_bytes` reads every
/// file it is handed, which a 2,023-file random sample puts at about ten
/// minutes and 49 GB through `fs::read`.
fn collect_files(root: &Path, files: &mut Vec<PathBuf>) -> TaskResult {
    let not_the_tree = [root.join("target"), root.join(".git")];
    collect_files_below(root, &not_the_tree, files)
}

/// `__pycache__` is skipped by name at any depth, and it is the one exception to
/// matching by path. It is gitignored, so nothing inside it can be checked into
/// the repository, which is the only thing this walk exists to prevent -- and a
/// `.pyc` marshals its source's string constants adjacent to one another, so a
/// test that deliberately holds `</h1>` and `lastModifiedDate` apart compiles to
/// bytecode that reads as a served topic body. Running the Python suites once
/// turned `cargo xtask coverage --check` red on a file git will never see.
fn ignored_build_output(path: &Path) -> bool {
    path.file_name().is_some_and(|name| name == "__pycache__")
}

fn linked_worktree_root(path: &Path) -> bool {
    path.join(".git").is_file()
}

fn collect_files_below(
    directory: &Path,
    not_the_tree: &[PathBuf; 2],
    files: &mut Vec<PathBuf>,
) -> TaskResult {
    for entry in
        fs::read_dir(directory).map_err(|error| format!("{}: {error}", directory.display()))?
    {
        let path = entry.map_err(|error| error.to_string())?.path();
        if path.is_dir() {
            if not_the_tree.contains(&path)
                || ignored_build_output(&path)
                || linked_worktree_root(&path)
            {
                continue;
            }
            collect_files_below(&path, not_the_tree, files)?;
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
    fn current_cics_application_contract_satisfies_its_schema() {
        let root = repository_root().expect("repository root");
        for (catalog, schema) in [
            (
                "conformance/subsystems/cics/application/generated/cics-application-command-contracts.json",
                "conformance/subsystems/cics/application/schemas/cics-application-command-contracts.schema.json",
            ),
            (
                "conformance/subsystems/cics/application/cics/command-descriptors.json",
                "conformance/subsystems/cics/application/schemas/cics-command-descriptors.schema.json",
            ),
            (
                "conformance/subsystems/cics/application/cics/typed-execution-registrations.json",
                "conformance/subsystems/cics/application/schemas/cics-typed-execution-registrations.schema.json",
            ),
        ] {
            let catalog = root.join(catalog);
            let schema = root.join(schema);
            validate_schema_instance(&json(&schema).unwrap(), &json(&catalog).unwrap(), &catalog)
                .unwrap();
        }
    }

    #[test]
    fn dataset_reference_simulation_cannot_satisfy_the_licensed_receipt_gate() {
        let root = repository_root().expect("repository root");
        let pending = check_dataset_oracle_receipt(&root, None)
            .expect_err("missing licensed receipt must remain pending");
        assert!(pending.contains("licensed z/OS 3.2 differential is pending"));
        let simulation =
            root.join("crates/tooling/mainframe-env-conformance/src/dataset_reference.rs");
        assert!(check_dataset_oracle_receipt(&root, Some(simulation)).is_err());
        let local_certification =
            root.join("conformance/subsystems/dataset/evidence/dataset-certification.json");
        assert!(check_dataset_oracle_receipt(&root, Some(local_certification)).is_err());
    }

    #[test]
    fn blocked_cics_source_review_fails_the_ordinary_check() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = env::temp_dir().join(format!(
            "mainframe-env-cics-source-review-{}-{nonce}",
            std::process::id()
        ));
        let review = root.join(
            "conformance/subsystems/cics/application/cics/application-api-sources-a-review.json",
        );
        let schema = root
            .join("conformance/subsystems/cics/application/schemas/cics-source-review.schema.json");
        let checker = root.join(
            "conformance/subsystems/cics/application/tools/review_cics_application_sources.py",
        );
        for path in [&review, &schema, &checker] {
            fs::create_dir_all(path.parent().expect("parent")).expect("create parent");
        }
        fs::write(&review, br#"{"review_status":"blocked"}"#).expect("blocked review");
        fs::write(
            &schema,
            br#"{
                "$schema":"https://json-schema.org/draft/2020-12/schema",
                "type":"object",
                "additionalProperties":false,
                "required":["review_status"],
                "properties":{"review_status":{"const":"blocked"}}
            }"#,
        )
        .expect("review schema");
        fs::write(
            &checker,
            b"import sys\nraise SystemExit(1 if sys.argv[1:] == ['--check'] else 2)\n",
        )
        .expect("review checker");

        let error = check_cics_source_review_batch(&root, "a")
            .expect_err("blocked automatic review must fail the ordinary check");
        assert!(error.contains("freshness guard failed"));
        fs::remove_dir_all(root).expect("clean up");
    }

    /// The two halves of a served topic, joined only at run time.
    ///
    /// `check_publication_bytes` walks this file too, and the seam test would
    /// refuse it if the sample below were written out whole. Assembling it is
    /// not a workaround for the guard, it is the guard's rule applied to the
    /// guard: this repository holds no verbatim publication bytes, and there is
    /// no exception for the test that looks for them.
    const HEADING_HALF: &str = "<div><article role=\"article\" aria-labelledby=\"t__1\">\n\
                                <h1 class=\"topictitle1\" id=\"t__1\">DD statement</h1>";
    const STAMP_HALF: &str = "<div id=\"lastModifiedDate\"><span>Last Updated</span>: 2026-01-28\
                              </div>\n<div class=\"body\"><p class=\"p\">The DD statement \
                              describes a data set.</p></div></article></div>\n";

    fn served_topic() -> String {
        format!("{HEADING_HALF}{STAMP_HALF}")
    }

    #[test]
    fn a_publication_body_is_recognised_by_its_bytes_and_not_by_its_name() {
        let served = served_topic();
        assert_eq!(
            publication_body(served.as_bytes()),
            Some("a topic body served by IBM Documentation")
        );
        // Renaming is what the extension rule could not survive, so the bytes
        // decide: the same body called anything at all is the same body.
        assert!(publication_body(format!("\u{feff}  \n{served}").as_bytes()).is_some());
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

    /// The two evasions the leading-`<` clause allowed, and one it never did.
    ///
    /// Both of these printed nothing at HEAD: `publication_body` returned `None`
    /// for a body with a sentence in front of it and for the same body as a JSON
    /// field, because neither file opens with `<`. The guard's own docstring
    /// said defeating it required editing IBM's bytes.
    #[test]
    fn a_served_body_is_caught_wherever_in_the_file_it_sits() {
        let served = served_topic();

        // One line of prose, and at HEAD the whole file was exempt.
        let with_a_note = format!("Kept for reference while the reader is written.\n\n{served}");
        assert_eq!(
            publication_body(with_a_note.as_bytes()),
            Some("a topic body served by IBM Documentation")
        );

        // A JSON envelope, which also escapes every quote in the body -- so the
        // markers themselves stop matching until `\"` is undone.
        let envelope = format!(
            "{{\n  \"topic_path\": \"SSLTBW_3.2.0/.../iea3b6_dd.htm\",\n  \"body\": {}\n}}\n",
            serde_json::to_string(&served).expect("encode")
        );
        assert!(!envelope.contains("id=\"lastModifiedDate\""));
        assert_eq!(
            publication_body(envelope.as_bytes()),
            Some("a topic body served by IBM Documentation")
        );

        // What still gets past, asserted so the docstring cannot drift from it:
        // prose in front to lose the first test, and the heading separated from
        // the stamp to lose the second. Reflowing alone is not enough -- the
        // file still opens with `<` and still carries both markers.
        let reflowed = format!("{HEADING_HALF}\n<p>reformatted</p>\n{STAMP_HALF}");
        assert_eq!(
            publication_body(reflowed.as_bytes()),
            Some("a topic body served by IBM Documentation")
        );
        assert_eq!(
            publication_body(format!("note\n{reflowed}").as_bytes()),
            None
        );

        // Prose in front of a `<h1>` that is not a served heading stays legal,
        // which is the false positive the seam has to avoid.
        assert_eq!(
            publication_body(b"notes\n<h1 class=\"topictitle1\">ours</h1>\n<p>no stamp</p>\n"),
            None
        );
    }

    /// The guard reads bytes; this is about which bytes it is ever given.
    ///
    /// `collect_files` skipped by directory name at every depth, so the whole
    /// content check above was one `mkdir` from being unreachable. Each of these
    /// five paths was verified against the real tree before the skip was
    /// anchored: `coverage: pass` for all five at HEAD, and the same five
    /// refused afterwards.
    #[test]
    fn only_the_build_directory_at_the_root_is_outside_the_tree() {
        let root = std::env::temp_dir().join(format!(
            "xtask-collect-files-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&root);
        let body = b"<div><h1 class=\"topictitle1\">ALTER</h1>\
                     <div id=\"lastModifiedDate\">Last Updated: 2026-09-03</div></div>";
        let plant = |relative: &str| {
            let path = root.join(relative);
            fs::create_dir_all(path.parent().expect("parent")).expect("create");
            fs::write(&path, body).expect("write");
            path
        };

        // Cargo's build directory and git's object store, both at the root.
        plant("target/leak.md");
        plant(".git/leak.md");
        check_publication_bytes(&root).expect("the root build directory is not the tree");

        // Everything else, at every depth, is.
        for relative in [
            "docs/target/leak.md",
            "docs/.git/leak.html",
            "conformance/subsystems/dataset/target/leak.html",
            "crates/mainframe-env-cobol/target/deep/leak.htm",
            "conformance/subsystems/jcl/tools/target/a/b/c/leak.json",
        ] {
            let path = plant(relative);
            let error =
                check_publication_bytes(&root).expect_err("a planted topic body must be refused");
            assert!(error.contains(relative), "{error} does not name {relative}");
            fs::remove_file(&path).expect("remove");
        }

        check_publication_bytes(&root).expect("nothing is left planted");
        fs::remove_dir_all(&root).expect("clean up");
    }

    #[test]
    fn repository_inventories_do_not_enter_nested_git_worktrees() {
        let root = std::env::temp_dir().join(format!(
            "xtask-collect-named-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("member/src")).expect("member directory");
        fs::create_dir_all(root.join(".claude/worktrees/probe/src"))
            .expect("nested worktree directory");
        fs::write(root.join(".git"), b"gitdir: candidate\n").expect("candidate worktree marker");
        fs::write(root.join("Cargo.toml"), b"[workspace]\n").expect("root manifest");
        fs::write(
            root.join("member/Cargo.toml"),
            b"[package]\nname = \"member\"\nversion = \"0.1.0\"\n",
        )
        .expect("member manifest");
        fs::write(
            root.join(".claude/worktrees/probe/.git"),
            b"gitdir: elsewhere\n",
        )
        .expect("worktree marker");
        fs::write(
            root.join(".claude/worktrees/probe/Cargo.toml"),
            b"[workspace]\n",
        )
        .expect("nested root manifest");
        fs::write(
            root.join(".claude/worktrees/probe/src/leak.rs"),
            served_topic(),
        )
        .expect("nested build output");

        let mut manifests = Vec::new();
        collect_named(&root, OsStr::new("Cargo.toml"), &mut manifests).expect("manifest inventory");
        manifests.sort();
        assert_eq!(
            manifests,
            vec![root.join("Cargo.toml"), root.join("member/Cargo.toml")]
        );
        let mut rust_files = Vec::new();
        collect_extension(&root, OsStr::new("rs"), &mut rust_files).expect("Rust source inventory");
        assert!(rust_files.is_empty());
        check_publication_bytes(&root).expect("linked worktree bytes are another candidate");
        fs::remove_dir_all(&root).expect("clean up");
    }

    #[test]
    fn jes_oracle_requires_an_external_reviewed_receipt() {
        let root = repository_root().expect("repository root");
        let pending = check_jes_oracle_receipt(&root, None)
            .expect_err("missing licensed receipt must remain pending");
        assert!(pending.contains("licensed z/OS 3.2 JES2 differential is pending"));
        let local_artifact =
            root.join("conformance/subsystems/jes/oracles/jes-licensed-differential.json");
        let error = check_jes_oracle_receipt(&root, Some(local_artifact))
            .expect_err("a candidate-tree artifact must not substitute for a licensed receipt");
        assert!(error.contains("must remain external to the candidate tree"));
    }

    #[test]
    fn strict_cli_rejects_unknown_duplicate_and_surplus_arguments() {
        for arguments in [
            vec!["xtask", "subsystems", "--unknown"],
            vec!["xtask", "subsystems", "--check", "--check"],
            vec!["xtask", "subsystems", "surplus"],
            vec!["xtask", "release"],
            vec!["xtask", "versions"],
            vec!["xtask", "evidence", "seal"],
        ] {
            assert!(Cli::try_parse_from(arguments).is_err());
        }
        assert!(Cli::try_parse_from(["xtask", "subsystems", "--check"]).is_ok());
        assert!(Cli::try_parse_from(["xtask", "jes-oracle-candidate"]).is_ok());
        assert!(Cli::try_parse_from(["xtask", "jes-oracle-candidate", "surplus"]).is_err());
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
    fn schema_discovery_includes_nested_subsystems_and_profiles() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = env::temp_dir().join(format!(
            "mainframe-env-schema-discovery-{}-{nonce}",
            std::process::id()
        ));
        for relative in [
            "conformance/subsystems/platform/schemas/old.json",
            "conformance/subsystems/cics/application/schemas/new.json",
            "conformance/subsystems/cics/system/schemas/nested/future.json",
            "conformance/spec/schemas/not-versioned.json",
        ] {
            let path = root.join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, b"{}\n").unwrap();
        }
        let discovered = subsystem_schema_files(&root)
            .unwrap()
            .into_iter()
            .map(|path| path.strip_prefix(&root).unwrap().to_path_buf())
            .collect::<Vec<_>>();
        assert_eq!(
            discovered,
            vec![
                PathBuf::from("conformance/subsystems/cics/application/schemas/new.json"),
                PathBuf::from("conformance/subsystems/cics/system/schemas/nested/future.json"),
                PathBuf::from("conformance/subsystems/platform/schemas/old.json"),
            ]
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn fast_spec_gate_rejects_same_count_catalog_mutation_under_stale_index() {
        let root = temporary_git_repository("catalog-closure-drift");
        let catalog_relative = "conformance/subsystems/coverage/catalogs/mock.json";
        let catalog_path = root.join(catalog_relative);
        fs::create_dir_all(catalog_path.parent().expect("catalog parent"))
            .expect("catalog directory");
        fs::write(&catalog_path, br#"{"rows":[{"label":"one"}]}"#).expect("catalog");
        let expected = format!(
            "sha256:{}",
            file_digest(&catalog_path).expect("catalog digest")
        );
        let index_path = root.join("conformance/subsystems/coverage/catalogs/index.json");
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
}

fn smoke_runtime_target(root: &Path, target: &str) -> TaskResult {
    let host = host_target(root)?;
    require(
        host == target,
        &format!(
            "runtime target {target} cannot be certified on host {host}; exact binaries must execute"
        ),
    )?;
    let version = env!("CARGO_PKG_VERSION").to_string();
    let cli = runtime_binary(root, target, "mainframe-env");
    let server = runtime_binary(root, target, "mainframe-env-server");
    for binary in [&cli, &server] {
        require(
            binary.is_file(),
            &format!("{} is missing", binary.display()),
        )?;
        probe_runtime_binary(binary, "--version", &version)?;
        probe_runtime_binary(binary, "--help", "Usage:")?;
    }
    runtime_server_sqlite_smoke(root, &server)
}

fn runtime_server_sqlite_smoke(root: &Path, binary: &Path) -> TaskResult {
    require(binary.is_file(), "runtime server binary is missing")?;
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
    let result = (|| {
        let (mut first, first_port) =
            spawn_runtime_server(root, binary, &directory, &database, true)?;
        let first_result = wait_for_runtime_readiness(&mut first, first_port);
        stop_runtime_server(&mut first);
        first_result?;

        let (mut restarted, restarted_port) =
            spawn_runtime_server(root, binary, &directory, &database, false)?;
        let restart_result = wait_for_runtime_readiness(&mut restarted, restarted_port)
            .and_then(|()| authenticate_runtime_administrator(restarted_port));
        stop_runtime_server(&mut restarted);
        restart_result
    })();
    if directory.exists() {
        fs::remove_dir_all(&directory)
            .map_err(|error| format!("{}: {error}", directory.display()))?;
    }
    result
}
#[cfg(test)]
mod common_program_policy_tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    const CATALOG: &str = "conformance/subsystems/coverage/programs/common-programs.json";
    const PROGRAM: &str = "crates/apps/mainframe-env-batch/src/program.rs";
    const GENERATED: &str = "crates/apps/mainframe-env-batch/src/generated/common_programs.rs";
    const SCHEMA: &str =
        "conformance/subsystems/coverage/schemas/common-program-catalog.schema.json";

    struct Fixture {
        root: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let source = repository_root().unwrap();
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let root =
                env::temp_dir().join(format!("common-policy-{}-{nonce}", std::process::id()));
            for relative in [
                CATALOG,
                SCHEMA,
                PROGRAM,
                "crates/apps/mainframe-env-batch/src/service.rs",
                "conformance/subsystems/coverage/inventory/contracts.json",
                "docs/architecture/PROGRAM-AND-ROUTE-REGISTRIES.md",
                "tools/check_typed_semantic_boundaries.py",
            ] {
                let target = root.join(relative);
                fs::create_dir_all(target.parent().unwrap()).unwrap();
                fs::copy(source.join(relative), target).unwrap();
            }
            let fixture = Self { root };
            fixture.regenerate();
            fixture
        }
        fn regenerate(&self) {
            let generated = render_program_registry(&self.root).unwrap();
            let path = self.root.join(GENERATED);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, generated).unwrap();
        }
        fn mutate(&self, mutate: impl FnOnce(&mut Value)) {
            let path = self.root.join(CATALOG);
            let mut catalog = json(&path).unwrap();
            mutate(&mut catalog);
            assert_eq!(catalog["programs"].as_array().unwrap().len(), 21);
            assert_eq!(catalog["generated_coverage_credit"], 0);
            fs::write(path, serde_json::to_vec_pretty(&catalog).unwrap()).unwrap();
        }
        fn append_program_source(&self, extra: &str) {
            let path = self.root.join(PROGRAM);
            let original = read(&path).unwrap();
            fs::write(path, format!("{original}\n{extra}\n")).unwrap();
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn common_program_policy_actual_registry_control_is_current_and_deterministic() {
        let fixture = Fixture::new();
        let first = render_program_registry(&fixture.root).unwrap();
        assert_eq!(render_program_registry(&fixture.root).unwrap(), first);
        assert_eq!(fs::read(fixture.root.join(GENERATED)).unwrap(), first);
        check_program_registry(&fixture.root).unwrap();
    }

    #[test]
    fn common_program_policy_actual_registry_rejects_production_name_policy() {
        let fixture = Fixture::new();
        fixture.append_program_source(
            "fn cv208_policy_mutant(program: &str) -> bool {\n\
             match program { \"IEBCOPY\" => true, _ => false }\n}",
        );
        // Generated bytes/digest remain coherent; no stale-output refusal may count.
        assert_eq!(
            render_program_registry(&fixture.root).unwrap(),
            fs::read(fixture.root.join(GENERATED)).unwrap()
        );
        let refusal = check_program_registry(&fixture.root);
        assert!(
            refusal.is_err(),
            "handwritten production control policy passed: {refusal:?}"
        );
        assert!(
            refusal.unwrap_err().contains("dispatch"),
            "the declared policy boundary, not unrelated metadata, must refuse"
        );
    }

    #[test]
    fn common_program_policy_actual_registry_keeps_independent_test_expectations() {
        let fixture = Fixture::new();
        fixture.append_program_source(
            "#[cfg(test)] mod cv208_expected_route {\n\
             fn literal_expectation(program: &str) -> bool {\n\
             match program { \"IEBCOPY\" => true, _ => false }\n}}\n\
             fn cv208_production_tail() -> bool { true }\n",
        );
        // Reuse the sealed production scanner; literal test expectations are legitimate.
        check_program_registry(&fixture.root).unwrap();
    }

    #[test]
    fn common_program_policy_generator_refuses_unbound_builtin_in_one_utility() {
        let fixture = Fixture::new();
        fixture.mutate(|catalog| {
            let row = catalog["programs"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .find(|row| row["name"] == "IEBCOPY")
                .unwrap();
            row["builtin"] = json!("cv208_unknown");
        });
        // Count, names, lower-case spelling and unique builtin cardinality remain valid.
        // A policyless identity must be refused by the owner before Rust compilation.
        let rendered = render_program_registry(&fixture.root);
        assert!(rendered.is_err(), "unbound builtin generated successfully");
    }

    #[test]
    fn common_program_policy_frozen_v1_does_not_silently_accept_new_policy_fields() {
        let fixture = Fixture::new();
        fixture.mutate(|catalog| {
            let row = catalog["programs"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .find(|row| row["name"] == "IEBCOPY")
                .unwrap();
            row["control"] = json!({"grammar":"ignore-input","sources":[]});
        });
        let schema = json(&fixture.root.join(SCHEMA)).unwrap();
        let catalog = json(&fixture.root.join(CATALOG)).unwrap();
        let compiled = compile_draft_2020_12_schema(&schema, &fixture.root.join(SCHEMA)).unwrap();
        assert!(
            !compiled.is_valid(&catalog),
            "frozen @1 unexpectedly permits policy fields"
        );
        assert!(
            render_program_registry(&fixture.root).is_err(),
            "generator silently ignored a schema-invalid @1 policy override"
        );
    }

    #[test]
    fn common_program_policy_generator_keeps_missing_utility_binding_refusal() {
        let fixture = Fixture::new();
        fixture.mutate(|catalog| {
            let row = catalog["programs"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .find(|row| row["name"] == "IEBCOPY")
                .unwrap();
            row["builtin"] = Value::Null;
        });
        assert!(render_program_registry(&fixture.root).is_err());
    }

    #[test]
    fn common_program_policy_generator_keeps_missing_nested_tso_binding_refusal() {
        let fixture = Fixture::new();
        fixture.mutate(|catalog| {
            let row = catalog["programs"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .find(|row| row["name"] == "DSNTIAD")
                .unwrap();
            row["tso_action"] = Value::Null;
        });
        assert!(render_program_registry(&fixture.root).is_err());
    }

    #[test]
    fn common_program_policy_pairing_refuses_idcams_builtin_with_program_service() {
        let fixture = Fixture::new();
        fixture.mutate(|catalog| {
            let row = catalog["programs"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .find(|row| row["name"] == "IDCAMS")
                .unwrap();
            assert_eq!(row["builtin"], "idcams");
            assert_eq!(row["execution"], "idcams");
            row["execution"] = json!("program-service");
        });
        let schema = json(&fixture.root.join(SCHEMA)).unwrap();
        let catalog = json(&fixture.root.join(CATALOG)).unwrap();
        validate_schema_instance(&schema, &catalog, &fixture.root.join(CATALOG)).unwrap();
        let rendered = render_program_registry(&fixture.root);
        assert!(
            rendered.is_err(),
            "native-valid Idcams builtin with ProgramService execution generated successfully"
        );
        assert!(rendered.unwrap_err().contains("builtin/execution pairing"));
    }

    #[test]
    fn common_program_policy_pairing_refuses_other_builtins_with_idcams() {
        for (name, builtin) in [
            ("IEBCOMPR", "iebcompr"),
            ("IEBCOPY", "iebcopy"),
            ("IEBDG", "iebdg"),
            ("IEBEDIT", "iebedit"),
            ("IEBGENER", "iebgener"),
            ("IEBUPDTE", "iebupdte"),
            ("IEFBR14", "iefbr14"),
            ("SORT", "sort"),
        ] {
            let fixture = Fixture::new();
            fixture.mutate(|catalog| {
                let row = catalog["programs"]
                    .as_array_mut()
                    .unwrap()
                    .iter_mut()
                    .find(|row| row["name"] == name)
                    .unwrap();
                assert_eq!(row["builtin"], builtin);
                assert_eq!(row["execution"], "program-service");
                row["execution"] = json!("idcams");
            });
            let schema = json(&fixture.root.join(SCHEMA)).unwrap();
            let catalog = json(&fixture.root.join(CATALOG)).unwrap();
            validate_schema_instance(&schema, &catalog, &fixture.root.join(CATALOG)).unwrap();
            let rendered = render_program_registry(&fixture.root);
            assert!(
                rendered.is_err(),
                "native-valid {builtin} builtin with Idcams execution generated successfully"
            );
            assert!(rendered.unwrap_err().contains("builtin/execution pairing"));
        }
    }
}
