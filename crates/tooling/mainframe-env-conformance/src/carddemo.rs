//! Fail-closed verification for the externally supplied CardDemo corpus.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::env;
use std::fmt;
use std::fs;
use std::path::Path;
use std::process::Command;

const CORPUS_ENV: &str = "CARDEMO_CORPUS_DIR";

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
struct CorpusContract {
    repository: String,
    commit: String,
    tree: String,
    license: String,
    license_sha256: String,
    executable_content_identity: ContentIdentity,
    runtime_oracles: Vec<RuntimeOracleContract>,
    file_count_checks: Vec<FileCountContract>,
}

#[derive(Clone, Debug, Deserialize)]
struct ContentIdentity {
    algorithm: String,
    sha256: String,
}

#[derive(Clone, Debug, Deserialize)]
struct RuntimeOracleContract {
    path: String,
    sha256: String,
}

#[derive(Clone, Debug, Deserialize)]
struct FileCountContract {
    id: String,
    roots: Vec<String>,
    #[serde(default)]
    extensions: Vec<String>,
    #[serde(default)]
    excluded_names: Vec<String>,
    expected: usize,
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

pub fn verify_carddemo_corpus_from_env(
    inventory_path: &Path,
) -> Result<CardDemoCorpusReceipt, CorpusProblem> {
    let corpus = env::var_os(CORPUS_ENV).ok_or_else(|| {
        CorpusProblem::new(
            "carddemo.corpus.environment_missing",
            "CARDEMO_CORPUS_DIR is required for CardDemo gates",
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

fn read_runtime_oracle(root: &Path, identity: &str) -> Result<Vec<u8>, CorpusProblem> {
    let Some((archive, entry)) = identity.split_once('!') else {
        validate_relative_path(identity, "runtime oracle")?;
        return read_corpus_file(root, &root.join(identity));
    };
    validate_relative_path(archive, "runtime archive")?;
    validate_relative_path(entry, "runtime archive entry")?;
    let archive_path = root.join(archive);
    if !archive_path.is_file() {
        return Err(CorpusProblem::new(
            "carddemo.corpus.file_missing",
            format!("cannot read corpus file {archive}"),
        ));
    }
    let output = Command::new("unzip")
        .args(["-p"])
        .arg(&archive_path)
        .arg(entry)
        .output()
        .map_err(|error| {
            CorpusProblem::new(
                "carddemo.corpus.archive_reader_unavailable",
                format!("cannot inspect runtime archive metadata: {error}"),
            )
        })?;
    if !output.status.success() {
        return Err(CorpusProblem::new(
            "carddemo.corpus.runtime_archive_drift",
            format!("cannot read declared runtime archive entry {archive}!{entry}"),
        ));
    }
    Ok(output.stdout)
}

fn read_contract(path: &Path) -> Result<CorpusContract, CorpusProblem> {
    let bytes = fs::read(path).map_err(|error| {
        CorpusProblem::new(
            "carddemo.corpus.contract_missing",
            format!("cannot read corpus inventory: {error}"),
        )
    })?;
    serde_json::from_slice(&bytes).map_err(|error| {
        CorpusProblem::new(
            "carddemo.corpus.contract_invalid",
            format!("cannot parse corpus inventory: {error}"),
        )
    })
}

fn validate_contract(contract: &CorpusContract) -> Result<(), CorpusProblem> {
    if contract.executable_content_identity.algorithm != "sha256-u64be-path-u64be-content-v1" {
        return Err(CorpusProblem::new(
            "carddemo.corpus.contract_invalid",
            "unsupported executable content identity algorithm",
        ));
    }
    for (name, value, expected_len) in [
        ("commit", contract.commit.as_str(), 40),
        ("tree", contract.tree.as_str(), 40),
        ("license_sha256", contract.license_sha256.as_str(), 64),
        (
            "executable content SHA-256",
            contract.executable_content_identity.sha256.as_str(),
            64,
        ),
    ] {
        if value.len() != expected_len || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(CorpusProblem::new(
                "carddemo.corpus.contract_invalid",
                format!("{name} is not a {expected_len}-digit hexadecimal identity"),
            ));
        }
    }
    if contract.runtime_oracles.is_empty() || contract.file_count_checks.is_empty() {
        return Err(CorpusProblem::new(
            "carddemo.corpus.contract_invalid",
            "runtime archive and file-count checks must be declared",
        ));
    }
    Ok(())
}

fn require_corpus_directory(path: &Path) -> Result<(), CorpusProblem> {
    if path.is_dir() {
        Ok(())
    } else {
        Err(CorpusProblem::new(
            "carddemo.corpus.directory_missing",
            "CARDEMO_CORPUS_DIR does not name an available directory",
        ))
    }
}

fn git(root: &Path, arguments: &[&str]) -> Result<String, CorpusProblem> {
    let output = Command::new("git")
        .args(arguments)
        .current_dir(root)
        .output()
        .map_err(|error| {
            CorpusProblem::new(
                "carddemo.corpus.git_unavailable",
                format!("cannot execute Git verification: {error}"),
            )
        })?;
    if !output.status.success() {
        return Err(CorpusProblem::new(
            "carddemo.corpus.git_failed",
            format!("Git verification {:?} failed", arguments),
        ));
    }
    String::from_utf8(output.stdout)
        .map(|value| value.trim().to_string())
        .map_err(|_| {
            CorpusProblem::new(
                "carddemo.corpus.git_failed",
                "Git verification returned non-UTF-8 output",
            )
        })
}

fn tracked_files(root: &Path) -> Result<Vec<String>, CorpusProblem> {
    let output = Command::new("git")
        .args(["ls-files", "-z"])
        .current_dir(root)
        .output()
        .map_err(|error| {
            CorpusProblem::new(
                "carddemo.corpus.git_unavailable",
                format!("cannot enumerate tracked corpus files: {error}"),
            )
        })?;
    if !output.status.success() {
        return Err(CorpusProblem::new(
            "carddemo.corpus.git_failed",
            "cannot enumerate tracked corpus files",
        ));
    }
    let text = String::from_utf8(output.stdout).map_err(|_| {
        CorpusProblem::new(
            "carddemo.corpus.path_invalid",
            "tracked corpus paths must be UTF-8",
        )
    })?;
    let mut files = text
        .split('\0')
        .filter(|path| !path.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    files.sort();
    if files.is_empty() {
        return Err(CorpusProblem::new(
            "carddemo.corpus.content_missing",
            "the CardDemo checkout has no tracked files",
        ));
    }
    Ok(files)
}

fn canonical_content_digest(root: &Path, tracked: &[String]) -> Result<String, CorpusProblem> {
    let mut digest = Sha256::new();
    for relative in tracked {
        validate_relative_path(relative, "tracked file")?;
        let bytes = read_corpus_file(root, &root.join(relative))?;
        digest.update((relative.len() as u64).to_be_bytes());
        digest.update(relative.as_bytes());
        digest.update((bytes.len() as u64).to_be_bytes());
        digest.update(bytes);
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn count_files(tracked: &[String], check: &FileCountContract) -> Result<usize, CorpusProblem> {
    if check.id.is_empty() || check.roots.is_empty() {
        return Err(CorpusProblem::new(
            "carddemo.corpus.contract_invalid",
            "file-count checks require an identity and at least one root",
        ));
    }
    for root in &check.roots {
        validate_relative_path(root, "file-count root")?;
    }
    let extensions = check
        .extensions
        .iter()
        .map(|extension| extension.trim_start_matches('.').to_ascii_lowercase())
        .collect::<Vec<_>>();
    Ok(tracked
        .iter()
        .filter(|path| {
            check.roots.iter().any(|root| {
                path.as_str() == root
                    || path
                        .strip_prefix(root)
                        .is_some_and(|suffix| suffix.starts_with('/'))
            })
        })
        .filter(|path| {
            let file_name = Path::new(path)
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("");
            !check.excluded_names.iter().any(|name| name == file_name)
        })
        .filter(|path| {
            extensions.is_empty()
                || Path::new(path)
                    .extension()
                    .and_then(|extension| extension.to_str())
                    .map(str::to_ascii_lowercase)
                    .is_some_and(|extension| extensions.contains(&extension))
        })
        .count())
}

fn validate_relative_path(path: &str, kind: &str) -> Result<(), CorpusProblem> {
    let candidate = Path::new(path);
    if path.is_empty()
        || candidate.is_absolute()
        || candidate
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(CorpusProblem::new(
            "carddemo.corpus.contract_invalid",
            format!("{kind} must be a normalized repository-relative path"),
        ));
    }
    Ok(())
}

fn read_corpus_file(root: &Path, path: &Path) -> Result<Vec<u8>, CorpusProblem> {
    let relative = path.strip_prefix(root).map_err(|_| {
        CorpusProblem::new(
            "carddemo.corpus.path_invalid",
            "corpus file escaped the declared root",
        )
    })?;
    fs::read(path).map_err(|error| {
        CorpusProblem::new(
            "carddemo.corpus.file_missing",
            format!("cannot read corpus file {}: {error}", relative.display()),
        )
    })
}

fn require_equal(
    code: &str,
    label: &str,
    expected: &str,
    actual: &str,
) -> Result<(), CorpusProblem> {
    if expected == actual {
        Ok(())
    } else {
        Err(CorpusProblem::new(
            code,
            format!("{label} expected {expected} but found {actual}"),
        ))
    }
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
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
                }]
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
}
