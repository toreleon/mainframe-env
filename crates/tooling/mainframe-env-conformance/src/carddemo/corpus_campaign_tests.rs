//! Actual pinned-input producer, separate from ordinary synthetic unit controls.
use super::*;

const DRIVER: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../tools/carddemo_corpus_campaign.py"
));
const DISPATCHER: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../xtask/src/main.rs"
));
const SUPERVISOR: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../tools/ci_assurance.py"
));

#[test]
#[ignore = "internal subprocess probe; requires a declared campaign case"]
fn corpus_campaign_child_probe() {
    let case = env::var("CARDDEMO_CORPUS_CONTROL_CASE").expect("missing declared control case");
    let inventory = PathBuf::from(
        env::var_os("CARDDEMO_CORPUS_CONTROL_INVENTORY")
            .expect("missing declared control inventory"),
    );
    let corpus = PathBuf::from(env::var_os(CORPUS_ENV).expect("missing declared control corpus"));
    if case == "baseline" {
        let receipt = verify_carddemo_corpus_from_env(&inventory).unwrap();
        corpus_observations::require_relative_evidence(&corpus, &receipt).unwrap();
        let expected: serde_json::Value =
            serde_json::from_slice(corpus_observations::INVENTORY).unwrap();
        assert_eq!(receipt.commit, expected["commit"].as_str().unwrap());
        assert_eq!(receipt.tree, expected["tree"].as_str().unwrap());
        assert_eq!(
            receipt.content.sha256,
            expected["executable_content_identity"]["sha256"]
                .as_str()
                .unwrap()
        );
        let tracked = tracked_files(&corpus).unwrap();
        println!(
            "\nCD001_PROBE_JSON={}",
            serde_json::json!({"case":case,
            "receipt":receipt, "tracked_paths_sha256":sha256(&serde_json::to_vec(&tracked).unwrap())})
        );
        return;
    }
    let code = match case.as_str() {
        "dirty" | "missing-tracked" => "carddemo.corpus.dirty",
        "same-count-corrupt" | "content-drift" => "carddemo.corpus.content_drift",
        "commit-drift" => "carddemo.corpus.commit_drift",
        "tree-drift" => "carddemo.corpus.tree_drift",
        "license-sha-drift" => "carddemo.corpus.license_drift",
        "unlicensed" => "carddemo.corpus.license_unaccepted",
        "runtime-archive-drift" => "carddemo.corpus.runtime_archive_drift",
        "file-count-drift" => "carddemo.corpus.file_count_drift",
        "repository-local-path" => "carddemo.corpus.repository_drift",
        "missing-directory" => "carddemo.corpus.directory_missing",
        _ => panic!("undeclared corpus control case"),
    };
    if case == "same-count-corrupt" {
        assert!(
            git(
                &corpus,
                &["status", "--porcelain=v1", "--untracked-files=all"]
            )
            .unwrap()
            .is_empty()
        );
        let expected = read_contract(&inventory).unwrap();
        let tracked = tracked_files(&corpus).unwrap();
        for check in &expected.file_count_checks {
            assert_eq!(count_files(&tracked, check).unwrap(), check.expected);
        }
    }
    let error = verify_carddemo_corpus_from_env(&inventory).unwrap_err();
    assert_eq!(error.code, code);
    assert!(!error.to_string().contains(corpus.to_str().unwrap()));
    assert!(!error.to_string().contains(inventory.to_str().unwrap()));
    if case == "repository-local-path" {
        assert_eq!(
            error.detail,
            "repository origin differs from the pinned inventory"
        );
    }
    let tracked_paths = if case == "same-count-corrupt" {
        Some(sha256(
            &serde_json::to_vec(&tracked_files(&corpus).unwrap()).unwrap(),
        ))
    } else {
        None
    };
    println!(
        "\nCD001_PROBE_JSON={}",
        serde_json::json!({"case":case,
        "code":error.code, "detail":error.detail, "tracked_paths_sha256":tracked_paths})
    );
}

#[test]
#[ignore = "requires exact pinned corpus, release xtask and fresh external evidence"]
fn pinned_corpus_source_complete_negative_campaign() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .unwrap();
    let python = env::var_os("CARDDEMO_CORPUS_CAMPAIGN_PYTHON").expect("missing pinned Python");
    let xtask =
        env::var_os("CARDDEMO_CORPUS_CAMPAIGN_XTASK").expect("missing actual compiled xtask");
    let corpus = env::var_os(CORPUS_ENV).expect("missing exact pinned corpus");
    let evidence =
        env::var_os("CARDDEMO_CORPUS_CAMPAIGN_EVIDENCE").expect("missing fresh evidence");
    let status = Command::new(python)
        .arg("-B")
        .arg(root.join("tools/carddemo_corpus_campaign.py"))
        .arg("--repo")
        .arg(&root)
        .arg("--corpus")
        .arg(corpus)
        .arg("--evidence")
        .arg(&evidence)
        .arg("--test-binary")
        .arg(env::current_exe().unwrap())
        .arg("--xtask-binary")
        .arg(xtask)
        .arg("--driver-sha256")
        .arg(sha256(DRIVER))
        .arg("--supervisor-sha256")
        .arg(sha256(SUPERVISOR))
        .arg("--dispatcher-sha256")
        .arg(sha256(DISPATCHER))
        .current_dir(&root)
        .status()
        .unwrap();
    // The driver owns all namespaces and child sessions. A failed/unknown
    // command or retention proof keeps its copy; this test never deletes it.
    assert!(
        status.success(),
        "source-complete corpus campaign failed; retain its namespace"
    );
    let result: serde_json::Value =
        serde_json::from_slice(&fs::read(PathBuf::from(evidence).join("campaign.json")).unwrap())
            .unwrap();
    assert_eq!(result["status"], "pass");
    assert_eq!(result["cleanup"]["status"], "removed-and-absence-verified");
    assert_eq!(result["application_credit"], 0);
    assert_eq!(result["licensed_runtime_credit"], 0);
    // No prior sidecar is admissible: the driver exclusively claimed the fresh
    // namespace and this exact test awaited its actual successful completion.
    let expected = [
        "CARDDEMO_CORPUS_DIR is required only for CardDemo gates",
        "commit, tree, cleanliness, license, content, runtime-archive, and file-count identities are checked",
        "dirty, missing, drifted, or unlicensed inputs fail",
        "no absolute local path enters evidence",
    ];
    let observed = result["requirements"].as_array().unwrap();
    assert_eq!(observed.len(), expected.len());
    let mut observations = RouteObservations::default();
    for (actual, expected) in observed.iter().zip(expected) {
        observations
            .compare(
                journey_closure::AuthorityKind::Issue,
                "CD-001",
                expected,
                actual.as_str() != Some(expected),
                || {
                    Ok(CorpusProblem::new(
                        "carddemo.corpus.observation_invalid",
                        "actual campaign predicate differs",
                    ))
                },
            )
            .unwrap();
    }
    assert_eq!(observations.issues()[0].1.len(), 4);
    // Transient issue proof only. Full-profile wiring needs a fresh whole
    // campaign in that owner, rather than reuse of this test's retained result.
}
