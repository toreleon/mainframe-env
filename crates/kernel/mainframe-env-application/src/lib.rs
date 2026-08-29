//! Generic content-addressed mainframe application package installation.

#![forbid(unsafe_code)]

use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

pub const APPLICATION_PACKAGE_CONTRACT: &str = "mainframe-env.application-package@1";

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum EntryKind {
    Source,
    Resource,
    Program,
    Data,
    Profile,
    Migration,
}

impl EntryKind {
    const fn slug(self) -> &'static str {
        match self {
            Self::Source => "source",
            Self::Resource => "resource",
            Self::Program => "program",
            Self::Data => "data",
            Self::Profile => "profile",
            Self::Migration => "migration",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackageEntry {
    pub path: String,
    pub kind: EntryKind,
    pub sha256: String,
    pub bytes: usize,
    pub depends_on: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApplicationManifest {
    pub name: String,
    pub version: String,
    pub target_product: String,
    pub entries: Vec<PackageEntry>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApplicationPackage {
    pub manifest: ApplicationManifest,
    pub blobs: BTreeMap<String, Vec<u8>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InstallState {
    Staged,
    Ready,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InstallRecord {
    pub package: String,
    pub version: String,
    pub identity: String,
    pub state: InstallState,
    pub entries: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InstallProblem {
    InvalidIdentity,
    InvalidPath,
    DuplicateEntry,
    MissingKind,
    MissingBlob,
    ContentMismatch,
    OrphanDependency,
    IncompatibleProduct,
    IdentityConflict,
    UnknownStage,
    Poisoned,
}

#[derive(Clone)]
pub struct ApplicationInstaller {
    product: String,
    records: Arc<Mutex<BTreeMap<String, InstallRecord>>>,
}

impl ApplicationInstaller {
    #[must_use]
    pub fn new(product: impl Into<String>) -> Self {
        Self {
            product: product.into(),
            records: Arc::new(Mutex::new(BTreeMap::new())),
        }
    }

    pub fn stage(&self, package: &ApplicationPackage) -> Result<InstallRecord, InstallProblem> {
        validate_package(package, &self.product)?;
        let identity = package_identity(&package.manifest)?;
        let key = package.manifest.name.to_ascii_uppercase();
        let mut records = self.records.lock().map_err(|_| InstallProblem::Poisoned)?;
        if let Some(existing) = records.get(&key) {
            if existing.identity != identity {
                return Err(InstallProblem::IdentityConflict);
            }
            return Ok(existing.clone());
        }
        let record = InstallRecord {
            package: package.manifest.name.clone(),
            version: package.manifest.version.clone(),
            identity,
            state: InstallState::Staged,
            entries: package.manifest.entries.len(),
        };
        records.insert(key, record.clone());
        Ok(record)
    }

    pub fn commit(&self, package: &ApplicationPackage) -> Result<InstallRecord, InstallProblem> {
        validate_package(package, &self.product)?;
        let identity = package_identity(&package.manifest)?;
        let key = package.manifest.name.to_ascii_uppercase();
        let mut records = self.records.lock().map_err(|_| InstallProblem::Poisoned)?;
        let existing = records.get_mut(&key).ok_or(InstallProblem::UnknownStage)?;
        if existing.identity != identity {
            return Err(InstallProblem::IdentityConflict);
        }
        existing.state = InstallState::Ready;
        Ok(existing.clone())
    }

    pub fn install(&self, package: &ApplicationPackage) -> Result<InstallRecord, InstallProblem> {
        let staged = self.stage(package)?;
        if staged.state == InstallState::Ready {
            return Ok(staged);
        }
        self.commit(package)
    }

    pub fn record(&self, package: &str) -> Result<Option<InstallRecord>, InstallProblem> {
        Ok(self
            .records
            .lock()
            .map_err(|_| InstallProblem::Poisoned)?
            .get(&package.to_ascii_uppercase())
            .cloned())
    }
}

pub fn package_identity(manifest: &ApplicationManifest) -> Result<String, InstallProblem> {
    validate_text(&manifest.name)?;
    validate_text(&manifest.version)?;
    validate_text(&manifest.target_product)?;
    let mut digest = Sha256::new();
    digest_field(&mut digest, APPLICATION_PACKAGE_CONTRACT.as_bytes());
    digest_field(&mut digest, manifest.name.as_bytes());
    digest_field(&mut digest, manifest.version.as_bytes());
    digest_field(&mut digest, manifest.target_product.as_bytes());
    for entry in &manifest.entries {
        digest_field(&mut digest, entry.path.as_bytes());
        digest_field(&mut digest, entry.kind.slug().as_bytes());
        digest_field(&mut digest, entry.sha256.as_bytes());
        digest_field(&mut digest, &(entry.bytes as u64).to_be_bytes());
        for dependency in &entry.depends_on {
            digest_field(&mut digest, dependency.as_bytes());
        }
    }
    Ok(format!("sha256:{:x}", digest.finalize()))
}

fn validate_package(package: &ApplicationPackage, product: &str) -> Result<(), InstallProblem> {
    if package.manifest.target_product != product {
        return Err(InstallProblem::IncompatibleProduct);
    }
    let mut paths = BTreeSet::new();
    let mut kinds = BTreeSet::new();
    for entry in &package.manifest.entries {
        validate_path(&entry.path)?;
        validate_sha256(&entry.sha256)?;
        if !paths.insert(entry.path.clone()) {
            return Err(InstallProblem::DuplicateEntry);
        }
        kinds.insert(entry.kind);
        let blob = package
            .blobs
            .get(&entry.sha256)
            .ok_or(InstallProblem::MissingBlob)?;
        if blob.len() != entry.bytes || sha256(blob) != entry.sha256 {
            return Err(InstallProblem::ContentMismatch);
        }
    }
    let required = BTreeSet::from([
        EntryKind::Source,
        EntryKind::Resource,
        EntryKind::Program,
        EntryKind::Data,
        EntryKind::Profile,
        EntryKind::Migration,
    ]);
    if !required.is_subset(&kinds) {
        return Err(InstallProblem::MissingKind);
    }
    for entry in &package.manifest.entries {
        if entry
            .depends_on
            .iter()
            .any(|dependency| !paths.contains(dependency))
        {
            return Err(InstallProblem::OrphanDependency);
        }
    }
    Ok(())
}

fn validate_text(value: &str) -> Result<(), InstallProblem> {
    if value.is_empty() || value.len() > 256 || value.chars().any(char::is_control) {
        Err(InstallProblem::InvalidIdentity)
    } else {
        Ok(())
    }
}

fn validate_path(path: &str) -> Result<(), InstallProblem> {
    if path.is_empty()
        || path.starts_with('/')
        || path
            .split('/')
            .any(|part| part.is_empty() || matches!(part, "." | ".."))
    {
        Err(InstallProblem::InvalidPath)
    } else {
        Ok(())
    }
}

fn validate_sha256(value: &str) -> Result<(), InstallProblem> {
    if value.len() != 71
        || !value.starts_with("sha256:")
        || !value[7..].bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        Err(InstallProblem::InvalidIdentity)
    } else {
        Ok(())
    }
}

fn sha256(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

fn digest_field(digest: &mut Sha256, bytes: &[u8]) {
    digest.update((bytes.len() as u64).to_be_bytes());
    digest.update(bytes);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn package() -> ApplicationPackage {
        let kinds = [
            EntryKind::Source,
            EntryKind::Resource,
            EntryKind::Program,
            EntryKind::Data,
            EntryKind::Profile,
            EntryKind::Migration,
        ];
        let mut entries = Vec::new();
        let mut blobs = BTreeMap::new();
        for (index, kind) in kinds.into_iter().enumerate() {
            let bytes = format!("payload-{index}").into_bytes();
            let identity = sha256(&bytes);
            blobs.insert(identity.clone(), bytes.clone());
            entries.push(PackageEntry {
                path: format!("{}/{index}", kind.slug()),
                kind,
                sha256: identity,
                bytes: bytes.len(),
                depends_on: (index > 0)
                    .then(|| "source/0".to_string())
                    .into_iter()
                    .collect(),
            });
        }
        ApplicationPackage {
            manifest: ApplicationManifest {
                name: "DEMO".into(),
                version: "1".into(),
                target_product: "0.1.1".into(),
                entries,
            },
            blobs,
        }
    }

    #[test]
    fn stage_is_not_ready_commit_is_atomic_and_reinstall_is_idempotent() {
        let installer = ApplicationInstaller::new("0.1.1");
        let package = package();
        let staged = installer.stage(&package).unwrap();
        assert_eq!(staged.state, InstallState::Staged);
        let ready = installer.commit(&package).unwrap();
        assert_eq!(ready.state, InstallState::Ready);
        assert_eq!(installer.install(&package).unwrap(), ready);
    }

    #[test]
    fn invalid_partial_or_conflicting_packages_never_become_ready() {
        let installer = ApplicationInstaller::new("0.1.1");
        let mut partial = package();
        partial.manifest.entries.pop();
        assert_eq!(
            installer.install(&partial),
            Err(InstallProblem::MissingKind)
        );
        assert!(installer.record("DEMO").unwrap().is_none());

        let package = package();
        installer.install(&package).unwrap();
        let mut conflict = package.clone();
        conflict.manifest.version = "2".into();
        assert_eq!(
            installer.install(&conflict),
            Err(InstallProblem::IdentityConflict)
        );
    }
}
