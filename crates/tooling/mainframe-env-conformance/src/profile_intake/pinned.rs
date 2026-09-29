//! Immutable Git-blob reads for external corpus source and licence files.

use super::IntakeError;
use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

pub(super) struct PinnedFiles<'a> {
    root: &'a Path,
    blobs: BTreeMap<String, String>,
}

impl<'a> PinnedFiles<'a> {
    pub(super) fn open(root: &'a Path, commit: &str) -> Result<Self, IntakeError> {
        if commit.len() != 40 || !commit.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(IntakeError::Pin("invalid commit identity".into()));
        }
        let output = git_bytes(root, &["ls-tree", "-r", "-z", "--full-tree", commit])?;
        let mut blobs = BTreeMap::new();
        for entry in output.split(|byte| *byte == 0).filter(|entry| !entry.is_empty()) {
            let entry = std::str::from_utf8(entry)
                .map_err(|_| IntakeError::Pin("non-UTF-8 tracked path".into()))?;
            let (header, path) = entry
                .split_once('\t')
                .ok_or_else(|| IntakeError::Pin("invalid Git tree entry".into()))?;
            let fields = header.split_ascii_whitespace().collect::<Vec<_>>();
            if fields.len() != 3
                || !matches!(fields[0], "100644" | "100755")
                || fields[1] != "blob"
            {
                return Err(IntakeError::Pin(format!(
                    "non-regular tracked entry is unsupported: {path}"
                )));
            }
            let oid = fields[2];
            if oid.len() != 40
                || !oid.bytes().all(|byte| byte.is_ascii_hexdigit())
                || path.is_empty()
                || path.starts_with('/')
                || path.split('/').any(|part| matches!(part, "" | "." | ".."))
                || blobs.insert(path.to_string(), oid.to_string()).is_some()
            {
                return Err(IntakeError::Pin("invalid pinned file identity".into()));
            }
        }
        Ok(Self { root, blobs })
    }

    pub(super) fn paths(&self) -> Vec<String> {
        self.blobs.keys().cloned().collect()
    }

    pub(super) fn read(&self, path: &str) -> Result<Vec<u8>, IntakeError> {
        let oid = self
            .blobs
            .get(path)
            .ok_or_else(|| IntakeError::Pin(format!("file absent from pinned tree: {path}")))?;
        // Read the blob, not a working-tree path: no symlinks, checkout filters,
        // replacement objects, or later working-tree/index edits can change it.
        git_bytes(self.root, &["cat-file", "blob", oid])
    }
}

fn git_bytes(root: &Path, args: &[&str]) -> Result<Vec<u8>, IntakeError> {
    let output = Command::new("git")
        .arg("--no-replace-objects")
        .args(args)
        .current_dir(root)
        .output()
        .map_err(|error| IntakeError::Pin(error.to_string()))?;
    if !output.status.success() {
        return Err(IntakeError::Pin(
            String::from_utf8_lossy(&output.stderr).trim().into(),
        ));
    }
    Ok(output.stdout)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(0);

    struct Fixture(PathBuf);

    impl Fixture {
        fn new() -> Self {
            let tick = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let root = std::env::temp_dir().join(format!(
                "mainframe-env-profile-pin-{}-{tick}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&root).unwrap();
            let fixture = Self(root);
            fixture.git(&["init", "-q"]);
            fixture.git(&["config", "user.name", "Profile intake test"]);
            fixture.git(&["config", "user.email", "profile-intake@example.invalid"]);
            fixture
        }

        fn git(&self, args: &[&str]) -> String {
            let output = Command::new("git")
                .args(args)
                .current_dir(&self.0)
                .output()
                .unwrap();
            assert!(output.status.success(), "{:?}", output);
            String::from_utf8(output.stdout).unwrap().trim().to_string()
        }

        fn commit(&self) -> String {
            self.git(&["add", "-A"]);
            self.git(&["commit", "-qm", "fixture"]);
            self.git(&["rev-parse", "HEAD"])
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn pinned_files_read_exact_blobs_not_worktree_or_index() {
        let fixture = Fixture::new();
        let original = b"  source\n\0\xff\n";
        fs::write(fixture.0.join("source.cbl"), original).unwrap();
        fs::write(fixture.0.join("LICENSE"), b"licence\n").unwrap();
        let commit = fixture.commit();
        let pinned = PinnedFiles::open(&fixture.0, &commit).unwrap();
        fs::write(fixture.0.join("source.cbl"), b"changed").unwrap();
        fs::write(fixture.0.join("new.cpy"), b"not pinned").unwrap();
        fixture.git(&["add", "-A"]);
        assert_eq!(pinned.read("source.cbl").unwrap(), original);
        assert_eq!(pinned.read("LICENSE").unwrap(), b"licence\n");
        assert_eq!(pinned.paths(), vec!["LICENSE", "source.cbl"]);
        assert!(pinned.read("new.cpy").is_err());
        assert!(pinned.read("../outside").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn pinned_files_reject_source_copybook_and_license_symlinks() {
        use std::os::unix::fs::symlink;

        for name in ["source.cbl", "copy.cpy", "LICENSE"] {
            let fixture = Fixture::new();
            let outside = Fixture::new();
            let target = outside.0.join("external");
            fs::write(&target, b"before").unwrap();
            symlink(&target, fixture.0.join(name)).unwrap();
            let commit = fixture.commit();
            assert!(fixture.git(&["status", "--porcelain"]).is_empty());
            assert!(PinnedFiles::open(&fixture.0, &commit).is_err());
            fs::write(&target, b"after").unwrap();
            assert!(fixture.git(&["status", "--porcelain"]).is_empty());
            assert!(PinnedFiles::open(&fixture.0, &commit).is_err());
        }
    }

    #[test]
    fn pinned_files_ignore_local_blob_replacements() {
        let fixture = Fixture::new();
        fs::write(fixture.0.join("source.cbl"), b"original\n").unwrap();
        let commit = fixture.commit();
        let original = fixture.git(&["rev-parse", &format!("{commit}:source.cbl")]);
        fs::write(fixture.0.join("source.cbl"), b"replacement\n").unwrap();
        let replacement_commit = fixture.commit();
        let replacement =
            fixture.git(&["rev-parse", &format!("{replacement_commit}:source.cbl")]);
        fixture.git(&["replace", &original, &replacement]);
        let pinned = PinnedFiles::open(&fixture.0, &commit).unwrap();
        assert_eq!(pinned.read("source.cbl").unwrap(), b"original\n");
    }
}
