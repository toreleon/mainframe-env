use super::*;

const DOMAIN: &[u8] = b"mainframe-env.work-package-seal@1\0";

#[derive(Clone, Debug)]
struct ChangedPath {
    status: char,
    path: String,
    bytes: Vec<u8>,
}

pub(super) fn run(root: &Path, args: &WorkPackageSealArgs) -> TaskResult {
    validate_token_argument(&args.id, "work-package ID")?;
    validate_token_argument(&args.target_version, "target version")?;
    let expected = validate_allowlist(&args.paths)?;
    let changes = if args.check {
        committed_changes(root)?
    } else {
        staged_changes(root)?
    };
    let actual = changes
        .iter()
        .map(|change| change.path.clone())
        .collect::<BTreeSet<_>>();
    require(
        actual == expected,
        &format!(
            "work-package allowlist differs: missing={:?} extra={:?}",
            expected.difference(&actual).collect::<Vec<_>>(),
            actual.difference(&expected).collect::<Vec<_>>()
        ),
    )?;
    let digest = digest(&args.id, &args.target_version, &changes);
    let message = format!(
        "Complete {}\n\nWork-Package: {}=pass\nTarget-Version: {}\nEvidence-Digest: sha256:{}\n",
        args.id, args.id, args.target_version, digest
    );
    if args.check {
        let actual_message = command_text(root, "git", &["show", "-s", "--format=%B", "HEAD"])?;
        require(
            actual_message.trim_end() == message.trim_end(),
            "HEAD commit message differs from the generated work-package seal",
        )?;
    } else {
        require(
            !changes.is_empty(),
            "work-package staged allowlist is empty",
        )?;
        print!("{message}");
    }
    Ok(())
}

fn validate_token_argument(value: &str, label: &str) -> TaskResult {
    require(
        !value.is_empty()
            && value.len() <= 64
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_')),
        &format!("invalid {label}"),
    )
}

fn validate_allowlist(paths: &[String]) -> TaskResult<BTreeSet<String>> {
    require(!paths.is_empty(), "work-package path allowlist is empty")?;
    let mut result = BTreeSet::new();
    for path in paths {
        let candidate = Path::new(path);
        require(
            !candidate.is_absolute()
                && !path.is_empty()
                && !path.contains('\0')
                && !candidate
                    .components()
                    .any(|component| matches!(component, std::path::Component::ParentDir)),
            &format!("unsafe work-package path {path}"),
        )?;
        require(
            result.insert(path.clone()),
            &format!("duplicate work-package path {path}"),
        )?;
    }
    Ok(result)
}

fn staged_changes(root: &Path) -> TaskResult<Vec<ChangedPath>> {
    changes(
        root,
        &["diff", "--cached", "--name-status", "--diff-filter=AMD"],
        |path, status| {
            if status == 'D' {
                Ok(Vec::new())
            } else {
                git_index_bytes_for_seal(root, path)
            }
        },
    )
}

fn git_index_bytes_for_seal(root: &Path, relative: &str) -> TaskResult<Vec<u8>> {
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

fn committed_changes(root: &Path) -> TaskResult<Vec<ChangedPath>> {
    changes(
        root,
        &[
            "diff-tree",
            "--no-commit-id",
            "--name-status",
            "-r",
            "--diff-filter=AMD",
            "HEAD^",
            "HEAD",
        ],
        |path, status| {
            if status == 'D' {
                Ok(Vec::new())
            } else {
                git_file_bytes(root, "HEAD", path)
            }
        },
    )
}

fn changes(
    root: &Path,
    arguments: &[&str],
    bytes: impl Fn(&str, char) -> TaskResult<Vec<u8>>,
) -> TaskResult<Vec<ChangedPath>> {
    let listing = command_text(root, "git", arguments)?;
    let mut result = Vec::new();
    for line in listing.lines().filter(|line| !line.is_empty()) {
        let (raw_status, path) = line
            .split_once('\t')
            .ok_or_else(|| format!("invalid Git name-status row {line:?}"))?;
        let status = raw_status
            .chars()
            .next()
            .ok_or_else(|| format!("missing Git status for {path}"))?;
        require(
            matches!(status, 'A' | 'M' | 'D') && raw_status.len() == 1,
            &format!("unsupported work-package Git status {raw_status}"),
        )?;
        result.push(ChangedPath {
            status,
            path: path.to_string(),
            bytes: bytes(path, status)?,
        });
    }
    result.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(result)
}

fn digest(id: &str, target_version: &str, changes: &[ChangedPath]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(DOMAIN);
    digest_field(&mut hasher, id.as_bytes());
    digest_field(&mut hasher, target_version.as_bytes());
    hasher.update(
        u64::try_from(changes.len())
            .unwrap_or(u64::MAX)
            .to_be_bytes(),
    );
    for change in changes {
        hasher.update([change.status as u8]);
        digest_field(&mut hasher, change.path.as_bytes());
        digest_field(&mut hasher, &change.bytes);
    }
    format!("{:x}", hasher.finalize())
}

fn digest_field(hasher: &mut Sha256, value: &[u8]) {
    hasher.update(u64::try_from(value.len()).unwrap_or(u64::MAX).to_be_bytes());
    hasher.update(value);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digest_is_ordered_status_and_content_sensitive() {
        let one = vec![ChangedPath {
            status: 'A',
            path: "a".into(),
            bytes: b"one".to_vec(),
        }];
        let mut two = one.clone();
        two[0].bytes = b"two".to_vec();
        assert_ne!(
            digest("SEC-501", "0.5.0", &one),
            digest("SEC-501", "0.5.0", &two)
        );
        two[0].bytes = b"one".to_vec();
        two[0].status = 'D';
        assert_ne!(
            digest("SEC-501", "0.5.0", &one),
            digest("SEC-501", "0.5.0", &two)
        );
    }

    #[test]
    fn allowlist_rejects_parent_paths_and_duplicates() {
        assert!(validate_allowlist(&["../outside".into()]).is_err());
        assert!(validate_allowlist(&["one".into(), "one".into()]).is_err());
    }
}
