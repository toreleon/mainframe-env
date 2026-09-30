use super::*;

pub(super) fn read_runtime_oracle(root: &Path, identity: &str) -> Result<Vec<u8>, CorpusProblem> {
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

#[derive(Clone, Debug, Deserialize)]
pub(super) struct CorpusContract {
    pub(super) repository: String,
    pub(super) commit: String,
    pub(super) tree: String,
    pub(super) license: String,
    pub(super) license_sha256: String,
    pub(super) executable_content_identity: ContentIdentity,
    pub(super) runtime_oracles: Vec<RuntimeOracleContract>,
    pub(super) file_count_checks: Vec<FileCountContract>,
    pub(super) external_compatibility_copybooks: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub(super) struct ContentIdentity {
    pub(super) algorithm: String,
    pub(super) sha256: String,
}

#[derive(Clone, Debug, Deserialize)]
pub(super) struct RuntimeOracleContract {
    pub(super) path: String,
    pub(super) sha256: String,
}

#[derive(Clone, Debug, Deserialize)]
pub(super) struct FileCountContract {
    pub(super) id: String,
    pub(super) roots: Vec<String>,
    #[serde(default)]
    pub(super) extensions: Vec<String>,
    #[serde(default)]
    pub(super) excluded_names: Vec<String>,
    pub(super) expected: usize,
}

pub(super) fn read_contract(path: &Path) -> Result<CorpusContract, CorpusProblem> {
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

pub(super) fn validate_contract(contract: &CorpusContract) -> Result<(), CorpusProblem> {
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
    if contract.external_compatibility_copybooks.is_empty()
        || contract
            .external_compatibility_copybooks
            .iter()
            .collect::<BTreeSet<_>>()
            .len()
            != contract.external_compatibility_copybooks.len()
    {
        return Err(CorpusProblem::new(
            "carddemo.corpus.contract_invalid",
            "external compatibility copybook identities are empty or duplicated",
        ));
    }
    Ok(())
}

pub(super) fn require_corpus_directory(path: &Path) -> Result<(), CorpusProblem> {
    if path.is_dir() {
        Ok(())
    } else {
        Err(CorpusProblem::new(
            "carddemo.corpus.directory_missing",
            "CARDDEMO_CORPUS_DIR does not name an available directory",
        ))
    }
}

pub(super) fn git(root: &Path, arguments: &[&str]) -> Result<String, CorpusProblem> {
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

pub(super) fn tracked_files(root: &Path) -> Result<Vec<String>, CorpusProblem> {
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

pub(super) fn canonical_content_digest(
    root: &Path,
    tracked: &[String],
) -> Result<String, CorpusProblem> {
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

pub(super) fn count_files(
    tracked: &[String],
    check: &FileCountContract,
) -> Result<usize, CorpusProblem> {
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

pub(super) fn validate_relative_path(path: &str, kind: &str) -> Result<(), CorpusProblem> {
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

pub(super) fn read_corpus_file(root: &Path, path: &Path) -> Result<Vec<u8>, CorpusProblem> {
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

pub(super) fn require_equal(
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

pub(super) fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
