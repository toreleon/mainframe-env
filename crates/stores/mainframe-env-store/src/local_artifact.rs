use mainframe_env_execution_api::ArtifactRef;
use mainframe_env_store_api::{ArtifactRecord, ArtifactStore, StoreError};
use sha2::{Digest, Sha256};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

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
        std::fs::create_dir_all(root.join("objects"))
            .map_err(|error| StoreError::Infrastructure(error.to_string()))?;
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
        self.root.join("objects").is_dir()
    }
}

impl ArtifactStore for LocalArtifactStore {
    fn put_artifact(&self, record: ArtifactRecord) -> Result<(), StoreError> {
        if record.payload.len() > self.max_artifact_bytes || record.media_type.is_empty() {
            return Err(StoreError::PayloadTooLarge);
        }
        let digest: [u8; 32] = Sha256::digest(&record.payload).into();
        if record.payload_digest != digest
            || record.artifact.as_str() != format!("sha256:{}", hex(&digest))
        {
            return Err(StoreError::IncompatibleVersion);
        }
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
        std::fs::create_dir_all(parent)
            .map_err(|error| StoreError::Infrastructure(error.to_string()))?;
        let temporary = parent.join(format!(
            ".{}.{}.tmp",
            path.file_name()
                .and_then(|value| value.to_str())
                .ok_or(StoreError::IncompatibleVersion)?,
            std::process::id()
        ));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|error| StoreError::Infrastructure(error.to_string()))?;
        let write_result = file
            .write_all(&encoded)
            .and_then(|()| file.sync_all())
            .and_then(|()| std::fs::rename(&temporary, &path));
        if let Err(error) = write_result {
            let _ = std::fs::remove_file(temporary);
            return Err(StoreError::Infrastructure(error.to_string()));
        }
        Ok(())
    }

    fn get_artifact(&self, id: &ArtifactRef) -> Result<Option<ArtifactRecord>, StoreError> {
        let path = self.path(id)?;
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(StoreError::Infrastructure(error.to_string())),
        };
        if bytes.len() > self.max_artifact_bytes.saturating_add(4096) {
            return Err(StoreError::PayloadTooLarge);
        }
        decode(id, &bytes, self.max_artifact_bytes).map(Some)
    }
}

fn encode(record: &ArtifactRecord) -> Result<Vec<u8>, StoreError> {
    let mut output = b"MEAR1".to_vec();
    output.extend_from_slice(
        &u32::try_from(record.media_type.len())
            .map_err(|_| StoreError::PayloadTooLarge)?
            .to_be_bytes(),
    );
    output.extend_from_slice(record.media_type.as_bytes());
    output.extend_from_slice(&record.payload_digest);
    output.extend_from_slice(&record.payload);
    Ok(output)
}

fn decode(artifact: &ArtifactRef, bytes: &[u8], max: usize) -> Result<ArtifactRecord, StoreError> {
    if bytes.get(..5) != Some(b"MEAR1") || bytes.len() < 41 {
        return Err(StoreError::IncompatibleVersion);
    }
    let media_length = usize::try_from(u32::from_be_bytes(
        bytes[5..9]
            .try_into()
            .map_err(|_| StoreError::IncompatibleVersion)?,
    ))
    .map_err(|_| StoreError::IncompatibleVersion)?;
    let digest_start = 9usize
        .checked_add(media_length)
        .ok_or(StoreError::PayloadTooLarge)?;
    let payload_start = digest_start
        .checked_add(32)
        .ok_or(StoreError::PayloadTooLarge)?;
    let media_type = String::from_utf8(
        bytes
            .get(9..digest_start)
            .ok_or(StoreError::IncompatibleVersion)?
            .to_vec(),
    )
    .map_err(|_| StoreError::IncompatibleVersion)?;
    let payload_digest: [u8; 32] = bytes
        .get(digest_start..payload_start)
        .ok_or(StoreError::IncompatibleVersion)?
        .try_into()
        .map_err(|_| StoreError::IncompatibleVersion)?;
    let payload = bytes
        .get(payload_start..)
        .ok_or(StoreError::IncompatibleVersion)?
        .to_vec();
    if payload.len() > max
        || Sha256::digest(&payload).as_slice() != payload_digest
        || artifact.as_str() != format!("sha256:{}", hex(&payload_digest))
    {
        return Err(StoreError::IncompatibleVersion);
    }
    Ok(ArtifactRecord {
        artifact: artifact.clone(),
        media_type,
        payload_digest,
        payload,
    })
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[allow(dead_code)]
fn provider_private_path(_: &Path) {}

#[cfg(test)]
mod tests {
    use super::*;
    use mainframe_env_execution_api::InvocationLimits;

    #[test]
    fn immutable_artifact_roundtrips_and_detects_corruption() {
        let directory = std::env::temp_dir().join(format!(
            "mainframe-env-artifact-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let store = LocalArtifactStore::open(&directory, 1024).unwrap();
        let payload = b"artifact".to_vec();
        let digest: [u8; 32] = Sha256::digest(&payload).into();
        let artifact = ArtifactRef::new(
            format!("sha256:{}", hex(&digest)),
            InvocationLimits::default(),
        )
        .unwrap();
        let record = ArtifactRecord {
            artifact: artifact.clone(),
            media_type: "application/octet-stream".into(),
            payload_digest: digest,
            payload,
        };
        store.put_artifact(record.clone()).unwrap();
        store.put_artifact(record.clone()).unwrap();
        assert_eq!(store.get_artifact(&artifact).unwrap(), Some(record));
        let path = store.path(&artifact).unwrap();
        std::fs::write(&path, b"corrupt").unwrap();
        assert_eq!(
            store.get_artifact(&artifact),
            Err(StoreError::IncompatibleVersion)
        );
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_dir_all(directory);
    }
}
