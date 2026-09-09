use super::{binary, binary_back, decode, encode, hex, number, optional_string, string};
use crate::validation;
use mainframe_env_execution_api::ArtifactRef;
use mainframe_env_store_api::{ArtifactRecord, StoreError};
use serde_json::json;

pub(super) fn encode_artifact(record: &ArtifactRecord) -> Result<Vec<u8>, StoreError> {
    let executable = record
        .executable
        .as_ref()
        .map(validation::encode_executable_metadata)
        .transpose()?
        .map(|bytes| binary(&bytes));
    encode(
        json!({"schema":2,"media":record.media_type,"digest":hex(&record.payload_digest),"payload":binary(&record.payload),"executable":executable}),
    )
}

pub(super) fn decode_artifact(
    id: &ArtifactRef,
    bytes: &[u8],
) -> Result<ArtifactRecord, StoreError> {
    let value = decode(bytes)?;
    let schema = number(&value, "schema")?;
    if !matches!(schema, 1 | 2) {
        return Err(StoreError::IncompatibleVersion);
    }
    let executable = if schema == 1 {
        None
    } else {
        optional_string(&value, "executable")?
            .map(|encoded| binary_back(&encoded))
            .transpose()?
            .map(|bytes| validation::decode_executable_metadata(&bytes))
            .transpose()?
    };
    let record = ArtifactRecord {
        artifact: id.clone(),
        media_type: string(&value, "media")?.into(),
        payload_digest: super::digest_back(string(&value, "digest")?)?,
        payload: binary_back(string(&value, "payload")?)?,
        executable,
    };
    validation::artifact(&record)?;
    Ok(record)
}
