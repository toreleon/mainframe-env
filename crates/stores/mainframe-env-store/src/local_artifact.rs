use crate::validation;
use mainframe_env_execution_api::ArtifactRef;
use mainframe_env_store_api::{ArtifactRecord, ArtifactStore, ArtifactStoreHealth, StoreError};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static TEMPORARY_SEQUENCE: AtomicU64 = AtomicU64::new(1);

pub struct LocalArtifactStore {
    root: PathBuf,
    max_artifact_bytes: usize,
}

impl LocalArtifactStore {
    pub fn open(root: impl Into<PathBuf>, max_artifact_bytes: usize) -> Result<Self, StoreError> {
        let root = root.into();
        if root.as_os_str().is_empty() || max_artifact_bytes == 0 {
            return Err(StoreError::CapacityExceeded);
        }
        let objects = root.join("objects");
        std::fs::create_dir_all(&objects)
            .map_err(|error| StoreError::Infrastructure(error.to_string()))?;
        sync_directory(&objects)?;
        Ok(Self {
            root,
            max_artifact_bytes,
        })
    }

    fn path(&self, artifact: &ArtifactRef) -> Result<PathBuf, StoreError> {
        let digest = artifact
            .as_str()
            .strip_prefix("sha256:")
            .ok_or(StoreError::IncompatibleVersion)?;
        if digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(StoreError::IncompatibleVersion);
        }
        Ok(self.root.join("objects").join(&digest[..2]).join(digest))
    }

    #[must_use]
    pub fn is_ready(&self) -> bool {
        self.health().is_ok_and(ArtifactStoreHealth::ready)
    }
}

impl ArtifactStore for LocalArtifactStore {
    fn health(&self) -> Result<ArtifactStoreHealth, StoreError> {
        let objects = self.root.join("objects");
        std::fs::read_dir(&objects)
            .map_err(|error| StoreError::Infrastructure(error.to_string()))?;
        let destination = objects.join("readiness-probe");
        let (temporary, mut file) = temporary_file(&objects, &destination)?;
        if let Err(error) = file.write_all(b"ready").and_then(|()| file.sync_all()) {
            let _ = std::fs::remove_file(&temporary);
            return Err(StoreError::Infrastructure(error.to_string()));
        }
        drop(file);
        std::fs::remove_file(&temporary)
            .map_err(|error| StoreError::Infrastructure(error.to_string()))?;
        sync_directory(&objects)?;
        Ok(ArtifactStoreHealth {
            readable: true,
            writable: true,
            used_objects: None,
            max_objects: None,
            used_bytes: None,
            max_bytes: None,
        })
    }

    fn put_artifact(&self, record: ArtifactRecord) -> Result<(), StoreError> {
        if record.payload.len() > self.max_artifact_bytes
            || record.media_type.is_empty()
            || record.media_type.len() > 4_096
        {
            return Err(StoreError::PayloadTooLarge);
        }
        validation::artifact(&record)?;
        let path = self.path(&record.artifact)?;
        let encoded = encode(&record)?;
        if path.exists() {
            return if std::fs::read(&path)
                .map_err(|error| StoreError::Infrastructure(error.to_string()))?
                == encoded
            {
                Ok(())
            } else {
                Err(StoreError::Conflict)
            };
        }
        let parent = path.parent().ok_or(StoreError::IncompatibleVersion)?;
        let parent_existed = parent.is_dir();
        std::fs::create_dir_all(parent)
            .map_err(|error| StoreError::Infrastructure(error.to_string()))?;
        if !parent_existed {
            sync_directory(parent.parent().ok_or(StoreError::IncompatibleVersion)?)?;
        }
        let (temporary, mut file) = temporary_file(parent, &path)?;
        if let Err(error) = file.write_all(&encoded).and_then(|()| file.sync_all()) {
            let _ = std::fs::remove_file(&temporary);
            return Err(StoreError::Infrastructure(error.to_string()));
        }
        drop(file);
        match std::fs::hard_link(&temporary, &path) {
            Ok(()) => {
                sync_directory(parent)?;
                std::fs::remove_file(&temporary)
                    .map_err(|error| StoreError::Infrastructure(error.to_string()))?;
                sync_directory(parent)
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                let existing = std::fs::read(&path)
                    .map_err(|error| StoreError::Infrastructure(error.to_string()));
                let _ = std::fs::remove_file(&temporary);
                sync_directory(parent)?;
                if existing? == encoded {
                    Ok(())
                } else {
                    Err(StoreError::Conflict)
                }
            }
            Err(error) => {
                let _ = std::fs::remove_file(&temporary);
                let _ = sync_directory(parent);
                Err(StoreError::Infrastructure(error.to_string()))
            }
        }
    }

    fn get_artifact(&self, id: &ArtifactRef) -> Result<Option<ArtifactRecord>, StoreError> {
        let path = self.path(id)?;
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(StoreError::Infrastructure(error.to_string())),
        };
        if bytes.len() > self.max_artifact_bytes.saturating_add(256 * 1024 + 4096) {
            return Err(StoreError::PayloadTooLarge);
        }
        decode(id, &bytes, self.max_artifact_bytes).map(Some)
    }

    fn delete_artifact(&self, id: &ArtifactRef) -> Result<(), StoreError> {
        let path = self.path(id)?;
        match std::fs::remove_file(&path) {
            Ok(()) => sync_directory(path.parent().ok_or(StoreError::IncompatibleVersion)?),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Err(StoreError::NotFound),
            Err(error) => Err(StoreError::Infrastructure(error.to_string())),
        }
    }
}

fn temporary_file(
    parent: &Path,
    destination: &Path,
) -> Result<(PathBuf, std::fs::File), StoreError> {
    let name = destination
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or(StoreError::IncompatibleVersion)?;
    for _ in 0..1_024 {
        let sequence = TEMPORARY_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = parent.join(format!(".{name}.{}.{}.tmp", std::process::id(), sequence));
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => return Ok((path, file)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(StoreError::Infrastructure(error.to_string())),
        }
    }
    Err(StoreError::Infrastructure(
        "artifact temporary-name space is exhausted".into(),
    ))
}

fn sync_directory(path: &Path) -> Result<(), StoreError> {
    std::fs::File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| StoreError::Infrastructure(error.to_string()))
}

fn encode(record: &ArtifactRecord) -> Result<Vec<u8>, StoreError> {
    let metadata = record
        .executable
        .as_ref()
        .map(validation::encode_executable_metadata)
        .transpose()?
        .unwrap_or_default();
    let mut output = b"MEAR2".to_vec();
    output.extend_from_slice(
        &u32::try_from(record.media_type.len())
            .map_err(|_| StoreError::PayloadTooLarge)?
            .to_be_bytes(),
    );
    output.extend_from_slice(
        &u32::try_from(metadata.len())
            .map_err(|_| StoreError::PayloadTooLarge)?
            .to_be_bytes(),
    );
    output.extend_from_slice(record.media_type.as_bytes());
    output.extend_from_slice(&record.payload_digest);
    output.extend_from_slice(&metadata);
    output.extend_from_slice(&record.payload);
    Ok(output)
}

fn decode(artifact: &ArtifactRef, bytes: &[u8], max: usize) -> Result<ArtifactRecord, StoreError> {
    let legacy = bytes.get(..5) == Some(b"MEAR1");
    if !legacy && bytes.get(..5) != Some(b"MEAR2") {
        return Err(StoreError::IncompatibleVersion);
    }
    let header = if legacy { 9 } else { 13 };
    if bytes.len() < header + 32 {
        return Err(StoreError::IncompatibleVersion);
    }
    let media_length = usize::try_from(u32::from_be_bytes(
        bytes[5..9]
            .try_into()
            .map_err(|_| StoreError::IncompatibleVersion)?,
    ))
    .map_err(|_| StoreError::IncompatibleVersion)?;
    let metadata_length = if legacy {
        0
    } else {
        usize::try_from(u32::from_be_bytes(
            bytes[9..13]
                .try_into()
                .map_err(|_| StoreError::IncompatibleVersion)?,
        ))
        .map_err(|_| StoreError::IncompatibleVersion)?
    };
    let digest_start = header
        .checked_add(media_length)
        .ok_or(StoreError::PayloadTooLarge)?;
    let metadata_start = digest_start
        .checked_add(32)
        .ok_or(StoreError::PayloadTooLarge)?;
    let payload_start = metadata_start
        .checked_add(metadata_length)
        .ok_or(StoreError::PayloadTooLarge)?;
    let media_type = String::from_utf8(
        bytes
            .get(header..digest_start)
            .ok_or(StoreError::IncompatibleVersion)?
            .to_vec(),
    )
    .map_err(|_| StoreError::IncompatibleVersion)?;
    if media_type.is_empty() || media_type.len() > 4_096 {
        return Err(StoreError::IncompatibleVersion);
    }
    let payload_digest: [u8; 32] = bytes
        .get(digest_start..metadata_start)
        .ok_or(StoreError::IncompatibleVersion)?
        .try_into()
        .map_err(|_| StoreError::IncompatibleVersion)?;
    let executable = if metadata_length == 0 {
        None
    } else {
        Some(validation::decode_executable_metadata(
            bytes
                .get(metadata_start..payload_start)
                .ok_or(StoreError::IncompatibleVersion)?,
        )?)
    };
    let payload = bytes
        .get(payload_start..)
        .ok_or(StoreError::IncompatibleVersion)?
        .to_vec();
    if payload.len() > max {
        return Err(StoreError::PayloadTooLarge);
    }
    let record = ArtifactRecord {
        artifact: artifact.clone(),
        media_type,
        payload_digest,
        payload,
        executable,
    };
    validation::artifact(&record)?;
    Ok(record)
}

#[allow(dead_code)]
fn provider_private_path(_: &Path) {}

#[cfg(test)]
mod tests {
    use super::*;
    use mainframe_env_execution_api::InvocationLimits;
    use mainframe_env_store_api::ExecutableArtifactMetadata;
    use sha2::{Digest, Sha256};
    use std::collections::{BTreeMap, BTreeSet};
    use std::sync::Arc;

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    fn record(payload: &[u8], media_type: &str) -> ArtifactRecord {
        let payload = payload.to_vec();
        let digest: [u8; 32] = Sha256::digest(&payload).into();
        ArtifactRecord {
            artifact: ArtifactRef::new(
                format!("sha256:{}", hex(&digest)),
                InvocationLimits::default(),
            )
            .unwrap(),
            media_type: media_type.into(),
            payload_digest: digest,
            payload,
            executable: None,
        }
    }

    fn directory(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "mainframe-env-artifact-{label}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ))
    }

    #[test]
    fn immutable_artifact_roundtrips_and_detects_corruption() {
        let directory = directory("roundtrip");
        let store = LocalArtifactStore::open(&directory, 1024).unwrap();
        let record = record(b"artifact", "application/octet-stream");
        let artifact = record.artifact.clone();
        store.put_artifact(record.clone()).unwrap();
        store.put_artifact(record.clone()).unwrap();
        assert_eq!(store.get_artifact(&artifact).unwrap(), Some(record.clone()));
        let path = store.path(&artifact).unwrap();
        std::fs::write(&path, b"corrupt").unwrap();
        assert_eq!(
            store.get_artifact(&artifact),
            Err(StoreError::IncompatibleVersion)
        );
        assert_eq!(store.put_artifact(record), Err(StoreError::Conflict));
        assert_eq!(
            store.get_artifact(&artifact),
            Err(StoreError::IncompatibleVersion)
        );
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn concurrent_publish_is_atomic_no_replace_across_store_instances() {
        let directory = directory("concurrent");
        let left = LocalArtifactStore::open(&directory, 1024).unwrap();
        let right = LocalArtifactStore::open(&directory, 1024).unwrap();
        let left_record = record(b"same bytes", "application/x-left");
        let right_record = record(b"same bytes", "application/x-right");
        let artifact = left_record.artifact.clone();
        let barrier = Arc::new(std::sync::Barrier::new(3));
        let left_barrier = barrier.clone();
        let left_worker = std::thread::spawn(move || {
            left_barrier.wait();
            left.put_artifact(left_record)
        });
        let right_barrier = barrier.clone();
        let right_worker = std::thread::spawn(move || {
            right_barrier.wait();
            right.put_artifact(right_record)
        });
        barrier.wait();
        let results = [left_worker.join().unwrap(), right_worker.join().unwrap()];
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        assert_eq!(
            results
                .iter()
                .filter(|result| **result == Err(StoreError::Conflict))
                .count(),
            1
        );
        let reopened = LocalArtifactStore::open(&directory, 1024).unwrap();
        let stored = reopened.get_artifact(&artifact).unwrap().unwrap();
        assert!(matches!(
            stored.media_type.as_str(),
            "application/x-left" | "application/x-right"
        ));
        let parent = reopened
            .path(&artifact)
            .unwrap()
            .parent()
            .unwrap()
            .to_path_buf();
        assert!(std::fs::read_dir(parent).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with(".tmp")
        }));
        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn executable_manifest_metadata_survives_reopen_exactly() {
        let directory = directory("executable-metadata");
        let store = LocalArtifactStore::open(&directory, 1024).unwrap();
        let mut record = record(
            b"canonical artifact",
            "application/vnd.mainframe-env.core-mir",
        );
        record.executable = Some(
            ExecutableArtifactMetadata {
                artifact_contract: "mainframe-env.artifact@2".into(),
                compatibility_profile: "mainframe-env.cobol.reference@1".into(),
                compiler_generation: "mainframe-env-cobol-0.8.3".into(),
                target: "reference".into(),
                options: BTreeMap::from([("cobol.flag".into(), "on".into())]),
                host_interfaces: BTreeSet::from(["mainframe-env.host@1".into()]),
                ir_contract: "mainframe-env.ir-envelope@1".into(),
                dialect_contracts: None,
                semantic_identity: format!("semantic-sha256:{:064x}", 1),
                manifest_payload_digest: [0; 32],
            }
            .bind_to_payload(&record.payload_digest),
        );
        store.put_artifact(record.clone()).unwrap();
        drop(store);
        let reopened = LocalArtifactStore::open(&directory, 1024).unwrap();
        assert_eq!(
            reopened.get_artifact(&record.artifact).unwrap(),
            Some(record.clone())
        );
        let mut tampered = record;
        tampered.executable.as_mut().unwrap().semantic_identity =
            format!("semantic-sha256:{:064x}", 2);
        assert_eq!(
            reopened.put_artifact(tampered),
            Err(StoreError::IncompatibleVersion)
        );
        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn stale_crash_temporary_cannot_replace_a_published_object() {
        let directory = directory("crash");
        let store = LocalArtifactStore::open(&directory, 1024).unwrap();
        let record = record(b"durable", "application/octet-stream");
        let path = store.path(&record.artifact).unwrap();
        let parent = path.parent().unwrap();
        std::fs::create_dir_all(parent).unwrap();
        std::fs::write(parent.join(".stale-crash.tmp"), b"partial").unwrap();

        store.put_artifact(record.clone()).unwrap();
        drop(store);
        let reopened = LocalArtifactStore::open(&directory, 1024).unwrap();
        assert_eq!(
            reopened.get_artifact(&record.artifact).unwrap(),
            Some(record)
        );
        let _ = std::fs::remove_dir_all(directory);
    }
}
