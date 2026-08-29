//! Deterministic repository checks for mainframe-env.

#![forbid(unsafe_code)]

use mainframe_env_conformance::{
    verify_carddemo_corpus_from_env, verify_carddemo_data_layouts_from_env,
    verify_carddemo_source_closures_from_env, verify_carddemo_source_preprocessing_from_env,
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
    let check = args.any(|arg| arg == "--check");

    match command.as_str() {
        "versions" => check_versions(&root),
        "architecture" => check_architecture(&root),
        "runtime-architecture" => check_runtime_architecture(&root),
        "profiles" => check_profiles(&root),
        "schemas" => check_schemas(&root),
        "inventory" => check_inventory(&root),
        "evidence" => check_evidence(&root),
        "conformance" => {
            check_versions(&root)?;
            check_architecture(&root)?;
            check_profiles(&root)?;
            check_schemas(&root)?;
            check_inventory(&root)?;
            check_evidence(&root)
        }
        "certification" => check_certification(&root),
        "carddemo-corpus" => check_carddemo_corpus(&root),
        "carddemo-source" => check_carddemo_source(&root),
        "carddemo-closure" => check_carddemo_closure(&root),
        "carddemo-layout" => check_carddemo_layout(&root),
        "digest" => print_digest(&root),
        "release" if check => check_release_artifacts(&root),
        "release" => generate_release_artifacts(&root),
        "help" | "--help" | "-h" => {
            println!(
                "cargo xtask <versions|architecture|runtime-architecture|profiles|schemas|inventory|evidence|conformance|certification|carddemo-corpus|carddemo-source|carddemo-closure|carddemo-layout|digest|release> --check"
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
        version == "0.1.0-alpha.0",
        "VERSION must remain at the initial development identity",
    )?;
    require(
        cargo_version == version,
        "Cargo workspace version differs from VERSION",
    )?;
    require(
        release_version == version,
        "release.toml version differs from VERSION",
    )?;
    require(release_line == "0.1", "release line must be 0.1")?;
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

    let inventory_path = root.join("conformance/0.1/inventory/versions.json");
    let inventory = json(&inventory_path)?;
    require(
        text(&inventory, "product", &inventory_path)? == version,
        "machine version inventory differs from VERSION",
    )?;

    let notes = read(&root.join("docs/releases/0.1.md"))?;
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
    let additions_path = root.join("conformance/0.1.1/inventory/dependency-graph-additions.json");
    if additions_path.is_file() {
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
        "mainframe-env-source" | "mainframe-env-encoding" => &[],
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
    let directory = root.join("conformance/0.1/schemas");
    let mut files = Vec::new();
    collect_extension(&directory, OsStr::new("json"), &mut files)?;
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
    check_runtime_architecture(root)?;

    let inventory = root.join("conformance/0.1/inventory");
    let packages_path = inventory.join("packages.json");
    let packages = json(&packages_path)?;
    let package_names = array(&packages, "packages", &packages_path)?
        .iter()
        .map(|row| text(row, "name", &packages_path).map(str::to_string))
        .collect::<TaskResult<BTreeSet<_>>>()?;
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
    check_release_artifacts(root)?;
    Ok(())
}

fn print_digest(root: &Path) -> TaskResult {
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
    println!("sha256:{:x}", digest.finalize());
    Ok(())
}

fn generate_release_artifacts(root: &Path) -> TaskResult {
    let documents = release_documents(root)?;
    for (relative, bytes) in documents {
        let path = root.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| format!("{}: {error}", parent.display()))?;
        }
        fs::write(&path, bytes).map_err(|error| format!("{}: {error}", path.display()))?;
    }
    Ok(())
}

fn check_release_artifacts(root: &Path) -> TaskResult {
    for (relative, expected) in release_documents(root)? {
        let path = root.join(&relative);
        let actual = fs::read(&path).map_err(|error| format!("{}: {error}", path.display()))?;
        require(
            actual == expected,
            &format!("{} is stale; run cargo xtask release", relative.display()),
        )?;
    }
    Ok(())
}

fn release_documents(root: &Path) -> TaskResult<BTreeMap<PathBuf, Vec<u8>>> {
    let directory = PathBuf::from("release/0.1.0-alpha.0");
    let server = root.join("target/release/mainframe-env-server");
    let cli = root.join("target/release/mainframe-env");
    require(server.is_file(), "release server binary has not been built")?;
    require(cli.is_file(), "release CLI binary has not been built")?;
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
            "component":{"type":"application","name":"mainframe-env","version":"0.1.0-alpha.0"},
            "properties":[
                {"name":"mainframe-env:phase-base","value":phase_base},
                {"name":"mainframe-env:cargo-lock-sha256","value":lock_digest}
            ]
        },
        "components":components
    });
    let manifest = json!({
        "schema_version":"mainframe-env.release-manifest@1",
        "product":"mainframe-env",
        "version":"0.1.0-alpha.0",
        "channel":"alpha",
        "phase_base_revision":phase_base,
        "toolchain":rustc,
        "target":format!("{}-{}",std::env::consts::ARCH,std::env::consts::OS),
        "profile":"release/core-server",
        "contracts":"conformance/0.1/inventory/versions.json",
        "migration_head":"0001-durable-state",
        "artifacts":[
            {"path":"bin/mainframe-env-server","sha256":server_digest},
            {"path":"bin/mainframe-env","sha256":cli_digest},
            {"path":"config/mainframe-env.toml","sha256":config_digest},
            {"path":"migrations/sqlite/0001-durable-state.sql","sha256":sqlite_migration},
            {"path":"migrations/postgres/0001-durable-state.sql","sha256":postgres_migration}
        ],
        "tag":null,
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
                "externalParameters":{"profile":"release","locked":true,"all_features":true},
                "resolvedDependencies":[{"uri":"Cargo.lock","digest":{"sha256":lock_digest}}]
            },
            "runDetails":{"builder":{"id":"local-codex-workspace"},"metadata":{"invocationId":"ME.V7-local"}}
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
