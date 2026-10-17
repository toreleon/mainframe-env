//! Real focused CLI boundaries; finite existing local bindings only.

use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

const VERDICT: &str = "mainframe-env.conformance-verdict@1";
const LEDGER: &str = "mainframe-env.conformance-ledger@1";
const COBOL: &str = "cobol.frontend.basis.valid";
const JCL: &str = "jcl.dd.parameters-0001.recognized";
static SEQUENCE: AtomicU64 = AtomicU64::new(0);

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

struct Receipts(PathBuf);

impl Receipts {
    fn new(name: &str) -> Self {
        let parent = std::env::var_os("CV209_CLI_RECEIPTS")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        fs::create_dir_all(&parent).unwrap();
        let directory = parent.join(format!(
            "cv209-{name}-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed),
        ));
        fs::create_dir(&directory).unwrap();
        eprintln!("CV-209 real CLI receipts: {}", directory.display());
        Self(directory)
    }

    fn save(&self, name: &str, command: &Command, output: &Output) {
        fs::write(
            self.0.join(format!("{name}.command")),
            format!("{command:?}\n"),
        )
        .unwrap();
        fs::write(self.0.join(format!("{name}.stdout")), &output.stdout).unwrap();
        fs::write(self.0.join(format!("{name}.stderr")), &output.stderr).unwrap();
        fs::write(
            self.0.join(format!("{name}.exit")),
            format!("{}\n", output.status),
        )
        .unwrap();
    }

    fn run(&self, name: &str, args: &[&str], destination: Option<&Path>) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_xtask"));
        command.current_dir(root()).arg("conformance").args(args);
        if let Some(destination) = destination {
            command.arg("--output").arg(destination);
        }
        let output = command.output().unwrap();
        self.save(name, &command, &output);
        output
    }
}

fn success(output: &Output) {
    assert!(
        output.status.success(),
        "exit={} stderr={}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("conformance: pass"));
    assert!(!stderr.contains(VERDICT));
    assert!(!stderr.contains(LEDGER));
}

fn report(
    bytes: &[u8],
    test: &str,
    row: &str,
    obligation: &str,
    gate: &str,
    event_count: usize,
) -> Vec<Value> {
    assert!(bytes.ends_with(b"\n"));
    let records: Vec<Value> = bytes
        .strip_suffix(b"\n")
        .unwrap()
        .split(|byte| *byte == b'\n')
        .map(|line| {
            serde_json::from_slice(line)
                .expect("only canonical JSON records belong in the machine stream")
        })
        .collect();
    assert_eq!(records.len(), event_count + 1);
    let ledger = records.last().unwrap();
    assert_eq!(ledger["schema_version"], LEDGER);
    assert_eq!(ledger["rows"].as_array().unwrap().len(), 1506);
    let counts = ledger["counts"].as_array().unwrap();
    assert_eq!(counts.len(), 6);
    let names: std::collections::BTreeSet<_> = counts
        .iter()
        .map(|count| count["gate"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        [
            "recognized",
            "validated",
            "executed",
            "conditioned",
            "recovered",
            "differential"
        ]
        .into_iter()
        .collect()
    );
    for count in counts {
        let denominator: u64 = ["pass", "fail", "pending", "non_applicable"]
            .iter()
            .map(|key| count[key].as_u64().unwrap())
            .sum();
        assert_eq!(denominator, 1506);
    }
    for event in &records[..event_count] {
        assert_eq!(event["schema_version"], VERDICT);
        assert_eq!(event["verdict"], "pass");
        assert_eq!(event["catalog_digest"], ledger["catalog_digest"]);
        assert_eq!(event["spec_digest"], ledger["spec_digest"]);
        assert_eq!(event["spec_version"], ledger["spec_version"]);
        assert!(event["oracle_receipt_digest"].is_null());
        assert!(!event["source_locator"].as_str().unwrap().is_empty());
        assert_eq!(
            event["replay"],
            format!(
                "cargo xtask conformance --replay {}",
                event["test_id"].as_str().unwrap()
            )
        );
    }
    let event = records[..event_count]
        .iter()
        .find(|event| event["test_id"] == test)
        .unwrap();
    assert_eq!(event["row_id"], row);
    assert_eq!(event["obligation_id"], obligation);
    assert_eq!(event["gate"], gate);
    records
}

#[test]
fn dataset_retains_selected_event_and_original_ledger() {
    let receipts = Receipts::new("dataset");
    let output = receipts.run(
        "replay",
        &["--replay", "dataset.esds.recognized", "--check"],
        None,
    );
    success(&output);
    report(
        &output.stdout,
        "dataset.esds.recognized",
        "ibm-zos-3.2-dfsms-ams-2026-06:vsam-primary-organizations:0001",
        "valid-access",
        "recognized",
        1,
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("reference-organizations=5 reference-commands=31 reference-properties=13 observation-perturbations-rejected=8 differential-credit=0"));
}

#[test]
fn racf_retains_selected_event_and_original_ledger() {
    let receipts = Receipts::new("racf");
    let output = receipts.run(
        "replay",
        &["--replay", "racf.command.0001.syntax.recognized", "--check"],
        None,
    );
    success(&output);
    report(
        &output.stdout,
        "racf.command.0001.syntax.recognized",
        "ibm-zos-3.2-racf-saf-2026:racf-command-families:0001",
        "syntax",
        "recognized",
        1,
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("bindings=1 recognized=1/0/1505/0"));
}

#[test]
fn cobol_stdout_contains_only_canonical_events_and_ledger() {
    let receipts = Receipts::new("cobol");
    let output = receipts.run("replay", &["--replay", COBOL, "--check"], None);
    success(&output);
    report(
        &output.stdout,
        COBOL,
        "ibm-enterprise-cobol-6.5-2026-05-31:compiler-directing-statements:0001",
        "valid-forms",
        "recognized",
        1,
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("batches=1 verdicts=1 pass=1 fail=0"));
}

#[test]
fn mq_stdout_keeps_partial_ledger_and_pending_full26() {
    let receipts = Receipts::new("mq");
    let output = receipts.run(
        "replay",
        &["--replay", "mq.mqconn.memory.executed", "--check"],
        None,
    );
    success(&output);
    let records = report(
        &output.stdout,
        "mq.mqconn.memory.executed",
        "ibm-mq-9.4-mqi-2026-08-31:mqi-calls-unique:0008",
        "selected-local-v1-memory",
        "executed",
        1,
    );
    let ledger = records.last().unwrap();
    assert_eq!(
        ledger["rows"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|row| row["subsystem"] == "mq")
            .count(),
        26
    );
    for count in ledger["counts"].as_array().unwrap() {
        assert_eq!(count["pass"], 0);
        assert_eq!(count["fail"], 0);
        assert_eq!(count["pending"], 1506);
    }
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("full26 rows pending; no installed/native/licensed credit")
    );
}

#[test]
fn cics_accepted_scenario_keeps_all_thirty_existing_credits() {
    let receipts = Receipts::new("cics");
    let output = receipts.run(
        "replay",
        &["--replay", "cics.pilot.read-update.executed", "--check"],
        None,
    );
    success(&output);
    report(
        &output.stdout,
        "cics.pilot.read-update.executed",
        "ibm-cics-ts-6x-2026-08-31:api-commands:0156",
        "read-update",
        "executed",
        30,
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("batches=26 verdicts=30 pass=30 fail=0")
    );
}

fn jcl_artifacts() -> [Option<Vec<u8>>; 2] {
    ["verdicts.json", "ledger.json"].map(|name| {
        let path = root().join("target/conformance/jcl-jes2").join(name);
        match fs::read(&path) {
            Ok(bytes) => Some(bytes),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => panic!("{}: {error}", path.display()),
        }
    })
}

fn jcl_report(bytes: &[u8]) {
    report(
        bytes,
        JCL,
        "ibm-zos-3.2-jcl-jes2-2026-06:dd-parameters:0001",
        "dd.parameters-0001.valid",
        "recognized",
        1,
    );
}

#[test]
fn jcl_explicit_external_file_replaces_default_artifacts() {
    let receipts = Receipts::new("jcl-file");
    let destination = receipts.0.join("report.jsonl");
    fs::write(&destination, b"previous external report\n").unwrap();
    let before = jcl_artifacts();
    let output = receipts.run("replay", &["--replay", JCL, "--check"], Some(&destination));
    success(&output);
    assert!(output.stdout.is_empty());
    jcl_report(&fs::read(destination).unwrap());
    assert_eq!(jcl_artifacts(), before);
    assert!(!fs::read_dir(&receipts.0).unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .ends_with(".tmp")
    }));
}

#[test]
fn jcl_defaults_to_jsonl_stdout_without_implicit_files() {
    let receipts = Receipts::new("jcl-stdout");
    let before = jcl_artifacts();
    let output = receipts.run("replay", &["--replay", JCL, "--check"], None);
    success(&output);
    jcl_report(&output.stdout);
    assert_eq!(jcl_artifacts(), before);
}

fn relative_from(base: &Path, destination: &Path) -> PathBuf {
    let left: Vec<_> = base.components().collect();
    let right: Vec<_> = destination.components().collect();
    let common = left
        .iter()
        .zip(&right)
        .take_while(|(left, right)| left == right)
        .count();
    let mut path = PathBuf::new();
    for _ in common..left.len() {
        path.push("..");
    }
    for component in &right[common..] {
        path.push(component.as_os_str());
    }
    path
}

#[test]
fn explicit_relative_path_uses_invoking_cwd() {
    let receipts = Receipts::new("relative-file");
    let destination = receipts.0.join("relative.jsonl");
    let relative = relative_from(&root(), &destination);
    assert!(!relative.is_absolute());
    let output = receipts.run("replay", &["--replay", COBOL, "--check"], Some(&relative));
    success(&output);
    assert!(output.stdout.is_empty());
    report(
        &fs::read(destination).unwrap(),
        COBOL,
        "ibm-enterprise-cobol-6.5-2026-05-31:compiler-directing-statements:0001",
        "valid-forms",
        "recognized",
        1,
    );
}

fn refusal(output: &Output, message: &str) {
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains(message), "{stderr}");
    assert!(!stderr.contains("conformance: pass"));
    assert!(!stderr.contains("panicked"));
}

#[test]
fn output_only_and_invalid_selectors_refuse_without_truncating() {
    let receipts = Receipts::new("selector-refusal");
    let destination = receipts.0.join("sentinel.jsonl");
    let cases: &[(&str, &[&str], &str)] = &[
        (
            "output-only",
            &["--check"],
            "--output requires focused --subsystem or --replay",
        ),
        (
            "gate-only",
            &["--gate", "recognized", "--check"],
            "--output requires focused",
        ),
        (
            "exclusive",
            &["--replay", COBOL, "--subsystem", "cobol", "--check"],
            "mutually exclusive",
        ),
        (
            "replay-gate",
            &["--replay", COBOL, "--gate", "recognized", "--check"],
            "--replay cannot be combined",
        ),
        (
            "malformed-gate",
            &["--subsystem", "cobol", "--gate", "invalid-gate", "--check"],
            "unknown coverage gate",
        ),
        (
            "unknown-replay",
            &[
                "--replay",
                "cobol.output-boundary-does-not-exist",
                "--check",
            ],
            "selection has no executable bindings",
        ),
        (
            "parse-refusal",
            &["--not-an-option", "--check"],
            "unexpected argument",
        ),
    ];
    for (name, args, expected) in cases {
        fs::write(
            &destination,
            b"external output sentinel; keep unchanged on refusal\n",
        )
        .unwrap();
        let output = receipts.run(name, args, Some(&destination));
        refusal(&output, expected);
        assert_eq!(
            output.status.code(),
            Some(if *name == "parse-refusal" { 2 } else { 1 })
        );
        assert_eq!(
            fs::read(&destination).unwrap(),
            b"external output sentinel; keep unchanged on refusal\n"
        );
    }
}

#[test]
fn ims_official_pending_and_output_preparation_controls_stay_uncredited() {
    let receipts = Receipts::new("ims-refusal");
    let destination = receipts.0.join("sentinel.jsonl");
    fs::write(&destination, b"pending report sentinel\n").unwrap();
    let official = receipts.run(
        "official",
        &["--subsystem", "ims", "--gate", "recognized", "--check"],
        Some(&destination),
    );
    refusal(&official, "pending HUMAN maintainer acceptance");
    assert_eq!(official.status.code(), Some(1));
    let preparation = receipts.run(
        "preparation-output",
        &["--subsystem", "ims", "--prepare-candidates", "--check"],
        Some(&destination),
    );
    refusal(
        &preparation,
        "--output cannot be combined with candidate preparation",
    );
    assert_eq!(preparation.status.code(), Some(1));
    let replay = receipts.run(
        "preparation-replay",
        &[
            "--replay",
            "ims.output-not-accepted",
            "--prepare-candidates",
            "--check",
        ],
        Some(&destination),
    );
    refusal(
        &replay,
        "--output cannot be combined with candidate preparation",
    );
    assert_eq!(replay.status.code(), Some(1));
    assert_eq!(fs::read(destination).unwrap(), b"pending report sentinel\n");
}

#[test]
fn empty_filtered_run_is_refused_without_a_report_or_success() {
    let receipts = Receipts::new("empty-filter");
    let destination = receipts.0.join("absent.jsonl");
    let output = receipts.run(
        "filtered",
        &[
            "--subsystem",
            "cobol",
            "--gate",
            "differential",
            "--shard",
            "4095",
            "--check",
        ],
        Some(&destination),
    );
    refusal(&output, "selection has no executable bindings");
    assert_eq!(output.status.code(), Some(1));
    assert!(!destination.exists());
}

#[test]
fn real_file_sink_failures_are_nonzero_and_leave_sentinels_intact() {
    let receipts = Receipts::new("sink-failure");
    // Fail the actual selected driver at its filesystem boundary, before opening a store.
    // The committed fixture, observation and product implementation remain unchanged.
    let invalid_temporary_parent = receipts.0.join("temporary-parent-is-a-file");
    fs::write(
        &invalid_temporary_parent,
        b"keep temporary-parent sentinel\n",
    )
    .unwrap();
    for file_output in [false, true] {
        let destination = receipts.0.join("selected-failure.jsonl");
        let mut command = Command::new(env!("CARGO_BIN_EXE_xtask"));
        command
            .current_dir(root())
            .args([
                "conformance",
                "--replay",
                "mq.mqconn.sqlite.executed",
                "--check",
            ])
            .env("TMPDIR", &invalid_temporary_parent);
        if file_output {
            command.arg("--output").arg(&destination);
        }
        let output = command.output().unwrap();
        receipts.save(
            if file_output {
                "selected-fail-file"
            } else {
                "selected-fail-stdout"
            },
            &command,
            &output,
        );
        assert_eq!(output.status.code(), Some(1));
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("MQ selected driver produced failing evidence"));
        assert!(!stderr.contains("conformance: pass"));
        assert!(!stderr.contains("panicked"));
        let bytes = if file_output {
            assert!(output.stdout.is_empty());
            fs::read(&destination).unwrap()
        } else {
            output.stdout
        };
        let records: Vec<Value> = bytes
            .strip_suffix(b"\n")
            .unwrap()
            .split(|byte| *byte == b'\n')
            .map(|line| serde_json::from_slice(line).unwrap())
            .collect();
        assert_eq!(records.len(), 2);
        let event = &records[0];
        let ledger = &records[1];
        assert_eq!(event["schema_version"], VERDICT);
        assert_eq!(event["test_id"], "mq.mqconn.sqlite.executed");
        assert_eq!(
            event["row_id"],
            "ibm-mq-9.4-mqi-2026-08-31:mqi-calls-unique:0008"
        );
        assert_eq!(event["obligation_id"], "selected-local-v1-sqlite");
        assert_eq!(event["gate"], "executed");
        assert_eq!(event["driver"], "mq.selected.product");
        assert_eq!(event["fixture_or_seed"], "mq.sqlite.local-v1");
        assert_eq!(event["verdict"], "fail");
        assert!(event["actual"].as_str().unwrap().contains("driver=error:"));
        assert!(event["oracle_receipt_digest"].is_null());
        assert_eq!(ledger["schema_version"], LEDGER);
        assert_eq!(event["spec_digest"], ledger["spec_digest"]);
        assert_eq!(event["catalog_digest"], ledger["catalog_digest"]);
        let rows = ledger["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 1506);
        let selected = rows
            .iter()
            .find(|row| row["row_id"] == event["row_id"])
            .unwrap();
        let executed = selected["gates"]
            .as_array()
            .unwrap()
            .iter()
            .find(|gate| gate["gate"] == "executed")
            .unwrap();
        assert_eq!(executed["state"], "failed");
        let counts = ledger["counts"].as_array().unwrap();
        assert_eq!(counts.len(), 6);
        for count in counts {
            assert_eq!(count["pass"], 0);
            assert_eq!(count["fail"], u64::from(count["gate"] == "executed"));
            let denominator: u64 = ["pass", "fail", "pending", "non_applicable"]
                .iter()
                .map(|key| count[key].as_u64().unwrap())
                .sum();
            assert_eq!(denominator, 1506);
        }
        assert_eq!(
            fs::read(&invalid_temporary_parent).unwrap(),
            b"keep temporary-parent sentinel\n"
        );
    }
    let directory = receipts.0.join("destination-directory");
    fs::create_dir(&directory).unwrap();
    let sentinel = directory.join("sentinel");
    fs::write(&sentinel, b"keep").unwrap();
    let output = receipts.run(
        "directory",
        &["--replay", COBOL, "--check"],
        Some(&directory),
    );
    refusal(&output, "not a regular file");
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(fs::read(sentinel).unwrap(), b"keep");
    let missing = receipts.0.join("missing/report.jsonl");
    let output = receipts.run(
        "missing-parent",
        &["--replay", COBOL, "--check"],
        Some(&missing),
    );
    refusal(&output, "conformance output create");
    assert_eq!(output.status.code(), Some(1));
    assert!(!missing.parent().unwrap().exists());
}

#[test]
fn real_broken_stdout_pipe_is_nonzero_without_panic() {
    let receipts = Receipts::new("broken-pipe");
    let mut command = Command::new(env!("CARGO_BIN_EXE_xtask"));
    command
        .current_dir(root())
        .args(["conformance", "--replay", COBOL, "--check"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().unwrap();
    drop(child.stdout.take().unwrap());
    let output = child.wait_with_output().unwrap();
    receipts.save("closed-pipe", &command, &output);
    assert_eq!(output.status.code(), Some(1));
    refusal(&output, "conformance output write/flush failed");
}

#[test]
fn help_declares_explicit_output_path() {
    let receipts = Receipts::new("help");
    let output = receipts.run("help", &["--help"], None);
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("--output <PATH>"));
}
