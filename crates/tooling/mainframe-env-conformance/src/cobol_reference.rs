//! Bounded GnuCOBOL development-reference campaign for approved portable 0.4 cases.
//!
//! GnuCOBOL remains an out-of-process secondary implementation. This module is
//! tooling-only, grants no IBM differential credit, and rejects tool identity
//! drift before compiling any fixture.

use mainframe_env_execution_api::MachineDrive;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

const ALLOWLIST: &str =
    include_str!("../../../../conformance/0.4/cobol/gnucobol-reference-allowlist.json");
const OFFICIAL_LANGUAGE: &str = include_str!("../../../../conformance/0.3/cobol/language.json");
const COMPILER_INSTALLED: &str = "/Users/tore/.homebrew/bin/cobc";
const COMPILER_RESOLVED: &str = "/Users/tore/.homebrew/Cellar/gnucobol/3.2_1/bin/cobc";
const COMPILER_DIGEST: &str =
    "sha256:27240345b880a92924b4869ba77c457cf12d1d2ed6a2cd4e54f425ea1e7f39e9";
const RUNTIME_INSTALLED: &str = "/Users/tore/.homebrew/bin/cobcrun";
const RUNTIME_RESOLVED: &str = "/Users/tore/.homebrew/Cellar/gnucobol/3.2_1/bin/cobcrun";
const RUNTIME_DIGEST: &str =
    "sha256:4ec157f16524270196eb6dd5f6f335db3020bee809e1ecb12a4d84b5edfb6715";
const VERSION: &str = "3.2.0";
const MAX_CASES: usize = 32;
const MAX_SOURCE_BYTES: usize = 16_384;
const MAX_OUTPUT_BYTES: usize = 65_536;
const PROCESS_TIMEOUT: Duration = Duration::from_secs(10);
const COMPILER_FLAGS: &[&str] = &["-std=ibm-strict", "-free", "-m"];
const REVIEWED_COMPILE_STDERR: &str = "ld: warning: -undefined suppress is deprecated\nld: warning: -undefined suppress is deprecated\n";
const ENCODING_ASSUMPTIONS: &[&str] = &[
    "allowlisted source is UTF-8 restricted to portable syntax and ASCII-subset data",
    "GnuCOBOL stdout and stderr must be valid UTF-8 after CRLF normalization",
    "mainframe-env output must be valid UTF-8 after CRLF normalization",
];
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Allowlist {
    schema_version: String,
    target_version: String,
    reference: String,
    reference_version: String,
    dialect: String,
    source_format: String,
    source_encoding: String,
    normalization_rules: Vec<String>,
    cases: Vec<ReferenceCase>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReferenceCase {
    id: String,
    cohort: String,
    row_id: String,
    source_locator: String,
    program_id: String,
    portable_rationale: String,
    source: String,
    expected_stdout: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct GnuCobolReferenceReceipt {
    pub schema_version: String,
    pub target_version: String,
    pub status: String,
    pub baseline_id: String,
    pub reference: String,
    pub licensed_differential_credit: usize,
    pub licensed_cases_pending: usize,
    pub candidate_identity: String,
    pub fixture_digest: String,
    pub compiler: ToolIdentity,
    pub runtime: ToolIdentity,
    pub compiler_flags: Vec<String>,
    pub runtime_arguments: Vec<String>,
    pub locale: BTreeMap<String, String>,
    pub encoding_assumptions: Vec<String>,
    pub source_format: String,
    pub normalization_rules: Vec<String>,
    pub limits: CampaignLimits,
    pub case_count: usize,
    pub passed_cases: usize,
    pub mutants_killed: usize,
    pub cases: Vec<ReferenceCaseReceipt>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ToolIdentity {
    installed_path: String,
    resolved_path: String,
    sha256: String,
    version: String,
    version_exit_status: i32,
    version_output_sha256: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct CampaignLimits {
    max_cases: usize,
    max_source_bytes: usize,
    max_output_bytes: usize,
    process_timeout_ms: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ReferenceCaseReceipt {
    id: String,
    cohort: String,
    row_id: String,
    source_locator: String,
    program_id: String,
    portable_rationale: String,
    source_sha256: String,
    compile_exit_status: i32,
    compile_stdout: String,
    compile_stderr: String,
    reference_exit_status: i32,
    reference_stdout: String,
    reference_stderr: String,
    product_exit_status: i32,
    product_stdout: String,
    comparison: String,
}

#[derive(Clone, Copy)]
struct Comparison<'a> {
    compile_status: i32,
    compile_stdout: &'a str,
    compile_stderr: &'a str,
    reference_status: i32,
    reference_stdout: &'a str,
    reference_stderr: &'a str,
    product_status: i32,
    product_stdout: &'a str,
    expected_stdout: &'a str,
}

struct TemporaryDirectory(PathBuf);

impl TemporaryDirectory {
    fn create() -> Result<Self, String> {
        let base = std::env::temp_dir();
        for _ in 0..64 {
            let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let path = base.join(format!(
                "mainframe-env-gnucobol-{}-{sequence}",
                std::process::id()
            ));
            match fs::create_dir(&path) {
                Ok(()) => return Ok(Self(path)),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(format!("create GnuCOBOL work directory: {error}")),
            }
        }
        Err("could not allocate a unique GnuCOBOL work directory".into())
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TemporaryDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub fn gnucobol_reference_fixture_digest() -> String {
    digest(ALLOWLIST.as_bytes())
}

pub fn verify_gnucobol_reference_allowlist() -> Result<usize, String> {
    let allowlist = load_allowlist()?;
    verify_allowlist(&allowlist)?;
    Ok(allowlist.cases.len())
}

pub fn run_gnucobol_reference_campaign(
    candidate_identity: &str,
) -> Result<GnuCobolReferenceReceipt, String> {
    if !valid_digest(candidate_identity) {
        return Err("GnuCOBOL campaign candidate identity is not a SHA-256 digest".into());
    }
    let allowlist = load_allowlist()?;
    verify_allowlist(&allowlist)?;
    let directory = TemporaryDirectory::create()?;
    let compiler = pinned_tool(
        "compiler",
        COMPILER_INSTALLED,
        COMPILER_RESOLVED,
        COMPILER_DIGEST,
        "cobc (GnuCOBOL) 3.2.0",
        directory.path(),
    )?;
    let runtime = pinned_tool(
        "runtime",
        RUNTIME_INSTALLED,
        RUNTIME_RESOLVED,
        RUNTIME_DIGEST,
        "cobcrun (GnuCOBOL) 3.2.0",
        directory.path(),
    )?;
    let mut cases = Vec::with_capacity(allowlist.cases.len());
    for case in &allowlist.cases {
        cases.push(run_case(case, directory.path())?);
    }
    if cases.len() != allowlist.cases.len() || cases.iter().any(|case| case.comparison != "pass") {
        return Err("GnuCOBOL reference campaign did not close its allowlist".into());
    }
    Ok(GnuCobolReferenceReceipt {
        schema_version: "mainframe-env.cobol-gnucobol-reference-receipt@1".into(),
        target_version: "0.4.0".into(),
        status: "pass-with-licensed-differential-pending".into(),
        baseline_id: "ibm-enterprise-cobol-6.5-2026-05-31".into(),
        reference: "gnucobol".into(),
        licensed_differential_credit: 0,
        licensed_cases_pending: 153,
        candidate_identity: candidate_identity.into(),
        fixture_digest: gnucobol_reference_fixture_digest(),
        compiler,
        runtime,
        compiler_flags: COMPILER_FLAGS
            .iter()
            .map(|value| (*value).to_string())
            .collect(),
        runtime_arguments: vec!["<program-id>".into()],
        locale: BTreeMap::from([
            ("LANG".into(), "C.UTF-8".into()),
            ("LC_ALL".into(), "C.UTF-8".into()),
        ]),
        encoding_assumptions: ENCODING_ASSUMPTIONS
            .iter()
            .map(|value| (*value).to_string())
            .collect(),
        source_format: "free".into(),
        normalization_rules: allowlist.normalization_rules,
        limits: CampaignLimits {
            max_cases: MAX_CASES,
            max_source_bytes: MAX_SOURCE_BYTES,
            max_output_bytes: MAX_OUTPUT_BYTES,
            process_timeout_ms: PROCESS_TIMEOUT.as_millis() as u64,
        },
        case_count: cases.len(),
        passed_cases: cases.len(),
        mutants_killed: 4,
        cases,
    })
}

fn load_allowlist() -> Result<Allowlist, String> {
    serde_json::from_str(ALLOWLIST).map_err(|error| format!("GnuCOBOL allowlist: {error}"))
}

fn verify_allowlist(allowlist: &Allowlist) -> Result<(), String> {
    if allowlist.schema_version != "mainframe-env.cobol-gnucobol-reference-allowlist@1"
        || allowlist.target_version != "0.4.0"
        || allowlist.reference != "gnucobol"
        || allowlist.reference_version != VERSION
        || allowlist.dialect != "ibm-strict"
        || allowlist.source_format != "free"
        || allowlist.source_encoding != "UTF-8"
        || allowlist.normalization_rules.len() < 4
        || allowlist.normalization_rules.len() > 16
        || allowlist.cases.len() != 16
        || allowlist.cases.len() > MAX_CASES
    {
        return Err("GnuCOBOL allowlist envelope drifted".into());
    }
    let normalization = allowlist
        .normalization_rules
        .iter()
        .collect::<BTreeSet<_>>();
    if normalization.len() != allowlist.normalization_rules.len() {
        return Err("GnuCOBOL normalization rules are not unique".into());
    }
    let language: Value = serde_json::from_str(OFFICIAL_LANGUAGE)
        .map_err(|error| format!("official COBOL language inventory: {error}"))?;
    let mut authorities = BTreeMap::new();
    for family in ["procedure_statements", "intrinsic_functions"] {
        let rows = language
            .get(family)
            .and_then(Value::as_array)
            .ok_or("official COBOL language inventory is malformed")?;
        for row in rows {
            let row_id = row
                .get("row_id")
                .and_then(Value::as_str)
                .ok_or("official COBOL row omits row_id")?;
            let source_locator = row
                .get("source_locator")
                .and_then(Value::as_str)
                .ok_or("official COBOL row omits source_locator")?;
            authorities.insert(row_id, source_locator);
        }
    }
    let mut ids = BTreeSet::new();
    let mut programs = BTreeSet::new();
    let mut cohort_counts = BTreeMap::<&str, usize>::new();
    for case in &allowlist.cases {
        if !ids.insert(case.id.as_str())
            || !programs.insert(case.program_id.as_str())
            || !matches!(
                case.cohort.as_str(),
                "statement-control" | "decimal-arithmetic" | "move-string" | "intrinsic"
            )
            || authorities.get(case.row_id.as_str()).copied() != Some(case.source_locator.as_str())
            || case.portable_rationale.len() < 32
            || case.portable_rationale.len() > 512
            || case.source.len() > MAX_SOURCE_BYTES
            || !case.source.is_ascii()
            || !case.source.starts_with("IDENTIFICATION DIVISION.")
            || !case
                .source
                .contains(&format!("PROGRAM-ID. {}.", case.program_id))
            || case.expected_stdout != "PASS\n"
        {
            return Err(format!("GnuCOBOL allowlist case {} drifted", case.id));
        }
        let upper = case.source.to_ascii_uppercase();
        for excluded in [
            " NATIONAL",
            " UTF-8",
            " DBCS",
            " COMP-",
            " BINARY",
            " POINTER",
            " OBJECT REFERENCE",
            " SPECIAL-NAMES",
            " FILE SECTION",
            " JSON ",
            " XML ",
            " CALL ",
            " INVOKE ",
            " ACCEPT ",
            " CURRENT-DATE",
            " RANDOM",
            " EXEC ",
        ] {
            if upper.contains(excluded) {
                return Err(format!(
                    "GnuCOBOL allowlist case {} crosses excluded boundary {excluded}",
                    case.id
                ));
            }
        }
        *cohort_counts.entry(case.cohort.as_str()).or_default() += 1;
    }
    if cohort_counts
        != BTreeMap::from([
            ("decimal-arithmetic", 2),
            ("intrinsic", 7),
            ("move-string", 4),
            ("statement-control", 3),
        ])
    {
        return Err("GnuCOBOL portable cohort denominator drifted".into());
    }
    Ok(())
}

fn run_case(case: &ReferenceCase, root: &Path) -> Result<ReferenceCaseReceipt, String> {
    let directory = root.join(&case.id);
    fs::create_dir(&directory)
        .map_err(|error| format!("create GnuCOBOL case directory {}: {error}", case.id))?;
    let source_name = format!("{}.cbl", case.program_id);
    let source = if case.source.ends_with('\n') {
        case.source.clone()
    } else {
        format!("{}\n", case.source)
    };
    fs::write(directory.join(&source_name), source.as_bytes())
        .map_err(|error| format!("write GnuCOBOL case {}: {error}", case.id))?;
    let compiler_args = COMPILER_FLAGS
        .iter()
        .map(OsString::from)
        .chain(std::iter::once(OsString::from(&source_name)))
        .collect::<Vec<_>>();
    let compile = run_bounded(
        Path::new(COMPILER_RESOLVED),
        &compiler_args,
        &directory,
        &[],
    )?;
    let compile_status = exit_status(&compile, &case.id, "compile")?;
    let compile_stdout = normalize_output(&compile.stdout, &case.id, "compile stdout")?;
    let compile_stderr = normalize_output(&compile.stderr, &case.id, "compile stderr")?;
    if compile_status != 0
        || !compile_stdout.is_empty()
        || compile_stderr != REVIEWED_COMPILE_STDERR
    {
        return Err(format!(
            "GnuCOBOL case {} did not compile cleanly: status={compile_status}; stdout={compile_stdout:?}; stderr={compile_stderr:?}",
            case.id
        ));
    }
    let runtime = run_bounded(
        Path::new(RUNTIME_RESOLVED),
        &[OsString::from(&case.program_id)],
        &directory,
        &[(
            OsString::from("COB_LIBRARY_PATH"),
            directory.as_os_str().into(),
        )],
    )?;
    let reference_status = exit_status(&runtime, &case.id, "runtime")?;
    let reference_stdout = normalize_output(&runtime.stdout, &case.id, "runtime stdout")?;
    let reference_stderr = normalize_output(&runtime.stderr, &case.id, "runtime stderr")?;
    let artifact = crate::compile(&source)
        .map_err(|error| format!("mainframe-env case {} compile: {error}", case.id))?;
    let (product_status, product_bytes) = match crate::execute(&artifact, MAX_OUTPUT_BYTES as u64) {
        MachineDrive::Completed(completion) => {
            (completion.return_code, completion.output.bytes().to_vec())
        }
        other => {
            return Err(format!(
                "mainframe-env case {} reached nonportable terminal {other:?}",
                case.id
            ));
        }
    };
    let product_stdout = normalize_output(&product_bytes, &case.id, "product stdout")?;
    let comparison = Comparison {
        compile_status,
        compile_stdout: &compile_stdout,
        compile_stderr: &compile_stderr,
        reference_status,
        reference_stdout: &reference_stdout,
        reference_stderr: &reference_stderr,
        product_status,
        product_stdout: &product_stdout,
        expected_stdout: &case.expected_stdout,
    };
    if !comparison_passes(comparison) {
        return Err(format!(
            "GnuCOBOL disagreement for {} requires classification as product bug, harness bug, or documented dialect/runtime divergence: reference=status {reference_status}, stdout {reference_stdout:?}, stderr {reference_stderr:?}; product=status {product_status}, stdout {product_stdout:?}; expected={:?}; authority={}",
            case.id, case.expected_stdout, case.source_locator
        ));
    }
    Ok(ReferenceCaseReceipt {
        id: case.id.clone(),
        cohort: case.cohort.clone(),
        row_id: case.row_id.clone(),
        source_locator: case.source_locator.clone(),
        program_id: case.program_id.clone(),
        portable_rationale: case.portable_rationale.clone(),
        source_sha256: digest(source.as_bytes()),
        compile_exit_status: compile_status,
        compile_stdout,
        compile_stderr,
        reference_exit_status: reference_status,
        reference_stdout,
        reference_stderr,
        product_exit_status: product_status,
        product_stdout,
        comparison: "pass".into(),
    })
}

fn comparison_passes(comparison: Comparison<'_>) -> bool {
    comparison.compile_status == 0
        && comparison.compile_stdout.is_empty()
        && comparison.compile_stderr == REVIEWED_COMPILE_STDERR
        && comparison.reference_status == 0
        && comparison.reference_stderr.is_empty()
        && comparison.product_status == 0
        && comparison.reference_stdout == comparison.expected_stdout
        && comparison.product_stdout == comparison.expected_stdout
        && comparison.reference_stdout == comparison.product_stdout
}

fn pinned_tool(
    role: &str,
    installed: &str,
    expected_resolved: &str,
    expected_digest: &str,
    expected_version_line: &str,
    directory: &Path,
) -> Result<ToolIdentity, String> {
    let installed_path = Path::new(installed);
    let resolved = fs::canonicalize(installed_path)
        .map_err(|error| format!("pinned GnuCOBOL {role} is unavailable: {error}"))?;
    if resolved != Path::new(expected_resolved) {
        return Err(format!(
            "pinned GnuCOBOL {role} resolved path drifted: {}",
            resolved.display()
        ));
    }
    let bytes =
        fs::read(&resolved).map_err(|error| format!("read pinned GnuCOBOL {role}: {error}"))?;
    if bytes.len() > 64 * 1024 * 1024 || digest(&bytes) != expected_digest {
        return Err(format!("pinned GnuCOBOL {role} binary digest drifted"));
    }
    let version = run_bounded(&resolved, &[OsString::from("--version")], directory, &[])?;
    let version_status = exit_status(&version, role, "version")?;
    let version_stdout = normalize_output(&version.stdout, role, "version stdout")?;
    let version_stderr = normalize_output(&version.stderr, role, "version stderr")?;
    if version_status != 0
        || version_stdout.lines().next() != Some(expected_version_line)
        || !version_stderr.is_empty()
    {
        return Err(format!("pinned GnuCOBOL {role} version identity drifted"));
    }
    Ok(ToolIdentity {
        installed_path: installed.into(),
        resolved_path: resolved.to_string_lossy().into_owned(),
        sha256: expected_digest.into(),
        version: VERSION.into(),
        version_exit_status: version_status,
        version_output_sha256: digest(version_stdout.as_bytes()),
    })
}

fn run_bounded(
    program: &Path,
    arguments: &[OsString],
    directory: &Path,
    additional_environment: &[(OsString, OsString)],
) -> Result<Output, String> {
    let mut command = Command::new(program);
    command
        .args(arguments)
        .current_dir(directory)
        .env_clear()
        .env("PATH", "/usr/bin:/bin:/Users/tore/.homebrew/bin")
        .env("LANG", "C.UTF-8")
        .env("LC_ALL", "C.UTF-8")
        .env("TMPDIR", directory)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (key, value) in additional_environment {
        command.env(key, value);
    }
    let mut child = command
        .spawn()
        .map_err(|error| format!("spawn pinned tool {}: {error}", program.display()))?;
    let start = Instant::now();
    loop {
        match child
            .try_wait()
            .map_err(|error| format!("poll pinned tool {}: {error}", program.display()))?
        {
            Some(_) => {
                let output = child.wait_with_output().map_err(|error| {
                    format!("collect pinned tool {}: {error}", program.display())
                })?;
                if output.stdout.len() > MAX_OUTPUT_BYTES || output.stderr.len() > MAX_OUTPUT_BYTES
                {
                    return Err(format!(
                        "pinned tool {} exceeded the output bound",
                        program.display()
                    ));
                }
                return Ok(output);
            }
            None if start.elapsed() >= PROCESS_TIMEOUT => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!(
                    "pinned tool {} exceeded the process timeout",
                    program.display()
                ));
            }
            None => thread::sleep(Duration::from_millis(10)),
        }
    }
}

fn exit_status(output: &Output, case: &str, phase: &str) -> Result<i32, String> {
    output
        .status
        .code()
        .ok_or_else(|| format!("GnuCOBOL {case} {phase} terminated without an exit status"))
}

fn normalize_output(bytes: &[u8], case: &str, stream: &str) -> Result<String, String> {
    if bytes.len() > MAX_OUTPUT_BYTES {
        return Err(format!(
            "GnuCOBOL {case} {stream} exceeded the output bound"
        ));
    }
    let value = std::str::from_utf8(bytes)
        .map_err(|_| format!("GnuCOBOL {case} {stream} was not valid UTF-8"))?;
    Ok(value.replace("\r\n", "\n"))
}

fn digest(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

fn valid_digest(value: &str) -> bool {
    value.len() == 71
        && value.starts_with("sha256:")
        && value[7..]
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn passing() -> Comparison<'static> {
        Comparison {
            compile_status: 0,
            compile_stdout: "",
            compile_stderr: REVIEWED_COMPILE_STDERR,
            reference_status: 0,
            reference_stdout: "PASS\n",
            reference_stderr: "",
            product_status: 0,
            product_stdout: "PASS\n",
            expected_stdout: "PASS\n",
        }
    }

    #[test]
    fn approved_allowlist_is_exact_bounded_portable_and_zero_credit() {
        assert_eq!(verify_gnucobol_reference_allowlist().unwrap(), 16);
        assert!(valid_digest(&gnucobol_reference_fixture_digest()));
        assert_eq!(COMPILER_FLAGS, ["-std=ibm-strict", "-free", "-m"]);
    }

    #[test]
    fn reference_product_exit_and_expected_output_mutants_are_killed() {
        assert!(comparison_passes(passing()));
        let mut reference_output = passing();
        reference_output.reference_stdout = "MUTANT\n";
        assert!(!comparison_passes(reference_output));
        let mut product_output = passing();
        product_output.product_stdout = "MUTANT\n";
        assert!(!comparison_passes(product_output));
        let mut exit_status = passing();
        exit_status.reference_status = 1;
        assert!(!comparison_passes(exit_status));
        let mut generic_success = passing();
        generic_success.expected_stdout = "REVIEWED\n";
        assert!(!comparison_passes(generic_success));
    }

    #[test]
    fn line_endings_are_the_only_output_normalization() {
        assert_eq!(
            normalize_output(b"A  \r\nB\rC\n", "case", "stdout").unwrap(),
            "A  \nB\rC\n"
        );
    }

    #[test]
    fn pinned_paths_and_digests_are_frozen_without_running_external_tools() {
        assert_eq!(COMPILER_INSTALLED, "/Users/tore/.homebrew/bin/cobc");
        assert_eq!(
            COMPILER_RESOLVED,
            "/Users/tore/.homebrew/Cellar/gnucobol/3.2_1/bin/cobc"
        );
        assert_eq!(RUNTIME_INSTALLED, "/Users/tore/.homebrew/bin/cobcrun");
        assert_eq!(
            RUNTIME_RESOLVED,
            "/Users/tore/.homebrew/Cellar/gnucobol/3.2_1/bin/cobcrun"
        );
        assert!(valid_digest(COMPILER_DIGEST));
        assert!(valid_digest(RUNTIME_DIGEST));
    }

    #[test]
    fn production_manifests_do_not_link_or_package_gnucobol() {
        for manifest in [
            include_str!("../../../kernel/mainframe-env-compiler/Cargo.toml"),
            include_str!("../../../kernel/mainframe-env-interpreter/Cargo.toml"),
            include_str!("../../../apps/mainframe-env-batch/Cargo.toml"),
            include_str!("../../../apps/mainframe-env-server/Cargo.toml"),
        ] {
            let lower = manifest.to_ascii_lowercase();
            assert!(!lower.contains("gnucobol"));
            assert!(!lower.contains("libcob"));
        }
    }

    #[test]
    fn os_string_arguments_remain_explicit() {
        let arguments = COMPILER_FLAGS
            .iter()
            .map(OsString::from)
            .collect::<Vec<_>>();
        assert_eq!(
            arguments.first().map(OsString::as_os_str),
            Some(std::ffi::OsStr::new("-std=ibm-strict"))
        );
    }
}
