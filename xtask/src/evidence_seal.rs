use super::*;

const PROFILE_PATH: &str = "conformance/0.2/evidence/review-repair-round-5-profile.json";
const RECEIPT_PATH: &str = "conformance/0.2/evidence/review-repair-round-5.json";
const PROGRAM_STATUS_PATH: &str = "conformance/0.2/evidence/program-status.json";
const WORKLOAD_LEDGER_PATH: &str = "conformance/0.2/evidence/workload-ledger.json";
const AMENDMENTS_PATH: &str = "conformance/0.2/evidence/work-package-amendments.json";
const FULL_REGRESSION_PATH: &str = "conformance/0.2/evidence/full-regression.json";
const ROUND_FOUR_INPUT_PATH: &str = "conformance/0.2/evidence/review-repair-round-4-inputs.json";
const ROUND_FOUR_RECEIPT_PATH: &str = "conformance/0.2/evidence/review-repair-round-4.json";
const COMMIT_SUBJECT: &str = "Repair 0.2.0 fifth review findings";
const EXPECTED_BRANCH: &str = "impl/0.2.0";
const EXPECTED_PARENT: &str = "36344c542e82a4174f5be7da6038f7095ce6cba8";
const ROUND_FOUR_PARENT: &str = "b61d604fbdf4867154f235d45fd4453ef7e3b79d";
const ROUND_FOUR_INPUT_SHA256: &str =
    "48ff647b06f276a9e5e733de2bdfccbb9b4c6c634fd5ff3912f3794a673ed693";
const ROUND_FOUR_RECEIPT_SHA256: &str =
    "7f9fe7538d9653dfb83e9a65d1ad0c20c50951f991d633a9b090cb8599ef9cc9";
const ROUND_FOUR_EVIDENCE_DIGEST: &str =
    "sha256:fa5ea3ce742892135f2811f559b913bc44bb1fdf6ca0a6e5acba803db4cb3fa3";
const COMMIT_MESSAGE_NAME: &str = "mainframe-env-round-5-commit-message.txt";

const SOURCE_PATHS: [&str; 6] = [
    "conformance/0.2/evidence/review-repair-round-5-profile.json",
    "conformance/0.2/schemas/review-repair-round-5-profile.schema.json",
    "conformance/0.2/schemas/review-repair-round-5.schema.json",
    "docs/releases/0.2.md",
    "xtask/src/evidence_seal.rs",
    "xtask/src/main.rs",
];
const PROJECTION_PATHS: [&str; 5] = [
    RECEIPT_PATH,
    PROGRAM_STATUS_PATH,
    WORKLOAD_LEDGER_PATH,
    AMENDMENTS_PATH,
    FULL_REGRESSION_PATH,
];
const RELEASE_TARGETS: [&str; 2] = ["aarch64-apple-darwin", "x86_64-unknown-linux-gnu"];
const RELEASE_DOCUMENTS: [&str; 6] = [
    "LICENSES.md",
    "build-inputs.json",
    "checksums.sha256",
    "manifest.json",
    "provenance.intoto.json",
    "sbom.cdx.json",
];

#[derive(Clone, Copy)]
struct SealProfile {
    schema_version: &'static str,
    profile_id: &'static str,
    accepted_parent: &'static str,
    subject: &'static str,
    finding_trailer: &'static str,
    source_paths: &'static [&'static str],
    projection_paths: &'static [&'static str],
}

const ROUND_FIVE_PROFILE: SealProfile = SealProfile {
    schema_version: "mainframe-env.seal-profile@1",
    profile_id: "review-repair-round-5",
    accepted_parent: EXPECTED_PARENT,
    subject: COMMIT_SUBJECT,
    finding_trailer: "Fifth-Review-Findings: 4=closed",
    source_paths: &SOURCE_PATHS,
    projection_paths: &PROJECTION_PATHS,
};

#[derive(Clone)]
struct Snapshot {
    files: BTreeMap<PathBuf, Vec<u8>>,
}

impl Snapshot {
    fn from_index(root: &Path) -> TaskResult<Self> {
        let output = Command::new("git")
            .args(["ls-files", "-z", "--cached"])
            .current_dir(root)
            .output()
            .map_err(|error| format!("git ls-files: {error}"))?;
        require(output.status.success(), "git ls-files failed")?;
        let mut files = BTreeMap::new();
        for raw in output
            .stdout
            .split(|byte| *byte == 0)
            .filter(|raw| !raw.is_empty())
        {
            let relative = std::str::from_utf8(raw)
                .map_err(|_| "Git index contains a non-UTF-8 path")?
                .to_string();
            files.insert(PathBuf::from(&relative), git_index_bytes(root, &relative)?);
        }
        Ok(Self { files })
    }

    fn from_commit(root: &Path, commit: &str) -> TaskResult<Self> {
        let listing = command_text(root, "git", &["ls-tree", "-r", "--name-only", commit])?;
        let mut files = BTreeMap::new();
        for relative in listing.lines() {
            files.insert(
                PathBuf::from(relative),
                git_file_bytes(root, commit, relative)?,
            );
        }
        Ok(Self { files })
    }

    fn bytes(&self, relative: &str) -> TaskResult<&[u8]> {
        self.files
            .get(Path::new(relative))
            .map(Vec::as_slice)
            .ok_or_else(|| format!("sealed input is missing {relative}"))
    }

    fn value(&self, relative: &str) -> TaskResult<Value> {
        serde_json::from_slice(self.bytes(relative)?)
            .map_err(|error| format!("{relative}: {error}"))
    }

    fn with_file(&self, relative: &str, bytes: Vec<u8>) -> Self {
        let mut next = self.clone();
        next.files.insert(PathBuf::from(relative), bytes);
        next
    }
}

struct SealOutput {
    files: BTreeMap<PathBuf, Vec<u8>>,
    commit_message: Vec<u8>,
    candidate_digest: String,
    evidence_digest: String,
}

pub(super) fn generate(root: &Path) -> TaskResult {
    validate_precommit_context(root)?;
    validate_historical_round_four(root)?;
    require(
        round_five_completion_commits(root, None)?.is_empty(),
        "round-five completion already exists; seal generation is pre-commit only",
    )?;
    let output = render(root, &Snapshot::from_index(root)?)?;
    for (relative, bytes) in &output.files {
        let path = root.join(relative);
        require(
            path.parent().is_some_and(Path::is_dir),
            "sealed projection parent must already exist",
        )?;
        fs::write(&path, bytes).map_err(|error| format!("{}: {error}", path.display()))?;
    }
    let commit_message = commit_message_path(root)?;
    fs::write(&commit_message, &output.commit_message)
        .map_err(|error| format!("{}: {error}", commit_message.display()))?;
    println!("candidate={}", output.candidate_digest);
    println!("evidence={}", output.evidence_digest);
    println!("commit-message={}", commit_message.display());
    Ok(())
}

pub(super) fn check(root: &Path) -> TaskResult {
    let live_receipt = root.join(RECEIPT_PATH);
    require(
        live_receipt.is_file(),
        "round-five sealed receipt is missing",
    )?;
    validate_historical_round_four(root)?;
    let evidence = json(&live_receipt)?;
    let evidence_digest = evidence["evidence_digest"]
        .as_str()
        .ok_or("round-five sealed evidence digest is missing")?;
    let completions = round_five_completion_commits(root, Some(evidence_digest))?;
    let (snapshot, committed) = match completions.as_slice() {
        [] => {
            validate_precommit_context(root)?;
            (Snapshot::from_index(root)?, None)
        }
        [completion] => {
            validate_committed_context(root, completion)?;
            (
                Snapshot::from_commit(root, completion)?,
                Some(completion.as_str()),
            )
        }
        _ => return Err("round-five completion identity is duplicated".into()),
    };
    let output = render(root, &snapshot)?;
    compare_outputs(&snapshot, &output)?;
    if let Some(completion) = committed {
        let actual = command_text(root, "git", &["show", "-s", "--format=%B", completion])?;
        require(
            actual.as_bytes() == trim_final_newline(&output.commit_message),
            "round-five completion trailers differ from the sealed commit message",
        )?;
        let receipt = snapshot.value(RECEIPT_PATH)?;
        reject_self_reference(&receipt, completion)?;
    } else {
        let path = commit_message_path(root)?;
        let actual = fs::read(&path).map_err(|error| format!("{}: {error}", path.display()))?;
        require(
            actual == output.commit_message,
            "generated round-five commit-message file is stale",
        )?;
    }
    Ok(())
}

pub(super) fn callback(root: &Path) -> TaskResult {
    require_clean_worktree(root)?;
    let head = callback_tip(root)?;
    let evidence = json(&root.join(RECEIPT_PATH))?;
    let evidence_digest = evidence["evidence_digest"]
        .as_str()
        .ok_or("round-five evidence digest is missing")?;
    let completions = round_five_completion_commits(root, Some(evidence_digest))?;
    require(
        completions.as_slice() == [head.as_str()],
        "callback HEAD is not the unique sealed round-five completion",
    )?;
    let remote = command_text(
        root,
        "git",
        &["ls-remote", "origin", "refs/heads/impl/0.2.0"],
    )?;
    require(
        remote.split_whitespace().next() == Some(head.as_str()),
        "callback remote branch does not equal Git HEAD",
    )?;
    let pr = command_json(
        root,
        "gh",
        &[
            "pr",
            "view",
            "1",
            "--json",
            "number,url,state,headRefName,baseRefName,headRefOid",
        ],
    )?;
    require(
        pr["number"].as_u64() == Some(1)
            && pr["state"] == Value::String("OPEN".into())
            && pr["headRefName"] == Value::String(EXPECTED_BRANCH.into())
            && pr["baseRefName"] == Value::String("main".into())
            && pr["headRefOid"] == Value::String(head.clone()),
        "callback PR identity does not match the sealed branch",
    )?;
    let runs = command_json(
        root,
        "gh",
        &[
            "run",
            "list",
            "--commit",
            &head,
            "--workflow",
            "mainframe-env",
            "--limit",
            "20",
            "--json",
            "databaseId,event,headSha,status,conclusion,name,url",
        ],
    )?;
    let run_rows = runs.as_array().ok_or("callback run list is not an array")?;
    let mut checks = Vec::new();
    for event in ["push", "pull_request"] {
        let matching = run_rows
            .iter()
            .filter(|run| {
                run["headSha"].as_str() == Some(head.as_str())
                    && run["event"].as_str() == Some(event)
                    && run["name"].as_str() == Some("mainframe-env")
            })
            .collect::<Vec<_>>();
        require(
            matching.len() == 1,
            &format!("callback requires one {event} workflow run for HEAD"),
        )?;
        let run = matching[0];
        require(
            run["status"] == Value::String("completed".into())
                && run["conclusion"] == Value::String("success".into()),
            &format!("callback {event} workflow run is not terminal success"),
        )?;
        let run_id = run["databaseId"]
            .as_u64()
            .ok_or("callback run ID is missing")?;
        let jobs = command_json(
            root,
            "gh",
            &[
                "api",
                &format!("repos/toreleon/mainframe-env/actions/runs/{run_id}/jobs"),
            ],
        )?;
        let job_rows = validate_callback_jobs(
            &head,
            event,
            run_id,
            jobs["jobs"].as_array().ok_or("callback jobs are missing")?,
        )?;
        checks.push(json!({
            "event": event,
            "run_id": run_id,
            "head_sha": head,
            "status": "completed",
            "conclusion": "success",
            "url": run["url"],
            "jobs": job_rows
        }));
    }
    let value = json!({
        "schema_version": "mainframe-env.round-5-completion@1",
        "target_version": "0.2.0",
        "pr_number": pr["number"],
        "pr_url": pr["url"],
        "branch": EXPECTED_BRANCH,
        "final_tip": head,
        "candidate_digest": evidence["receipt"]["candidate_source_digest"],
        "evidence_digest": evidence_digest,
        "gate_status": "pass",
        "check_status": {"overall": "success", "runs": checks},
        "blocker": Value::Null,
        "remote_actions": {"merged": false, "tagged": false, "published": false, "deployed": false}
    });
    let bytes = pretty_json(&value)?;
    std::io::stdout()
        .write_all(&bytes)
        .map_err(|error| format!("write callback JSON to stdout: {error}"))?;
    Ok(())
}

fn render(root: &Path, snapshot: &Snapshot) -> TaskResult<SealOutput> {
    let profile = snapshot.value(PROFILE_PATH)?;
    validate_profile(&profile)?;
    validate_release_inventory(snapshot)?;
    let amendments = project_amendments(snapshot)?;
    let candidate_snapshot = snapshot.with_file(AMENDMENTS_PATH, pretty_json(&amendments)?);
    let candidate_digest = snapshot_digest(&candidate_snapshot);
    let full_regression = project_full_regression(snapshot, &candidate_digest)?;
    let program_status = project_program_status(snapshot, &candidate_digest)?;
    let workload_ledger = project_workload_ledger(snapshot)?;
    let mut projected = BTreeMap::from([
        (PathBuf::from(AMENDMENTS_PATH), pretty_json(&amendments)?),
        (
            PathBuf::from(FULL_REGRESSION_PATH),
            pretty_json(&full_regression)?,
        ),
        (
            PathBuf::from(PROGRAM_STATUS_PATH),
            pretty_json(&program_status)?,
        ),
        (
            PathBuf::from(WORKLOAD_LEDGER_PATH),
            pretty_json(&workload_ledger)?,
        ),
    ]);
    let receipt = build_receipt(root, snapshot, &projected, &profile, &candidate_digest)?;
    let evidence_digest = canonical_evidence_digest(
        receipt
            .as_object()
            .ok_or("generated round-five receipt is not an object")?,
    )?;
    let evidence = json!({
        "schema_version": "mainframe-env.review-repair-round-5@1",
        "derived": true,
        "status": "pass",
        "evidence_digest": evidence_digest,
        "receipt": receipt
    });
    projected.insert(PathBuf::from(RECEIPT_PATH), pretty_json(&evidence)?);
    Ok(SealOutput {
        files: projected,
        commit_message: commit_message(&evidence_digest),
        candidate_digest,
        evidence_digest,
    })
}

fn build_receipt(
    root: &Path,
    snapshot: &Snapshot,
    projected: &BTreeMap<PathBuf, Vec<u8>>,
    profile: &Value,
    candidate_digest: &str,
) -> TaskResult<Value> {
    let mut artifact_paths = ROUND_FIVE_PROFILE
        .source_paths
        .iter()
        .chain(ROUND_FIVE_PROFILE.projection_paths.iter())
        .copied()
        .filter(|path| *path != RECEIPT_PATH)
        .map(str::to_string)
        .collect::<Vec<_>>();
    artifact_paths.extend(retained_paths());
    artifact_paths.sort();
    artifact_paths.dedup();
    let artifacts = artifact_paths
        .iter()
        .map(|relative| {
            validate_relative(relative)?;
            let bytes = projected
                .get(Path::new(relative))
                .map(Vec::as_slice)
                .unwrap_or(snapshot.bytes(relative)?);
            Ok(json!({"path": relative, "sha256": format!("sha256:{}", digest_bytes(bytes))}))
        })
        .collect::<TaskResult<Vec<_>>>()?;
    Ok(json!({
        "target_version": "0.2.0",
        "accepted_parent": ROUND_FIVE_PROFILE.accepted_parent,
        "candidate_source_digest": candidate_digest,
        "branch": EXPECTED_BRANCH,
        "profile": {
            "path": PROFILE_PATH,
            "schema_version": profile["schema_version"],
            "profile_id": profile["profile_id"],
            "sha256": format!("sha256:{}", digest_bytes(snapshot.bytes(PROFILE_PATH)?))
        },
        "historical_round_four": {
            "completion_commit": EXPECTED_PARENT,
            "accepted_parent": ROUND_FOUR_PARENT,
            "input": {"path": ROUND_FOUR_INPUT_PATH, "sha256": format!("sha256:{ROUND_FOUR_INPUT_SHA256}")},
            "receipt": {"path": ROUND_FOUR_RECEIPT_PATH, "sha256": format!("sha256:{ROUND_FOUR_RECEIPT_SHA256}"), "evidence_digest": ROUND_FOUR_EVIDENCE_DIGEST},
            "validation": "exact Git object bytes and completion trailers",
            "remote_evidence_credit": "none"
        },
        "findings": [
            {"id": 1, "state": "closed", "content_rule": "current receipt schema has no command, count, toolchain, GitHub identity, conclusion, or future-CI fields"},
            {"id": 2, "state": "closed", "content_rule": "exact candidate/projection path sets and exactly six retained documents for each of two targets"},
            {"id": 3, "state": "closed", "content_rule": "live callback requires the exact v0-foundation and contract-msrv job set for both current-tip events"},
            {"id": 4, "state": "closed", "content_rule": "callback emits canonical JSON to stdout and accepts no filesystem path"}
        ],
        "artifacts": artifacts,
        "release_inventory": release_inventory_value(),
        "sealing": {
            "canonicalization": "schema-versioned serde_json::to_vec(receipt)",
            "candidate": "canonical sorted length-delimited Git/index path and repository bytes with exact profile paths",
            "commit_message": COMMIT_MESSAGE_NAME,
            "completion_identity": "discovered after commit by unique subject and generated trailers",
            "callback": "canonical Git/GitHub-derived JSON on stdout only"
        },
        "parent_repository_digest": repository_digest_at_commit(root, EXPECTED_PARENT)?
    }))
}

fn project_amendments(snapshot: &Snapshot) -> TaskResult<Value> {
    let mut value = snapshot.value(AMENDMENTS_PATH)?;
    for amendment in value["amendments"]
        .as_array_mut()
        .ok_or("work-package amendments are missing")?
    {
        for artifact in amendment["repair_artifacts"]
            .as_array_mut()
            .ok_or("work-package amendment artifacts are missing")?
        {
            let relative = artifact["path"]
                .as_str()
                .ok_or("work-package amendment artifact path is missing")?;
            artifact["sha256"] = Value::String(format!(
                "sha256:{}",
                digest_bytes(snapshot.bytes(relative)?)
            ));
        }
    }
    Ok(value)
}

fn project_full_regression(snapshot: &Snapshot, candidate_digest: &str) -> TaskResult<Value> {
    let mut value = snapshot.value(FULL_REGRESSION_PATH)?;
    value["candidate"]["source_digest"] = Value::String(candidate_digest.into());
    Ok(value)
}

fn project_program_status(snapshot: &Snapshot, candidate_digest: &str) -> TaskResult<Value> {
    let mut value = snapshot.value(PROGRAM_STATUS_PATH)?;
    value["dirty_tree_identity"]["digest"] = Value::String(candidate_digest.into());
    let evidence_paths = value["evidence_paths"]
        .as_array_mut()
        .ok_or("program status evidence paths are missing")?;
    if !evidence_paths
        .iter()
        .any(|path| path.as_str() == Some(RECEIPT_PATH))
    {
        evidence_paths.push(Value::String(RECEIPT_PATH.into()));
    }
    value["next_smallest_executable_step"] = Value::String(
        "Commit the generated round-five repair message, push impl/0.2.0, await the exact terminal CI job set, then capture callback JSON stdout without changing repository evidence."
            .into(),
    );
    Ok(value)
}

fn project_workload_ledger(snapshot: &Snapshot) -> TaskResult<Value> {
    let mut value = snapshot.value(WORKLOAD_LEDGER_PATH)?;
    let exit_gates = value["exit_gates"]
        .as_array_mut()
        .ok_or("workload ledger exit gates are missing")?;
    if !exit_gates
        .iter()
        .any(|gate| gate["id"] == Value::String("content-evidence-sealing".into()))
    {
        exit_gates.push(
            json!({"id": "content-evidence-sealing", "state": "pass", "evidence": RECEIPT_PATH}),
        );
    }
    Ok(value)
}

pub(super) fn validate_profile(profile: &Value) -> TaskResult {
    let keys = profile
        .as_object()
        .ok_or("seal profile is not an object")?
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    require(
        keys == BTreeSet::from([
            "schema_version",
            "profile_id",
            "source_paths",
            "projection_paths",
            "retained_paths",
        ]) && profile["schema_version"] == Value::String(ROUND_FIVE_PROFILE.schema_version.into())
            && profile["profile_id"] == Value::String(ROUND_FIVE_PROFILE.profile_id.into())
            && string_array(&profile["source_paths"])? == ROUND_FIVE_PROFILE.source_paths
            && string_array(&profile["projection_paths"])? == ROUND_FIVE_PROFILE.projection_paths
            && string_array(&profile["retained_paths"])? == retained_paths(),
        "seal profile differs from the exact compiled content profile",
    )?;
    Ok(())
}

fn string_array(value: &Value) -> TaskResult<Vec<&str>> {
    value
        .as_array()
        .ok_or("profile path field is not an array")?
        .iter()
        .map(|value| {
            value
                .as_str()
                .ok_or_else(|| "profile path is not text".into())
        })
        .collect()
}

fn retained_paths() -> Vec<String> {
    let mut paths = vec![
        ".github/workflows/ci.yml".to_string(),
        ROUND_FOUR_INPUT_PATH.to_string(),
        ROUND_FOUR_RECEIPT_PATH.to_string(),
    ];
    for target in RELEASE_TARGETS {
        for document in RELEASE_DOCUMENTS {
            paths.push(format!("release/0.2.0/targets/{target}/{document}"));
        }
    }
    paths
}

fn release_inventory_value() -> Value {
    Value::Array(
        RELEASE_TARGETS
            .iter()
            .map(|target| json!({"target": target, "documents": RELEASE_DOCUMENTS}))
            .collect(),
    )
}

fn validate_release_inventory(snapshot: &Snapshot) -> TaskResult {
    let prefix = Path::new("release/0.2.0/targets");
    let actual = snapshot
        .files
        .keys()
        .filter(|path| path.starts_with(prefix))
        .map(|path| path.to_string_lossy().to_string())
        .collect::<BTreeSet<_>>();
    let expected = retained_paths()
        .into_iter()
        .filter(|path| path.starts_with("release/0.2.0/targets/"))
        .collect::<BTreeSet<_>>();
    require(
        actual == expected,
        "release inventory has missing, surplus, nested, alternate, or unsupported target paths",
    )
}

fn validate_callback_jobs(
    head: &str,
    event: &str,
    run_id: u64,
    jobs: &[Value],
) -> TaskResult<Vec<Value>> {
    require(
        jobs.len() == 2,
        "callback run must contain exactly two jobs",
    )?;
    let mut rows = Vec::new();
    for name in ["contract-msrv", "v0-foundation"] {
        let matching = jobs
            .iter()
            .filter(|job| job["name"].as_str() == Some(name))
            .collect::<Vec<_>>();
        require(
            matching.len() == 1,
            &format!("callback {event} run requires exactly one {name} job"),
        )?;
        let job = matching[0];
        require(
            job["status"] == Value::String("completed".into())
                && job["conclusion"] == Value::String("success".into())
                && job["run_id"].as_u64().is_none_or(|value| value == run_id)
                && job["head_sha"].as_str().is_none_or(|value| value == head),
            &format!("callback {event}/{name} job is stale or not terminal success"),
        )?;
        rows.push(json!({
            "id": job["id"],
            "name": name,
            "status": "completed",
            "conclusion": "success",
            "html_url": job["html_url"]
        }));
    }
    Ok(rows)
}

pub(super) fn validate_actions_source(root: &Path, source: &Value) -> TaskResult {
    require(
        source["schema_version"] == Value::String("mainframe-env.github-actions-receipt@1".into())
            && source["repository"] == Value::String("toreleon/mainframe-env".into())
            && source["workflow"] == json!({"id": 346290607, "name": "mainframe-env"})
            && source["provenance"]["method"] == Value::String("authenticated-gh-api".into())
            && source["provenance"]["cryptographic_signature"].is_null(),
        "GitHub Actions source identity or assurance is invalid",
    )?;
    let runs = source["runs"]
        .as_array()
        .ok_or("GitHub Actions source runs are missing")?;
    let expected = [
        (
            "failed-round-three-completion",
            "ab4894d6111910b08f37579134520a1de93a1e8a",
            "pull_request",
            "failure",
        ),
        (
            "failed-container-history-follow-up",
            "a3a27b46cad6e75b788fec2fa7c7e34f5d90e875",
            "pull_request",
            "failure",
        ),
        (
            "failed-native-evidence-bootstrap",
            "06a0490ecdf5dabb6f6ffffec7f189ce81a5f80e",
            "pull_request",
            "failure",
        ),
        ("accepted-push", ROUND_FOUR_PARENT, "push", "success"),
        (
            "accepted-pull-request",
            ROUND_FOUR_PARENT,
            "pull_request",
            "success",
        ),
    ];
    require(
        runs.len() == expected.len(),
        "GitHub Actions source run count is invalid",
    )?;
    let mut run_ids = BTreeSet::new();
    let mut job_ids = BTreeSet::new();
    for (run, (role, head, event, conclusion)) in runs.iter().zip(expected) {
        require(
            run["role"] == Value::String(role.into())
                && run["head_sha"] == Value::String(head.into())
                && run["head_branch"] == Value::String(EXPECTED_BRANCH.into())
                && run["event"] == Value::String(event.into())
                && run["status"] == Value::String("completed".into())
                && run["conclusion"] == Value::String(conclusion.into())
                && run_ids.insert(run["id"].as_u64().ok_or("source run ID is missing")?)
                && command_text(root, "git", &["rev-parse", head])? == head,
            &format!("GitHub Actions source run {role} is inconsistent"),
        )?;
        let jobs = run["jobs"].as_array().ok_or("source jobs are missing")?;
        require(jobs.len() == 2, "source run must contain exactly two jobs")?;
        for name in ["v0-foundation", "contract-msrv"] {
            let matching = jobs
                .iter()
                .filter(|job| job["name"].as_str() == Some(name))
                .collect::<Vec<_>>();
            require(
                matching.len() == 1,
                "source job name is missing or duplicated",
            )?;
            let job = matching[0];
            require(
                job["status"] == Value::String("completed".into())
                    && job_ids.insert(job["id"].as_u64().ok_or("source job ID is missing")?),
                "source job identity is not unique terminal data",
            )?;
            if name == "v0-foundation" {
                require(
                    job["conclusion"] == Value::String(conclusion.into()),
                    "source v0-foundation conclusion disagrees with its run",
                )?;
            } else {
                require(
                    job["conclusion"] == Value::String("success".into()),
                    "source contract-msrv job is not successful",
                )?;
            }
        }
    }
    Ok(())
}

fn validate_historical_round_four(root: &Path) -> TaskResult {
    require(
        command_text(root, "git", &["rev-parse", &format!("{EXPECTED_PARENT}^")])?
            == ROUND_FOUR_PARENT,
        "historical round-four completion parent drifted",
    )?;
    for (relative, expected_digest) in [
        (ROUND_FOUR_INPUT_PATH, ROUND_FOUR_INPUT_SHA256),
        (ROUND_FOUR_RECEIPT_PATH, ROUND_FOUR_RECEIPT_SHA256),
    ] {
        let object_bytes = git_file_bytes(root, EXPECTED_PARENT, relative)?;
        require(
            digest_bytes(&object_bytes) == expected_digest,
            &format!("historical round-four Git object drifted: {relative}"),
        )?;
        require(
            fs::read(root.join(relative)).map_err(|error| error.to_string())? == object_bytes,
            &format!("historical round-four path was rewritten: {relative}"),
        )?;
    }
    let receipt: Value = serde_json::from_slice(&git_file_bytes(
        root,
        EXPECTED_PARENT,
        ROUND_FOUR_RECEIPT_PATH,
    )?)
    .map_err(|error| error.to_string())?;
    let canonical = canonical_evidence_digest(
        receipt["receipt"]
            .as_object()
            .ok_or("historical round-four receipt object is missing")?,
    )?;
    require(
        canonical == ROUND_FOUR_EVIDENCE_DIGEST
            && receipt["evidence_digest"] == Value::String(canonical),
        "historical round-four evidence canonicalization drifted",
    )?;
    let message = command_text(root, "git", &["show", "-s", "--format=%B", EXPECTED_PARENT])?;
    require(
        message.lines().next() == Some("Repair 0.2.0 fourth review findings")
            && message
                .lines()
                .any(|line| line == "Fourth-Review-Findings: 3=closed")
            && message.lines().any(|line| line == "Target-Version: 0.2.0")
            && message
                .lines()
                .any(|line| line == format!("Evidence-Digest: {ROUND_FOUR_EVIDENCE_DIGEST}")),
        "historical round-four completion trailers drifted",
    )
}

fn compare_outputs(snapshot: &Snapshot, output: &SealOutput) -> TaskResult {
    compare_named_bytes(&output.files, &snapshot.files)
}

fn validate_precommit_context(root: &Path) -> TaskResult {
    require_clean_unstaged_and_untracked(root)?;
    validate_branch_parent(root, EXPECTED_BRANCH, EXPECTED_PARENT)?;
    validate_changed_scope(
        root,
        &["diff", "--cached", "--name-only", EXPECTED_PARENT],
        false,
    )
}

fn validate_committed_context(root: &Path, completion: &str) -> TaskResult {
    require_clean_worktree(root)?;
    require(
        command_text(root, "git", &["rev-parse", &format!("{completion}^")])? == EXPECTED_PARENT,
        "sealed round-five completion has the wrong parent",
    )?;
    validate_changed_scope(
        root,
        &["diff", "--name-only", EXPECTED_PARENT, completion],
        true,
    )
}

fn validate_changed_scope(
    root: &Path,
    arguments: &[&str],
    require_projections: bool,
) -> TaskResult {
    let paths = command_text(root, "git", arguments)?;
    let actual = paths.lines().map(str::to_string).collect::<BTreeSet<_>>();
    validate_candidate_path_set(&actual, require_projections)?;
    let retained = retained_paths();
    for relative in actual.iter().chain(retained.iter()) {
        let stage = command_text(root, "git", &["ls-files", "--stage", "--", relative])?;
        let mode = stage.split_whitespace().next().unwrap_or_default();
        validate_regular_git_mode(mode, relative)?;
    }
    Ok(())
}

fn validate_regular_git_mode(mode: &str, relative: &str) -> TaskResult {
    require(
        mode == "100644" || mode == "100755",
        &format!("sealed content path is missing, a symlink, or non-regular: {relative}"),
    )
}

fn validate_candidate_path_set(actual: &BTreeSet<String>, require_projections: bool) -> TaskResult {
    let source = ROUND_FIVE_PROFILE
        .source_paths
        .iter()
        .map(|path| (*path).to_string())
        .collect::<BTreeSet<_>>();
    let complete = source
        .iter()
        .cloned()
        .chain(
            ROUND_FIVE_PROFILE
                .projection_paths
                .iter()
                .map(|path| (*path).to_string()),
        )
        .collect::<BTreeSet<_>>();
    let expected = if require_projections {
        &complete
    } else {
        &source
    };
    require(
        actual == expected || (!require_projections && actual == &complete),
        &format!(
            "sealed candidate differs from the exact profile; missing={:?}; surplus={:?}",
            expected.difference(actual).collect::<Vec<_>>(),
            actual.difference(expected).collect::<Vec<_>>()
        ),
    )
}

fn require_clean_unstaged_and_untracked(root: &Path) -> TaskResult {
    require(
        command_text(root, "git", &["diff", "--name-only"])?.is_empty(),
        "evidence seal rejects unstaged or dirty-overlap inputs",
    )?;
    require(
        command_text(root, "git", &["ls-files", "--others", "--exclude-standard"])?.is_empty(),
        "evidence seal rejects untracked inputs",
    )
}

fn require_clean_worktree(root: &Path) -> TaskResult {
    require(
        command_text(root, "git", &["status", "--porcelain=v1"])?.is_empty(),
        "evidence operation requires a clean worktree",
    )
}

pub(super) fn round_four_completion_commits(
    root: &Path,
    digest: Option<&str>,
) -> TaskResult<Vec<String>> {
    completion_commits(
        root,
        "Repair 0.2.0 fourth review findings",
        "Fourth-Review-Findings: 3=closed",
        digest,
    )
}

pub(super) fn round_five_completion_commits(
    root: &Path,
    digest: Option<&str>,
) -> TaskResult<Vec<String>> {
    completion_commits(
        root,
        ROUND_FIVE_PROFILE.subject,
        ROUND_FIVE_PROFILE.finding_trailer,
        digest,
    )
}

fn completion_commits(
    root: &Path,
    subject: &str,
    finding_trailer: &str,
    digest: Option<&str>,
) -> TaskResult<Vec<String>> {
    let output = Command::new("git")
        .args(["log", "--format=%H%x1f%B%x1e", "HEAD"])
        .current_dir(root)
        .output()
        .map_err(|error| format!("git log: {error}"))?;
    require(output.status.success(), "git log failed")?;
    let history = String::from_utf8(output.stdout).map_err(|error| error.to_string())?;
    Ok(history
        .split('\u{1e}')
        .filter_map(|record| record.trim().split_once('\u{1f}'))
        .filter(|(_, message)| {
            message.lines().next() == Some(subject)
                && message
                    .lines()
                    .filter(|line| {
                        line.starts_with("Fourth-Review-Findings:")
                            || line.starts_with("Fifth-Review-Findings:")
                    })
                    .eq([finding_trailer])
                && message
                    .lines()
                    .filter(|line| line.starts_with("Target-Version:"))
                    .eq(["Target-Version: 0.2.0"])
                && digest.is_none_or(|digest| {
                    message
                        .lines()
                        .filter(|line| line.starts_with("Evidence-Digest:"))
                        .eq([format!("Evidence-Digest: {digest}").as_str()])
                })
        })
        .map(|(commit, _)| commit.trim().to_string())
        .collect())
}

fn reject_self_reference(value: &Value, completion: &str) -> TaskResult {
    fn contains(value: &Value, needle: &str) -> bool {
        match value {
            Value::String(text) => text == needle,
            Value::Array(values) => values.iter().any(|value| contains(value, needle)),
            Value::Object(values) => values.values().any(|value| contains(value, needle)),
            _ => false,
        }
    }
    require(
        !contains(value, completion),
        "sealed receipt contains its own completion commit hash",
    )
}

fn snapshot_digest(snapshot: &Snapshot) -> String {
    let mut digest = Sha256::new();
    for (relative, bytes) in &snapshot.files {
        if repository_digest_excluded(relative)
            || ROUND_FIVE_PROFILE
                .projection_paths
                .iter()
                .filter(|path| **path != AMENDMENTS_PATH)
                .any(|path| relative == Path::new(path))
        {
            continue;
        }
        let path = relative.to_string_lossy();
        digest.update((path.len() as u64).to_be_bytes());
        digest.update(path.as_bytes());
        digest.update((bytes.len() as u64).to_be_bytes());
        digest.update(bytes);
    }
    format!("sha256:{:x}", digest.finalize())
}

fn commit_message(evidence_digest: &str) -> Vec<u8> {
    format!(
        "{COMMIT_SUBJECT}\n\nFifth-Review-Findings: 4=closed\nTarget-Version: 0.2.0\nEvidence-Digest: {evidence_digest}\n"
    )
    .into_bytes()
}

fn commit_message_path(root: &Path) -> TaskResult<PathBuf> {
    let git_dir = PathBuf::from(command_text(root, "git", &["rev-parse", "--git-dir"])?);
    Ok(if git_dir.is_absolute() {
        git_dir.join(COMMIT_MESSAGE_NAME)
    } else {
        root.join(git_dir).join(COMMIT_MESSAGE_NAME)
    })
}

fn git_index_bytes(root: &Path, relative: &str) -> TaskResult<Vec<u8>> {
    validate_relative(relative)?;
    let output = Command::new("git")
        .args(["show", &format!(":{relative}")])
        .current_dir(root)
        .output()
        .map_err(|error| format!("git show :{relative}: {error}"))?;
    require(
        output.status.success(),
        &format!("Git index object is missing: {relative}"),
    )?;
    Ok(output.stdout)
}

fn validate_relative(relative: &str) -> TaskResult {
    let path = Path::new(relative);
    require(
        !path.is_absolute()
            && !relative.contains("//")
            && !relative.starts_with("./")
            && path
                .components()
                .all(|component| matches!(component, std::path::Component::Normal(_))),
        "sealed path is unsafe or non-canonical",
    )
}

fn digest_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn trim_final_newline(bytes: &[u8]) -> &[u8] {
    bytes.strip_suffix(b"\n").unwrap_or(bytes)
}

fn command_json(root: &Path, program: &str, arguments: &[&str]) -> TaskResult<Value> {
    let output = Command::new(program)
        .args(arguments)
        .current_dir(root)
        .output()
        .map_err(|error| format!("{program}: {error}"))?;
    require(output.status.success(), &format!("{program} failed"))?;
    serde_json::from_slice(&output.stdout).map_err(|error| format!("{program} JSON: {error}"))
}

fn callback_tip(root: &Path) -> TaskResult<String> {
    command_text(root, "git", &["rev-parse", "HEAD"])
}

fn validate_branch_parent(root: &Path, branch: &str, parent: &str) -> TaskResult {
    require(
        command_text(root, "git", &["branch", "--show-current"])? == branch,
        "evidence seal is on the wrong branch",
    )?;
    require(
        command_text(root, "git", &["rev-parse", "HEAD"])? == parent,
        "evidence seal has the wrong accepted parent",
    )
}

fn compare_named_bytes(
    expected: &BTreeMap<PathBuf, Vec<u8>>,
    actual: &BTreeMap<PathBuf, Vec<u8>>,
) -> TaskResult {
    for (relative, expected) in expected {
        let actual = actual
            .get(relative)
            .ok_or_else(|| format!("sealed projection is missing: {}", relative.display()))?;
        require(
            actual == expected,
            &format!("sealed projection is stale: {}", relative.display()),
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Map;

    fn exact_profile() -> Value {
        json!({
            "schema_version": ROUND_FIVE_PROFILE.schema_version,
            "profile_id": ROUND_FIVE_PROFILE.profile_id,
            "source_paths": ROUND_FIVE_PROFILE.source_paths,
            "projection_paths": ROUND_FIVE_PROFILE.projection_paths,
            "retained_paths": retained_paths()
        })
    }

    fn exact_source_paths() -> BTreeSet<String> {
        ROUND_FIVE_PROFILE
            .source_paths
            .iter()
            .map(|path| (*path).to_string())
            .collect()
    }

    fn release_snapshot() -> Snapshot {
        Snapshot {
            files: retained_paths()
                .into_iter()
                .filter(|path| path.starts_with("release/0.2.0/targets/"))
                .map(|path| (PathBuf::from(path), Vec::new()))
                .collect(),
        }
    }

    fn callback_jobs(head: &str, run_id: u64) -> Vec<Value> {
        ["contract-msrv", "v0-foundation"]
            .iter()
            .enumerate()
            .map(|(index, name)| {
                json!({
                    "id": run_id * 10 + index as u64,
                    "run_id": run_id,
                    "head_sha": head,
                    "name": name,
                    "status": "completed",
                    "conclusion": "success",
                    "html_url": format!("https://github.example/jobs/{run_id}/{index}")
                })
            })
            .collect()
    }

    #[test]
    fn profile_excludes_fabricated_commands_counts_toolchains_and_remote_identities() {
        let profile = exact_profile();
        assert!(validate_profile(&profile).is_ok());
        for (field, fabricated) in [
            ("commands", json!([{"command": "false", "exit_code": 0}])),
            ("workspace", json!({"tests_passed": 999999})),
            ("toolchains", json!({"rust": "forged"})),
            ("github_jobs", json!([{"id": 12345678901_u64}])),
            ("conclusion", json!("success")),
            ("future_ci_success", json!(true)),
        ] {
            let mut changed = profile.clone();
            changed[field] = fabricated;
            assert!(
                validate_profile(&changed).is_err(),
                "profile accepted forbidden field {field}"
            );
        }
        let receipt = Map::from_iter([
            ("candidate".into(), Value::String("sha256:fixture".into())),
            ("artifacts".into(), json!([])),
        ]);
        assert_eq!(
            canonical_evidence_digest(&receipt).unwrap(),
            canonical_evidence_digest(&receipt).unwrap()
        );
    }

    #[test]
    fn candidate_scope_rejects_extra_files_in_both_targets_missing_files_and_new_targets() {
        let exact = exact_source_paths();
        assert!(validate_candidate_path_set(&exact, false).is_ok());
        for extra in [
            "release/0.2.0/targets/aarch64-apple-darwin/unexpected.projection",
            "release/0.2.0/targets/x86_64-unknown-linux-gnu/unexpected.projection",
            "release/0.2.0/targets/powerpc64-unknown-linux-gnu/manifest.json",
            "release/0.2.0/targets/aarch64-apple-darwin/nested/manifest.json",
            "README.md",
        ] {
            let mut changed = exact.clone();
            changed.insert(extra.into());
            assert!(validate_candidate_path_set(&changed, false).is_err());
        }
        let mut missing = exact.clone();
        missing.remove("xtask/src/main.rs");
        assert!(validate_candidate_path_set(&missing, false).is_err());
    }

    #[test]
    fn release_inventory_is_exact_for_two_targets_and_six_documents() {
        let exact = release_snapshot();
        assert!(validate_release_inventory(&exact).is_ok());
        for extra in [
            "release/0.2.0/targets/aarch64-apple-darwin/unexpected.projection",
            "release/0.2.0/targets/x86_64-unknown-linux-gnu/unexpected.projection",
            "release/0.2.0/targets/powerpc64-unknown-linux-gnu/manifest.json",
            "release/0.2.0/targets/aarch64-apple-darwin/nested/manifest.json",
        ] {
            let mut changed = exact.clone();
            changed.files.insert(PathBuf::from(extra), Vec::new());
            assert!(validate_release_inventory(&changed).is_err());
        }
        let mut missing = exact.clone();
        missing.files.remove(Path::new(
            "release/0.2.0/targets/x86_64-unknown-linux-gnu/LICENSES.md",
        ));
        assert!(validate_release_inventory(&missing).is_err());
    }

    #[test]
    fn candidate_scope_rejects_symlink_and_non_regular_git_modes() {
        assert!(validate_regular_git_mode("100644", "regular").is_ok());
        assert!(validate_regular_git_mode("100755", "executable").is_ok());
        for mode in ["120000", "160000", "040000", ""] {
            assert!(validate_regular_git_mode(mode, "not-regular").is_err());
        }
    }

    #[test]
    fn callback_requires_exact_successful_current_tip_job_set() {
        let head = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let jobs = callback_jobs(head, 42);
        let rows = validate_callback_jobs(head, "push", 42, &jobs).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["name"], "contract-msrv");
        assert_eq!(rows[1]["name"], "v0-foundation");

        for (pointer, replacement) in [
            ("/0/name", json!("renamed")),
            ("/0/status", json!("queued")),
            ("/0/conclusion", json!("skipped")),
            ("/0/head_sha", json!("stale")),
            ("/0/run_id", json!(99_u64)),
            ("/1/conclusion", json!("failure")),
        ] {
            let mut changed = Value::Array(jobs.clone());
            *changed.pointer_mut(pointer).unwrap() = replacement;
            assert!(
                validate_callback_jobs(head, "pull_request", 42, changed.as_array().unwrap())
                    .is_err()
            );
        }

        let mut missing = jobs.clone();
        missing.pop();
        assert!(validate_callback_jobs(head, "push", 42, &missing).is_err());
        let mut duplicate = jobs.clone();
        duplicate[1] = duplicate[0].clone();
        assert!(validate_callback_jobs(head, "push", 42, &duplicate).is_err());
        let mut extra = jobs;
        extra.push(json!({
            "id": 999,
            "run_id": 42,
            "head_sha": head,
            "name": "unrelated-green",
            "status": "completed",
            "conclusion": "success"
        }));
        assert!(validate_callback_jobs(head, "push", 42, &extra).is_err());
    }

    #[test]
    fn callback_implementation_is_stdout_only_and_has_no_filesystem_mutation() {
        let source = include_str!("evidence_seal.rs");
        let callback = source
            .split_once("pub(super) fn callback(root: &Path) -> TaskResult {")
            .unwrap()
            .1
            .split_once("\nfn render(")
            .unwrap()
            .0;
        assert!(callback.contains("std::io::stdout()"));
        for forbidden in [
            "output_path",
            "fs::write",
            "fs::rename",
            "fs::create_dir",
            "OpenOptions",
        ] {
            assert!(
                !callback.contains(forbidden),
                "callback contains filesystem operation {forbidden}"
            );
        }
    }

    #[test]
    fn projections_and_commit_message_are_byte_deterministic() {
        let snapshot = Snapshot {
            files: BTreeMap::from([
                (PathBuf::from("source"), b"source".to_vec()),
                (PathBuf::from(RECEIPT_PATH), b"excluded".to_vec()),
            ]),
        };
        assert_eq!(snapshot_digest(&snapshot), snapshot_digest(&snapshot));
        let digest = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        assert_eq!(commit_message(digest), commit_message(digest));
        assert!(
            String::from_utf8(commit_message(digest))
                .unwrap()
                .contains("Fifth-Review-Findings: 4=closed")
        );
    }

    #[test]
    fn changed_projection_bytes_and_self_reference_are_rejected() {
        let expected = BTreeMap::from([
            (PathBuf::from("receipt"), b"a".to_vec()),
            (PathBuf::from("ledger"), b"b".to_vec()),
            (PathBuf::from("status"), b"c".to_vec()),
        ]);
        for changed in expected.keys() {
            let mut actual = expected.clone();
            actual.get_mut(changed).unwrap().push(b'!');
            assert!(compare_named_bytes(&expected, &actual).is_err());
        }
        let completion = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        assert!(reject_self_reference(&json!({"tip": completion}), completion).is_err());
        assert!(reject_self_reference(&json!({"parent": EXPECTED_PARENT}), completion).is_ok());
    }

    #[test]
    fn historical_round_three_source_remains_informational_and_immutable() {
        let root = repository_root().unwrap();
        let source_path = root.join("conformance/0.2/evidence/sources/round-3-ci-runs.json");
        let source = json(&source_path).unwrap();
        assert!(validate_actions_source(&root, &source).is_ok());
        let historical = git_file_bytes(
            &root,
            "ab4894d6111910b08f37579134520a1de93a1e8a",
            "conformance/0.2/evidence/review-repair-round-3.json",
        )
        .unwrap();
        assert_eq!(
            historical,
            fs::read(root.join("conformance/0.2/evidence/review-repair-round-3.json")).unwrap()
        );
    }
}
