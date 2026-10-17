//! Private held receipt lease shared by first-party source owners.
use super::*;
use std::io::{Seek, SeekFrom, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};

// Preserve the original diagnostic classification used by every existing caller.
fn refused(detail: &str) -> CorpusProblem {
    CorpusProblem::new("carddemo.db2.missing98_diagnostic", detail)
}

pub(super) fn require_owned_directory(
    path: &Path,
    expected: (u64, u64),
) -> Result<(), CorpusProblem> {
    let metadata =
        fs::symlink_metadata(path).map_err(|_| refused("owned directory identity unavailable"))?;
    if !metadata.is_dir()
        || metadata.file_type().is_symlink()
        || (metadata.dev(), metadata.ino()) != expected
        || metadata.permissions().mode() & 0o777 != 0o700
        || path
            .canonicalize()
            .map_err(|_| refused("owned directory canonical identity unavailable"))?
            != path
    {
        return Err(refused(
            "owned directory identity changed; preserve artifacts",
        ));
    }
    Ok(())
}

// Keep the originally admitted file descriptor. Pre/post path observations
// refuse replacements; writes never reopen a rebound receipt path. These fixed
// private-owner observations are not atomic filesystem/process containment.
pub(super) struct ReceiptOwner {
    pub(super) evidence: PathBuf,
    pub(super) evidence_identity: (u64, u64),
    pub(super) artifacts: PathBuf,
    pub(super) artifact_identity: (u64, u64),
    pub(super) file: fs::File,
    pub(super) file_identity: (u64, u64),
    pub(super) artifacts_removed: bool,
    pub(super) retention_failed: bool,
}

impl ReceiptOwner {
    pub(super) fn open(
        evidence: &Path,
        evidence_identity: (u64, u64),
        artifacts: &Path,
        artifact_identity: (u64, u64),
    ) -> Result<Self, CorpusProblem> {
        require_owned_directory(evidence, evidence_identity)?;
        require_owned_directory(artifacts, artifact_identity)?;
        // create_new refuses existing files/symlinks. No receipt bytes are
        // written until both roots and this exact private handle are rechecked.
        let file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(evidence.join("actual.json"))
            .map_err(|_| refused("exclusive receipt admission failed"))?;
        let metadata = file
            .metadata()
            .map_err(|_| refused("receipt handle identity unavailable"))?;
        let owner = Self {
            evidence: evidence.into(),
            evidence_identity,
            artifacts: artifacts.into(),
            artifact_identity,
            file,
            file_identity: (metadata.dev(), metadata.ino()),
            artifacts_removed: false,
            retention_failed: false,
        };
        owner.check()?;
        Ok(owner)
    }

    pub(super) fn check(&self) -> Result<(), CorpusProblem> {
        if self.retention_failed {
            return Err(refused("retention already refused; preserve artifacts"));
        }
        require_owned_directory(&self.evidence, self.evidence_identity)?;
        if self.artifacts_removed {
            match fs::symlink_metadata(&self.artifacts) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                _ => return Err(refused("checked artifact absence changed or unknown")),
            }
        } else {
            require_owned_directory(&self.artifacts, self.artifact_identity)?;
        }
        let path = fs::symlink_metadata(self.evidence.join("actual.json"))
            .map_err(|_| refused("receipt path identity unavailable"))?;
        let handle = self
            .file
            .metadata()
            .map_err(|_| refused("receipt handle identity unavailable"))?;
        if [&path, &handle].into_iter().any(|metadata| {
            !metadata.is_file()
                || metadata.file_type().is_symlink()
                || metadata.nlink() != 1
                || metadata.permissions().mode() & 0o777 != 0o600
                || (metadata.dev(), metadata.ino()) != self.file_identity
        }) {
            return Err(refused(
                "receipt path or handle identity changed; preserve artifacts",
            ));
        }
        Ok(())
    }

    pub(super) fn save(&mut self, receipt: &serde_json::Value) -> Result<(), CorpusProblem> {
        let saved = (|| {
            self.check()?;
            let bytes = serde_json::to_vec_pretty(receipt)
                .map_err(|_| refused("receipt encoding failed"))?;
            self.file
                .seek(SeekFrom::Start(0))
                .map_err(|_| refused("receipt seek failed"))?;
            self.file
                .write_all(&bytes)
                .map_err(|_| refused("receipt retention failed"))?;
            self.file
                .set_len(bytes.len() as u64)
                .map_err(|_| refused("receipt length retention failed"))?;
            self.file
                .sync_all()
                .map_err(|_| refused("receipt sync failed"))?;
            self.check()
        })();
        // A failed identity/write/retention stage cannot later authorize server
        // admission or artifact cleanup; retain the roots and any actual bytes.
        if saved.is_err() {
            self.retention_failed = true;
        }
        saved
    }
}
