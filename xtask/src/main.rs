//! Deterministic repository checks for mainframe-env.

#![forbid(unsafe_code)]

use mainframe_env_conformance::{
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
    verify_carddemo_vsam_from_env, verify_host_abi_libraries,
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

fn main() {
    if let Err(error) = run() {
        eprintln!("xtask: {error}");
        std::process::exit(1);
    }
}

fn run() -> TaskResult {
    let root = repository_root()?;
    let mut args = env::args().skip(1);
    let command = args.next().unwrap_or_else(|| "help".to_string());
    let arguments = args.collect::<Vec<_>>();
    let check = arguments.iter().any(|arg| arg == "--check");

    match command.as_str() {
        "versions" => check_versions(&root),
        "architecture" => check_architecture(&root),
        "runtime-architecture" => check_runtime_architecture(&root),
        "profiles" => check_profiles(&root),
        "schemas" => check_schemas(&root),
        "inventory" => check_inventory(&root),
        "evidence" => check_evidence(&root),
        "coverage" => check_coverage(&root),
        "application-packages" => check_application_packages(&root),
        "db2-catalog" => check_db2_catalog(&root),
        "batch-controllers" => check_batch_controllers(&root),
        "abi-libraries" if check => check_host_abi_libraries(&root),
        "abi-libraries" => generate_host_abi_inventory(&root),
        "program-registry" if check => check_program_registry(&root),
        "program-registry" => generate_program_registry(&root),
        "route-registries" if check => check_route_registries(&root),
        "route-registries" => generate_route_registries(&root),
        "dehardcoding" => check_dehardcoding(&root),
        "ledger-consistency" => check_workload_ledger_consistency(&root),
        "migration-rollback" => check_migration_rollback_rollup(&root),
        "full-regression" => check_full_regression(&root),
        "review-repair" => check_review_repair(&root),
        "semantic-identities" if check => check_semantic_identities(&root),
        "semantic-identities" => generate_semantic_identities(&root),
        "conformance" => {
            check_versions(&root)?;
            check_architecture(&root)?;
            check_profiles(&root)?;
            check_schemas(&root)?;
            check_inventory(&root)?;
            check_evidence(&root)?;
            check_coverage(&root)?;
            check_semantic_identities(&root)?;
            check_application_packages(&root)?;
            check_db2_catalog(&root)?;
            check_batch_controllers(&root)?;
            check_host_abi_libraries(&root)?;
            check_program_registry(&root)?;
            check_route_registries(&root)?;
            check_migration_rollback_rollup(&root)?;
            check_full_regression(&root)?;
            check_review_repair(&root)
        }
        "certification" => check_certification(&root),
        "carddemo-corpus" => check_carddemo_corpus(&root),
        "carddemo-source" => check_carddemo_source(&root),
        "carddemo-closure" => check_carddemo_closure(&root),
        "carddemo-layout" => check_carddemo_layout(&root),
        "carddemo-control" => check_carddemo_control(&root),
        "carddemo-core" => check_carddemo_core(&root),
        "carddemo-file-call" => check_carddemo_file_call(&root),
        "carddemo-host" => check_carddemo_host(&root),
        "carddemo-package" => check_carddemo_package(&root),
        "carddemo-resources" => check_carddemo_resources(&root),
        "carddemo-programs" => check_carddemo_programs(&root),
        "carddemo-cics" => check_carddemo_cics(&root),
        "carddemo-cics-runtime" => check_carddemo_cics_runtime(&root),
        "carddemo-vsam" => check_carddemo_vsam(&root),
        "carddemo-dataset-catalog" => check_carddemo_dataset_catalog(&root),
        "carddemo-seeds" => check_carddemo_seeds(&root),
        "carddemo-security" => check_carddemo_security(&root),
        "carddemo-terminal" => check_carddemo_terminal(&root),
        "carddemo-base-online" => check_carddemo_base_online(&root),
        "carddemo-jcl" => check_carddemo_jcl(&root),
        "carddemo-utilities" => check_carddemo_utilities(&root),
        "carddemo-batch-programs" => check_carddemo_batch_programs(&root),
        "carddemo-base-batch" => check_carddemo_base_batch(&root),
        "carddemo-db2" => check_carddemo_db2(&root),
        "carddemo-ims" => check_carddemo_ims(&root),
        "carddemo-mq-authorization" => check_carddemo_mq_authorization(&root),
        "carddemo-operator-install" => check_carddemo_operator_install(&root),
        "carddemo-operator-compile" => check_carddemo_operator_compile(&root),
        "carddemo-operator-submit" => check_carddemo_operator_submit(&root),
        "carddemo-operator-reset" => check_carddemo_operator_reset(&root),
        "carddemo-full" => check_carddemo_full(&root),
        "digest" => print_digest(&root),
        "release" if check => {
            let target = explicit_release_target(&arguments)?;
            check_release_artifacts(&root, &target)
        }
        "release" => {
            let target = explicit_release_target(&arguments)?;
            generate_release_artifacts(&root, &target)
        }
        "help" | "--help" | "-h" => {
            println!(
                "cargo xtask <versions|architecture|runtime-architecture|profiles|schemas|inventory|evidence|coverage|semantic-identities|application-packages|db2-catalog|batch-controllers|abi-libraries|program-registry|route-registries|dehardcoding|ledger-consistency|migration-rollback|full-regression|review-repair|conformance|certification|carddemo-corpus|carddemo-source|carddemo-closure|carddemo-layout|carddemo-control|carddemo-core|carddemo-file-call|carddemo-host|carddemo-package|carddemo-resources|carddemo-programs|carddemo-cics|carddemo-cics-runtime|carddemo-vsam|carddemo-dataset-catalog|carddemo-seeds|carddemo-security|carddemo-terminal|carddemo-base-online|carddemo-jcl|carddemo-utilities|carddemo-batch-programs|carddemo-base-batch|carddemo-db2|carddemo-ims|carddemo-mq-authorization|carddemo-operator-install|carddemo-operator-compile|carddemo-operator-submit|carddemo-operator-reset|carddemo-full|digest|release> --check; release additionally requires --target <triple>"
            );
            Ok(())
        }
        other => Err(format!("unknown command {other:?}")),
    }?;

    if check {
        println!("{command}: pass");
    }
    Ok(())
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
    let receipt_value = serde_json::to_value(&receipt).map_err(|error| error.to_string())?;
    let receipt_digest = format!(
        "sha256:{:x}",
        Sha256::digest(serde_json::to_vec(&receipt_value).map_err(|error| error.to_string())?)
    );
    println!(
        "{}",
        serde_json::to_string_pretty(&receipt).map_err(|error| error.to_string())?
    );
    let evidence = json(&root.join("conformance/0.1.1/evidence/issues/CD-017.json"))?;
    require(
        evidence["issue"] == Value::String("CD-017".into())
            && evidence["derived"] == Value::Bool(true)
            && evidence["status"] == Value::String("pass".into()),
        "CD-017 evidence is not a derived pass",
    )?;
    require(
        evidence["security_receipt"] == receipt_value,
        "CD-017 security receipt is stale",
    )?;
    require(
        evidence["evidence_digest"].as_str() == Some(receipt_digest.as_str()),
        "CD-017 evidence digest differs",
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
    let receipt_digest = format!(
        "sha256:{:x}",
        Sha256::digest(serde_json::to_vec(&receipt_value).map_err(|error| error.to_string())?)
    );
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
    require(
        evidence["jcl_receipt"] == receipt_value,
        "CD-020 JCL receipt is stale",
    )?;
    require(
        evidence["evidence_digest"].as_str() == Some(receipt_digest.as_str()),
        "CD-020 evidence digest differs",
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
    let evidence = json(&root.join("conformance/0.1.1/evidence/issues/CD-023.json"))?;
    require(
        receipt.status == "pass"
            && receipt.journeys_passed == 3
            && evidence["issue"] == Value::String("CD-023".into())
            && evidence["derived"] == Value::Bool(true)
            && evidence["status"] == Value::String("pass".into()),
        "CD-023 evidence is not a complete derived pass",
    )?;
    require(
        evidence["base_batch_receipt"] == receipt_value,
        "CD-023 base-batch receipt is stale",
    )?;
    require(
        evidence["evidence_digest"].as_str() == Some(receipt_digest.as_str()),
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

fn check_versions(root: &Path) -> TaskResult {
    let version = read(&root.join("VERSION"))?.trim().to_string();
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
    let msrv = cargo["workspace"]["package"]["rust-version"]
        .as_str()
        .ok_or("workspace.package.rust-version is missing")?;
    let pinned = release["rust"]["pinned"]
        .as_str()
        .ok_or("release rust.pinned is missing")?;

    require(
        version == "0.2.0",
        "VERSION must identify the 0.2.0 candidate",
    )?;
    require(
        cargo_version == version,
        "Cargo workspace version differs from VERSION",
    )?;
    require(
        release_version == version,
        "release.toml version differs from VERSION",
    )?;
    require(release_line == "0.2", "release line must be 0.2")?;
    require(msrv == "1.95", "workspace MSRV must be 1.95")?;
    require(pinned == "1.98.0", "pinned Rust toolchain must be 1.98.0")?;

    let mut manifests = Vec::new();
    collect_named(root, OsStr::new("Cargo.toml"), &mut manifests)?;
    for manifest in manifests {
        if manifest == root.join("Cargo.toml") {
            continue;
        }
        let parsed: toml::Value = read(&manifest)?
            .parse()
            .map_err(|error| format!("{}: {error}", manifest.display()))?;
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

    let inventory_path = root.join("conformance/0.2/inventory/versions.json");
    let inventory = json(&inventory_path)?;
    require(
        text(&inventory, "product", &inventory_path)? == version,
        "machine version inventory differs from VERSION",
    )?;

    let notes = read(&root.join(format!("docs/releases/{release_line}.md")))?;
    require(
        notes.contains(&version),
        "release notes omit current version",
    )?;
    Ok(())
}

fn check_architecture(root: &Path) -> TaskResult {
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
    check_runtime_unit_gates(root)?;
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
    let known: BTreeSet<_> = array(&inventory, "packages", &inventory_path)?
        .iter()
        .filter_map(|row| row.get("name").and_then(Value::as_str))
        .collect();
    let excluded = excluded_names(root)?;

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
        let declared = array(&profiles, "profiles", &profiles_path)?
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
    require(!files.is_empty(), "no evidence schemas found")?;
    for file in files {
        let value = json(&file)?;
        let root_object = object(&value, &file)?;
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
    }
    Ok(())
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
            && migration["provider_state_changed"] == Value::Bool(false)
            && migration["atomicity"]["validate_all_references_before_stage"] == Value::Bool(true)
            && migration["atomicity"]["staged_generation_selectable"] == Value::Bool(false)
            && migration["atomicity"]["ready_and_selection_same_critical_section"]
                == Value::Bool(true)
            && migration["atomicity"]["production_trust_authority_injected"] == Value::Bool(true)
            && migration["atomicity"]["selected_handle_required_for_publication"]
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
        "applications_v2: ApplicationInstallerV2",
        "pub fn open_with_package_trust",
        "HmacSha256PackageTrust",
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
        production_product.contains("pub fn install_application_batch_controllers")
            && production_product.contains("selected_application_v2")
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
    let mut scanned_files = 0usize;
    for rust_file in rust_files {
        if rust_file.starts_with(&conformance) {
            continue;
        }
        scanned_files += 1;
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
    let hardcode_path = root.join("conformance/0.2/evidence/hardcode/no-application-hardcode.json");
    let hardcode = json(&hardcode_path)?;
    require(
        hardcode["schema_version"]
            == Value::String("mainframe-env.no-application-hardcode@1".into())
            && hardcode["before"]["production_h1_h3_lines"].as_u64() == Some(32)
            && hardcode["after"]["production_h1_h3_lines"].as_u64() == Some(0)
            && hardcode["after"]["scanned_rust_files"].as_u64() == Some(scanned_files as u64)
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
    for entry in fs::read_dir(root.join("conformance/0.2"))
        .map_err(|error| format!("conformance/0.2: {error}"))?
    {
        let path = entry.map_err(|error| error.to_string())?.path();
        require(
            !matches!(
                path.extension().and_then(OsStr::to_str),
                Some("pdf" | "html")
            ),
            "official publication bytes must not be checked into conformance/0.2",
        )?;
    }
    check_coverage_work_package_evidence(root)?;
    check_coverage_program_status(root)?;
    check_workload_ledger_consistency(root)
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
        let canonical = serde_json::to_vec(receipt).map_err(|error| error.to_string())?;
        let digest = format!("sha256:{:x}", Sha256::digest(canonical));
        require(
            evidence["evidence_digest"].as_str() == Some(digest.as_str()),
            &format!("{} evidence digest is stale", path.display()),
        )?;
        let receipt = Value::Object(receipt.clone());
        require(
            receipt["target_version"] == Value::String("0.2.0".into())
                && receipt["source_identity"]
                    .as_str()
                    .is_some_and(|identity| identity.len() == 40)
                && receipt["commands"]
                    .as_array()
                    .is_some_and(|commands| !commands.is_empty())
                && receipt["invariants"].is_object(),
            &format!("{} receipt is incomplete", path.display()),
        )?;
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
                expected == format!("sha256:{}", file_digest(&root.join(artifact_path))?),
                &format!("{} artifact {artifact_path} drifted", path.display()),
            )?;
        }
    }
    Ok(())
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
        recorded_digest == repository_digest(root)?,
        "0.2 program status dirty-tree digest is stale",
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
    require(
        candidate["source_digest"].as_str() == Some(repository_digest(root)?.as_str())
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
        "cargo xtask release --check --target",
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
    for (name, relative) in [
        (
            "manifest_sha256",
            "release/0.2.0/targets/aarch64-apple-darwin/manifest.json",
        ),
        (
            "sbom_sha256",
            "release/0.2.0/targets/aarch64-apple-darwin/sbom.cdx.json",
        ),
        (
            "provenance_sha256",
            "release/0.2.0/targets/aarch64-apple-darwin/provenance.intoto.json",
        ),
        (
            "checksums_sha256",
            "release/0.2.0/targets/aarch64-apple-darwin/checksums.sha256",
        ),
        (
            "licenses_sha256",
            "release/0.2.0/targets/aarch64-apple-darwin/LICENSES.md",
        ),
    ] {
        require(
            receipt["release"][name].as_str() == Some(file_digest(&root.join(relative))?.as_str()),
            &format!("full regression release artifact digest drifted: {relative}"),
        )?;
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
    check_application_packages(root)?;
    check_db2_catalog(root)?;
    check_batch_controllers(root)?;
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
    let canonical = serde_json::to_vec(receipt).map_err(|error| error.to_string())?;
    let digest = format!("sha256:{:x}", Sha256::digest(canonical));
    require(
        evidence["evidence_digest"].as_str() == Some(digest.as_str()),
        "review repair evidence digest is stale",
    )?;
    let receipt = Value::Object(receipt.clone());
    require(
        receipt["target_version"] == Value::String("0.2.0".into())
            && receipt["source_identity"]
                == Value::String("cf834b8d57514f933d1ff6d439949ffb0b8065fa".into())
            && receipt["candidate_source_digest"].as_str()
                == Some(repository_digest(root)?.as_str()),
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
            expected == format!("sha256:{}", file_digest(&root.join(relative))?),
            &format!("review repair artifact digest drifted: {relative}"),
        )?;
    }
    let workflow = read(&root.join(".github/workflows/ci.yml"))?;
    require(
        workflow.contains("fetch-depth: 0")
            && workflow.contains("fetch-tags: true")
            && workflow.contains("cargo xtask release --check --target x86_64-unknown-linux-gnu"),
        "CI does not exercise full-history conformance and portable release verification",
    )?;
    let manifest = json(&root.join("release/0.2.0/targets/aarch64-apple-darwin/manifest.json"))?;
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
    let binary = root.join("target/release/mainframe-env-server");
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

fn repository_digest(root: &Path) -> TaskResult<String> {
    let mut files = Vec::new();
    collect_files(root, &mut files)?;
    files.sort();
    let mut digest = Sha256::new();
    for file in files {
        let relative = file.strip_prefix(root).map_err(|error| error.to_string())?;
        let relative_text = relative.to_string_lossy();
        if relative.starts_with(".git")
            || relative.starts_with("target")
            || relative.starts_with("conformance/0.1/evidence/raw")
            || relative == Path::new("conformance/0.1/evidence/program-status.json")
            || relative == Path::new("conformance/0.2/evidence/program-status.json")
            || relative == Path::new("conformance/0.2/evidence/workload-ledger.json")
            || relative == Path::new("conformance/0.2/evidence/full-regression.json")
            || relative == Path::new("conformance/0.2/evidence/review-repair.json")
            || relative == Path::new("conformance/0.2/evidence/work-packages/CV-209.json")
            || relative == Path::new("docs/delivery/coverage-versions/status/0.2.0.md")
            || (relative_text.starts_with("conformance/0.1/evidence/phase-v")
                && relative.extension() == Some(OsStr::new("json")))
        {
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
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.')),
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
    let status = Command::new("cargo")
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
        .current_dir(root)
        .status()
        .map_err(|error| format!("cargo release build for {target}: {error}"))?;
    require(
        status.success(),
        &format!("release build failed for {target}"),
    )
}

fn generate_release_artifacts(root: &Path, target: &str) -> TaskResult {
    build_release_target(root, target)?;
    let documents = release_documents(root, target)?;
    validate_release_documents(&documents, target)?;
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
    build_release_target(root, target)?;
    let documents = release_documents(root, target)?;
    validate_release_documents(&documents, target)?;
    let directory = root.join(format!(
        "release/{}/targets/{target}",
        read(&root.join("VERSION"))?.trim()
    ));
    if directory.is_dir() {
        for (relative, expected) in documents {
            let path = root.join(&relative);
            let actual = fs::read(&path).map_err(|error| format!("{}: {error}", path.display()))?;
            require(
                actual == expected,
                &format!(
                    "{} is stale; run cargo xtask release --target {target}",
                    relative.display()
                ),
            )?;
        }
    }
    validate_checked_in_release_targets(root)?;
    Ok(())
}

fn validate_checked_in_release_targets(root: &Path) -> TaskResult {
    let version = read(&root.join("VERSION"))?.trim().to_string();
    let directory = root.join(format!("release/{version}/targets"));
    if !directory.is_dir() {
        return Ok(());
    }
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
        let mut documents = BTreeMap::new();
        for name in [
            "manifest.json",
            "sbom.cdx.json",
            "provenance.intoto.json",
            "checksums.sha256",
            "LICENSES.md",
        ] {
            let document = path.join(name);
            documents.insert(
                document
                    .strip_prefix(root)
                    .map_err(|error| error.to_string())?
                    .to_path_buf(),
                fs::read(&document).map_err(|error| format!("{}: {error}", document.display()))?,
            );
        }
        validate_release_documents(&documents, target)?;
    }
    Ok(())
}

fn validate_release_documents(documents: &BTreeMap<PathBuf, Vec<u8>>, target: &str) -> TaskResult {
    require(documents.len() == 5, "target release receipt is incomplete")?;
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
    require(
        manifest["schema_version"] == Value::String("mainframe-env.release-manifest@1".into())
            && manifest["target"] == Value::String(target.into())
            && manifest["published"] == Value::Bool(false)
            && sbom["bomFormat"] == Value::String("CycloneDX".into())
            && sbom["specVersion"] == Value::String("1.6".into())
            && provenance["predicateType"]
                == Value::String("https://slsa.dev/provenance/v1".into())
            && provenance["predicate"]["buildDefinition"]["externalParameters"]["target"]
                == Value::String(target.into())
            && licenses.starts_with("# License expressions\n"),
        "target release receipt metadata is inconsistent",
    )?;
    let artifacts = manifest["artifacts"]
        .as_array()
        .ok_or("release manifest artifacts are missing")?;
    require(
        artifacts.len() == 5,
        "release manifest artifact count drifted",
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
    let version = read(&root.join("VERSION"))?.trim().to_string();
    let release_config: toml::Value = read(&root.join("release.toml"))?
        .parse()
        .map_err(|error| format!("release.toml: {error}"))?;
    let channel = release_config["product"]["channel"]
        .as_str()
        .ok_or("product.channel is missing")?;
    let directory = PathBuf::from(format!("release/{version}/targets/{target}"));
    let executable_suffix = if target.contains("windows") {
        ".exe"
    } else {
        ""
    };
    let server = root.join(format!(
        "target/{target}/release/mainframe-env-server{executable_suffix}"
    ));
    let cli = root.join(format!(
        "target/{target}/release/mainframe-env{executable_suffix}"
    ));
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
            {"path":"migrations/postgres/0001-durable-state.sql","sha256":postgres_migration}
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
                "resolvedDependencies":[{"uri":"Cargo.lock","digest":{"sha256":lock_digest}}]
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
        "{server_digest}  bin/mainframe-env-server\n{cli_digest}  bin/mainframe-env\n{config_digest}  config/mainframe-env.toml\n{sqlite_migration}  migrations/sqlite/0001-durable-state.sql\n{postgres_migration}  migrations/postgres/0001-durable-state.sql\n"
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
    ]))
}

fn pretty_json(value: &Value) -> TaskResult<Vec<u8>> {
    let mut bytes = serde_json::to_vec_pretty(value).map_err(|error| error.to_string())?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn file_digest(path: &Path) -> TaskResult<String> {
    let bytes = fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
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
