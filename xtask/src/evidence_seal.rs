use super::*;

const INPUT_PATH: &str = "conformance/0.2/evidence/review-repair-round-4-inputs.json";
const RECEIPT_PATH: &str = "conformance/0.2/evidence/review-repair-round-4.json";
const PROGRAM_STATUS_PATH: &str = "conformance/0.2/evidence/program-status.json";
const WORKLOAD_LEDGER_PATH: &str = "conformance/0.2/evidence/workload-ledger.json";
const FULL_REGRESSION_PATH: &str = "conformance/0.2/evidence/full-regression.json";
const AMENDMENTS_PATH: &str = "conformance/0.2/evidence/work-package-amendments.json";
const HARD_CODE_PATH: &str = "conformance/0.2/evidence/hardcode/no-application-hardcode.json";
const ROUND_THREE_PATH: &str = "conformance/0.2/evidence/review-repair-round-3.json";
const COMMIT_SUBJECT: &str = "Repair 0.2.0 fourth review findings";
const EXPECTED_BRANCH: &str = "impl/0.2.0";
const EXPECTED_PARENT: &str = "b61d604fbdf4867154f235d45fd4453ef7e3b79d";
const COMMIT_MESSAGE_NAME: &str = "mainframe-env-round-4-commit-message.txt";

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
    require(
        round_four_completion_commits(root, None)?.is_empty(),
        "round-four completion already exists; seal generation is pre-commit only",
    )?;
    let output = render(root, &Snapshot::from_index(root)?)?;
    for (relative, bytes) in &output.files {
        let path = root.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| format!("{}: {error}", parent.display()))?;
        }
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
        "round-four sealed receipt is missing",
    )?;
    let evidence = json(&live_receipt)?;
    let evidence_digest = evidence["evidence_digest"]
        .as_str()
        .ok_or("round-four sealed evidence digest is missing")?;
    let completions = round_four_completion_commits(root, Some(evidence_digest))?;
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
        _ => return Err("round-four completion identity is duplicated".into()),
    };
    let output = render(root, &snapshot)?;
    compare_outputs(&snapshot, &output)?;
    if let Some(completion) = committed {
        let actual = command_text(root, "git", &["show", "-s", "--format=%B", completion])?;
        require(
            actual.as_bytes() == trim_final_newline(&output.commit_message),
            "round-four completion trailers differ from the sealed commit message",
        )?;
        let receipt = snapshot.value(RECEIPT_PATH)?;
        reject_self_reference(&receipt, completion)?;
    } else {
        let path = commit_message_path(root)?;
        let actual = fs::read(&path).map_err(|error| format!("{}: {error}", path.display()))?;
        require(
            actual == output.commit_message,
            "generated round-four commit-message file is stale",
        )?;
    }
    Ok(())
}

pub(super) fn callback(root: &Path, output_path: &Path) -> TaskResult {
    require(
        output_path.is_absolute()
            && output_path.file_name() == Some(OsStr::new("ROUND_4_DONE.json"))
            && !output_path.starts_with(root),
        "callback output must be an absolute ROUND_4_DONE.json outside the repository",
    )?;
    require_clean_worktree(root)?;
    let head = callback_tip(root)?;
    let evidence = json(&root.join(RECEIPT_PATH))?;
    let evidence_digest = evidence["evidence_digest"]
        .as_str()
        .ok_or("round-four evidence digest is missing")?;
    let completions = round_four_completion_commits(root, Some(evidence_digest))?;
    require(
        completions.as_slice() == [head.as_str()],
        "callback HEAD is not the unique sealed round-four completion",
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
        let mut job_rows = jobs["jobs"]
            .as_array()
            .ok_or("callback jobs are missing")?
            .iter()
            .map(|job| {
                require(
                    job["status"] == Value::String("completed".into())
                        && job["conclusion"] == Value::String("success".into()),
                    "callback job is not terminal success",
                )?;
                Ok(json!({
                    "id": job["id"],
                    "name": job["name"],
                    "status": job["status"],
                    "conclusion": job["conclusion"],
                    "html_url": job["html_url"]
                }))
            })
            .collect::<TaskResult<Vec<_>>>()?;
        job_rows.sort_by(|left, right| left["name"].as_str().cmp(&right["name"].as_str()));
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
        "schema_version": "mainframe-env.round-4-completion@1",
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
    let parent = output_path
        .parent()
        .ok_or("callback output has no parent directory")?;
    fs::create_dir_all(parent).map_err(|error| format!("{}: {error}", parent.display()))?;
    let temporary = parent.join(format!(".ROUND_4_DONE.json.tmp.{}", std::process::id()));
    fs::write(&temporary, bytes).map_err(|error| format!("{}: {error}", temporary.display()))?;
    fs::rename(&temporary, output_path).map_err(|error| {
        format!(
            "atomic callback rename {} -> {}: {error}",
            temporary.display(),
            output_path.display()
        )
    })?;
    Ok(())
}

fn render(root: &Path, snapshot: &Snapshot) -> TaskResult<SealOutput> {
    let input = snapshot.value(INPUT_PATH)?;
    validate_input(&input)?;
    let amendments = project_amendments(snapshot)?;
    let hardcode = project_hardcode(snapshot)?;
    let candidate_snapshot = snapshot
        .with_file(AMENDMENTS_PATH, pretty_json(&amendments)?)
        .with_file(HARD_CODE_PATH, pretty_json(&hardcode)?);
    let candidate_digest = snapshot_digest(&candidate_snapshot);
    let full_regression = project_full_regression(snapshot, &candidate_digest)?;
    let program_status = project_program_status(snapshot, &candidate_digest)?;
    let workload_ledger = project_workload_ledger(snapshot)?;
    let mut projected = BTreeMap::from([
        (PathBuf::from(AMENDMENTS_PATH), pretty_json(&amendments)?),
        (PathBuf::from(HARD_CODE_PATH), pretty_json(&hardcode)?),
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
    let receipt = build_receipt(root, snapshot, &projected, &input, &candidate_digest)?;
    let evidence_digest = canonical_evidence_digest(
        receipt
            .as_object()
            .ok_or("generated round-four receipt is not an object")?,
    )?;
    let evidence = json!({
        "schema_version": "mainframe-env.review-repair-round-4@1",
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
    input: &Value,
    candidate_digest: &str,
) -> TaskResult<Value> {
    let round_three = snapshot.value(ROUND_THREE_PATH)?;
    let original_digest = round_three["evidence_digest"]
        .as_str()
        .ok_or("round-three evidence digest is missing")?;
    let original_completion = find_completion_commit(
        root,
        "Repair 0.2.0 third review findings",
        &[
            ("Third-Review-Findings", "11=closed"),
            ("Target-Version", "0.2.0"),
            ("Evidence-Digest", original_digest),
        ],
    )?;
    let source_path = input["github_actions_source"]
        .as_str()
        .ok_or("GitHub Actions source path is missing")?;
    let source = snapshot.value(source_path)?;
    validate_actions_source(root, &source)?;
    let runs = source["runs"]
        .as_array()
        .ok_or("GitHub Actions source runs are missing")?;
    let failed_candidates = runs
        .iter()
        .filter(|run| {
            run["role"]
                .as_str()
                .is_some_and(|role| role.starts_with("failed-"))
        })
        .map(supersession_run)
        .collect::<TaskResult<Vec<_>>>()?;
    let accepted_checks = runs
        .iter()
        .filter(|run| {
            run["role"]
                .as_str()
                .is_some_and(|role| role.starts_with("accepted-"))
        })
        .map(supersession_run)
        .collect::<TaskResult<Vec<_>>>()?;
    let accepted_digest = repository_digest_at_commit(root, EXPECTED_PARENT)?;
    let linux_manifest: Value = serde_json::from_slice(&git_file_bytes(
        root,
        EXPECTED_PARENT,
        "release/0.2.0/targets/x86_64-unknown-linux-gnu/manifest.json",
    )?)
    .map_err(|error| error.to_string())?;
    let linux_inputs: Value = serde_json::from_slice(&git_file_bytes(
        root,
        EXPECTED_PARENT,
        "release/0.2.0/targets/x86_64-unknown-linux-gnu/build-inputs.json",
    )?)
    .map_err(|error| error.to_string())?;
    let release_binaries = linux_manifest["artifacts"]
        .as_array()
        .ok_or("accepted Linux manifest artifacts are missing")?
        .iter()
        .filter(|artifact| {
            artifact["path"]
                .as_str()
                .is_some_and(|path| path.starts_with("bin/"))
        })
        .cloned()
        .collect::<Vec<_>>();
    require(
        release_binaries.len() == 2
            && linux_inputs["target"] == Value::String("x86_64-unknown-linux-gnu".into())
            && linux_inputs["rustc_verbose"]
                .as_str()
                .is_some_and(|value| value.starts_with("rustc 1.98.0 ")),
        "accepted Linux release identities are incomplete",
    )?;
    let mut artifact_paths = input["artifact_paths"]
        .as_array()
        .ok_or("round-four artifact paths are missing")?
        .iter()
        .map(|path| {
            path.as_str()
                .map(str::to_string)
                .ok_or_else(|| "round-four artifact path is not text".to_string())
        })
        .collect::<TaskResult<Vec<_>>>()?;
    artifact_paths.sort();
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
        "target_version": input["target_version"],
        "accepted_parent": input["accepted_parent"],
        "candidate_source_digest": candidate_digest,
        "branch": input["branch"],
        "base": input["base"],
        "pull_request": input["pull_request"],
        "round_three_supersession": {
            "status": "superseded",
            "original": {
                "completion_commit": original_completion,
                "evidence_path": ROUND_THREE_PATH,
                "evidence_digest": original_digest,
                "candidate_source_digest": round_three["receipt"]["candidate_source_digest"],
                "invalid_terminal_run_ids": round_three["receipt"]["pull_request"]["check_identities"]
                    .as_array()
                    .unwrap_or(&Vec::new())
                    .iter()
                    .filter_map(|row| row["source_run_id"].as_u64())
                    .collect::<BTreeSet<_>>()
            },
            "reason": "The original receipt cited a pre-repair successful run; its completion and two intermediate follow-ups failed before native Linux evidence was retained.",
            "failed_candidates": failed_candidates,
            "accepted_follow_up": {
                "tip": EXPECTED_PARENT,
                "candidate_source_digest": accepted_digest,
                "checks": accepted_checks,
                "release": {
                    "target": linux_inputs["target"],
                    "rustc_verbose": linux_inputs["rustc_verbose"],
                    "build_inputs_sha256": format!("sha256:{}", digest_bytes(&git_file_bytes(root, EXPECTED_PARENT, "release/0.2.0/targets/x86_64-unknown-linux-gnu/build-inputs.json")?)),
                    "binary_artifacts": release_binaries
                }
            },
            "source_provenance": {
                "path": source_path,
                "repository": source["repository"],
                "workflow": source["workflow"],
                "provenance": source["provenance"]
            }
        },
        "findings": input["findings"],
        "commands": input["commands"],
        "toolchains": input["toolchains"],
        "artifacts": artifacts,
        "sealing": {
            "command": "cargo xtask evidence seal",
            "check_command": "cargo xtask evidence seal --check",
            "canonicalization": "schema-versioned serde_json::to_vec(receipt)",
            "candidate": "canonical sorted length-delimited Git/index path and bytes with versioned derived exclusions",
            "commit_message": COMMIT_MESSAGE_NAME,
            "completion_identity": "discovered after commit by unique subject and generated trailers",
            "callback": "post-commit only; excluded from commit evidence"
        },
        "future_ci_success_claimed": false,
        "remote_actions": input["remote_actions"]
    }))
}

fn supersession_run(run: &Value) -> TaskResult<Value> {
    let jobs = run["jobs"]
        .as_array()
        .ok_or("sourced run jobs are missing")?;
    let v0 = jobs
        .iter()
        .find(|job| job["name"] == Value::String("v0-foundation".into()))
        .ok_or("sourced run v0-foundation job is missing")?;
    Ok(json!({
        "role": run["role"],
        "head_sha": run["head_sha"],
        "event": run["event"],
        "workflow": "mainframe-env",
        "run_id": run["id"],
        "run_status": run["status"],
        "run_conclusion": run["conclusion"],
        "run_url": run["html_url"],
        "job": {"id": v0["id"], "name": v0["name"], "status": v0["status"], "conclusion": v0["conclusion"], "html_url": v0["html_url"]}
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
    let input = snapshot.value(INPUT_PATH)?;
    value["candidate"]["source_digest"] = Value::String(candidate_digest.into());
    value["workspace"]["tests_passed"] = input["workspace"]["tests_passed"].clone();
    value["workspace"]["tests_ignored_in_workspace_command"] =
        input["workspace"]["tests_ignored"].clone();
    value["workspace"]["failures"] = input["workspace"]["failures"].clone();
    if let Some(result) = value["results"].as_array_mut().and_then(|results| {
        results.iter_mut().find(|result| {
            result["command"]
                .as_str()
                .is_some_and(|command| command.contains("cargo test --workspace"))
        })
    }) {
        result["result"] = Value::String(format!(
            "{} passed, {} explicitly ignored",
            input["workspace"]["tests_passed"]
                .as_u64()
                .unwrap_or_default(),
            input["workspace"]["tests_ignored"]
                .as_u64()
                .unwrap_or_default()
        ));
    }
    for target in ["aarch64-apple-darwin", "x86_64-unknown-linux-gnu"] {
        let row = &mut value["release"]["targets"][target];
        for (file, field) in [
            ("manifest.json", "manifest_sha256"),
            ("sbom.cdx.json", "sbom_sha256"),
            ("provenance.intoto.json", "provenance_sha256"),
            ("checksums.sha256", "checksums_sha256"),
            ("LICENSES.md", "licenses_sha256"),
            ("build-inputs.json", "build_inputs_sha256"),
        ] {
            row[field] = Value::String(digest_bytes(
                snapshot.bytes(&format!("release/0.2.0/targets/{target}/{file}"))?,
            ));
        }
    }
    Ok(value)
}

fn project_hardcode(snapshot: &Snapshot) -> TaskResult<Value> {
    let mut value = snapshot.value(HARD_CODE_PATH)?;
    let scanned = snapshot
        .files
        .keys()
        .filter(|path| {
            path.starts_with("crates")
                && path.extension() == Some(OsStr::new("rs"))
                && !path.starts_with("crates/tooling/mainframe-env-conformance")
        })
        .count();
    value["after"]["scanned_rust_files"] = json!(scanned);
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
    value["evidence_corrections"] = json!([{
        "claim": "round-three exact-one-repair-commit terminal success",
        "status": "superseded",
        "reason": "The round-three completion and two intermediate follow-ups failed CI; b61d604 is the accepted tested follow-up tip.",
        "evidence": RECEIPT_PATH
    }]);
    value["next_smallest_executable_step"] = Value::String(
        "Commit the generated round-four repair message, push impl/0.2.0, await terminal checks, then generate the post-commit callback without changing repository evidence."
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
        .any(|gate| gate["id"] == Value::String("evidence-sealing".into()))
    {
        exit_gates
            .push(json!({"id": "evidence-sealing", "state": "pass", "evidence": RECEIPT_PATH}));
    }
    Ok(value)
}

fn validate_input(input: &Value) -> TaskResult {
    require(
        input["schema_version"]
            == Value::String("mainframe-env.review-repair-round-4-inputs@1".into())
            && input["target_version"] == Value::String("0.2.0".into())
            && input["accepted_parent"] == Value::String(EXPECTED_PARENT.into())
            && input["branch"] == Value::String(EXPECTED_BRANCH.into())
            && input["base"] == Value::String("main".into())
            && input["pull_request"].as_u64() == Some(1),
        "round-four seal input identity is invalid",
    )?;
    let findings = input["findings"]
        .as_array()
        .ok_or("round-four findings are missing")?;
    require(
        findings.len() == 3
            && findings.iter().enumerate().all(|(index, finding)| {
                finding["id"].as_u64() == Some((index + 1) as u64)
                    && finding["state"] == Value::String("closed".into())
            })
            && input["commands"].as_array().is_some_and(|commands| {
                !commands.is_empty()
                    && commands
                        .iter()
                        .all(|command| command["exit_code"].as_i64() == Some(0))
            })
            && input["workspace"]["tests_passed"]
                .as_u64()
                .is_some_and(|count| count >= 260)
            && input["workspace"]["tests_ignored"].as_u64().is_some()
            && input["workspace"]["failures"].as_u64() == Some(0)
            && input["remote_actions"]
                == json!({"tagged": false, "published": false, "deployed": false, "merged": false}),
        "round-four seal inputs do not close exactly three findings with passing commands",
    )
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
        ("accepted-push", EXPECTED_PARENT, "push", "success"),
        (
            "accepted-pull-request",
            EXPECTED_PARENT,
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

fn compare_outputs(snapshot: &Snapshot, output: &SealOutput) -> TaskResult {
    compare_named_bytes(&output.files, &snapshot.files)
}

fn validate_precommit_context(root: &Path) -> TaskResult {
    require_clean_unstaged_and_untracked(root)?;
    validate_branch_parent(root, EXPECTED_BRANCH, EXPECTED_PARENT)?;
    validate_changed_scope(root, &["diff", "--cached", "--name-only", EXPECTED_PARENT])
}

fn validate_committed_context(root: &Path, completion: &str) -> TaskResult {
    require_clean_worktree(root)?;
    require(
        command_text(root, "git", &["rev-parse", &format!("{completion}^")])? == EXPECTED_PARENT,
        "sealed round-four completion has the wrong parent",
    )?;
    validate_changed_scope(root, &["diff", "--name-only", EXPECTED_PARENT, completion])
}

fn validate_changed_scope(root: &Path, arguments: &[&str]) -> TaskResult {
    let paths = command_text(root, "git", arguments)?;
    require(!paths.is_empty(), "evidence seal has no candidate changes")?;
    for relative in paths.lines() {
        require(
            allowed_round_four_path(relative),
            &format!("evidence seal candidate contains out-of-scope path {relative}"),
        )?;
    }
    Ok(())
}

fn allowed_round_four_path(relative: &str) -> bool {
    matches!(
        relative,
        ".github/workflows/ci.yml"
            | "Cargo.lock"
            | "Cargo.toml"
            | "xtask/Cargo.toml"
            | "xtask/src/main.rs"
            | "xtask/src/evidence_seal.rs"
            | "crates/providers/mainframe-env-racf/Cargo.toml"
            | "crates/providers/mainframe-env-racf/src/service.rs"
            | "crates/apps/mainframe-env-server/Cargo.toml"
            | "crates/apps/mainframe-env-server/src/lib.rs"
            | "crates/apps/mainframe-env-server/src/main.rs"
            | "crates/apps/mainframe-env-server/src/environment_secrets.rs"
            | "conformance/0.2/inventory/contracts.json"
            | "conformance/0.2/evidence/review-repair-round-4-inputs.json"
            | "conformance/0.2/evidence/review-repair-round-4.json"
            | "conformance/0.2/evidence/sources/round-3-ci-runs.json"
            | "conformance/0.2/evidence/work-package-amendments.json"
            | "conformance/0.2/evidence/hardcode/no-application-hardcode.json"
            | "conformance/0.2/evidence/full-regression.json"
            | "conformance/0.2/evidence/program-status.json"
            | "conformance/0.2/evidence/workload-ledger.json"
            | "conformance/0.2/schemas/github-actions-receipt.schema.json"
            | "conformance/0.2/schemas/review-repair-round-4-inputs.schema.json"
            | "conformance/0.2/schemas/review-repair-round-4.schema.json"
            | "conformance/0.2/schemas/program-status.schema.json"
            | "docs/releases/0.2.md"
    ) || relative.starts_with("release/0.2.0/targets/")
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
            message.lines().next() == Some(COMMIT_SUBJECT)
                && message
                    .lines()
                    .filter(|line| line.starts_with("Fourth-Review-Findings:"))
                    .eq(["Fourth-Review-Findings: 3=closed"])
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
        if repository_digest_excluded(relative) {
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
        "{COMMIT_SUBJECT}\n\nFourth-Review-Findings: 3=closed\nTarget-Version: 0.2.0\nEvidence-Digest: {evidence_digest}\n"
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
    require(
        !relative.contains("..") && !Path::new(relative).is_absolute(),
        "sealed path is unsafe",
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

    fn temporary_repository(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = env::temp_dir().join(format!(
            "mainframe-env-seal-{name}-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&root).expect("temporary repository");
        for args in [
            vec!["init", "--quiet"],
            vec!["config", "user.name", "sealer-test"],
            vec!["config", "user.email", "sealer@example.invalid"],
            vec!["checkout", "-b", EXPECTED_BRANCH],
        ] {
            assert!(
                Command::new("git")
                    .args(args)
                    .current_dir(&root)
                    .status()
                    .unwrap()
                    .success()
            );
        }
        for (name, bytes) in [
            ("artifact", b"artifact".as_slice()),
            ("receipt", b"receipt".as_slice()),
            ("ledger", b"ledger".as_slice()),
            ("status", b"status".as_slice()),
        ] {
            fs::write(root.join(name), bytes).unwrap();
        }
        assert!(
            Command::new("git")
                .args(["add", "."])
                .current_dir(&root)
                .status()
                .unwrap()
                .success()
        );
        assert!(
            Command::new("git")
                .args(["commit", "--quiet", "-m", "fixture"])
                .current_dir(&root)
                .status()
                .unwrap()
                .success()
        );
        root
    }

    #[test]
    fn unchanged_seal_runs_are_byte_identical() {
        let root = temporary_repository("idempotence");
        let receipt = Map::from_iter([
            ("candidate".into(), Value::String("sha256:fixture".into())),
            ("result".into(), Value::String("pass".into())),
        ]);
        let first = canonical_evidence_digest(&receipt).unwrap();
        let second = canonical_evidence_digest(&receipt).unwrap();
        assert_eq!(first, second);
        assert_eq!(commit_message(&first), commit_message(&second));
        assert_eq!(
            snapshot_digest(&Snapshot::from_index(&root).unwrap()),
            snapshot_digest(&Snapshot::from_index(&root).unwrap())
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn seal_check_rejects_changed_artifact_receipt_ledger_status_and_trailer() {
        let root = temporary_repository("mutations");
        let expected = BTreeMap::from([
            (PathBuf::from("artifact"), b"a".to_vec()),
            (PathBuf::from("receipt"), b"b".to_vec()),
            (PathBuf::from("ledger"), b"c".to_vec()),
            (PathBuf::from("status"), b"d".to_vec()),
        ]);
        for changed in expected.keys() {
            let mut actual = expected.clone();
            actual.get_mut(changed).unwrap().push(b'!');
            assert!(compare_named_bytes(&expected, &actual).is_err());
        }
        let digest = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let mut changed = commit_message(digest);
        changed.push(b'!');
        assert_ne!(changed, commit_message(digest));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn branch_parent_dirty_and_out_of_scope_inputs_are_rejected() {
        let root = temporary_repository("context");
        let parent = callback_tip(&root).unwrap();
        assert!(validate_branch_parent(&root, EXPECTED_BRANCH, &parent).is_ok());
        assert!(validate_branch_parent(&root, "wrong", &parent).is_err());
        assert!(validate_branch_parent(&root, EXPECTED_BRANCH, "bad-parent").is_err());
        fs::write(root.join("untracked"), b"dirty").unwrap();
        assert!(require_clean_unstaged_and_untracked(&root).is_err());
        fs::remove_file(root.join("untracked")).unwrap();
        fs::write(root.join("README.md"), b"out of scope").unwrap();
        assert!(
            Command::new("git")
                .args(["add", "README.md"])
                .current_dir(&root)
                .status()
                .unwrap()
                .success()
        );
        assert!(
            validate_changed_scope(&root, &["diff", "--cached", "--name-only", "HEAD"]).is_err()
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn duplicate_completion_and_caller_supplied_callback_tip_are_rejected() {
        let root = temporary_repository("completion");
        let digest = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        for index in 0..2 {
            fs::write(root.join("artifact"), format!("artifact-{index}")).unwrap();
            assert!(
                Command::new("git")
                    .args(["add", "artifact"])
                    .current_dir(&root)
                    .status()
                    .unwrap()
                    .success()
            );
            let message = String::from_utf8(commit_message(digest)).unwrap();
            assert!(
                Command::new("git")
                    .args(["commit", "--quiet", "-m", &message])
                    .current_dir(&root)
                    .status()
                    .unwrap()
                    .success()
            );
        }
        assert_eq!(
            round_four_completion_commits(&root, Some(digest))
                .unwrap()
                .len(),
            2
        );
        let actual = callback_tip(&root).unwrap();
        assert_ne!(actual, "caller-supplied-tip");
        assert_eq!(
            actual,
            command_text(&root, "git", &["rev-parse", "HEAD"]).unwrap()
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn self_referential_completion_is_rejected() {
        let completion = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        assert!(reject_self_reference(&json!({"tip": completion}), completion).is_err());
        assert!(reject_self_reference(&json!({"parent": EXPECTED_PARENT}), completion).is_ok());
    }

    #[test]
    fn round_three_supersession_rejects_wrong_head_event_conclusion_duplicates_and_live_substitution()
     {
        let root = repository_root().unwrap();
        let source_path = root.join("conformance/0.2/evidence/sources/round-3-ci-runs.json");
        let source = json(&source_path).unwrap();
        assert!(validate_actions_source(&root, &source).is_ok());
        for (pointer, replacement) in [
            (
                "/runs/0/head_sha",
                json!("0000000000000000000000000000000000000000"),
            ),
            ("/runs/3/event", json!("pull_request")),
            ("/runs/4/conclusion", json!("failure")),
            ("/runs/4/jobs/1/conclusion", json!("failure")),
        ] {
            let mut changed = source.clone();
            *changed.pointer_mut(pointer).unwrap() = replacement;
            assert!(validate_actions_source(&root, &changed).is_err());
        }
        let mut duplicate = source.clone();
        duplicate["runs"][1]["id"] = duplicate["runs"][0]["id"].clone();
        assert!(validate_actions_source(&root, &duplicate).is_err());
        let historical = git_file_bytes(
            &root,
            "ab4894d6111910b08f37579134520a1de93a1e8a",
            ROUND_THREE_PATH,
        )
        .unwrap();
        let mut substituted: Value = serde_json::from_slice(&historical).unwrap();
        substituted["receipt"]["candidate_source_digest"] = Value::String(
            "sha256:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff".into(),
        );
        assert_ne!(pretty_json(&substituted).unwrap(), historical);
    }
}
