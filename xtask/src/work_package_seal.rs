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
        &[
            "diff",
            "--cached",
            "--name-status",
            "--no-renames",
            "--diff-filter=AMD",
        ],
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
            "--no-renames",
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

    #[test]
    fn staged_and_committed_renames_are_sealed_as_delete_add_pairs() {
        let root = std::env::temp_dir().join(format!(
            "mainframe-env-work-package-renames-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        git(&root, &["init", "--quiet"]);
        git(&root, &["config", "user.email", "tests@mainframe.invalid"]);
        git(&root, &["config", "user.name", "mainframe-env tests"]);
        for (path, bytes) in [
            ("pure old name.txt", b"same bytes".as_slice()),
            ("edited old name.txt", b"before edit".as_slice()),
        ] {
            std::fs::write(root.join(path), bytes).unwrap();
        }
        git(&root, &["add", "."]);
        git(&root, &["commit", "--quiet", "-m", "baseline"]);

        std::fs::rename(
            root.join("pure old name.txt"),
            root.join("pure new name.txt"),
        )
        .unwrap();
        std::fs::rename(
            root.join("edited old name.txt"),
            root.join("edited new name.txt"),
        )
        .unwrap();
        std::fs::write(root.join("edited new name.txt"), b"after edit").unwrap();
        git(&root, &["add", "-A"]);

        let staged = staged_changes(&root).unwrap();
        assert_eq!(
            staged
                .iter()
                .map(|change| (change.status, change.path.as_str()))
                .collect::<Vec<_>>(),
            vec![
                ('A', "edited new name.txt"),
                ('D', "edited old name.txt"),
                ('A', "pure new name.txt"),
                ('D', "pure old name.txt"),
            ]
        );
        assert_eq!(
            staged
                .iter()
                .find(|change| change.path == "edited new name.txt")
                .unwrap()
                .bytes,
            b"after edit"
        );
        assert!(
            staged
                .iter()
                .filter(|change| change.status == 'D')
                .all(|change| change.bytes.is_empty())
        );

        git(&root, &["commit", "--quiet", "-m", "rename"]);
        let committed = committed_changes(&root).unwrap();
        assert_eq!(
            committed
                .iter()
                .map(|change| (change.status, change.path.as_str()))
                .collect::<Vec<_>>(),
            staged
                .iter()
                .map(|change| (change.status, change.path.as_str()))
                .collect::<Vec<_>>()
        );
        let _ = std::fs::remove_dir_all(root);
    }

    fn git(root: &Path, arguments: &[&str]) {
        let output = Command::new("git")
            .args(arguments)
            .current_dir(root)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {:?}: {}",
            arguments,
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
